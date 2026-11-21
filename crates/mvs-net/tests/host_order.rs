//! Where a custom game and the queue open (the online clarity plan, part 1), over the in-memory net: a custom game on
//! the relay, else Epic, else LAN, else a local lobby (the host core in the service with no backend under it); a relay
//! whose cores run another version of the game's protocol is left before the page hears of it, remembered until its
//! sign-in or its address changes, and the open goes on to the next step; the queue then takes Epic or closes
//! `RelayVersion`. Every `Opened` says what the session is connected through. The memory net's hosted cores answer a
//! knock with their first message alone (`MemoryNetOptions::core_proto`). LAN over 127.0.0.1.

use std::net::TcpListener;
use std::sync::{Arc, Mutex};
use std::time::Instant;

use serde_json::{Value, json};

use mvs_net::backend::BackendKind;
use mvs_net::backend::memory::{MemoryNet, MemoryNetOptions};
use mvs_net::service::api::{
    ClosedWhy, DirectOptions, LanOptions, LocalWhy, NetEvent, NetRequest, SessionOn, SessionTarget, StatusWhy,
};
use mvs_net::service::{NetService, PageId, ServiceConfig, ServiceOptions};
use mvs_protocol::version::PROTOCOL;

const RELAY: &str = "127.0.0.1:7777";

/// The release's relay is the config's.
fn config() -> ServiceConfig {
    ServiceConfig { relay: Some(RELAY.to_string()), default_relay: Some(RELAY.to_string()), ..ServiceConfig::default() }
}

struct Rig {
    net: MemoryNet,
    svc: NetService,
    t: f64,
    log: Arc<Mutex<Vec<String>>>,
}

impl Rig {
    /// A service with the config and the backends named signed in; the relay's cores of protocol `proto`.
    fn new(c: ServiceConfig, relay: bool, epic: bool, proto: u32) -> Rig {
        let o = MemoryNetOptions { relay_ids: true, core_proto: Some(proto), ..Default::default() };
        let net = MemoryNet::new(o, 0.0);
        let log = Arc::new(Mutex::new(Vec::new()));
        let lines = log.clone();
        let o = ServiceOptions {
            log: Some(Box::new(move |l: &str| lines.lock().expect("the log").push(l.to_string()))),
            ..ServiceOptions::default()
        };
        let mut svc = NetService::new(o, 0.0);
        svc.configure(c, 0.0);
        if relay {
            svc.start_backend(BackendKind::Relay, Box::new(net.player(Some("R1"), BackendKind::Relay)), 1.0);
        }
        if epic {
            svc.start_backend(BackendKind::Epic, Box::new(net.player(Some("E1"), BackendKind::Epic)), 1.0);
        }
        Rig { net, svc, t: 1.0, log }
    }

    /// Runs the service and the net `ms` more, 1 ms at a time; the page's events in that time.
    fn run(&mut self, page: PageId, ms: f64) -> Vec<NetEvent> {
        let mut seen = Vec::new();
        let to = self.t + ms;
        while self.t < to {
            self.t += 1.0;
            self.net.pump(self.t);
            self.svc.pump(self.t);
            seen.extend(self.svc.events(page));
        }
        seen
    }

    /// A page that opens `target`; its events in the first 200 ms (its first status left out).
    fn open(&mut self, target: SessionTarget) -> (PageId, Vec<NetEvent>) {
        let page = self.svc.open_page(self.t);
        let _ = self.svc.events(page);
        self.svc.take(page, NetRequest::Open(target), self.t);
        let mut seen = self.svc.events(page);
        seen.extend(self.run(page, 200.0));
        (page, seen)
    }

    fn logged(&self, part: &str) -> bool {
        self.log.lock().expect("the log").iter().any(|l| l.contains(part))
    }

    fn clear_log(&self) {
        self.log.lock().expect("the log").clear();
    }
}

/// The page's `Opened` events: what each says the session is on, its code and `chosen`.
fn opened(seen: &[NetEvent]) -> Vec<(SessionOn, String, bool)> {
    seen.iter()
        .filter_map(|e| match e {
            NetEvent::Opened { on, code, chosen, .. } => Some((on.clone(), code.clone(), *chosen)),
            _ => None,
        })
        .collect()
}

/// The page's messages from its session's core.
fn msgs(seen: &[NetEvent]) -> Vec<Value> {
    seen.iter()
        .filter_map(|e| match e {
            NetEvent::Msg(t) => serde_json::from_str(t).ok(),
            _ => None,
        })
        .collect()
}

