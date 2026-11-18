//! The online lobby's menu tapes: the oracle's lobby-ready and lobby-code scripts played
//! on the custom game page, a fake `OnlineView` standing for the page's net client. The oracle patched a local lobby by
//! hand (`lobbySet`, `lobbyCode`); here the page runs an online lobby fed the server's messages that give the same
//! rules. The tape is written when `MVS_LOBBY_TAPE_OUT` names a folder, to compare with `menu-tape-diff --path
//! CustomGame`. Needs the dev store; without one each test says so and passes.

use std::path::PathBuf;
use std::sync::Arc;

use mvs_protocol::lobby_rules::{LobbyData, LobbyPlayer, LobbyRules};
use mvs_protocol::messages::ServerMessage;
use mvs_settings::Settings;
use mvs_ui::Bundle;
use mvs_ui::bindings::Bindings;
use mvs_ui::element::ElementList;
use mvs_ui::menu::controller::MenuRun;
use mvs_ui::menu::input::Pointer;
use mvs_ui::menu::online::{BrowseAnswer, BrowseNote, OnlineEvent, OnlineView, SessionView};
use mvs_ui::menu::tape::{MenuHeader, MenuTape};
use mvs_ui::menu::widgets::custom_game::CustomGame;
use mvs_ui::menu::{Manager, MenuInput, MenuScreen, MenuView, NoSound};
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

const SIZE: [u32; 2] = [1920, 1080];

fn out_dir() -> Option<PathBuf> {
    std::env::var_os("MVS_LOBBY_TAPE_OUT").map(PathBuf::from)
}

fn player(id: &str, name: &str, bot: bool, character: &str, difficulty: i32) -> LobbyPlayer {
    LobbyPlayer {
        account_id: id.into(),
        username: name.into(),
        is_bot: bot,
        character: character.into(),
        skin: String::new(),
        difficulty,
        icon: String::new(),
    }
}

/// lobbySet: {leader, humans [[id, name, team]], bot (team or -1), ready []}; "me" is this player.
fn lobby_set(leader: &str, humans: &[(&str, &str, usize)], bot: i32, ready: &[&str]) -> LobbyData {
    let id = |a: &str| if a == "me" { "local".to_string() } else { a.to_string() };
    let mut teams = vec![vec![player("local", "Tester", false, "shaggy", 0)], Vec::new()];
    for (account, name, team) in humans {
        teams[*team].push(player(account, name, false, "wonderwoman", 0));
    }
    if bot >= 0 {
        teams[bot as usize].push(player("bot1", "Bot", true, "irongiant", 1));
    }
    // (the rules a new lobby starts with, as the oracle's local lobby had them)
    LobbyData { leader_id: id(leader), teams, ready: ready.iter().map(|r| id(r)).collect(), ..LobbyRules::new().data() }
}

fn state(ev: &str, data: LobbyData) -> OnlineEvent {
    OnlineEvent::Server(Box::new(ServerMessage::LobbyState { id: "L1".into(), ev: ev.into(), data, who: None }))
}

fn session() -> SessionView {
    SessionView { code: String::new(), me: "local".into(), host: false, chosen: true, lobby: None, queued: false }
}

/// A frame of the script: `online` the view the page's client gives this frame.
struct Play {
    env: Env,
    screen: MenuScreen,
    tape: MenuTape,
    out: ElementList,
}

impl Play {
    fn new(env: Option<Env>, script: &str) -> Option<Play> {
        let mut env = env?;
        let view = MenuView::new(env.bundle.clone(), env.text.clone(), SIZE);
        let mut screen = MenuScreen::new(view, &mut env.run());
        // the tape's root 0 is the main menu under the page: a bare layer holds its place
        screen.with_cx(&mut env.run(), |cx| {
            let bundle = cx.view.bundle.clone();
            let w = cx.view.tree_mut().create_plain(&bundle, "CanvasPanel", "UI_MainMenu");
            MenuView::add_to_viewport(cx, w, 0);
        });
        let header =
            MenuHeader { tape: "menu".into(), version: 1, script: script.into(), start: "menu".into(), size: SIZE };
        Some(Play { env, screen, tape: MenuTape { header, frames: Vec::new() }, out: ElementList::default() })
    }

    fn show(&mut self, cls: &str, id: &str) {
        let (s, e) = (&mut self.screen, &mut self.env);
        s.with_cx(&mut e.run(), |cx| Manager::show_modal(cx, cls, id, "", "")).expect("modal shown");
    }

    fn dismiss(&mut self, id: &str) {
        let (s, e) = (&mut self.screen, &mut self.env);
        s.with_cx(&mut e.run(), |cx| Manager::dismiss_modal(cx, id));
    }

