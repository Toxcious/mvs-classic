//! The game's sound, as the build's Wwise plays it.
//!
//! The game thread holds an [`Audio`]: it sends [`Cmd`]s stamped with the sim tick over a lock-free ring to the
//! mixer thread and reads back [`Note`]s and the global game parameters ([`RtpcMirror`]). The mixer thread owns the
//! rest and allocates nothing: the interpreter (`ctl`: banks, posts, voices, game syncs, positioning, music) turns
//! commands into [`spec::VoiceSpec`]s and [`spec::ParamChange`]s; the mixer (`mix`) plays them through the voices,
//! the bus tree and its effects (`fx`) into the output (`out`: WASAPI, or offline to a buffer for the checks).
//! Media come from the store's `sound` kind as Vorbis packets (`decode`); the banks from its `soundbank` kind
//! ([`mvs_data::sound`]).
//!
//! Names (events, game parameters, switch groups and values, states) cross as their Wwise ShortIDs
//! ([`mvs_data::sound::short_id`]); a sound's key as its [`Key`] hash.

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use mvs_data::sound::{Curve, Id, InitBank};

/// A path not ported yet: panics with what it is.
#[macro_export]
macro_rules! unported {
    ($what:literal $(, $arg:expr)* $(,)?) => {{
        $(let _ = &$arg;)*
        panic!(concat!("unported: ", $what))
    }};
}

pub mod ctl;
pub mod decode;
pub mod fx;
pub mod math;
pub mod mix;
pub mod out;
mod send;
pub mod spec;
pub mod tape;

/// The mixer's sample rate (the build's Wwise init settings).
pub const RATE: u32 = 48_000;
/// The mixer's quantum, samples.
// Deviation: the build mixes 1024-sample audio frames; the port mixes 256-sample quanta for latency, and the rules
// tied to the build's frame (parameter changes, stop fades, transitions) count four quanta to a frame.
pub const QUANTUM: usize = 256;
/// The build's audio frame, samples.
pub const FRAME: usize = 1024;
/// Quanta per build frame.
pub const QUANTA_PER_FRAME: usize = FRAME / QUANTUM;

/// The game-to-mixer command ring's size (what it has no room for waits on the game side: `send`).
const CMD_RING: usize = 4096;
/// The mixer-to-game note ring's size (a scope let go sends back a note per medium it held).
const NOTE_RING: usize = 16_384;

/// A point or a direction in UE units and axes.
pub type V3 = [f64; 3];

/// A Wwise game object as the game numbers them: a fighter by its id, a minion from 100, the global object -1,
/// the pooled location components 1000 + i, the ships 2000 + owner, the scenery's emitters 3000 + n.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, serde::Serialize, serde::Deserialize)]
pub struct GameObj(pub i32);

impl GameObj {
    pub const GLOBAL: GameObj = GameObj(-1);
    pub const POOL: i32 = 1000;
    pub const POOL_SIZE: i32 = 11;
    pub const SHIP: i32 = 2000;
    pub const SCENERY: i32 = 3000;
}

/// FNV-1a 64 of a key's bytes.
fn key_hash(s: &str) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in s.bytes() {
        h = (h ^ u64::from(b)).wrapping_mul(0x0100_0000_01b3);
    }
    h
}

/// An AkCurveInterpolation by its name.
pub fn curve_named(name: &str) -> Result<Curve, AudioError> {
    serde_json::from_value(serde_json::Value::String(name.to_owned()))
        .map_err(|_| AudioError::Call(format!("no curve {name}")))
}

/// The key a continuous sound is stopped by later (the game's strings: a fighter's `<tag>:<n>`, the scenery's
/// `<group>:<key>`): the hash of the whole key and of its part before the first ':' (the whole key without one),
/// so a stop of every key but a group's finds the group's keys (a group the game keeps has no ':' of its own).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Key {
    pub id: u64,
    pub under: u64,
}

impl Key {
    pub fn new(key: &str) -> Key {
        let under = key.split_once(':').map_or(key, |(g, _)| g);
        Key { id: key_hash(key), under: key_hash(under) }
    }
    /// The hash a [`Cmd::StopAll`] keeps a group by.
    pub fn group(group: &str) -> u64 {
        key_hash(group)
    }
}

/// A game object's transform: location, forward and up.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Emitter {
    pub pos: V3,
    pub front: V3,
    pub top: V3,
}

/// The listener: location and axes.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Listener {
    pub pos: V3,
    pub right: V3,
    pub up: V3,
    pub front: V3,
}

/// An AkReverbVolume of the level: its brush's box (UE axes), its late reverb's aux bus, send level, fade rate
/// (share of the send level per second) and priority.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ReverbVolume {
    pub bus: Id,
    pub lo: V3,
    pub hi: V3,
    pub level: f64,
    pub rate: f64,
    pub prio: f64,
}

/// A post's id, given by [`Audio::post`]: what its [`Note::EventEnd`] names.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct PostId(pub u32);

