//! The net service's sign-ins: every backend the online config names at once, each on its own.
//! The relay and Epic are signed in to and handed to the sessions (NetService::start_shared), LAN signs in to nothing
//! (NetService::start_lan); each one's state is kept for the owner on each change. A change of one backend's options
//! ends that one's sessions (stop_backend: their byes go out on it, and it shuts down BYE_MS + 100 later) and signs in
//! to it again; the others are left as they are. Epic that does not sign in, or signs out, stays off until its options
//! change; a relay that cannot be reached or drops is tried again RETRY_MS later (the last one over and over), and its
//! first success starts the waits again. One sign-in at a time per backend: its shutdowns and sign-ins run one after
//! another, a change cancels a relay's connect in flight, and a sign-in that comes back for options no longer in the
//! config is shut down, never kept.
//!
//! Sans-IO as the service: each call is given the clock's `now`, `pump(now)` runs at `next_due()`. A sign-in is a
//! `Load` the loaders start (a relay's connect on a thread of its own by default), polled at each pump.
//!
//! A backend shut down (its options changed, signed out, or a sign-in no longer wanted) is polled on at its next_due
//! while it has one: EOS's parked platform still answers its leaves, refuses connection requests and renews its
//! sign-in only inside the ticks its polls make (eos.rs), until a sign-in of its kind takes the platform over.
//!
//! The relay's key: a sign-in with a pin (`RelayOptions::pin`) fails on a relay that proves another key, off as
//! `OffWhy::RelayKey` and tried again at the same pace; one without a pin keeps the key the relay proved
//! (`SignInService::know_relay`, the app's next config pins it), except in a dev run (`SignInOptions::record_relay`).

use std::collections::VecDeque;
use std::sync::{Arc, Mutex};
use std::time::Instant;

use mvs_protocol::relay_wire::KnownRelay;

use crate::backend::eos::EosOptions;
use crate::backend::{Backend, BackendEvent, BackendKind};
use crate::link::relay::key::RelayKey;
use crate::link::relay::{ClientOptions, NetFaults, RelayClient, RelayError, StartWith};
use crate::pending::Pending;
use crate::service::api::{BackendState, EpicBackend, EpicMissing, LanOptions, OffWhy, OnlineBackends, RelayOptions};
use crate::service::{BYE_MS, NetService, ServiceConfig, Shared, lock};

/// A relay's next try after a failed one: 2, 5, 10, then every 30 s.
pub const RETRY_MS: [f64; 4] = [2000.0, 5000.0, 10000.0, 30000.0];

/// A sign-in under way: its backend once it is signed in, or why not.
pub trait Load: Send {
    /// None: not yet.
    fn poll(&mut self, now: f64) -> Option<Result<Box<dyn Backend>, String>>;
    /// No more wanted (a relay's connect in flight stops where it can).
    fn cancel(&mut self) {}
    /// When `poll` has something to do next (the service's clock, ms); by default at once.
    fn next_due(&self) -> Option<f64> {
        Some(f64::NEG_INFINITY)
    }
    /// A relay's sign-in, once `poll` gave its backend: the id of the key the relay proved (`relay_wire::key_id`).
    fn relay_id(&self) -> Option<String> {
        None
    }
}

/// Starts the sign-ins (tests give their own).
pub trait Loaders: Send {
    fn relay(&mut self, o: &RelayOptions, now: f64) -> Box<dyn Load>;
    fn epic(&mut self, o: &EosOptions, now: f64) -> Box<dyn Load>;
}

/// A relay's connect on a worker (link/relay.rs begin_connect): dropped, it is cancelled and a client it still makes is
/// shut down. `proved`: the id of the key the relay proved, once it is up.
struct Connecting(Pending<Result<RelayClient, RelayError>>, Option<String>);

impl Load for Connecting {
    fn poll(&mut self, _now: f64) -> Option<Result<Box<dyn Backend>, String>> {
        let r = self.0.poll()?;
        Some(match r {
            Ok(c) => {
                self.1 = Some(c.relay_id());
                Ok(Box::new(c) as Box<dyn Backend>)
            }
            Err(e) => Err(e.to_string()),
        })
    }
    fn cancel(&mut self) {
        self.0.cancel();
    }
    fn relay_id(&self) -> Option<String> {
        self.1.clone()
    }
}

