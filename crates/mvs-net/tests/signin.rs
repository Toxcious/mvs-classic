//! The net service's sign-ins (service/signin.rs) against the oracle's sign-in test: every backend at once over a fake
//! of the sessions' service and fake loads of a relay and of Epic (a relay's connect over 5 ms, Epic's over 15 ms, the
//! retry waits 40 and 80 ms), on a clock the test steps 1 ms at a time. Two backends on together, one that drops
//! leaving the other as it is, a relay's retries, a change of one backend's options, LAN on, moved and off, the quit.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};

use indexmap::IndexSet;

use mvs_net::backend::eos::{EosFiles, EosOptions};
use mvs_net::backend::{Ask, Backend, BackendEvent, BackendKind, Channel, Listed, Presence, RelayMode};
use mvs_net::link::relay::RelayError;
use mvs_net::service::api::{
    BackendState, DirectOptions, EpicBackend, EpicMissing, LanOptions, OffWhy, OnlineBackends, RelayOptions, RelayPin,
};
use mvs_net::service::signin::{Load, Loaders, SignIn, SignInOptions, SignInService};
use mvs_net::service::{BYE_MS, ServiceConfig, Shared, lock};
use mvs_protocol::relay_wire::KnownRelay;

/// What the sign-in asked of the service, in order.
#[derive(Clone, Debug, PartialEq)]
enum Call {
    Start(BackendKind, String),
    Stop(BackendKind, OffWhy),
    StartLan(u16),
    StopLan(OffWhy),
    Configure(ServiceConfig),
}

/// The sessions' service as a list of the calls it got.
#[derive(Default)]
struct FakeService {
    calls: Vec<Call>,
    lan: Option<LanOptions>,
    /// LAN cannot start (the install's key unreadable).
    lan_fails: String,
    /// The relays the sign-in kept the key of.
    known: Vec<KnownRelay>,
}

impl FakeService {
    fn of_start(&self, kind: BackendKind) -> usize {
        self.calls.iter().filter(|c| matches!(c, Call::Start(k, _) if *k == kind)).count()
    }
    fn stops(&self, kind: BackendKind) -> Vec<OffWhy> {
        self.calls
            .iter()
            .filter_map(|c| match c {
                Call::Stop(k, w) if *k == kind => Some(w.clone()),
                _ => None,
            })
            .collect()
    }
    fn lan_calls(&self) -> Vec<Call> {
        self.calls.iter().filter(|c| matches!(c, Call::StartLan(_) | Call::StopLan(_))).cloned().collect()
    }
    fn not_configure(&self) -> usize {
        self.calls.iter().filter(|c| !matches!(c, Call::Configure(_))).count()
    }
}

impl SignInService for FakeService {
    fn start_backend(&mut self, kind: BackendKind, b: Shared, _now: f64) {
        let me = lock(&b).me().to_string();
        self.calls.push(Call::Start(kind, me));
    }
    fn stop_backend(&mut self, kind: BackendKind, why: OffWhy, _now: f64) {
        self.calls.push(Call::Stop(kind, why));
    }
    fn start_lan(&mut self, o: LanOptions, _now: f64) {
        self.calls.push(Call::StartLan(o.port));
        self.lan = if self.lan_fails.is_empty() { Some(o) } else { None };
    }
    fn stop_lan(&mut self, why: OffWhy, _now: f64) {
        self.calls.push(Call::StopLan(why));
        self.lan = None;
    }
    fn configure(&mut self, c: ServiceConfig, _now: f64) {
        self.calls.push(Call::Configure(c));
    }
    fn know_relay(&mut self, k: KnownRelay) {
        self.known.push(k);
    }
    fn backend_state(&self, kind: BackendKind) -> (bool, Option<OffWhy>) {
        let online = kind == BackendKind::Lan && self.lan.is_some();
        if online {
            (true, None)
        } else if kind == BackendKind::Lan && !self.lan_fails.is_empty() {
            (false, Some(OffWhy::Backend { text: self.lan_fails.clone() }))
        } else {
            (false, Some(OffWhy::Off))
        }
    }
}