    fn on_game<R>(&mut self, f: impl FnOnce(&mut CustomGame, &mut mvs_ui::menu::controller::Cx<'_>) -> R) -> R {
        let (s, e) = (&mut self.screen, &mut self.env);
        s.with_cx(&mut e.run(), |cx| {
            let w = cx.view.manager.get_modal("customGame").expect("the custom game is up");
            cx.visit::<CustomGame, _>(w, f).expect("a CustomGame")
        })
    }

    /// The frame `n` at the script's clock, the page's net view given first.
    fn frame(&mut self, n: u32, online: OnlineView, input: &[MenuInput]) {
        self.screen.view.session.online = online;
        let now = f64::from(n) / 60.0;
        self.screen.frame(&mut self.env.run(), input, now, &mut self.out);
        self.tape.frames.push(self.screen.tape_frame());
    }

    fn finish(self, name: &str) {
        if let Some(d) = out_dir() {
            self.tape.write(&d.join(name)).unwrap();
        }
    }
}

/// The page's view of a signed-in client with a session open.
fn view(events: Vec<OnlineEvent>, open: bool) -> OnlineView {
    OnlineView {
        connected: true,
        me: "local".into(),
        name: "Tester".into(),
        session: open.then(session),
        events,
        ..OnlineView::default()
    }
}

#[test]
fn lobby_ready_script() {
    let Some(mut p) = Play::new(env(), "lobby-ready") else { return };
    if out_dir().is_none() {
        eprintln!("MVS_LOBBY_TAPE_OUT not set: nothing written");
    }
    let sets: [(u32, LobbyData); 4] = [
        (270, lobby_set("velma", &[("velma", "Velma", 0), ("fred", "Fred", 1)], 1, &["velma"])),
        (378, lobby_set("me", &[("velma", "Velma", 0), ("fred", "Fred", 1)], 1, &["me", "velma"])),
        (432, lobby_set("me", &[("velma", "Velma", 0), ("fred", "Fred", 1)], 1, &["me", "velma", "fred"])),
        (
            486,
            lobby_set(
                "me",
                &[("velma", "Velma_With_A_Rather_Long_Name_1234", 0), ("fred", "WWWWWWWWWWWWWWWWWWWWWWWW", 1)],
                -1,
                &["me", "velma"],
            ),
        ),
    ];
    let mut open = false;
    for n in 0..=558u32 {
        let mut events = Vec::new();
        // (the modal's layer is up in the tape one frame after its show: the oracle's do runs inside the frame)
        if n == 29 {
            p.show("WBP_CustomGame", "customGame");
        }
        // the session opens and answers with the lobby (lobby.enter): the first frame after the page is up
        if n == 29 {
            events.push(OnlineEvent::Opened);
            open = true;
        }
        if n == 29 {
            events.push(state("started", lobby_set("me", &[], -1, &["me"])));
        }
        // (the oracle's hook runs before its frame's tick; the page's view is given before the frame, so one frame early)
        if let Some((_, d)) = sets.iter().find(|(f, _)| *f == n) {
            events.push(state("teams", d.clone()));
        }
        // startClicked (f324): the READY button; the server's answer comes with the next frame
        if n == 324 {
            p.on_game(|g, cx| g.start_clicked(cx));
        }
        if n == 325 {
            events.push(state(
                "ready",
                lobby_set("velma", &[("velma", "Velma", 0), ("fred", "Fred", 1)], 1, &["velma", "me"]),
            ));
        }
        p.frame(n, view(events, open), &[]);
    }
    p.finish("lobby-ready.native.menutape.zst");
}

#[test]
fn lobby_code_script() {
    let Some(mut p) = Play::new(env(), "lobby-code") else { return };
    let code = "0f3a9c1e5b7d42a6b8e1c3d5f7a9b2c4";
    let mouse = |x: f64, y: f64| {
        [
            vec![MenuInput::MouseMove { x, y }],
            vec![MenuInput::MouseButton { button: 0, down: true }],
            vec![MenuInput::MouseButton { button: 0, down: false }],
        ]
    };
    let click = mouse(1571.0, 59.0 + 12.0);
    for n in 0..=1038u32 {
        // the join card opens the custom game (a local lobby: no client to join through); the oracle's identity
        // is named Tester
        if n == 253 {
            p.show("WBP_CustomGame", "customGame");
        }
        // lobbyCode: the code button shown with the code as its label
        if n == 720 {
            p.on_game(|g, cx| g.force_code(cx, code));
        }
        if n == 827 {
            p.dismiss("customGame");
        }
        let input: &[MenuInput] = match n {
            780 => &click[0],
            782 => &click[1],
            784 => &click[2],
            _ => &[],
        };
        p.frame(n, OnlineView { name: "Tester".into(), ..OnlineView::default() }, input);
    }
    p.finish("lobby-code.native.menutape.zst");
}

#[test]
fn browser_lists_the_answer() {
    use mvs_protocol::lobby_listing::{ListingState, LobbyListing};
    use mvs_ui::menu::online::BrowsedGame;
    use mvs_ui::menu::widgets::custom_browser::CustomGameBrowser;
    let Some(mut p) = Play::new(env(), "browser") else { return };
    let listing = |name: &str, humans: u32, state: ListingState| LobbyListing {
        v: 1,
        name: name.into(),
        icon: String::new(),
        compat: "c1".into(),
        app: "1.0".into(),
        style: 0,
        maps: Vec::new(),
        map_count: 0,
        players: humans,
        humans,
        slots: 2,
        state,
        time: 0,
        ringouts: 0,
        hazards: false,
    };
    let game =
        |code: &str, l: LobbyListing| BrowsedGame { code: code.into(), backend: None, members: 1, max: 4, listing: l };
    let answer = BrowseAnswer {
        games: vec![
            game("full", listing("Full", 2, ListingState::Lobby)),
            game("b", listing("Bob", 1, ListingState::Lobby)),
            game("a", listing("Al", 1, ListingState::Lobby)),
        ],
        why: BrowseNote::NoApp,
    };
    let (s, e) = (&mut p.screen, &mut p.env);
    s.with_cx(&mut e.run(), |cx| Manager::show_modal(cx, "CustomGameBrowser", "customGames", "", "")).expect("shown");
    let base = |browse| OnlineView { compat: "c1".into(), connected: true, browse, ..OnlineView::default() };
    for n in 0..20 {
        p.frame(n, base(None), &[]);
    }
    for n in 20..24 {
        p.frame(n, base(Some(answer.clone())), &[]);
    }
    let (s, e) = (&mut p.screen, &mut p.env);
    let rows = s.with_cx(&mut e.run(), |cx| {
        let w = cx.view.manager.get_modal("customGames").expect("up");
        cx.visit::<CustomGameBrowser, _>(w, |b, _| b.rows()).expect("a browser")
    });
    assert_eq!(rows, vec![("a".to_string(), true), ("b".to_string(), true), ("full".to_string(), false)]);
}

/// The lobby-code script up to the join (f0..f500): the PLAY screen's Join Lobby card, the browser with no app, the
/// room code box, the typed code and GO. The oracle's hooks: showModal f30, joinCardEnable f192 (the card enabled:
/// the page's client can join), joinSubmit f480 (GO).
#[test]
fn lobby_code_join_flow() {
    let Some(mut p) = Play::new(env(), "lobby-code") else { return };
    let click = |x: f64, y: f64| {
        [
            vec![MenuInput::MouseMove { x, y }],
            vec![MenuInput::MouseButton { button: 0, down: true }],
            vec![MenuInput::MouseButton { button: 0, down: false }],
        ]
    };
    let clicks = [(345u32, click(1302.0, 71.0)), (420, click(923.0, 576.0))];
    let code = "0f3a9c1e5b7d42a6b8e1c3d5f7a9b2c4";
    let typed: Vec<MenuInput> = code.chars().map(MenuInput::Char).collect();
    for n in 0..=500u32 {
        // (the PLAY screen's Join Lobby card, f252, opens the browser; its enabling follows online.can_join in the
        // match options track's code, so the browser is shown directly)
        if n == 256 {
            p.show("CustomGameBrowser", "customGames");
        }
        let mut input: Vec<MenuInput> = Vec::new();
        for (f, c) in &clicks {
            for (i, step) in c.iter().enumerate() {
                if n == f + 2 * i as u32 {
                    input.extend(step.iter().cloned());
                }
            }
        }
        if n == 438 {
            input.extend(typed.iter().cloned());
        }
        // joinSubmit: the box's GO
        if n == 479 {
            let (sc, e) = (&mut p.screen, &mut p.env);
            sc.with_cx(&mut e.run(), |cx| {
                let w = cx.view.manager.get_modal("joinLobby").expect("the code box is up");
                cx.visit::<mvs_ui::menu::widgets::dialogs::SetUsername, _>(w, |b, cx| b.press_go(cx));
            });
        }
        // (the net process's answer: no app; the oracle's came after its real-time wait, about f310)
        let browse = (n >= 310).then(|| BrowseAnswer { games: Vec::new(), why: BrowseNote::NoApp });
        let view = OnlineView { name: "Tester".into(), can_join: n >= 192, browse, ..OnlineView::default() };
        p.frame(n, view, &input);
    }
    p.finish("lobby-code-flow.native.menutape.zst");
}