/// A load that is over at once.
struct Ready(Option<Result<Box<dyn Backend>, String>>);

impl Load for Ready {
    fn poll(&mut self, _now: f64) -> Option<Result<Box<dyn Backend>, String>> {
        self.0.take()
    }
}

/// A backend loaded signing in (eos.rs load_eos): the load holds until its `SignedIn`, or fails with its
/// `SignedOut`.
pub struct SigningIn(pub Option<Box<dyn Backend>>);

impl Load for SigningIn {
    fn poll(&mut self, now: f64) -> Option<Result<Box<dyn Backend>, String>> {
        loop {
            match self.0.as_mut()?.poll(now)? {
                BackendEvent::SignedIn => return self.0.take().map(Ok),
                BackendEvent::SignedOut(why) => {
                    self.0 = None;
                    return Some(Err(why));
                }
                // (nothing else comes before the sign-in: no lobby, no link)
                _ => {}
            }
        }
    }
    fn cancel(&mut self) {
        if let Some(mut b) = self.0.take() {
            let _ = b.shutdown();
        }
    }
    // (EOS signs in through the ticks its polls make: at its next_due)
    fn next_due(&self) -> Option<f64> {
        self.0.as_ref().and_then(|b| b.next_due())
    }
}

/// The app's loaders: a relay's connect on a thread (RelayClient), Epic's sign-in on the service's thread (load_eos).
pub struct SystemLoaders {
    /// The service's clock's zero (the relay client's).
    pub epoch: Instant,
}

impl Loaders for SystemLoaders {
    fn relay(&mut self, o: &RelayOptions, _now: f64) -> Box<dyn Load> {
        let key = match RelayKey::load(&o.key_file) {
            Ok(k) => k,
            Err(e) => return Box::new(Ready(Some(Err(e.to_string())))),
        };
        let mut c = ClientOptions::new(&format!("{} {}", o.product, o.version), self.epoch);
        c.faults = o.faults;
        let x = StartWith { relay_id: o.pin.as_ref().map(|p| p.id.clone()), ..StartWith::default() };
        Box::new(Connecting(RelayClient::begin_connect(o.address.clone(), key, c, x), None))
    }

    fn epic(&mut self, o: &EosOptions, _now: f64) -> Box<dyn Load> {
        match crate::backend::eos::load_eos(o) {
            Ok(b) => Box::new(SigningIn(Some(b))),
            Err(e) => Box::new(Ready(Some(Err(e)))),
        }
    }
}

/// The calls of the sessions' service a sign-in makes (a test's fake may stand in).
pub trait SignInService {
    fn start_backend(&mut self, kind: BackendKind, b: Shared, now: f64);
    fn stop_backend(&mut self, kind: BackendKind, why: OffWhy, now: f64);
    fn start_lan(&mut self, o: LanOptions, now: f64);
    fn stop_lan(&mut self, why: OffWhy, now: f64);
    fn configure(&mut self, c: ServiceConfig, now: f64);
    fn backend_state(&self, kind: BackendKind) -> (bool, Option<OffWhy>);
    /// The config's relay, signed in to without a pin, proved that key: kept as a known relay
    /// (`ServiceNote::RelayKnown`) unless one is kept for that address already (a sign-in never replaces a key kept:
    /// only the player's yes to a code's `AskWhat::RelayKeyChanged` does).
    fn know_relay(&mut self, _k: KnownRelay) {}
}

impl SignInService for NetService {
    fn start_backend(&mut self, kind: BackendKind, b: Shared, now: f64) {
        self.start_shared(kind, b, now);
    }
    fn stop_backend(&mut self, kind: BackendKind, why: OffWhy, now: f64) {
        NetService::stop_backend(self, kind, why, now);
    }
    fn start_lan(&mut self, o: LanOptions, now: f64) {
        NetService::start_lan(self, o, now);
    }
    fn stop_lan(&mut self, why: OffWhy, now: f64) {
        NetService::stop_lan(self, why, now);
    }
    fn configure(&mut self, c: ServiceConfig, now: f64) {
        NetService::configure(self, c, now);
    }
    fn backend_state(&self, kind: BackendKind) -> (bool, Option<OffWhy>) {
        NetService::backend_state(self, kind)
    }
    fn know_relay(&mut self, k: KnownRelay) {
        if self.known_relay(&k.address).is_some_and(|r| r.id.is_some()) {
            return;
        }
        NetService::know_relay(self, k);
    }
}

