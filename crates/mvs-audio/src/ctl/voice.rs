//! The interpreter's voices: a sound's plan from its node chain (volume, pitch, filters, bus, aux sends,
//! positioning, attenuation, game-parameter curves), the playback limits and the virtual voices, and the per-frame
//! rules that move a playing voice's parameters.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use indexmap::IndexMap;
use mvs_data::sound::{Att, Curve, Id, MeterParams, Node, Obj, PlayMode, RandomMode, Rtpc, TransitionMode};

use super::curves::{
    ContinuousPlaylist, FRAME_SECS, curve_value, db_to_gain, eval_cutoff, interp, next_frame_after, stop_ratios,
};
use super::pan::{LFE_BIT, PAN_DIRS, RING_BITS, STEREO_MIX, VORBIS_ORDER, att_curve, att_lin, speaker_volumes};
use super::sync::Syncs;
use super::{Ctl, Out, samples};
use crate::math::{log10, pow};
use crate::spec::{
    Control, Fade, MAX_CHANNELS, MAX_INSERTS, MAX_SENDS, Param, ParamChange, Ramp, Send, Target, VoiceId, VoiceSpec,
};
use crate::{Emitter, GameObj, Key, Listener, PostId, V3};

/// A trigger-rate container's next sound is warmed this long before its start (a decoder would start then; here
/// only the timer's steps keep to it).
const TRIGGER_WARM: f64 = 2.0;

/// The game object of a global post: never placed, it is heard from the origin facing the integration's +X.
// Unverified: that start (the origin, facing the sound engine's default front and top, the integration's +X and +Z)
// is the SDK's documented default, not read in the engine's code.
const GLOBAL_EMITTER: Emitter = Emitter { pos: [0.0, 0.0, 0.0], front: [1.0, 0.0, 0.0], top: [0.0, 0.0, 1.0] };

const SQRT1_2: f64 = std::f64::consts::FRAC_1_SQRT_2;

/// A Web Audio AudioParam holds a float.
fn fround(v: f64) -> f64 {
    f64::from(v as f32)
}

/// A linear gain in dB.
fn db_of(g: f64) -> f64 {
    if g > 0.0 { 20.0 * log10(g) } else { f64::NEG_INFINITY }
}

/// What a sound's post takes from the bank, the same at every post: kept per sound.
#[derive(Clone, Debug, Default)]
pub struct Plan {
    /// The node chain from the sound up, and their ids.
    pub nodes: Vec<Node>,
    pub ids: Vec<Id>,
    /// The Volume (0), Pitch (2) and MakeUpGain (7) curves of the nodes, then the Volume and MakeUpGain curves of
    /// the buses above the voice.
    pub rtpc: Vec<Rtpc>,
    /// The output bus (None: the master).
    pub bus: Option<Id>,
    /// The nodes the bus, the user aux sends and the HDR envelope are read from (index into `nodes`).
    pub bus_of: Option<usize>,
    pub aux_of: Option<usize>,
    pub env_of: usize,
    /// GameAuxSendVolume, dB, when the chain uses game-defined aux sends.
    pub gaux_db: Option<f64>,
    /// Loudness normalisation on.
    pub norm: bool,
    pub att: Option<Att>,
    /// The 3D spatialization bits (0: none).
    pub mode3d: u8,
}

/// A voice parameter positioning moves: the value last asked for, the float the
/// parameter holds once it is there, the audio clock time (s) after its last ramp has run (0: set at once), and the
/// parameter's value as a Web Audio AudioParam reads it (the float of the last value set or ramped to). Values
/// are the audio graph's units (a filter's in Hz).
#[derive(Clone, Copy, Debug)]
pub struct Track {
    pub param: Param,
    pub want: f64,
    pub held: f64,
    pub end: f64,
    pub value: f64,
}

impl Track {
    pub fn new(param: Param, value: f64) -> Track {
        Track { param, want: f64::NAN, held: f64::NAN, end: 0.0, value: fround(value) }
    }

    /// AudioParam.value of the track, not read when its last ramp has run.
    pub fn read(&self, t: f64) -> f64 {
        if !self.held.is_nan() && t >= self.end { self.held } else { self.value }
    }
}

/// A positioned voice's parameters.
#[derive(Clone, Debug)]
pub struct Spatial {
    /// A sound posted at a location keeps it; None: the object's emitter.
    pub em: Option<Emitter>,
    pub att: Option<Att>,
    /// The directions of the panned channels (None: not panned), the decoded channel each is, and its left and
    /// right gains.
    pub dirs: Option<Vec<V3>>,
    pub pan_ch: Vec<u8>,
    pub pan: Vec<[Track; 2]>,
    pub dist: Track,
    /// The user aux sends' base gains (send slots 0..).
    pub base: Vec<f64>,
    pub lpf: Option<Track>,
    pub hpf: Option<Track>,
}

/// A voice's game-defined aux sends: one per reverb volume, in the send slots after the
/// user sends.
#[derive(Clone, Debug)]
pub struct GameSends {
    pub em: Option<Emitter>,
    pub first: usize,
    pub base: f64,
}

/// What a voice pushes into its HDR bus: its volume (dB, without make-up and
/// normalisation), the medium's envelope (when the chain enables it) and peak, HDRActiveRange, the medium's rate
/// and length, and the looped region in samples.
#[derive(Clone, Debug)]
pub struct VoiceHdr {
    pub bus: Id,
    pub vol: f64,
    /// The medium whose envelope counts (None: no envelope).
    pub env: Option<Id>,
    pub peak: f64,
    pub range: f64,
    pub rate: f64,
    pub samples: f64,
    pub looped: bool,
    pub l0: f64,
    pub l1: f64,
}

/// A playing voice as the interpreter tracks it.
#[derive(Clone, Debug)]
pub struct Voice {
    pub id: VoiceId,
    pub post: PostId,
    /// The object it plays on (the global object when the post named none).
    pub obj: GameObj,
    pub key: Option<Key>,
    /// The sound and its plan.
    pub node: Id,
    pub plan: Arc<Plan>,
    /// When it starts, s (moved by a pause).
    pub start: f64,
    /// When it runs out by itself, s (infinite: never).
    pub end: f64,
    /// The play order (the voice's number) and priority.
    pub seq: u32,
    pub prio: f64,
    /// The resolved virtual voice behaviour: 0 continue to play, 1 kill, 2 virtual.
    pub below: u8,
    /// The limiters it sits in.
    pub lims: Vec<LimKey>,
    /// The static volume (dB, without make-up and normalisation) and the output bus volume (dB).
    pub vol: f64,
    pub dry: f64,
    pub bus: Option<Id>,
    /// The aux sends by slot: their bus and gain.
    pub sends: Vec<(Id, Track)>,
    /// Forced virtual by a limiter this frame; every connection at or under the threshold; silent (forced,
    /// paused or muted).
    pub forced: bool,
    pub under: bool,
    pub silent: bool,
    /// When a Pause action stopped it, s.
    pub paused: Option<f64>,
    /// The static pitch, cents, and the playback rate as a Web Audio AudioParam holds it (a float).
    pub cents: f64,
    pub rate: f64,
    /// The play action's fade-in.
    pub fade: Fade,
    pub streamed: bool,
    pub hdr: Option<VoiceHdr>,
    pub spat: Option<Spatial>,
    pub game: Option<GameSends>,
    /// A child of a continuous container: its chain and how long it runs (negative: it does not end by itself).
    pub chain: Option<u32>,
    pub runs: f64,
}

/// A limiter's key: kind (0 global, 1 bus, 2 node), id, and the object for a per-object node limiter.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct LimKey {
    pub kind: u8,
    pub id: Id,
    pub obj: Option<GameObj>,
}

/// A limiter's maximum, flags (1 kill newest, 2 use virtual voice behaviour) and the engine's sort key.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Limiter {
    max: f64,
    lim: u8,
    ord: [u64; 3],
}

/// A post's options as they go down the hierarchy.
#[derive(Clone, Debug, Default)]
pub(crate) struct Opts {
    pub post: PostId,
    pub obj: Option<GameObj>,
    pub key: Option<Key>,
    pub stop_key: Option<Key>,
    pub at: Option<V3>,
    /// The container whose initial delay has been taken already.
    pub skip_delay: Option<Id>,
    /// The continuous container's chain a voice continues.
    pub chain: Option<u32>,
    /// `Post::sure`.
    pub sure: bool,
}

