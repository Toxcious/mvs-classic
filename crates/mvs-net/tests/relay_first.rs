//! Relay first (online plan v4 section 5): a custom game is hosted on the relay, else on Epic, else on LAN, else
//! locally (host_order.rs has the rest of that order); the queue
//! stays on the relay, else Epic. A relay-hosted session's match path that wants its Epic links counts in Epic's relay
//! control (`NetService::relay_check`): a forced path (a queue's) has Epic's relays forced at once when it says it
//! wants its links (`path_wanted`) and holds them QUEUE_HOLD_MS after its end, a direct custom path alone allows
//! direct links. Over the in-memory net; PathSide here never opens a path, so the paths here are told through
//! `path_wanted` alone (that a real path calls it before its first ping is path_referee.rs). LAN over
//! 127.0.0.1.

use mvs_net::backend::memory::{MemoryNet, MemoryNetOptions};
use mvs_net::backend::{BackendKind, RelayMode};
use std::net::TcpListener;
use std::time::Instant;

use mvs_net::service::api::{DirectOptions, LanOptions, NetEvent, NetRequest, SessionOn, SessionTarget};
use mvs_net::service::{NetService, PageId, QUEUE_HOLD_MS, ServiceConfig, ServiceOptions};
use mvs_protocol::version::PROTOCOL;

const RELAY: &str = "127.0.0.1:7777";

/// The release's relay is the config's: its lobbies' codes are short ('r').
fn config() -> ServiceConfig {
    ServiceConfig { relay: Some(RELAY.to_string()), default_relay: Some(RELAY.to_string()), ..ServiceConfig::default() }
}

struct Rig {
    net: MemoryNet,
    svc: NetService,
    t: f64,
}

