//! A playing voice: its source (a decoded medium or
//! a stream) through the resampler, the meters, the low-pass and high-pass, the gain (level, fade-in, stop fade,
//! game-parameter volume, mute), the panner to stereo, then the aux sends and the dry path (distance volume, output
//! bus volume) into the buses.

use std::sync::Arc;

use mvs_data::sound::{FxParams, Id};

use super::auto::Auto;
use super::bus::{Buses, MAX_DEPTH, db_lin};
use super::stream::Streamer;
use crate::ctl::curves::{FRAME_SECS, interp};
use crate::decode::Pcm;
use crate::fx::Fx;
use crate::fx::biquad::{Biquad, voice_hpf, voice_lpf};
use crate::spec::{MAX_CHANNELS, MAX_INSERTS, MAX_SENDS, Param, ParamChange, Target, VoiceSpec};
use crate::{FRAME, QUANTUM, RATE};

/// The voice's source.
pub(crate) enum Source {
    None,
    Mem(Arc<Pcm>),
    /// A streamer slot.
    Stream(usize),
}

/// The read place of a decoded medium.
#[derive(Default)]
struct MemPos {
    /// The frame and the fraction past it.
    i: u32,
    f: f64,
    /// Source frames consumed (the count of plays is a length of plays).
    played: f64,
}

/// The read place of a stream: the frame under the read place and the next, and the fraction past the first.
#[derive(Default)]
struct StreamPos {
    a: [f32; MAX_CHANNELS],
    b: [f32; MAX_CHANNELS],
    f: f64,
    has_a: bool,
    primed: bool,
    /// `b` is past the stream's end; `a` is.
    past: bool,
    done: bool,
}

pub(crate) struct Voice {
    pub(crate) spec: VoiceSpec,
    pub(crate) src: Source,
    /// The medium's rate and channels.
    rate: u32,
    channels: usize,
    mem: MemPos,
    st: StreamPos,
    /// The loop region of a decoded medium, end exclusive, and the source frames to play in all (`loops` medium
    /// lengths; infinite for 0).
    region: [u32; 2],
    total: f64,
    /// The static level: Volume, MakeUpGain and the normalisation, linear.
    level: f64,
    dry: f64,
    /// The fade-in's audio frames (0: none).
    fade_frames: u64,
    fade: Auto,
    rtpc: Auto,
    mute: Auto,
    pause: Auto,
    cents: Auto,
    lpf: Auto,
    hpf: Auto,
    /// The distance attenuation's filter pair, in series after the gain.
    att_lpf: Auto,
    att_hpf: Auto,
    pan: [[Auto; 2]; MAX_CHANNELS],
    dist: Auto,
    sends: [Auto; MAX_SENDS],
    /// Bus indices: the output bus (None: not in the tree, straight to the output), the sends', and the chain from
    /// the output bus up (whose state low-pass the voice takes).
    bus: Option<usize>,
    send_bus: [Option<usize>; MAX_SENDS],
    chain: [u16; MAX_DEPTH],
    chain_n: usize,
    lp: Biquad,
    hp: Biquad,
    lp_on: bool,
    hp_on: bool,
    lp_val: f64,
    hp_val: f64,
    att: [Biquad; 2],
    att_on: [bool; 2],
    att_val: [f64; 2],
    /// The samples after the start the fade-in is held at once a transition took the gain over (a stop's fade
    /// starts from the gain the fade-in reached).
    fade_hold: Option<u64>,
    pub(crate) meters: [Option<Fx>; MAX_INSERTS],
    /// A stop asked for, and the sample the voice ends on.
    stop_at: Option<u64>,
    end_at: u64,
    /// Ended: the sample, and whether the source ran out (else it was stopped).
    pub(crate) ended: Option<(u64, bool)>,
}

/// The per-quantum buffers a voice renders with (one set for all voices).
pub(crate) struct Scratch {
    pub(crate) sig: [[f32; QUANTUM]; MAX_CHANNELS],
    gain: [f32; QUANTUM],
    tmp: [f32; QUANTUM],
    cents: [f32; QUANTUM],
    pause: [f32; QUANTUM],
    pan: [[[f32; QUANTUM]; 2]; MAX_CHANNELS],
    pub(crate) stereo: [[f32; QUANTUM]; 2],
    meter: Vec<super::Block>,
}