impl Opts {
    pub fn owner(&self) -> GameObj {
        self.obj.unwrap_or(GameObj::GLOBAL)
    }
}

/// What a stop is tested on: a voice's or a waiting container's key, object and node chain.
pub(crate) struct Who<'a> {
    pub key: Option<Key>,
    pub owner: GameObj,
    pub chain: &'a [Id],
    pub seq: Option<u32>,
}

/// A trigger-rate container between two of its sounds.
#[derive(Clone, Debug)]
struct Trigger {
    n: u32,
    key: Option<Key>,
    owner: GameObj,
    chain: Vec<Id>,
    playlist: ContinuousPlaylist,
    cur: Id,
    at: f64,
    warm: bool,
    fade: Fade,
    opts: Opts,
    /// TransitionTime and its two modifiers, ms.
    tt: [f64; 3],
    /// The step timer: due time (s) and the order it was set in.
    due: Option<(f64, u64)>,
}

/// A continuous container with transition mode Disabled: its picks and what its next child is played with.
#[derive(Clone, Debug)]
struct Chain {
    playlist: ContinuousPlaylist,
    curve: Curve,
    opts: Opts,
    path: Vec<Id>,
}

/// The playing voices.
#[derive(Default)]
pub struct Voices {
    /// In play order; a voice leaves when it is stopped or runs out.
    pub list: Vec<Voice>,
    seq: u32,
    plans: HashMap<Id, Arc<Plan>>,
    recent: HashMap<Id, Vec<Id>>,
    seq_index: HashMap<Id, u32>,
    limiters: IndexMap<LimKey, Limiter>,
    lim_order: Option<Vec<LimKey>>,
    triggers: Vec<Trigger>,
    trigger_n: u32,
    timer_n: u64,
    chains: HashMap<u32, Chain>,
    chain_n: u32,
    /// The per-frame pass's interval: when it was set (samples), and the audio frame it last ran in.
    pub(crate) timer: Option<u64>,
    last: i64,
}

impl Voices {
    /// The live voices that read a game parameter: (index).
    fn reads(&self, i: usize, id: Id) -> bool {
        self.list[i].plan.rtpc.iter().any(|r| r.id == id)
    }
}

fn change(out: &mut Out, at: f64, voice: VoiceId, param: Param, value: f64, ramp: Ramp) {
    Ctl::change(out, ParamChange { at: samples(at), target: Target::Voice(voice), param, value, ramp });
}

/// The bus above `bus` and whether `bus` is under one of `set` (the voice's own bus, else the master).
pub(crate) fn bus_under(ctl: &Ctl, bus: Option<Id>, set: &[Id]) -> bool {
    let mut b = bus.or(ctl.master);
    while let Some(id) = b {
        if set.contains(&id) {
            return true;
        }
        b = ctl.init.buses.get(&id).and_then(|d| d.parent);
    }
    false
}

// ---- playing a node ---------------------------------------------------------------------------------------------------

/// Plays a node of the actor-mixer hierarchy at `when` s.
#[allow(clippy::too_many_arguments)]
pub(crate) fn play_node(ctl: &mut Ctl, id: Id, when: f64, fade: Fade, opts: &Opts, path: &[Id], t: f64, out: &mut Out) {
    let Some(o) = ctl.banks.obj(id).cloned() else {
        ctl.warn("missing node", u64::from(id), out);
        return;
    };
    let mut p = path.to_vec();
    p.push(id);
    match o {
        Obj::SoundSfxVoice(_) => play_voice(ctl, id, when, fade, opts, t, out),
        Obj::RandomSequenceContainer(c) => {
            // the container picks from its own playlist, never from its children
            let list = &c.playlist;
            if list.is_empty() {
                return;
            }
            if c.continuous {
                // A looping container (LoopCount 0: the stages' ambient emitters, c021's idle spark) refills its
                // playlist and plays on until a stop (a plain node plays one sound for LoopCount 1).
                let plain = c.mode == PlayMode::Random
                    && c.random_mode == RandomMode::Normal
                    && list.iter().all(|x| x.weight == list[0].weight);
                if c.transition == TransitionMode::TriggerRate && plain {
                    play_trigger_rate(ctl, id, &c, when, fade, opts, p, t, out);
                    return;
                }
                if c.transition == TransitionMode::Disabled
                    && plain
                    && list.iter().all(|x| matches!(ctl.banks.obj(x.id), Some(Obj::SoundSfxVoice(_))))
                {
                    play_continuous(ctl, &c, when, fade, opts, p, t, out);
                    return;
                }
                ctl.warn("continuous container mode not ported: one sound played", u64::from(id), out);
            }
            let pick = if c.mode == PlayMode::Sequence {
                let i = ctl.voices.seq_index.get(&id).copied().unwrap_or(0);
                ctl.voices.seq_index.insert(id, i + 1);
                list[i as usize % list.len()].id
            } else {
                let hist = ctl.voices.recent.remove(&id).unwrap_or_default();
                let avoid = usize::from(c.avoid_repeat).min(list.len() - 1);
                let recent = &hist[hist.len().saturating_sub(avoid)..];
                let pool: Vec<_> = list.iter().filter(|x| avoid == 0 || !recent.contains(&x.id)).collect();
                let total: f64 = pool.iter().map(|x| f64::from(x.weight)).sum();
                let mut r = ctl.draw(out) * total;
                let mut pick = pool[pool.len() - 1].id;
                for x in &pool {
                    r -= f64::from(x.weight);
                    if r < 0.0 {
                        pick = x.id;
                        break;
                    }
                }
                let mut hist = hist;
                hist.push(pick);
                if hist.len() > 8 {
                    hist.remove(0);
                }
                ctl.voices.recent.insert(id, hist);
                pick
            };
            play_node(ctl, pick, when, fade, opts, &p, t, out);
        }
        Obj::SwitchContainer(c) => {
            // a switch group reads the owner's game object, else the default switch
            let sw = (c.group_type == 0)
                .then(|| ctl.syncs.switch(c.group, opts.owner()))
                .flatten()
                .filter(|&v| v != 0)
                .unwrap_or(c.default);
            if let Some(pkg) = c.packages.iter().find(|s| s.switch == sw) {
                for n in &pkg.nodes {
                    play_node(ctl, *n, when, fade, opts, &p, t, out);
                }
            }
        }
        Obj::LayerContainer(c) => {
            for n in &c.children {
                play_node(ctl, *n, when, fade, opts, &p, t, out);
            }
        }
        _ => ctl.warn("unsupported play target", u64::from(id), out),
    }
}

/// A continuous random container with the trigger rate transition: the sounds
/// start one transition time apart, whatever their lengths.
// Unverified: a fresh playlist per post (IsGlobal and g_kMinimumTriggerRateDelay are not read).
#[allow(clippy::too_many_arguments)]
fn play_trigger_rate(
    ctl: &mut Ctl,
    id: Id,
    c: &mvs_data::sound::RanSeq,
    when: f64,
    fade: Fade,
    opts: &Opts,
    path: Vec<Id>,
    t: f64,
    out: &mut Out,
) {
    let mut playlist = ContinuousPlaylist::new(c.playlist.iter().map(|x| x.id).collect(), c.avoid_repeat, c.loop_count);
    let r = ctl.draw(out);
    let cur = playlist.next(r).expect("a playlist that is not empty");
    let at = when + c.node.p.get(59).unwrap_or(0.0) + ctl.rand(c.node.r.get(59), out);
    ctl.voices.trigger_n += 1;
    let n = ctl.voices.trigger_n;
    ctl.voices.triggers.push(Trigger {
        n,
        key: opts.key,
        owner: opts.owner(),
        chain: path,
        playlist,
        cur,
        at,
        warm: false,
        fade,
        opts: Opts { skip_delay: Some(id), ..opts.clone() },
        tt: [c.transition_time, c.transition_mod[0], c.transition_mod[1]],
        due: None,
    });
    trigger_step(ctl, n, t, out);
}

/// The least time between two sounds of a trigger-rate container (s): one quantum.
const MIN_TRIGGER_STEP: f64 = crate::QUANTUM as f64 / crate::RATE as f64;

