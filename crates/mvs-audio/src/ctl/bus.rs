//! The buses' control: volumes down the tree with their game-parameter curves, the states' volume, low-pass,
//! make-up and bypass, HDR, the effects' game-parameter curves and the meters' ducking.
//!
//! The control side of a bus tree: its bus nodes, HDR tick, state chunks and the effects' parameters. Each node's
//! moving parts are
//! values the mixer's bus tree takes as [`ParamChange`]s on `Target::Bus` and `Target::BusFx`:
//! - `BusVolume`: the bus's own volume (its base and the curves on BusVolume) plus its state chunk's BusVolume, dB;
//! - `BusMakeUp`: the state chunk's MakeUpGain, dB; `BusBypass`: its BypassAllFX; `BusLpf`: its low-pass;
//! - `BusHdrGain`: the HDR window's gain, linear;
//! - `FxParam(id)` on `BusFx { bus, slot }`: a plugin parameter, base plus its game-parameter curves; the slot is the
//!   effect's index in the bus's `fx` list.
//!
//! A gain ramped linearly in amplitude (the curves' gain over one audio frame, a state chunk's over the
//! group's transition time) is stepped here once per quantum along that ramp, each step a linear-in-dB ramp over
//! the quantum.
// The steps keep a ramp's shape to the quantum.
// Deviation: a state's MakeUpGain belongs, as the bus's own, to the voices routed into the bus (GetAudioParameters);
// here it is one gain at the bus input (`BusMakeUp`), so it does not reach those voices' aux sends.

use std::collections::BTreeMap;

use mvs_data::sound::{Accum, Bus, Id, InitBank, Rtpc};

use super::bank::BankView;
use super::curves::{FRAME_SECS, curve_value, db_to_gain};
use super::sync::Syncs;
use super::voice::Voices;
use super::{Ctl, Out, secs};
use crate::GameObj;
use crate::fx::Fx;
use crate::math::{exp, log10};
use crate::spec::{Param, ParamChange, Ramp, Target};

/// The floor a gain's dB takes at zero gain (the mixer's `db_to_gain` of it is 0 in f32).
const SILENT_DB: f64 = -1000.0;

/// A value ramping linearly from `from` at `t0` to `to` at `t1` (an AudioParam's linear ramp from its value at the
/// moment it was set).
#[derive(Clone, Copy, Debug, Default)]
struct Lin {
    from: f64,
    to: f64,
    t0: f64,
    t1: f64,
}

impl Lin {
    fn fixed(v: f64) -> Lin {
        Lin { from: v, to: v, t0: 0.0, t1: 0.0 }
    }

    fn at(&self, t: f64) -> f64 {
        if t >= self.t1 { self.to } else { self.from + (self.to - self.from) * (t - self.t0) / (self.t1 - self.t0) }
    }

    fn moving(&self, t: f64) -> bool {
        t < self.t1
    }

    fn retarget(&mut self, t: f64, to: f64, dur: f64) {
        *self = Lin { from: self.at(t), to, t0: t, t1: t + dur };
    }

    fn hold(&mut self, t: f64) {
        let v = self.at(t);
        *self = Lin { from: v, to: v, t0: t, t1: t };
    }
}

/// A gain ramping linearly in amplitude, read in dB; at its target it reads the target's dB as given.
#[derive(Clone, Copy, Debug)]
struct Gain {
    lin: Lin,
    to_db: f64,
}

impl Gain {
    fn fixed(db: f64) -> Gain {
        Gain { lin: Lin::fixed(gain_of(db)), to_db: db }
    }

    fn db(&self, t: f64) -> f64 {
        if t >= self.lin.t1 { self.to_db } else { lin_db(self.lin.at(t)) }
    }

    fn retarget(&mut self, t: f64, db: f64, dur: f64) {
        self.lin.retarget(t, gain_of(db), dur);
        self.to_db = db;
    }

    fn hold(&mut self, t: f64) {
        self.lin.hold(t);
        self.to_db = lin_db(self.lin.to);
    }
}

fn gain_of(db: f64) -> f64 {
    if db == f64::NEG_INFINITY { 0.0 } else { db_to_gain(db) }
}