/// A backend's interface as a load gives it: `shut` says it was shut down.
struct FakeEos {
    me: String,
    kind: BackendKind,
    shut: Arc<AtomicBool>,
}

impl Backend for FakeEos {
    fn me(&self) -> &str {
        &self.me
    }
    fn kind(&self) -> BackendKind {
        self.kind
    }
    fn create_lobby(&mut self, _bucket: &str, _max: u32, _listed: Option<Listed>) -> Ask {
        Ask(0)
    }
    fn search_lobbies(&mut self, _bucket: &str, _limit: u32, _full: bool) -> Ask {
        Ask(0)
    }
    fn set_listed(&mut self, _id: &str, _listed: Option<Listed>) -> Ask {
        Ask(0)
    }
    fn join_lobby(&mut self, _id: &str) -> Ask {
        Ask(0)
    }
    fn leave_lobby(&mut self, _id: &str) -> Ask {
        Ask(0)
    }
    fn kick(&mut self, _lobby: &str, _member: &str) {}
    fn send(&mut self, _to: &str, _channel: Channel, _bytes: &[u8]) -> bool {
        false
    }
    fn close_peer(&mut self, _peer: &str) {}
    fn relayed(&self, _peer: &str) -> Option<bool> {
        None
    }
    fn linked_peers(&self) -> Vec<String> {
        Vec::new()
    }
    fn set_relay_mode(&mut self, _mode: RelayMode) -> Result<(), String> {
        Ok(())
    }
    fn set_accept(&mut self, _ids: IndexSet<String>) {}
    fn invite(&mut self, _to: &str, _payload: &str) -> Ask {
        Ask(0)
    }
    fn presence(&mut self) -> Option<&mut dyn Presence> {
        None
    }
    fn poll(&mut self, _now: f64) -> Option<BackendEvent> {
        None
    }
    fn shutdown(&mut self) -> Ask {
        self.shut.store(true, Ordering::SeqCst);
        Ask(0)
    }
}

/// The loads' side: relay loads fail while `relay_down`, Epic's with `epic_fails`; the relays prove the key
/// `relay_key` (a pinned load of another fails as the start's OtherKey).
#[derive(Default)]
struct Made {
    relay_loads: usize,
    epic_loads: usize,
    relay_down: bool,
    relay_key: String,
    epic_fails: String,
    relays: Vec<(String, Arc<AtomicBool>)>,
    epics: Vec<(String, Arc<AtomicBool>)>,
}

type Rig = Arc<Mutex<Made>>;

fn made(r: &Rig) -> MutexGuard<'_, Made> {
    r.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// A load over at `at` (it ignores a cancel, as the oracle's promise does: the sign-in shuts a late one down).
struct FakeLoad {
    at: f64,
    rig: Rig,
    /// The relay's address, or None for Epic.
    relay: Option<String>,
    /// The key id a relay's load must see proved.
    pin: Option<String>,
    /// The key id the relay proved, once up.
    proved: Option<String>,
}

impl Load for FakeLoad {
    fn poll(&mut self, now: f64) -> Option<Result<Box<dyn Backend>, String>> {
        if now < self.at {
            return None;
        }
        self.at = f64::INFINITY;
        let mut m = made(&self.rig);
        let shut = Arc::new(AtomicBool::new(false));
        let (me, kind) = match &self.relay {
            Some(a) => {
                if m.relay_down {
                    return Some(Err(format!("nothing at {a}")));
                }
                if self.pin.as_ref().is_some_and(|p| *p != m.relay_key) {
                    return Some(Err(RelayError::OtherKey { proved: m.relay_key.clone() }.to_string()));
                }
                self.proved = Some(m.relay_key.clone());
                let me = format!("relay@{a}");
                m.relays.push((me.clone(), shut.clone()));
                (me, BackendKind::Relay)
            }
            None => {
                if !m.epic_fails.is_empty() {
                    return Some(Err(m.epic_fails.clone()));
                }
                m.epics.push(("epic".to_string(), shut.clone()));
                ("epic".to_string(), BackendKind::Epic)
            }
        };
        Some(Ok(Box::new(FakeEos { me, kind, shut })))
    }
    fn relay_id(&self) -> Option<String> {
        self.proved.clone()
    }
}

