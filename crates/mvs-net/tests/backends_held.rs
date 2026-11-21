//! A change of the backends the player makes while the game runs (`NetHandle::backends_when_free`, the launcher's
//! Online tab): set at once while no custom lobby or queue is open, else held until it closes, a newer change
//! replacing the one held; backends set at once (the update gate's) drop it. Over the in-memory net: a lobby on the
//! relay, a queue session on the relay, and a local lobby (no backend under it).

use std::path::PathBuf;

use mvs_net::backend::BackendKind;
use mvs_net::backend::memory::{MemoryNet, MemoryNetOptions};
use mvs_net::service::api::{DirectOptions, NetRequest, OnlineBackends, SessionTarget};
use mvs_net::service::{NetService, PageId, ServiceConfig, ServiceOptions};
use mvs_protocol::version::PROTOCOL;

const RELAY: &str = "127.0.0.1:7777";

fn direct() -> DirectOptions {
    DirectOptions {
        key_file: PathBuf::from("relay.key"),
        product: "mvs".into(),
        version: "test".into(),
        loopback: true,
    }
}

/// Backends with only `allow_direct` told apart (what is held is handed back whole).
fn backends(allow_direct: bool) -> Box<OnlineBackends> {
    Box::new(OnlineBackends {
        relay: None,
        epic: None,
        lan: None,
        allow_direct,
        default_relay: None,
        known_relays: Vec::new(),
    })
}

struct Rig {
    net: MemoryNet,
    svc: NetService,
    t: f64,
}

impl Rig {
    /// A service with the memory relay signed in (`relay`), else nothing.
    fn new(relay: bool) -> Rig {
        let o = MemoryNetOptions { relay_ids: true, core_proto: Some(PROTOCOL), ..Default::default() };
        let net = MemoryNet::new(o, 0.0);
        let mut svc = NetService::new(ServiceOptions::default(), 0.0);
        let c = ServiceConfig {
            relay: Some(RELAY.to_string()),
            default_relay: Some(RELAY.to_string()),
            ..ServiceConfig::default()
        };
        svc.configure(c, 0.0);
        if relay {
            svc.start_backend(BackendKind::Relay, Box::new(net.player(Some("R1"), BackendKind::Relay)), 1.0);
        }
        Rig { net, svc, t: 1.0 }
    }

    fn run(&mut self, page: PageId, ms: f64) {
        let to = self.t + ms;
        while self.t < to {
            self.t += 1.0;
            self.net.pump(self.t);
            self.svc.pump(self.t);
            let _ = self.svc.events(page);
        }
    }

    /// A page with `target` open (200 ms run).
    fn open(&mut self, target: SessionTarget) -> PageId {
        let page = self.svc.open_page(self.t);
        self.svc.take(page, NetRequest::Open(target), self.t);
        self.run(page, 200.0);
        page
    }

    fn close(&mut self, page: PageId) {
        self.svc.take(page, NetRequest::Close, self.t);
        self.run(page, 200.0);
    }
}

#[test]
fn a_change_with_no_session_open_is_set_at_once() {
    let mut r = Rig::new(true);
    let page = r.svc.open_page(r.t);
    assert!(!r.svc.session_open());
    let now = r.svc.hold_backends(backends(true), direct());
    assert_eq!(now.map(|(b, _)| b.allow_direct), Some(true));
    assert!(!r.svc.backends_held());
    r.run(page, 10.0);
    assert!(r.svc.free_backends().is_none());
}

#[test]
fn a_change_waits_for_the_lobby_or_the_queue_to_close() {
    for (relay, target, what) in [
        (true, SessionTarget::Host, "a lobby on the relay"),
        (true, SessionTarget::Queue, "a queue on the relay"),
        (false, SessionTarget::Host, "a local lobby"),
    ] {
        let mut r = Rig::new(relay);
        let page = r.open(target);
        assert!(r.svc.session_open(), "{what}: open");
        assert!(r.svc.hold_backends(backends(false), direct()).is_none(), "{what}: held");
        // (a newer change replaces the one held)
        assert!(r.svc.hold_backends(backends(true), direct()).is_none(), "{what}: held again");
        assert!(r.svc.backends_held(), "{what}");
        r.run(page, 500.0);
        assert!(r.svc.free_backends().is_none(), "{what}: still open");
        r.close(page);
        assert!(!r.svc.session_open(), "{what}: closed");
        let freed = r.svc.free_backends();
        assert_eq!(freed.map(|(b, _)| b.allow_direct), Some(true), "{what}: the newest change, once closed");
        assert!(!r.svc.backends_held() && r.svc.free_backends().is_none(), "{what}: given once");
    }
}

#[test]
fn backends_set_at_once_drop_a_change_held() {
    let mut r = Rig::new(false);
    let page = r.open(SessionTarget::Host);
    assert!(r.svc.hold_backends(backends(true), direct()).is_none());
    r.svc.drop_held_backends();
    assert!(!r.svc.backends_held());
    r.close(page);
    assert!(r.svc.free_backends().is_none());
}
