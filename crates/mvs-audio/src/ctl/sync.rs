//! Game syncs: game parameters per object and global (with their transitions and the InitBank's ramps), switches
//! per object, states (and the Pause All hold on their transitions).
//!
//! Setting a game parameter, a switch and a state, and the
//! value reads of the buses.

use std::collections::{BTreeMap, HashMap};

use indexmap::IndexMap;
use mvs_data::sound::{Id, InitBank, Rtpc, RtpcRamp};

use super::curves::{FRAME_SECS, curve_value};
use super::secs;
use crate::{GameObj, RtpcMirror};

/// Called on each value a game parameter takes, as it takes it (after the value is
/// stored): the parameter and the object (the global object for a global value). The interpreter moves the buses'
/// curves, the voices and the music from it, reading the syncs as they are at that value.
pub type Moved<'a> = &'a mut dyn FnMut(&Syncs, Id, GameObj);

/// A game parameter's running transition, in audio frames.
#[derive(Clone, Copy, Debug)]
struct Trans {
    id: Id,
    obj: GameObj,
    from: f64,
    to: f64,
    cur: f64,
    t0: i64,
    dur: f64,
    /// The one-pole coefficient of a FilteringOverTime parameter, 0 without.
    coeff: f64,
}

/// Room the maps of values and transitions take up front: the interpreter runs on the mixer thread, which allocates
/// nothing while they stay under it.
// Deviation: past it a map grows on the mixer thread (a match with that many game objects, parameters or switches
// set at once).
const ROOM: usize = 4096;

/// The game syncs' values.
#[derive(Default)]
pub struct Syncs {
    /// The game parameters an object holds a value for (the global object: the global values).
    vals: HashMap<(GameObj, Id), f64>,
    /// The transitions, in the order they began.
    trans: IndexMap<(GameObj, Id), Trans>,
    /// The audio frame the transitions last moved at.
    last_tick: i64,
    /// A switch group's value on an object.
    switches: HashMap<(GameObj, Id), Id>,
    states: HashMap<Id, Id>,
    defaults: BTreeMap<Id, f64>,
    ramps: BTreeMap<Id, RtpcRamp>,
    state_groups: BTreeMap<Id, f64>,
    /// The audio clock time (s) the last state transition ends at.
    state_end: f64,
    /// While a Pause All holds the state transitions, the ms they have left.
    state_hold: Option<f64>,
}

impl Syncs {
    pub fn new(init: &InitBank) -> Syncs {
        Syncs {
            defaults: init.rtpc_defaults.clone(),
            ramps: init.rtpc_ramps.clone(),
            state_groups: init.state_groups.clone(),
            last_tick: -1,
            vals: HashMap::with_capacity(ROOM),
            trans: IndexMap::with_capacity(ROOM),
            switches: HashMap::with_capacity(ROOM),
            states: HashMap::with_capacity(init.state_groups.len().max(64)),
            ..Syncs::default()
        }
    }

