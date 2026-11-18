//! The interpreter: the sound engine's side that runs on the mixer thread before the DSP. It takes the game's
//! commands, reads the loaded banks (merged on the game side), plays events (actions, the actor-mixer and music hierarchies, limits, virtual
//! voices), keeps the game syncs, places voices against the listener and drives the buses' parameters, and says
//! what to play as [`Control`]s.
//!
//! Times inside are seconds on the audio clock as f64 (`now as f64 / RATE`), the Web Audio clock's
//! arithmetic; a [`Control`]'s time is that rounded to samples ([`samples`]).

pub mod bank;
pub mod bus;
pub mod curves;
pub mod music;
pub mod pan;
pub mod post;
pub mod scope;
pub mod sync;
pub mod voice;

#[cfg(test)]
mod bank_tests;
#[cfg(test)]
mod tape_tests;

use std::collections::HashMap;
use std::sync::Arc;

use mvs_data::sound::{Id, InitBank, Rtpc};

use crate::spec::{Control, ParamChange, VoiceId};
use crate::tape::voice::Line;
use crate::{Cmd, Emitter, GameObj, Note, ReverbVolume, RtpcMirror};

/// The interpreter's random source: a run draws from its own generator, and the voice tape replays the draws it
/// holds.
#[derive(Clone, Debug)]
pub enum Rng {
    /// SplitMix64 from a seed.
    Seeded(u64),
    /// The draws a tape logged, in order.
    Replay { draws: Vec<f64>, at: usize },
}

impl Rng {
    /// A value in [0, 1).
    pub fn draw(&mut self) -> f64 {
        match self {
            Rng::Seeded(s) => {
                *s = s.wrapping_add(0x9e37_79b9_7f4a_7c15);
                let mut z = *s;
                z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
                z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
                z ^= z >> 31;
                (z >> 11) as f64 / (1u64 << 53) as f64
            }
            Rng::Replay { draws, at } => {
                let v = *draws.get(*at).unwrap_or_else(|| panic!("audio: the tape's random draws ran out at {at}"));
                *at += 1;
                v
            }
        }
    }
}

/// Where the interpreter writes: the controls for the DSP (capacity reserved, never grown on the mixer thread) and
/// the notes for the game.
pub struct Out<'a> {
    pub controls: &'a mut Vec<Control>,
    pub notes: &'a mut rtrb::Producer<Note>,
}

/// The audio clock in seconds.
pub fn secs(now: u64) -> f64 {
    now as f64 / f64::from(crate::RATE)
}

/// A time in seconds on the audio clock as samples (a [`Control`]'s time).
pub fn samples(t: f64) -> u64 {
    (t * f64::from(crate::RATE)).round().max(0.0) as u64
}

/// The first id of the voices the music starts ([`Ctl::music_voice`]); the hierarchy's voices count up from 1.
pub const MUSIC_VOICES: u32 = 0x8000_0000;

/// A game object's fade per reverb volume: its control values and when they last moved.
#[derive(Clone, Debug, Default)]
pub(crate) struct ReverbFade {
    pub t: f64,
    pub v: Vec<f64>,
}

/// The interpreter's state.
pub struct Ctl {
    pub(crate) init: Arc<InitBank>,
    pub(crate) rtpc: Arc<RtpcMirror>,
    pub(crate) rng: Rng,
    /// The loaded banks merged, as the game side last sent them (`Cmd::Banks`).
    pub(crate) banks: Arc<bank::BankView>,
    pub(crate) syncs: sync::Syncs,
    pub(crate) buses: bus::BusCtl,
    pub(crate) music: music::Music,
    pub(crate) voices: voice::Voices,
    /// None until the game sets it.
    pub(crate) listener: Option<crate::Listener>,
    /// The top bus: the first in id order with no parent and a child.
    pub(crate) master: Option<Id>,
    /// The game objects' emitters, by object.
    pub(crate) emitters: HashMap<i32, Emitter>,
    /// The level's reverb volumes (at most 4, by priority) and the objects' fades through them.
    pub(crate) reverb: Vec<ReverbVolume>,
    pub(crate) reverb_fade: HashMap<i32, ReverbFade>,
    /// The buses a Mute action silenced.
    pub(crate) muted: Vec<Id>,
    /// Media that qualify play streamed (the voice tape records with streaming off).
    pub(crate) streaming: bool,
    /// The next music voice's number.
    music_seq: u32,
    /// What was warned once, by kind and id.
    warned: std::collections::HashSet<(&'static str, u64)>,
    /// Tests: the random draws and music decisions with the number of controls written before them, to put them
    /// between the voice and parameter lines.
    pub(crate) trace: Option<Vec<(usize, Line)>>,
}

impl Ctl {
    pub fn new(init: Arc<InitBank>, rtpc: Arc<RtpcMirror>, rng: Rng) -> Ctl {
        let master = init
            .buses
            .iter()
            .find(|(id, b)| b.parent.is_none() && init.buses.values().any(|c| c.parent == Some(**id)))
            .map(|(id, _)| *id);
        Ctl {
            syncs: sync::Syncs::new(&init),
            buses: bus::BusCtl::new(&init),
            init,
            rtpc,
            rng,
            banks: bank::BankView::empty(),
            music: music::Music::default(),
            voices: voice::Voices::default(),
            listener: None,
            master,
            emitters: HashMap::new(),
            reverb: Vec::new(),
            reverb_fade: HashMap::new(),
            muted: Vec::new(),
            streaming: true,
            music_seq: 0,
            warned: std::collections::HashSet::new(),
            trace: None,
        }
    }