/// An event posted (PostOpts): on its owner's game object (None: none named, so a Stop it holds acts on every
/// object), with the key that stops it later, as the Stop of another sound's key, or at a location on a pooled
/// component (PF_Post_Event_At_Location).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Post {
    pub id: PostId,
    pub event: Id,
    pub obj: Option<GameObj>,
    pub key: Option<Key>,
    pub stop_key: Option<Key>,
    pub at: Option<V3>,
}

/// The order the values of a music switch group can take: the group, the value the flow starts with, and per value
/// the values that can follow it.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct MusicFlow {
    pub group: Id,
    pub start: Id,
    pub next: Vec<(Id, Vec<Id>)>,
}

impl MusicFlow {
    /// A tape's flow by name.
    pub fn of(f: &tape::post::FlowArg) -> MusicFlow {
        use mvs_data::sound::short_id;
        MusicFlow {
            group: short_id(&f.group),
            start: short_id(&f.start),
            next: f.next.iter().map(|(k, v)| (short_id(k), v.iter().map(|x| short_id(x)).collect())).collect(),
        }
    }
}

/// A music command: hold the media of an event's switch values (preload), make them the playing scope's (scope,
/// with the flow its switch values take), or let them go (release).
#[derive(Debug)]
pub enum MusicCmd {
    Preload {
        event: Id,
        values: Box<[Id]>,
        media: Box<ctl::scope::ScopeLoad>,
    },
    Scope {
        event: Id,
        values: Box<[Id]>,
        flow: Option<Box<MusicFlow>>,
        media: Box<ctl::scope::ScopeLoad>,
    },
    /// `media`: what the values reach, let go once no run of the music has it.
    Release {
        event: Id,
        values: Box<[Id]>,
        media: Box<[Id]>,
    },
}

/// A command from the game to the mixer thread. Its size is fixed: what is large crosses boxed, made on the game
/// thread, and comes back in a [`Note::Free`] to be dropped there.
#[derive(Debug)]
pub enum Cmd {
    Post(Post),
    /// A game parameter on an object (the global object: the global value), reached over `interp_ms`.
    SetRtpc {
        id: Id,
        value: f64,
        obj: GameObj,
        interp_ms: f64,
    },
    SetSwitch {
        group: Id,
        value: Id,
        obj: GameObj,
    },
    SetState {
        group: Id,
        value: Id,
    },
    SetEmitter {
        obj: GameObj,
        pos: V3,
        front: V3,
    },
    SetListener(Listener),
    SetReverbVolumes(Box<[ReverbVolume]>),
    /// Stop the sounds of a key over `fade` s (None: the default fade curve).
    StopKey {
        key: Key,
        fade: f64,
        curve: Option<Curve>,
    },
    /// Stop every sound over `fade` s but those of the key or group hashed `keep`.
    StopAll {
        fade: f64,
        keep: Option<u64>,
    },
    /// The loaded banks merged anew (a scope held or let go, a music event's bank), made on the game side.
    Banks(Arc<ctl::bank::BankView>),
    /// A scope's media loaded (its name's hash; its banks came before it in a `Cmd::Banks`).
    Scope {
        name: u64,
        load: Box<ctl::scope::ScopeLoad>,
    },
    /// A scope let go.
    Unscope {
        name: u64,
    },
    /// Media of a scope's lazy events, decoded when one was first posted, held under the scope (its name's hash)
    /// until it is let go. Sent before the post that plays them.
    Media {
        owner: u64,
        media: Box<[Arc<decode::Pcm>]>,
    },
    Music(MusicCmd),
    /// The master volume, linear.
    SetVolume(f64),
    /// The output resumed (after a device change or a suspend).
    Resume,
}

/// A command and the sim tick it was sent on.
#[derive(Debug)]
pub struct Stamped {
    pub tick: u32,
    pub cmd: Cmd,
}

/// What a command carried that the mixer is done with, to be dropped on the game thread.
#[derive(Debug)]
pub enum Garbage {
    Scope(Box<ctl::scope::ScopeLoad>),
    ReverbVolumes(Box<[ReverbVolume]>),
    MusicValues(Box<[Id]>),
    MusicFlow(Box<MusicFlow>),
    /// A decoded medium the mixer held (its scope let go, or the voice that played it ended after).
    Media(Arc<decode::Pcm>),
    /// A lazy media command's list (the mixer holds its own references).
    MediaList(Box<[Arc<decode::Pcm>]>),
    /// A streamed medium the mixer held.
    Coded(Arc<decode::Coded>),
}

/// A note from the mixer thread to the game.
#[derive(Debug)]
pub enum Note {
    /// The last voice of a post ended.
    EventEnd {
        post: PostId,
    },
    /// A scope's command was taken in.
    ScopeReady {
        name: u64,
    },
    Free(Garbage),
    /// Something the interpreter could not do (a missing object or medium), with the id it names.
    Warn {
        what: &'static str,
        id: u64,
    },
}