    /// Sets a game parameter on an object (the global object: the global value, mirrored), reached over
    /// `interp_ms` from audio clock `now`: each value it takes at once goes to `moved`.
    ///
    /// CAkRTPCMgr::AkRTPCEntry::SetRTPC (180055680). The start is the key's own value, else what it falls back to.
    /// A parameter with ramping (InitBank RTPCRampingParams) raises the time: SlewRate |delta| / rate,
    /// FilteringOverTime the up / down time, taken as whole ms. Under 1 ms, or onto the key's own value, the key's
    /// transition is removed and the value set at once; otherwise CreateOrModifyTransition (180051990) starts a
    /// transition from the current value, or retargets the running one from where it is
    /// (CAkTransitionManager::ChangeParameter 18003ee40).
    #[allow(clippy::too_many_arguments)]
    pub fn set_rtpc(
        &mut self,
        id: Id,
        value: f64,
        obj: GameObj,
        interp_ms: f64,
        now: u64,
        mirror: &RtpcMirror,
        moved: Moved,
    ) {
        let key = (obj, id);
        let own = self.vals.get(&(obj, id)).copied();
        if own == Some(value) {
            self.trans.shift_remove(&key);
            return;
        }
        let cur = own.unwrap_or_else(|| self.rtpc(id, obj));
        let mut ms = interp_ms.trunc();
        let rp = self.ramps.get(&id).copied();
        if let Some(rp) = rp {
            let rate = if value > cur { rp.up } else { rp.down };
            let t = if rp.kind == 1 {
                if rate != 0.0 { ((value - cur).abs() / rate * 1000.0).trunc() } else { 0.0 }
            } else {
                (rate * 1000.0).trunc()
            };
            ms = mvs_math::js::max(ms, t);
        }
        if ms < 1.0 {
            self.trans.shift_remove(&key);
            self.apply(id, value, obj, mirror, moved);
            return;
        }
        // CAkTransition::InitParameters (18003e6b0): whole buffer ticks, rounded up, from the current tick; the
        // FilteringOverTime coefficient is exp(0.10416666 / ms * -samples per frame) (UpdateFilteringCoeff
        // 18003ec70).
        let tick = frame_of(now);
        let dur = (ms / (FRAME_SECS * 1000.0)).ceil();
        if let Some(tr) = self.trans.get_mut(&key) {
            if tr.cur == value {
                self.trans.shift_remove(&key);
                return;
            }
            tr.from = tr.cur;
            tr.to = value;
            tr.t0 = tick;
            tr.dur = dur;
        } else {
            // a key without a value of its own takes the start value first (AddedNewRtpcValue)
            if own.is_none() {
                self.apply(id, cur, obj, mirror, moved);
            }
            let coeff =
                if rp.is_some_and(|r| r.kind == 2) { crate::math::exp(0.104_166_66 / ms * -1024.0) } else { 0.0 };
            self.trans.insert(key, Trans { id, obj, from: cur, to: value, cur, t0: tick, dur, coeff });
        }
    }

    /// The parameter takes a value: stored, mirrored when global, and told to `moved`.
    fn apply(&mut self, id: Id, value: f64, obj: GameObj, mirror: &RtpcMirror, moved: Moved) {
        if self.vals.insert((obj, id), value) == Some(value) {
            return;
        }
        if obj == GameObj::GLOBAL {
            mirror.set(id, value);
        }
        moved(self, id, obj);
    }

    /// A game parameter's value on an object: its own, else the global value, else the InitBank's default.
    pub fn rtpc(&self, id: Id, obj: GameObj) -> f64 {
        self.vals
            .get(&(obj, id))
            .or_else(|| self.vals.get(&(GameObj::GLOBAL, id)))
            .or_else(|| self.defaults.get(&id))
            .copied()
            .unwrap_or(0.0)
    }

    /// The additive offsets of a chain's curves on an object: (dB of the Volume and
    /// MakeUpGain curves, cents of the Pitch curves).
    pub fn rtpc_of(&self, list: &[Rtpc], obj: GameObj) -> (f64, f64) {
        let (mut db, mut cents) = (0.0, 0.0);
        for r in list {
            let y = curve_value(r, self.rtpc(r.id, obj));
            if r.param == 2 {
                cents += y;
            } else if r.param == 0 || r.param == 7 {
                db += y;
            }
        }
        (db, cents)
    }

    pub fn set_switch(&mut self, group: Id, value: Id, obj: GameObj) {
        self.switches.insert((obj, group), value);
    }

    /// A switch group's value on an object, None while unset.
    pub fn switch(&self, group: Id, obj: GameObj) -> Option<Id> {
        self.switches.get(&(obj, group)).copied()
    }

    /// Sets a state at `now`: the ms the buses' state chunks move over (the group's
    /// transition time), or None while a Pause All holds the transitions (they take that time at the resume).
    pub fn set_state(&mut self, group: Id, value: Id, now: u64) -> Option<f64> {
        self.states.insert(group, value);
        let ms = self.state_groups.get(&group).copied().unwrap_or(0.0);
        if self.state_hold.is_some() {
            self.state_hold = Some(ms);
            return None;
        }
        self.state_end = secs(now) + ms / 1000.0;
        Some(ms)
    }