/// How a SignIn is made.
pub struct SignInOptions {
    pub loaders: Box<dyn Loaders>,
    /// The relay's waits (tests).
    pub retry_ms: Vec<f64>,
    /// The key a relay signed in to without a pin proved is kept (`SignInService::know_relay`); false in a dev run,
    /// whose relay is a scratch one.
    pub record_relay: bool,
}

impl SignInOptions {
    /// The app's: its loaders, the build's waits, the relay's key kept.
    pub fn system(epoch: Instant) -> SignInOptions {
        SignInOptions { loaders: Box::new(SystemLoaders { epoch }), retry_ms: RETRY_MS.to_vec(), record_relay: true }
    }
}

/// A backend's options as the lane holds them.
#[derive(Clone, Debug, PartialEq)]
enum Signed {
    Relay(RelayOptions),
    Epic(EpicBackend),
}

/// A lane's steps, each started once the one before has settled.
enum Step {
    /// The old backend shut down then (after its sessions' byes).
    Shutdown(Shared, f64),
    /// A sign-in of the options of that number.
    SignIn(u32),
}

/// One backend that signs in (the relay or Epic).
struct Lane {
    b: Option<Signed>,
    eos: Option<Shared>,
    /// The service has this backend (start called, no stop since).
    started: bool,
    /// The id of the key the relay signed in to proved (None: not signed in, or Epic).
    proved: Option<String>,
    /// The options' number: a sign-in of older ones is let go.
    generation: u32,
    tries: usize,
    retry: Option<(f64, u32)>,
    steps: VecDeque<Step>,
    loading: Option<(u32, Box<dyn Load>)>,
    state: BackendState,
}

impl Lane {
    fn new(kind: BackendKind) -> Lane {
        Lane {
            b: None,
            eos: None,
            started: false,
            proved: None,
            generation: 0,
            tries: 0,
            retry: None,
            steps: VecDeque::new(),
            loading: None,
            state: BackendState { kind, on: false, online: false, why: Some(OffWhy::Off) },
        }
    }

    /// No sign-in of these options from here on (a retry waiting, a connect in flight).
    fn cancel(&mut self) {
        self.generation += 1;
        self.tries = 0;
        self.retry = None;
        if let Some((_, l)) = self.loading.as_mut() {
            l.cancel();
        }
    }
}

/// The sign-ins.
pub struct SignIn {
    o: SignInOptions,
    relay: Lane,
    epic: Lane,
    lan_options: Option<LanOptions>,
    lan: BackendState,
    last: Option<Vec<BackendState>>,
    posts: Vec<Vec<BackendState>>,
    quitting: bool,
    /// The backends shut down, polled at their next_due while they have one (EOS's parked platform: its leaves
    /// answered, requests refused, its sign-in renewed) until a sign-in of their kind takes over.
    parked: Vec<(BackendKind, Shared)>,
}

fn signing_why(b: &Signed) -> OffWhy {
    match b {
        Signed::Relay(o) => OffWhy::Connecting { address: o.address.clone() },
        Signed::Epic(_) => OffWhy::SigningIn,
    }
}

impl SignIn {
    pub fn new(o: SignInOptions) -> SignIn {
        SignIn {
            o,
            relay: Lane::new(BackendKind::Relay),
            epic: Lane::new(BackendKind::Epic),
            lan_options: None,
            lan: BackendState { kind: BackendKind::Lan, on: false, online: false, why: Some(OffWhy::Off) },
            last: None,
            posts: Vec::new(),
            quitting: false,
            parked: Vec::new(),
        }
    }