fn lin_db(g: f64) -> f64 {
    if g <= 0.0 { f64::NEG_INFINITY } else { 20.0 * log10(g) }
}

/// An effect's parameters that game-parameter curves drive: each with its value before any curve.
struct FxCtl {
    params: Vec<(u16, f64)>,
    curves: Vec<Rtpc>,
}

/// A bus's HDR window.
struct HdrWin {
    thr: f64,
    ratio: f64,
    release: f64,
    top: f64,
    max: f64,
    /// The gain last sent (NaN before the first).
    sent: f64,
}

/// A bus in the tree.
struct Node {
    bus: Bus,
    has_mk: bool,
    has_bypass: bool,
    has_lpf: bool,
    fx: Vec<FxCtl>,
    used: bool,
    /// The curves' gain (base volume and BusVolume curves) and the state chunk's volume gain.
    curves: Gain,
    state: Gain,
    mk: Gain,
    lp: Lin,
    /// What the mixer holds (it starts at 0 for each).
    sent_vol: f64,
    sent_mk: f64,
    sent_lp: f64,
    hdr: Option<HdrWin>,
}

/// The buses' control state.
#[derive(Default)]
pub struct BusCtl {
    nodes: BTreeMap<Id, Node>,
    /// The buses in use, in the order they came into use.
    order: Vec<Id>,
    /// The audio clock time (s) of the last HDR tick.
    hdr_last: f64,
}

impl BusCtl {
    pub fn new(init: &InitBank) -> BusCtl {
        let mut nodes = BTreeMap::new();
        for (&id, bus) in &init.buses {
            let any =
                |f: fn(&mvs_data::sound::StateSet) -> f64| bus.states.values().any(|m| m.values().any(|s| f(s) != 0.0));
            // each effect's driven parameters' values before any curve, read off the effect itself
            let fx = bus
                .fx
                .iter()
                .map(|f| {
                    let mut driven: Vec<u16> = Vec::new();
                    for r in &f.rtpc {
                        if !driven.contains(&r.param) {
                            driven.push(r.param);
                        }
                    }
                    let live = Fx::with_curves(&f.fx, f.er.as_ref(), crate::RATE, 2, &driven);
                    let params = driven
                        .iter()
                        .filter_map(|&p| live.as_ref().and_then(|x| x.base_param(p)).map(|b| (p, b)))
                        .collect();
                    FxCtl { params, curves: f.rtpc.clone() }
                })
                .collect();
            nodes.insert(
                id,
                Node {
                    bus: bus.clone(),
                    has_mk: any(|s| s.mk),
                    has_bypass: any(|s| s.bypass),
                    has_lpf: any(|s| s.lpf),
                    fx,
                    used: false,
                    curves: Gain::fixed(0.0),
                    state: Gain::fixed(0.0),
                    mk: Gain::fixed(0.0),
                    lp: Lin::fixed(0.0),
                    sent_vol: 0.0,
                    sent_mk: 0.0,
                    sent_lp: 0.0,
                    hdr: None,
                },
            );
        }
        BusCtl { order: Vec::with_capacity(nodes.len()), nodes, hdr_last: 0.0 }
    }