/// A trigger-rate container's step: waits on its timer until the next sound is due, then plays it.
fn trigger_step(ctl: &mut Ctl, n: u32, t: f64, out: &mut Out) {
    loop {
        let Some(i) = ctl.voices.triggers.iter().position(|x| x.n == n) else { return };
        let tr = &mut ctl.voices.triggers[i];
        tr.due = None;
        let left = tr.at - t;
        if left > 0.2 {
            if !tr.warm && left <= TRIGGER_WARM + 0.5 {
                tr.warm = true;
            }
            let ms = (left - if tr.warm { 0.1 } else { TRIGGER_WARM }).min(5.0) * 1000.0;
            ctl.voices.timer_n += 1;
            let tr = &mut ctl.voices.triggers[i];
            tr.due = Some((t + ms / 1000.0, ctl.voices.timer_n));
            return;
        }
        tr.warm = false;
        let nxt = if tr.playlist.done() {
            None
        } else {
            let r = ctl.draw(out);
            ctl.voices.triggers[i].playlist.next(r)
        };
        // (its options and path lent to the play, given back if it is still there: no copy per sound)
        let tr = &mut ctl.voices.triggers[i];
        let (cur, at, fade) = (tr.cur, tr.at, tr.fade);
        let (opts, chain) = (std::mem::take(&mut tr.opts), std::mem::take(&mut tr.chain));
        play_node(ctl, cur, at, fade, &opts, &chain, t, out);
        let Some(i) = ctl.voices.triggers.iter().position(|x| x.n == n) else { return };
        let Some(nxt) = nxt else {
            ctl.voices.triggers.remove(i);
            return;
        };
        let r = ctl.draw(out);
        let tr = &mut ctl.voices.triggers[i];
        (tr.opts, tr.chain) = (opts, chain);
        tr.cur = nxt;
        // Unverified: DelayPlayback's floor g_kMinimumTriggerRateDelay is not read (the build's two containers step
        // 30 s); one quantum stands for it, so that a container of no transition time and no loop count cannot play
        // for ever within one step.
        tr.at += ((tr.tt[0] + tr.tt[1] + r * (tr.tt[2] - tr.tt[1])) / 1000.0).max(MIN_TRIGGER_STEP);
    }
}

/// A continuous random container with transition mode Disabled: the next child
/// plays at the first audio frame after the last one ran out; a stop ends the chain. Only the first child takes the
/// action's fade-in and the container's own initial delay.
// Unverified: a child that does not start ends the chain (the build plays the next pick 10 frames later).
#[allow(clippy::too_many_arguments)]
fn play_continuous(
    ctl: &mut Ctl,
    c: &mvs_data::sound::RanSeq,
    when: f64,
    fade: Fade,
    opts: &Opts,
    path: Vec<Id>,
    t: f64,
    out: &mut Out,
) {
    let mut playlist = ContinuousPlaylist::new(c.playlist.iter().map(|x| x.id).collect(), c.avoid_repeat, c.loop_count);
    let r = ctl.draw(out);
    let pick = playlist.next(r).expect("a playlist that is not empty");
    let cntr = *path.last().expect("the container is on its path");
    ctl.voices.chain_n += 1;
    let n = ctl.voices.chain_n;
    let later = Opts { skip_delay: Some(cntr), chain: Some(n), ..opts.clone() };
    ctl.voices.chains.insert(n, Chain { playlist, curve: fade.curve, opts: later, path: path.clone() });
    let first = Opts { chain: Some(n), ..opts.clone() };
    play_node(ctl, pick, when, fade, &first, &path, t, out);
}

/// What playVoice takes from the bank for a sound, the same at every post.
fn plan(ctl: &mut Ctl, id: Id, out: &mut Out) -> Arc<Plan> {
    if let Some(p) = ctl.voices.plans.get(&id) {
        return p.clone();
    }
    let mut p = Plan::default();
    let mut gaux_done = false;
    let mut cur = Some(id);
    let mut depth = 0;
    let mut warns = Vec::new();
    while let (Some(c), true) = (cur, depth < 32) {
        depth += 1;
        let Some(o) = ctl.banks.obj(c) else { break };
        let n = o.node().cloned().unwrap_or_default();
        p.rtpc.extend(n.rtpc.iter().filter(|r| matches!(r.param, 0 | 2 | 7)).cloned());
        for r in &n.rtpc {
            if !matches!(r.param, 0 | 2 | 7 | 16 | 17 | 39..=43) {
                warns.push(c);
            }
        }
        let i = p.nodes.len();
        if p.bus.is_none() && n.bus.is_some_and(|b| b != 0) {
            p.bus = n.bus;
            p.bus_of = Some(i);
        }
        if p.aux_of.is_none() && (n.aux_ov || n.parent.is_none()) {
            p.aux_of = Some(i);
        }
        // game-defined sends: the flag and GameAuxSendVolume of the first node that overrides the setting, else
        // of the top node
        if !gaux_done && (n.gaux & 1 != 0 || n.parent.is_none()) {
            gaux_done = true;
            if n.gaux & 2 != 0 {
                p.gaux_db = Some(n.p.get(23).unwrap_or(0.0));
            }
        }
        cur = n.parent;
        p.ids.push(c);
        p.nodes.push(n);
    }
    for c in warns {
        ctl.warn("RTPC target not ported", u64::from(c), out);
    }
    // HdrEnvelopeFlags are inherited from the nearest node that overrides them, else the top node
    let inherit = |bit: u8| p.nodes.iter().position(|n| n.hdr & bit != 0).unwrap_or(p.nodes.len().saturating_sub(1));
    // the Volume / MakeUpGain curves of the buses above the voice count as its own
    let mut b = p.bus.or(ctl.master);
    while let Some(id) = b {
        let Some(d) = ctl.init.buses.get(&id) else { break };
        p.rtpc.extend(d.rtpc.iter().filter(|r| matches!(r.param, 0 | 7)).cloned());
        b = d.parent;
    }
    // positioning from the nearest node that overrides it, with that node's attenuation share set
    if let Some(pn) = p.nodes.iter().find(|n| n.pos.is_some()) {
        let pos = pn.pos.expect("found by it");
        if pos[0] & 2 != 0 {
            if pos[1] & 8 != 0 {
                p.att = pn.att.and_then(|a| ctl.init.atts.get(&a)).cloned();
            }
            p.mode3d = pos[1] & 3;
        }
    }
    let ni = inherit(2);
    p.norm = p.nodes.get(ni).is_some_and(|n| n.hdr & 4 != 0);
    p.env_of = inherit(1);
    let p = Arc::new(p);
    ctl.voices.plans.insert(id, p.clone());
    p
}