struct FakeLoaders(Rig);

impl Loaders for FakeLoaders {
    fn relay(&mut self, o: &RelayOptions, now: f64) -> Box<dyn Load> {
        made(&self.0).relay_loads += 1;
        let pin = o.pin.as_ref().map(|p| p.id.clone());
        Box::new(FakeLoad { at: now + 5.0, rig: self.0.clone(), relay: Some(o.address.clone()), pin, proved: None })
    }
    fn epic(&mut self, _o: &EosOptions, now: f64) -> Box<dyn Load> {
        made(&self.0).epic_loads += 1;
        Box::new(FakeLoad { at: now + 15.0, rig: self.0.clone(), relay: None, pin: None, proved: None })
    }
}

/// A sign-in over a fake service, on a clock of its own.
struct T {
    sign: SignIn,
    svc: FakeService,
    rig: Rig,
    now: f64,
}

impl T {
    fn new() -> T {
        T::with_record(true)
    }
    /// `record`: a release run's sign-in (it keeps an unpinned relay's key), else a dev run's.
    fn with_record(record_relay: bool) -> T {
        let rig: Rig = Arc::default();
        made(&rig).relay_key = "r0".into();
        let loaders = Box::new(FakeLoaders(rig.clone()));
        let sign = SignIn::new(SignInOptions { loaders, retry_ms: vec![40.0, 80.0], record_relay });
        T { sign, svc: FakeService::default(), rig, now: 1000.0 }
    }
    fn set(&mut self, b: OnlineBackends) {
        self.sign.set(&mut self.svc, b, self.now);
    }
    fn state(&self, kind: BackendKind) -> BackendState {
        self.sign.states().into_iter().find(|s| s.kind == kind).expect("a state per backend")
    }
    fn online(&self, kind: BackendKind) -> bool {
        self.state(kind).online
    }
    fn why(&self, kind: BackendKind) -> Option<OffWhy> {
        self.state(kind).why
    }
    /// Steps the clock 1 ms at a time, each with a pump, until `cond` or `ms` are over; whether `cond` holds.
    fn wait_for(&mut self, ms: f64, cond: impl Fn(&T) -> bool) -> bool {
        let end = self.now + ms;
        while !cond(self) && self.now < end {
            self.now += 1.0;
            self.sign.pump(&mut self.svc, self.now);
        }
        cond(self)
    }
    fn sleep(&mut self, ms: f64) {
        self.wait_for(ms, |_| false);
    }
    fn current_me(&self, kind: BackendKind) -> Option<String> {
        self.sign.current(kind).map(|b| lock(&b).me().to_string())
    }
    fn signed_out(&mut self, kind: BackendKind, why: &str) {
        self.sign.signed_out(&mut self.svc, kind, why, self.now);
    }
}

fn relay_of(address: &str) -> RelayOptions {
    RelayOptions {
        address: address.to_string(),
        key_file: Default::default(),
        product: "test".into(),
        version: "test".into(),
        host_key: None,
        pin: None,
        faults: Default::default(),
    }
}

fn epic_on() -> EpicBackend {
    let files = EosFiles { dll: "x".into(), credentials: "x".into(), cache: "x".into() };
    EpicBackend {
        options: Some(EosOptions { files, product: "test".into(), version: "test".into(), ..Default::default() }),
        missing: None,
    }
}

fn lan_of(port: u16) -> LanOptions {
    LanOptions {
        direct: DirectOptions {
            key_file: Default::default(),
            product: "test".into(),
            version: "test".into(),
            loopback: true,
        },
        port,
    }
}

