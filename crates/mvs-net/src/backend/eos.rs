//! Epic Online Services as a backend over the SDK's DLL (`sdk`): the Device ID sign-in with
//! forced relays, lobbies (created, listed, searched, joined, left, kicked), P2P packets on one socket and custom
//! invites.
//!
//! The SDK is called from the service's thread alone: `load_eos` returns at once with the platform signing in, and
//! everything after (the sign-in's steps, the SDK's callbacks, the ticks) runs inside `poll`, at `next_due`. EOS sends
//! and takes packets only inside EOS_Platform_Tick, so the service polls at the tick rule's times (TICK_MS).
//! Unverified: that the SDK takes its calls from a thread other than the one that loaded it, as long as one thread
//! calls at a time (the service may load on one thread and poll on another).
//!
//! The platform is the process's one: every handle load_eos returns is on it, a poll of any of them ticks it, and its
//! events (the answers included) go to whichever handle polls first. Parked, it still renews its sign-in and refuses
//! connection requests at once, but only while polled: the service keeps polling a parked EOS backend at its
//! next_due (the idle tick) until end_eos. The platform does not tick itself.

mod callbacks;
mod handle;
pub mod sdk;

use std::collections::VecDeque;
use std::ffi::{CStr, CString, c_char, c_void};
use std::path::PathBuf;
use std::ptr::{null, null_mut};
use std::sync::{Arc, Mutex, MutexGuard, OnceLock, PoisonError};
use std::time::{Duration, Instant};

use indexmap::{IndexMap, IndexSet};

use super::{Answer, Ask, Backend, BackendEvent, Channel, FoundLobby, Listed, LobbyChange, P2P_MAX_PACKET, RelayMode};
use callbacks::{
    INBOX, Info, InviteCtx, Note, on_auth_expiration, on_closed, on_create_user, on_established, on_invite,
    on_lobby_id, on_lobby_update, on_login, on_login_status, on_member_status, on_request, on_result,
};
use handle::EosBackend;
use sdk::{Handle, Sdk};

/// Where the SDK and the product's credentials are (the game finds them: beside the exe in a release).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct EosFiles {
    /// EOSSDK-Win64-Shipping.dll.
    pub dll: PathBuf,
    /// A JSON file { productId, sandboxId, deploymentId, clientId, clientSecret }: never in the repo, never logged.
    pub credentials: PathBuf,
    /// The SDK's cache folder.
    pub cache: PathBuf,
}

/// What the EOS backend is loaded with: the files, the product name and version EOS is told,
/// and the dev runs' switches.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct EosOptions {
    pub files: EosFiles,
    pub product: String,
    pub version: String,
    /// Dev runs only: a new Device ID instead of this Windows user's (a second identity on one PC; the first one's
    /// running session keeps working).
    pub fresh_device: bool,
    /// Dev runs only: a tick on every poll while a link is open, so a round trip is the link's own and not the
    /// ticks' (the service then polls without a pause).
    pub spin: bool,
}

/// The attribute of a listed lobby that orders the searchers (FoundLobby.at).
const ATTR_AT: &str = "at";
/// The attribute a searcher reads of a listed lobby (FoundLobby.info).
const ATTR_INFO: &str = "info";
/// The attribute of the code its host wrote (FoundLobby.code).
const ATTR_CODE: &str = "code";

/// The auth-expiration notice comes about a minute before the token ends: the renewal is tried every
/// RENEW_RETRY_MS until RENEW_WINDOW_MS after it.
const RENEW_RETRY_MS: f64 = 5000.0;
const RENEW_WINDOW_MS: f64 = 50000.0;

/// How long a connection request that the accept set turned down is held, in case the requester becomes a member.
// Unverified: a request can come before the lobby's JOINED notice for its sender; 5 s is a guess at the longest gap.
const REQUEST_HOLD_MS: f64 = 5000.0;

/// The ticking: EOS sends and takes packets only inside EOS_Platform_Tick.
// Unverified: while a link is open or a packet moved in the last ACTIVE_MS, a tick every TICK_MS with the timer
// resolution raised to 1 ms (winmm timeBeginPeriod), one at the next poll after each send, and every waiting packet
// read each tick; else a tick every IDLE_TICK_MS. Measured on one PC only: the round trip
// about
// 1.5 ms over a tick on every turn, for about 5 % of a core.
const TICK_MS: f64 = 1.0;
const IDLE_TICK_MS: f64 = 16.0;
const ACTIVE_MS: f64 = 1000.0;

/// How long a shutdown waits for EOS's answers to its lobby leaves.
const PARK_WAIT_MS: f64 = 3000.0;

// ---- the process's one platform

// EOS_Initialize may be called once per process, and EOS_Shutdown ends it for good. So the platform, once made, is
// kept for the process's life: a backend's shutdown (a switch to another backend) parks it (its lobbies left, its
// links closed, nothing taken), a later load_eos reopens it (signed in again on the same platform when the sign-in
// was lost meanwhile), and only end_eos (the program's quit) releases it and shuts the SDK down.
struct Global {
    initialized: bool,
    ended: bool,
    /// The platform made, parked or in use (one whose first sign-in failed is dead and made again).
    made: Option<Arc<Mutex<Platform>>>,
}

static GLOBAL: Mutex<Global> = Mutex::new(Global { initialized: false, ended: false, made: None });
/// The SDK, bound once per process and never unloaded.
static SDK: OnceLock<Sdk> = OnceLock::new();

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Signs in with this Windows user's Device ID (made on its first launch) and forced relays. Returns at once: the
/// backend signs in through its polls and tells `SignedIn`, or `SignedOut` with the reason it failed. Err: the reason
/// online is off before any sign-in (no credentials, the DLL missing or not loaded, EOS refused the platform).
pub fn load_eos(o: &EosOptions) -> Result<Box<dyn Backend>, String> {
    let mut g = lock(&GLOBAL);
    if g.ended {
        return Err("EOS was shut down in this process".to_owned());
    }
    if let Some(made) = g.made.clone() {
        let mut p = lock(&made);
        // A second load is refused while the first signs in (its SignedIn goes to the first handle's polls).
        if p.phase == Phase::SigningIn {
            return Err("EOS is signing in already".to_owned());
        }
        if p.phase != Phase::Dead {
            // Deviation: options that differ from the first load's (another cache folder, product or version) are
            // not applied: the platform stays the one made first.
            if p.sdk.path != o.files.dll {
                return Err("EOS is bound to another DLL in this process".to_owned());
            }
            p.reopen();
            let me = p.me.clone();
            drop(p);
            return Ok(Box::new(EosBackend { p: made, me }));
        }
    }
    let p = create(o, &mut g)?;
    let made = Arc::new(Mutex::new(p));
    g.made = Some(made.clone());
    Ok(Box::new(EosBackend { p: made, me: String::new() }))
}

/// The program's quit: the platform (parked or in use) released and the SDK shut down; no load_eos after it. The
/// service calls it once its backend's shutdown is answered; a platform not parked yet is parked here first, waiting
/// for its lobby leaves (PARK_WAIT_MS at the most).
pub fn end_eos() {
    let mut g = lock(&GLOBAL);
    if g.ended {
        return;
    }
    let made = g.made.take();
    match made.as_ref().map(|m| lock(m)) {
        Some(mut p) if p.phase != Phase::Dead => p.end(),
        _ => {
            if g.initialized
                && let Some(sdk) = SDK.get()
            {
                // SAFETY: EOS_Shutdown takes nothing; the SDK was initialised and no platform of it is in use.
                unsafe { (sdk.shutdown)() };
            }
        }
    }
    g.ended = true;
}

