//! The bus tree: a [`Block`] per InitBank bus that the voices routed into
//! it (their dry path) and their aux sends add to; per quantum each bus, children before parents, runs its input
//! gains (HDR, the state MakeUpGain), its effects in order (skipped while a state bypasses them), its volume, and
//! adds the result to its parent's input. A bus without a parent
//! (the Master Audio Bus) goes to the output.

use mvs_data::sound::{Id, InitBank};

use super::Block;
use super::auto::Auto;
use crate::fx::Fx;
use crate::spec::{Param, ParamChange, Target};
use crate::{QUANTUM, RATE};

/// The buses' channels: the graph is stereo from the voices' panners on.
pub const BUS_CHANNELS: usize = 2;

/// The most buses deep a chain is followed (a cycle in a bank stops there).
pub(crate) const MAX_DEPTH: usize = 32;

pub(crate) struct Bus {
    pub(crate) id: Id,
    pub(crate) parent: Option<usize>,
    pub(crate) depth: usize,
    pub(crate) input: Block,
    /// The input gain the HDR window sets (linear).
    hdr: Auto,
    /// The state MakeUpGain, linear (the change's dB converted), and BypassAllFX (above 0.5: bypassed).
    makeup: Auto,
    bypass: Auto,
    /// BusVolume, linear (the change's dB converted).
    vol: Auto,
    /// The state low-pass (Wwise units) the voices routed into the bus or below it add to their own.
    pub(crate) lpf: Auto,
    /// Whether the bus has a state with a low-pass: a voice below it has a low-pass filter from its start.
    pub(crate) state_lpf: bool,
    /// The effects by slot (None: a plugin the port does not run).
    fx: Vec<Option<Fx>>,
}

pub(crate) struct Buses {
    /// By id.
    pub(crate) list: Vec<Bus>,
    /// Indices into `list`, children before parents.
    order: Vec<usize>,
    gain: [f32; QUANTUM],
    /// The input before the effects, for a bypass.
    dry: Block,
    /// The level check's meter (mix/level_tests.rs), None but in that check.
    #[cfg(test)]
    pub(crate) meter: Option<Box<Meter>>,
}

/// The level check's meter on some buses' outputs (after BusVolume, as they go to their parent): per bus, by index,
/// the samples' sum of squares and peak over both channels since the check took them.
#[cfg(test)]
#[derive(Default)]
pub(crate) struct Meter {
    pub(crate) taps: Vec<(usize, Level)>,
}

#[cfg(test)]
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct Level {
    pub(crate) sq: f64,
    /// Samples, every channel's counted.
    pub(crate) n: u64,
    pub(crate) peak: f32,
}

#[cfg(test)]
impl Level {
    pub(crate) fn add(&mut self, s: &[f32]) {
        for &x in s {
            self.sq += f64::from(x) * f64::from(x);
            self.peak = self.peak.max(x.abs());
        }
        self.n += s.len() as u64;
    }
}

/// A dB value as the linear gain a gain node takes (-inf: 0).
pub(crate) fn db_lin(db: f64) -> f64 {
    if db == f64::NEG_INFINITY { 0.0 } else { crate::math::db_to_gain(db) }
}

fn same(v: f64) -> f64 {
    v
}

impl Buses {
    pub(crate) fn new(init: &InitBank) -> Buses {
        let ids: Vec<Id> = init.buses.keys().copied().collect();
        let index = |id: Id| ids.binary_search(&id).ok();
        let mut list: Vec<Bus> = init
            .buses
            .iter()
            .map(|(&id, b)| Bus {
                id,
                parent: b.parent.and_then(index),
                depth: 0,
                input: Block::new(BUS_CHANNELS),
                hdr: Auto::new(1.0),
                makeup: Auto::new(1.0),
                bypass: Auto::new(0.0),
                vol: Auto::new(db_lin(b.vol)),
                lpf: Auto::new(0.0),
                state_lpf: b.states.values().any(|m| m.values().any(|s| s.lpf != 0.0)),
                fx: b
                    .fx
                    .iter()
                    .map(|f| {
                        let driven: Vec<u16> = f.rtpc.iter().map(|r| r.param).collect();
                        Fx::with_curves(&f.fx, f.er.as_ref(), RATE, BUS_CHANNELS, &driven)
                    })
                    .collect(),
            })
            .collect();
        for i in 0..list.len() {
            let mut d = 0;
            let mut p = list[i].parent;
            while let Some(j) = p {
                d += 1;
                if d >= MAX_DEPTH {
                    eprintln!("audio: bus {} is under a cycle", list[i].id);
                    list[i].parent = None;
                    break;
                }
                p = list[j].parent;
            }
            list[i].depth = d;
        }
        let mut order: Vec<usize> = (0..list.len()).collect();
        order.sort_by_key(|&i| std::cmp::Reverse(list[i].depth));
        Buses {
            list,
            order,
            gain: [0.0; QUANTUM],
            dry: Block::new(BUS_CHANNELS),
            #[cfg(test)]
            meter: None,
        }
    }

