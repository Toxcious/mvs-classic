//! The custom lobby driven by the mouse and the keys as a player does: a click in the chat's box gives it the keys,
//! the chat's hint goes while the box has them or holds text, and each of the lobby's modals opens after another one
//! closed. Needs the dev store; without one each test says so and passes.

use std::sync::Arc;

use mvs_protocol::lobby_rules::{LobbyData, LobbyPlayer, LobbyRules};
use mvs_protocol::messages::{ClientMessage, ServerMessage};
use mvs_protocol::online::{PmBegin, PmKind};
use mvs_settings::Settings;
use mvs_ui::Bundle;
use mvs_ui::bindings::Bindings;
use mvs_ui::element::ElementList;
use mvs_ui::menu::controller::MenuRun;
use mvs_ui::menu::input::Pointer;
use mvs_ui::menu::online::{
    LocalReason, OfflineWhy, OnlineCmd, OnlineEvent, OnlineLine, OnlineView, SessionLink, SessionView,
};
use mvs_ui::menu::widgets::controls::drop_menu::DropMenu;
use mvs_ui::menu::{Manager, MenuInput, MenuScreen, MenuView, NoSound};
use mvs_ui::slate::{Vis, WidgetId};
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

fn me() -> LobbyPlayer {
    LobbyPlayer {
        account_id: "local".into(),
        username: "Tester".into(),
        is_bot: false,
        character: "shaggy".into(),
        skin: String::new(),
        difficulty: 0,
        icon: String::new(),
    }
}

/// The lobby a new session answers with: this player alone, its leader.
fn started() -> OnlineEvent {
    let data = LobbyData { leader_id: "local".into(), teams: vec![vec![me()], Vec::new()], ..LobbyRules::new().data() };
    OnlineEvent::Server(Box::new(ServerMessage::LobbyState { id: "L1".into(), ev: "started".into(), data, who: None }))
}

fn online(events: Vec<OnlineEvent>) -> OnlineView {
    let session = SessionView {
        code: "L1".into(),
        me: "local".into(),
        host: false,
        chosen: true,
        lobby: None,
        queued: false,
        link: Some(SessionLink::Relay),
    };
    OnlineView {
        connected: true,
        can_join: true,
        me: "local".into(),
        name: "Tester".into(),
        session: Some(session),
        events,
        ..OnlineView::default()
    }
}

struct Lobby {
    env: Env,
    screen: MenuScreen,
    out: ElementList,
    /// The frames of the menu's clock.
    n: u32,
    /// Online play's clock (`OnlineView::now`), ms: it runs with the frames and on when the menu's clock starts again.
    online_ms: f64,
}

impl Lobby {
    /// The custom game over an online lobby (so it has its chat), a second of frames after it is up.
    fn new(env: Option<Env>) -> Option<Lobby> {
        Some(Lobby::open(env?, 0.0, vec![OnlineEvent::Opened, started()]))
    }

    /// The custom game shown on a new menu (its clock starts) with online play's clock at `online_ms`, a second of
    /// frames after it is up; `events`: the session's first.
    fn open(mut env: Env, online_ms: f64, events: Vec<OnlineEvent>) -> Lobby {
        let view = MenuView::new(env.bundle.clone(), env.text.clone(), SIZE);
        let mut screen = MenuScreen::new(view, &mut env.run());
        screen.view.session.online = online(Vec::new());
        screen.with_cx(&mut env.run(), |cx| Manager::show_modal(cx, "WBP_CustomGame", "customGame", "", ""));
        let mut l = Lobby { env, screen, out: ElementList::default(), n: 0, online_ms };
        l.screen.view.session.online = online(events);
        l.frames(60, &[]);
        l
    }

    fn frames(&mut self, count: u32, input: &[MenuInput]) {
        for i in 0..count {
            self.n += 1;
            let now = 1.0 + f64::from(self.n) / 60.0;
            let input = if i == 0 { input } else { &[] };
            self.online_ms += 1000.0 / 60.0;
            self.screen.view.session.online.now = self.online_ms;
            self.screen.frame(&mut self.env.run(), input, now, &mut self.out);
            // (the server's messages come once)
            self.screen.view.session.online.events.clear();
        }
    }

    /// The widget `name` under the modal `id`.
    fn find(&self, id: &str, name: &str) -> WidgetId {
        let v = &self.screen.view;
        let m = v.manager.get_modal(id).unwrap_or_else(|| panic!("the modal {id} is up"));
        v.tree().find_in(m, name)
    }