fn read_credentials(path: &std::path::Path) -> Option<serde_json::Value> {
    let text = std::fs::read_to_string(path).ok()?;
    serde_json::from_str(&text).ok()
}

/// The SDK bound from `dll`, once per process.
fn bind(dll: &std::path::Path) -> Result<&'static Sdk, String> {
    if let Some(sdk) = SDK.get() {
        if sdk.path != dll {
            return Err("EOS is bound to another DLL in this process".to_owned());
        }
        return Ok(sdk);
    }
    let sdk = Sdk::load(dll).map_err(|e| format!("the EOS DLL did not load: {e}"))?;
    // (loads are serialised by GLOBAL: the cell is empty here)
    let _ = SDK.set(sdk);
    SDK.get().ok_or_else(|| "the EOS DLL did not load".to_owned())
}

/// A C string of `s` up to its first null (an id or a text the SDK reads).
fn cstr(s: &str) -> CString {
    let s = s.split('\0').next().unwrap_or("");
    CString::new(s).unwrap_or_default()
}

/// A C string's text ('' for null).
///
/// # Safety
/// `p` is null or a null-terminated string that lives through the call.
unsafe fn text_of(p: *const c_char) -> String {
    if p.is_null() {
        return String::new();
    }
    // SAFETY: the caller's.
    unsafe { CStr::from_ptr(p) }.to_string_lossy().into_owned()
}

/// EOS's name of a result (EOS_EResult_ToString).
fn result_text(sdk: &Sdk, r: sdk::EResult) -> String {
    // SAFETY: EOS_EResult_ToString takes any value and returns a static string or null.
    unsafe { text_of((sdk.result_to_string)(r)) }
}

/// A product user id's text ('' for none or one EOS cannot write).
fn id_text(sdk: &Sdk, id: Handle) -> String {
    if id.is_null() {
        return String::new();
    }
    let mut buf = [0 as c_char; sdk::USER_ID_BUFFER + 1];
    let mut len = i32::try_from(buf.len()).unwrap_or(0);
    // SAFETY: `id` is a product user id EOS gave; the buffer and its length are this frame's.
    if unsafe { (sdk.user_id_to_string)(id, buf.as_mut_ptr(), &mut len) } != sdk::SUCCESS {
        return String::new();
    }
    let n = usize::try_from(len - 1).unwrap_or(0).min(buf.len());
    let bytes: Vec<u8> = buf[..n].iter().map(|&c| c.cast_unsigned()).collect();
    String::from_utf8_lossy(&bytes).into_owned()
}

fn token_ptr(token: u64) -> *mut c_void {
    std::ptr::without_provenance_mut(usize::try_from(token).unwrap_or(usize::MAX))
}

// ---- the platform

/// The process's timer resolution, raised to 1 ms while ticking fast (Windows' default is about 15.6 ms).
struct TimerResolution {
    winmm: Option<(libloading::Library, unsafe extern "system" fn(u32) -> u32, unsafe extern "system" fn(u32) -> u32)>,
}

impl TimerResolution {
    fn new() -> TimerResolution {
        #[cfg(windows)]
        {
            // SAFETY: winmm.dll is the system's; timeBeginPeriod and timeEndPeriod take and return a UINT.
            let winmm = unsafe {
                libloading::Library::new("winmm.dll").ok().and_then(|lib| {
                    let begin = *lib.get::<unsafe extern "system" fn(u32) -> u32>("timeBeginPeriod").ok()?;
                    let end = *lib.get::<unsafe extern "system" fn(u32) -> u32>("timeEndPeriod").ok()?;
                    Some((lib, begin, end))
                })
            };
            TimerResolution { winmm }
        }
        #[cfg(not(windows))]
        TimerResolution { winmm: None }
    }

    fn begin(&self) {
        if let Some((_, begin, _)) = &self.winmm {
            // SAFETY: timeBeginPeriod(1) is paired with an end in `end`.
            unsafe { begin(1) };
        }
    }

