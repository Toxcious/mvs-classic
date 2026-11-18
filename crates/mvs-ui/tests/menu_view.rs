//! The menu framework on the menu bundle: a screen's frame (the fade in, the pointer, the tape line), a user
//! widget with a controller made, laid out and painted, a modal without a controller. Needs the dev store; without
//! one each test says so and passes.

use std::sync::Arc;

use mvs_settings::Settings;
use mvs_ui::Bundle;
use mvs_ui::bindings::Bindings;
use mvs_ui::element::ElementList;
use mvs_ui::menu::controller::MenuRun;
use mvs_ui::menu::input::{MenuInput, Pointer};
use mvs_ui::menu::widgets::common::Quadrilateral;
use mvs_ui::menu::widgets::modals::show_modal_generic;
use mvs_ui::menu::{MenuScreen, MenuView, NoSound};
use mvs_ui::slate::Vis;
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
}

#[test]
fn the_menu_bundle_has_its_data() {
    let Some(e) = env() else { return };
    let m = e.bundle.menu.as_ref().expect("the menu bundle's data");
    assert!(!m.roster.is_empty());
    assert!(!m.fighters.is_empty());
}

#[test]
fn a_level_fades_in_from_black_over_three_tenths() {
    let Some(mut e) = env() else { return };
    let mut s = e.screen();
    s.level_fade_in(&mut e.run(), 10.0);
    let mut out = ElementList::default();
    s.frame(&mut e.run(), &[], 10.0, &mut out);
    // the fade starts at the transition's first tick: black
    assert_eq!(s.view.manager.fader, 1.0);
    assert!(!out.elements.is_empty(), "the fader is painted");
    s.frame(&mut e.run(), &[], 10.15, &mut ElementList::default());
    assert!((s.view.manager.fader - 0.5).abs() < 1e-6, "half way: {}", s.view.manager.fader);
    s.frame(&mut e.run(), &[], 10.35, &mut ElementList::default());
    assert_eq!(s.view.manager.fader, 0.0);
    assert!(!s.view.manager.is_transitioning());
    // (a fade with no level opens none)
    assert!(s.frame(&mut e.run(), &[], 10.4, &mut ElementList::default()).is_none());
}

#[test]
fn the_mouse_moves_the_pointer_of_the_window_and_the_view() {
    let Some(mut e) = env() else { return };
    let mut s = e.screen();
    // (each window frame as app.rs runs it: the claims cleared, the window's moves, the screens' frames)
    let frame = |e: &mut Env, s: &mut MenuScreen, input: &[MenuInput], t: f64| {
        e.pointer.begin_frame();
        e.pointer.window_moves(input);
        s.frame(&mut e.run(), input, t, &mut ElementList::default());
    };
    frame(&mut e, &mut s, &[MenuInput::MouseMove { x: 400.0, y: 300.0 }], 1.0);
    assert!(e.pointer.os_cursor_hidden(), "an active screen with the game's cursor hides the OS cursor");
    assert!(e.pointer.input_taken());
    assert_eq!(e.pointer.at, Some([400.0, 300.0]));
    assert_eq!(s.view.mouse, Some([400.0, 300.0]));
    assert_eq!(s.tape_frame().mouse, Some([400.0, 300.0]));
    // a screen no longer active gives the OS cursor back and takes no move into its view
    s.set_active(&mut e.run(), false);
    frame(&mut e, &mut s, &[MenuInput::MouseMove { x: 10.0, y: 20.0 }], 1.1);
    assert!(!e.pointer.os_cursor_hidden());
    assert!(!e.pointer.input_taken());
    assert_eq!(e.pointer.at, Some([10.0, 20.0]));
    assert_eq!(s.view.mouse, Some([400.0, 300.0]));
    s.remove(&mut e.run());
}

#[test]
fn a_screen_without_the_cursor_leaves_the_os_cursor() {
    let Some(mut e) = env() else { return };
    let view = MenuView::new(e.bundle.clone(), e.text.clone(), [1600, 900]);
    let mut s = MenuScreen::without_cursor(view);
    e.pointer.begin_frame();
    s.frame(&mut e.run(), &[MenuInput::MouseMove { x: 400.0, y: 300.0 }], 1.0, &mut ElementList::default());
    assert!(!e.pointer.os_cursor_hidden(), "the title has no game cursor: the OS cursor shows");
    assert!(e.pointer.input_taken());
    s.remove(&mut e.run());
}

#[test]
fn a_new_screen_starts_from_the_window_s_last_interaction() {
    use mvs_ui::menu::manager::Interaction;
    let Some(mut e) = env() else { return };
    // the pad last (another screen's, or a match's touches)
    e.pointer.interaction = Interaction { last_type: 0, input_type: 0, cursor_mode: false };
    let mut s = e.screen();
    e.pointer.begin_frame();
    s.frame(&mut e.run(), &[], 1.0, &mut ElementList::default());
    let m = &s.view.manager;
    assert_eq!((m.last_interaction_type[0], m.input_type[0], m.cursor_mode[0]), (0, 0, false));
    // the mouse in this screen: the window's record follows
    e.pointer.begin_frame();
    let moves = [MenuInput::MouseMove { x: 400.0, y: 300.0 }, MenuInput::MouseMove { x: 420.0, y: 300.0 }];
    s.frame(&mut e.run(), &moves, 1.1, &mut ElementList::default());
    assert_eq!(e.pointer.interaction, Interaction { last_type: 2, input_type: 2, cursor_mode: true });
    s.remove(&mut e.run());
}