    /// A backend's signed-in interface.
    pub fn current(&self, kind: BackendKind) -> Option<Shared> {
        match kind {
            BackendKind::Relay => self.relay.eos.clone(),
            BackendKind::Epic => self.epic.eos.clone(),
            BackendKind::Lan => None,
        }
    }

    /// Every backend's state, the relay, Epic and LAN in that order.
    pub fn states(&self) -> Vec<BackendState> {
        vec![self.relay.state.clone(), self.epic.state.clone(), self.lan.clone()]
    }

    /// Every change of the states since the last call (each the whole list).
    pub fn take_posts(&mut self) -> Vec<Vec<BackendState>> {
        std::mem::take(&mut self.posts)
    }

    fn lane(&mut self, kind: BackendKind) -> &mut Lane {
        if kind == BackendKind::Epic { &mut self.epic } else { &mut self.relay }
    }

    fn post(&mut self) {
        let s = self.states();
        if self.last.as_ref() == Some(&s) {
            return;
        }
        self.last = Some(s.clone());
        self.posts.push(s);
    }

    /// Signs in to each backend of `b` whose options changed, and off the ones it no longer has.
    pub fn set(&mut self, svc: &mut dyn SignInService, b: OnlineBackends, now: f64) {
        if self.quitting {
            return;
        }
        // (before any backend starts: the sessions read the config's relay and the known ones as the relay signs in)
        svc.configure(
            ServiceConfig {
                relay_faults: b.relay.as_ref().map_or_else(NetFaults::default, |r| r.faults),
                relay: b.relay.as_ref().map(|r| r.address.clone()),
                allow_direct: b.allow_direct,
                default_relay: b.default_relay.clone(),
                known_relays: b.known_relays.clone(),
            },
            now,
        );
        self.set_lane(svc, BackendKind::Relay, b.relay.map(Signed::Relay), now);
        self.set_lane(svc, BackendKind::Epic, b.epic.map(Signed::Epic), now);
        self.set_lan(svc, b.lan, now);
        self.post();
        self.pump(svc, now);
    }

    /// The app's quit: no sign-in from here on; the backends to shut down (their sessions ended first).
    pub fn quit(&mut self) -> Vec<Shared> {
        self.quitting = true;
        let mut out = Vec::new();
        for l in [&mut self.relay, &mut self.epic] {
            l.cancel();
            l.loading = None;
            l.steps.clear();
            out.extend(l.eos.clone());
        }
        out
    }

    fn set_state(&mut self, kind: BackendKind, online: bool, why: Option<OffWhy>) {
        let l = self.lane(kind);
        l.state = BackendState { kind, on: l.b.is_some(), online, why };
        self.post();
    }

    /// The backend's sessions end (when the service has it) and it says why.
    fn off(&mut self, svc: &mut dyn SignInService, kind: BackendKind, why: OffWhy, now: f64) {
        let l = self.lane(kind);
        let started = std::mem::take(&mut l.started);
        if started {
            svc.stop_backend(kind, why.clone(), now);
        }
        self.set_state(kind, false, Some(why));
    }

    fn set_lane(&mut self, svc: &mut dyn SignInService, kind: BackendKind, b: Option<Signed>, now: f64) {
        let l = self.lane(kind);
        if l.b == b {
            return;
        }
        // (the relay signed in to proved the key the new pin names, its other options the same: the pin is kept for
        // the next sign-in, this one stays; the key an unpinned sign-in recorded comes back as the pin this way)
        if let (Some(Signed::Relay(was)), Some(Signed::Relay(new))) = (&l.b, &b)
            && l.eos.is_some()
            && new.pin.as_ref().is_none_or(|p| l.proved.as_ref() == Some(&p.id))
            && (RelayOptions { pin: None, ..was.clone() }) == (RelayOptions { pin: None, ..new.clone() })
        {
            l.b = b;
            return;
        }
        l.b = b.clone();
        l.cancel();
        let g = l.generation;
        let old = l.eos.take();
        let started = std::mem::take(&mut l.started);
        if started {
            svc.stop_backend(kind, if b.is_some() { OffWhy::SettingsChanged } else { OffWhy::TurnedOff }, now);
        }
        self.set_state(kind, false, Some(b.as_ref().map_or(OffWhy::Off, signing_why)));
        let l = self.lane(kind);
        if let Some(old) = old {
            l.steps.push_back(Step::Shutdown(old, now + BYE_MS + 100.0));
        }
        if b.is_some() {
            l.steps.push_back(Step::SignIn(g));
        }
    }