/// The global values of the game parameters the InitBank names, as the mixer last set them: the game reads them
/// (the HUD's meters, the scenery's) without a round trip.
pub struct RtpcMirror {
    ids: Box<[Id]>,
    vals: Box<[AtomicU64]>,
    /// Whether a slot was set since the mirror was made.
    was_set: Box<[std::sync::atomic::AtomicBool]>,
}

impl RtpcMirror {
    /// One slot per game parameter of `init`, at its default.
    pub fn new(init: &InitBank) -> RtpcMirror {
        let ids: Box<[Id]> = init.rtpc_defaults.keys().copied().collect();
        let vals = init.rtpc_defaults.values().map(|v| AtomicU64::new(v.to_bits())).collect();
        let was_set = ids.iter().map(|_| std::sync::atomic::AtomicBool::new(false)).collect();
        RtpcMirror { ids, vals, was_set }
    }
    fn slot(&self, id: Id) -> Option<usize> {
        self.ids.binary_search(&id).ok()
    }
    pub fn get(&self, id: Id) -> Option<f64> {
        self.slot(id).map(|i| f64::from_bits(self.vals[i].load(Ordering::Relaxed)))
    }
    /// The value if it was set since the mirror was made, else None (the default is not a value the game set).
    pub fn get_set(&self, id: Id) -> Option<f64> {
        let i = self.slot(id).filter(|&i| self.was_set[i].load(Ordering::Acquire))?;
        Some(f64::from_bits(self.vals[i].load(Ordering::Relaxed)))
    }
    /// Sets a global value; false for a game parameter the InitBank does not name.
    pub fn set(&self, id: Id, v: f64) -> bool {
        self.slot(id)
            .map(|i| {
                self.vals[i].store(v.to_bits(), Ordering::Relaxed);
                self.was_set[i].store(true, Ordering::Release);
            })
            .is_some()
    }
}

/// Where the mix goes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Backend {
    /// The default device, shared mode, followed across device changes.
    Wasapi,
    /// A buffer the caller pumps: the checks' renders.
    Offline,
}