    /// Takes a command at audio clock `now` (samples).
    pub fn apply(&mut self, cmd: Cmd, now: u64, out: &mut Out) {
        let t = secs(now);
        match cmd {
            Cmd::Post(p) => post::post(self, &p, now, out),
            Cmd::SetRtpc { id, value, obj, interp_ms } => self.set_rtpc(id, value, obj, interp_ms, now, out),
            Cmd::SetSwitch { group, value, obj } => {
                self.syncs.set_switch(group, value, obj);
                if obj == GameObj::GLOBAL {
                    music::switch_moved(self, now, out);
                }
            }
            Cmd::SetState { group, value } => self.set_state(group, value, now, out),
            Cmd::SetEmitter { obj, pos, front } => self.set_emitter(obj, pos, front, t),
            Cmd::SetListener(l) => {
                self.listener = Some(l);
                for i in 0..self.voices.list.len() {
                    voice::place_voice(self, i, false, t, out);
                }
            }
            Cmd::SetReverbVolumes(list) => {
                // the volumes whose bus the bank has, by priority (the first of equals kept first), at most 4
                self.reverb.clear();
                for r in list.iter() {
                    if self.init.buses.contains_key(&r.bus) {
                        self.reverb.push(*r);
                    } else {
                        self.warn("reverb volume bus not in the bank", u64::from(r.bus), out);
                    }
                }
                self.reverb.sort_by(|a, b| b.prio.total_cmp(&a.prio));
                self.reverb.truncate(4);
                self.reverb_fade.clear();
                let _ = out.notes.push(Note::Free(crate::Garbage::ReverbVolumes(list)));
            }
            Cmd::StopKey { key, fade, curve } => post::stop_key(self, key, fade, curve, now, out),
            Cmd::StopAll { fade, keep } => post::stop_all(self, fade, keep, now, out),
            // (the scope's banks came before it, merged: Cmd::Banks)
            Cmd::Scope { name, load } => {
                let _ = out.notes.push(Note::ScopeReady { name });
                let _ = out.notes.push(Note::Free(crate::Garbage::Scope(load)));
            }
            // (the game side keeps each view it sent until the mixer lets it go: none is freed here)
            Cmd::Banks(v) => self.banks = v,
            Cmd::Unscope { .. } => {}
            Cmd::Media { media, .. } => {
                let _ = out.notes.push(Note::Free(crate::Garbage::MediaList(media)));
            }
            Cmd::Music(m) => music::cmd(m, out),
            Cmd::SetVolume(_) => unreachable!("the mixer takes the master volume itself"),
            Cmd::Resume => {}
        }
    }

    /// Runs the quantum starting at `now`: transitions, the voices' per-frame rules, positioning, music.
    pub fn quantum(&mut self, now: u64, out: &mut Out) {
        self.quantum_with(now, true, out);
    }

    /// The quantum, the music's part left out without `music` (a test whose run plays none).
    pub(crate) fn quantum_with(&mut self, now: u64, music: bool, out: &mut Out) {
        let t = secs(now);
        // the intervals in this order (game parameter transitions, the voices' frame, the
        // HDR window, the music), then the timers due
        let Ctl { syncs, buses, voices, rtpc, .. } = self;
        syncs.quantum(now, rtpc, &mut |s, id, obj| {
            if obj == GameObj::GLOBAL {
                buses.rtpc_moved(id, now, s, out);
            }
            voice::rtpc_moved(voices, s, id, obj, t, out);
        });
        voice::voice_tick(self, now, t, out);
        let Ctl { syncs, buses, voices, banks, .. } = self;
        buses.quantum(now, syncs, voices, banks, out);
        if music {
            music::quantum(self, now, out);
        }
        voice::timers(self, t, out);
    }

    /// The DSP says a voice ran out (its medium's end, not a stop) at audio clock `at`.
    pub fn ended(&mut self, voice: VoiceId, at: u64, out: &mut Out) {
        voice::ended(self, voice, secs(at), out);
    }

    /// The DSP says a meter (a voice insert or a bus effect) read `value` for its game parameter at audio clock `at`:
    /// the global value moves (mirrored) and the curves on it with it.
    pub fn meter(&mut self, game_param: Id, value: f64, at: u64, out: &mut Out) {
        // Deviation: the build sets the highest value of the meters that write the parameter (CAkMeterManager::Execute
        // 1800eea10); the mixer reports one meter at a time, so the last report is the value.
        self.set_rtpc(game_param, value, GameObj::GLOBAL, 0.0, at, out);
    }