/// Plays a sound.
fn play_voice(ctl: &mut Ctl, id: Id, when: f64, fade_in: Fade, opts: &Opts, t: f64, out: &mut Out) {
    let Some(Obj::SoundSfxVoice(s)) = ctl.banks.obj(id).cloned() else { return };
    // the Silence source: a deliberate "play nothing" pick
    if s.plugin.as_deref() == Some("AkFxSrcSilenceSource") {
        return;
    }
    let media = ctl.banks.media(s.src).cloned();
    let pl = plan(ctl, id, out);
    let owner = opts.owner();
    // the output bus volume (RTPC 43 of the node that carries the bus) and the user aux sends are read at the start
    let mut dry_db = 0.0;
    if let Some(bi) = pl.bus_of {
        for r in pl.nodes[bi].rtpc.iter().filter(|r| r.param == 43) {
            dry_db += curve_value(r, ctl.syncs.rtpc(r.id, owner));
        }
    }
    let mut sends: Vec<(Id, f64)> = Vec::new();
    if let Some(ai) = pl.aux_of {
        let an = &pl.nodes[ai];
        for (i, &aid) in an.aux.iter().enumerate() {
            if aid == 0 || !ctl.init.buses.contains_key(&aid) {
                continue;
            }
            let i = i as u16;
            let mut sdb = an.p.get(19 + i).unwrap_or(0.0);
            for r in an.rtpc.iter().filter(|r| r.param == 39 + i) {
                sdb += curve_value(r, ctl.syncs.rtpc(r.id, owner));
            }
            if sdb > -96.3 {
                sends.push((aid, db_to_gain(sdb).min(4.0)));
            }
        }
    }
    // the props up the hierarchy, each node's ranges drawn in this order
    let (mut db, mut vol, mut cents, mut lpf, mut hpf, mut delay) = (0.0, 0.0, 0.0, 0.0, 0.0, 0.0);
    for (i, n) in pl.nodes.iter().enumerate() {
        vol += n.p.get(0).unwrap_or(0.0) + ctl.rand(n.r.get(0), out);
        db += n.p.get(6).unwrap_or(0.0) + ctl.rand(n.r.get(6), out);
        cents += n.p.get(2).unwrap_or(0.0) + ctl.rand(n.r.get(2), out);
        lpf += n.p.get(3).unwrap_or(0.0) + ctl.rand(n.r.get(3), out);
        hpf += n.p.get(4).unwrap_or(0.0) + ctl.rand(n.r.get(4), out);
        if Some(pl.ids[i]) != opts.skip_delay {
            delay += n.p.get(59).unwrap_or(0.0) + ctl.rand(n.r.get(59), out);
        }
    }
    let makeup = db;
    let norm = if pl.norm { media.as_ref().and_then(|m| m.norm).unwrap_or(0.0) } else { 0.0 };
    let env_node = &pl.nodes[pl.env_of];
    let use_env = env_node.hdr & 8 != 0 && media.as_ref().is_some_and(|m| !m.env.is_empty());
    let bus = pl.bus;
    let now = samples(t);
    let syncs = &ctl.syncs;
    ctl.buses.touch(bus.or(ctl.master).unwrap_or(0), now, syncs, out);
    let loops = s.node.p.get(58);
    let Some(adm) = admit(ctl, &pl, bus, owner, loops == Some(0.0), t, out) else { return };
    // a sound whose behaviour is kill does not start when its volume is at or under the threshold
    if adm.below == 1 {
        let mut v0 = vol;
        for r in pl.rtpc.iter().filter(|r| r.param == 0) {
            v0 += curve_value(r, ctl.syncs.rtpc(r.id, owner));
        }
        if v0 <= ctl.init.volume_threshold {
            return;
        }
    }
    if adm.below == 2 {
        let vn = pl.nodes.iter().find(|n| n.lim & 16 != 0).unwrap_or(&pl.nodes[pl.nodes.len() - 1]);
        if vn.vq != 1 {
            ctl.warn("virtual queue behaviour not ported", u64::from(id), out);
        }
    }
    let Some(media) = media else {
        ctl.warn("missing medium", u64::from(s.src), out);
        return;
    };
    let start = when + delay;
    let streamed = ctl.streaming && ctl.banks.streamed(s.src);
    let (rt_db, rt_cents) = ctl.syncs.rtpc_of(&pl.rtpc, owner);
    let len = f64::from(media.frames) / f64::from(media.rate);
    let rate = if streamed { 1.0 } else { fround(pow(2.0, (cents + rt_cents) / 1200.0)) };
    // the medium's loop points: the part before the loop start plays once, then the loop repeats
    // Unverified: the source's own use of the points is not read in the sound engine (CAkSrcFileVorbis).
    let mut loop_points = None;
    if loops.is_some() && !streamed {
        let r = f64::from(media.rate);
        let (mut l0, mut l1) = match media.loop_points {
            Some([a, b]) => ((f64::from(a) / r).min(len), (f64::from(b) / r).min(len)),
            None => (0.0, len),
        };
        if l1 <= l0 {
            (l0, l1) = (0.0, len);
        }
        if l0 != 0.0 || l1 != len {
            loop_points = Some([(l0 * r).round() as u32, (l1 * r).round() as u32]);
        }
    }
    // the insert effects of the nearest node that has them: meters only
    let mut meters: [Option<MeterParams>; MAX_INSERTS] = [None; MAX_INSERTS];
    let mut mi = 0;
    if let Some(n) = pl.nodes.iter().find(|n| !n.fx.is_empty()) {
        for fid in &n.fx {
            if let Some(Obj::FxShareSet(f) | Obj::FxCustom(f)) = ctl.banks.obj(*fid)
                && let mvs_data::sound::FxParams::AkMeterFX(m) = &f.fx
                && mi < MAX_INSERTS
            {
                meters[mi] = Some(*m);
                mi += 1;
            }
        }
    }
    let t0 = start.max(t);
    let channels = usize::from(media.channels);
    // the speaker bit of each decoded channel of a source with more than two
    let mut bits: Option<Vec<u8>> = None;
    if channels > 2 {
        let order = VORBIS_ORDER.get(channels).filter(|o| !o.is_empty());
        let set: Vec<u8> = match media.mask {
            Some(m) => (0..STEREO_MIX.len() as u8).filter(|b| m >> b & 1 != 0).collect(),
            None => Vec::new(),
        };
        match order {
            Some(o) if set.len() == channels && media.mask.is_some_and(|m| m >> STEREO_MIX.len() == 0) => {
                bits = Some(o.iter().map(|&c| set[usize::from(c)]).collect());
            }
            _ => ctl.warn("channel layout not ported", u64::from(s.src), out),
        }
    }
    let full: Option<Vec<u8>> = bits.as_ref().map(|b| b.iter().copied().filter(|&x| x != LFE_BIT).collect());
    let mut ring: Option<Vec<V3>> = None;
    if let (Some(f), true) = (&full, pl.mode3d != 0)
        && f.len() > 2
    {
        let rb = RING_BITS.get(f.len()).copied().unwrap_or(&[]);
        if !rb.is_empty() && f.iter().all(|b| rb.contains(b)) {
            ring = Some(
                f.iter().map(|b| PAN_DIRS[f.len()][rb.iter().position(|x| x == b).expect("in the ring")]).collect(),
            );
        } else {
            ctl.warn("3D panning of a speaker mask not ported", u64::from(s.src), out);
        }
    }
    // the decoded channel each panned channel is
    let mut n_in = channels;
    let mut pan_ch: Vec<u8> = (0..channels as u8).collect();
    let mut mix_bits = bits.clone();
    if let (Some(b), Some(f), true) = (&bits, &full, pl.mode3d != 0)
        && (f.len() <= 2 || ring.is_some())
    {
        // 3D: the LFE is not panned, the rest is the source the panner sees
        pan_ch = f.iter().map(|x| b.iter().position(|y| y == x).expect("a full channel") as u8).collect();
        n_in = f.len();
        mix_bits = None;
    }
    let mut spat: Option<Spatial> = None;
    let att = pl.att.clone();
    if att.is_some() || pl.mode3d != 0 {
        let lpf_t = att.as_ref().is_some_and(|a| a.uses[3] >= 0).then(|| Track::new(Param::AttLpf, 24_000.0));
        let hpf_t = att.as_ref().is_some_and(|a| a.uses[4] >= 0).then(|| Track::new(Param::AttHpf, 0.0));
        let dirs: Option<Vec<V3>> = if pl.mode3d == 0 {
            None
        } else if n_in <= 2 {
            Some(PAN_DIRS[n_in].to_vec())
        } else {
            ring.clone()
        };
        if pl.mode3d == 1 {
            ctl.warn("position-only spatialization played with the emitter orientation", u64::from(id), out);
        }
        let mut pan = Vec::new();
        if let Some(d) = &dirs {
            if opts.at.is_none() && owner != GameObj::GLOBAL && !ctl.emitters.contains_key(&owner.0) {
                ctl.warn("3D sound without an emitter", u64::from(id), out);
            }
            for (c, &ch) in pan_ch.iter().enumerate().take(d.len()) {
                let g = |side: usize| {
                    if n_in == 2 {
                        f64::from(u8::from(side == c))
                    } else if n_in == 1 {
                        SQRT1_2
                    } else {
                        0.5
                    }
                };
                pan.push([Track::new(Param::Pan { ch, side: 0 }, g(0)), Track::new(Param::Pan { ch, side: 1 }, g(1))]);
            }
        }
        spat = Some(Spatial {
            em: opts.at.map(|pos| Emitter { pos, front: [1.0, 0.0, 0.0], top: [0.0, 0.0, 1.0] }),
            att,
            dirs,
            pan_ch: pan_ch.clone(),
            pan,
            dist: Track::new(Param::Dist, 1.0),
            base: Vec::new(),
            lpf: lpf_t,
            hpf: hpf_t,
        });
    }
    // the aux sends: the user sends, then the game-defined ones
    let mut vsends: Vec<(Id, Track)> = Vec::new();
    for &(aid, g) in &sends {
        let syncs = &ctl.syncs;
        ctl.buses.touch(aid, now, syncs, out);
        vsends.push((aid, Track::new(Param::Send(vsends.len() as u8), g)));
        if let Some(sp) = &mut spat {
            sp.base.push(g);
        }
    }
    let mut game = None;
    if let Some(gdb) = pl.gaux_db
        && gdb > -96.3
        && !ctl.reverb.is_empty()
        && (opts.at.is_some() || ctl.emitters.contains_key(&owner.0))
    {
        let em = opts.at.map(|pos| {
            spat.as_ref().and_then(|s| s.em).unwrap_or(Emitter { pos, front: [1.0, 0.0, 0.0], top: [0.0, 0.0, 1.0] })
        });
        game = Some(GameSends { em, first: vsends.len(), base: db_to_gain(gdb).min(4.0) });
        for r in ctl.reverb.clone() {
            let syncs = &ctl.syncs;
            ctl.buses.touch(r.bus, now, syncs, out);
            vsends.push((r.bus, Track::new(Param::Send(vsends.len() as u8), 0.0)));
        }
    }
    let n_sends = vsends.len().min(MAX_SENDS);
    let mut hdr_bus = None;
    let mut b = bus.or(ctl.master);
    while let Some(bid) = b {
        let Some(d) = ctl.init.buses.get(&bid) else { break };
        if d.hdr.is_some() {
            hdr_bus = Some(bid);
            break;
        }
        b = d.parent;
    }
    let hdr = hdr_bus.map(|hb| VoiceHdr {
        bus: hb,
        vol,
        env: use_env.then_some(s.src),
        peak: media.peak.unwrap_or(0.0),
        range: env_node.p.get(33).unwrap_or(12.0),
        rate: f64::from(media.rate),
        samples: f64::from(media.frames),
        looped: loops.is_some(),
        l0: if streamed { 0.0 } else { media.loop_points.map_or(0.0, |l| f64::from(l[0])) },
        l1: match (streamed, media.loop_points) {
            (false, Some(l)) if l[1] != 0 => f64::from(l[1]),
            _ => f64::from(media.frames),
        },
    });
    // a child of a continuous container: how long it runs, negative when it does not end by itself or streams
    let runs = match (opts.chain, streamed, loops) {
        (None, ..) | (_, true, _) => -1.0,
        (_, _, None) => len / rate,
        (_, _, Some(n)) if n > 1.0 => len * n / rate,
        _ => -1.0,
    };
    if opts.chain.is_some() && streamed {
        ctl.warn("streamed child of a continuous container not ported: the chain ends", u64::from(id), out);
    }
    let end = match loops {
        None => t0 + len / rate,
        Some(n) if n > 1.0 => t0 + len * n / rate,
        _ => f64::INFINITY,
    };
    let voice = Voice {
        id: VoiceId(adm.seq),
        post: opts.post,
        obj: owner,
        key: opts.key,
        node: id,
        plan: pl.clone(),
        start: t0,
        end,
        seq: adm.seq,
        prio: adm.prio,
        below: adm.below,
        lims: adm.lims,
        vol,
        dry: dry_db,
        bus,
        sends: vsends,
        forced: false,
        under: false,
        silent: false,
        paused: None,
        cents,
        rate,
        fade: fade_in,
        streamed,
        hdr,
        spat,
        game,
        chain: opts.chain,
        runs,
    };
    ctl.voices.list.push(voice);
    let vi = ctl.voices.list.len() - 1;
    place_voice(ctl, vi, true, t, out);
    let v = &ctl.voices.list[vi];
    let mut pan = [[0.0; 2]; MAX_CHANNELS];
    for (c, row) in pan.iter_mut().enumerate().take(channels.min(MAX_CHANNELS)) {
        *row = if let Some(sp) = v.spat.as_ref().filter(|s| s.dirs.is_some()) {
            match sp.pan_ch.iter().position(|&x| usize::from(x) == c) {
                Some(i) => [sp.pan[i][0].value, sp.pan[i][1].value],
                None => [0.0, 0.0],
            }
        } else if channels == 1 {
            [SQRT1_2, SQRT1_2]
        } else if let Some(b) = &mix_bits {
            STEREO_MIX[usize::from(b[c])]
        } else if channels == 2 {
            [f64::from(u8::from(c == 0)), f64::from(u8::from(c == 1))]
        } else {
            [0.0, 0.0]
        };
    }
    let mut spec_sends = [Send::default(); MAX_SENDS];
    for (i, (bus, tr)) in v.sends.iter().take(MAX_SENDS).enumerate() {
        spec_sends[i] = Send { bus: *bus, gain: tr.value };
    }
    let spec = VoiceSpec {
        voice: v.id,
        at: samples(t0),
        medium: s.src,
        offset: 0,
        loops: match loops {
            None => 1,
            Some(n) if n > 1.0 => n as u16,
            Some(_) => 0,
        },
        loop_points,
        cents: cents + rt_cents,
        bus: bus.or(ctl.master).unwrap_or(0),
        vol,
        makeup,
        norm,
        dry: dry_db,
        rtpc_gain: if rt_db == f64::NEG_INFINITY { 0.0 } else { db_to_gain(rt_db) },
        fade_in,
        lpf: if lpf > 0.0 { lpf } else { 0.0 },
        hpf: if hpf > 0.0 { hpf } else { 0.0 },
        meters,
        channels: media.channels as u8,
        pan,
        dist: v.spat.as_ref().map_or(1.0, |s| s.dist.value),
        sends_n: n_sends as u8,
        sends: spec_sends,
        streamed,
    };
    out.controls.push(Control::Start(spec));
    // the attenuation's filters as placed at the start
    if let Some(sp) = &v.spat {
        let (vid, a) = (v.id, sp.att.clone());
        for tr in [sp.lpf, sp.hpf].into_iter().flatten() {
            if !tr.want.is_nan() {
                let w = att_units(tr.param, &a, ctl.listener.as_ref(), v, ctl);
                change(out, t, vid, tr.param, w, Ramp::Set);
            }
        }
    }
    let has_hdr = v.hdr.is_some();
    voice_frame(ctl, true, t, out);
    if ctl.voices.timer.is_none() {
        ctl.voices.timer = Some(now);
    }
    if has_hdr && t0 <= t + 0.005 {
        let Ctl { buses, syncs, voices, banks, .. } = ctl;
        buses.hdr(now, syncs, voices, banks, out);
    }
}