fn config() -> OnlineBackends {
    OnlineBackends {
        relay: None,
        epic: None,
        lan: None,
        allow_direct: true,
        default_relay: None,
        known_relays: Vec::new(),
    }
}

fn shut(v: &[(String, Arc<AtomicBool>)], i: usize) -> bool {
    v[i].1.load(Ordering::SeqCst)
}

const RELAY: BackendKind = BackendKind::Relay;
const EPIC: BackendKind = BackendKind::Epic;
const LAN: BackendKind = BackendKind::Lan;

#[test]
fn two_backends_at_once() {
    let mut t = T::new();
    let both = || OnlineBackends {
        relay: Some(relay_of("a:1")),
        epic: Some(epic_on()),
        allow_direct: false,
        default_relay: Some("a:1".into()),
        known_relays: vec![KnownRelay { address: "b:2".into(), id: None }],
        ..config()
    };
    t.set(both());
    assert_eq!(
        t.svc.calls.first(),
        Some(&Call::Configure(ServiceConfig {
            relay_faults: Default::default(),
            relay: Some("a:1".into()),
            allow_direct: false,
            default_relay: Some("a:1".into()),
            known_relays: vec![KnownRelay { address: "b:2".into(), id: None }],
        })),
        "config: the sessions are told the relay, direct links, the default relay and the known relays first"
    );
    assert!(
        t.state(RELAY).on && t.why(RELAY) == Some(OffWhy::Connecting { address: "a:1".into() }),
        "both: signing in"
    );
    assert!(t.state(EPIC).on && !t.state(LAN).on, "both: Epic signing in, LAN off");
    assert!(t.wait_for(3000.0, |t| t.online(RELAY) && t.online(EPIC)), "both: signed in together");
    assert_eq!((t.svc.of_start(RELAY), t.svc.of_start(EPIC)), (1, 1));
    assert!(!t.svc.calls.iter().any(|c| matches!(c, Call::Stop(..))));
    assert_eq!(t.current_me(RELAY).as_deref(), Some("relay@a:1"), "each backend's interface is its own");
    assert_eq!(t.current_me(EPIC).as_deref(), Some("epic"));

    // the relay drops: Epic is left as it is, the relay tries again on its own waits
    made(&t.rig).relay_down = true;
    let loads = made(&t.rig).relay_loads;
    t.signed_out(RELAY, "the relay closed the connection");
    assert_eq!(t.svc.stops(RELAY).len(), 1, "relay drops: its sessions end");
    assert_eq!(
        t.why(RELAY),
        Some(OffWhy::Retrying { text: "the relay closed the connection".into(), in_ms: 40 }),
        "relay drops: the reason and the next try"
    );
    assert!(!t.online(RELAY));
    assert!(
        t.svc.stops(EPIC).is_empty() && t.online(EPIC) && !shut(&made(&t.rig).epics, 0),
        "relay drops: Epic untouched"
    );
    let retry = OffWhy::Retrying { text: "nothing at a:1".into(), in_ms: 80 };
    assert!(t.wait_for(1000.0, |t| t.why(RELAY) == Some(retry.clone())), "relay retry: the next wait longer");
    assert_eq!(made(&t.rig).relay_loads, loads + 1, "relay retry: tried again after its wait");
    assert_eq!(t.svc.stops(RELAY).len(), 1, "relay retry: a failed try ends no sessions again");
    made(&t.rig).relay_down = false;
    assert!(t.wait_for(1000.0, |t| t.online(RELAY)), "relay retry: back once the relay answers");
    assert_eq!(t.svc.of_start(RELAY), 2);
    assert!(shut(&made(&t.rig).relays, 0));
    assert_eq!(t.svc.of_start(EPIC), 1);

    // Epic signs out: it stays off (no retry), the relay is not touched
    let epic_loads = made(&t.rig).epic_loads;
    t.signed_out(EPIC, "signed out by EOS");
    t.sleep(150.0);
    assert!(!t.online(EPIC), "epic drops: off");
    assert_eq!(t.why(EPIC), Some(OffWhy::Backend { text: "signed out by EOS".into() }), "epic drops: with the reason");
    assert_eq!(made(&t.rig).epic_loads, epic_loads, "epic drops: not tried again");
    assert_eq!(t.svc.stops(EPIC).len(), 1);
    assert!(t.online(RELAY) && t.svc.stops(RELAY).len() == 1, "epic drops: the relay is not touched");

    // the same config again changes nothing
    let calls = t.svc.not_configure();
    t.set(both());
    t.sleep(30.0);
    assert_eq!(t.svc.not_configure(), calls, "the same config: nothing signs in again");
    assert_eq!(made(&t.rig).epic_loads, epic_loads);
    t.sign.quit();
}

