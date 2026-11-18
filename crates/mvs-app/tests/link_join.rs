//! A join link's code (the launcher's hand-over file to a running game) and `--online-open join:` end as the join
//! box's GO does: the lobby's pending join and the customGame modal, once the pending commands no longer wait. The
//! hub's half needs nothing; the menus' half needs the dev store (without one it says so and passes).

use std::sync::Arc;

use mvs_app::menus::hand_link_join;
use mvs_app::online::gate::Gate;
use mvs_app::online::{glue, link};
use mvs_net::service::api::SessionTarget;
use mvs_settings::Settings;
use mvs_settings::stats::StatsFile;
use mvs_ui::Bundle;
use mvs_ui::bindings::Bindings;
use mvs_ui::element::ElementList;
use mvs_ui::menu::controller::MenuRun;
use mvs_ui::menu::input::Pointer;
use mvs_ui::menu::online::{OnlineCmd, OnlineEvent, OnlineNotice};
use mvs_ui::menu::screens::main_menu;
use mvs_ui::menu::session::Session;
use mvs_ui::menu::{MenuScreen, MenuView, NoSound};
use mvs_ui::text::{Fonts, GameText};

/// The menu bundle's parts a screen runs on.
struct Env {
    bundle: Arc<Bundle>,
    text: Arc<GameText>,
    fonts: Fonts,
    prefs: Settings,
    bindings: Bindings,
    sink: NoSound,
    pointer: Pointer,
}

impl Env {
    fn load() -> Option<Env> {
        let store = match mvs_store::Store::open(mvs_store::dev_store_dir()) {
            Ok(s) if s.version("font").is_some() && s.version("ui").is_some() => s,
            _ => {
                eprintln!("no dev store with the font and ui kinds: the menus' half skipped");
                return None;
            }
        };
        let bundle = match Bundle::load(&store, "menu") {
            Ok(b) => b,
            Err(e) => {
                eprintln!("no menu bundle ({e}): the menus' half skipped");
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

/// The main menu over a session holding `code` as its link join; the online commands of its first frames.
fn menus_open(env: &mut Env, code: &str) -> (bool, Vec<OnlineCmd>) {
    let mut view = MenuView::new(env.bundle.clone(), env.text.clone(), [1600, 900]);
    view.session = Session::new(StatsFile::default());
    view.session.online.can_join = true;
    view.session.main_menu.link_join = Some(code.to_owned());
    let mut screen = MenuScreen::new(view, &mut env.run());
    main_menu::show(&mut screen, &mut env.run());
    let mut cmds = Vec::new();
    let mut out = ElementList::default();
    let mut t = 1.0;
    for _ in 0..30 {
        screen.frame(&mut env.run(), &[], t, &mut out);
        cmds.extend(screen.view.take_online_cmds());
        t += 1.0 / 60.0;
    }
    assert!(screen.view.session.main_menu.link_join.is_none(), "the main menu took the link's code");
    (screen.view.manager.get_modal("customGame").is_some(), cmds)
}

#[test]
fn a_join_link_and_online_open_join_open_the_lobby_screen() {
    let data = std::env::temp_dir().join(format!("mvs-link-join-{}", std::process::id()));
    let _ = std::fs::create_dir_all(&data);
    let instance = link::hold_instance(&data).expect("the data folder's instance");
    let hub = glue::start_offline(&data);
    let mut h = hub.borrow_mut();
    h.script_clock = Some(1000.0);
    let mut session = Session::new(StatsFile::default());

    // a link handed over while the gate holds online play: kept, with the notice that it waits
    h.set_gate(Some(Gate::Checking));
    link::hand_over(&data, "abc234").unwrap();
    link::poll(&mut h, &data);
    assert_eq!(h.link_join.as_deref(), Some("abc234"));
    assert!(h.pending.is_empty(), "no session opened without the lobby screen");
    let notes = h.frame(glue::prefs_of(&Settings::default()), 1000.0).events;
    assert!(
        notes.iter().any(|e| matches!(e, OnlineEvent::Notice(OnlineNotice::Host(t))
            if t == mvs_ui::menu::custom_text::LINK_HELD)),
        "the link-held notice: {notes:?}"
    );
    let held = h.pending_held(1000.0);
    assert!(held);
    hand_link_join(&mut h, held, &mut session);
    assert!(session.main_menu.link_join.is_none(), "held while the gate is set");

    // `--online-open join:` (the launcher's start of a closed game): the later code wins
    h.push_open(OnlineCmd::Open(SessionTarget::Join("xyz789".into())));
    h.push_open(OnlineCmd::Open(SessionTarget::Host));
    assert_eq!(h.link_join.as_deref(), Some("xyz789"));
    assert!(matches!(h.pending[..], [OnlineCmd::Open(SessionTarget::Host)]), "a host open stays a command");

    // the gate cleared (and past any sign-in wait): the code goes to the main menu
    h.set_gate(None);
    let late = 1000.0 + mvs_app::online::hub::PENDING_WAIT_MS + 1.0;
    let held = h.pending_held(late);
    assert!(!held);
    hand_link_join(&mut h, held, &mut session);
    assert_eq!(session.main_menu.link_join.as_deref(), Some("xyz789"));
    assert!(h.link_join.is_none());
    drop(h);
    drop(instance);
    let _ = std::fs::remove_dir_all(&data);

    // the main menu opens the lobby screen on it, whose net lobby joins the code
    let Some(mut env) = Env::load() else { return };
    let (shown, cmds) = menus_open(&mut env, "xyz789");
    assert!(shown, "the customGame modal is up");
    assert!(
        cmds.iter().any(|c| matches!(c, OnlineCmd::Open(SessionTarget::Join(c)) if c == "xyz789")),
        "the lobby screen's join of the code: {cmds:?}"
    );
}