#[test]
fn a_menu_header_is_made_laid_out_and_painted() {
    let Some(mut e) = env() else { return };
    if !e.bundle.widgets.contains_key("WBP_MenuHeader") {
        eprintln!("no WBP_MenuHeader in the bundle: skipped");
        return;
    }
    let mut s = e.screen();
    let w = s.with_cx(&mut e.run(), |cx| {
        let w = MenuView::create(cx, "WBP_MenuHeader");
        MenuView::add_to_viewport(cx, w, 0);
        w
    });
    assert!(s.view.is_constructed(w));
    // PreConstruct: ShowStrip is not in the class defaults, so the strip is collapsed
    let strip = s.view.tree().find_in(w, "StripVerticalBox");
    assert_eq!(s.view.tree().widget(strip).vis, Vis::Collapsed);
    let mut out = ElementList::default();
    s.frame(&mut e.run(), &[], 2.0, &mut out);
    assert!(!out.elements.is_empty());
    assert_eq!(s.tape_frame().roots.len(), 1);
}

/// A widget taken off the viewport and added again is live again: Construct runs a second time and its hover and
/// delays are heard (a re-added screen's interactor was deaf).
#[test]
fn a_widget_added_again_is_live_again() {
    let Some(mut e) = env() else { return };
    if !e.bundle.widgets.contains_key("WBP_MenuHeader") {
        eprintln!("no WBP_MenuHeader in the bundle: skipped");
        return;
    }
    let mut s = e.screen();
    let w = s.with_cx(&mut e.run(), |cx| {
        let w = MenuView::create(cx, "WBP_MenuHeader");
        MenuView::add_to_viewport(cx, w, 0);
        w
    });
    s.with_cx(&mut e.run(), |cx| MenuView::remove_from_parent(cx, w));
    assert!(s.view.is_destroyed(w));
    assert!(!s.view.is_constructed(w));
    s.with_cx(&mut e.run(), |cx| MenuView::add_to_viewport(cx, w, 0));
    assert!(s.view.is_constructed(w));
    assert!(!s.view.is_destroyed(w));
}

/// The RootCanvas add_to_view wraps a widget in goes with it at remove_from_parent: its index is the next one made,
/// under a new generation.
#[test]
fn a_root_canvas_is_destroyed_with_its_layer() {
    let Some(mut e) = env() else { return };
    if !e.bundle.widgets.contains_key("WBP_MenuHeader") {
        eprintln!("no WBP_MenuHeader in the bundle: skipped");
        return;
    }
    let mut s = e.screen();
    let (w, canvas) = s.with_cx(&mut e.run(), |cx| {
        let w = MenuView::create(cx, "WBP_MenuHeader");
        MenuView::add_to_view(cx, w, false, 0);
        (w, cx.view.tree().widget(w).parent.expect("a RootCanvas"))
    });
    s.with_cx(&mut e.run(), |cx| MenuView::remove_from_parent(cx, w));
    assert!(s.view.tree().widget(w).parent.is_none());
    assert!(!s.view.tree().alive(canvas));
    assert!(!s.view.in_viewport(w));
    s.with_cx(&mut e.run(), |cx| MenuView::add_to_view(cx, w, false, 0));
    let again = s.view.tree().widget(w).parent.expect("a new RootCanvas");
    assert_eq!(again.index(), canvas.index());
    assert_ne!(again, canvas);
    assert!(s.view.in_viewport(w));
    assert!(s.view.is_constructed(w));
}

#[test]
fn a_modal_whose_class_has_no_controller_is_not_shown() {
    let Some(mut e) = env() else { return };
    let mut s = e.screen();
    let cls = "UI_Cursor";
    assert!(e.bundle.widgets.contains_key(cls));
    let m = s.with_cx(&mut e.run(), |cx| show_modal_generic(cx, true, cls, "test", ""));
    assert!(m.is_none());
    assert!(s.view.manager.get_modal("test").is_none());
}

#[test]
fn a_quadrilateral_wobbles_its_corners() {
    let Some(mut e) = env() else { return };
    if !e.bundle.widgets.contains_key("WBP_Quadrilateral") {
        eprintln!("no WBP_Quadrilateral in the bundle: skipped");
        return;
    }
    let mut s = e.screen();
    let w = s.with_cx(&mut e.run(), |cx| {
        let w = MenuView::create(cx, "WBP_Quadrilateral");
        MenuView::add_to_viewport(cx, w, 0);
        w
    });
    let before = s.view.tree().widget(w).quad.clone();
    s.with_cx(&mut e.run(), |cx| {
        cx.visit::<Quadrilateral, _>(w, |q, cx| q.wobble(cx, true)).expect("the quad's controller");
    });
    assert_eq!(s.view.tree().widget(w).opacity, 1.0);
    for f in 0..120 {
        s.frame(&mut e.run(), &[], 3.0 + f64::from(f) / 60.0, &mut ElementList::default());
    }

    let after = s.view.tree().widget(w).quad.clone();
    eprintln!("quad before {before:?}\nafter {after:?}");
    s.with_cx(&mut e.run(), |cx| {
        cx.visit::<Quadrilateral, _>(w, |q, cx| q.wobble(cx, false));
    });
    assert_eq!(s.view.tree().widget(w).opacity, 0.0);
}