    /// A backend signed out by itself (the service's note): off, a relay tried again (unless another copy of its key
    /// took the seat).
    pub fn signed_out(&mut self, svc: &mut dyn SignInService, kind: BackendKind, why: &str, now: f64) {
        let l = self.lane(kind);
        let Some(e) = l.eos.take() else { return };
        let g = l.generation;
        self.lost(svc, kind, g, why, now);
        // (let go: a relay's link closed)
        self.let_go(kind, e);
    }

    fn lost(&mut self, svc: &mut dyn SignInService, kind: BackendKind, g: u32, why: &str, now: f64) {
        // (a relay seat another copy of this key took: off, not tried again)
        if kind != BackendKind::Relay || crate::link::relay::taken_elsewhere(why) {
            return self.off(svc, kind, OffWhy::Backend { text: why.to_string() }, now);
        }
        let waits = self.o.retry_ms.clone();
        let l = self.lane(kind);
        let ms = waits[l.tries.min(waits.len() - 1)];
        l.tries += 1;
        l.retry = Some((now + ms, g));
        // (the relay proved another key than its pin's: that reason, tried again at the same pace, as the key the
        // address answers with may come back)
        let pin = match &l.b {
            Some(Signed::Relay(o)) => o.pin.as_ref().map(|p| p.release),
            _ => None,
        };
        let why = match pin {
            Some(release) if crate::link::relay::other_key(why).is_some() => {
                OffWhy::RelayKey { release, in_ms: ms as u32 }
            }
            _ => OffWhy::Retrying { text: why.to_string(), in_ms: ms as u32 },
        };
        self.off(svc, kind, why, now);
    }

    /// Runs what is due by `now`: each lane's steps, its sign-in's answer, its retry.
    pub fn pump(&mut self, svc: &mut dyn SignInService, now: f64) {
        self.poll_parked(now);
        for kind in [BackendKind::Relay, BackendKind::Epic] {
            loop {
                if !self.lane_step(svc, kind, now) {
                    break;
                }
            }
        }
    }

    /// One step of a lane; false when it waits.
    fn lane_step(&mut self, svc: &mut dyn SignInService, kind: BackendKind, now: f64) -> bool {
        let quitting = self.quitting;
        let record = self.o.record_relay;
        let l = self.lane(kind);
        if let Some((g, load)) = l.loading.as_mut() {
            let g = *g;
            let Some(r) = load.poll(now) else { return false };
            let proved = load.relay_id();
            l.loading = None;
            match r {
                Err(e) => {
                    if g == l.generation {
                        self.lost(svc, kind, g, &e, now);
                    }
                }
                Ok(b) => {
                    let b: Shared = Arc::new(Mutex::new(b));
                    // (a sign-in for options no longer in the config: closed before the next step starts)
                    if g != l.generation || quitting {
                        self.let_go(kind, b);
                        return true;
                    }
                    l.eos = Some(b.clone());
                    l.tries = 0;
                    l.started = true;
                    l.proved.clone_from(&proved);
                    // (a relay signed in to without a pin: the key it proved is the one expected of it from now on;
                    // never a dev run's)
                    if let (Some(Signed::Relay(o)), Some(id)) = (&l.b, proved)
                        && o.pin.is_none()
                        && record
                    {
                        svc.know_relay(KnownRelay { address: o.address.clone(), id: Some(id) });
                    }
                    svc.start_backend(kind, b, now);
                    self.set_state(kind, true, None);
                }
            }
            return true;
        }
        if let Some((at, g)) = l.retry
            && at <= now
        {
            l.retry = None;
            if g == l.generation {
                l.steps.push_back(Step::SignIn(g));
            }
        }
        let l = self.lane(kind);
        match l.steps.front() {
            None => false,
            Some(Step::Shutdown(_, at)) => {
                if *at > now {
                    return false;
                }
                if let Some(Step::Shutdown(b, _)) = l.steps.pop_front() {
                    self.let_go(kind, b);
                }
                true
            }
            Some(Step::SignIn(g)) => {
                let g = *g;
                l.steps.pop_front();
                if g == l.generation && !quitting {
                    self.sign_in(kind, g, now);
                }
                true
            }
        }
    }