    /// The centre of `w` as last painted, in device pixels.
    fn centre(&self, w: WidgetId) -> (f64, f64) {
        let p = self.screen.view.tree().painted(w).expect("painted");
        let (x, y) = (p.size[0] / 2.0, p.size[1] / 2.0);
        let m = p.transform;
        (m[0] * x + m[2] * y + m[4], m[1] * x + m[3] * y + m[5])
    }

    /// A left click at the centre of `w`: a move, the button down, the button up, a few frames apart.
    fn click(&mut self, w: WidgetId) {
        let (x, y) = self.centre(w);
        self.frames(3, &[MenuInput::MouseMove { x, y }]);
        self.frames(3, &[MenuInput::MouseButton { button: 0, down: true }]);
        self.frames(10, &[MenuInput::MouseButton { button: 0, down: false }]);
    }

    fn key(&mut self, key: &'static str) {
        self.frames(2, &[MenuInput::Key { key, down: true, repeat: false, player: 0 }]);
        self.frames(10, &[MenuInput::Key { key, down: false, repeat: false, player: 0 }]);
    }

    fn up(&self, id: &str) -> bool {
        self.screen.view.manager.get_modal(id).is_some()
    }

    fn chat_box(&self) -> WidgetId {
        let input = self.screen.view.tree().find_in(self.find("customGame", "LobbyChatInputRow"), "UI_TextInput");
        self.screen.view.tree().find_in(input, "TextBox")
    }

    fn typing(&self) -> bool {
        self.screen.view.has_keyboard_focus(self.chat_box())
    }

    /// A line typed in the chat's box and committed with Enter.
    fn say(&mut self, line: &str) {
        if !self.typing() {
            self.key("Enter");
        }
        self.frames(2, &line.chars().map(MenuInput::Char).collect::<Vec<_>>());
        self.key("Enter");
    }

    /// The chat lines the page sent since the last call.
    fn chat_sent(&mut self) -> Vec<String> {
        let sent = self.screen.view.take_online_cmds().into_iter().filter_map(|c| match c {
            OnlineCmd::Send(m) => match *m {
                ClientMessage::Chat { text, .. } => Some(text),
                _ => None,
            },
            _ => None,
        });
        sent.collect()
    }

    fn hint_shown(&self) -> bool {
        self.screen.view.tree().widget(self.find("customGame", "LobbyChatHint")).vis != Vis::Collapsed
    }
}

#[test]
fn a_click_in_the_chat_box_gives_it_the_keys() {
    let Some(mut l) = Lobby::new(env()) else { return };
    assert!(!l.typing(), "the chat box starts without the keys");
    let b = l.chat_box();
    l.click(b);
    assert!(l.typing(), "the clicked chat box has the keys");
    assert!(!l.hint_shown(), "the hint goes with the click");
    l.frames(5, &[MenuInput::Char('h'), MenuInput::Char('i')]);
    let tb = l.chat_box();
    let text = l.screen.view.tree().widget(tb).children[0];
    assert_eq!(l.screen.view.tree().widget(text).text, "hi", "the keys typed go in the clicked box");
}

#[test]
fn the_chat_hint_shows_only_on_an_empty_box_without_the_keys() {
    let Some(mut l) = Lobby::new(env()) else { return };
    assert!(l.hint_shown(), "the hint is up at first");
    l.key("Enter");
    assert!(l.typing(), "Enter opens the chat");
    assert!(!l.hint_shown(), "the hint goes while the box has the keys");
    l.frames(1, &[MenuInput::Char('h'), MenuInput::Char('i')]);
    l.frames(5, &[]);
    l.key("Escape");
    assert!(!l.typing(), "Escape leaves the box");
    assert!(!l.hint_shown(), "the hint stays off over the text");
    l.key("Enter");
    l.frames(1, &[MenuInput::Key { key: "BackSpace", down: true, repeat: false, player: 0 }]);
    l.frames(1, &[MenuInput::Key { key: "BackSpace", down: false, repeat: false, player: 0 }]);
    l.frames(1, &[MenuInput::Key { key: "BackSpace", down: true, repeat: false, player: 0 }]);
    l.frames(5, &[MenuInput::Key { key: "BackSpace", down: false, repeat: false, player: 0 }]);
    l.key("Escape");
    assert!(!l.typing(), "Escape leaves the box");
    assert!(l.hint_shown(), "the hint is back on the empty box without the keys");
}