impl Scratch {
    pub(crate) fn new() -> Scratch {
        Scratch {
            sig: [[0.0; QUANTUM]; MAX_CHANNELS],
            gain: [0.0; QUANTUM],
            tmp: [0.0; QUANTUM],
            cents: [0.0; QUANTUM],
            pause: [0.0; QUANTUM],
            pan: [[[0.0; QUANTUM]; 2]; MAX_CHANNELS],
            stereo: [[0.0; QUANTUM]; 2],
            meter: (1..=MAX_CHANNELS).map(super::Block::new).collect(),
        }
    }
}

fn same(v: f64) -> f64 {
    v
}

/// The source's rate step at `cents` of pitch.
fn step(cents: f64, rate: u32) -> f64 {
    crate::math::pow(2.0, cents / 1200.0) * f64::from(rate) / f64::from(RATE)
}

impl Voice {
    /// A free slot's voice (its filters made for the most channels, so a start allocates nothing).
    pub(crate) fn empty() -> Voice {
        let a = Auto::new(0.0);
        Voice {
            spec: VoiceSpec::default(),
            src: Source::None,
            rate: RATE,
            channels: 0,
            mem: MemPos::default(),
            st: StreamPos::default(),
            region: [0, 0],
            total: 0.0,
            level: 0.0,
            dry: 1.0,
            fade_frames: 0,
            fade: a,
            rtpc: a,
            mute: a,
            pause: a,
            cents: a,
            lpf: a,
            hpf: a,
            att_lpf: a,
            att_hpf: a,
            pan: [[a; 2]; MAX_CHANNELS],
            dist: a,
            sends: [a; MAX_SENDS],
            bus: None,
            send_bus: [None; MAX_SENDS],
            chain: [0; MAX_DEPTH],
            chain_n: 0,
            lp: Biquad::new(MAX_CHANNELS),
            hp: Biquad::new(MAX_CHANNELS),
            lp_on: false,
            hp_on: false,
            lp_val: f64::NAN,
            hp_val: f64::NAN,
            att: [Biquad::new(MAX_CHANNELS), Biquad::new(MAX_CHANNELS)],
            att_on: [false; 2],
            att_val: [f64::NAN; 2],
            fade_hold: None,
            meters: [const { None }; MAX_INSERTS],
            stop_at: None,
            end_at: u64::MAX,
            ended: None,
        }
    }

    /// Starts the voice of `spec` on `src` (`rate` / `channels` / `frames`: the medium's) in this slot.
    pub(crate) fn start(
        &mut self,
        spec: &VoiceSpec,
        src: Source,
        rate: u32,
        channels: usize,
        frames: u32,
        buses: &Buses,
    ) {
        let s = spec;
        self.spec = *s;
        self.src = src;
        self.rate = rate;
        self.channels = channels.min(MAX_CHANNELS);
        self.mem = MemPos { i: s.offset, f: 0.0, played: 0.0 };
        self.st = StreamPos::default();
        // the medium's loop points, as a Web Audio buffer source takes them: clamped to the medium, the whole medium
        // where they are empty
        let [a, b] = s.loop_points.unwrap_or([0, frames]);
        let (a, b) = (a.min(frames), b.min(frames));
        self.region = if b > a { [a, b] } else { [0, frames] };
        self.total = if s.loops == 0 { f64::INFINITY } else { f64::from(frames) * f64::from(s.loops) };
        self.level = db_lin(s.vol + s.makeup + s.norm);
        self.dry = db_lin(s.dry);
        self.fade_frames = if s.fade_in.secs > 0.0 { (s.fade_in.secs / FRAME_SECS - 1e-6).ceil() as u64 } else { 0 };
        self.fade = Auto::new(1.0);
        self.rtpc = Auto::new(s.rtpc_gain);
        self.mute = Auto::new(1.0);
        self.pause = Auto::new(0.0);
        self.cents = Auto::new(s.cents);
        self.lpf = Auto::new(s.lpf);
        self.hpf = Auto::new(s.hpf);
        self.att_lpf = Auto::new(0.0);
        self.att_hpf = Auto::new(0.0);
        for (c, p) in self.pan.iter_mut().enumerate() {
            *p = [Auto::new(s.pan[c][0]), Auto::new(s.pan[c][1])];
        }
        self.dist = Auto::new(s.dist);
        for i in 0..MAX_SENDS {
            self.sends[i] = Auto::new(s.sends[i].gain);
            self.send_bus[i] = if i < usize::from(s.sends_n) { buses.index(s.sends[i].bus) } else { None };
        }
        self.bus = buses.index(s.bus);
        self.chain_n = buses.chain(self.bus, &mut self.chain);
        let state_lpf = self.chain[..self.chain_n].iter().any(|&b| buses.list[usize::from(b)].state_lpf);
        let [al, ah] = &mut self.att;
        for z in self.lp.z.iter_mut().chain(self.hp.z.iter_mut()).chain(al.z.iter_mut()).chain(ah.z.iter_mut()) {
            *z = Default::default();
        }
        self.lp_on = s.lpf > 0.0 || state_lpf;
        self.hp_on = s.hpf > 0.0;
        self.lp_val = f64::NAN;
        self.hp_val = f64::NAN;
        self.att_on = [false; 2];
        self.att_val = [f64::NAN; 2];
        self.fade_hold = None;
        for (m, p) in self.meters.iter_mut().zip(&s.meters) {
            *m = p.as_ref().and_then(|p| Fx::new(&FxParams::AkMeterFX(*p), None, RATE, self.channels));
        }
        self.stop_at = None;
        self.end_at = u64::MAX;
        self.ended = None;
    }

