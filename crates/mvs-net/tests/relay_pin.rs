//! The relay's key in the net service: two relays on 127.0.0.1 with different key files, one after the other on one
//! port (the relay at an address swapping its key). A code join to a relay not met before asks, and keeps the key it
//! proved; after the swap the join asks again (the key changed), a no keeps it refused and the key kept as it was, a
//! yes joins pinned to the new key and keeps it. A sign-in pinned to the first key is off as `OffWhy::RelayKey`; one
//! without a pin keeps the key the relay proved.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use mvs_net::backend::BackendKind;
use mvs_net::codes::{LobbyCode, encode_lobby_code};
use mvs_net::service::api::{
    AskWhat, ClosedWhy, DirectOptions, JoinWhy, NetEvent, NetRequest, OffWhy, OnlineBackends, RelayOptions, RelayPin,
    SessionTarget,
};
use mvs_net::service::page::PageId;
use mvs_net::service::signin::{SignIn, SignInOptions};
use mvs_net::service::{NetService, ServiceConfig, ServiceNote, ServiceOptions};
use mvs_protocol::relay_wire::KnownRelay;
use mvs_relay::server::{Options, start_relay};

/// A relay on 127.0.0.1 in a thread of its own, its key from `key_file`, until it is stopped.
struct Relay {
    port: u16,
    id: String,
    stop: Arc<AtomicBool>,
    done: std::sync::mpsc::Receiver<()>,
}

impl Relay {
    fn start(port: u16, key_file: &Path) -> Relay {
        let (tx, rx) = std::sync::mpsc::channel();
        let (done_tx, done) = std::sync::mpsc::channel();
        let stop = Arc::new(AtomicBool::new(false));
        let stopped = stop.clone();
        let key_file = key_file.to_path_buf();
        std::thread::spawn(move || {
            let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().expect("a runtime");
            let local = tokio::task::LocalSet::new();
            local.block_on(&rt, async move {
                let o = Options { port, host: "127.0.0.1".into(), key_file: Some(key_file), ..Options::default() };
                let r = start_relay(o).expect("the relay listens");
                tx.send((r.port, r.id.clone())).expect("the port");
                while !stopped.load(Ordering::SeqCst) {
                    tokio::time::sleep(Duration::from_millis(10)).await;
                }
                r.close().await;
            });
            let _ = done_tx.send(());
        });
        let (port, id) = rx.recv_timeout(Duration::from_secs(5)).expect("the relay started");
        Relay { port, id, stop, done }
    }

    /// Closed, its port free.
    fn stop(self) {
        self.stop.store(true, Ordering::SeqCst);
        self.done.recv_timeout(Duration::from_secs(10)).expect("the relay closed");
    }
}

/// A scratch folder of this test process, removed when dropped.
struct Dir(PathBuf);

impl Dir {
    fn new(name: &str) -> Dir {
        let d = std::env::temp_dir().join(format!("mvs-relay-pin-{}-{name}", std::process::id()));
        std::fs::create_dir_all(&d).expect("the scratch folder");
        Dir(d)
    }
}

impl Drop for Dir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// A guest: a net service signed in to nothing, its install's key, one page.
struct Guest {
    epoch: Instant,
    svc: NetService,
    page: PageId,
    events: Vec<NetEvent>,
    known: Vec<KnownRelay>,
}

impl Guest {
    fn new(dir: &Path) -> Guest {
        let epoch = Instant::now();
        let mut svc = NetService::new(ServiceOptions { epoch, ..ServiceOptions::default() }, 0.0);
        svc.configure(ServiceConfig::default(), 0.0);
        let direct = DirectOptions {
            key_file: dir.join("guest.key"),
            product: "mvs-test".into(),
            version: "0".into(),
            loopback: true,
        };
        svc.set_direct(Some(direct));
        let page = svc.open_page(0.0);
        Guest { epoch, svc, page, events: Vec::new(), known: Vec::new() }
    }