#[test]
fn a_click_on_the_social_tab_opens_the_panel() {
    let Some(mut l) = Lobby::new(env()) else { return };
    let panel = l.find("customGame", "UI_SocialPanel_CustomGame");
    let (tab, small) = {
        let t = l.screen.view.tree();
        (t.find_in(panel, "hoverBox"), t.find_in(panel, "headerSmall"))
    };
    assert_ne!(l.screen.view.tree().widget(small).vis, Vis::Collapsed, "the panel starts small");
    l.click(tab);
    // (the page over the panel took this click while the slot's ZOrder 1 was not kept, and the panel had no context)
    assert_eq!(l.screen.view.tree().widget(small).vis, Vis::Collapsed, "the clicked tab opens the panel");
}

/// The painted box of `w` as (top, bottom), in device pixels.
fn top_bottom(l: &Lobby, w: WidgetId) -> Option<(f64, f64)> {
    let p = l.screen.view.tree().painted(w)?;
    let m = p.transform;
    let (a, b) = (m[5], m[3] * p.size[1] + m[5]);
    Some((a.min(b), a.max(b)))
}

#[test]
fn the_experimental_settings_lists_stay_on_screen() {
    let Some(mut l) = Lobby::new(env()) else { return };
    let b = l.find("customGame", "ResetToDefaultsButton");
    l.click(b);
    assert!(l.up("customGameExperimental"));
    let rows: Vec<WidgetId> = {
        let t = l.screen.view.tree();
        let like = l.find("customGameExperimental", "UI_DropMenu_TeamStyle");
        let parent = t.widget(like).parent.expect("the rows' box");
        t.widget(parent)
            .children
            .iter()
            .copied()
            .filter(|&c| t.user_class(c) == Some("UI_DropMenuRework") && t.widget(c).vis != Vis::Collapsed)
            .collect()
    };
    assert!(!rows.is_empty(), "the page has its rows");
    for (k, row) in rows.into_iter().enumerate() {
        let open = l.screen.view.tree().find_in(row, "mainInteractor");
        l.click(open);
        l.frames(30, &[]);
        let t = l.screen.view.tree();
        let menus: Vec<WidgetId> = t
            .ids()
            .filter(|&w| t.user_class(w) == Some("UI_ContextMenu") && t.widget(w).parent.is_some())
            .map(|w| t.find_in(w, "menu"))
            .collect();
        assert_eq!(menus.len(), 1, "row {k} opens its list");
        let (top, bottom) = top_bottom(&l, menus[0]).expect("the list is painted");
        assert!(top >= 0.0 && bottom <= f64::from(SIZE[1]), "row {k}'s list spans {top}..{bottom}");
        l.key("Escape");
        l.frames(20, &[]);
    }
}

#[test]
fn each_lobby_modal_opens_after_another_closed() {
    let Some(mut l) = Lobby::new(env()) else { return };
    let close = |l: &mut Lobby, id: &str| {
        let b = l.find(id, "buttonClose");
        l.click(b);
        assert!(!l.up(id), "{id} closed by its button");
    };
    let maps = |l: &mut Lobby| {
        let b = l.find("customGame", "SelectMapsButton");
        l.click(b);
        assert!(l.up("mapSelector"), "Select Maps opens the map selector");
        let b = l.find("mapSelector", "btn_Cancel");
        l.click(b);
        assert!(!l.up("mapSelector"), "the map selector closed by its Cancel");
    };
    // Experimental Settings, then Select Maps
    let b = l.find("customGame", "ResetToDefaultsButton");
    l.click(b);
    assert!(l.up("customGameExperimental"), "the experimental settings open");
    close(&mut l, "customGameExperimental");
    maps(&mut l);
    // Room Settings, then Select Maps, then Experimental Settings again
    let b = l.find("customGame", "LobbySettingsButton");
    l.click(b);
    assert!(l.up("customGameSettings"), "the room settings open");
    close(&mut l, "customGameSettings");
    maps(&mut l);
    let b = l.find("customGame", "ResetToDefaultsButton");
    l.click(b);
    assert!(l.up("customGameExperimental"), "the experimental settings open again");
    close(&mut l, "customGameExperimental");
    let b = l.find("customGame", "LobbySettingsButton");
    l.click(b);
    assert!(l.up("customGameSettings"), "the room settings open again");
}

/// Two profile icons of the catalogue that have a picture and are not the empty icon's stand-in (the gizmo icon
/// SetDefaultProfileIcon replaces): their asset names and their pictures.
fn two_icons(e: &Env) -> Option<[(String, String, String); 2]> {
    let c = mvs_ui::menu::hydra::cosmetics_in(&e.bundle)?;
    let mut it = c.profile_icon.iter().filter_map(|i| {
        let tex = mvs_ui::menu::hydra::picture_name(i.icon.as_deref()?)?;
        (!i.slug.eq_ignore_ascii_case("profile_icon_amb_gre_gizmo_1") && e.bundle.textures.contains_key(&tex))
            .then(|| (i.slug.clone(), i.path.clone(), tex))
    });
    Some([it.next()?, it.nth(3)?])
}

