//! A toast of another player (PandaGameInstance On Toast Received @141815 -> Send Toast
//! Notification): the frame's online event shows the toast notification with the toaster's row and TOAST BACK, and
//! none when this player toasted the toaster within the ToastedRecently window. Needs the dev store; without one the
//! test says so and passes.

use std::sync::Arc;

use mvs_settings::Settings;
use mvs_ui::Bundle;
use mvs_ui::bindings::Bindings;
use mvs_ui::menu::controller::MenuRun;
use mvs_ui::menu::input::Pointer;
use mvs_ui::menu::online::OnlineEvent;
use mvs_ui::menu::toasts::Toasts;
use mvs_ui::menu::{MenuScreen, MenuView, NoSound};
use mvs_ui::slate::Vis;
use mvs_ui::text::{Fonts, GameText};

#[test]
fn a_toast_shows_its_toaster_and_toast_back_once() {
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

    // the toast message of p2 in the frame's events (the results screen's or the menus'): a notification
    screen.view.session.online.events = vec![OnlineEvent::Toast { from: "p2".into(), name: "Two".into() }];
    screen.view.set_now(1.0 / 60.0);
    screen.with_cx(&mut run, MenuView::tick);
    screen.view.session.online.events.clear();
    let shown = screen.view.manager.notifications_shown().to_vec();
    assert_eq!(shown.len(), 1, "the toast notification");
    let n = shown[0];
    let tree = screen.view.tree();
    let row = tree.find_opt(n, "UI_SocialPlayer").expect("the notification's player row");
    assert_eq!(tree.widget(row).vis, Vis::Visible);
    assert_eq!(tree.widget(tree.find_in(row, "username")).text, "Two");
    assert_eq!(tree.widget(tree.find_in(n, "buttonAction1")).vis, Vis::Visible, "TOAST BACK");

    // p3 toasted first, then p3's toast: no TOAST BACK (the toaster in ToastedRecently)
    screen.with_cx(&mut run, |cx| Toasts::toast(cx, "p3", true));
    let n = screen.with_cx(&mut run, |cx| Toasts::received(cx, 4, "p3", "Three")).expect("the second notification");
    let tree = screen.view.tree();
    assert_eq!(tree.widget(tree.find_in(n, "buttonAction1")).vis, Vis::Collapsed, "no TOAST BACK");
}