    /// The fade-in's factor `e` samples after the start: the transition steps once per audio frame along the
    /// action's curve, a line within each frame (playVoice's linearRampToValueAtTime per frame).
    fn fade_in(&self, e: u64) -> f64 {
        let n = self.fade_frames;
        if n == 0 {
            return 1.0;
        }
        let frame = FRAME as u64;
        let k = e / frame;
        if k >= n {
            return 1.0;
        }
        let x = (e % frame) as f64 / frame as f64;
        let c = self.spec.fade_in.curve;
        let a = if k == 0 { 0.0 } else { interp(c, k as f64 / n as f64, 0.0, 1.0) };
        let b = interp(c, (k + 1) as f64 / n as f64, 0.0, 1.0);
        a + (b - a) * x
    }

    /// Renders the quantum at `now` into the buses (and `loose`, for a voice whose bus is not in the tree); the
    /// meters' values go to `meters`.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn render(
        &mut self,
        now: u64,
        due: &[ParamChange],
        buses: &mut Buses,
        streamer: &mut Streamer,
        sc: &mut Scratch,
        loose: &mut super::Block,
        meters: &mut Vec<(Id, f64, u64)>,
    ) {
        let t = Target::Voice(self.spec.voice);
        // the parameters per sample (their changes taken at their sample)
        self.cents.fill(now, due, t, Param::Cents, same, &mut sc.cents);
        self.pause.fill(now, due, t, Param::Pause, same, &mut sc.pause);
        // a transition cancels the fade-in and starts from the gain it reached (the gain's value,
        // held, ramped by the ratios)
        // Held at the change's sample.
        if self.fade_hold.is_none()
            && let Some(c) = due.iter().find(|c| c.target == t && c.param == Param::Fade)
        {
            self.fade_hold = Some(c.at.max(now).saturating_sub(self.spec.at));
        }
        self.fade.fill(now, due, t, Param::Fade, same, &mut sc.gain);
        self.rtpc.fill(now, due, t, Param::RtpcGain, same, &mut sc.tmp);
        mul(&mut sc.gain, &sc.tmp);
        self.mute.fill(now, due, t, Param::Mute, same, &mut sc.tmp);
        mul(&mut sc.gain, &sc.tmp);
        self.lpf.take(now, due, t, Param::Lpf, same);
        self.hpf.take(now, due, t, Param::Hpf, same);
        self.att_lpf.take(now, due, t, Param::AttLpf, same);
        self.att_hpf.take(now, due, t, Param::AttHpf, same);
        let n_in = self.channels;
        for c in 0..n_in {
            for side in 0..2 {
                self.pan[c][side].fill(
                    now,
                    due,
                    t,
                    Param::Pan { ch: c as u8, side: side as u8 },
                    same,
                    &mut sc.pan[c][side],
                );
            }
        }
        if let Some(c) = due.iter().find(|c| c.target == t && c.param == Param::Stop) {
            let at = c.at.max(now);
            self.stop_at = Some(self.stop_at.map_or(at, |s| s.min(at)));
        }
        if let Some(s) = self.stop_at {
            // a stop ends the voice once its stop fade has run (at once without one)
            self.end_at = s.max(self.fade.settles().unwrap_or(0));
        }

        // the source, resampled
        let start = self.spec.at;
        let cents_held = sc.cents.iter().all(|&c| c == sc.cents[0]);
        let mut rate = step(f64::from(sc.cents[0]), self.rate);
        for c in sc.sig.iter_mut().take(n_in) {
            c.fill(0.0);
        }
        for i in 0..QUANTUM {
            let tt = now + i as u64;
            if self.ended.is_some() {
                continue;
            }
            if tt >= self.end_at {
                self.ended = Some((tt, false));
                continue;
            }
            if tt < start {
                continue;
            }
            if sc.pause[i] > 0.5 {
                continue;
            }
            if !cents_held {
                rate = step(f64::from(sc.cents[i]), self.rate);
            }
            let ok = match &self.src {
                Source::Mem(p) => {
                    mem_next(&mut self.mem, p, self.region, self.total, self.spec.loops, rate, n_in, &mut sc.sig, i)
                }
                Source::Stream(slot) => stream_next(&mut self.st, streamer, *slot, rate, n_in, &mut sc.sig, i),
                Source::None => false,
            };
            if !ok {
                self.ended = Some((tt, true));
            }
            // A voice that starts late (its start before the quantum) is part-way into its fade-in.
            let e = (tt - start).min(self.fade_hold.unwrap_or(u64::MAX));
            let g = self.level * self.fade_in(e);
            sc.gain[i] *= g as f32;
        }
        // samples outside the voice's life are silent already; the gain before the start is not used

        // the meters (pass-through): on a copy of the source
        // a value per 256-sample quantum
        for m in self.meters.iter_mut().flatten() {
            let b = &mut sc.meter[n_in.max(1) - 1];
            for c in 0..n_in {
                b.ch[c] = sc.sig[c];
            }
            m.process(b);
            if let Some((gp, v)) = m.meter()
                && meters.len() < meters.capacity()
            {
                meters.push((gp, v, now));
            }
        }

        // the low-pass: the voice's value and its buses' state values
        let mut lpv = self.lpf.value(now);
        for &b in &self.chain[..self.chain_n] {
            lpv += buses.list[usize::from(b)].lpf.value(now);
        }
        if lpv > 0.0 {
            self.lp_on = true;
        }
        if self.lp_on {
            if lpv != self.lp_val {
                self.lp_val = lpv;
                self.lp.c = voice_lpf(lpv.min(100.0), RATE);
            }
            for c in 0..n_in {
                self.lp.run(c, &mut sc.sig[c]);
            }
        }
        let hpv = self.hpf.value(now);
        if hpv > 0.0 {
            self.hp_on = true;
        }
        if self.hp_on {
            if hpv != self.hp_val {
                self.hp_val = hpv;
                self.hp.c = voice_hpf(hpv.min(100.0), RATE);
            }
            for c in 0..n_in {
                self.hp.run(c, &mut sc.sig[c]);
            }
        }

        // the gain, then the distance attenuation's filter pair, then the panner to stereo
        for x in sc.sig.iter_mut().take(n_in) {
            mul(x, &sc.gain);
        }
        let att = [self.att_lpf.value(now), self.att_hpf.value(now)];
        for (k, (&a, f)) in att.iter().zip(self.att.iter_mut()).enumerate() {
            if a > 0.0 {
                self.att_on[k] = true;
            }
            if !self.att_on[k] {
                continue;
            }
            if a != self.att_val[k] {
                self.att_val[k] = a;
                let v = a.min(100.0);
                f.c = if k == 0 { voice_lpf(v, RATE) } else { voice_hpf(v, RATE) };
            }
            for (c, x) in sc.sig.iter_mut().enumerate().take(n_in) {
                f.run(c, x);
            }
        }
        for s in sc.stereo.iter_mut() {
            s.fill(0.0);
        }
        for c in 0..n_in {
            let x = &sc.sig[c];
            for side in 0..2 {
                let p = &sc.pan[c][side];
                let o = &mut sc.stereo[side];
                for i in 0..QUANTUM {
                    o[i] += x[i] * p[i];
                }
            }
        }

        // the aux sends, from the panned signal
        for k in 0..usize::from(self.spec.sends_n).min(MAX_SENDS) {
            self.sends[k].fill(now, due, t, Param::Send(k as u8), same, &mut sc.tmp);
            if let Some(b) = self.send_bus[k] {
                add_scaled(&mut buses.list[b].input, &sc.stereo, &sc.tmp, 1.0);
            }
        }
        // the dry path: distance volume, output bus volume
        self.dist.fill(now, due, t, Param::Dist, same, &mut sc.tmp);
        match self.bus {
            Some(b) => add_scaled(&mut buses.list[b].input, &sc.stereo, &sc.tmp, self.dry as f32),
            None => add_scaled(loose, &sc.stereo, &sc.tmp, self.dry as f32),
        }
    }
}