/// The attenuation filter value a track was last set to, in Wwise units (the line's value), from its Hz.
fn att_units(param: Param, a: &Option<Att>, lis: Option<&Listener>, v: &Voice, ctl: &Ctl) -> f64 {
    let Some(lis) = lis else { return 0.0 };
    let Some(em) = emitter_of(ctl, v) else { return 0.0 };
    let d = mvs_math::hypot::hypot3(em.pos[0] - lis.pos[0], em.pos[1] - lis.pos[1], em.pos[2] - lis.pos[2]);
    let i = if param == Param::AttLpf { 3 } else { 4 };
    let x = att_curve(a.as_ref(), i, d).unwrap_or(0.0);
    if x > 0.0 { x.min(100.0) } else { 0.0 }
}

/// The emitter a voice is placed from: the post's location, else its object's, else the global object's.
fn emitter_of(ctl: &Ctl, v: &Voice) -> Option<Emitter> {
    let own = match (&v.spat, &v.game) {
        (Some(s), _) => s.em,
        (None, Some(g)) => g.em,
        (None, None) => return None,
    };
    own.or_else(|| ctl.emitters.get(&v.obj.0).copied()).or((v.obj == GameObj::GLOBAL).then_some(GLOBAL_EMITTER))
}

/// The admission of a new voice: the global limiter, the node and bus limiters, priority
/// and the virtual voice behaviour.
struct Admit {
    seq: u32,
    prio: f64,
    below: u8,
    lims: Vec<LimKey>,
}

