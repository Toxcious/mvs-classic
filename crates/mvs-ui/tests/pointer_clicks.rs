//! The mouse's clicks on the screens over a level: the podium's buttons and the pre-match screen's skin tiles, moved
//! to and clicked as the window gives them (a move, a press, a release, a frame each). Needs the dev store; without
//! one each test says so and passes.

use std::sync::Arc;

use mvs_settings::Settings;
use mvs_ui::Bundle;
use mvs_ui::bindings::Bindings;
use mvs_ui::element::ElementList;
use mvs_ui::menu::controller::MenuRun;
use mvs_ui::menu::input::{MenuInput, Pointer};
use mvs_ui::menu::{MenuExit, MenuScreen, MenuView, NoSound};
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
}

/// The centre of the `nth` shown widget whose path ends in `name` at frame `frame`.
fn centre_of(screen: &MenuScreen, frame: u32, name: &str, nth: usize) -> Option<[f64; 2]> {
    let tape = screen.view.tape_frame(frame);
    let suffix = format!("/{name}");
    let w = tape
        .roots
        .iter()
        .flat_map(|r| r.widgets.iter())
        .filter(|w| w.vis != 1 && w.vis != 2 && (w.path == name || w.path.ends_with(&suffix)))
        .nth(nth)?;
    let [a, b, c, d, e, f] = w.matrix;
    let [x, y] = [w.size[0] / 2.0, w.size[1] / 2.0];
    Some([a * x + c * y + e, b * x + d * y + f])
}

/// A mouse click on `at` as the window gives it from frame `n` on: a move next to it, a move onto it, a press, a
/// release, two frames apart.
fn click_input(n: u32, from: u32, at: [f64; 2]) -> Vec<MenuInput> {
    let [x, y] = at;
    match n.checked_sub(from) {
        Some(0) => vec![MenuInput::MouseMove { x: x - 3.0, y }],
        Some(2) => vec![MenuInput::MouseMove { x, y }],
        Some(4) => vec![MenuInput::MouseButton { button: 0, down: true }],
        Some(6) => vec![MenuInput::MouseButton { button: 0, down: false }],
        _ => Vec::new(),
    }
}

/// The podium's NEXT (UI_SteppedNextButton) is disabled in the tree and enabled by its BPI Start
/// (SetIsEnabled(true)): the mouse's click on it completes the step, and the Local list's UI_EOG_ReturnHome after it
/// opens the main menu.
#[test]
fn the_podium_next_button_takes_a_click() {
    use mvs_ui::menu::screens::results;
    use mvs_ui::results::EndOfGame;
    let Some(mut e) = env() else { return };
    let view = MenuView::new(e.bundle.clone(), e.text.clone(), [1920, 1080]);
    let mut s = MenuScreen::new(view, &mut e.run());
    // a 1v1 the local player won (the screen fades in from the match's black only with a record of its own)
    let entry = |i: i32, winner: bool| mvs_sim::world::flow::PodiumEntry {
        player_index: i,
        team_index: i,
        ring_outs: 0,
        damage_dealt: mvs_sim::math::narrow(0.0),
        falls: 0,
        score: mvs_sim::math::narrow(0.0),
        is_winner: winner,
        assists: 0,
        off_platform_ring_outs: 0,
        assisted: 0,
        doubles: 0,
        dodges: 0,
    };
    let eog = EndOfGame {
        local_play: true,
        me: 0,
        names: vec!["P1".into(), "P2".into()],
        players: vec![entry(0, true), entry(1, false)],
        ..EndOfGame::default()
    };
    let mut st = results::show(&mut s, &mut e.run(), &eog, None);
    struct NoPoints;
    impl results::PodiumPoints for NoPoints {
        fn point(&self, _: mvs_render::menuscene::ScenePoint) -> Option<[f64; 3]> {
            None
        }
        fn screen_place(&self, _: [f64; 3]) -> Option<([f64; 2], f64)> {
            None
        }
    }
    let mut t = 1.0;
    let mut end = None;
    let mut at = None;
    for n in 0..700u32 {
        if n == 60 {
            let arg = mvs_render::menuscene::format::EventArg::Str("ShowUI".into());
            results::podium_event(&mut s, &mut e.run(), &mut st, &NoPoints, "PodiumEvent", &[arg]);
        }
        if n == 500 {
            at = Some(centre_of(&s, n - 1, "buttonNext", 0).expect("NEXT shown"));
        }
        let input = at.map_or_else(Vec::new, |a| click_input(n, 500, a));
        if let Some(x) = s.frame(&mut e.run(), &input, t, &mut ElementList::default()) {
            assert!(n > 500, "nothing ends the screen before the click");
            end = Some(x);
            break;
        }
        t += 1.0 / 60.0;
    }
    let level = match end {
        Some(MenuExit::OpenLevel { level, .. }) => level,
        other => panic!("the click on NEXT opens a level: {other:?}"),
    };
    assert_eq!(level, "New_MainMenu");
}