fn closed(seen: &[NetEvent]) -> Vec<ClosedWhy> {
    seen.iter()
        .filter_map(|e| match e {
            NetEvent::Closed(w) => Some(w.clone()),
            _ => None,
        })
        .collect()
}

/// The page heard one `Opened`, then its core's `server` of this game's protocol, and no `Closed`.
fn one_open(seen: &[NetEvent], what: &str) -> (SessionOn, String, bool) {
    let o = opened(seen);
    assert_eq!(o.len(), 1, "{what}: one Opened: {seen:?}");
    assert!(closed(seen).is_empty(), "{what}: no Closed: {seen:?}");
    let first = seen.iter().position(|e| matches!(e, NetEvent::Msg(_)));
    let at = seen.iter().position(|e| matches!(e, NetEvent::Opened { .. }));
    assert!(first.is_none_or(|m| at.is_some_and(|o| o < m)), "{what}: the Opened before any message: {seen:?}");
    let m = msgs(seen);
    assert!(
        m.first().is_some_and(|m| m["t"] == "server" && m["proto"] == PROTOCOL),
        "{what}: the core's first message is this game's server: {m:?}"
    );
    o.into_iter().next().expect("one")
}

#[test]
fn a_custom_game_opens_on_the_relay_then_epic_then_locally() {
    let mut r = Rig::new(config(), true, true, PROTOCOL);
    let (_, seen) = r.open(SessionTarget::Host);
    let (on, code, chosen) = one_open(&seen, "the relay");
    assert_eq!((on, chosen), (SessionOn::Relay, true));
    assert!(code.starts_with('r'), "a relay code: {code}");

    let mut r = Rig::new(config(), false, true, PROTOCOL);
    let (_, seen) = r.open(SessionTarget::Host);
    let (on, code, chosen) = one_open(&seen, "Epic");
    assert_eq!((on, chosen), (SessionOn::Epic, true));
    assert!(code.starts_with('e'), "an Epic code: {code}");

    // (nothing on: a local lobby, its why the status's; not on a backend, so online going off never ends it)
    let mut r = Rig::new(config(), false, false, PROTOCOL);
    let (_, seen) = r.open(SessionTarget::Host);
    let (on, code, chosen) = one_open(&seen, "local");
    assert!(matches!(on, SessionOn::Local { why: LocalWhy::Status { why: StatusWhy::SigningIn } }), "{on:?}");
    assert_eq!((code.as_str(), chosen), ("", false));
}

#[test]
fn a_relay_of_another_version_is_left_for_epic_and_remembered_until_it_signs_in_again() {
    let mut r = Rig::new(config(), true, true, PROTOCOL - 1);
    let (page, seen) = r.open(SessionTarget::Host);
    let (on, code, _) = one_open(&seen, "past the relay");
    assert_eq!(on, SessionOn::Epic);
    assert!(code.starts_with('e'), "an Epic code: {code}");
    assert!(r.logged(&format!("the relay runs protocol {}", PROTOCOL - 1)), "the skip is logged");
    // (the relay's lobby is left: its leader gone, the lobby with it)
    r.run(page, 1000.0);
    assert!(r.net.lobbies().iter().all(|l| l.owner != "R1"), "the relay's lobby is left: {:?}", r.net.lobbies());

    // (the next open skips the relay at once, and says why)
    r.clear_log();
    let (_, seen) = r.open(SessionTarget::Host);
    assert_eq!(one_open(&seen, "the next open").0, SessionOn::Epic);
    assert!(r.logged("custom: not hosted on the relay: it runs protocol"), "the skip at once is logged");
    assert!(!r.logged("the relay runs protocol"), "no lobby was made on the relay");

    // (the relay redeployed and signed in again: tried again, and taken)
    r.net.set_core_proto(Some(PROTOCOL));
    let t = r.t;
    r.svc.start_backend(BackendKind::Relay, Box::new(r.net.player(Some("R2"), BackendKind::Relay)), t);
    let (_, seen) = r.open(SessionTarget::Host);
    assert_eq!(one_open(&seen, "the relay again").0, SessionOn::Relay);
}

#[test]
fn a_relay_of_another_version_alone_gives_a_local_lobby_until_its_address_changes() {
    let mut r = Rig::new(config(), true, false, PROTOCOL + 1);
    let (_, seen) = r.open(SessionTarget::Host);
    let (on, code, chosen) = one_open(&seen, "local");
    assert_eq!(on, SessionOn::Local { why: LocalWhy::RelayVersion });
    assert_eq!((code.as_str(), chosen), ("", false));
    let (_, seen) = r.open(SessionTarget::Host);
    assert_eq!(one_open(&seen, "again").0, SessionOn::Local { why: LocalWhy::RelayVersion });

    // (the relay's address changed in the options: tried again)
    r.net.set_core_proto(Some(PROTOCOL));
    let other = "127.0.0.1:7778".to_string();
    let t = r.t;
    r.svc.configure(ServiceConfig { relay: Some(other.clone()), default_relay: Some(other), ..config() }, t);
    let (_, seen) = r.open(SessionTarget::Host);
    assert_eq!(one_open(&seen, "the new address").0, SessionOn::Relay);
}

