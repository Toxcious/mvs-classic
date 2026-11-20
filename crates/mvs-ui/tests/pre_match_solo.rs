//! The pre-match screen's solo layout (the port's own, `PreMatchSetup::solo`): a Waves run's screen names the mode
//! in the mode label and builds no second side: no opponent's plate, and after the timer's lock no opponent's card;
//! its own plate shows no stat badge, even one equipped.
//! Needs the dev store; without one the test says so and passes.

use std::sync::Arc;

use mvs_settings::Settings;
use mvs_sim::setup::WavesVariant;
use mvs_ui::Bundle;
use mvs_ui::bindings::Bindings;
use mvs_ui::element::ElementList;
use mvs_ui::menu::controller::MenuRun;
use mvs_ui::menu::input::Pointer;
use mvs_ui::menu::screens::pre_match::{self, setup::*};
use mvs_ui::menu::{MenuScreen, MenuView, NoSound};
use mvs_ui::slate::Vis;
use mvs_ui::text::{Fonts, GameText};

#[test]
fn a_solo_screen_has_one_side_and_the_modes_title() {
    let store = match mvs_store::Store::open(mvs_store::dev_store_dir()) {
        Ok(s) if s.version("font").is_some() && s.version("ui").is_some() => s,
        _ => {
            eprintln!("no dev store with the font and ui kinds: skipped");
            return;
        }
    };
    let bundle = Arc::new(Bundle::load(&store, "menu").expect("the menu bundle"));
    let mut fonts = Fonts::new(&store, &bundle).expect("the menu fonts");
    let text = Arc::new(GameText::load(&store).unwrap_or_default());
    let (mut prefs, mut bindings, mut sink, mut pointer) =
        (Settings::default(), Bindings::default(), NoSound, Pointer::default());
    // a badge equipped, which the solo plate does not show
    prefs.hydra.stat_trackers[0] = mvs_settings::AssetRef {
        slug: "stat_tracking_tt2_games_played".into(),
        path: "/Game/Panda_Main/Blueprints/Rewards/StatTrackingBundle/StatTracking_TT2_GamesPlayed.StatTracking_TT2_GamesPlayed".into(),
    };
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
    let setup = mvs_ui::menu::waves::pre_match_setup("me", WavesVariant::RingOuts100, None, String::new());
    let title = setup.solo.as_ref().map(|m| m.title.clone()).expect("a solo set-up");
    let pm = pre_match::show(&mut s, &mut run, PreMatchStart { setup, lobby: None });
    pm.set_level(&mut s, false);
    let mut t = 1.0;
    // past the 45 s select (the timer locks the pick), the found players' 2 s and into the perks
    for _ in 0..3200 {
        s.frame(&mut run, &[], t, &mut ElementList::default());
        t += 1.0 / 60.0;
    }
    let tree = s.view.tree();
    let si = tree.find_in(pm.widget, "UI_PM_StateInfo");
    let mode = tree.widget(tree.find_in(si, "ModeLabel")).text.clone();
    assert!(mode.contains(&title), "the mode label {mode:?} names {title:?}");
    let vis = |name: &str| tree.widget(tree.find_in(pm.widget, name)).vis;
    assert_eq!(vis("UI_PM_PlayerInfo_1"), Vis::Collapsed, "the opponent's plate");
    assert_eq!(vis("UI_Perks_LoadoutP1"), Vis::Collapsed, "the opponent's card");
    assert_ne!(vis("UI_Perks_LoadoutP0"), Vis::Collapsed, "the player's own card");
    let plate = tree.find_in(pm.widget, "UI_PM_PlayerInfo_0");
    for b in ["StatBadge", "StatBadge2", "StatBadge3"] {
        assert_eq!(tree.widget(tree.find_in(plate, b)).vis, Vis::Collapsed, "the plate's {b}");
    }
}