#[allow(clippy::too_many_arguments)]
fn admit(
    ctl: &mut Ctl,
    pl: &Plan,
    bus: Option<Id>,
    owner: GameObj,
    infinite: bool,
    t: f64,
    out: &mut Out,
) -> Option<Admit> {
    let nodes = &pl.nodes;
    let top = &nodes[nodes.len() - 1];
    let pn = nodes.iter().find(|n| n.prio_ov).unwrap_or(top);
    let mut prio = pn.p.get(7).unwrap_or(50.0);
    for r in pn.rtpc.iter().filter(|r| r.param == 17) {
        prio += curve_value(r, ctl.syncs.rtpc(r.id, owner));
    }
    let prio = prio.clamp(0.0, 100.0);
    let mut below = nodes.iter().find(|n| n.lim & 16 != 0).unwrap_or(top).below.unwrap_or(0);
    if below == 3 {
        below = if infinite { 2 } else { 1 };
    }
    let mut lims: Vec<LimKey> = Vec::new();
    let mut kicks = 0;
    let mut res = 1u8; // 1 play, 2 refused, 0x50 virtual
    // the global limiter first
    let gmax = f64::from(ctl.init.max_voices);
    if gmax > 0.0 {
        let mut all: Vec<(f64, u32, u8)> =
            ctl.voices.list.iter().filter(|v| !v.under && !v.forced).map(|v| (v.prio, v.seq, v.below)).collect();
        if all.len() as f64 + 1.0 > gmax {
            all.sort_by(|a, b| b.0.total_cmp(&a.0).then(a.1.cmp(&b.1)));
            let (mut cand, mut weakest, mut slot, mut n) = (None, 101.0, false, 0.0);
            for v in &all {
                n += 1.0;
                if v.0 > prio {
                    continue;
                }
                if v.2 != 0 {
                    cand = Some(*v);
                    weakest = v.0;
                } else if n <= gmax {
                    slot = true;
                }
            }
            match cand {
                Some(c) if prio > weakest => {
                    if c.2 == 1 {
                        stop_seq(ctl, c.1, t, out);
                    }
                }
                _ => {
                    if !slot {
                        res = 0x50;
                    }
                }
            }
        }
    }
    let mut run = |ctl: &mut Ctl, key: LimKey, max: f64, lim: u8, global: bool, ord: [u64; 3], out: &mut Out| {
        let key = if global { key } else { LimKey { obj: Some(owner), ..key } };
        if !lims.contains(&key) {
            lims.push(key);
        }
        if !ctl.voices.limiters.contains_key(&key) {
            ctl.voices.lim_order = None;
        }
        ctl.voices.limiters.insert(key, Limiter { max, lim, ord });
        let mut live: Vec<(f64, u32, u8)> =
            ctl.voices.list.iter().filter(|v| v.lims.contains(&key)).map(|v| (v.prio, v.seq, v.below)).collect();
        let n_live = live.len() as f64;
        if if global { kicks > 0 || n_live < max } else { n_live - f64::from(kicks) < max } {
            return;
        }
        kicks += 1;
        let (newest, virt) = (lim & 1 != 0, lim & 2 != 0);
        live.sort_by(|a, b| b.0.total_cmp(&a.0).then(if newest { a.1.cmp(&b.1) } else { b.1.cmp(&a.1) }));
        let (mut cand, mut weakest, mut slot, mut n) = (None, 101.0, false, 0.0);
        for v in &live {
            n += 1.0;
            if v.0 > prio {
                continue;
            }
            if virt && v.2 == 0 && n <= max {
                slot = true;
            } else {
                cand = Some(*v);
                weakest = v.0;
            }
        }
        if n_live + 1.0 <= max {
            return;
        }
        match cand {
            Some(c) if !(prio < weakest || (newest && prio == weakest)) => {
                if !virt || c.2 == 1 {
                    stop_seq(ctl, c.1, t, out);
                }
            }
            _ => {
                if !slot {
                    res = if virt && res != 2 { 0x50 } else { 2 };
                }
            }
        }
    };
    let (mut considered, mut bus_done) = (false, false);
    let last = nodes.len() as u64 - 1;
    for (i, n) in nodes.iter().enumerate() {
        if !considered {
            // an RTPC on MaxNumInstances replaces a nonzero maximum
            let r = (n.max_inst != 0).then(|| n.rtpc.iter().find(|x| x.param == 16)).flatten();
            let max = match r {
                Some(r) => curve_value(r, ctl.syncs.rtpc(r.id, owner)),
                None => f64::from(n.max_inst),
            };
            if max != 0.0 {
                let key = LimKey { kind: 2, id: pl.ids[i], obj: None };
                run(ctl, key, max, n.lim, n.lim & 4 != 0, [2, last - i as u64, u64::from(pl.ids[i])], out);
            }
            considered = n.lim & 8 != 0;
        }
        if !bus_done && n.bus.is_some_and(|b| b != 0) {
            bus_done = true;
            let mut b = bus;
            while let Some(bid) = b {
                let Some(d) = ctl.init.buses.get(&bid) else { break };
                let (max_inst, lim, parent) = (d.max_inst, d.lim, d.parent);
                if max_inst != 0 {
                    let depth = bus_depth(ctl, bid);
                    let key = LimKey { kind: 1, id: bid, obj: None };
                    run(ctl, key, f64::from(max_inst), lim, true, [1, depth, u64::from(bid)], out);
                }
                if lim & 4 != 0 {
                    break;
                }
                b = parent;
            }
        }
    }
    if res == 2 || (res == 0x50 && below == 1) {
        return None;
    }
    ctl.voices.seq += 1;
    Some(Admit { seq: ctl.voices.seq, prio, below, lims })
}

fn bus_depth(ctl: &Ctl, id: Id) -> u64 {
    let mut n = 0;
    let mut b = ctl.init.buses.get(&id).and_then(|d| d.parent);
    while let (Some(x), true) = (b, n < 32) {
        n += 1;
        b = ctl.init.buses.get(&x).and_then(|d| d.parent);
    }
    n
}

// ---- the per-frame pass -----------------------------------------------------------------------------------------------

/// The volume a voice's connections are tested with, dB: the highest connection.
fn voice_db(ctl: &Ctl, v: &Voice, t: f64) -> f64 {
    let mut vol = v.vol;
    for r in v.plan.rtpc.iter().filter(|r| r.param == 0) {
        vol += curve_value(r, ctl.syncs.rtpc(r.id, v.obj));
    }
    let down = |b: Option<Id>| b.map_or(0.0, |b| ctl.buses.volume_db(b, &ctl.syncs));
    let dist = v.spat.as_ref().map_or(1.0, |s| s.dist.read(t));
    let mut top = vol + v.dry + db_of(dist) + down(v.bus.or(ctl.master));
    for (b, tr) in &v.sends {
        top = top.max(vol + db_of(tr.value) + down(Some(*b)));
    }
    top
}

/// The play action's fade ratio a voice is tested with, dB.
fn fade_db(v: &Voice, t: f64) -> f64 {
    let n = (v.fade.secs / FRAME_SECS - 1e-6).ceil();
    let r = if n > 0.0 {
        interp(v.fade.curve, (((t - v.start) / FRAME_SECS + 1e-6).floor() + 1.0).min(n) / n, 0.0, 1.0)
    } else {
        1.0
    };
    20.0 * log10(r)
}

fn voice_muted(ctl: &Ctl, v: &Voice) -> bool {
    !ctl.muted.is_empty() && bus_under(ctl, v.bus, &ctl.muted)
}

/// Virtual voices and the limiters, once per audio frame and after a post that started a voice:
/// `now` sets a voice's mute at once, else over a frame.
pub(crate) fn voice_frame(ctl: &mut Ctl, now: bool, t: f64, out: &mut Out) {
    let thr = ctl.init.volume_threshold;
    let mut kill = false;
    for i in 0..ctl.voices.list.len() {
        let v = &ctl.voices.list[i];
        if v.paused.is_none() {
            let under =
                v.below != 0 && (t < v.start || voice_muted(ctl, v) || voice_db(ctl, v, t) + fade_db(v, t) <= thr);
            ctl.voices.list[i].under = under;
        }
        let v = &ctl.voices.list[i];
        if v.under && v.below == 1 && t >= v.start {
            kill = true;
        }
    }
    if kill {
        let doomed: Vec<u32> =
            ctl.voices.list.iter().filter(|v| v.under && v.below == 1 && t >= v.start).map(|v| v.seq).collect();
        stop_where(ctl, &|w: &Who| w.seq.is_some_and(|s| doomed.contains(&s)), 0.0, 0.0, false, None, t, out);
    }
    let mut forced: HashSet<u32> = HashSet::new();
    let mut counts: HashMap<LimKey, usize> = HashMap::new();
    for v in &ctl.voices.list {
        for k in &v.lims {
            *counts.entry(*k).or_default() += 1;
        }
    }
    let live = ctl.voices.list.len();
    let mut stale = false;
    if ctl.init.max_voices != 0 {
        stale = limit_pass(ctl, None, f64::from(ctl.init.max_voices), 3, live, stale, &mut forced, t, out);
    }
    let order = match &ctl.voices.lim_order {
        Some(o) => o.clone(),
        None => {
            let mut o: Vec<(LimKey, [u64; 3])> = ctl.voices.limiters.iter().map(|(k, l)| (*k, l.ord)).collect();
            o.sort_by_key(|a| a.1);
            let o: Vec<LimKey> = o.into_iter().map(|x| x.0).collect();
            ctl.voices.lim_order = Some(o.clone());
            o
        }
    };
    for k in order {
        let Some(l) = ctl.voices.limiters.get(&k).copied() else { continue };
        let n = counts.get(&k).copied().unwrap_or(0);
        stale = limit_pass(ctl, Some(k), l.max, l.lim, n, stale, &mut forced, t, out);
    }
    for i in 0..ctl.voices.list.len() {
        let v = &ctl.voices.list[i];
        let f_forced = forced.contains(&v.seq);
        let f = f_forced || v.paused.is_some() || voice_muted(ctl, v);
        let v = &mut ctl.voices.list[i];
        v.forced = f_forced;
        if f == v.silent {
            continue;
        }
        v.silent = f;
        let (id, val) = (v.id, if f { 0.0 } else { 1.0 });
        change(out, t, id, Param::Mute, val, if now { Ramp::Set } else { Ramp::Linear(FRAME_SECS) });
    }
}