    /// A bus comes into use at `now` (made the first time a voice or a send is routed into
    /// it): from then on its curves, state chunk and HDR window run. Nothing for a bus in use already.
    pub fn touch(&mut self, bus: Id, now: u64, syncs: &Syncs, out: &mut Out) {
        let Some(n) = self.nodes.get_mut(&bus) else { return };
        if n.used {
            return;
        }
        n.used = true;
        self.order.push(bus);
        // the one other target of the build is the LPF of bus 2579105490, which no exported sound routes into
        if n.bus.rtpc.iter().any(|r| ![0, 5, 7].contains(&r.param)) {
            let _ =
                out.notes.push(crate::Note::Warn { what: "bus game-parameter target not ported", id: u64::from(bus) });
        }
        n.curves = Gain::fixed(curves_db(&n.bus, syncs));
        if !n.bus.states.is_empty() {
            n.state = Gain::fixed(state_sum(&n.bus, syncs, Key::Vol));
        }
        if n.has_mk {
            n.mk = Gain::fixed(state_sum(&n.bus, syncs, Key::Mk));
        }
        if n.has_lpf {
            n.lp = Lin::fixed(state_sum(&n.bus, syncs, Key::Lpf));
        }
        let target = Target::Bus(bus);
        let vol = pair_db(&n.curves, &n.state, 0.0);
        if vol != 0.0 {
            put(out, now, target, Param::BusVolume, vol, Ramp::Set);
        }
        n.sent_vol = vol;
        if n.has_mk {
            n.sent_mk = n.mk.to_db;
            put(out, now, target, Param::BusMakeUp, n.sent_mk, Ramp::Set);
        }
        if n.has_bypass && !n.fx.is_empty() {
            put(out, now, target, Param::BusBypass, bypass_of(&n.bus, syncs), Ramp::Set);
        }
        if n.has_lpf {
            n.sent_lp = n.lp.to;
            put(out, now, target, Param::BusLpf, n.sent_lp, Ramp::Set);
        }
        for (slot, f) in n.fx.iter().enumerate() {
            for &(p, base) in &f.params {
                let v = fx_prm(&f.curves, p, base, syncs);
                put(out, now, Target::BusFx { bus, slot: slot as u8 }, Param::FxParam(p), v, Ramp::Set);
            }
        }
        if let Some(h) = n.bus.hdr {
            n.hdr = Some(HdrWin {
                thr: h.threshold,
                ratio: h.ratio,
                release: h.release,
                top: h.threshold,
                max: h.threshold,
                sent: f64::NAN,
            });
        }
        if let Some(p) = n.bus.parent {
            self.touch(p, now, syncs, out);
        }
    }

    /// The bus volumes from a bus down to the master, dB (Volume and the BusVolume
    /// curves on global values).
    pub fn volume_db(&self, bus: Id, syncs: &Syncs) -> f64 {
        let mut db = 0.0;
        let mut at = Some(bus);
        while let Some(b) = at {
            let Some(n) = self.nodes.get(&b) else { break };
            db += n.bus.vol;
            for r in n.bus.rtpc.iter().filter(|r| r.param == 5) {
                db += curve_value(r, syncs.rtpc(r.id, GameObj::GLOBAL));
            }
            at = n.bus.parent;
        }
        db
    }

    /// A global game parameter took a value: the curves of the buses in use and of their
    /// effects on it move.
    pub fn rtpc_moved(&mut self, id: Id, now: u64, syncs: &Syncs, out: &mut Out) {
        let t = secs(now);
        for &bus in &self.order {
            let n = self.nodes.get_mut(&bus).expect("a bus in use");
            if n.bus.rtpc.iter().any(|r| r.param == 5 && r.id == id) {
                // the gain moves over one audio frame
                n.curves.retarget(t, curves_db(&n.bus, syncs), FRAME_SECS);
            }
            for (slot, f) in n.fx.iter().enumerate() {
                for &(p, base) in &f.params {
                    if f.curves.iter().any(|r| r.param == p && r.id == id) {
                        let v = fx_prm(&f.curves, p, base, syncs);
                        put(out, now, Target::BusFx { bus, slot: slot as u8 }, Param::FxParam(p), v, Ramp::Set);
                    }
                }
            }
        }
    }

    /// The states changed: the state chunks of the buses in use move to the current
    /// states over `ms`, or with `hold` stop where they are.
    pub fn states_moved(&mut self, ms: f64, hold: bool, now: u64, syncs: &Syncs, out: &mut Out) {
        let t = secs(now);
        let dur = mvs_math::js::max(ms / 1000.0, 0.005);
        for &bus in &self.order {
            let n = self.nodes.get_mut(&bus).expect("a bus in use");
            if n.has_mk {
                if hold {
                    n.mk.hold(t);
                } else {
                    n.mk.retarget(t, state_sum(&n.bus, syncs, Key::Mk), dur);
                }
            }
            if n.has_bypass && !n.fx.is_empty() && !hold {
                put(out, now, Target::Bus(bus), Param::BusBypass, bypass_of(&n.bus, syncs), Ramp::Set);
            }
            if !n.bus.states.is_empty() {
                if hold {
                    n.state.hold(t);
                } else {
                    n.state.retarget(t, state_sum(&n.bus, syncs, Key::Vol), dur);
                }
                if n.has_lpf {
                    let from = n.lp.at(t);
                    let to = if hold { from } else { state_sum(&n.bus, syncs, Key::Lpf) };
                    n.lp = Lin { from, to, t0: t, t1: t + dur };
                }
            }
        }
    }