#[test]
fn one_backend_changes() {
    let mut t = T::new();
    let with = |relay: &str, epic: bool, lan: Option<u16>| OnlineBackends {
        relay: Some(relay_of(relay)),
        epic: epic.then(epic_on),
        lan: lan.map(lan_of),
        ..config()
    };
    t.set(with("a:1", true, Some(7777)));
    t.wait_for(3000.0, |t| t.online(RELAY) && t.online(EPIC));
    assert!(t.online(LAN) && t.svc.lan_calls() == vec![Call::StartLan(7777)], "lan: on at once with no sign-in");
    t.set(with("c:3", true, Some(7777)));
    assert_eq!(t.svc.stops(RELAY).last(), Some(&OffWhy::SettingsChanged), "relay moved: its sessions end at once");
    assert_eq!(t.why(RELAY), Some(OffWhy::Connecting { address: "c:3".into() }), "relay moved: the new one signing in");
    assert!(!shut(&made(&t.rig).relays, 0), "relay moved: the old link is kept for the byes");
    assert!(t.wait_for(BYE_MS + 1000.0, |t| t.online(RELAY)), "relay moved: the new one signed in");
    assert!(shut(&made(&t.rig).relays, 0), "relay moved: the old link shut down");
    assert_eq!(t.current_me(RELAY).as_deref(), Some("relay@c:3"));
    assert_eq!(made(&t.rig).epic_loads, 1, "relay moved: Epic untouched");
    assert!(t.svc.stops(EPIC).is_empty() && t.svc.lan_calls().len() == 1, "relay moved: Epic and LAN untouched");

    // LAN's port moves, then LAN goes
    t.set(with("c:3", true, Some(7800)));
    assert_eq!(
        t.svc.lan_calls()[1..],
        [Call::StopLan(OffWhy::LanPortChanged), Call::StartLan(7800)],
        "lan moved: stopped and started on the new port"
    );
    let loads = {
        let m = made(&t.rig);
        (m.relay_loads, m.epic_loads)
    };
    assert_eq!(loads, (2, 1), "lan moved: nothing else");
    t.set(with("c:3", true, None));
    assert_eq!(t.svc.calls.last(), Some(&Call::StopLan(OffWhy::TurnedOff)), "lan off: stopped");
    assert!(!t.state(LAN).on && t.why(LAN) == Some(OffWhy::Off), "lan off: its state off");

    // Epic off and on again: shut down after the byes, then signed in again
    t.set(with("c:3", false, None));
    assert_eq!(t.svc.stops(EPIC).last(), Some(&OffWhy::TurnedOff), "epic off: its sessions end");
    assert!(!t.state(EPIC).on, "epic off: its state off");
    t.set(with("c:3", true, None));
    assert!(t.wait_for(BYE_MS + 1000.0, |t| t.online(EPIC)), "epic on again: signed in again");
    {
        let m = made(&t.rig);
        assert!(shut(&m.epics, 0) && !shut(&m.epics, 1), "epic on again: its last shut down first");
        assert_eq!((m.epic_loads, m.relay_loads), (2, 2));
    }
    assert!(t.online(RELAY), "epic on again: the relay untouched");

    // missing files: off with the reason
    let mut c = with("c:3", false, None);
    c.epic = Some(EpicBackend { options: None, missing: Some(EpicMissing::NotInstalled) });
    t.set(c);
    let missing = Some(OffWhy::Missing { why: EpicMissing::NotInstalled });
    assert!(t.wait_for(BYE_MS + 1000.0, |t| t.why(EPIC) == missing), "epic missing: off with the reason");

    // the quit: every signed-in backend to shut down
    let down = t.sign.quit();
    let mes: Vec<String> = down.iter().map(|b| lock(b).me().to_string()).collect();
    assert_eq!(mes, vec!["relay@c:3".to_string()], "quit: the signed-in backends to shut down");
    t.set(with("d:4", false, None));
    t.sleep(100.0);
    assert_eq!(made(&t.rig).relay_loads, 2, "quit: no sign-in after it");
}