fn mul(a: &mut [f32; QUANTUM], b: &[f32; QUANTUM]) {
    for (a, b) in a.iter_mut().zip(b) {
        *a *= b;
    }
}

fn add_scaled(dst: &mut super::Block, src: &[[f32; QUANTUM]; 2], g: &[f32; QUANTUM], k: f32) {
    for (d, s) in dst.ch.iter_mut().zip(src) {
        for i in 0..QUANTUM {
            d[i] += s[i] * g[i] * k;
        }
    }
}

/// Writes sample `i` of a decoded medium's voice and moves on by `rate`; false when it has run out.
// Unverified: the source resamples by linear interpolation, as Wwise's pitch resampler does (a medium not at the
// mixer's rate goes through the same step).
#[allow(clippy::too_many_arguments)]
fn mem_next(
    m: &mut MemPos,
    p: &Pcm,
    region: [u32; 2],
    total: f64,
    loops: u16,
    rate: f64,
    n_in: usize,
    sig: &mut [[f32; QUANTUM]; MAX_CHANNELS],
    i: usize,
) -> bool {
    let frames = p.frames;
    if m.played >= total || m.i >= frames {
        return false;
    }
    let looping = loops != 1;
    let mut j = m.i + 1;
    if looping && j >= region[1] {
        j = region[0];
    }
    let n = frames as usize;
    let (i0, i1) = (m.i as usize, j as usize);
    let f = m.f as f32;
    for (c, s) in sig.iter_mut().enumerate().take(n_in.min(usize::from(p.channels))) {
        let x = &p.data[c * n..(c + 1) * n];
        let a = x[i0];
        s[i] = if f == 0.0 {
            a
        } else {
            let b = if i1 < n { x[i1] } else { 0.0 };
            a + (b - a) * f
        };
    }
    // advance
    m.played += rate;
    m.f += rate;
    let k = m.f.floor();
    m.f -= k;
    let mut pos = u64::from(m.i) + k as u64;
    let still = loops != 1 && m.played < total;
    if still && pos >= u64::from(region[1]) {
        let len = u64::from(region[1] - region[0]);
        pos = u64::from(region[0]) + (pos - u64::from(region[1])) % len.max(1);
    }
    m.i = pos.min(u64::from(u32::MAX)) as u32;
    true
}

