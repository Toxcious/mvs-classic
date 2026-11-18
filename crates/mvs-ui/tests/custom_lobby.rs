//! The custom lobby widgets on the menu bundle: each is made, ticked and painted without a panic. Needs the dev
//! store; without one each test says so and passes.

use std::sync::Arc;

use mvs_settings::Settings;
use mvs_ui::Bundle;
use mvs_ui::bindings::Bindings;
use mvs_ui::element::ElementList;
use mvs_ui::menu::Manager;
use mvs_ui::menu::controller::MenuRun;
use mvs_ui::menu::input::Pointer;
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
}

fn shown(cls: &str, id: &str) {
    let Some(mut e) = env() else { return };
    let mut s = e.screen();
    let w = s.with_cx(&mut e.run(), |cx| Manager::show_modal(cx, cls, id, "", ""));
    assert!(w.is_some(), "{cls} shown");
    let mut out = ElementList::default();
    for f in 0..8 {
        s.frame(&mut e.run(), &[], 1.0 + f64::from(f) / 60.0, &mut out);
    }
}

#[test]
fn the_custom_game_lobby_shows() {
    shown("WBP_CustomGame", "customGame");
}

/// A player who is not ready keeps a full-colour portrait (the port's own: the build's dim
/// SetReady portrait read as a player still joining) and shows NOT READY under the name when the lobby
/// has a READY button and the row is not the leader's.
#[test]
fn a_player_who_is_not_ready_keeps_the_portrait_and_shows_a_tag() {
    use mvs_ui::element::Element;
    use mvs_ui::menu::widgets::custom_game::CustomGamePlayer;
    use mvs_ui::slate::Vis;
    let Some(mut e) = env() else { return };
    let mut s = e.screen();
    let w = s.with_cx(&mut e.run(), |cx| Manager::show_modal(cx, "WBP_CustomGame", "customGame", "", ""));
    let w = w.expect("the lobby is shown");
    let mut out = ElementList::default();
    s.frame(&mut e.run(), &[], 1.0, &mut out);
    let dim = |out: &ElementList| {
        out.elements
            .iter()
            .filter(|p| {
                let tint = match &p.element {
                    Element::Box(b) => b.tint,
                    Element::Material(m) => m.tint,
                    Element::Retained(r) => r.tint,
                    _ => return false,
                };
                tint[..3].iter().all(|c| (c - 0.213542).abs() < 1e-4)
            })
            .count()
    };
    assert_eq!(dim(&out), 0, "the leader is ready");
    let rows: Vec<_> = {
        let t = s.view.tree();
        t.post_order(w).into_iter().filter(|&r| t.user_class(r) == Some("WBP_CustomGamePlayer")).collect()
    };
    let unready = s.with_cx(&mut e.run(), |cx| {
        rows.iter().copied().find(|&r| {
            cx.visit::<CustomGamePlayer, _>(r, |c, cx| {
                let Some(p) = c.player.clone() else { return false };
                // (another player leads; the lobby has a READY button)
                let mut f = c.facts.clone();
                f.ready_button = true;
                c.update_data(cx, p, "someone-else", false, false, f);
                true
            }) == Some(true)
        })
    });
    let row = unready.expect("the leader's row");
    s.frame(&mut e.run(), &[], 1.1, &mut out);
    assert_eq!(dim(&out), 0, "the portrait keeps its colour");
    let t = s.view.tree();
    let tag = t.widget(t.find_in(row, "ReadyTag"));
    assert_eq!(tag.text, mvs_ui::menu::custom_text::SLOT_NOT_READY);
    assert_ne!(tag.vis, Vis::Collapsed);
}

#[test]
fn the_custom_browser_shows() {
    shown("CustomGameBrowser", "customBrowser");
}

#[test]
fn the_quick_match_settings_show() {
    shown("UI_QuickMatchSettings", "quickMatch");
}

#[test]
fn the_input_binding_modal_shows() {
    shown("UI_InputBindingModal", "inputSettings");
}