/// One limiter of the per-frame pass (None: the global one); `n` its voices when none has been stopped in the
/// pass (`stale` false). Whether a voice has been stopped by now.
#[allow(clippy::too_many_arguments)]
fn limit_pass(
    ctl: &mut Ctl,
    key: Option<LimKey>,
    max: f64,
    lim: u8,
    n: usize,
    stale: bool,
    forced: &mut HashSet<u32>,
    t: f64,
    out: &mut Out,
) -> bool {
    let mut stale = stale;
    if !stale && n > 0 && n as f64 <= max {
        return false;
    }
    let mut list: Vec<(f64, u32, u8, bool)> = if !stale && n == 0 {
        Vec::new()
    } else {
        ctl.voices
            .list
            .iter()
            .filter(|v| key.is_none_or(|k| v.lims.contains(&k)))
            .map(|v| (v.prio, v.seq, v.below, v.under))
            .collect()
    };
    if list.is_empty() {
        if let Some(k) = key
            && ctl.voices.limiters.shift_remove(&k).is_some()
        {
            ctl.voices.lim_order = None;
        }
        return stale;
    }
    if list.len() as f64 <= max {
        return stale;
    }
    list.sort_by(|a, b| b.0.total_cmp(&a.0).then(if lim & 1 != 0 { a.1.cmp(&b.1) } else { b.1.cmp(&a.1) }));
    let (mut c, mut i) = (0.0, 0);
    while i < list.len() && c < max {
        if !list[i].3 && !forced.contains(&list[i].1) {
            c += 1.0;
        }
        i += 1;
    }
    for v in &list[i..] {
        if lim & 2 == 0 || v.2 == 1 {
            stop_seq(ctl, v.1, t, out);
            stale = true;
        } else if v.2 == 2 {
            forced.insert(v.1);
        }
    }
    stale
}

/// The per-frame pass's interval: once per audio frame while voices play.
pub(crate) fn voice_tick(ctl: &mut Ctl, now: u64, t: f64, out: &mut Out) {
    match ctl.voices.timer {
        Some(at) if at < now => {}
        _ => return,
    }
    let tick = (t / FRAME_SECS).floor() as i64;
    if tick == ctl.voices.last {
        return;
    }
    ctl.voices.last = tick;
    voice_frame(ctl, false, t, out);
    if ctl.voices.list.is_empty() {
        ctl.voices.timer = None;
    }
}

/// The trigger-rate containers' timers due by `t`, by due time then the order they were set in.
pub(crate) fn timers(ctl: &mut Ctl, t: f64, out: &mut Out) {
    loop {
        let next = ctl
            .voices
            .triggers
            .iter()
            .filter_map(|x| x.due.filter(|d| d.0 <= t).map(|d| (d, x.n)))
            .min_by(|a, b| a.0.0.total_cmp(&b.0.0).then(a.0.1.cmp(&b.0.1)));
        let Some((_, n)) = next else { return };
        trigger_step(ctl, n, t, out);
    }
}

/// A voice ran out at `t` (not a stop): it leaves, and a continuous container plays its next child.
pub(crate) fn ended(ctl: &mut Ctl, voice: VoiceId, t: f64, out: &mut Out) {
    let Some(i) = ctl.voices.list.iter().position(|v| v.id == voice) else { return };
    let v = ctl.voices.list.remove(i);
    post_ended(ctl, v.post, out);
    let Some(n) = v.chain else { return };
    if v.runs < 0.0 {
        return;
    }
    let Some(ch) = ctl.voices.chains.get_mut(&n) else { return };
    let pick = if ch.playlist.done() {
        None
    } else {
        let r = ctl.draw(out);
        ctl.voices.chains.get_mut(&n).and_then(|c| c.playlist.next(r))
    };
    let Some(pick) = pick else {
        ctl.voices.chains.remove(&n);
        return;
    };
    let ch = ctl.voices.chains[&n].clone();
    let at = next_frame_after(v.start, v.runs).max(t);
    play_node(ctl, pick, at, Fade { secs: 0.0, curve: ch.curve }, &ch.opts, &ch.path, t, out);
}

/// The game is told when the last voice of a post has left.
fn post_ended(ctl: &mut Ctl, post: PostId, out: &mut Out) {
    if post != PostId::default() && !ctl.voices.list.iter().any(|v| v.post == post) {
        let _ = out.notes.push(crate::Note::EventEnd { post });
    }
}

// ---- stops ------------------------------------------------------------------------------------------------------------

/// The curve a stop fades along: the fade-out takes curve 8 - c (InvSCurve and SCurve kept).
fn stop_curve(c: Curve) -> Curve {
    match c {
        Curve::Log3 => Curve::Exp3,
        Curve::Exp3 => Curve::Log3,
        Curve::Sine => Curve::SineRecip,
        Curve::SineRecip => Curve::Sine,
        Curve::Log1 => Curve::Exp1,
        Curve::Exp1 => Curve::Log1,
        x => x,
    }
}

/// Stops one voice at once (a kick, a kill: no fade time).
fn stop_seq(ctl: &mut Ctl, seq: u32, t: f64, out: &mut Out) {
    stop_where(ctl, &|w: &Who| w.seq == Some(seq), 0.0, 0.0, false, None, t, out);
}

/// Stops the voices `pred` takes after `delay` over `fade` s; with `waiting` the
/// trigger-rate containers it takes too. With a curve the sound engine's per-frame ratios; without, a line over
/// the fade (at least 10 ms).
#[allow(clippy::too_many_arguments)]
pub(crate) fn stop_where(
    ctl: &mut Ctl,
    pred: &dyn Fn(&Who) -> bool,
    fade: f64,
    delay: f64,
    waiting: bool,
    curve: Option<Curve>,
    t: f64,
    out: &mut Out,
) {
    if waiting {
        ctl.voices.triggers.retain(|x| !pred(&Who { key: x.key, owner: x.owner, chain: &x.chain, seq: None }));
    }
    let ts = t + delay;
    let mut ratios = Vec::new();
    // None: no curve; Some(false): at once
    let curved = curve.map(|c| stop_ratios(c, (fade * 1000.0).round(), &mut ratios));
    let mut posts = Vec::new();
    for v in &ctl.voices.list {
        if !pred(&Who { key: v.key, owner: v.obj, chain: &v.plan.ids, seq: Some(v.seq) }) {
            continue;
        }
        let f = match curved {
            Some(true) => {
                let f = (ratios.len() - 1) as f64 * FRAME_SECS;
                let c = stop_curve(curve.expect("curved"));
                change(out, ts, v.id, Param::Fade, 0.0, Ramp::Curve { secs: f, curve: c });
                f
            }
            Some(false) => {
                change(out, ts, v.id, Param::Fade, 0.0, Ramp::Linear(0.01));
                0.01
            }
            None => {
                let f = fade.max(0.01);
                change(out, ts, v.id, Param::Fade, 0.0, Ramp::Linear(f));
                f
            }
        };
        change(out, ts + f + 0.01, v.id, Param::Stop, 0.0, Ramp::Set);
        posts.push(v.seq);
    }
    if posts.is_empty() {
        return;
    }
    let mut gone = Vec::new();
    let mut chains = Vec::new();
    ctl.voices.list.retain(|v| {
        let stop = posts.contains(&v.seq);
        if stop {
            gone.push(v.post);
            chains.extend(v.chain);
        }
        !stop
    });
    // a stopped child ends its continuous container's chain
    for c in chains {
        ctl.voices.chains.remove(&c);
    }
    for p in gone {
        post_ended(ctl, p, out);
    }
}