#[test]
fn quit_in_a_retry_wait() {
    let mut t = T::new();
    made(&t.rig).relay_down = true;
    t.set(OnlineBackends { relay: Some(relay_of("a:1")), ..config() });
    let first = Some(OffWhy::Retrying { text: "nothing at a:1".into(), in_ms: 40 });
    assert!(t.wait_for(1000.0, |t| t.why(RELAY) == first));
    let loads = made(&t.rig).relay_loads;
    assert!(t.sign.quit().is_empty(), "quit in a retry wait: nothing signed in to shut down");
    made(&t.rig).relay_down = false;
    // (past both waits: a retry the quit did not cancel would have loaded by now)
    t.sleep(250.0);
    assert_eq!(made(&t.rig).relay_loads, loads, "quit in a retry wait: the retry never runs");
    assert_eq!(t.svc.of_start(RELAY), 0);
}

#[test]
fn epic_load_after_epic_off() {
    let mut t = T::new();
    t.set(OnlineBackends { epic: Some(epic_on()), ..config() });
    t.sleep(2.0);
    t.set(config());
    assert!(t.wait_for(1000.0, |t| made(&t.rig).epics.len() == 1));
    t.sleep(50.0);
    assert!(shut(&made(&t.rig).epics, 0), "epic off while loading: the late sign-in shut down");
    assert_eq!(t.svc.of_start(EPIC), 0, "epic off while loading: never handed to the sessions");
    assert!(t.sign.current(EPIC).is_none() && !t.state(EPIC).on);
}

#[test]
fn lan_fails() {
    let mut t = T::new();
    t.svc.lan_fails = "the install's key could not be read".into();
    t.set(OnlineBackends { lan: Some(lan_of(7777)), ..config() });
    let s = t.state(LAN);
    assert!(s.on && !s.online, "lan fails: on in the config, not online");
    assert_eq!(
        s.why,
        Some(OffWhy::Backend { text: "the install's key could not be read".into() }),
        "lan fails: the reason"
    );
    assert!(!t.state(RELAY).on && !t.state(EPIC).on, "lan fails: the others stay off");
}

#[test]
fn fast_relay_changes() {
    let mut t = T::new();
    for i in 0..4 {
        t.set(OnlineBackends { relay: Some(relay_of(&format!("r:{}", i + 1))), epic: Some(epic_on()), ..config() });
        t.sleep(1.0);
    }
    assert!(t.wait_for(3000.0, |t| t.online(RELAY) && t.online(EPIC)));
    t.sleep(50.0);
    assert_eq!(t.current_me(RELAY).as_deref(), Some("relay@r:4"), "fast changes: the newest relay kept");
    let m = made(&t.rig);
    assert_eq!(
        m.relays.iter().filter(|r| !r.1.load(Ordering::SeqCst)).count(),
        1,
        "fast changes: every other shut down"
    );
    assert_eq!(m.epic_loads, 1, "fast changes: Epic signed in once");
    drop(m);
    t.sign.quit();
}