    pub(crate) fn index(&self, id: Id) -> Option<usize> {
        self.list.binary_search_by_key(&id, |b| b.id).ok()
    }

    /// The chain of buses from `bus` up, written into `out`; its length.
    pub(crate) fn chain(&self, bus: Option<usize>, out: &mut [u16; MAX_DEPTH]) -> usize {
        let mut n = 0;
        let mut b = bus;
        while let Some(i) = b {
            if n == MAX_DEPTH {
                break;
            }
            out[n] = i as u16;
            n += 1;
            b = self.list[i].parent;
        }
        n
    }

    /// Takes the changes due in the quantum at `now` that are not per sample: the state low-pass and the effects'
    /// parameters (the plugins read their parameters once a frame).
    pub(crate) fn take(&mut self, now: u64, due: &[ParamChange]) {
        for c in due {
            match c.target {
                Target::Bus(id) if c.param == Param::BusLpf => {
                    if let Some(i) = self.index(id) {
                        self.list[i].lpf.take(now, std::slice::from_ref(c), c.target, c.param, same);
                    }
                }
                Target::BusFx { bus, slot } => {
                    let Param::FxParam(p) = c.param else { continue };
                    if let Some(i) = self.index(bus)
                        && let Some(Some(fx)) = self.list[i].fx.get_mut(usize::from(slot))
                    {
                        fx.set_param(p, c.value);
                    }
                }
                _ => {}
            }
        }
    }

    pub(crate) fn clear(&mut self) {
        for b in &mut self.list {
            b.input.clear();
        }
    }

    /// Runs the tree for the quantum at `now` and adds the top buses' output to `out`; the meters' values go to
    /// `meters` as (game parameter, value).
    pub(crate) fn run(&mut self, now: u64, due: &[ParamChange], out: &mut Block, meters: &mut Vec<(Id, f64)>) {
        for k in 0..self.order.len() {
            let i = self.order[k];
            let gain = &mut self.gain;
            let b = &mut self.list[i];
            let t = Target::Bus(b.id);
            if b.hdr.fill(now, due, t, Param::BusHdrGain, same, gain) != Some(1.0) {
                apply(&mut b.input, gain);
            }
            // Deviation: the state MakeUpGain at the bus input, not the voices': their aux sends go without it.
            if b.makeup.fill(now, due, t, Param::BusMakeUp, db_lin, gain) != Some(1.0) {
                apply(&mut b.input, gain);
            }
            // a bypass switches to the dry input at its sample; the effects run on meanwhile (the wet and dry gains
            // after them)
            let bypass = b.bypass.fill(now, due, t, Param::BusBypass, same, gain);
            let has_fx = b.fx.iter().any(Option::is_some);
            if has_fx && bypass != Some(0.0) {
                for (d, s) in self.dry.ch.iter_mut().zip(&b.input.ch) {
                    d.copy_from_slice(s);
                }
            }
            for fx in b.fx.iter_mut().flatten() {
                fx.process(&mut b.input);
                if let Some(m) = fx.meter()
                    && meters.len() < meters.capacity()
                {
                    meters.push(m);
                }
            }
            if has_fx && bypass != Some(0.0) {
                for (d, s) in b.input.ch.iter_mut().zip(&self.dry.ch) {
                    for ((d, s), g) in d.iter_mut().zip(s).zip(gain.iter()) {
                        if *g > 0.5 {
                            *d = *s;
                        }
                    }
                }
            }
            if b.vol.fill(now, due, t, Param::BusVolume, db_lin, gain) != Some(1.0) {
                apply(&mut b.input, gain);
            }
            #[cfg(test)]
            if let Some(m) = &mut self.meter {
                for (_, l) in m.taps.iter_mut().filter(|(j, _)| *j == i) {
                    for c in b.input.ch.iter() {
                        l.add(c);
                    }
                }
            }
            let dst = match b.parent {
                Some(p) => {
                    // (children come first in `order`: the parent's block is another element)
                    let (src, dst) = two(&mut self.list, i, p);
                    add(&mut dst.input, &src.input);
                    continue;
                }
                None => &mut *out,
            };
            add(dst, &self.list[i].input);
        }
    }
}

/// Two distinct elements of a slice, mutably.
fn two(list: &mut [Bus], a: usize, b: usize) -> (&mut Bus, &mut Bus) {
    if a < b {
        let (x, y) = list.split_at_mut(b);
        (&mut x[a], &mut y[0])
    } else {
        let (x, y) = list.split_at_mut(a);
        (&mut y[0], &mut x[b])
    }
}

fn apply(b: &mut Block, g: &[f32; QUANTUM]) {
    for c in b.ch.iter_mut() {
        for (s, g) in c.iter_mut().zip(g) {
            *s *= g;
        }
    }
}

pub(crate) fn add(dst: &mut Block, src: &Block) {
    for (d, s) in dst.ch.iter_mut().zip(src.ch.iter()) {
        for (d, s) in d.iter_mut().zip(s) {
            *d += s;
        }
    }
}