/// The picture of the IconButton in the player row titled `name`.
fn row_icon(l: &Lobby, name: &str) -> Option<String> {
    let t = l.screen.view.tree();
    let m = l.screen.view.manager.get_modal("customGame")?;
    let title = t.post_order(m).into_iter().find(|&w| t.widget(w).name == "PlayerTitle" && t.widget(w).text == name)?;
    let mut w = t.widget(title).parent;
    while let Some(x) = w {
        if let Some(b) = t.find_opt(x, "IconButton") {
            return t.widget(t.find_in(b, "iconImage")).tex.clone();
        }
        w = t.widget(x).parent;
    }
    None
}

#[test]
fn the_lobby_rows_show_the_players_profile_icons() {
    let Some(mut e) = env() else { return };
    let Some([(slug, path, mine), (_, other_path, other)]) = two_icons(&e) else {
        eprintln!("no profile icon catalogue in the menu bundle: skipped");
        return;
    };
    e.prefs.hydra.profile_icon = mvs_settings::AssetRef { slug, path };
    let Some(mut l) = Lobby::new(Some(e)) else { return };
    // another player joins with the icon the server hands on (its asset name)
    let asset = mvs_ui::menu::lobby::loadout::asset_name(&other_path).to_string();
    let velma = LobbyPlayer { account_id: "velma".into(), username: "Velma".into(), icon: asset, ..me() };
    let data =
        LobbyData { leader_id: "local".into(), teams: vec![vec![me()], vec![velma]], ..LobbyRules::new().data() };
    l.screen.view.session.online.events = vec![OnlineEvent::Server(Box::new(ServerMessage::LobbyState {
        id: "L1".into(),
        ev: "teams".into(),
        data,
        who: None,
    }))];
    l.frames(30, &[]);
    assert_eq!(row_icon(&l, "Tester").as_deref(), Some(mine.as_str()), "the own row shows the stored icon");
    assert_eq!(row_icon(&l, "Velma").as_deref(), Some(other.as_str()), "another's row shows the icon it carries");
}

/// The lobby and its chat budget are kept across a match, and the menu after the match has a clock that starts
/// again: the budget is timed by online play's clock, so a line still goes out there.
#[test]
fn the_chat_sends_in_the_lobby_come_back_to_after_a_match() {
    let Some(mut l) = Lobby::new(env()) else { return };
    // ten minutes in the lobby, then a line
    l.n += 60 * 600;
    l.online_ms += 600_000.0;
    l.say("one");
    assert_eq!(l.chat_sent(), ["one"]);
    // the match starts: the page hands the lobby on, kept for the menu after the match
    let pm = PmBegin {
        id: "pm1".into(),
        kind: PmKind::Lobby,
        mode: None,
        team_style: 0,
        map: String::new(),
        players: Vec::new(),
        me: 0,
        rematch: None,
    };
    l.screen.view.session.online.events = vec![OnlineEvent::Server(Box::new(ServerMessage::PmBegin(pm)))];
    l.frames(90, &[]);
    assert!(!l.up("customGame"), "the lobby's page went with the match's start");
    // the menu after a two minute match: a new screen, its clock at the start again
    let Lobby { env, online_ms, .. } = l;
    let mut l = Lobby::open(env, online_ms + 120_000.0, Vec::new());
    l.say("two");
    assert_eq!(l.chat_sent(), ["two"]);
}

/// A line typed past the chat box's width stays in the box: the text is cut to the box and scrolled left so its end
/// (the caret) is in view; the caret is drawn.
#[test]
fn a_long_chat_line_stays_in_its_box() {
    let Some(mut l) = Lobby::new(env()) else { return };
    l.key("Enter");
    assert!(l.typing(), "Enter opens the chat");
    let line = "W".repeat(80);
    l.frames(2, &line.chars().map(MenuInput::Char).collect::<Vec<_>>());
    let tb = l.chat_box();
    let t = l.screen.view.tree();
    let text = t.widget(tb).children[0];
    assert_eq!(t.widget(text).caret, line.chars().count() as i32, "the caret is at the end of the text");
    let boxp = t.painted(tb).expect("the box painted");
    let textp = t.painted(text).expect("the text painted");
    let left = boxp.transform[4];
    let right = left + boxp.size[0] * boxp.transform[0];
    assert!(textp.transform[4] < left, "the text is scrolled left");
    // every element of the text is cut to the box
    let cut: Vec<_> = l
        .out
        .elements
        .iter()
        .filter(|p| matches!(&p.element, mvs_ui::element::Element::Text(e) if e.geometry.transform[4] == textp.transform[4]))
        .collect();
    assert!(!cut.is_empty(), "the text is drawn");
    for p in cut {
        let c = p.clip.expect("a clip");
        assert!(c[0] >= left - 0.5 && c[2] <= right + 0.5, "clip {c:?} in the box {left}..{right}");
    }
    l.key("Escape");
    assert_eq!(l.screen.view.tree().widget(text).caret, -1, "no caret without the keys");
}