/// A relay seat another copy of this key took ("resumed elsewhere"): the relay stays off with the reason, not tried
/// again (two copies would take the seat from each other).
#[test]
fn a_seat_taken_elsewhere_is_final() {
    let mut t = T::new();
    t.set(OnlineBackends { relay: Some(relay_of("a:1")), ..config() });
    assert!(t.wait_for(3000.0, |t| t.online(RELAY)), "signed in");
    let loads = made(&t.rig).relay_loads;
    let why = "the relay dropped this player: resumed elsewhere";
    t.signed_out(RELAY, why);
    t.sleep(150.0);
    assert!(!t.online(RELAY));
    assert_eq!(t.why(RELAY), Some(OffWhy::Backend { text: why.into() }));
    assert_eq!(made(&t.rig).relay_loads, loads, "not tried again");
    t.sign.quit();
}

fn pinned(address: &str, id: &str, release: bool) -> RelayOptions {
    RelayOptions { pin: Some(RelayPin { id: id.into(), release }), ..relay_of(address) }
}

/// The relay's key: an unpinned sign-in keeps the key the relay proved (a dev run's never), the pin that comes back
/// for it leaves the sign-in as it is, and a pinned sign-in to a relay that proves another key is off with
/// `OffWhy::RelayKey` and tried again at the usual pace, back once the key is the pin's again.
#[test]
fn the_relays_key() {
    let mut t = T::new();
    t.set(OnlineBackends { relay: Some(relay_of("a:1")), ..config() });
    assert!(t.wait_for(3000.0, |t| t.online(RELAY)), "unpinned: signed in");
    assert_eq!(t.svc.known, vec![KnownRelay { address: "a:1".into(), id: Some("r0".into()) }], "unpinned: key kept");

    // the app's next config pins the key kept: nothing signs in again
    let loads = made(&t.rig).relay_loads;
    t.set(OnlineBackends { relay: Some(pinned("a:1", "r0", false)), ..config() });
    t.sleep(50.0);
    assert!(t.online(RELAY) && t.svc.stops(RELAY).is_empty(), "the kept key's pin: the sign-in stays");
    assert_eq!(made(&t.rig).relay_loads, loads, "the kept key's pin: no new sign-in");
    assert_eq!(t.svc.known.len(), 1, "pinned: nothing kept again");

    // another pin: signs in again, the relay proves r0, off as RelayKey, retried at the usual pace
    t.set(OnlineBackends { relay: Some(pinned("a:1", "r9", true)), ..config() });
    let first = Some(OffWhy::RelayKey { release: true, in_ms: 40 });
    assert!(t.wait_for(BYE_MS + 1000.0, |t| t.why(RELAY) == first), "other key: RelayKey, the first wait");
    let second = Some(OffWhy::RelayKey { release: true, in_ms: 80 });
    assert!(t.wait_for(1000.0, |t| t.why(RELAY) == second), "other key: tried again, the next wait");
    assert_eq!(t.svc.known.len(), 1, "other key: nothing kept");
    made(&t.rig).relay_key = "r9".into();
    assert!(t.wait_for(1000.0, |t| t.online(RELAY)), "other key: back once the relay proves the pin's key");
    t.sign.quit();

    // a pin the player said yes to says so
    let mut t = T::new();
    t.set(OnlineBackends { relay: Some(pinned("b:2", "r9", false)), ..config() });
    let why = Some(OffWhy::RelayKey { release: false, in_ms: 40 });
    assert!(t.wait_for(1000.0, |t| t.why(RELAY) == why), "a known relay's pin: release false");
    t.sign.quit();

    // a dev run keeps no key
    let mut t = T::with_record(false);
    t.set(OnlineBackends { relay: Some(relay_of("127.0.0.1:1")), ..config() });
    assert!(t.wait_for(3000.0, |t| t.online(RELAY)));
    assert!(t.svc.known.is_empty(), "dev run: the relay's key not kept");
    t.sign.quit();
}
