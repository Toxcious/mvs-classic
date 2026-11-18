//! The fighter select's own parts (the port's own): the character tiles show no
//! mastery level (as on the Lab's and the Collection's grids, the same tile), and the arcade's select shows no timer
//! while a custom game's runs its 45 s. Needs the dev store; without one each test says so and passes.

use std::sync::Arc;

use mvs_settings::Settings;
use mvs_ui::Bundle;
use mvs_ui::bindings::Bindings;
use mvs_ui::element::ElementList;
use mvs_ui::menu::arcade::ArcadeRun;
use mvs_ui::menu::controller::MenuRun;
use mvs_ui::menu::input::Pointer;
use mvs_ui::menu::screens::lab_selectors::{GridItemList, LEVEL_PARTS};
use mvs_ui::menu::screens::pre_match::{self, setup::*};
use mvs_ui::menu::{MenuScreen, MenuView, NoSound};
use mvs_ui::slate::Vis;
use mvs_ui::text::{Fonts, GameText};

/// The pre-match screen of a 1v1 against a bot (`arcade`: an arcade round's), run for 400 frames: the timer's
/// visibility and each character tile's level parts' visibilities.
fn select(arcade: bool) -> Option<(Vis, Vec<Vec<Vis>>)> {
    let store = match mvs_store::Store::open(mvs_store::dev_store_dir()) {
        Ok(s) if s.version("font").is_some() && s.version("ui").is_some() => s,
        _ => {
            eprintln!("no dev store with the font and ui kinds: skipped");
            return None;
        }
    };
    let bundle = Arc::new(Bundle::load(&store, "menu").expect("the menu bundle"));
    let mut fonts = Fonts::new(&store, &bundle).expect("the menu fonts");
    let text = Arc::new(GameText::load(&store).unwrap_or_default());
    let (mut prefs, mut bindings, mut sink, mut pointer) =
        (Settings::default(), Bindings::default(), NoSound, Pointer::default());
    let mut run = MenuRun {
        fonts: &mut fonts,
        prefs: &mut prefs,
        bindings: &mut bindings,
        sink: &mut sink,
        pointer: &mut pointer,
        lab: None,
        pause: None,
        tutorial: None,
    };
    let mut s = MenuScreen::new(MenuView::new(bundle, text, [1920, 1080]), &mut run);
    let player = |id: &str, team: u8, is_bot: bool| PreMatchPlayer {
        account_id: id.into(),
        username: id.into(),
        team,
        is_bot,
        character: is_bot.then(|| "shaggy".into()),
        ..PreMatchPlayer::default()
    };
    let setup = PreMatchSetup {
        players: vec![player("local", 0, false), player("bot", 1, true)],
        leader: true,
        arcade: arcade.then(|| ArcadeRun { round: 1, continues: -1, ..ArcadeRun::default() }),
        ..PreMatchSetup::default()
    };
    let pm = pre_match::show(&mut s, &mut run, PreMatchStart { setup, lobby: None });
    pm.set_level(&mut s, false);
    let mut t = 1.0;
    for _ in 0..400 {
        s.frame(&mut run, &[], t, &mut ElementList::default());
        t += 1.0 / 60.0;
    }
    let grid = s.view.tree().find_in(pm.widget, "CharacterGrid");
    let items = s.view.controller::<GridItemList>(grid).map(|g| g.items()).expect("the character grid");
    let tree = s.view.tree();
    let si = tree.find_in(pm.widget, "UI_PM_StateInfo");
    let timer = tree.widget(tree.find_in(si, "CenterTimerContainer")).vis;
    let tiles =
        items.iter().map(|&w| LEVEL_PARTS.iter().map(|n| tree.widget(tree.find_in(w, n)).vis).collect()).collect();
    Some((timer, tiles))
}

#[test]
fn the_tiles_show_no_level() {
    let Some((_, tiles)) = select(false) else { return };
    assert!(!tiles.is_empty(), "the grid has tiles");
    for parts in tiles {
        assert_eq!(parts, [Vis::Collapsed; LEVEL_PARTS.len()]);
    }
}

#[test]
fn the_arcade_select_has_no_timer() {
    let Some((timer, _)) = select(true) else { return };
    assert_eq!(timer, Vis::Hidden, "the arcade's timer");
    let Some((timer, _)) = select(false) else { return };
    assert_eq!(timer, Vis::Visible, "a custom game's 45 s timer");
}