#[test]
fn the_queue_past_a_relay_of_another_version_takes_epic_or_says_why() {
    let mut r = Rig::new(config(), true, true, PROTOCOL - 1);
    let (_, seen) = r.open(SessionTarget::Queue);
    let o = opened(&seen);
    assert_eq!(o.len(), 1, "one Opened: {seen:?}");
    assert_eq!(o[0].0, SessionOn::Epic, "the queue on Epic: {seen:?}");
    assert!(closed(&seen).is_empty(), "no Closed: {seen:?}");
    assert!(msgs(&seen).iter().all(|m| m["proto"] != PROTOCOL - 1), "none of the relay core's messages: {seen:?}");

    let mut r = Rig::new(config(), true, false, PROTOCOL - 1);
    let (_, seen) = r.open(SessionTarget::Queue);
    assert!(opened(&seen).is_empty(), "never opened: {seen:?}");
    assert_eq!(closed(&seen), [ClosedWhy::RelayVersion], "{seen:?}");
    assert!(msgs(&seen).is_empty(), "none of the core's messages: {seen:?}");
    // (known now: closed at once, the relay not tried)
    let page = r.svc.open_page(r.t);
    let _ = r.svc.events(page);
    let t = r.t;
    r.svc.take(page, NetRequest::Open(SessionTarget::Queue), t);
    assert_eq!(closed(&r.svc.events(page)), [ClosedWhy::RelayVersion]);
}

