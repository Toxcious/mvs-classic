//! The menus' motion with the cursor and at a screen's start: the cursor's Parallaxers (UI_ParallaxItems and
//! WBP_GameModeCard's focus), the title's game cursor, Local Play's slots entering. Needs the dev store; without one
//! each test says so and passes.

use std::sync::Arc;

use mvs_settings::Settings;
use mvs_ui::Bundle;
use mvs_ui::bindings::Bindings;
use mvs_ui::element::ElementList;
use mvs_ui::menu::controller::MenuRun;
use mvs_ui::menu::cursor::Cursor;
use mvs_ui::menu::input::{MenuInput, Pointer};
use mvs_ui::menu::widgets::controls::parallax_items::ParallaxItems;
use mvs_ui::menu::widgets::match_options::GameModeCard;
use mvs_ui::menu::{MenuScreen, MenuView, NoSound};
use mvs_ui::slate::WidgetId;
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

    /// One window frame at `t` as app.rs runs it.
    fn frame(&mut self, s: &mut MenuScreen, input: &[MenuInput], t: f64) {
        self.pointer.begin_frame();
        self.pointer.window_moves(input);
        s.frame(&mut self.run(), input, t, &mut ElementList::default());
    }
}

fn parallaxers(s: &mut MenuScreen) -> Vec<(WidgetId, i32)> {
    let c = s.view.cursor.expect("the screen's cursor");
    s.view.controller::<Cursor>(c).map(|c| c.parallaxers.iter().map(|p| (p.widget, p.multiplier)).collect()).unwrap()
}

#[test]
fn a_mode_card_drifts_with_the_pointer_while_focused() {
    let Some(mut e) = env() else { return };
    let mut s = e.screen();
    let card = s.with_cx(&mut e.run(), |cx| MenuView::add(cx, "WBP_GameModeCard"));
    let (char1, char2, items) = {
        let t = s.view.tree();
        (t.find_in(card, "Char1"), t.find_in(card, "Char2"), t.find_in(card, "UI_ParallaxItems"))
    };
    let read: Vec<_> = s.view.controller::<ParallaxItems>(items).unwrap().items.iter().map(|i| i.widget).collect();
    assert_eq!(read, [Some(char1), Some(char2)], "the instance's items, looked up in the card's tree");
    // the Locate Timer finds the cursor at its first 0.05 s
    let mut t = 1.0;
    for _ in 0..6 {
        e.frame(&mut s, &[], t);
        t += 1.0 / 60.0;
    }
    assert_eq!(parallaxers(&mut s), [(char1, 0), (char2, 0)]);
    // Select: 40 and 10
    s.with_cx(&mut e.run(), |cx| cx.visit::<GameModeCard, _>(card, |c, cx| c.select(cx, false)));
    e.frame(&mut s, &[], t);
    assert_eq!(parallaxers(&mut s), [(char1, 40), (char2, 10)]);
    // the pointer at the right edge, the middle's height: the mouse turns cursor mode on, the visibility check
    // (every 0.25 s) shows the cursor, then OffsetView moves the two left
    let mut input = vec![MenuInput::MouseMove { x: 1500.0, y: 450.0 }, MenuInput::MouseMove { x: 1600.0, y: 450.0 }];
    for _ in 0..90 {
        t += 1.0 / 60.0;
        e.frame(&mut s, &input, t);
        input.clear();
    }
    let tr = |s: &MenuScreen, w| s.view.tree().widget(w).transform.translation;
    let (a, b) = (tr(&s, char1), tr(&s, char2));
    assert!(a[0] < -39.0 && a[0] >= -40.0 && a[1].abs() < 1e-9, "Char1 eased toward (-40, 0): {a:?}");
    assert!(b[0] < -9.0 && b[0] >= -10.0, "Char2 toward (-10, 0): {b:?}");
    // Deselect: back to 0
    s.with_cx(&mut e.run(), |cx| cx.visit::<GameModeCard, _>(card, |c, cx| c.deselect(cx, false)));
    for _ in 0..90 {
        t += 1.0 / 60.0;
        e.frame(&mut s, &[], t);
    }
    assert_eq!(parallaxers(&mut s), [(char1, 0), (char2, 0)]);
    assert!(tr(&s, char1)[0].abs() < 1e-3, "Char1 back: {:?}", tr(&s, char1));
    // the card removed: the cursor drops its entries
    s.with_cx(&mut e.run(), |cx| MenuView::remove_from_viewport(cx, card));
    e.frame(&mut s, &[], t + 0.1);
    assert!(parallaxers(&mut s).is_empty());
    s.remove(&mut e.run());
}

