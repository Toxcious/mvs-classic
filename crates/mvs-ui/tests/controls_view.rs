//! The menu controls, dialogs and toasts on the menu bundle: a dialog from its timer to its exit, a notification to
//! its time out, a context menu of the manager, the page pips. Needs the dev store; without one each test says so and
//! passes.

use std::sync::Arc;

use mvs_settings::Settings;
use mvs_ui::Bundle;
use mvs_ui::bindings::Bindings;
use mvs_ui::element::ElementList;
use mvs_ui::menu::controller::MenuRun;
use mvs_ui::menu::input::Pointer;
use mvs_ui::menu::manager::{ContextMenuButton, ContextMenuParams, DialogParams, Manager, NotificationParams};
use mvs_ui::menu::widgets::controls::pips::PaginationPips;
use mvs_ui::menu::{MenuScreen, MenuView, NoSound};
use mvs_ui::text::{Fonts, GameText};

struct Env {
    bundle: Arc<Bundle>,
    text: Arc<GameText>,
    fonts: Fonts,
    prefs: Settings,
    bindings: Bindings,
    sink: NoSound,
    pointer: Pointer,
}

fn env() -> Option<Env> {
    let store = match mvs_store::Store::open(mvs_store::dev_store_dir()) {
        Ok(s) if s.version("font").is_some() && s.version("ui").is_some() => s,
        _ => {
            eprintln!("no dev store with the font and ui kinds: skipped");
            return None;
        }
    };
    let bundle = match Bundle::load(&store, "menu") {
        Ok(b) => b,
        Err(e) => {
            eprintln!("no menu bundle ({e}): skipped");
            return None;
        }
    };
    let fonts = Fonts::new(&store, &bundle).unwrap();
    let text = GameText::load(&store).unwrap_or_default();
    Some(Env {
        bundle: Arc::new(bundle),
        text: Arc::new(text),
        fonts,
        prefs: Settings::default(),
        bindings: Bindings::default(),
        sink: NoSound,
        pointer: Pointer::default(),
    })
}

impl Env {
    fn run(&mut self) -> MenuRun<'_> {
        MenuRun {
            fonts: &mut self.fonts,
            prefs: &mut self.prefs,
            bindings: &mut self.bindings,
            sink: &mut self.sink,
            pointer: &mut self.pointer,
            lab: None,
            pause: None,
            tutorial: None,
        }
    }

    fn screen(&mut self) -> MenuScreen {
        let view = MenuView::new(self.bundle.clone(), self.text.clone(), [1600, 900]);
        MenuScreen::new(view, &mut self.run())
    }

    fn has(&self, classes: &[&str]) -> bool {
        let ok = classes.iter().all(|c| self.bundle.widgets.contains_key(*c));
        if !ok {
            eprintln!("a class of {classes:?} is not in the bundle: skipped");
        }
        ok
    }

    fn frames(&mut self, s: &mut MenuScreen, from: f64, seconds: f64) -> f64 {
        let mut t = from;
        while t < from + seconds {
            t += 1.0 / 60.0;
            s.frame(&mut self.run(), &[], t, &mut ElementList::default());
        }
        t
    }
}

#[test]
fn a_dialog_comes_up_on_its_timer_and_leaves_on_exit() {
    let Some(mut e) = env() else { return };
    if !e.has(&["UI_Dialog"]) {
        return;
    }
    let mut s = e.screen();
    let params = DialogParams {
        title: "Title".into(),
        body: "Body".into(),
        button_left_label: "Yes".into(),
        ..Default::default()
    };
    let id = s.with_cx(&mut e.run(), |cx| Manager::show_dialog(cx, params));
    assert_eq!(s.view.manager.dialog_stack, vec![id]);
    let t = e.frames(&mut s, 1.0, 1.0);
    assert!(s.view.in_viewport(id.0), "the delayed init put it on the viewport");
    assert!(s.view.dialog(id).unwrap().is_on_screen());
    let title = s.view.tree().find_in(id.0, "titleTITLE");
    assert_eq!(s.view.tree().widget(title).text, "TITLE");
    s.with_cx(&mut e.run(), |cx| {
        cx.visit_dyn(id.0, |c, cx| c.as_dialog().unwrap().exit(cx));
    });
    assert!(s.view.dialog(id).unwrap().is_exiting());
    e.frames(&mut s, t, 0.5);
    assert!(s.view.manager.dialog_stack.is_empty(), "the faded dialog left the stack");
    assert!(!s.view.in_viewport(id.0));
}