/// On the pre-match screen, a click on a character tile picks it and a click on a skin tile then picks that skin:
/// the skin selector's choice and the list's check mark move to it.
#[test]
fn the_pre_match_skin_tile_takes_a_click() {
    use mvs_ui::menu::screens::lab_selectors::{GridItemList, TileData};
    use mvs_ui::menu::screens::pre_match::child_widgets::PMSkinSelector;
    use mvs_ui::menu::screens::pre_match::{self, setup::*, widget::PreMatch};
    let Some(mut e) = env() else { return };
    let view = MenuView::new(e.bundle.clone(), e.text.clone(), [1920, 1080]);
    let mut s = MenuScreen::new(view, &mut e.run());
    let human = |id: &str, team: u8| PreMatchPlayer {
        account_id: id.into(),
        username: id.into(),
        team,
        is_bot: false,
        character: None,
        skin: String::new(),
        cosmetics: None,
        net_cosmetics: None,
    };
    let mut bot = human("bot", 1);
    bot.is_bot = true;
    bot.character = Some("shaggy".into());
    let setup = PreMatchSetup { players: vec![human("local", 0), bot], leader: true, ..PreMatchSetup::default() };
    let pm = pre_match::show(&mut s, &mut e.run(), PreMatchStart { setup, lobby: None });
    pm.set_level(&mut s, false);
    let character = "UI_PM_CharacterItem/sizeContainer/Overlay_214/Content/UI_InteractorP0/hitbox";
    let skin = "UI_EquipperItem_V2#1/VerticalBox_2/SizeContainer/Overlay_1/UI_Interactor/hitbox";
    let mut t = 1.0;
    let mut at = None;
    for n in 0..700u32 {
        if n == 400 || n == 600 {
            let name = if n == 400 { character } else { skin };
            at = Some(centre_of(&s, n - 1, name, 0).unwrap_or_else(|| panic!("{name} shown")));
        }
        let from = if n >= 600 { 600 } else { 400 };
        let input = at.map_or_else(Vec::new, |a| click_input(n, from, a));
        s.frame(&mut e.run(), &input, t, &mut ElementList::default());
        t += 1.0 / 60.0;
        if n == 599 {
            assert_eq!(s.view.controller::<PreMatch>(pm.widget).map(|p| p.state), Some(2), "the character click picks");
        }
    }
    let selector = s.view.tree().find_in(pm.widget, "UI_PM_SkinSelectorP0");
    let list = s.view.tree().find_in(selector, "skinList");
    let (second, marks) = s
        .view
        .controller::<GridItemList>(list)
        .map(|l| {
            let items = l.items();
            let marks: Vec<bool> = items
                .iter()
                .map(|&w| matches!(l.record(w), Some((Some(TileData::Skin(k)), _)) if k.selected))
                .collect();
            let second = match l.record(items[1]) {
                Some((Some(TileData::Skin(k)), _)) => k.skin.slug.clone(),
                _ => String::new(),
            };
            (second, marks)
        })
        .expect("the skin list");
    let picked =
        s.view.controller::<PMSkinSelector>(selector).and_then(|c| c.selected_skin.as_ref().map(|k| k.slug.clone()));
    assert_eq!(picked.as_deref(), Some(second.as_str()), "the click on the second skin picks it");
    assert_eq!(marks.iter().position(|&m| m), Some(1), "the check mark moves to the second skin: {marks:?}");
    assert_eq!(marks.iter().filter(|&&m| m).count(), 1);
}