/// The lobby's chat drawn to a PNG for a look (`LOBBY_CHAT_SHOT=<file.png> cargo test --test custom_lobby_clicks --
/// --ignored`): another player's line with emoji in the log and a long line with emoji being typed.
#[test]
#[ignore]
fn draw_the_lobby_chat() {
    use mvs_render::gpu::{Gpu, Subresource, TexDesc, TexKind, TexViews};
    use windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT_R8G8B8A8_UNORM;
    let Some(mut l) = Lobby::new(env()) else { return };
    let out = std::env::var("LOBBY_CHAT_SHOT").unwrap_or_else(|_| "lobby_chat.png".into());
    let velma = LobbyPlayer { account_id: "velma".into(), username: "Velma".into(), ..me() };
    let data =
        LobbyData { leader_id: "local".into(), teams: vec![vec![me()], vec![velma]], ..LobbyRules::new().data() };
    l.screen.view.session.online.events = vec![
        OnlineEvent::Server(Box::new(ServerMessage::LobbyState {
            id: "L1".into(),
            ev: "teams".into(),
            data,
            who: None,
        })),
        OnlineEvent::Server(Box::new(ServerMessage::Chat {
            from: "velma".into(),
            to: mvs_protocol::messages::ChatTo::Lobby,
            text: "gg \u{1F600}\u{1F525} one more? \u{1F44D}\u{1F3FD} \u{2764}\u{FE0F} \u{1F1FA}\u{1F1F8}".into(),
        })),
    ];
    l.frames(30, &[]);
    l.key("Enter");
    let typed: Vec<MenuInput> =
        "SURE \u{1F602} BUT THIS TIME I AM PICKING A FIGHTER YOU HAVE NEVER SEEN ME PLAY BEFORE \u{1F60E}"
            .chars()
            .map(MenuInput::Char)
            .collect();
    l.frames(2, &typed);
    l.frames(1, &[]);
    let gpu = Gpu::new(None, false).unwrap();
    let store = mvs_store::Store::open(mvs_store::dev_store_dir()).unwrap();
    let assets = mvs_render::Assets::open(store, &gpu).unwrap();
    let [w, h] = SIZE;
    let black = [0u8, 0, 0, 0xff].repeat((w * h) as usize);
    let target = gpu
        .texture(
            &TexDesc {
                kind: TexKind::D2,
                size: [w, h, 1],
                mips: 1,
                format: DXGI_FORMAT_R8G8B8A8_UNORM,
                views: TexViews { srv: true, rtv: true, ..Default::default() },
            },
            &[Subresource { data: &black, row_pitch: w * 4, slice_pitch: 0 }],
        )
        .unwrap();
    let mut ui = mvs_ui::element::Ui::new(&gpu, &assets).unwrap();
    ui.time = Some(1.0 + f64::from(l.n) / 60.0);
    ui.draw(&gpu, &assets, &mut l.env.fonts, &l.out, &target).unwrap();
    let px = gpu.read_texture(&target).unwrap();
    mvs_render::image::write_png(std::path::Path::new(&out), w, h, &px).unwrap();
    eprintln!("{} elements, {out}", l.out.elements.len());
}

impl Lobby {
    /// The map selector's tiles, in grid order, and the text of its Accept button.
    fn map_tiles(&self) -> Vec<WidgetId> {
        let t = self.screen.view.tree();
        let grid = t.find_in(self.find("mapSelector", "MapGrid"), "MapGrid");
        let grid = if grid == WidgetId::default() { self.find("mapSelector", "MapGrid") } else { grid };
        t.post_order(grid).into_iter().filter(|&w| t.user_class(w).is_some_and(|c| c.contains("Map"))).collect()
    }

