//! The social actions menu (UI_SocialContextMenu) in a menu view: opened for the party partner, placed by the
//! build's rule, its Leave Party sending the party leave and closing it, the cancel closing it. Needs the dev store;
//! without one each test says so and passes.

use std::sync::Arc;

use mvs_net::service::api::{PartyMember, PartyView};
use mvs_settings::Settings;
use mvs_ui::Bundle;
use mvs_ui::bindings::Bindings;
use mvs_ui::element::ElementList;
use mvs_ui::menu::controller::MenuRun;
use mvs_ui::menu::input::Pointer;
use mvs_ui::menu::manager::Manager;
use mvs_ui::menu::online::{OnlineCmd, PartyCmd};
use mvs_ui::menu::widgets::social_context_menu::{CLASS, player_data, show_social_actions_menu};
use mvs_ui::menu::{MenuInput, MenuScreen, MenuView, NoSound};
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
    if !bundle.widgets.contains_key(CLASS) {
        eprintln!("the menu bundle has no {CLASS} (an import before ui 30): skipped");
        return None;
    }
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

struct Menu {
    env: Env,
    screen: MenuScreen,
    out: ElementList,
    n: u32,
}

impl Menu {
    /// A screen online in a party of two, this player "me" leading, the partner "pal".
    fn new(env: Env) -> Menu {
        let mut env = env;
        let view = MenuView::new(env.bundle.clone(), env.text.clone(), [1600, 900]);
        let mut screen = MenuScreen::new(view, &mut env.run());
        let o = &mut screen.view.session.online;
        o.connected = true;
        let member = |id: &str| PartyMember { id: id.into(), name: id.into(), icon: String::new() };
        o.party = Some(PartyView { leader: "me".into(), members: vec![member("me"), member("pal")], me: "me".into() });
        Menu { env, screen, out: ElementList::default(), n: 0 }
    }

    fn frames(&mut self, count: u32, input: &[MenuInput]) {
        for i in 0..count {
            self.n += 1;
            let now = 1.0 + f64::from(self.n) / 60.0;
            let input = if i == 0 { input } else { &[] };
            self.screen.frame(&mut self.env.run(), input, now, &mut self.out);
        }
    }

    fn open(&mut self, at: [f64; 2]) -> Option<WidgetId> {
        self.screen.with_cx(&mut self.env.run(), |cx| {
            let d = player_data(cx, "pal");
            show_social_actions_menu(cx, d, at, "", &[])
        })
    }

    fn find(&self, m: WidgetId, name: &str) -> WidgetId {
        self.screen.view.tree().find_in(m, name)
    }

    fn click(&mut self, w: WidgetId) {
        let p = self.screen.view.tree().painted(w).expect("painted");
        let (x, y) = (p.size[0] / 2.0, p.size[1] / 2.0);
        let m = p.transform;
        let (x, y) = (m[0] * x + m[2] * y + m[4], m[1] * x + m[3] * y + m[5]);
        self.frames(3, &[MenuInput::MouseMove { x, y }]);
        self.frames(3, &[MenuInput::MouseButton { button: 0, down: true }]);
        self.frames(10, &[MenuInput::MouseButton { button: 0, down: false }]);
    }
}

#[test]
fn the_partners_menu_shows_leave_party_and_leaving_closes_it() {
    let Some(env) = env() else { return };
    let mut m = Menu::new(env);
    let vh = m.screen.view.viewport_size()[1];
    let w = m.open([200.0, vh - 100.0]).expect("the menu opens");
    assert!(m.open([200.0, 100.0]).is_none(), "one menu per player");
    m.frames(20, &[]);
    let t = m.screen.view.tree();
    for (n, v) in [("buttonLeave", Vis::Visible), ("buttonInviteToParty", Vis::Collapsed)] {
        assert_eq!(t.widget(m.find(w, n)).vis, v, "{n}");
    }
    assert_eq!(t.widget(m.find(w, "buttonProfile")).vis, Vis::Collapsed);
    let menu = t.widget(m.find(w, "menu"));
    assert_eq!((menu.offsets.l, menu.offsets.t), (200.0, vh - 100.0 - 450.0), "moved up near the bottom");
    m.screen.view.take_online_cmds();
    let leave = m.find(w, "buttonLeave");
    m.click(leave);
    assert!(m.screen.view.take_online_cmds().contains(&OnlineCmd::Party(PartyCmd::Leave)));
    assert!(m.screen.view.manager.get_modal("pal").is_none(), "the click closed it");
}

#[test]
fn the_cancel_closes_the_menu_and_a_stranger_alone_gets_no_menu() {
    let Some(env) = env() else { return };
    let mut m = Menu::new(env);
    m.open([300.0, 200.0]).expect("the menu opens");
    m.frames(10, &[]);
    m.screen.with_cx(&mut m.env.run(), |cx| Manager::cancel(cx, 0, true));
    m.frames(2, &[]);
    assert!(m.screen.view.manager.get_modal("pal").is_none());
    // no party and nobody listed: no action, so nothing opens
    m.screen.view.session.online.party = None;
    assert!(m.open([300.0, 200.0]).is_none());
}