#[test]
fn local_plays_slots_enter_column_by_column() {
    let Some(mut e) = env() else { return };
    let mut s = e.screen();
    let lp = s.with_cx(&mut e.run(), |cx| MenuView::add(cx, "UI_LocalPlay"));
    let slots: Vec<WidgetId> = (0..4).map(|i| s.view.tree().find_in(lp, &format!("UI_LP_Player{i}"))).collect();
    let cols: Vec<i32> = slots.iter().map(|&w| s.view.tree().widget(w).col).collect();
    let root = |s: &MenuScreen, w| {
        let r = s.view.tree().find_in(w, "rootContainer");
        s.view.tree().widget(r).opacity
    };
    // Enter: 0.1 s, then each slot's root hidden and its EnterAnim (0.3 s) after (column + 1) x 0.15 s
    let mut t = 1.0;
    e.frame(&mut s, &[], t);
    let start = t;
    let mut hidden = false;
    let mut played = [None; 4];
    while t < start + 1.5 {
        t += 1.0 / 60.0;
        e.frame(&mut s, &[], t);
        let dt = t - start;
        if dt > 0.12 && dt < 0.2 {
            hidden = true;
            for &w in &slots {
                assert_eq!(root(&s, w), 0.0, "every slot's root hidden after the first 0.1 s");
            }
        }
        for (i, &w) in slots.iter().enumerate() {
            if played[i].is_none() && s.view.is_animation_playing(w, "EnterAnim") {
                played[i] = Some(dt);
            }
        }
    }
    assert!(hidden);
    for i in 0..4 {
        let at = played[i].expect("the slot's EnterAnim played");
        let want = 0.1 + f64::from(cols[i] + 1) * 0.15;
        assert!((at - want).abs() < 2.5 / 60.0, "slot {i} (column {}) entered at {at}, want {want}", cols[i]);
        assert!((root(&s, slots[i]) - 1.0).abs() < 1e-6, "slot {i} shown at the animation's end");
    }
    s.remove(&mut e.run());
}

#[test]
fn a_pointer_only_screens_keys_turn_cursor_mode_off() {
    let Some(mut e) = env() else { return };
    let mut s = e.screen();
    s.pointer_only();
    let mut t = 1.0;
    let mut step = |e: &mut Env, s: &mut MenuScreen, input: &[MenuInput]| {
        t += 1.0 / 60.0;
        e.frame(s, input, t);
    };
    step(&mut e, &mut s, &[MenuInput::MouseMove { x: 500.0, y: 400.0 }]);
    step(&mut e, &mut s, &[MenuInput::MouseMove { x: 700.0, y: 500.0 }]);
    step(&mut e, &mut s, &[]);
    assert!(e.pointer.interaction.cursor_mode, "the mouse turned cursor mode on");
    step(&mut e, &mut s, &[MenuInput::Key { key: "Enter", down: true, repeat: false, player: 0 }]);
    step(&mut e, &mut s, &[MenuInput::Key { key: "Enter", down: false, repeat: false, player: 0 }]);
    assert!(!e.pointer.interaction.cursor_mode, "a key turned it off");
    assert_eq!(e.pointer.interaction.last_type, 1, "the keyboard the last device");
    step(&mut e, &mut s, &[MenuInput::MouseButton { button: 0, down: true }]);
    assert_eq!(e.pointer.interaction.last_type, 2, "a mouse button names the mouse");
    s.remove(&mut e.run());
}