impl Rig {
    /// A service with the config and the backends named signed in.
    fn new(c: ServiceConfig, relay: bool, epic: bool) -> Rig {
        // (the relay's cores answer with their first message: the page's Opened waits for it)
        let o = MemoryNetOptions { relay_ids: true, core_proto: Some(PROTOCOL), ..Default::default() };
        let net = MemoryNet::new(o, 0.0);
        let mut svc = NetService::new(ServiceOptions::default(), 0.0);
        svc.configure(c, 0.0);
        if relay {
            svc.start_backend(BackendKind::Relay, Box::new(net.player(Some("R1"), BackendKind::Relay)), 1.0);
        }
        if epic {
            svc.start_backend(BackendKind::Epic, Box::new(net.player(Some("E1"), BackendKind::Epic)), 1.0);
        }
        Rig { net, svc, t: 1.0 }
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

    /// A page that opens `target`; its events in the first 200 ms.
    fn open(&mut self, target: SessionTarget) -> (PageId, Vec<NetEvent>) {
        let page = self.svc.open_page(self.t);
        self.svc.take(page, NetRequest::Open(target), self.t);
        let seen = self.run(page, 200.0);
        (page, seen)
    }
}

/// This player's id in the page's session as it opened (its backend's: R1 the relay, E1 Epic).
fn opened_me(seen: &[NetEvent]) -> Option<String> {
    seen.iter().find_map(|e| match e {
        NetEvent::Opened { me, .. } => Some(me.clone()),
        _ => None,
    })
}

/// The code the page's session opened with ('' none).
fn opened_code(seen: &[NetEvent]) -> Option<String> {
    seen.iter().find_map(|e| match e {
        NetEvent::Opened { code, .. } if !code.is_empty() => Some(code.clone()),
        _ => None,
    })
}

#[test]
fn a_custom_game_is_hosted_on_the_relay_first() {
    let mut r = Rig::new(config(), true, true);
    let (_, seen) = r.open(SessionTarget::Host);
    let code = opened_code(&seen).unwrap_or_else(|| panic!("the lobby opens: {seen:?}"));
    assert!(code.starts_with('r'), "a relay code, not Epic's: {code}");
    assert_eq!(opened_me(&seen).as_deref(), Some("R1"));
}

#[test]
fn a_custom_game_is_hosted_on_epic_when_the_relay_is_off_or_cannot_host() {
    let mut off = Rig::new(config(), false, true);
    let (_, seen) = off.open(SessionTarget::Host);
    let code = opened_code(&seen).unwrap_or_else(|| panic!("the lobby opens: {seen:?}"));
    assert!(code.starts_with('e'), "relay off: an Epic code: {code}");
    assert_eq!(opened_me(&seen).as_deref(), Some("E1"));

    // (the relay signed in, but the config names no relay: no code of it can be made)
    let mut no_code = Rig::new(ServiceConfig::default(), true, true);
    let (_, seen) = no_code.open(SessionTarget::Host);
    let code = opened_code(&seen).unwrap_or_else(|| panic!("the lobby opens: {seen:?}"));
    assert!(code.starts_with('e'), "the relay cannot host: an Epic code: {code}");
}

#[test]
fn with_neither_a_custom_game_is_hosted_locally() {
    let mut r = Rig::new(config(), false, false);
    let (_, seen) = r.open(SessionTarget::Host);
    assert!(!seen.iter().any(|e| matches!(e, NetEvent::Closed(_))), "not closed: {seen:?}");
    let local = seen.iter().any(|e| matches!(e, NetEvent::Opened { on: SessionOn::Local { .. }, .. }));
    assert!(local, "a local lobby: {seen:?}");
}

#[test]
fn the_queue_stays_on_the_relay() {
    let mut r = Rig::new(config(), true, true);
    let (_, seen) = r.open(SessionTarget::Queue);
    assert!(!seen.iter().any(|e| matches!(e, NetEvent::Closed(_))), "the queue opens: {seen:?}");
    assert_eq!(opened_me(&seen).as_deref(), Some("R1"), "the queue is the relay's, not Epic's");
}

#[test]
fn path_wanted_forces_epics_relays_for_a_forced_path_and_its_end_holds_them() {
    let mut r = Rig::new(config(), true, true);
    let (page, _) = r.open(SessionTarget::Queue);
    // (Epic's relays are forced with no session open on Epic; a direct custom path first allows them, as it would at
    // a queue's)
    let (warm, _) = r.open(SessionTarget::Host);
    let t = r.t;
    r.svc.path_wanted(warm, true, &["E4".to_string()], t);
    assert_eq!(r.svc.epic_relay_mode(), Some(RelayMode::Allow));

    // (a queue's path told its humans: forced at once)
    let t = r.t;
    r.svc.path_wanted(page, false, &["E2".to_string()], t);
    assert_eq!(r.svc.epic_relay_mode(), Some(RelayMode::Force), "forced at path_wanted, in the same call");

    // (the path's end is seen when its session goes back with no path open: the stub's never is)
    r.svc.take(page, NetRequest::Send(Vec::new()), t);
    assert_eq!(r.svc.epic_relay_mode(), Some(RelayMode::Force));

    // (a direct custom path right after: still forced, the hold not over)
    let (custom, _) = r.open(SessionTarget::Host);
    let t = r.t;
    r.svc.path_wanted(custom, true, &["E3".to_string()], t);
    assert_eq!(r.svc.epic_relay_mode(), Some(RelayMode::Force), "held after a forced path");

    // (the hold over: allowed; the direct path stays wanted through the wait, told again after each put-back)
    let wait = QUEUE_HOLD_MS - (r.t - t) + 1.0;
    let mut left = wait;
    while left > 0.0 {
        let step = left.min(100.0);
        r.run(custom, step);
        let t = r.t;
        r.svc.path_wanted(custom, true, &["E3".to_string()], t);
        left -= step;
    }
    r.svc.flush(r.t);
    assert_eq!(r.svc.epic_relay_mode(), Some(RelayMode::Allow), "a direct path alone allows direct links");
}

#[test]
fn a_direct_path_is_forced_beside_a_forced_one_or_when_direct_is_off() {
    let mut r = Rig::new(config(), true, true);
    let (a, _) = r.open(SessionTarget::Host);
    let (b, _) = r.open(SessionTarget::Queue);
    let t = r.t;
    r.svc.path_wanted(a, true, &["E2".to_string()], t);
    assert_eq!(r.svc.epic_relay_mode(), Some(RelayMode::Allow));
    r.svc.path_wanted(b, false, &["E3".to_string()], t);
    assert_eq!(r.svc.epic_relay_mode(), Some(RelayMode::Force), "every open way must be a direct custom one");

    let mut off = Rig::new(ServiceConfig { allow_direct: false, ..config() }, true, true);
    let (a, _) = off.open(SessionTarget::Host);
    let t = off.t;
    off.svc.path_wanted(a, true, &["E2".to_string()], t);
    assert_eq!(off.svc.epic_relay_mode(), Some(RelayMode::Force), "Direct connections off in the config");
}

/// The relay and Epic off (the relay signed in but unable to host, then not signed in): LAN's direct host hosts it.
#[test]
fn a_custom_game_is_hosted_on_lan_last() {
    let dir = std::env::temp_dir().join(format!("mvs-relay-first-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("a folder");
    for relay in [true, false] {
        let mut r = Rig::new(ServiceConfig::default(), relay, false);
        let port = TcpListener::bind("127.0.0.1:0").expect("a port").local_addr().expect("an address").port();
        let direct = DirectOptions {
            key_file: dir.join(format!("lan-{relay}.key")),
            product: "test".into(),
            version: "test".into(),
            loopback: true,
        };
        r.svc.start_lan(LanOptions { direct, port }, r.t);
        let page = r.svc.open_page(r.t);
        r.svc.take(page, NetRequest::Open(SessionTarget::Host), r.t);
        // (the direct host listens for real: the wall clock, at most 5 s)
        let start = Instant::now();
        let mut seen = Vec::new();
        while opened_code(&seen).is_none() && start.elapsed().as_secs_f64() < 5.0 {
            seen.extend(r.run(page, 1.0));
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
        let code = opened_code(&seen).unwrap_or_else(|| panic!("relay signed in {relay}: the lobby opens: {seen:?}"));
        assert!(code.starts_with('d'), "relay signed in {relay}: a LAN direct code: {code}");
        r.svc.shutdown(r.t);
    }
    let _ = std::fs::remove_dir_all(&dir);
}