/// The pad's left stick moves the game's cursor and with it the OS pointer (UI_Cursor's Set Position is
/// SetMouseLocation): each write of the stick is the window's point and a warp for the program; a mouse move is the
/// mouse's own and asks for none.
#[test]
fn the_stick_cursor_moves_the_os_pointer() {
    use mvs_ui::menu::input::MenuPad;
    let Some(mut e) = env() else { return };
    let view = MenuView::new(e.bundle.clone(), e.text.clone(), [1920, 1080]);
    let mut s = MenuScreen::new(view, &mut e.run());
    let mut t = 1.0;
    let frame = |s: &mut MenuScreen, e: &mut Env, input: &[MenuInput], t: &mut f64| {
        s.frame(&mut e.run(), input, *t, &mut ElementList::default());
        *t += 1.0 / 60.0;
    };
    frame(&mut s, &mut e, &[MenuInput::MouseMove { x: 400.0, y: 300.0 }], &mut t);
    frame(&mut s, &mut e, &[MenuInput::MouseMove { x: 420.0, y: 300.0 }], &mut t);
    assert_eq!(e.pointer.at, Some([420.0, 300.0]));
    assert_eq!(e.pointer.take_warp(), None, "a mouse move moves no OS pointer");
    // the left stick to the right, held
    let pad = MenuInput::Pad { pad: 0, player: 0, state: MenuPad { axes: [1.0, 0.0, 0.0, 0.0], buttons: 0 } };
    let mut last = None;
    for _ in 0..60 {
        frame(&mut s, &mut e, &[pad], &mut t);
        if let Some(w) = e.pointer.take_warp() {
            assert_eq!(Some(w), e.pointer.at, "the warp is the window's point");
            last = Some(w);
        }
    }
    let [x, y] = last.expect("the stick moved the cursor");
    assert!(x > 420.0 && y == 300.0, "to the right: {x}, {y}");
}

/// The main menu party partner's click target (BP_SocialMenu_3's WBP_3dClick, placed as a screen rectangle): a click
/// inside it while enabled is taken for the partner's account; alone (disabled and not placed) a click is not.
#[test]
fn the_partner_click_target_takes_a_click() {
    use mvs_ui::menu::screens::main_menu::PartnerClick;
    let Some(mut e) = env() else { return };
    if !e.bundle.widgets.contains_key("WBP_3dClick") {
        eprintln!("no WBP_3dClick in the ui kind: skipped");
        return;
    }
    let view = MenuView::new(e.bundle.clone(), e.text.clone(), [1920, 1080]);
    let mut s = MenuScreen::new(view, &mut e.run());
    s.view.show_cursor(true);
    let mut c = PartnerClick::new(&mut s, &mut e.run()).expect("the class");
    c.set(&mut s, &mut e.run(), Some("pal"));
    // a 120 x 200 pixel rectangle about (1400, 500)
    let edges = [Some([1340.0, 500.0]), Some([1460.0, 500.0]), Some([1400.0, 600.0]), Some([1400.0, 400.0])];
    let mut t = 1.0;
    let mut clicks = Vec::new();
    for n in 0..200u32 {
        c.place(&mut s.view, Some(([1400.0, 500.0], 300.0)), edges);
        if n == 100 {
            c.set(&mut s, &mut e.run(), None);
        }
        // inside the rectangle near its corner, then (alone) at its centre
        let input = match n {
            0..100 => click_input(n, 40, [1450.0, 590.0]),
            _ => click_input(n, 140, [1400.0, 500.0]),
        };
        s.frame(&mut e.run(), &input, t, &mut ElementList::default());
        clicks.extend(c.take_clicks(&mut s.view).into_iter().map(|id| (n, id)));
        t += 1.0 / 60.0;
    }
    assert_eq!(clicks.len(), 1, "one click taken: {clicks:?}");
    assert!(clicks[0].0 < 100 && clicks[0].1 == "pal", "{clicks:?}");
}