    /// Shuts a backend down, polled on while it has something to do.
    fn let_go(&mut self, kind: BackendKind, b: Shared) {
        let _ = lock(&b).shutdown();
        self.parked.push((kind, b));
    }

    /// The backends shut down, polled when due (their events are let go: a parked platform tells only its answers),
    /// and dropped once they have nothing more to do.
    fn poll_parked(&mut self, now: f64) {
        self.parked.retain(|(_, b)| {
            let mut b = lock(b);
            if b.next_due().is_some_and(|t| t <= now) {
                while b.poll(now).is_some() {}
            }
            b.next_due().is_some()
        });
    }

    fn sign_in(&mut self, kind: BackendKind, g: u32, now: f64) {
        let Some(b) = self.lane(kind).b.clone() else { return };
        // (a new sign-in of this kind polls the platform a parked one is on: the parked handle is let go, so the two
        // take no events from each other)
        self.parked.retain(|(k, _)| *k != kind);
        self.set_state(kind, false, Some(signing_why(&b)));
        let load: Box<dyn Load> = match &b {
            Signed::Relay(o) => self.o.loaders.relay(o, now),
            Signed::Epic(e) => match &e.options {
                Some(o) => self.o.loaders.epic(o, now),
                None => {
                    let why = e.missing.unwrap_or(EpicMissing::NotInstalled);
                    // (missing: off with the reason, never a retry)
                    self.lane(kind).loading = None;
                    let l = self.lane(kind);
                    l.state = BackendState { kind, on: true, online: false, why: Some(OffWhy::Missing { why }) };
                    self.post();
                    return;
                }
            },
        };
        self.lane(kind).loading = Some((g, load));
    }

    /// When `pump` has something to do next: a step's or a retry's time, a sign-in under way's or a parked backend's
    /// next_due.
    pub fn next_due(&self) -> Option<f64> {
        let parked = self.parked.iter().filter_map(|(_, b)| lock(b).next_due());
        [&self.relay, &self.epic]
            .into_iter()
            .flat_map(|l| {
                let step = match l.steps.front() {
                    Some(Step::Shutdown(_, at)) => Some(*at),
                    Some(Step::SignIn(_)) => Some(f64::NEG_INFINITY),
                    None => None,
                };
                let loading = l.loading.as_ref().and_then(|(_, l)| l.next_due());
                [step, loading, l.retry.map(|r| r.0)]
            })
            .flatten()
            .chain(parked)
            .reduce(f64::min)
    }

    /// LAN signs in to nothing: on at once with its options, unless the install's key cannot be read.
    fn set_lan(&mut self, svc: &mut dyn SignInService, o: Option<LanOptions>, now: f64) {
        if self.lan_options == o {
            return;
        }
        let was = self.lan_options.is_some();
        self.lan_options = o.clone();
        if was {
            svc.stop_lan(if o.is_some() { OffWhy::LanPortChanged } else { OffWhy::TurnedOff }, now);
        }
        if let Some(o) = o {
            svc.start_lan(o, now);
        }
        self.lan_follows(svc);
    }

    /// LAN's state from the service's (off when it is not in the config): on the service's note of a change.
    pub fn lan_follows(&mut self, svc: &dyn SignInService) {
        self.lan = if self.lan_options.is_none() {
            BackendState { kind: BackendKind::Lan, on: false, online: false, why: Some(OffWhy::Off) }
        } else {
            let (online, why) = svc.backend_state(BackendKind::Lan);
            let why = if online { None } else { Some(why.filter(|w| *w != OffWhy::Off).unwrap_or(OffWhy::LanFailed)) };
            BackendState { kind: BackendKind::Lan, on: true, online, why }
        };
        self.post();
    }
}