#[derive(Debug, thiserror::Error)]
pub enum AudioError {
    #[error(transparent)]
    Sound(#[from] mvs_data::sound::SoundError),
    #[error(transparent)]
    Store(#[from] mvs_store::StoreError),
    #[error("audio output: {0}")]
    Output(String),
    /// A call the engine cannot take (a curve name it does not know).
    #[error("audio call: {0}")]
    Call(String),
}

/// The game's side of the sound.
pub struct Audio {
    pub(crate) store: Arc<mvs_store::Store>,
    pub(crate) init: Arc<InitBank>,
    /// The command ring with what waits for room on it, and the bank merge.
    pub(crate) send: send::Sender,
    pub(crate) rx: rtrb::Consumer<Note>,
    /// The notes read before a merge (so that the loose banks' uses go first), for the next [`Audio::take_notes`].
    pub(crate) inbox: Vec<Note>,
    pub(crate) rtpc: Arc<RtpcMirror>,
    pub(crate) run: out::Run,
    pub(crate) next_post: u32,
    /// The scopes' lazy media and the posts waiting for theirs.
    pub(crate) lazy: ctl::scope::Lazy,
    /// The music events' banks a release read, by event: read once.
    pub(crate) music_banks: Vec<(Id, mvs_data::sound::EventBank)>,
    /// The scopes loaded since the caller last took them, for the load readout.
    pub scope_stats: Vec<ScopeStat>,
    /// The decoded media and setup headers, kept across scopes and matches.
    pub(crate) media: Arc<decode::cache::MediaCache>,
    /// Scopes and music prepared off this thread for the calls to come (`Audio::offer_scopes`): a call of the same
    /// name and lists installs its offer instead of loading.
    pub(crate) offered: Offers,
}

/// The scopes and music scopes offered to the calls to come.
#[derive(Default)]
pub(crate) struct Offers {
    scopes: Vec<PreparedScope>,
    music: Vec<PreparedMusic>,
}

/// What a scope's load reads: the store, the InitBank and the decoded media, shared with the audio. A clone goes to
/// any thread; [`ScopeSource::prepare`] there reads and decodes a scope that [`Audio::install_scope`] then sends.
#[derive(Clone)]
pub struct ScopeSource {
    store: Arc<mvs_store::Store>,
    init: Arc<InitBank>,
    media: Arc<decode::cache::MediaCache>,
}

impl ScopeSource {
    /// Reads a scope's banks and decodes the media of `keep` (the cache's misses on the loader's threads, the
    /// calling thread waiting); those of `lazy` are listed for the worker.
    pub fn prepare(&self, name: &str, keep: &[&str], lazy: &[&str]) -> Result<PreparedScope, AudioError> {
        let t0 = std::time::Instant::now();
        let (load, decoded) = ctl::scope::load(&self.store, &self.init, &self.media, keep, lazy)?;
        let stat = ScopeStat {
            name: name.to_owned(),
            ms: t0.elapsed().as_secs_f64() * 1e3,
            media: load.media.len(),
            decoded,
            bytes: load.media.iter().map(|p| p.data.len() * size_of::<f32>()).sum(),
            streamed: load.streamed.len(),
            streamed_bytes: load.streamed.iter().map(|c| c.entry.len()).sum(),
        };
        let owned = |v: &[&str]| v.iter().map(|&x| x.to_owned()).collect();
        Ok(PreparedScope { name: name.to_owned(), keep: owned(keep), lazy: owned(lazy), load: Box::new(load), stat })
    }

    /// A music event's scope (`Audio::scope_music`'s load) read on this thread.
    pub fn prepare_music(
        &self,
        event: &str,
        values: &[&str],
        flow: Option<MusicFlow>,
    ) -> Result<PreparedMusic, AudioError> {
        let media = Box::new(music_scope_load(&self.store, &self.init, &self.media, event, values, flow.as_ref())?);
        let values = values.iter().map(|&v| v.to_owned()).collect();
        Ok(PreparedMusic { event: event.to_owned(), values, flow, media })
    }

    /// The InitBank the scopes read (its events: the glue's index of known names).
    pub fn init(&self) -> &InitBank {
        &self.init
    }

    /// The store the scopes read.
    pub fn store(&self) -> &mvs_store::Store {
        &self.store
    }
}

/// A music scope's bank read and media held, not sent yet ([`ScopeSource::prepare_music`]): taken by the
/// [`Audio::scope_music`] of the same event, values and flow.
pub struct PreparedMusic {
    event: String,
    values: Vec<String>,
    flow: Option<MusicFlow>,
    media: Box<ctl::scope::ScopeLoad>,
}

/// A scope read and decoded, not sent yet ([`ScopeSource::prepare`], [`Audio::install_scope`]).
pub struct PreparedScope {
    name: String,
    /// The events it was prepared for (an offer is taken by a call of the same lists).
    keep: Vec<String>,
    lazy: Vec<String>,
    load: Box<ctl::scope::ScopeLoad>,
    stat: ScopeStat,
}

/// A scope's load as the load readout shows it.
#[derive(Clone, Debug, Default)]
pub struct ScopeStat {
    pub name: String,
    pub ms: f64,
    /// Its in-memory media, and of them the ones decoded by this load (the rest were held already).
    pub media: usize,
    pub decoded: usize,
    /// Its media's decoded bytes.
    pub bytes: usize,
    /// Its media kept compressed (decoded as they play), and their bytes.
    pub streamed: usize,
    pub streamed_bytes: usize,
}

impl ScopeStat {
    /// "scope <name> <ms> ms (<media> media, <decoded> decoded, <MB> MB; <n> streamed, <MB> MB)".
    pub fn line(&self) -> String {
        let mb = |b: usize| b as f64 / (1024.0 * 1024.0);
        format!(
            "scope {} {:.0} ms ({} media, {} decoded, {:.0} MB; {} streamed, {:.0} MB)",
            self.name,
            self.ms,
            self.media,
            self.decoded,
            mb(self.bytes),
            self.streamed,
            mb(self.streamed_bytes)
        )
    }
}

impl Audio {
    /// Reads the InitBank, starts the mixer thread and the output.
    pub fn new(store: Arc<mvs_store::Store>, backend: Backend) -> Result<Audio, AudioError> {
        let init = Arc::new(mvs_data::sound::load_init(&store)?);
        let rtpc = Arc::new(RtpcMirror::new(&init));
        let (tx, cmds) = rtrb::RingBuffer::new(CMD_RING);
        let (notes, rx) = rtrb::RingBuffer::new(NOTE_RING);
        let channels = match backend {
            Backend::Wasapi => out::wasapi::Wasapi::device_channels()?,
            Backend::Offline => 2,
        };
        let mut mixer = Box::new(mix::Mixer::new(init.clone(), cmds, notes, rtpc.clone(), channels));
        if backend == Backend::Offline {
            mixer.inline_streams();
        }
        let run = match backend {
            Backend::Wasapi => out::Run::Wasapi(out::wasapi::Wasapi::start(mixer)?),
            Backend::Offline => out::Run::Offline { mixer, out: Vec::new() },
        };
        let media = Arc::new(decode::cache::MediaCache::new());
        let lazy = ctl::scope::Lazy::new(backend == Backend::Wasapi, media.clone());
        Ok(Audio {
            store,
            init,
            send: send::Sender::new(tx, backend == Backend::Wasapi),
            rx,
            inbox: Vec::new(),
            rtpc,
            run,
            next_post: 0,
            lazy,
            music_banks: Vec::new(),
            scope_stats: Vec::new(),
            media,
            offered: Offers::default(),
        })
    }

    /// Sends a command stamped with the sim tick: on the ring, or after those waiting for room on it. A stop by key
    /// takes the posts waiting for their media that it matches with it.
    pub fn send(&mut self, tick: u32, cmd: Cmd) {
        match &cmd {
            &Cmd::StopKey { key, .. } => self.lazy.cancel(&|k| k == Some(key)),
            &Cmd::StopAll { keep, .. } => self.lazy.cancel(&|k| match (keep, k) {
                (None, _) | (Some(_), None) => true,
                (Some(g), Some(key)) => key.id != g && key.under != g,
            }),
            // (a stop key acts in a Stop action alone: ctl/post.rs)
            Cmd::Post(p) if p.stop_key.is_some() && self.lazy.stops(p.event) => {
                self.lazy.cancel(&|k| k == p.stop_key);
            }
            _ => {}
        }
        if let Cmd::Post(p) = cmd {
            let Audio { lazy, store, send, .. } = self;
            lazy.post(store, tick, p, &mut |t, c| send.send(t, c));
            return;
        }
        self.send.send(tick, cmd);
    }

    /// Posts an event (its id given here) and returns the id. A lazy event's post whose media are not decoded yet
    /// waits for them ([`Audio::poll`]).
    pub fn post(&mut self, tick: u32, mut post: Post) -> PostId {
        self.next_post = self.next_post.wrapping_add(1);
        post.id = PostId(self.next_post);
        self.poll();
        self.send(tick, Cmd::Post(post));
        post.id
    }

    /// Sends the commands that waited for room on the ring, then the lazy media decoded since the last call and the
    /// posts that waited for them; once a frame (the calls that post, [`Audio::take_notes`] and [`Audio::pump`]
    /// call it too).
    pub fn poll(&mut self) {
        let Audio { lazy, send, .. } = self;
        send.flush();
        lazy.poll(&mut |t, c| send.send(t, c));
    }

    /// Posts dropped because too many commands waited for room on the command ring.
    pub fn dropped(&self) -> u32 {
        self.send.dropped
    }

    /// Commands waiting for room on the command ring or for a bank merge.
    pub fn backlog(&self) -> usize {
        self.send.waiting()
    }

    /// Reads the mixer's notes into the inbox: what they free dropped (a scope's decoded media stay in the cache
    /// until the next match's install trims it, so a restart with the same fighters decodes nothing).
    fn read_notes(&mut self) {
        while let Ok(n) = self.rx.pop() {
            match n {
                Note::Free(_) => {}
                n => self.inbox.push(n),
            }
        }
    }

    /// Posts waiting for their lazy media.
    pub fn waiting(&self) -> usize {
        self.lazy.waiting()
    }

    /// Makes a call by name (the glue's, a post tape's line): its names hashed, its keys and reverb volumes bound,
    /// a scope's or the music's media loaded.
    pub fn call(&mut self, line: &tape::post::Line) -> Result<(), AudioError> {
        use mvs_data::sound::short_id;
        use tape::post::Call;
        let tick = line.tick;
        let obj = |o: i32| GameObj(o);
        fn names(v: &[String]) -> Vec<&str> {
            v.iter().map(String::as_str).collect()
        }
        match &line.call {
            Call::Post { ev, owner, key, stop_key, at } => {
                let p = Post {
                    id: PostId::default(),
                    event: short_id(ev),
                    obj: owner.map(obj),
                    key: key.as_deref().map(Key::new),
                    stop_key: stop_key.as_deref().map(Key::new),
                    at: *at,
                };
                self.post(tick, p);
            }
            &Call::SetRtpc { ref name, value, owner, interp_ms } => {
                self.send(tick, Cmd::SetRtpc { id: short_id(name), value, obj: obj(owner), interp_ms });
            }
            Call::SetSwitch { group, value, owner } => {
                self.send(tick, Cmd::SetSwitch { group: short_id(group), value: short_id(value), obj: obj(*owner) });
            }
            Call::SetState { group, value } => {
                self.send(tick, Cmd::SetState { group: short_id(group), value: short_id(value) });
            }
            &Call::SetEmitter { owner, pos, front } => {
                self.send(tick, Cmd::SetEmitter { obj: obj(owner), pos, front });
            }
            &Call::SetListener { pos, right, up, front } => {
                self.send(tick, Cmd::SetListener(Listener { pos, right, up, front }));
            }
            Call::SetReverbVolumes { list } => {
                // setReverbVolumes: the brush's box is the 200-unit cube scaled; the volumes of a bus the bank
                // has, by priority (the highest first, stable), at most 4.
                let mut vols: Vec<ReverbVolume> = Vec::new();
                for r in list {
                    let bus = short_id(&r.bus);
                    if !self.init.buses.contains_key(&bus) {
                        eprintln!("audio: reverb volume bus {bus} not in the bank");
                        continue;
                    }
                    let lo = [0, 1, 2].map(|a| r.loc[a] - 100.0 * r.scale[a].abs());
                    let hi = [0, 1, 2].map(|a| r.loc[a] + 100.0 * r.scale[a].abs());
                    let (level, rate, prio) =
                        (r.level.unwrap_or(1.0), r.rate.unwrap_or(0.5), r.priority.unwrap_or(1.0));
                    vols.push(ReverbVolume { bus, lo, hi, level, rate, prio });
                }
                vols.sort_by(|a, b| b.prio.total_cmp(&a.prio));
                vols.truncate(4);
                self.send(tick, Cmd::SetReverbVolumes(vols.into_boxed_slice()));
            }
            &Call::StopKey { ref key, fade, ref curve } => {
                let curve = curve.as_deref().map(curve_named).transpose()?;
                self.send(tick, Cmd::StopKey { key: Key::new(key), fade, curve });
            }
            &Call::StopAll { fade, ref keep } => {
                self.send(tick, Cmd::StopAll { fade, keep: keep.as_deref().map(Key::group) });
            }
            Call::Scope { name, keep, lazy } => self.scope(tick, name, &names(keep), &names(lazy))?,
            Call::ScopeMusic { ev, values, flow } => {
                self.scope_music(tick, ev, &names(values), flow.as_ref().map(MusicFlow::of))?;
            }
            Call::PreloadMusic { ev, values } => self.preload_music(tick, ev, &names(values))?,
            Call::ReleaseMusic { ev, values } => self.release_music(tick, ev, &names(values))?,
            Call::Resume => {
                self.send(tick, Cmd::Resume);
            }
        }
        Ok(())
    }

    /// A game parameter's global value as the mixer last set it.
    pub fn rtpc(&self, id: Id) -> Option<f64> {
        self.rtpc.get(id)
    }

    /// The mirror, for a reader off the game thread's `Audio` (the scenery's sound sink).
    pub fn rtpc_mirror(&self) -> Arc<RtpcMirror> {
        self.rtpc.clone()
    }

    /// Whether the build has an event of that name.
    pub fn has(&self, event: &str) -> bool {
        self.init.events.contains_key(&event.to_ascii_lowercase())
    }

    pub fn init(&self) -> &InitBank {
        &self.init
    }

    pub fn store(&self) -> &Arc<mvs_store::Store> {
        &self.store
    }

    /// The notes the mixer and the lazy media's posts sent since the last call; drops what they free.
    pub fn take_notes(&mut self) -> Vec<Note> {
        self.poll();
        self.read_notes();
        let mut out = std::mem::take(&mut self.inbox);
        out.append(&mut self.lazy.notes);
        out.extend(self.send.ends.drain(..).map(|post| Note::EventEnd { post }));
        out
    }

    /// The decoded media the scopes hold, and those of scopes let go until the next trim.
    pub fn media(&self) -> &decode::cache::MediaCache {
        &self.media
    }

    /// Loads a scope's banks and media (the events of `keep`, their media decoded; those of `lazy` decoded when
    /// first posted) and sends it to the mixer: [`ScopeSource::prepare`] here, then [`Audio::install_scope`].
    pub fn scope(&mut self, tick: u32, name: &str, keep: &[&str], lazy: &[&str]) -> Result<(), AudioError> {
        let offer = self.offered.scopes.iter().position(|p| p.name == name && p.keep == keep && p.lazy == lazy);
        let p = match offer {
            Some(k) => self.offered.scopes.swap_remove(k),
            None => self.source().prepare(name, keep, lazy)?,
        };
        self.install_scope(tick, p);
        Ok(())
    }

    /// Scopes prepared on another thread for the scope calls to come: each is installed by the first call of its name
    /// and lists ([`Audio::scope`]) instead of a load here; [`Audio::take_offers`] drops what no call took.
    pub fn offer_scopes(&mut self, scopes: Vec<PreparedScope>, music: Vec<PreparedMusic>) {
        self.offered.scopes.extend(scopes);
        self.offered.music.extend(music);
    }

    /// The offers no call took (dropped by the caller: their media go back to the cache), by name.
    pub fn take_offers(&mut self) -> Vec<String> {
        let music = self.offered.music.drain(..).map(|p| p.event);
        self.offered.scopes.drain(..).map(|p| p.name).chain(music).collect()
    }

    /// What a scope's load reads, for a thread of its own ([`ScopeSource::prepare`]).
    pub fn source(&self) -> ScopeSource {
        ScopeSource { store: self.store.clone(), init: self.init.clone(), media: self.media.clone() }
    }

    /// Sends a prepared scope to the mixer (its lazy media to the worker): no read, no decode. Its banks are merged
    /// off the mixer thread (`send`) and go before it.
    pub fn install_scope(&mut self, tick: u32, p: PreparedScope) {
        self.scope_stats.push(p.stat);
        let name = Key::group(&p.name);
        let mut load = p.load;
        self.lazy.scoped(name, &mut load);
        let banks = std::mem::take(&mut load.banks);
        self.send.hold(tick, name, banks);
        self.send(tick, Cmd::Scope { name, load });
    }

    /// Lets a scope go.
    pub fn unscope(&mut self, tick: u32, name: &str) {
        let name = Key::group(name);
        self.lazy.unscoped(name);
        self.send(tick, Cmd::Unscope { name });
        self.send.unhold(tick, name);
    }

    /// A music command, its load's banks merged first.
    fn send_music(&mut self, tick: u32, mut m: MusicCmd) {
        if let MusicCmd::Preload { media, .. } | MusicCmd::Scope { media, .. } = &mut m {
            let banks = std::mem::take(&mut media.banks);
            self.send.hold_music(tick, banks);
        }
        self.send(tick, Cmd::Music(m));
    }

    /// Holds the media of a music event's switch values (every playlist when none is named) and its bank.
    pub fn preload_music(&mut self, tick: u32, event: &str, values: &[&str]) -> Result<(), AudioError> {
        use ctl::music::{Reach, event_id, load};
        let values: Box<[Id]> = values.iter().map(|v| mvs_data::sound::short_id(v)).collect();
        let media = Box::new(load(&self.store, &self.init, &self.media, event, &values, Reach::Preload)?);
        self.send_music(tick, MusicCmd::Preload { event: event_id(event), values, media });
        Ok(())
    }

    /// Makes a music event's switch values the playing scope's, with the flow its switch values take.
    // Every state of the flow is held: the media are compressed entries the mixer streams, so no state's media are
    // decoded as the state moves.
    pub fn scope_music(
        &mut self,
        tick: u32,
        event: &str,
        values: &[&str],
        flow: Option<MusicFlow>,
    ) -> Result<(), AudioError> {
        let offer = self.offered.music.iter().position(|p| p.event == event && p.values == values && p.flow == flow);
        let media = match offer {
            Some(k) => self.offered.music.swap_remove(k).media,
            None => Box::new(music_scope_load(&self.store, &self.init, &self.media, event, values, flow.as_ref())?),
        };
        let values: Box<[Id]> = values.iter().map(|v| mvs_data::sound::short_id(v)).collect();
        let flow = flow.map(Box::new);
        self.send_music(tick, MusicCmd::Scope { event: ctl::music::event_id(event), values, flow, media });
        Ok(())
    }

    /// Lets a music event's switch values go. Its bank is read from the store once and kept; a failed read still
    /// sends the release (no media to let go) and is returned.
    pub fn release_music(&mut self, tick: u32, event: &str, values: &[&str]) -> Result<(), AudioError> {
        let values: Box<[Id]> = values.iter().map(|v| mvs_data::sound::short_id(v)).collect();
        let id = ctl::music::event_id(event);
        let mut read = Ok(());
        if !self.music_banks.iter().any(|(e, _)| *e == id) {
            match ctl::music::music_bank(&self.store, &self.init, event) {
                Ok(b) => self.music_banks.push((id, b)),
                Err(e) => read = Err(e),
            }
        }
        let media = self
            .music_banks
            .iter()
            .find(|(e, _)| *e == id)
            .map_or_else(Box::default, |(_, b)| ctl::music::reached(b, &values).into());
        self.send(tick, Cmd::Music(MusicCmd::Release { event: id, values, media }));
        read
    }

    /// Offline: mixes `frames` samples (a multiple of [`QUANTUM`]), taking the commands of `tick` first.
    pub fn pump(&mut self, tick: u32, frames: usize) {
        self.poll();
        let out::Run::Offline { mixer, out } = &mut self.run else {
            panic!("audio: pump on a live output");
        };
        assert!(frames.is_multiple_of(QUANTUM), "audio: pump of {frames} frames, not a multiple of {QUANTUM}");
        mixer.until = tick;
        let at = out.len();
        out.resize(at + frames * mixer.channels, 0.0);
        // (what waits for room on the ring goes between the quanta)
        for q in out[at..].chunks_exact_mut(QUANTUM * mixer.channels) {
            mixer.render(q);
            self.send.flush();
        }
    }

    /// Offline: the interleaved output mixed so far, taken.
    pub fn take_output(&mut self) -> Vec<f32> {
        match &mut self.run {
            out::Run::Offline { out, .. } => std::mem::take(out),
            out::Run::Wasapi(_) => Vec::new(),
        }
    }
}

/// A music scope's load: the event's bank and the media its values reach (with the flow's next values).
fn music_scope_load(
    store: &mvs_store::Store,
    init: &InitBank,
    media: &decode::cache::MediaCache,
    event: &str,
    values: &[&str],
    flow: Option<&MusicFlow>,
) -> Result<ctl::scope::ScopeLoad, AudioError> {
    use ctl::music::{Reach, flow_values, load};
    let values: Vec<Id> = values.iter().map(|v| mvs_data::sound::short_id(v)).collect();
    let held = match (flow, values.is_empty()) {
        (Some(f), false) => flow_values(&values, f),
        _ => values,
    };
    load(store, init, media, event, &held, Reach::Scope)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tape::post::Line;

    /// An Audio without a mixer, its command ring's reading end beside it.
    fn bare(init: InitBank) -> (Audio, rtrb::Consumer<Stamped>) {
        let (tx, cmds) = rtrb::RingBuffer::new(16);
        let (_notes, rx) = rtrb::RingBuffer::new(16);
        let rtpc = Arc::new(RtpcMirror::new(&init));
        // (a folder per call: the tests run at once)
        static N: AtomicU64 = AtomicU64::new(0);
        let n = N.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!("mvs-audio-bare-{}-{n}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("store.json"), r#"{"format":2,"buildId":"test","kinds":{}}"#).unwrap();
        let store = Arc::new(mvs_store::Store::open(&dir).unwrap());
        std::fs::remove_dir_all(&dir).unwrap();
        let run = out::Run::Wasapi(out::wasapi::Wasapi { thread: None, stop: Arc::default() });
        let media = Arc::new(decode::cache::MediaCache::new());
        let lazy = ctl::scope::Lazy::new(false, media.clone());
        (
            Audio {
                store,
                init: Arc::new(init),
                send: crate::send::Sender::new(tx, false),
                rx,
                inbox: Vec::new(),
                rtpc,
                run,
                next_post: 0,
                lazy,
                music_banks: Vec::new(),
                scope_stats: Vec::new(),
                media,
                offered: Offers::default(),
            },
            cmds,
        )
    }

    #[test]
    fn calls_by_name() {
        let mut init = InitBank::default();
        init.buses.insert(short_id_of("Rev_Small"), mvs_data::sound::Bus::default());
        init.buses.insert(short_id_of("Rev_Big"), mvs_data::sound::Bus::default());
        let (mut a, mut cmds) = bare(init);
        let text = concat!(
            r#"{"tick":5,"call":"post","ev":"Play_X","owner":1,"key":"STAGE:wind"}"#,
            "\n",
            r#"{"tick":6,"call":"setReverbVolumes","list":[{"bus":"Rev_Small","loc":[0,0,0],"scale":[1,2,-3]},{"bus":"Nope","loc":[0,0,0],"scale":[1,1,1]},{"bus":"Rev_Big","loc":[10,0,0],"scale":[1,1,1],"priority":5}]}"#,
            "\n",
            r#"{"tick":7,"call":"stopKey","key":"STAGE:wind","fade":0.05,"curve":"Log1"}"#,
            "\n",
            r#"{"tick":8,"call":"stopAll","fade":0.1,"keep":"STAGE"}"#,
            "\n",
        );
        for l in crate::tape::parse::<Line>(text, "test").unwrap() {
            a.call(&l).unwrap();
        }
        let got: Vec<Stamped> = std::iter::from_fn(|| cmds.pop().ok()).collect();
        let Cmd::Post(p) = &got[0].cmd else { panic!("{:?}", got[0]) };
        assert_eq!((got[0].tick, p.id, p.event, p.obj), (5, PostId(1), short_id_of("play_x"), Some(GameObj(1))));
        assert_eq!(p.key, Some(Key::new("STAGE:wind")));
        let Cmd::SetReverbVolumes(v) = &got[1].cmd else { panic!("{:?}", got[1]) };
        assert_eq!(v.iter().map(|r| r.bus).collect::<Vec<_>>(), [short_id_of("Rev_Big"), short_id_of("Rev_Small")]);
        assert_eq!((v[1].lo, v[1].hi, v[1].rate), ([-100.0, -200.0, -300.0], [100.0, 200.0, 300.0], 0.5));
        assert!(matches!(&got[2].cmd, Cmd::StopKey { curve: Some(Curve::Log1), .. }));
        let Cmd::StopAll { keep: Some(keep), .. } = got[3].cmd else { panic!("{:?}", got[3]) };
        assert_eq!(keep, p.key.unwrap().under);
        assert!(curve_named("Bogus").is_err());
    }

    fn short_id_of(s: &str) -> Id {
        mvs_data::sound::short_id(s)
    }

    #[test]
    fn a_full_ring_loses_nothing() {
        // (the bare ring holds 16: 40 commands, a stop among them, go over three frames in order)
        let (mut a, mut cmds) = bare(InitBank::default());
        for i in 0..40u32 {
            let cmd =
                if i == 20 { Cmd::StopAll { fade: 0.1, keep: None } } else { Cmd::SetState { group: i, value: i } };
            a.send(i, cmd);
        }
        assert_eq!(a.backlog(), 24);
        let mut got = Vec::new();
        for _ in 0..3 {
            got.extend(std::iter::from_fn(|| cmds.pop().ok()));
            a.poll();
        }
        assert_eq!(got.iter().map(|s| s.tick).collect::<Vec<_>>(), (0..40).collect::<Vec<_>>());
        assert!(matches!(got[20].cmd, Cmd::StopAll { keep: None, .. }));
        assert_eq!((a.backlog(), a.dropped()), (0, 0));
    }

    #[test]
    fn keys_group() {
        let k = Key::new("STAGE_AMBIENCE:wind");
        assert_eq!(k.under, Key::group("STAGE_AMBIENCE"));
        assert_ne!(k.id, k.under);
        let lone = Key::new("charge");
        assert_eq!(lone.id, lone.under);
    }

    #[test]
    fn mirror() {
        let mut init = InitBank::default();
        init.rtpc_defaults.insert(7, 0.5);
        init.rtpc_defaults.insert(3, 1.0);
        let m = RtpcMirror::new(&init);
        assert_eq!(m.get(7), Some(0.5));
        assert!(m.set(3, 2.0));
        assert_eq!(m.get(3), Some(2.0));
        assert!(!m.set(4, 1.0));
        assert_eq!(m.get(4), None);
    }
}