    fn end(&self) {
        if let Some((_, _, end)) = &self.winmm {
            // SAFETY: ends the period `begin` raised.
            unsafe { end(1) };
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Phase {
    /// The first sign-in is under way.
    SigningIn,
    Ready,
    /// load_eos on the parked platform: its park finishing, then (when the sign-in was lost) a sign-in again.
    Reopening {
        signing_in: bool,
    },
    /// The first sign-in failed: released, made again by the next load_eos.
    Dead,
}

/// A lobby this player is in. `bucket`: an owned lobby's; `listed`: the bucket of an owned lobby the searches find.
struct Joined {
    owned: bool,
    members: Vec<String>,
    bucket: Option<String>,
    listed: Option<String>,
}

/// What follows a lobby modification's update.
enum Modified {
    Create { ask: Ask, id: String, bucket: String },
    SetListed { ask: Ask, id: String, listed: bool },
}

/// What follows a lobby's leave.
enum Left {
    Answer(Ask, Result<Answer, String>),
    Park,
}

/// A one-shot call out, by its token (the callback's client data).
enum Op {
    DeleteDevice,
    CreateDevice,
    Login,
    CreateUser,
    Renew,
    Create { ask: Ask, bucket: String, listed: Option<Listed> },
    Modify { m: Handle, then: Modified },
    Leave { id: String, then: Left },
    Kick,
    Search { ask: Ask, search: Handle, limit: u32 },
    Join { ask: Ask, id: String },
    Invite { ask: Ask },
}

/// A shutdown's park in progress: the lobby leaves not answered yet, until `deadline`, and the asks to answer.
struct Parking {
    leaving: usize,
    deadline: f64,
    asks: Vec<Ask>,
}

/// The notifications bound after the sign-in (0: not bound).
#[derive(Default)]
struct Notes {
    auth_expiration: sdk::NotificationId,
    login_status: sdk::NotificationId,
    lobby_update: sdk::NotificationId,
    member_status: sdk::NotificationId,
    request: sdk::NotificationId,
    established: sdk::NotificationId,
    closed: sdk::NotificationId,
    invite: sdk::NotificationId,
}

struct Platform {
    sdk: &'static Sdk,
    platform: Handle,
    connect: Handle,
    lobby_api: Handle,
    p2p: Handle,
    invites: Handle,
    product: CString,
    spin: bool,
    phase: Phase,
    /// This player's product user id and its handle (null until signed in).
    me: String,
    local: Handle,
    handles: IndexMap<String, usize>,

    /// The service's clock as of the last poll.
    now: f64,
    running: bool,
    fast: bool,
    next_tick: f64,
    /// A tick at the next poll (a send goes out at once).
    flush: bool,
    last_active: f64,
    resolution: TimerResolution,
    recv: Vec<u8>,

    events: VecDeque<BackendEvent>,
    next_ask: u32,
    next_token: u64,
    ops: IndexMap<u64, Op>,

    lobbies: IndexMap<String, Joined>,
    accept: IndexSet<String>,
    /// Connection requests not accepted yet, and when each is refused.
    pending: IndexMap<String, f64>,
    /// The peers with an open link, and whether EOS relays it.
    relayed: IndexMap<String, bool>,
    /// The relay control set (None: not known, a park's reset refused).
    mode: Option<RelayMode>,
    /// The relay control each link to a peer was opened under: at its first send or its accept, kept to its close.
    opened_under: IndexMap<String, Option<RelayMode>>,

    /// Parked (shutdown was called, and no reopen since): every call does nothing or is refused.
    closed: bool,
    /// Released for good (end_eos).
    released: bool,
    parking: Option<Parking>,
    signed_out: bool,
    /// A renewal under way, until when it is tried; the time of its next try.
    renewing: Option<f64>,
    renew_at: Option<f64>,
    notes: Notes,
    /// The invite notification's client data (Box::into_raw), freed at release.
    invite_ctx: *mut InviteCtx,
}

// SAFETY: the raw handles are EOS's, used under the platform's mutex by one thread at a time (see the module's head
// for the one assumption this rests on).
unsafe impl Send for Platform {}

fn create(o: &EosOptions, g: &mut Global) -> Result<Platform, String> {
    let cred = read_credentials(&o.files.credentials).ok_or_else(|| "no EOS credentials".to_owned())?;
    let sdk = bind(&o.files.dll)?;
    let field = |k: &str| cred.get(k).and_then(|v| v.as_str()).map(cstr);
    let ptr = |c: &Option<CString>| c.as_ref().map_or(null(), |c| c.as_ptr());

    if !g.initialized {
        let product = cstr(&o.product);
        let version = cstr(&o.version);
        let io = sdk::InitializeOptions {
            api_version: 5,
            allocate_memory_function: null_mut(),
            reallocate_memory_function: null_mut(),
            release_memory_function: null_mut(),
            product_name: product.as_ptr(),
            product_version: version.as_ptr(),
            reserved: null_mut(),
            system_initialize_options: null_mut(),
            override_thread_affinity: null_mut(),
        };
        // SAFETY: the options and their strings live through the call.
        let r = unsafe { (sdk.initialize)(&io) };
        if r != sdk::SUCCESS {
            return Err(format!("EOS initialize: {}", result_text(sdk, r)));
        }
        g.initialized = true;
    }
    std::fs::create_dir_all(&o.files.cache).map_err(|e| e.to_string())?;
    let (product_id, sandbox_id, deployment_id) = (field("productId"), field("sandboxId"), field("deploymentId"));
    let (client_id, client_secret) = (field("clientId"), field("clientSecret"));
    let cache = cstr(&o.files.cache.to_string_lossy());
    let po = sdk::PlatformOptions {
        api_version: 15,
        reserved: null_mut(),
        product_id: ptr(&product_id),
        sandbox_id: ptr(&sandbox_id),
        client_credentials: sdk::ClientCredentials { client_id: ptr(&client_id), client_secret: ptr(&client_secret) },
        is_server: 0,
        encryption_key: null(),
        override_country_code: null(),
        override_locale_code: null(),
        deployment_id: ptr(&deployment_id),
        flags: sdk::PF_DISABLE_OVERLAY,
        cache_directory: cache.as_ptr(),
        tick_budget_in_milliseconds: 0,
        rtc_options: null_mut(),
        integrated_platform_options_container_handle: null_mut(),
        system_specific_options: null_mut(),
        task_network_timeout_seconds: null_mut(),
    };
    // SAFETY: the options and their strings live through the call.
    let platform = unsafe { (sdk.platform_create)(&po) };
    if platform.is_null() {
        return Err("EOS platform create failed (the credentials?)".to_owned());
    }
    // SAFETY: `platform` is the platform just made.
    let (connect, lobby_api, p2p, invites) = unsafe {
        (
            (sdk.get_connect)(platform),
            (sdk.get_lobby)(platform),
            (sdk.get_p2p)(platform),
            (sdk.get_custom_invites)(platform),
        )
    };
    lock(&INBOX).clear();
    let mut p = Platform {
        sdk,
        platform,
        connect,
        lobby_api,
        p2p,
        invites,
        product: cstr(&o.product),
        spin: o.spin,
        phase: Phase::SigningIn,
        me: String::new(),
        local: null_mut(),
        handles: IndexMap::new(),
        now: 0.0,
        running: true,
        fast: false,
        next_tick: 0.0,
        flush: false,
        last_active: f64::NEG_INFINITY,
        resolution: TimerResolution::new(),
        recv: vec![0; P2P_MAX_PACKET],
        events: VecDeque::new(),
        next_ask: 0,
        next_token: 1,
        ops: IndexMap::new(),
        lobbies: IndexMap::new(),
        accept: IndexSet::new(),
        pending: IndexMap::new(),
        relayed: IndexMap::new(),
        mode: Some(RelayMode::Force),
        opened_under: IndexMap::new(),
        closed: false,
        released: false,
        parking: None,
        signed_out: false,
        renewing: None,
        renew_at: None,
        notes: Notes::default(),
        invite_ctx: null_mut(),
    };
    if let Err(r) = p.relay_control(RelayMode::Force) {
        p.stop();
        p.release_platform();
        return Err(format!("EOS relay setting: {}", result_text(sdk, r)));
    }
    p.start_sign_in(o.fresh_device);
    Ok(p)
}

impl Platform {
    fn ask(&mut self) -> Ask {
        self.next_ask += 1;
        Ask(self.next_ask)
    }

    fn answer(&mut self, ask: Ask, r: Result<Answer, String>) {
        self.events.push_back(BackendEvent::Answered(ask, r));
    }

    /// Tells the service, unless parked: then only the answers to its calls go out.
    fn emit(&mut self, e: BackendEvent) {
        if self.closed && !matches!(e, BackendEvent::Answered(..)) {
            return;
        }
        self.events.push_back(e);
    }

    fn text(&self, r: sdk::EResult) -> String {
        result_text(self.sdk, r)
    }

    fn id_text(&self, id: Handle) -> String {
        id_text(self.sdk, id)
    }

    fn id_of(&mut self, id: &str) -> Handle {
        if let Some(&h) = self.handles.get(id) {
            return std::ptr::with_exposed_provenance_mut(h);
        }
        let s = cstr(id);
        // SAFETY: the string lives through the call; the id EOS returns lives as long as the platform.
        let h = unsafe { (self.sdk.user_id_from_string)(s.as_ptr()) };
        self.handles.insert(id.to_owned(), h.expose_provenance());
        h
    }

    /// The token a one-shot call's callback brings back.
    fn op(&mut self, op: Op) -> *mut c_void {
        let token = self.next_token;
        self.next_token += 1;
        self.ops.insert(token, op);
        token_ptr(token)
    }

    /// Calls that need the platform signed in and not parked.
    fn open(&self) -> Result<(), String> {
        if self.closed {
            return Err("EOS is shut down".to_owned());
        }
        if self.phase != Phase::Ready {
            return Err("EOS is not signed in".to_owned());
        }
        Ok(())
    }

    fn relay_control(&mut self, mode: RelayMode) -> Result<(), sdk::EResult> {
        let relay_control = match mode {
            RelayMode::Allow => sdk::RC_ALLOW_RELAYS,
            RelayMode::Force => sdk::RC_FORCE_RELAYS,
        };
        let o = sdk::SetRelayControlOptions { api_version: 1, relay_control };
        // SAFETY: the P2P interface is the platform's; the options are this frame's.
        let r = unsafe { (self.sdk.set_relay_control)(self.p2p, &o) };
        if r == sdk::SUCCESS { Ok(()) } else { Err(r) }
    }

    fn peer_call(&mut self, peer: &str, accept: bool) {
        let remote = self.id_of(peer);
        let socket = sdk::socket_id();
        let o =
            sdk::PeerOptions { api_version: 1, local_user_id: self.local, remote_user_id: remote, socket_id: &socket };
        // SAFETY: the handles are the platform's; the options and the socket id are this frame's.
        unsafe {
            if accept {
                (self.sdk.accept_connection)(self.p2p, &o);
            } else {
                (self.sdk.close_connection)(self.p2p, &o);
            }
        }
    }

    fn opening(&mut self, peer: &str) {
        if !self.opened_under.contains_key(peer) {
            self.opened_under.insert(peer.to_owned(), self.mode);
        }
    }

    /// A link that stood is gone: forgotten, and told.
    fn gone(&mut self, peer: &str) {
        self.opened_under.shift_remove(peer);
        if self.relayed.shift_remove(peer).is_some() {
            self.emit(BackendEvent::Link { peer: peer.to_owned(), relayed: None });
        }
    }

    fn stop(&mut self) {
        self.running = false;
        if self.fast {
            self.resolution.end();
            self.fast = false;
        }
    }

    fn release_platform(&mut self) {
        // SAFETY: the platform is released once; nothing of it is called after.
        unsafe { (self.sdk.platform_release)(self.platform) };
    }

    // ---- ticking

    /// Ticks when due (and at once after a send), and runs what is due on the platform's timers.
    fn pump(&mut self) {
        if !self.running {
            return;
        }
        self.step_reopen();
        if self.renew_at.is_some_and(|t| self.now >= t) {
            self.renew_at = None;
            self.renew_try();
        }
        let refused: Vec<String> =
            self.pending.iter().filter(|(_, t)| self.now >= **t).map(|(k, _)| k.clone()).collect();
        for from in refused {
            self.pending.shift_remove(&from);
            if !self.released {
                self.peer_call(&from, false);
            }
        }
        self.check_park();
        let due = self.now >= self.next_tick;
        if due {
            if !self.pending.is_empty() {
                self.recheck();
            }
            // (parked: the idle tick, so a parked platform the service polls rarely holds no 1 ms timer resolution)
            let want = !self.closed && (!self.relayed.is_empty() || self.now - self.last_active < ACTIVE_MS);
            if want != self.fast {
                self.fast = want;
                if want {
                    self.resolution.begin();
                } else {
                    self.resolution.end();
                }
            }
            self.next_tick = if self.spin && self.fast {
                self.now
            } else {
                self.now + if self.fast { TICK_MS } else { IDLE_TICK_MS }
            };
        }
        if due || self.flush {
            self.flush = false;
            self.tick();
        }
    }

    fn next_due(&self) -> Option<f64> {
        let reopen_due = self.phase == (Phase::Reopening { signing_in: false }) && self.parking.is_none();
        if !self.events.is_empty() || self.flush || reopen_due {
            return Some(self.now);
        }
        if !self.running {
            return None;
        }
        let mut t = self.next_tick;
        for d in self.renew_at.iter().chain(self.pending.values()).chain(self.parking.iter().map(|p| &p.deadline)) {
            t = t.min(*d);
        }
        Some(t)
    }

    /// One EOS_Platform_Tick: the notifications it made handled in their order, the waiting packets read, then the
    /// one-shot calls it answered (an answer is handed over after the tick's own notifications).
    fn tick(&mut self) {
        // SAFETY: the platform is in use (not released).
        unsafe { (self.sdk.platform_tick)(self.platform) };
        let notes = std::mem::take(&mut *lock(&INBOX));
        let mut answered = Vec::new();
        for n in notes {
            match n {
                Note::Done { token, info } => answered.push((token, info)),
                n => self.notified(n),
            }
        }
        self.drain();
        for (token, info) in answered {
            if let Some(op) = self.ops.shift_remove(&token) {
                self.finish(op, info);
            }
        }
    }

    fn drain(&mut self) {
        if self.local.is_null() {
            return;
        }
        let next = sdk::GetNextReceivedPacketSizeOptions {
            api_version: 2,
            local_user_id: self.local,
            requested_channel: null(),
        };
        let recv = sdk::ReceivePacketOptions {
            api_version: 2,
            local_user_id: self.local,
            max_data_size_bytes: u32::try_from(self.recv.len()).unwrap_or(0),
            requested_channel: null(),
        };
        loop {
            let mut size = 0u32;
            // SAFETY: the P2P interface is the platform's; the options and the out value are this frame's.
            if unsafe { (self.sdk.next_packet_size)(self.p2p, &next, &mut size) } != sdk::SUCCESS {
                break;
            }
            let mut peer: Handle = null_mut();
            let mut socket = sdk::socket_id();
            let mut channel = 0u8;
            let mut written = 0u32;
            // SAFETY: the buffer holds max_data_size_bytes; the out values are this frame's.
            let r = unsafe {
                (self.sdk.receive_packet)(
                    self.p2p,
                    &recv,
                    &mut peer,
                    &mut socket,
                    &mut channel,
                    self.recv.as_mut_ptr(),
                    &mut written,
                )
            };
            if r != sdk::SUCCESS {
                break;
            }
            self.last_active = self.now;
            // A packet on a channel of neither kind is read and dropped.
            let channel = match channel {
                0 => Channel::Messages,
                1 => Channel::Match,
                _ => continue,
            };
            let n = usize::try_from(written).unwrap_or(0).min(self.recv.len());
            let from = self.id_text(peer);
            let bytes = self.recv[..n].to_vec();
            self.emit(BackendEvent::Packet { from, channel, bytes });
        }
    }

    // ---- sign-in

    fn device_login(&mut self, op: Op) {
        let credentials =
            sdk::ConnectCredentials { api_version: 1, token: null(), kind: sdk::ECT_DEVICEID_ACCESS_TOKEN };
        let info = sdk::UserLoginInfo { api_version: 2, display_name: self.product.as_ptr(), nsa_id_token: null() };
        let o = sdk::LoginOptions { api_version: 2, credentials: &credentials, user_login_info: &info };
        let cd = self.op(op);
        // SAFETY: the connect interface is the platform's; the options are copied by the call.
        unsafe { (self.sdk.login)(self.connect, &o, cd, on_login) };
    }

    fn create_device(&mut self) {
        let model = cstr("PC Windows");
        let o = sdk::CreateDeviceIdOptions { api_version: 1, device_model: model.as_ptr() };
        let cd = self.op(Op::CreateDevice);
        // SAFETY: as device_login.
        unsafe { (self.sdk.create_device_id)(self.connect, &o, cd, on_result) };
    }

    fn start_sign_in(&mut self, fresh: bool) {
        if fresh {
            // a new Device ID for this Windows user: a second identity on one PC (a running session of the old one
            // keeps working)
            let o = sdk::DeleteDeviceIdOptions { api_version: 1 };
            let cd = self.op(Op::DeleteDevice);
            // SAFETY: as device_login.
            unsafe { (self.sdk.delete_device_id)(self.connect, &o, cd, on_result) };
        } else {
            self.create_device();
        }
    }

    fn signed_in(&mut self, user: Handle) {
        if self.phase == Phase::SigningIn {
            self.me = self.id_text(user);
            let me = self.me.clone();
            self.local = self.id_of(&me);
            if let Err(why) = self.bind_notes() {
                self.release();
                self.phase = Phase::Dead;
                self.events.push_back(BackendEvent::SignedOut(why));
                return;
            }
            self.phase = Phase::Ready;
            self.events.push_back(BackendEvent::SignedIn);
            return;
        }
        if self.id_text(user) != self.me {
            self.sign_in_failed("EOS signs this PC in as another player now (restart the app)".to_owned());
            return;
        }
        self.signed_out = false;
        // (a shutdown since cancelled the reopen: signed in again, still parked)
        if matches!(self.phase, Phase::Reopening { .. }) {
            self.reopened();
        }
    }

    fn sign_in_failed(&mut self, why: String) {
        if self.phase == Phase::SigningIn {
            self.stop();
            self.release_platform();
            self.phase = Phase::Dead;
            self.events.push_back(BackendEvent::SignedOut(why));
        } else if matches!(self.phase, Phase::Reopening { .. }) {
            // a reopen: the platform stays parked, signed out
            self.phase = Phase::Ready;
            self.events.push_back(BackendEvent::SignedOut(why));
        }
        // (a reopen a shutdown cancelled: nothing to tell)
    }

    fn bind_notes(&mut self) -> Result<(), String> {
        let sdk = self.sdk;
        let version = sdk::ApiVersionOptions { api_version: 1 };
        let add = sdk::AddNotifyOptions { api_version: 1 };
        let socket = sdk::socket_id();
        let peer = sdk::PeerNotifyOptions { api_version: 1, local_user_id: self.local, socket_id: &socket };
        self.invite_ctx = Box::into_raw(Box::new(InviteCtx { invites: self.invites, local: self.local }));
        let ctx_ptr = self.invite_ctx.cast::<c_void>();
        // SAFETY: the interfaces are the platform's; the options are copied by each call; the callbacks are static
        // and the invite's client data lives until its notification is removed (release).
        let n = unsafe {
            Notes {
                auth_expiration: (sdk.add_notify_auth_expiration)(self.connect, &add, null_mut(), on_auth_expiration),
                login_status: (sdk.add_notify_login_status_changed)(self.connect, &add, null_mut(), on_login_status),
                lobby_update: (sdk.add_notify_lobby_update)(self.lobby_api, &version, null_mut(), on_lobby_update),
                member_status: (sdk.add_notify_member_status)(self.lobby_api, &version, null_mut(), on_member_status),
                request: (sdk.add_notify_request)(self.p2p, &peer, null_mut(), on_request),
                established: (sdk.add_notify_established)(self.p2p, &peer, null_mut(), on_established),
                closed: (sdk.add_notify_closed)(self.p2p, &peer, null_mut(), on_closed),
                invite: (sdk.add_notify_invite)(self.invites, &version, ctx_ptr, on_invite),
            }
        };
        let failed: Vec<&str> = [
            ("authExpiration", n.auth_expiration),
            ("loginStatus", n.login_status),
            ("lobbyUpdate", n.lobby_update),
            ("memberStatus", n.member_status),
            ("request", n.request),
            ("established", n.established),
            ("closed", n.closed),
            ("invite", n.invite),
        ]
        .into_iter()
        .filter(|(_, id)| *id == 0)
        .map(|(k, _)| k)
        .collect();
        self.notes = n;
        if failed.is_empty() { Ok(()) } else { Err(format!("EOS notifications not bound: {}", failed.join(", "))) }
    }

    /// Removes the notifications, stops the ticking and releases the platform.
    fn release(&mut self) {
        let sdk = self.sdk;
        let n = std::mem::take(&mut self.notes);
        // SAFETY: each id was returned by its interface's add call and is removed once.
        unsafe {
            let remove: [(unsafe extern "C" fn(Handle, sdk::NotificationId), Handle, sdk::NotificationId); 8] = [
                (sdk.remove_notify_auth_expiration, self.connect, n.auth_expiration),
                (sdk.remove_notify_login_status_changed, self.connect, n.login_status),
                (sdk.remove_notify_lobby_update, self.lobby_api, n.lobby_update),
                (sdk.remove_notify_member_status, self.lobby_api, n.member_status),
                (sdk.remove_notify_request, self.p2p, n.request),
                (sdk.remove_notify_established, self.p2p, n.established),
                (sdk.remove_notify_closed, self.p2p, n.closed),
                (sdk.remove_notify_invite, self.invites, n.invite),
            ];
            for (f, h, id) in remove {
                if id != 0 {
                    f(h, id);
                }
            }
        }
        self.stop();
        self.release_platform();
        if !self.invite_ctx.is_null() {
            // SAFETY: made by Box::into_raw in bind_notes; its notification is removed above, so EOS holds it no more.
            drop(unsafe { Box::from_raw(self.invite_ctx) });
            self.invite_ctx = null_mut();
        }
    }

    // The sign-in is renewed with this Windows user's Device ID. Another process may have made a new one since (a
    // second identity on one PC): a renewal that signs in as another player signs out instead.
    fn renew_try(&mut self) {
        // (renewed while parked too, so a switch back finds it signed in)
        if self.released || self.signed_out {
            self.renewing = None;
            return;
        }
        self.device_login(Op::Renew);
    }

    fn sign_out(&mut self, why: String) {
        if self.signed_out {
            return;
        }
        self.signed_out = true;
        let ids: Vec<String> = self.lobbies.keys().cloned().collect();
        for lobby in ids {
            self.drop_lobby(&lobby, "signed out");
        }
        self.emit(BackendEvent::SignedOut(why));
    }

    // ---- a switch back

    fn reopen(&mut self) {
        if !matches!(self.phase, Phase::Reopening { .. }) {
            self.phase = Phase::Reopening { signing_in: false };
        }
    }

    /// A reopen goes on once the park is done: signed in again first when the sign-in was lost (parked or not).
    fn step_reopen(&mut self) {
        if self.phase != (Phase::Reopening { signing_in: false }) || self.parking.is_some() {
            return;
        }
        if self.signed_out {
            self.phase = Phase::Reopening { signing_in: true };
            self.start_sign_in(false);
        } else {
            self.reopened();
        }
    }

    fn reopened(&mut self) {
        self.phase = Phase::Ready;
        self.closed = false;
        self.events.push_back(BackendEvent::SignedIn);
    }

    // ---- the callbacks' continuations

    fn notified(&mut self, n: Note) {
        match n {
            Note::Done { .. } => {}
            Note::AuthExpiration => {
                if self.renewing.is_none() {
                    self.renewing = Some(self.now + RENEW_WINDOW_MS);
                    self.renew_try();
                }
            }
            Note::LoginStatus { user, status } => {
                if status == sdk::LS_NOT_LOGGED_IN && user == self.me {
                    self.sign_out("EOS signed out".to_owned());
                }
            }
            Note::LobbyUpdate(lobby) => self.refresh(&lobby),
            Note::MemberStatus { lobby, target, status } => {
                if !self.lobbies.contains_key(&lobby) {
                    return;
                }
                if status == sdk::LMS_CLOSED {
                    return self.drop_lobby(&lobby, "closed");
                }
                if target == self.me {
                    if status == sdk::LMS_KICKED {
                        return self.drop_lobby(&lobby, "kicked");
                    }
                    if status == sdk::LMS_LEFT || status == sdk::LMS_DISCONNECTED {
                        return self.drop_lobby(&lobby, "left");
                    }
                }
                self.refresh(&lobby);
            }
            Note::Request(from) => {
                self.last_active = self.now;
                // (parked: nothing here is in use, so a request is refused at once)
                if self.closed {
                    if !self.released {
                        self.peer_call(&from, false);
                    }
                    return;
                }
                if self.accept.contains(&from) {
                    self.opening(&from);
                    self.peer_call(&from, true);
                    return;
                }
                // held: accepted when its sender becomes a member (rechecked each tick), refused by closing it after
                // REQUEST_HOLD_MS
                if !self.pending.contains_key(&from) {
                    self.pending.insert(from, self.now + REQUEST_HOLD_MS);
                }
            }
            Note::Established { peer, network } => {
                // A side that forced relays when it opened or accepted the link takes it as relayed: Force wins
                // whoever opens (the round trip is the relays'), but the forcing side's NetworkType can say Direct
                // when the other side allows. A link this side did not open or accept (none known): the mode now.
                let under = match self.opened_under.get(&peer) {
                    Some(m) => *m,
                    None => self.mode,
                };
                let r = under == Some(RelayMode::Force) || network == sdk::NCT_RELAYED;
                self.relayed.insert(peer.clone(), r);
                self.emit(BackendEvent::Link { peer, relayed: Some(r) });
            }
            Note::Closed(peer) => {
                self.gone(&peer);
                self.emit(BackendEvent::PeerClosed(peer));
            }
            Note::Invite { from, payload } => self.emit(BackendEvent::Invite { from, payload }),
        }
    }

    fn finish(&mut self, op: Op, info: Info) {
        let code = match &info {
            Info::Result(c) | Info::Login { code: c, .. } | Info::CreateUser { code: c, .. } => *c,
            Info::LobbyId { code, .. } => *code,
        };
        match op {
            Op::DeleteDevice => self.create_device(),
            Op::CreateDevice => {
                if code != sdk::SUCCESS && code != sdk::DUPLICATE_NOT_ALLOWED {
                    self.sign_in_failed(format!("EOS device id: {}", self.text(code)));
                } else {
                    self.device_login(Op::Login);
                }
            }
            Op::Login => {
                let Info::Login { local, continuance, .. } = info else { return };
                if code == sdk::SUCCESS {
                    return self.signed_in(std::ptr::with_exposed_provenance_mut(local));
                }
                if code != sdk::INVALID_USER {
                    return self.sign_in_failed(format!("EOS sign-in: {}", self.text(code)));
                }
                // the first sign-in of this Device ID: its product user is made from the continuance token
                let o = sdk::CreateUserOptions {
                    api_version: 1,
                    continuance_token: std::ptr::with_exposed_provenance_mut(continuance),
                };
                let cd = self.op(Op::CreateUser);
                // SAFETY: the connect interface is the platform's; the token is the login's, unused before.
                unsafe { (self.sdk.create_user)(self.connect, &o, cd, on_create_user) };
            }
            Op::CreateUser => {
                let Info::CreateUser { local, .. } = info else { return };
                if code != sdk::SUCCESS {
                    return self.sign_in_failed(format!("EOS create user: {}", self.text(code)));
                }
                self.signed_in(std::ptr::with_exposed_provenance_mut(local));
            }
            Op::Renew => {
                let Some(until) = self.renewing else { return };
                if code == sdk::SUCCESS {
                    let Info::Login { local, .. } = info else { return };
                    if self.id_text(std::ptr::with_exposed_provenance_mut(local)) != self.me {
                        self.sign_out("EOS sign-in renewal: this PC signs in as another player now".to_owned());
                    }
                    self.renewing = None;
                } else if self.now + RENEW_RETRY_MS > until {
                    self.sign_out(format!("EOS sign-in renewal: {}", self.text(code)));
                    self.renewing = None;
                } else {
                    self.renew_at = Some(self.now + RENEW_RETRY_MS);
                }
            }
            Op::Create { ask, bucket, listed } => {
                let Info::LobbyId { lobby: id, .. } = info else { return };
                if code != sdk::SUCCESS {
                    return self.answer(ask, Err(self.text(code)));
                }
                let me = self.me.clone();
                self.lobbies.insert(
                    id.clone(),
                    Joined { owned: true, members: vec![me], bucket: Some(bucket.clone()), listed: None },
                );
                let Some(listed) = listed else { return self.answer(ask, Ok(Answer::Created(id))) };
                let then = Modified::Create { ask, id: id.clone(), bucket };
                if let Err((e, then)) = self.modify(&id, |p, m| p.list_with(m, &listed), then) {
                    self.modify_failed(then, e);
                }
            }
            Op::Modify { m, then } => {
                // SAFETY: the modification handle was made for this update and is released once.
                unsafe { (self.sdk.modification_release)(m) };
                // Unverified: a change to what the lobby already is answers EOS_NoChange (taken as done;
                // eos_lobby.h does not list it for UpdateLobby).
                if code != sdk::SUCCESS && code != sdk::NO_CHANGE {
                    return self.modify_failed(then, self.text(code));
                }
                match then {
                    Modified::Create { ask, id, bucket } => {
                        if let Some(j) = self.lobbies.get_mut(&id) {
                            j.listed = Some(bucket);
                        }
                        self.answer(ask, Ok(Answer::Created(id)));
                    }
                    Modified::SetListed { ask, id, listed } => {
                        if let Some(j) = self.lobbies.get_mut(&id) {
                            j.listed = if listed { j.bucket.clone() } else { None };
                        }
                        self.answer(ask, Ok(Answer::Done));
                    }
                }
            }
            Op::Leave { id, then } => {
                self.drop_lobby(&id, "left");
                self.after_leave(then);
            }
            Op::Kick => {}
            Op::Search { ask, search, limit } => {
                let r = if code == sdk::NOT_FOUND {
                    Ok(Vec::new())
                } else if code != sdk::SUCCESS {
                    Err(self.text(code))
                } else {
                    Ok(self.results(search, limit))
                };
                // SAFETY: the search handle was made for this search and is released once.
                unsafe { (self.sdk.search_release)(search) };
                self.answer(ask, r.map(Answer::Found));
            }
            Op::Join { ask, id } => {
                if code != sdk::SUCCESS {
                    return self.answer(ask, Err(self.text(code)));
                }
                let read = self.details(&id, |p, d| {
                    let opts = sdk::ApiVersionOptions { api_version: 1 };
                    // SAFETY: `d` is the lobby's details handle, held through the read.
                    let owner = p.id_text(unsafe { (p.sdk.get_lobby_owner)(d, &opts) });
                    (owner, p.members_of(d))
                });
                let Some((owner, members)) = read else {
                    return self.answer(ask, Err(crate::fail::LOBBY_UNREAD.to_owned()));
                };
                self.lobbies.insert(id, Joined { owned: false, members: members.clone(), bucket: None, listed: None });
                self.answer(ask, Ok(Answer::Joined { owner, members, lobby: None }));
            }
            Op::Invite { ask } => {
                let r = if code == sdk::SUCCESS { Ok(Answer::Done) } else { Err(self.text(code)) };
                self.answer(ask, r);
            }
        }
    }

    // ---- lobbies

    /// Reads a lobby's details handle; None when EOS has none for it.
    fn details<T>(&mut self, lobby: &str, read: impl FnOnce(&mut Platform, Handle) -> T) -> Option<T> {
        let id = cstr(lobby);
        let o = sdk::CopyLobbyDetailsHandleOptions { api_version: 1, lobby_id: id.as_ptr(), local_user_id: self.local };
        let mut d: Handle = null_mut();
        // SAFETY: the lobby interface is the platform's; the options and the out handle are this frame's.
        if unsafe { (self.sdk.copy_lobby_details)(self.lobby_api, &o, &mut d) } != sdk::SUCCESS {
            return None;
        }
        let v = read(self, d);
        // SAFETY: the handle was copied above and is released once.
        unsafe { (self.sdk.lobby_details_release)(d) };
        Some(v)
    }

    fn members_of(&self, d: Handle) -> Vec<String> {
        let opts = sdk::ApiVersionOptions { api_version: 1 };
        // SAFETY: `d` is a details handle held by the caller.
        let n = unsafe { (self.sdk.get_member_count)(d, &opts) };
        (0..n)
            .map(|i| {
                let o = sdk::GetMemberByIndexOptions { api_version: 1, member_index: i };
                // SAFETY: as above; the index is below the count.
                self.id_text(unsafe { (self.sdk.get_member_by_index)(d, &o) })
            })
            .collect()
    }

    /// Sends the lobby's members on when they changed.
    fn refresh(&mut self, lobby: &str) {
        if !self.lobbies.contains_key(lobby) {
            return;
        }
        let Some(members) = self.details(lobby, |p, d| p.members_of(d)) else { return };
        let Some(j) = self.lobbies.get_mut(lobby) else { return };
        if j.members == members {
            return;
        }
        j.members = members.clone();
        self.emit(BackendEvent::Lobby(LobbyChange::Members { lobby: lobby.to_owned(), members }));
    }

    fn drop_lobby(&mut self, lobby: &str, why: &str) {
        if self.lobbies.shift_remove(lobby).is_some() {
            self.emit(BackendEvent::Lobby(LobbyChange::Closed { lobby: lobby.to_owned(), why: why.to_owned() }));
        }
    }

    /// Leaves a lobby (its owner destroys it), `then` once EOS has answered; `then` back (nothing done) when this
    /// player is not in it.
    fn leave(&mut self, id: &str, then: Left) -> Result<(), Left> {
        let Some(j) = self.lobbies.get(id) else { return Err(then) };
        let owned = j.owned;
        let lobby = cstr(id);
        let o = sdk::LeaveLobbyOptions { api_version: 1, local_user_id: self.local, lobby_id: lobby.as_ptr() };
        let cd = self.op(Op::Leave { id: id.to_owned(), then });
        let f = if owned { self.sdk.destroy_lobby } else { self.sdk.leave_lobby };
        // SAFETY: the lobby interface is the platform's; the options are copied by the call.
        unsafe { f(self.lobby_api, &o, cd, on_lobby_id) };
        Ok(())
    }

    fn after_leave(&mut self, then: Left) {
        match then {
            Left::Answer(ask, r) => self.answer(ask, r),
            Left::Park => {
                if let Some(p) = &mut self.parking {
                    p.leaving = p.leaving.saturating_sub(1);
                }
                self.check_park();
            }
        }
    }

    /// Applies an owner's change to its lobby (a lobby modification, `edit` returning its first failed result or
    /// EOS_Success), `then` once EOS has answered.
    // Unverified: a handle held over the answer is not released once the SDK is shut down (a quit while the call is
    // out); taken as harmless at the process's end.
    fn modify(
        &mut self,
        id: &str,
        edit: impl FnOnce(&mut Platform, Handle) -> sdk::EResult,
        then: Modified,
    ) -> Result<(), (String, Modified)> {
        let lobby = cstr(id);
        let o =
            sdk::UpdateLobbyModificationOptions { api_version: 1, local_user_id: self.local, lobby_id: lobby.as_ptr() };
        let mut m: Handle = null_mut();
        // SAFETY: the lobby interface is the platform's; the options and the out handle are this frame's.
        let made = unsafe { (self.sdk.update_lobby_modification)(self.lobby_api, &o, &mut m) };
        if made != sdk::SUCCESS {
            return Err((self.text(made), then));
        }
        let set = edit(self, m);
        if set != sdk::SUCCESS {
            // SAFETY: the modification was made above and is released once.
            unsafe { (self.sdk.modification_release)(m) };
            return Err((self.text(set), then));
        }
        let uo = sdk::UpdateLobbyOptions { api_version: 1, lobby_modification_handle: m };
        let cd = self.op(Op::Modify { m, then });
        // SAFETY: as above; the modification is released when the update is answered.
        unsafe { (self.sdk.update_lobby)(self.lobby_api, &uo, cd, on_lobby_id) };
        Ok(())
    }

    fn modify_failed(&mut self, then: Modified, e: String) {
        match then {
            // A new lobby whose listing failed is left, then the create refused.
            Modified::Create { ask, id, .. } => {
                if let Err(Left::Answer(ask, r)) = self.leave(&id, Left::Answer(ask, Err(e))) {
                    self.answer(ask, r);
                }
            }
            Modified::SetListed { ask, .. } => self.answer(ask, Err(e)),
        }
    }

    fn permission(&self, m: Handle, level: i32) -> sdk::EResult {
        let o = sdk::SetPermissionLevelOptions { api_version: 1, permission_level: level };
        // SAFETY: `m` is a modification handle held by the caller.
        unsafe { (self.sdk.modification_set_permission_level)(m, &o) }
    }

    fn add_attribute(&self, m: Handle, key: &str, value: sdk::AttributeValue, value_type: i32) -> sdk::EResult {
        let key = cstr(key);
        let data = sdk::AttributeData { api_version: 1, key: key.as_ptr(), value, value_type };
        let o = sdk::AddAttributeOptions { api_version: 2, attribute: &data, visibility: sdk::LAT_PUBLIC };
        // SAFETY: `m` is a modification handle held by the caller; the attribute and its strings live through the
        // call, which copies them.
        unsafe { (self.sdk.modification_add_attribute)(m, &o) }
    }

    /// A modification that lists the lobby with its attributes (public, found by searches).
    fn list_with(&self, m: Handle, listed: &Listed) -> sdk::EResult {
        let p = self.permission(m, sdk::LPL_PUBLICADVERTISED);
        if p != sdk::SUCCESS {
            return p;
        }
        // (`at` is ms since 1970: whole, far inside an i64)
        let at = self.add_attribute(m, ATTR_AT, sdk::AttributeValue { as_int64: listed.at as i64 }, sdk::AT_INT64);
        if at != sdk::SUCCESS {
            return at;
        }
        let text = |key: &str, v: &Option<String>| match v {
            None => sdk::SUCCESS,
            Some(v) => {
                let s = cstr(v);
                self.add_attribute(m, key, sdk::AttributeValue { as_utf8: s.as_ptr() }, sdk::AT_STRING)
            }
        };
        let info = text(ATTR_INFO, &listed.info);
        if info != sdk::SUCCESS { info } else { text(ATTR_CODE, &listed.code) }
    }

    /// A lobby's attribute of `key`, None when it has none of `value_type`.
    fn attribute(&self, d: Handle, key: &str, value_type: i32) -> Option<Attr> {
        let k = cstr(key);
        let o = sdk::CopyAttributeByKeyOptions { api_version: 1, attr_key: k.as_ptr() };
        let mut a: *mut sdk::Attribute = null_mut();
        // SAFETY: `d` is a details handle held by the caller; the options and the out pointer are this frame's.
        if unsafe { (self.sdk.details_copy_attribute)(d, &o, &mut a) } != sdk::SUCCESS || a.is_null() {
            return None;
        }
        // SAFETY: EOS copied the attribute; it and its data live until its release below. The union is read as the
        // member its ValueType names.
        let v = unsafe {
            let data = (*a).data;
            if data.is_null() || (*data).value_type != value_type {
                None
            } else if value_type == sdk::AT_INT64 {
                Some(Attr::Int((*data).value.as_int64))
            } else {
                let s = (*data).value.as_utf8;
                if s.is_null() { None } else { Some(Attr::Text(text_of(s))) }
            }
        };
        // SAFETY: released once.
        unsafe { (self.sdk.attribute_release)(a) };
        v
    }

    /// A search result as a FoundLobby; None when its details cannot be read.
    fn found(&self, d: Handle) -> Option<FoundLobby> {
        let opts = sdk::ApiVersionOptions { api_version: 1 };
        let mut info: *mut sdk::LobbyDetailsInfo = null_mut();
        // SAFETY: `d` is a details handle held by the caller.
        if unsafe { (self.sdk.details_copy_info)(d, &opts, &mut info) } != sdk::SUCCESS || info.is_null() {
            return None;
        }
        // SAFETY: EOS copied the info; it lives until its release below.
        let mut lobby = unsafe {
            let i = &*info;
            FoundLobby {
                id: text_of(i.lobby_id),
                owner: self.id_text(i.lobby_owner_user_id),
                members: i.max_members.saturating_sub(i.available_slots),
                max: i.max_members,
                at: 0.0,
                info: String::new(),
                code: String::new(),
            }
        };
        // SAFETY: released once.
        unsafe { (self.sdk.details_info_release)(info) };
        if let Some(Attr::Int(at)) = self.attribute(d, ATTR_AT, sdk::AT_INT64) {
            lobby.at = at as f64;
        }
        if let Some(Attr::Text(t)) = self.attribute(d, ATTR_INFO, sdk::AT_STRING) {
            lobby.info = t;
        }
        if let Some(Attr::Text(t)) = self.attribute(d, ATTR_CODE, sdk::AT_STRING) {
            lobby.code = t;
        }
        Some(lobby)
    }

    /// A finished search's results in EOS's order, at most `limit`, none of this player's own.
    fn results(&self, search: Handle, limit: u32) -> Vec<FoundLobby> {
        let opts = sdk::ApiVersionOptions { api_version: 1 };
        // SAFETY: the search handle is held until the caller releases it.
        let n = unsafe { (self.sdk.search_result_count)(search, &opts) };
        let mut list = Vec::new();
        for i in 0..n {
            if list.len() >= usize::try_from(limit).unwrap_or(usize::MAX) {
                break;
            }
            let o = sdk::CopySearchResultByIndexOptions { api_version: 1, lobby_index: i };
            let mut d: Handle = null_mut();
            // SAFETY: as above; the out handle is this frame's.
            if unsafe { (self.sdk.search_result)(search, &o, &mut d) } != sdk::SUCCESS {
                continue;
            }
            if let Some(l) = self.found(d)
                && l.owner != self.me
            {
                list.push(l);
            }
            // SAFETY: the result's details were copied above and are released once.
            unsafe { (self.sdk.lobby_details_release)(d) };
        }
        list
    }

    // ---- P2P

    fn recheck(&mut self) {
        let ready: Vec<String> = self.pending.keys().filter(|f| self.accept.contains(*f)).cloned().collect();
        for from in ready {
            self.pending.shift_remove(&from);
            self.opening(&from);
            self.peer_call(&from, true);
        }
    }

    // ---- park and end

    /// Parks the platform: its lobbies left and links closed, the service's handlers let go.
    fn shutdown(&mut self) -> Ask {
        let ask = self.ask();
        if self.phase == Phase::SigningIn {
            // a shutdown before the first sign-in ends: the platform let go, made again by the next load
            self.stop();
            self.release_platform();
            self.phase = Phase::Dead;
            self.answer(ask, Ok(Answer::Done));
            return ask;
        }
        if self.phase == Phase::Dead {
            self.answer(ask, Ok(Answer::Done));
            return ask;
        }
        if self.closed {
            // a reopen under way is cancelled: the platform stays parked
            if matches!(self.phase, Phase::Reopening { .. }) {
                self.phase = Phase::Ready;
            }
            match &mut self.parking {
                Some(p) => p.asks.push(ask),
                None => self.answer(ask, Ok(Answer::Done)),
            }
            return ask;
        }
        self.closed = true;
        // (the events not polled yet are dropped at the shutdown; the answers still go)
        self.events.retain(|e| matches!(e, BackendEvent::Answered(..)));
        self.accept.clear();
        self.pending.clear();
        let ids: Vec<String> = self.lobbies.keys().cloned().collect();
        let mut leaving = 0;
        for id in ids {
            if self.leave(&id, Left::Park).is_ok() {
                leaving += 1;
            }
        }
        // leaving waits for EOS's answers, a few seconds at most
        self.parking = Some(Parking { leaving, deadline: self.now + PARK_WAIT_MS, asks: vec![ask] });
        self.check_park();
        ask
    }

    fn check_park(&mut self) {
        let Some(p) = &self.parking else { return };
        if p.leaving > 0 && self.now < p.deadline {
            return;
        }
        let asks = self.parking.take().map(|p| p.asks).unwrap_or_default();
        if !self.released {
            let me = self.me.clone();
            let peers: Vec<String> = self.handles.keys().filter(|k| **k != me).cloned().collect();
            for peer in peers {
                self.peer_call(&peer, false);
            }
            self.relayed.clear();
            self.opened_under.clear();
            // (a switch back starts from forced relays; refused: not known, and the service sets it again before use)
            self.mode = self.relay_control(RelayMode::Force).ok().map(|()| RelayMode::Force);
        }
        if self.fast {
            self.fast = false;
            self.resolution.end();
        }
        for ask in asks {
            self.answer(ask, Ok(Answer::Done));
        }
    }

    /// The program's quit: parked (waiting here for the leaves), then released and the SDK shut down.
    fn end(&mut self) {
        if !self.closed {
            self.shutdown();
        }
        let start = Instant::now();
        let base = self.now;
        while self.parking.is_some() && self.running {
            self.now = base + start.elapsed().as_secs_f64() * 1000.0;
            self.tick();
            self.check_park();
            std::thread::sleep(Duration::from_millis(1));
        }
        self.released = true;
        if self.running {
            self.release();
        }
        // SAFETY: the platform is released; EOS_Shutdown ends the SDK for the process.
        unsafe { (self.sdk.shutdown)() };
    }
}

enum Attr {
    Int(i64),
    Text(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    fn options(dir: &std::path::Path, credentials: &str) -> EosOptions {
        EosOptions {
            files: EosFiles {
                dll: dir.join("no-such-dir").join("EOSSDK-Win64-Shipping.dll"),
                credentials: dir.join(credentials),
                cache: dir.join("cache"),
            },
            product: "MVS Classic".to_owned(),
            version: "0".to_owned(),
            fresh_device: false,
            spin: false,
        }
    }

    /// No credentials file, then no DLL: online is off with the reason, and nothing is loaded.
    #[test]
    fn load_without_the_sdk_is_refused() {
        let dir = std::env::temp_dir().join(format!("mvs-eos-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let none = load_eos(&options(&dir, "missing.json")).err();
        assert_eq!(none.as_deref(), Some("no EOS credentials"));
        std::fs::write(
            dir.join("eos.json"),
            r#"{"productId":"p","sandboxId":"s","deploymentId":"d","clientId":"c","clientSecret":"x"}"#,
        )
        .unwrap();
        let e = load_eos(&options(&dir, "eos.json")).err().unwrap_or_default();
        assert!(e.starts_with("the EOS DLL did not load"), "{e}");
        assert!(SDK.get().is_none());
        assert!(!dir.join("cache").exists());
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