/// LAN's direct host hosts it when the relay is off or of another version; its `Opened` names its address.
#[test]
fn a_custom_game_on_lan_names_its_address() {
    let dir = std::env::temp_dir().join(format!("mvs-host-order-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("a folder");
    for relay in [false, true] {
        let mut r = Rig::new(config(), relay, false, PROTOCOL - 1);
        let port = TcpListener::bind("127.0.0.1:0").expect("a port").local_addr().expect("an address").port();
        let direct = DirectOptions {
            key_file: dir.join(format!("lan-{relay}.key")),
            product: "test".into(),
            version: "test".into(),
            loopback: true,
        };
        r.svc.start_lan(LanOptions { direct, port }, r.t);
        let page = r.svc.open_page(r.t);
        let _ = r.svc.events(page);
        r.svc.take(page, NetRequest::Open(SessionTarget::Host), r.t);
        // (the direct host listens for real: the wall clock, at most 5 s)
        let start = Instant::now();
        let mut seen = Vec::new();
        while msgs(&seen).is_empty() && start.elapsed().as_secs_f64() < 5.0 {
            seen.extend(r.run(page, 1.0));
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
        let (on, code, chosen) = one_open(&seen, &format!("LAN, relay {relay}"));
        assert_eq!(on, SessionOn::Lan { at: format!("127.0.0.1:{port}") });
        assert!(code.starts_with('d') && chosen, "relay {relay}: a LAN direct code: {code}");
        r.svc.shutdown(r.t);
    }
    let _ = std::fs::remove_dir_all(&dir);
}

/// A page's run of a local lobby: its texts to the core, its events kept.
struct Local {
    r: Rig,
    page: PageId,
    seen: Vec<NetEvent>,
    taken: usize,
}

impl Local {
    fn say(&mut self, t: &str, body: Value) {
        let mut m = body;
        if t == "hello" {
            m["proto"] = json!(PROTOCOL);
            m["app"] = json!("test");
            m["name"] = json!("Host");
        }
        if t == "lobby.enter" {
            m["compat"] = json!("ctest.1");
        }
        m["t"] = json!(t);
        let now = self.r.t;
        self.r.svc.take(self.page, NetRequest::Msg(m.to_string()), now);
    }

    fn post(&mut self, m: NetRequest) {
        let now = self.r.t;
        self.r.svc.take(self.page, m, now);
    }

    /// Steps the service (no backend: to its next timer, 50 ms at the most) for `ms`.
    fn run(&mut self, ms: f64) {
        let to = self.r.t + ms;
        while self.r.t < to {
            let next = self.r.svc.next_due().unwrap_or(f64::INFINITY);
            self.r.t = next.min(self.r.t + 50.0).min(to).max(self.r.t + 1.0);
            let t = self.r.t;
            self.r.svc.pump(t);
            let page = self.page;
            self.seen.extend(self.r.svc.events(page));
        }
    }

    /// The first message of kind `t` since the last one taken, stepping up to 60 s of the service's clock for it.
    fn next(&mut self, t: &str) -> Value {
        for _ in 0..1200 {
            let page = self.page;
            self.seen.extend(self.r.svc.events(page));
            let at = self.seen[self.taken..].iter().position(|e| match e {
                NetEvent::Msg(x) => serde_json::from_str::<Value>(x).is_ok_and(|m| m["t"] == t),
                _ => false,
            });
            if let Some(i) = at {
                let i = self.taken + i;
                self.taken = i + 1;
                let NetEvent::Msg(x) = &self.seen[i] else { unreachable!() };
                return serde_json::from_str(x).expect("a message");
            }
            self.run(50.0);
        }
        panic!("no {t}: {:?}", msgs(&self.seen).iter().map(|m| m["t"].clone()).collect::<Vec<_>>());
    }
}

/// A local lobby carried through: its bot, the pre-match, its match (one human: the core's local match, which the
/// game plays by itself, no gate and no referee), its result, the next match; a backend signed in and an invite
/// meanwhile change nothing; the page's leave.
#[test]
fn a_local_lobby_plays_its_matches_with_bots() {
    let mut r = Rig::new(config(), false, false, PROTOCOL);
    let page = r.svc.open_page(r.t);
    let _ = r.svc.events(page);
    let t = r.t;
    r.svc.take(page, NetRequest::Open(SessionTarget::Host), t);
    let mut l = Local { r, page, seen: Vec::new(), taken: 0 };
    l.next("server");
    let me = match opened(&l.seen).first() {
        Some((SessionOn::Local { .. }, _, _)) => l.seen.iter().find_map(|e| match e {
            NetEvent::Opened { me, .. } => Some(me.clone()),
            _ => None,
        }),
        _ => None,
    }
    .unwrap_or_else(|| panic!("a local lobby: {:?}", l.seen));
    assert!(!me.is_empty(), "this player has an id in it");
    l.say("hello", json!({}));
    l.next("welcome");
    l.say("lobby.enter", json!({}));
    l.next("lobby.state");

    // (online coming up, and an invite: the lobby stays as it is)
    let t = l.r.t;
    l.r.svc.start_backend(BackendKind::Relay, Box::new(l.r.net.player(Some("R1"), BackendKind::Relay)), t);
    l.post(NetRequest::Invite { to: "R9".into(), name: "Host".into() });
    l.run(100.0);
    assert!(closed(&l.seen).is_empty(), "the local lobby stays: {:?}", l.seen);
    assert!(!l.seen.iter().any(|e| matches!(e, NetEvent::InviteRefused { .. })), "an invite does nothing");

    l.say("lobby.addBot", json!({ "character": "character_shaggy", "skin": "", "team": 1, "difficulty": 1 }));
    let bot = loop {
        let s = l.next("lobby.state");
        let teams = s["data"]["teams"].as_array().cloned().unwrap_or_default();
        if teams.iter().filter_map(Value::as_array).flatten().any(|p| p["isBot"] == true) {
            break s;
        }
    };
    assert!(bot["data"]["teams"].is_array());
    l.say("lobby.setTeamStyle", json!({ "style": 0 }));
    l.say("lobby.setMaps", json!({ "maps": ["map_a"] }));
    l.run(500.0);
    l.say("lobby.startMatch", json!({}));
    l.next("pm.begin");
    let loadout = json!({ "taunts": ["", "", "", ""] });
    l.say("pm.lock", json!({ "character": "shaggy", "skin": "", "loadout": loadout }));
    l.next("pm.all");
    l.say("pm.perks", json!({ "perks": [] }));
    let start = l.next("match.start");
    assert_eq!(start["local"], true, "one human and a bot: the game's own match: {start}");
    l.say("match.result", json!({}));
    l.say("match.leave", json!({}));
    l.run(500.0);
    assert!(closed(&l.seen).is_empty(), "the lobby outlives its match: {:?}", closed(&l.seen));
    // (back on the lobby's screen, as it was: the next match)
    l.say("lobby.enter", json!({}));
    l.next("lobby.state");
    l.say("lobby.startMatch", json!({}));
    l.next("pm.begin");

    l.post(NetRequest::Close);
    l.run(100.0);
    assert_eq!(closed(&l.seen), [ClosedWhy::Left]);
}