    /// A state group's value, None while unset.
    pub fn state(&self, group: Id) -> Option<Id> {
        self.states.get(&group).copied()
    }

    /// A Pause All at `now` holds the state transitions: true when they were not held
    /// already (the buses' chunks then stop where they are).
    pub fn hold_states(&mut self, now: u64) -> bool {
        if self.state_hold.is_some() {
            return false;
        }
        self.state_hold = Some(mvs_math::js::max(0.0, self.state_end - secs(now)) * 1000.0);
        true
    }

    /// A Resume All at `now` lets them go: the ms the chunks move over, None when they
    /// were not held.
    pub fn release_states(&mut self, now: u64) -> Option<f64> {
        let ms = self.state_hold.take()?;
        self.state_end = secs(now) + ms / 1000.0;
        Some(ms)
    }

    /// Moves the transitions for the audio frame of `now`, each value to `moved`.
    ///
    /// CAkTransition::ComputeTransition (18003e530), once per audio frame: inside the duration the value is the
    /// linear interpolation at (tick - start) / duration, through the one-pole filter when the transition has one;
    /// from the end tick on it is the target and the transition ends. TransUpdateValue (1800570b0) stores the value
    /// in the parameter and notifies its subscribers, so curves are read at the moving parameter.
    pub fn quantum(&mut self, now: u64, mirror: &RtpcMirror, moved: Moved) {
        if self.trans.is_empty() {
            return;
        }
        let tick = frame_of(now);
        if tick == self.last_tick {
            return;
        }
        self.last_tick = tick;
        let mut i = 0;
        while i < self.trans.len() {
            let (id, obj, cur, ended) = {
                let (_, tr) = self.trans.get_index_mut(i).expect("in range");
                let ended = tick >= tr.t0 + tr.dur as i64;
                if !ended {
                    let t = if tick <= tr.t0 { 0.0 } else { (tick - tr.t0) as f64 / tr.dur };
                    let mut v = tr.from + (tr.to - tr.from) * t;
                    if tr.coeff != 0.0 {
                        v = (tr.cur - tr.to) * tr.coeff + tr.to;
                    }
                    tr.cur = v;
                } else {
                    tr.cur = tr.to;
                }
                (tr.id, tr.obj, tr.cur, ended)
            };
            if ended {
                self.trans.shift_remove_index(i);
            } else {
                i += 1;
            }
            self.apply(id, cur, obj, mirror, moved);
        }
    }
}