// ---- game parameters and placement ------------------------------------------------------------------------------------

/// A game parameter took a value: the voices that read it move over one audio frame.
pub(crate) fn rtpc_moved(voices: &mut Voices, syncs: &Syncs, id: Id, obj: GameObj, t: f64, out: &mut Out) {
    for i in 0..voices.list.len() {
        if (obj != GameObj::GLOBAL && voices.list[i].obj != obj) || !voices.reads(i, id) {
            continue;
        }
        let v = &mut voices.list[i];
        let (db, cents) = syncs.rtpc_of(&v.plan.rtpc, v.obj);
        let g = if db == f64::NEG_INFINITY { 0.0 } else { db_to_gain(db) };
        change(out, t, v.id, Param::RtpcGain, g, Ramp::Linear(FRAME_SECS));
        // (a streamed voice has no pitch; a paused one takes its rate at the resume)
        if !v.streamed && v.paused.is_none() {
            v.rate = fround(pow(2.0, (v.cents + cents) / 1200.0));
            change(out, t, v.id, Param::Cents, v.cents + cents, Ramp::Linear(FRAME_SECS));
        }
    }
}

/// A voice parameter at once (`now`: no line, the start's spec has it), else over one audio frame when it moved
/// more than 1e-6; `line` is the value the change carries.
fn set_param(tr: &mut Track, val: f64, line: f64, now: bool, voice: VoiceId, t: f64, out: &mut Out) {
    if now {
        tr.value = fround(val);
        tr.want = val;
        tr.held = fround(val);
        tr.end = 0.0;
        return;
    }
    if val == tr.want && t >= tr.end {
        return;
    }
    if (tr.value - val).abs() > 1e-6 {
        change(out, t, voice, tr.param, line, Ramp::Linear(FRAME_SECS));
        tr.value = fround(val);
        tr.want = val;
        tr.held = fround(val);
        // (two render quanta past the ramp's end)
        tr.end = t + FRAME_SECS.max(0.005) + 256.0 / f64::from(crate::RATE);
    } else if t >= tr.end {
        tr.want = val;
    }
}

fn in_reverb(r: &crate::ReverbVolume, p: &V3) -> bool {
    (0..3).all(|a| p[a] >= r.lo[a] && p[a] <= r.hi[a])
}

/// Distance, filters, sends and panning of a voice.
pub(crate) fn place_voice(ctl: &mut Ctl, i: usize, now: bool, t: f64, out: &mut Out) {
    let v = &ctl.voices.list[i];
    if v.spat.is_none() && v.game.is_none() {
        return;
    }
    let Some(em) = emitter_of(ctl, v) else { return };
    let lis = ctl.listener;
    let (r0, r1, r2) = match &lis {
        Some(l) => (em.pos[0] - l.pos[0], em.pos[1] - l.pos[1], em.pos[2] - l.pos[2]),
        None => (0.0, 0.0, 0.0),
    };
    let dist = mvs_math::hypot::hypot3(r0, r1, r2);
    let att = if lis.is_some() { v.spat.as_ref().and_then(|s| s.att.clone()) } else { None };
    let a = att.as_ref();
    let fade = ctl.reverb_fade.get(&v.obj.0).cloned();
    let reverb = ctl.reverb.clone();
    let v = &mut ctl.voices.list[i];
    let id = v.id;
    if let Some(gs) = &v.game {
        // the game-defined sends: the game object's control value per reverb volume
        let k = gs.base * att_lin(a, 1, dist);
        let (first, own) = (gs.first, gs.em.is_some());
        for (j, (_, tr)) in v.sends[first..].iter_mut().enumerate() {
            let val = match reverb.get(j) {
                None => 0.0,
                Some(r) => {
                    k * if own {
                        if in_reverb(r, &em.pos) { r.level } else { 0.0 }
                    } else {
                        fade.as_ref().and_then(|f| f.v.get(j).copied()).unwrap_or(0.0)
                    }
                }
            };
            set_param(tr, val, val, now, id, t, out);
        }
    }
    let (Some(sp), Some(lis)) = (&mut v.spat, lis) else { return };
    let dv = att_lin(a, 0, dist);
    set_param(&mut sp.dist, dv, dv, now, id, t, out);
    let aux = att_lin(a, 2, dist);
    let n_user = sp.base.len();
    for (j, (_, tr)) in v.sends[..n_user].iter_mut().enumerate() {
        let val = sp.base[j] * aux;
        set_param(tr, val, val, now, id, t, out);
    }
    let lpf = att_curve(a, 3, dist).unwrap_or(0.0);
    let hpf = att_curve(a, 4, dist).unwrap_or(0.0);
    let rate = f64::from(crate::RATE);
    if let Some(tr) = &mut sp.lpf {
        let (hz, w) = if lpf > 0.0 { (eval_cutoff(lpf.min(100.0), rate), lpf.min(100.0)) } else { (rate / 2.0, 0.0) };
        set_param(tr, hz, w, now, id, t, out);
    }
    if let Some(tr) = &mut sp.hpf {
        let (hz, w) = if hpf > 0.0 { (eval_cutoff(100.0 - hpf.min(100.0), rate), hpf.min(100.0)) } else { (0.0, 0.0) };
        set_param(tr, hz, w, now, id, t, out);
    }
    if let Some(dirs) = &sp.dirs {
        let (lr, lf) = (lis.right, lis.front);
        let flat = mvs_math::hypot::hypot2(r0 * lr[0] + r1 * lr[1] + r2 * lr[2], r0 * lf[0] + r1 * lf[1] + r2 * lf[2]);
        let spread = (att_curve(a, 5, flat).unwrap_or(0.0) / 100.0).clamp(0.0, 1.0);
        let focus = (att_curve(a, 6, flat).unwrap_or(0.0) / 100.0).clamp(0.0, 1.0);
        let mut g = [[0.0; 2]; MAX_CHANNELS];
        let n = dirs.len();
        speaker_volumes(&lis, &em, dirs, spread, focus, &mut g[..n]);
        for (c, pair) in sp.pan.iter_mut().enumerate() {
            set_param(&mut pair[0], g[c][0], g[c][0], now, id, t, out);
            set_param(&mut pair[1], g[c][1], g[c][1], now, id, t, out);
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::sync::Arc;

    use mvs_data::sound::{EventBank, InitBank, Obj, PlaylistItem, RanSeq, TransitionMode};

    use super::{Opts, play_node};
    use crate::ctl::{Ctl, Out, Rng};
    use crate::spec::Fade;

    /// A looping trigger-rate container of no transition time steps one quantum a sound: its post returns, with
    /// the sounds of the step's 0.2 s played and the rest waiting on its timer.
    #[test]
    fn a_trigger_rate_of_no_time_steps_a_quantum() {
        let init = Arc::new(InitBank::default());
        let mut ctl = Ctl::new(init.clone(), Arc::new(crate::RtpcMirror::new(&init)), Rng::Seeded(3));
        let c = RanSeq {
            playlist: vec![PlaylistItem { id: 11, weight: 1 }],
            continuous: true,
            loop_count: 0,
            transition: TransitionMode::TriggerRate,
            transition_time: 0.0,
            ..RanSeq::default()
        };
        let objs = BTreeMap::from([(10, Obj::RandomSequenceContainer(c))]);
        let mut b = crate::ctl::bank::Banks::default();
        b.add([("b".to_owned(), Arc::new(EventBank { name: "b".into(), id: 1, objs, ..EventBank::default() }))]);
        ctl.banks = Arc::new(b.view());
        let (mut controls, (mut notes, _rx)) = (Vec::new(), rtrb::RingBuffer::new(1024));
        let mut out = Out { controls: &mut controls, notes: &mut notes };
        play_node(&mut ctl, 10, 0.0, Fade::default(), &Opts::default(), &[], 0.0, &mut out);
        let tr = &ctl.voices.triggers[0];
        assert!(tr.at > 0.2 && tr.at < 0.21, "the next sound waits: {}", tr.at);
        assert!(tr.due.is_some());
    }
}