    fn now(&self) -> f64 {
        self.epoch.elapsed().as_secs_f64() * 1000.0
    }

    fn step(&mut self) {
        let now = self.now();
        self.svc.pump(now);
        self.events.extend(self.svc.events(self.page));
        for n in self.svc.take_notes() {
            if let ServiceNote::RelayKnown(k) = n {
                self.known.push(k);
            }
        }
    }

    /// Runs the service until `f` finds something in its events, 10 s at the most.
    fn until<R>(&mut self, what: &str, mut f: impl FnMut(&Guest) -> Option<R>) -> R {
        let end = Instant::now() + Duration::from_secs(10);
        loop {
            self.step();
            if let Some(r) = f(self) {
                return r;
            }
            assert!(Instant::now() < end, "{what}: not in time; events {:?}", self.events);
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    /// The events so far dropped (an earlier session's end taken out first).
    fn fresh(&mut self) {
        self.step();
        self.events.clear();
    }

    fn take(&mut self, m: NetRequest) {
        let now = self.now();
        self.svc.take(self.page, m, now);
    }

    /// Joins by `code`: the question it asks (its id and what).
    fn join_asks(&mut self, code: &str) -> (u32, AskWhat) {
        self.fresh();
        self.take(NetRequest::Open(SessionTarget::Join(code.into())));
        self.until("the relay question", |g| {
            g.events.iter().find_map(|e| match e {
                NetEvent::Ask { id, what } => Some((*id, what.clone())),
                _ => None,
            })
        })
    }

    /// The session's end.
    fn closed(&mut self) -> ClosedWhy {
        self.until("the session's end", |g| {
            g.events.iter().find_map(|e| match e {
                NetEvent::Closed(w) => Some(w.clone()),
                _ => None,
            })
        })
    }

    /// Answers the question `id`, then waits for the session's end (the code names no lobby on the relay).
    fn answer(&mut self, id: u32, yes: bool) -> ClosedWhy {
        self.take(NetRequest::Answer { id, yes });
        let w = self.closed();
        self.take(NetRequest::Close);
        w
    }

    /// Runs the service `ms`: a link of a session's own let go once its session ended (RELEASE_MS).
    fn idle(&mut self, ms: u64) {
        let end = Instant::now() + Duration::from_millis(ms);
        while Instant::now() < end {
            self.step();
            std::thread::sleep(Duration::from_millis(5));
        }
    }
}

#[test]
fn a_relay_that_changes_its_key() {
    let dir = Dir::new("swap");
    let a = Relay::start(0, &dir.0.join("a.key"));
    let address = format!("127.0.0.1:{}", a.port);
    let lobby = "l0123456789abcdef0123".to_string();
    let code = encode_lobby_code(&LobbyCode::Relay { lobby, relay: Some(address.clone()) }, None).expect("a code");
    let mut g = Guest::new(&dir.0);

    // a relay not met before: asked, the key it proves kept on the yes
    let (id, what) = g.join_asks(&code);
    assert_eq!(what, AskWhat::NewRelay { address: address.clone() }, "new relay: asked");
    let why = g.answer(id, true);
    assert!(!matches!(why, ClosedWhy::JoinFailed { why: JoinWhy::RelayRefused { .. } }), "new relay: linked ({why:?})");
    let first = KnownRelay { address: address.clone(), id: Some(a.id.clone()) };
    assert_eq!(g.known, vec![first], "new relay: the key it proved kept");

    // the same relay, the same key: no question, nothing kept again
    g.fresh();
    g.take(NetRequest::Open(SessionTarget::Join(code.clone())));
    let why = g.closed();
    assert!(!g.events.iter().any(|e| matches!(e, NetEvent::Ask { .. })), "same key: not asked");
    assert!(!matches!(why, ClosedWhy::JoinFailed { why: JoinWhy::RelayRefused { .. } }), "same key: linked ({why:?})");
    assert_eq!(g.known.len(), 1, "same key: nothing kept again");
    g.take(NetRequest::Close);

    // another key at the address
    let port = a.port;
    let first_id = a.id.clone();
    // (the guest's link to it let go first: a link resuming would be taken again by the next join)
    g.idle(1000);
    a.stop();
    let b = Relay::start(port, &dir.0.join("b.key"));
    assert_ne!(b.id, first_id, "two key files: two keys");

    // asked again; a no keeps it refused and the key as it was
    let (id, what) = g.join_asks(&code);
    assert_eq!(what, AskWhat::RelayKeyChanged { address: address.clone() }, "key changed: asked");
    assert_eq!(
        g.answer(id, false),
        ClosedWhy::JoinFailed { why: JoinWhy::RelayNotTaken { address: address.clone() } },
        "key changed, no: refused"
    );
    assert_eq!(g.known.len(), 1, "key changed, no: nothing kept");
    let (_, what) = g.join_asks(&code);
    assert_eq!(what, AskWhat::RelayKeyChanged { address: address.clone() }, "key changed, no: asked again");
    g.take(NetRequest::Close);

    // a yes joins pinned to the new key and keeps it in place of the old
    let (id, _) = g.join_asks(&code);
    let why = g.answer(id, true);
    assert!(!matches!(why, ClosedWhy::JoinFailed { why: JoinWhy::RelayRefused { .. } }), "key changed, yes: linked");
    assert_eq!(g.known.last(), Some(&KnownRelay { address: address.clone(), id: Some(b.id.clone()) }));
    g.fresh();
    g.take(NetRequest::Open(SessionTarget::Join(code.clone())));
    g.closed();
    assert!(!g.events.iter().any(|e| matches!(e, NetEvent::Ask { .. })), "the new key kept: not asked");
    g.take(NetRequest::Close);
    drop(g);

    // a sign-in pinned to the old key: off as RelayKey, the release's
    let pin = Some(RelayPin { id: first_id.clone(), release: true });
    let (state, known) = sign_in(&dir.0, &address, pin, Vec::new());
    assert_eq!(state, Some(OffWhy::RelayKey { release: true, in_ms: 60_000 }), "pinned to the old key: refused");
    assert!(known.is_empty(), "pinned: nothing kept");
    // one without a pin: signed in, the key kept
    let (state, known) = sign_in(&dir.0, &address, None, Vec::new());
    assert_eq!(state, None, "unpinned: signed in");
    assert_eq!(known, vec![KnownRelay { address: address.clone(), id: Some(b.id.clone()) }], "unpinned: key kept");
    // one without a pin to a relay whose other key is kept: signed in, the key kept left as it is
    let kept = vec![KnownRelay { address: address.clone(), id: Some(first_id) }];
    let (state, known) = sign_in(&dir.0, &address, None, kept);
    assert_eq!(state, None, "unpinned, another key kept: signed in");
    assert!(known.is_empty(), "unpinned, another key kept: never replaced by a sign-in");
    b.stop();
}

/// Signs in to the relay at `address` (`pin`, the relays known before `kept`): its state once it settles (None:
/// online) and the keys kept.
fn sign_in(
    dir: &Path,
    address: &str,
    pin: Option<RelayPin>,
    kept: Vec<KnownRelay>,
) -> (Option<OffWhy>, Vec<KnownRelay>) {
    let epoch = Instant::now();
    let now = || epoch.elapsed().as_secs_f64() * 1000.0;
    let mut svc = NetService::new(ServiceOptions { epoch, ..ServiceOptions::default() }, now());
    let mut sign = SignIn::new(SignInOptions { retry_ms: vec![60_000.0], ..SignInOptions::system(epoch) });
    let relay = RelayOptions {
        address: address.into(),
        key_file: dir.join("signin.key"),
        product: "mvs-test".into(),
        version: "0".into(),
        host_key: None,
        pin,
        faults: Default::default(),
    };
    let b = OnlineBackends {
        relay: Some(relay),
        epic: None,
        lan: None,
        allow_direct: true,
        default_relay: None,
        known_relays: kept,
    };
    sign.set(&mut svc, b, now());
    let end = Instant::now() + Duration::from_secs(10);
    let mut known = Vec::new();
    let state = loop {
        svc.pump(now());
        sign.pump(&mut svc, now());
        for n in svc.take_notes() {
            if let ServiceNote::RelayKnown(k) = n {
                known.push(k);
            }
        }
        let s = sign.states().into_iter().find(|s| s.kind == BackendKind::Relay).expect("the relay's state");
        if s.online {
            break None;
        }
        if !matches!(s.why, Some(OffWhy::Connecting { .. })) {
            break s.why;
        }
        assert!(Instant::now() < end, "the sign-in did not settle");
        std::thread::sleep(Duration::from_millis(5));
    };
    for b in sign.quit() {
        let _ = mvs_net::service::lock(&b).shutdown();
    }
    (state, known)
}

/// Joins to one relay share its link (a relay takes one sign-in per key): a join that expects another key than the one
/// the shared link proved is asked, never put on it silently, whether the link is up or still being made.
#[test]
fn a_shared_link_with_another_key() {
    let dir = Dir::new("shared");
    let r = Relay::start(0, &dir.0.join("r.key"));
    let address = format!("127.0.0.1:{}", r.port);
    let lobby = "l0123456789abcdef0123".to_string();
    let code = encode_lobby_code(&LobbyCode::Relay { lobby, relay: Some(address.clone()) }, None).expect("a code");
    let other = "r0123456789abcdef0123456789abcdef".to_string();
    let mut g = Guest::new(&dir.0);
    let config = |id: &str| ServiceConfig {
        known_relays: vec![KnownRelay { address: address.clone(), id: Some(id.to_string()) }],
        ..ServiceConfig::default()
    };

    // up: a join pinned to the relay's key links and ends (no such lobby); its link kept RELEASE_MS, a join expecting
    // another key meanwhile is asked
    let now = g.now();
    g.svc.configure(config(&r.id), now);
    g.fresh();
    g.take(NetRequest::Open(SessionTarget::Join(code.clone())));
    let why = g.closed();
    assert!(!matches!(why, ClosedWhy::JoinFailed { why: JoinWhy::RelayRefused { .. } }), "pinned: linked ({why:?})");
    assert!(!g.events.iter().any(|e| matches!(e, NetEvent::Ask { .. })), "pinned to its key: not asked");
    g.take(NetRequest::Close);
    let now = g.now();
    g.svc.configure(config(&other), now);
    let (_, what) = g.join_asks(&code);
    assert_eq!(what, AskWhat::RelayKeyChanged { address: address.clone() }, "link up, another key expected: asked");
    g.take(NetRequest::Close);
    g.idle(1000);

    // in flight: two pages join at once, the first pinned to the relay's key, the second expecting another
    let first = g.page;
    let now = g.now();
    let second = g.svc.open_page(now);
    g.svc.configure(config(&r.id), now);
    g.fresh();
    g.take(NetRequest::Open(SessionTarget::Join(code.clone())));
    g.page = second;
    let now = g.now();
    g.svc.configure(config(&other), now);
    g.take(NetRequest::Open(SessionTarget::Join(code.clone())));
    let what = g.until("the second page's question", |g| {
        g.events.iter().find_map(|e| match e {
            NetEvent::Ask { what, .. } => Some(what.clone()),
            _ => None,
        })
    });
    assert_eq!(what, AskWhat::RelayKeyChanged { address: address.clone() }, "link in flight, another key: asked");
    // (the first page's join, pinned to the relay's key, may keep it over the config changed under it)
    assert!(g.known.iter().all(|k| k.id.as_ref() == Some(&r.id)), "shared links: only the key proved kept");
    g.take(NetRequest::Close);
    g.page = first;
    g.take(NetRequest::Close);
    g.idle(100);
    drop(g);
    r.stop();
}