/// The audio frame (1024 samples) of a clock time.
fn frame_of(now: u64) -> i64 {
    i64::try_from(now / 1024).unwrap_or(i64::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn init() -> InitBank {
        let mut i = InitBank::default();
        i.rtpc_defaults.insert(7, 10.0);
        i.rtpc_defaults.insert(8, 0.0);
        i.rtpc_ramps.insert(8, RtpcRamp { kind: 2, up: 0.5, down: 0.5 });
        i.state_groups.insert(1, 500.0);
        i
    }

    fn run(s: &mut Syncs, m: &RtpcMirror, ticks: u64) -> Vec<(u64, Id, GameObj, f64)> {
        let mut log = Vec::new();
        for k in 0..ticks {
            let now = k * 1024;
            s.quantum(now, m, &mut |s: &Syncs, id, obj| log.push((k, id, obj, s.rtpc(id, obj))));
        }
        log
    }

    #[test]
    fn instant_and_fallback() {
        let init = init();
        let m = RtpcMirror::new(&init);
        let mut s = Syncs::new(&init);
        let mut seen = vec![];
        assert_eq!(s.rtpc(7, GameObj(3)), 10.0);
        s.set_rtpc(7, 4.0, GameObj::GLOBAL, 0.0, 0, &m, &mut |_, id, o| seen.push((id, o)));
        assert_eq!(seen, vec![(7, GameObj::GLOBAL)]);
        assert_eq!(m.get_set(7), Some(4.0));
        assert_eq!(s.rtpc(7, GameObj(3)), 4.0);
        s.set_rtpc(7, 9.0, GameObj(3), 0.0, 0, &m, &mut |_, id, o| seen.push((id, o)));
        assert_eq!((s.rtpc(7, GameObj(3)), s.rtpc(7, GameObj(4))), (9.0, 4.0));
        // the same value again: nothing
        let n = seen.len();
        s.set_rtpc(7, 9.0, GameObj(3), 0.0, 0, &m, &mut |_, id, o| seen.push((id, o)));
        assert_eq!(seen.len(), n);
    }

    #[test]
    fn linear_transition_in_frames() {
        let init = init();
        let m = RtpcMirror::new(&init);
        let mut s = Syncs::new(&init);
        let mut starts = vec![];
        // 100 ms from the default 10 to 20: ceil(100 / 21.333) = 5 frames
        s.set_rtpc(7, 20.0, GameObj::GLOBAL, 100.0, 0, &m, &mut |_, id, o| starts.push((id, o)));
        assert_eq!(starts.len(), 1, "the start value is applied first");
        let log = run(&mut s, &m, 8);
        let vals: Vec<f64> = log.iter().map(|l| l.3).collect();
        assert_eq!(vals, vec![12.0, 14.0, 16.0, 18.0, 20.0]);
        assert_eq!(log[0].0, 1);
    }

    /// The oracle's own run (through a fake audio context): a
    /// transition retargeted in flight, FilteringOverTime and slew ramps, and an object's own value.
    #[test]
    fn oracle_scenario() {
        let probe: serde_json::Value = serde_json::from_str(include_str!("sync_probe.json")).unwrap();
        let mut init = InitBank::default();
        init.rtpc_defaults.extend([(7, 10.0), (8, 0.0), (9, 5.0)]);
        init.rtpc_ramps.insert(8, RtpcRamp { kind: 2, up: 0.5, down: 0.5 });
        init.rtpc_ramps.insert(9, RtpcRamp { kind: 1, up: 2.0, down: 4.0 });
        let m = RtpcMirror::new(&init);
        let mut s = Syncs::new(&init);
        let mut log: Vec<(u64, Id, f64)> = Vec::new();
        let mut k = 0u64;
        let set = |s: &mut Syncs, id: Id, v: f64, obj: GameObj, ms: f64, k: u64, log: &mut Vec<(u64, Id, f64)>| {
            s.set_rtpc(id, v, obj, ms, k * 1024 + 10, &m, &mut |s: &Syncs, id, o| {
                if o == GameObj::GLOBAL {
                    log.push((k, id, s.rtpc(id, o)));
                }
            });
        };
        set(&mut s, 7, 20.0, GameObj::GLOBAL, 100.0, 0, &mut log);
        set(&mut s, 8, 5.0, GameObj::GLOBAL, 0.0, 0, &mut log);
        set(&mut s, 9, 25.0, GameObj::GLOBAL, 0.0, 0, &mut log);
        while k < 60 {
            if k == 3 {
                set(&mut s, 7, 0.0, GameObj::GLOBAL, 100.0, k, &mut log);
            }
            if k == 30 {
                set(&mut s, 9, 5.0, GameObj::GLOBAL, 0.0, k, &mut log);
            }
            s.quantum(k * 1024 + 10, &m, &mut |s: &Syncs, id, o| log.push((k, id, s.rtpc(id, o))));
            k += 1;
        }
        let want: Vec<(u64, Id, f64)> = probe["log"]
            .as_array()
            .unwrap()
            .iter()
            .map(|e| (e[0].as_u64().unwrap(), e[1].as_u64().unwrap() as Id, e[2].as_f64().unwrap()))
            .collect();
        assert_eq!(log, want);
        let mut obj = Vec::new();
        let mut sink = Vec::new();
        set(&mut s, 7, 33.0, GameObj(3), 200.0, 60, &mut sink);
        for k in 60..80u64 {
            s.quantum(k * 1024 + 10, &m, &mut |_, _, _| {});
            obj.push((k, s.rtpc(7, GameObj(3))));
        }
        let want: Vec<(u64, f64)> =
            probe["obj"].as_array().unwrap().iter().map(|e| (e[0].as_u64().unwrap(), e[1].as_f64().unwrap())).collect();
        assert_eq!(obj, want);
    }
}