    fn accept_text(&self) -> String {
        let t = self.screen.view.tree();
        let b = self.find("mapSelector", "buttonAccept");
        t.post_order(b)
            .into_iter()
            .map(|w| t.widget(w).text.clone())
            .find(|s| s.starts_with("Accept"))
            .unwrap_or_default()
    }
}

#[test]
fn picked_maps_count_in_the_selectors_accept_button() {
    let Some(mut l) = Lobby::new(env()) else { return };
    let b = l.find("customGame", "SelectMapsButton");
    l.click(b);
    assert!(l.up("mapSelector"), "Select Maps opens the map selector");
    l.frames(30, &[]);
    let tiles = l.map_tiles();
    eprintln!(
        "tiles {}: {:?}",
        tiles.len(),
        tiles.iter().map(|&w| l.screen.view.tree().user_class(w).map(str::to_string)).collect::<Vec<_>>()
    );
    assert!(tiles.len() >= 2, "the grid has its tiles");
    let c = l.find("mapSelector", "btn_Clear");
    l.click(c);
    assert!(l.accept_text().contains("(0 "), "Clear All counts none: {}", l.accept_text());
    let before = l.accept_text();
    l.click(tiles[0]);
    let one = l.accept_text();
    l.click(tiles[1]);
    let two = l.accept_text();
    eprintln!("{before} / {one} / {two}");
    assert!(one.contains("(1 "), "one pick counts one: {one}");
    assert!(two.contains("(2 "), "two picks count two: {two}");
    // deselect the first
    l.click(tiles[0]);
    assert!(l.accept_text().contains("(1 "), "a second click deselects");
    // Select All, then Clear All
    let a = l.find("mapSelector", "btn_SelectAll");
    l.click(a);
    // (the build's 16 and the shipped custom maps)
    let all = format!("({} ", 16 + mvs_stage::shipped().len());
    assert!(l.accept_text().contains(&all), "Select All: {}", l.accept_text());
    l.click(c);
    assert!(l.accept_text().contains("(0 "), "Clear All again: {}", l.accept_text());
}

/// The experimental page's Mutators row is a checklist: each click on a row of its list asks the lobby for the set
/// with one more ticked (the second from the page's own copy, before the lobby answers the first), and the list stays
/// up for the next.
#[test]
fn the_mutators_list_ticks_more_than_one() {
    use mvs_protocol::messages::{ClientMessage, OptionWire};
    use mvs_ui::menu::custom_text;
    use mvs_ui::menu::online::OnlineCmd;
    let Some(mut l) = Lobby::new(env()) else { return };
    let b = l.find("customGame", "ResetToDefaultsButton");
    l.click(b);
    assert!(l.up("customGameExperimental"));
    let row = l.find("customGameExperimental", "Option_Mutators");
    let open = l.screen.view.tree().find_in(row, "mainInteractor");
    l.click(open);
    l.frames(30, &[]);
    let _ = l.screen.view.take_online_cmds();
    let rows = |l: &Lobby| -> Vec<(WidgetId, String)> {
        let t = l.screen.view.tree();
        t.ids()
            .filter(|&w| t.user_class(w) == Some("UI_ContextButton") && t.widget(w).parent.is_some())
            .map(|w| (w, t.widget(t.find_in(w, "label")).text.clone()))
            .collect()
    };
    let mut sent = Vec::new();
    for k in [2, 6] {
        let (w, _) = rows(&l).into_iter().find(|(_, t)| t.ends_with(custom_text::MUTATORS[k])).expect("the row");
        l.click(w);
        l.frames(20, &[]);
        for c in l.screen.view.take_online_cmds() {
            if let OnlineCmd::Send(m) = c
                && let ClientMessage::LobbySetOption { key, value: OptionWire::Number(n) } = *m
            {
                sent.push((key, n));
            }
        }
        assert_eq!(rows(&l).len(), custom_text::MUTATORS.len(), "the list stays up after a tick");
    }
    assert_eq!(sent, [("mutators".to_string(), 1 << 2), ("mutators".to_string(), (1 << 2) | (1 << 6))]);
}