    /// The HDR window now, from the voices' levels.
    ///
    /// HDR window (CAkBus HDR): the window top is the loudest pushed voice level above the threshold; it rises at
    /// once and falls back with the release time; every voice of the bus is lowered by the part of the window above
    /// the threshold that the ratio removes. Both HDR buses release exponentially (CAkBus::SetInitialParams,
    /// 18005f020: bit 1 of the HDR byte): the window top in dB decays by exp(-frame / release) per audio frame,
    /// which is the same decay taken here over the elapsed time.
    pub fn hdr(&mut self, now: u64, syncs: &Syncs, voices: &Voices, banks: &BankView, out: &mut Out) {
        let t = secs(now);
        let dt = mvs_math::js::max(t - self.hdr_last, 0.0);
        self.hdr_last = t;
        for &b in &self.order {
            if let Some(h) = &mut self.nodes.get_mut(&b).expect("a bus in use").hdr {
                h.max = h.thr;
            }
        }
        for v in &voices.list {
            let Some(h) = &v.hdr else { continue };
            let Some(win) = self.nodes.get_mut(&h.bus).and_then(|n| n.hdr.as_mut()) else { continue };
            if v.silent || t < v.start {
                continue;
            }
            let mut level = h.vol;
            for r in v.plan.rtpc.iter().filter(|r| r.param == 0) {
                level += curve_value(r, syncs.rtpc(r.id, v.obj));
            }
            if let Some(e) = h.env.and_then(|m| banks.media(m)).map(|m| &m.env).filter(|e| !e.is_empty()) {
                // Unverified: a streamed voice's position is its elapsed time at its rate too.
                let mut pos = (t - v.start) * v.rate * h.rate;
                if h.looped && pos >= h.l1 {
                    let len = h.l1 - h.l0;
                    pos = h.l0 + ((pos - h.l0) % if len == 0.0 || len.is_nan() { 1.0 } else { len });
                }
                let mut att = e[e.len() - 1][1];
                if pos <= e[0][0] {
                    att = e[0][1];
                } else {
                    for i in 0..e.len() - 1 {
                        if pos <= e[i + 1][0] {
                            let w = e[i + 1][0] - e[i][0];
                            att = e[i][1]
                                + (e[i + 1][1] - e[i][1]) * (pos - e[i][0])
                                    / if w == 0.0 || w.is_nan() { 1.0 } else { w };
                            break;
                        }
                    }
                }
                // the envelope relative to its peak
                let rel = -att - h.peak;
                if rel < -h.range {
                    continue;
                }
                level += rel;
            }
            if level > win.max {
                win.max = level;
            }
        }
        for &b in &self.order {
            let Some(h) = &mut self.nodes.get_mut(&b).expect("a bus in use").hdr else { continue };
            h.top =
                if h.max >= h.top || h.release <= 0.0 { h.max } else { h.max + (h.top - h.max) * exp(-dt / h.release) };
            let g = db_to_gain(-(h.top - h.thr) * (1.0 - 1.0 / h.ratio));
            if g.to_bits() != h.sent.to_bits() {
                h.sent = g;
                put(out, now, Target::Bus(b), Param::BusHdrGain, g, Ramp::Target(0.002));
            }
        }
    }