    /// A random value in [0, 1) (logged in tests).
    pub(crate) fn draw(&mut self, out: &Out) -> f64 {
        let v = self.rng.draw();
        if let Some(t) = &mut self.trace {
            t.push((out.controls.len(), Line::Random { v }));
        }
        v
    }

    /// A ranged property's draw: a value in [min, max], 0 (and no draw) without a range.
    pub(crate) fn rand(&mut self, r: Option<[f64; 2]>, out: &Out) -> f64 {
        match r {
            Some(_) => {
                let v = self.draw(out);
                curves::rand(r, v)
            }
            None => 0.0,
        }
    }

    /// Tests: a music decision at `t` s (a segment, a transition, a playlist item: `what`).
    pub(crate) fn music_line(&mut self, t: f64, node: Id, what: &str, out: &Out) {
        if let Some(tr) = &mut self.trace {
            tr.push((out.controls.len(), Line::Music { at: samples(t), node, what: what.to_owned() }));
        }
    }

    /// A number for a voice the music starts (a range of its own, apart from the hierarchy's voices).
    pub(crate) fn music_voice(&mut self) -> VoiceId {
        self.music_seq += 1;
        VoiceId(MUSIC_VOICES + self.music_seq)
    }

    /// Emits a parameter change.
    pub(crate) fn change(out: &mut Out, c: ParamChange) {
        out.controls.push(Control::Change(c));
    }

    /// Tells the game, once per kind and id, what the interpreter could not do.
    pub(crate) fn warn(&mut self, what: &'static str, id: u64, out: &mut Out) {
        if self.warned.insert((what, id)) {
            let _ = out.notes.push(Note::Warn { what, id });
        }
    }

    /// Sets a game parameter (Cmd::SetRtpc, a meter): the value or its transition (`Syncs`), and on each value it
    /// takes the buses' curves on it (global values), the voices that read it and the music.
    pub(crate) fn set_rtpc(&mut self, id: Id, value: f64, obj: GameObj, interp_ms: f64, now: u64, out: &mut Out) {
        let t = secs(now);
        let Ctl { syncs, buses, voices, rtpc, .. } = self;
        syncs.set_rtpc(id, value, obj, interp_ms, now, rtpc, &mut |s, id, obj| {
            if obj == GameObj::GLOBAL {
                buses.rtpc_moved(id, now, s, out);
            }
            voice::rtpc_moved(voices, s, id, obj, t, out);
        });
    }

    /// Sets a state (Cmd::SetState, a SetState action): the buses' state chunks move over the group's transition
    /// time unless a Pause All holds them.
    pub(crate) fn set_state(&mut self, group: Id, value: Id, now: u64, out: &mut Out) {
        if let Some(ms) = self.syncs.set_state(group, value, now) {
            let Ctl { buses, syncs, .. } = self;
            buses.states_moved(ms, false, now, syncs, out);
        }
    }

    /// A game object's place: with reverb volumes, its fade per volume moves toward the
    /// send level while it is inside, toward 0 outside, by send level x fade rate a second.
    // Unverified: the Wwise Unreal integration's model (UAkComponent's late reverb list and AkReverbFadeControl), not
    // decoded from the exe; the class defaults (send level 1, fade rate 0.5, priority 1) are the integration's.
    fn set_emitter(&mut self, obj: GameObj, pos: crate::V3, front: crate::V3, t: f64) {
        self.emitters.insert(obj.0, Emitter { pos, front, top: [0.0, 0.0, 1.0] });
        if self.reverb.is_empty() {
            return;
        }
        let n = self.reverb.len();
        let f = self.reverb_fade.entry(obj.0).or_insert_with(|| ReverbFade { t, v: vec![0.0; n] });
        let dt = (t - f.t).max(0.0);
        f.t = t;
        for (i, r) in self.reverb.iter().enumerate() {
            let step = r.level * r.rate * dt;
            let inside = (0..3).all(|a| pos[a] >= r.lo[a] && pos[a] <= r.hi[a]);
            f.v[i] = if inside { (f.v[i] + step).min(r.level) } else { (f.v[i] - step).max(0.0) };
        }
    }

    /// The additive game-parameter offsets of a chain's curves on an object: (dB, cents).
    pub(crate) fn rtpc_of(&self, list: &[Rtpc], obj: GameObj) -> (f64, f64) {
        self.syncs.rtpc_of(list, obj)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rng() {
        let mut r = Rng::Seeded(1);
        for _ in 0..1000 {
            let v = r.draw();
            assert!((0.0..1.0).contains(&v));
        }
        let mut t = Rng::Replay { draws: vec![0.25, 0.5], at: 0 };
        assert_eq!((t.draw(), t.draw()), (0.25, 0.5));
    }
}