/// The experimental page's Chaos Wheel row: On asks the lobby for the option `wheel`.
#[test]
fn the_wheel_row_asks_for_the_option() {
    use mvs_protocol::messages::{ClientMessage, OptionWire};
    use mvs_ui::menu::custom_text;
    use mvs_ui::menu::online::OnlineCmd;
    let Some(mut l) = Lobby::new(env()) else { return };
    let b = l.find("customGame", "ResetToDefaultsButton");
    l.click(b);
    assert!(l.up("customGameExperimental"));
    let row = l.find("customGameExperimental", "Option_Wheel");
    let open = l.screen.view.tree().find_in(row, "mainInteractor");
    l.click(open);
    l.frames(30, &[]);
    let _ = l.screen.view.take_online_cmds();
    let on = {
        let t = l.screen.view.tree();
        t.ids()
            .filter(|&w| t.user_class(w) == Some("UI_ContextButton") && t.widget(w).parent.is_some())
            .find(|&w| t.widget(t.find_in(w, "label")).text.ends_with(custom_text::ON))
            .expect("the On row")
    };
    l.click(on);
    l.frames(20, &[]);
    let sent: Vec<(String, bool)> = l
        .screen
        .view
        .take_online_cmds()
        .into_iter()
        .filter_map(|c| match c {
            OnlineCmd::Send(m) => match *m {
                ClientMessage::LobbySetOption { key, value: OptionWire::Bool(b) } => Some((key, b)),
                _ => None,
            },
            _ => None,
        })
        .collect();
    assert_eq!(sent, [("wheel".to_string(), true)]);
}

impl Lobby {
    /// The page's widget `name`; the connection line's are in the page's header (WBP_MenuHeader).
    fn page_w(&self, name: &str) -> WidgetId {
        if name.starts_with("LobbyConnection") {
            let header = self.find("customGame", "WBP_MenuHeader");
            return self.screen.view.tree().find_in(header, name);
        }
        self.find("customGame", name)
    }

    fn text(&self, name: &str) -> String {
        self.screen.view.tree().widget(self.page_w(name)).text.clone()
    }

    fn color(&self, name: &str) -> [f64; 4] {
        self.screen.view.tree().widget(self.page_w(name)).color
    }

    fn shown(&self, name: &str) -> bool {
        self.screen.view.tree().widget(self.page_w(name)).vis != Vis::Collapsed
    }

    /// The button `w`'s interactor takes clicks.
    fn takes_clicks(&self, w: WidgetId) -> bool {
        let i = self.screen.view.tree().find_in(w, "UI_Interactor");
        self.screen.view.manager.interactor(i).expect("an interactor").enabled
    }

    /// Every empty slot's invite button takes clicks.
    fn invites_take_clicks(&self) -> Vec<bool> {
        let t = self.screen.view.tree();
        let mut out = Vec::new();
        for team in ["WBP_CustomGameTeam", "WBP_CustomGameTeam_1", "WBP_CustomGameTeam_ffa"] {
            let team = self.find("customGame", team);
            for n in 1..=4 {
                let slot = t.find_in(team, &format!("EmptySlot{n}"));
                out.push(self.takes_clicks(t.find_in(slot, "btn_InvitePlayer")));
            }
        }
        out
    }

    /// The session's link becomes `link` (the next frames' view).
    fn link(&mut self, link: SessionLink) {
        if let Some(s) = self.screen.view.session.online.session.as_mut() {
            s.link = Some(link);
        }
        self.frames(2, &[]);
    }
}