    /// Moves the buses' parameters for the quantum at `now`: the HDR window while an HDR bus is in use (run at
    /// every quantum), and the gains and the low-pass that are moving.
    pub fn quantum(&mut self, now: u64, syncs: &Syncs, voices: &Voices, banks: &BankView, out: &mut Out) {
        let t0 = secs(now);
        let q = crate::QUANTUM as f64 / f64::from(crate::RATE);
        let t1 = t0 + q;
        for &bus in &self.order {
            let n = self.nodes.get_mut(&bus).expect("a bus in use");
            let target = Target::Bus(bus);
            let moving = n.curves.lin.moving(t0) || n.state.lin.moving(t0);
            let vol = if moving { pair_db(&n.curves, &n.state, t1) } else { pair_db(&n.curves, &n.state, t0) };
            step(out, now, target, Param::BusVolume, &mut n.sent_vol, vol, moving, q);
            let mk = if n.mk.lin.moving(t0) { n.mk.db(t1) } else { n.mk.db(t0) };
            let moving = n.mk.lin.moving(t0);
            step(out, now, target, Param::BusMakeUp, &mut n.sent_mk, mk, moving, q);
            if n.has_lpf {
                let moving = n.lp.moving(t0);
                step(out, now, target, Param::BusLpf, &mut n.sent_lp, n.lp.at(if moving { t1 } else { t0 }), moving, q);
            }
        }
        if self.order.iter().any(|b| self.nodes[b].hdr.is_some()) {
            self.hdr(now, syncs, voices, banks, out);
        }
    }
}

/// Sends a parameter's value when it differs from what the mixer holds: along a linear ramp over the quantum while
/// the gain moves, else at once.
#[allow(clippy::too_many_arguments)]
fn step(out: &mut Out, now: u64, target: Target, param: Param, sent: &mut f64, value: f64, moving: bool, q: f64) {
    let value = if value == f64::NEG_INFINITY { SILENT_DB } else { value };
    if value.to_bits() == sent.to_bits() {
        return;
    }
    *sent = value;
    put(out, now, target, param, value, if moving { Ramp::Linear(q) } else { Ramp::Set });
}

fn put(out: &mut Out, at: u64, target: Target, param: Param, value: f64, ramp: Ramp) {
    let value = if value == f64::NEG_INFINITY { SILENT_DB } else { value };
    Ctl::change(out, ParamChange { at, target, param, value, ramp });
}

/// The two gains' sum in dB at `t` (`extra` more).
fn pair_db(c: &Gain, s: &Gain, t: f64) -> f64 {
    c.db(t) + s.db(t)
}

/// A bus's own volume: its base and the curves on BusVolume, at the global values (dB).
fn curves_db(bus: &Bus, syncs: &Syncs) -> f64 {
    let mut db = bus.vol;
    for r in bus.rtpc.iter().filter(|r| r.param == 5) {
        db += curve_value(r, syncs.rtpc(r.id, GameObj::GLOBAL));
    }
    db
}

/// The state chunk values a bus's groups give, summed.
// Deviation: one gain / value holds the sum of a bus's state groups, each moved over its own group's transition
// time from where the sum is: exact while one group moves at a time (the engine runs a transition per property of
// each group's chunk).
#[derive(Clone, Copy)]
enum Key {
    Vol,
    Lpf,
    Mk,
    Bypass,
}

fn state_sum(bus: &Bus, syncs: &Syncs, key: Key) -> f64 {
    let mut v = 0.0;
    for (g, m) in &bus.states {
        if let Some(s) = syncs.state(*g).and_then(|st| m.get(&st)) {
            v += match key {
                Key::Vol => s.vol,
                Key::Lpf => s.lpf,
                Key::Mk => s.mk,
                Key::Bypass => s.bypass,
            };
        }
    }
    v
}

/// 1 while a state sets BypassAllFX.
fn bypass_of(bus: &Bus, syncs: &Syncs) -> f64 {
    if state_sum(bus, syncs, Key::Bypass) > 0.0 { 1.0 } else { 0.0 }
}