#[test]
fn a_notification_times_out() {
    let Some(mut e) = env() else { return };
    if !e.has(&["UI_Notification"]) {
        return;
    }
    let mut s = e.screen();
    let params = NotificationParams { title: "Hello".into(), screen_time: 1.0, ..Default::default() };
    let w = s.with_cx(&mut e.run(), |cx| Manager::show_notification(cx, params)).expect("an immediate toast");
    assert_eq!(s.view.manager.notifications_shown(), &[w]);
    // (from the view's time 0, where the toast's RemoveTimer started)
    let t = e.frames(&mut s, 0.0, 0.5);
    assert!(s.view.in_viewport(w));
    e.frames(&mut s, t, 1.5);
    assert!(s.view.manager.notifications_shown().is_empty(), "the RemoveTimer ended it");
}

#[test]
fn a_context_menu_has_a_row_per_button() {
    let Some(mut e) = env() else { return };
    if !e.has(&["UI_ContextMenu", "UI_ContextButton"]) {
        return;
    }
    let mut s = e.screen();
    let buttons =
        ["a", "b", "c"].map(|i| ContextMenuButton { id: i.into(), label: i.to_uppercase(), tooltip: String::new() });
    let params = ContextMenuParams {
        id: "test".into(),
        buttons: buttons.to_vec(),
        set_context: true,
        auto_close: true,
        position_offset: [100.0, 100.0],
        ..Default::default()
    };
    let (menu, rows) = s.with_cx(&mut e.run(), |cx| Manager::show_context_menu(cx, params, 0));
    assert_eq!(rows.iter().map(|r| r.0.as_str()).collect::<Vec<_>>(), ["a", "b", "c"]);
    let t = e.frames(&mut s, 1.0, 0.5);
    assert!(s.view.in_viewport(menu));
    s.with_cx(&mut e.run(), |cx| Manager::hide_context_menu(cx, "test", 0));
    e.frames(&mut s, t, 0.1);
    assert!(!s.view.in_viewport(menu));
}

/// A mouse click away from an open context menu (on its full-screen dismissTarget) closes it, as a drop menu's list.
#[test]
fn a_click_outside_a_context_menu_closes_it() {
    use mvs_ui::menu::input::MenuInput;
    let Some(mut e) = env() else { return };
    if !e.has(&["UI_ContextMenu", "UI_ContextButton"]) {
        return;
    }
    let mut s = e.screen();
    let buttons =
        ["a", "b"].map(|i| ContextMenuButton { id: i.into(), label: i.to_uppercase(), tooltip: String::new() });
    let params = ContextMenuParams {
        id: "test".into(),
        buttons: buttons.to_vec(),
        set_context: true,
        position_offset: [100.0, 100.0],
        ..Default::default()
    };
    let (menu, _) = s.with_cx(&mut e.run(), |cx| Manager::show_context_menu(cx, params, 0));
    let mut t = e.frames(&mut s, 1.0, 0.5);
    assert!(s.view.in_viewport(menu));
    for n in 0..40u32 {
        let input = match n {
            0 => vec![MenuInput::MouseMove { x: 1397.0, y: 700.0 }],
            4 => vec![MenuInput::MouseMove { x: 1400.0, y: 700.0 }],
            8 => vec![MenuInput::MouseButton { button: 0, down: true }],
            10 => vec![MenuInput::MouseButton { button: 0, down: false }],
            _ => Vec::new(),
        };
        t += 1.0 / 60.0;
        s.frame(&mut e.run(), &input, t, &mut ElementList::default());
    }
    assert!(!s.view.in_viewport(menu), "the click outside closed the menu");
}

#[test]
fn the_pips_turn_the_page_and_wrap() {
    let Some(mut e) = env() else { return };
    if !e.has(&["UI_PaginationPips", "UI_PaginationPip"]) {
        return;
    }
    let mut s = e.screen();
    let w = s.with_cx(&mut e.run(), |cx| {
        let w = MenuView::create(cx, "UI_PaginationPips");
        MenuView::add_to_viewport(cx, w, 0);
        cx.visit::<PaginationPips, _>(w, |p, cx| p.init(cx, 3, 0));
        w
    });
    let at = |s: &mut MenuScreen, e: &mut Env, d: i32| {
        s.with_cx(&mut e.run(), |cx| {
            cx.visit::<PaginationPips, _>(w, |p, cx| {
                if d != 0 {
                    p.increment_page(cx, d);
                }
                p.active_index
            })
            .unwrap()
        })
    };
    assert_eq!(at(&mut s, &mut e, 0), 0);
    assert_eq!(at(&mut s, &mut e, 1), 1);
    assert_eq!(at(&mut s, &mut e, 1), 2);
    assert_eq!(at(&mut s, &mut e, 1), 0, "past the last page: the first");
    assert_eq!(at(&mut s, &mut e, -1), 2, "before the first: the last");
}