/// Writes sample `i` of a streamed voice and moves on by `rate`; false when it has run out. A stream the streamer
/// has not filled yet holds the voice where it is.
// Deviation: a stream that runs dry (the streamer behind) stalls the voice, silent, until frames come, where the
// build's streamed source waits on its prefetch the same way but before the voice starts.
fn stream_next(
    st: &mut StreamPos,
    streamer: &mut Streamer,
    slot: usize,
    rate: f64,
    n_in: usize,
    sig: &mut [[f32; QUANTUM]; MAX_CHANNELS],
    i: usize,
) -> bool {
    if st.done {
        return false;
    }
    let s = &mut streamer.slots[slot];
    let ch = s.channels;
    let mut pop = |dst: &mut [f32; MAX_CHANNELS]| -> Option<bool> {
        // Some(true) a frame, Some(false) past the end, None nothing yet
        if s.rx.slots() >= ch {
            if let Ok(chunk) = s.rx.read_chunk(ch) {
                let (a, b) = chunk.as_slices();
                for (k, v) in a.iter().chain(b).enumerate() {
                    if k < MAX_CHANNELS {
                        dst[k] = *v;
                    }
                }
                chunk.commit_all();
            }
            return Some(true);
        }
        if s.ended() && s.rx.slots() < ch { Some(false) } else { None }
    };
    if !st.primed {
        if !st.has_a {
            match pop(&mut st.a) {
                Some(true) => st.has_a = true,
                Some(false) => return false,
                None => return true,
            }
        }
        match pop(&mut st.b) {
            Some(true) => {}
            Some(false) => {
                st.b = [0.0; MAX_CHANNELS];
                st.past = true;
            }
            None => return true,
        }
        st.primed = true;
    }
    // on to the read place (a stall leaves the sample silent and the place where it is)
    while st.f >= 1.0 {
        if st.past {
            st.done = true;
            return false;
        }
        let mut next = [0.0; MAX_CHANNELS];
        match pop(&mut next) {
            Some(true) => {}
            Some(false) => st.past = true,
            None => return true,
        }
        st.f -= 1.0;
        st.a = st.b;
        st.b = next;
    }
    let f = st.f as f32;
    for (c, s) in sig.iter_mut().enumerate().take(n_in) {
        s[i] = st.a[c] + (st.b[c] - st.a[c]) * f;
    }
    st.f += rate;
    true
}