/// An effect parameter: its base and the curves on it at the global values; an Exclusive curve replaces the base
///.
fn fx_prm(curves: &[Rtpc], param: u16, base: f64, syncs: &Syncs) -> f64 {
    let (mut v, mut add) = (base, 0.0);
    for r in curves.iter().filter(|r| r.param == param) {
        let y = curve_value(r, syncs.rtpc(r.id, GameObj::GLOBAL));
        if r.accum == Some(Accum::Exclusive) {
            v = y;
        } else {
            add += y;
        }
    }
    v + add
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use mvs_data::sound::{Curve, Pt, Scaling, StateSet};

    use super::*;
    use crate::Note;
    use crate::spec::Control;

    fn rig() -> (InitBank, Syncs) {
        let mut init = InitBank::default();
        let curve = Rtpc {
            id: 50,
            param: 5,
            scaling: Scaling::None,
            pts: vec![Pt { x: 0.0, y: 0.0, curve: Curve::Linear }, Pt { x: 100.0, y: -20.0, curve: Curve::Linear }],
            accum: None,
        };
        let mut states = BTreeMap::new();
        states.insert(10, BTreeMap::from([(11, StateSet { vol: -6.0, lpf: 40.0, ..StateSet::default() })]));
        init.buses.insert(1, Bus { vol: -3.0, rtpc: vec![curve], parent: Some(2), states, ..Bus::default() });
        init.buses.insert(2, Bus { vol: -1.0, ..Bus::default() });
        init.state_groups.insert(10, 200.0);
        let s = Syncs::new(&init);
        (init, s)
    }

    fn with_out<R>(f: impl FnOnce(&mut Out) -> R) -> (R, Vec<Control>) {
        let mut controls = Vec::new();
        let (mut tx, _rx) = rtrb::RingBuffer::<Note>::new(8);
        let r = f(&mut Out { controls: &mut controls, notes: &mut tx });
        (r, controls)
    }

    #[test]
    fn volume_down_the_tree() {
        let (init, mut s) = rig();
        let m = crate::RtpcMirror::new(&init);
        let b = BusCtl::new(&init);
        assert_eq!(b.volume_db(1, &s), -4.0);
        s.set_rtpc(50, 50.0, GameObj::GLOBAL, 0.0, 0, &m, &mut |_, _, _| {});
        assert_eq!(b.volume_db(1, &s), -14.0);
        assert_eq!(b.volume_db(2, &s), -1.0);
    }

    #[test]
    fn touch_and_state_ramp() {
        let (init, mut s) = rig();
        let mut b = BusCtl::new(&init);
        let (_, c) = with_out(|o| b.touch(1, 0, &s, o));
        // the bus's -3 dB and its parent's -1 dB, the low-pass at 0 stays unsent (no state set yet)
        let vols: Vec<_> = c
            .iter()
            .filter_map(|c| match c {
                Control::Change(p) if p.param == Param::BusVolume => Some((p.target, p.value)),
                _ => None,
            })
            .collect();
        assert_eq!(vols, vec![(Target::Bus(1), -3.0), (Target::Bus(2), -1.0)]);
        let ms = s.set_state(10, 11, 0).unwrap();
        assert_eq!(ms, 200.0);
        let (_, c) = with_out(|o| b.states_moved(ms, false, 0, &s, o));
        assert!(c.is_empty());
        // quantum 0: one step along the ramp, linear in amplitude, in dB
        let voices = Voices::default();
        let banks = BankView::default();
        let (_, c) = with_out(|o| b.quantum(0, &s, &voices, &banks, o));
        let q = 256.0 / 48000.0;
        let g = 1.0 + (db_to_gain(-6.0) - 1.0) * q / 0.2;
        let want = -3.0 + 20.0 * log10(g);
        let got: Vec<_> = c
            .iter()
            .filter_map(|c| match c {
                Control::Change(p) if p.param == Param::BusVolume => Some(p.value),
                _ => None,
            })
            .collect();
        assert_eq!(got.len(), 1);
        assert!((got[0] - want).abs() < 1e-9, "{} {}", got[0], want);
        // well after the transition: at the target, set at once
        let (_, c) = with_out(|o| b.quantum(48000, &s, &voices, &banks, o));
        let Control::Change(last) =
            c.iter().rev().find(|c| matches!(c, Control::Change(p) if p.param == Param::BusLpf)).unwrap()
        else {
            unreachable!()
        };
        assert_eq!(last.value, 40.0);
    }
}
