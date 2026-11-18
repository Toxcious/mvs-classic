//! A queue set's NEXT on the end-of-game screen: PLAY AGAIN's
//! transition fades with no level, and UI_EOG_PullToPreMatch's Start opens the PreMatch level through the manager
//! (manager.openLevelEvent), the level the screen leaves for. Needs the dev store; without one the test says so and
//! passes.

use std::sync::Arc;

use mvs_settings::Settings;
use mvs_ui::Bundle;
use mvs_ui::bindings::Bindings;
use mvs_ui::menu::controller::MenuRun;
use mvs_ui::menu::input::Pointer;
use mvs_ui::menu::widgets::end_of_game::{EOGPlayAgain, EOGPullToPreMatch};
use mvs_ui::menu::{MenuScreen, MenuView, NoSound};
use mvs_ui::text::{Fonts, GameText};

#[test]
fn next_leaves_for_the_pre_match_level() {
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
    let mut screen = MenuScreen::new(MenuView::new(bundle, text, [1920, 1080]), &mut run);

    // PLAY AGAIN: the fade to black, no level; the screen stays until the pull
    screen.with_cx(&mut run, |cx| {
        let w = MenuView::create(cx, "UI_EOG_PlayAgain");
        cx.visit::<EOGPlayAgain, _>(w, |c, cx| c.step(cx));
    });
    let mut now = 0.0;
    while now < 1.0 {
        now += 1.0 / 60.0;
        screen.view.set_now(now);
        screen.with_cx(&mut run, MenuView::tick);
    }
    assert!(screen.view.session.eog.play_again);
    assert_eq!(screen.view.manager.take_transition(), None, "PLAY AGAIN's transition has no level");

    // UI_EOG_PullToPreMatch: BPI_OpenLevel(PreMatch)
    screen.with_cx(&mut run, |cx| {
        let w = MenuView::create(cx, "UI_EOG_PullToPreMatch");
        cx.visit::<EOGPullToPreMatch, _>(w, |c, cx| c.start(cx));
    });
    let p = screen.view.manager.take_transition().expect("the pull opens a level");
    assert_eq!((p.next_level.as_str(), p.level_options.as_str()), ("PreMatch", "Lobby"));
}