/// (the port's own) The connection line under the page's title says how the lobby is connected, its state word in the
/// state's colour and no round trip (the top-right ping line's); a local lobby says why with what to do as its tip,
/// and keeps its code's and settings' buttons in view, greyed, with its invites off; online again gives them back.
#[test]
fn the_connection_line_says_the_link_and_greys_a_local_lobby() {
    use mvs_ui::hud::ping::GREEN;
    use mvs_ui::menu::custom_text as t;
    // the title's cream (WBP_MenuHeader txt_PageTitle)
    const CREAM: [f64; 4] = [0.89627, 0.973446, 0.871367, 1.0];
    let Some(mut l) = Lobby::new(env()) else { return };
    assert!(l.shown("LobbyConnection"));
    assert_eq!(l.text("LobbyConnectionHead"), "ONLINE");
    assert_eq!(l.color("LobbyConnectionHead"), GREEN);
    assert_eq!(l.text("LobbyConnectionDetail"), "via Relay");
    assert_eq!(l.color("LobbyConnectionDetail"), CREAM);
    assert!(!l.shown("LobbyConnectionTip"));
    let header = l.find("customGame", "WBP_MenuHeader");
    assert_eq!(l.screen.view.tree().find_opt(header, "LobbyConnectionPing"), None, "the line has no round trip");
    let (code, settings) = (l.find("customGame", "LobbyCodeButton"), l.find("customGame", "LobbySettingsButton"));
    assert!(l.takes_clicks(code) && l.takes_clicks(settings));
    assert!(l.invites_take_clicks().iter().all(|&b| b));
    assert!(l.shown("LobbyCodeView"));

    l.link(SessionLink::Epic);
    assert_eq!(l.text("LobbyConnectionHead"), "ONLINE");
    assert_eq!(l.text("LobbyConnectionDetail"), "via Epic");
    l.link(SessionLink::Lan { at: "192.168.1.5:7777".into() });
    assert_eq!(l.text("LobbyConnectionHead"), "LAN");
    assert_eq!(l.text("LobbyConnectionDetail"), "192.168.1.5:7777");

    l.link(SessionLink::Local(LocalReason::Offline(OfflineWhy::NotReached)));
    assert_eq!(l.text("LobbyConnectionHead"), "OFFLINE");
    assert_ne!(l.color("LobbyConnectionHead"), GREEN);
    assert_eq!(l.text("LobbyConnectionDetail"), t::WHY_NOT_REACHED);
    assert!(l.shown("LobbyConnectionTip"), "what to do is the line's tip");
    assert!(l.shown("LobbyCodeButton") && l.shown("LobbySettingsButton"), "greyed, not hidden");
    assert!(!l.takes_clicks(code) && !l.takes_clicks(settings));
    assert!(l.invites_take_clicks().iter().all(|&b| !b));
    assert!(!l.shown("LobbyCodeView"), "a local lobby has no code to show");
    l.link(SessionLink::Local(LocalReason::MadeOffline));
    assert_eq!(l.text("LobbyConnectionDetail"), t::LOBBY_LOCAL_MADE_OFFLINE);
    l.link(SessionLink::Local(LocalReason::CannotHost));
    assert_eq!(l.text("LobbyConnectionDetail"), t::LOBBY_LOCAL_CANNOT_HOST);

    l.link(SessionLink::Relay);
    assert!(l.takes_clicks(code) && l.takes_clicks(settings));
    assert!(l.invites_take_clicks().iter().all(|&b| b));
    assert!(!l.shown("LobbyConnectionTip"));
    for name in ["LobbyConnectionHead", "LobbyConnectionDetail"] {
        assert!(!l.text(name).contains('\u{b7}'), "no separator dot: {name}");
    }
}

/// (the port's own) A run with no net service at all makes a UI-only local lobby: its line is local for the menus'
/// reason.
#[test]
fn a_lobby_with_no_net_service_says_why_it_is_local() {
    let Some(mut env) = env() else { return };
    let view = MenuView::new(env.bundle.clone(), env.text.clone(), SIZE);
    let mut screen = MenuScreen::new(view, &mut env.run());
    screen.view.session.online =
        OnlineView { line: OnlineLine::Offline(OfflineWhy::TurnedOff), ..OnlineView::default() };
    screen.with_cx(&mut env.run(), |cx| Manager::show_modal(cx, "WBP_CustomGame", "customGame", "", ""));
    let mut l = Lobby { env, screen, out: ElementList::default(), n: 0, online_ms: 0.0 };
    l.frames(30, &[]);
    assert_eq!(l.text("LobbyConnectionHead"), "OFFLINE");
    assert_eq!(l.text("LobbyConnectionDetail"), mvs_ui::menu::custom_text::WHY_TURNED_OFF);
    let code = l.find("customGame", "LobbyCodeButton");
    assert!(l.shown("LobbyCodeButton") && !l.takes_clicks(code));
}

/// The Versus lobby shows its four places, and still does after Versus is picked again from the Game Mode drop (a
/// pick the lobby does not answer: the empty slots keep the last count of free places).
#[test]
fn a_versus_lobby_keeps_its_four_places() {
    let Some(mut l) = Lobby::new(env()) else { return };
    let open = |l: &Lobby, team: &str| {
        let t = l.find("customGame", team);
        (1..=4)
            .filter(|n| {
                let s = l.screen.view.tree().find_in(t, &format!("EmptySlot{n}"));
                l.screen.view.tree().widget(s).vis != Vis::Collapsed
            })
            .count()
    };
    let places = |l: &Lobby| (open(l, "WBP_CustomGameTeam"), open(l, "WBP_CustomGameTeam_1"));
    assert_eq!(places(&l), (1, 2));
    // (the drop's click event as its list's Versus row fires it)
    let drop = l.find("customGame", "UI_DropGameMode");
    l.screen.with_cx(&mut l.env.run(), |cx| {
        cx.visit::<DropMenu, _>(drop, |m, cx| cx.emit(m, |c| &c.on_click, &(0, "versus".to_string())));
    });
    l.frames(10, &[]);
    assert_eq!(places(&l), (1, 2));
}
