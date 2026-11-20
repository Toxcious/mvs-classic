//! The DSP offline on a control list: a synthetic medium, plain buses (no effects).

use std::cell::Cell;
use std::sync::Arc;

use mvs_data::sound::{Bus, Curve, Id, InitBank};

use super::Dsp;
use crate::ctl::scope::ScopeLoad;
use crate::decode::Pcm;
use crate::spec::{Control, Fade, Param, ParamChange, Ramp, Send, Target, VoiceId, VoiceSpec};
use crate::{Note, QUANTUM};

const MASTER: Id = 1;
const CHILD: Id = 2;
const AUX: Id = 3;
const MEDIUM: Id = 77;

fn init() -> InitBank {
    let mut i = InitBank::default();
    i.buses.insert(MASTER, Bus::default());
    i.buses.insert(CHILD, Bus { parent: Some(MASTER), ..Bus::default() });
    i.buses.insert(AUX, Bus { parent: Some(MASTER), aux: true, ..Bus::default() });
    i
}

/// A stereo medium: left is the frame number / 1000, right its negative.
fn medium(frames: u32, loop_points: Option<[u32; 2]>) -> Arc<Pcm> {
    let n = frames as usize;
    let mut data = vec![0.0f32; 2 * n];
    for k in 0..n {
        data[k] = k as f32 / 1000.0;
        data[n + k] = -(k as f32) / 1000.0;
    }
    Arc::new(Pcm { id: MEDIUM, rate: crate::RATE, channels: 2, frames, loop_points, data: data.into_boxed_slice() })
}

fn spec(at: u64) -> VoiceSpec {
    let mut s = VoiceSpec {
        voice: VoiceId(1),
        at,
        medium: MEDIUM,
        loops: 1,
        bus: CHILD,
        rtpc_gain: 1.0,
        channels: 2,
        dist: 1.0,
        ..VoiceSpec::default()
    };
    s.pan[0] = [1.0, 0.0];
    s.pan[1] = [0.0, 1.0];
    s
}

struct Rig {
    dsp: Dsp,
    notes: rtrb::Producer<Note>,
    rx: rtrb::Consumer<Note>,
}

fn rig(pcm: Arc<Pcm>) -> Rig {
    let (mut notes, rx) = rtrb::RingBuffer::new(1024);
    let mut dsp = Dsp::new(&init());
    dsp.hold(9, &ScopeLoad { media: vec![pcm], ..ScopeLoad::default() }, &mut notes);
    Rig { dsp, notes, rx }
}

impl Rig {
    fn apply(&mut self, cs: &[Control]) {
        for c in cs {
            self.dsp.apply(c, &mut self.notes);
        }
    }
    /// Renders `quanta` quanta; the stereo output.
    fn run(&mut self, quanta: usize) -> Vec<[f32; 2]> {
        let mut out = vec![0.0f32; QUANTUM * 2];
        let mut all = Vec::new();
        for _ in 0..quanta {
            self.dsp.render(&mut out, 2, &mut self.notes);
            all.extend(out.as_chunks::<2>().0.iter().copied());
        }
        all
    }
}

fn change(at: u64, param: Param, value: f64, ramp: Ramp) -> Control {
    Control::Change(ParamChange { at, target: Target::Voice(VoiceId(1)), param, value, ramp })
}

#[test]
fn plays_sample_exact() {
    let mut r = rig(medium(600, None));
    let at = 100;
    r.apply(&[Control::Start(spec(at))]);
    let out = r.run(4);
    for (t, f) in out.iter().enumerate() {
        let want = if (100..700).contains(&t) { (t - 100) as f32 / 1000.0 } else { 0.0 };
        assert_eq!(f, &[want, -want], "sample {t}");
    }
    assert_eq!(r.dsp.ended.as_slice(), &[(VoiceId(1), 700)]);
    assert_eq!(r.dsp.voices(), 0);
}

#[test]
fn loops_over_the_loop_points() {
    // three plays: the lead-in once, then the region until three medium lengths have played
    let mut r = rig(medium(30, Some([10, 20])));
    let mut s = spec(0);
    s.loops = 3;
    s.loop_points = Some([10, 20]);
    r.apply(&[Control::Start(s)]);
    let out = r.run(1);
    let mut want: Vec<usize> = (0..20).collect();
    while want.len() < 90 {
        want.extend(10..20);
    }
    want.truncate(90);
    for (t, f) in out.iter().enumerate() {
        let w = want.get(t).map_or(0.0, |&k| k as f32 / 1000.0);
        assert_eq!(f[0], w, "sample {t}");
    }
    assert_eq!(r.dsp.ended.as_slice(), &[(VoiceId(1), 90)]);

    // loops 0: until stopped
    let mut r = rig(medium(30, None));
    let mut s = spec(0);
    s.loops = 0;
    r.apply(&[Control::Start(s)]);
    let out = r.run(2);
    for (t, f) in out.iter().enumerate() {
        assert_eq!(f[0], (t % 30) as f32 / 1000.0, "sample {t}");
    }
    assert!(r.dsp.ended.is_empty());
    r.apply(&[change(600, Param::Stop, 0.0, Ramp::Set)]);
    let out = r.run(2);
    assert_eq!(out[599 - 512][0], (599 % 30) as f32 / 1000.0);
    assert!(out[600 - 512..].iter().all(|f| f[0] == 0.0));
    // a stop is not a run-out
    assert!(r.dsp.ended.is_empty());
    assert_eq!(r.dsp.voices(), 0);
}

#[test]
fn a_stop_behind_a_full_change_list_stops_the_loop() {
    let mut r = rig(medium(30, None));
    let mut s = spec(0);
    s.loops = 0;
    r.apply(&[Control::Start(s)]);
    // the list filled with another voice's far changes
    let other = Target::Voice(VoiceId(2));
    for k in 0..super::MAX_PENDING {
        let c =
            ParamChange { at: 1_000_000 + k as u64, target: other, param: Param::Cents, value: 0.0, ramp: Ramp::Set };
        r.apply(&[Control::Change(c)]);
    }
    r.apply(&[change(300, Param::Stop, 0.0, Ramp::Set)]);
    let out = r.run(2);
    assert_eq!(out[299][0], (299 % 30) as f32 / 1000.0);
    assert!(out[300..].iter().all(|f| f[0] == 0.0), "the loop plays on");
    assert_eq!(r.dsp.voices(), 0);
}

#[test]
fn a_change_ramps() {
    let mut r = rig(medium(2000, None));
    r.apply(&[
        Control::Start(spec(0)),
        // the stop fade: a line to 0 over 200 samples from sample 300, then the stop
        change(300, Param::Fade, 0.0, Ramp::Linear(200.0 / 48_000.0)),
        change(300, Param::Stop, 0.0, Ramp::Set),
        // a game-parameter volume set at 50
        change(50, Param::RtpcGain, 0.5, Ramp::Set),
    ]);
    let out = r.run(3);
    for (t, f) in out.iter().enumerate() {
        let x = t as f32 / 1000.0;
        let rt = if t >= 50 { 0.5f32 } else { 1.0 };
        let fade = if t < 300 {
            1.0
        } else if t < 500 {
            1.0 - (t - 300) as f64 / 200.0
        } else {
            0.0
        };
        let want = x * (fade as f32) * rt;
        assert_eq!(f[0], want, "sample {t}");
    }
    assert!(r.dsp.ended.is_empty());
    assert_eq!(r.dsp.voices(), 0);
}

#[test]
fn fades_in_per_frame() {
    let mut r = rig(medium(5000, None));
    let mut s = spec(0);
    // two build frames along Log1
    s.fade_in = Fade { secs: 2048.0 / 48_000.0, curve: Curve::Log1 };
    r.apply(&[Control::Start(s)]);
    let out = r.run(12);
    let k1 = crate::ctl::curves::interp(Curve::Log1, 0.5, 0.0, 1.0);
    let at = |t: usize| -> f64 {
        if t < 1024 {
            k1 * t as f64 / 1024.0
        } else if t < 2048 {
            k1 + (1.0 - k1) * (t - 1024) as f64 / 1024.0
        } else {
            1.0
        }
    };
    for t in [0, 512, 1024, 1500, 2048, 2500] {
        let want = (t as f32 / 1000.0) * (at(t) as f32);
        assert_eq!(out[t][0], want, "sample {t}");
    }
}

#[test]
fn a_stop_in_the_fade_in_fades_from_its_gain() {
    let mut r = rig(medium(5000, None));
    let mut s = spec(0);
    // two build frames, linear: at sample 512 the fade-in is at 0.25
    s.fade_in = Fade { secs: 2048.0 / 48_000.0, curve: Curve::Linear };
    r.apply(&[
        Control::Start(s),
        change(512, Param::Fade, 0.0, Ramp::Linear(512.0 / 48_000.0)),
        change(512, Param::Stop, 0.0, Ramp::Set),
    ]);
    let out = r.run(6);
    for (t, f) in out.iter().enumerate() {
        let x = t as f32 / 1000.0;
        let want = if t < 512 {
            x * (0.5 * t as f64 / 1024.0) as f32
        } else if t < 1024 {
            let fade = 1.0 - (t - 512) as f64 / 512.0;
            x * (fade as f32 * 0.25f32)
        } else {
            0.0
        };
        assert_eq!(f[0], want, "sample {t}");
    }
    assert_eq!(r.dsp.voices(), 0);
}

#[test]
fn a_bus_state_makes_up_and_bypasses() {
    let mut r = rig(medium(1000, None));
    let bus = |at: u64, param: Param, value: f64| {
        Control::Change(ParamChange { at, target: Target::Bus(CHILD), param, value, ramp: Ramp::Set })
    };
    r.apply(&[Control::Start(spec(0)), bus(100, Param::BusMakeUp, -6.0), bus(100, Param::BusBypass, 1.0)]);
    let out = r.run(2);
    let g = crate::math::db_to_gain(-6.0) as f32;
    for (t, f) in out.iter().enumerate() {
        let x = t as f32 / 1000.0;
        assert_eq!(f[0], if t < 100 { x } else { x * g }, "sample {t}");
    }
}

#[test]
fn buses_sum_and_send() {
    let mut r = rig(medium(1000, None));
    let mut s = spec(0);
    s.sends_n = 1;
    s.sends[0] = Send { bus: AUX, gain: 0.5 };
    s.dry = -6.0;
    r.apply(&[
        Control::Start(s),
        // the aux bus at -inf dB from sample 256: only the dry path is left
        Control::Change(ParamChange {
            at: 256,
            target: Target::Bus(AUX),
            param: Param::BusVolume,
            value: f64::NEG_INFINITY,
            ramp: Ramp::Set,
        }),
    ]);
    let out = r.run(2);
    let dry = crate::math::db_to_gain(-6.0) as f32;
    for (t, f) in out.iter().enumerate() {
        let x = t as f32 / 1000.0;
        let want = if t < 256 { x * dry + x * 0.5 } else { x * dry };
        assert_eq!(f[0], want, "sample {t}");
    }
}

#[test]
fn pitch_resamples_linearly() {
    let mut r = rig(medium(1000, None));
    let mut s = spec(0);
    s.cents = 1200.0; // twice the rate
    r.apply(&[Control::Start(s)]);
    let out = r.run(3);
    for (t, f) in out.iter().enumerate().take(500) {
        assert_eq!(f[0], (2 * t) as f32 / 1000.0, "sample {t}");
    }
    assert_eq!(r.dsp.ended.as_slice(), &[(VoiceId(1), 500)]);
}

#[test]
fn media_go_back_to_the_game() {
    let mut r = rig(medium(1000, None));
    r.apply(&[Control::Start(spec(0))]);
    r.run(1);
    // the scope goes while the voice plays: the table's reference goes back, the voice's after it ends
    let mut notes = std::mem::replace(&mut r.notes, rtrb::RingBuffer::new(1).0);
    r.dsp.release(9, &mut notes);
    r.notes = notes;
    r.run(4);
    let freed =
        std::iter::from_fn(|| r.rx.pop().ok()).filter(|n| matches!(n, Note::Free(crate::Garbage::Media(_)))).count();
    assert_eq!(freed, 2);
}

#[test]
fn streams_as_the_decoded_medium_plays() {
    // a medium of the oracle store (no synthetic Vorbis): the streamed voice against the decoded one, two plays
    let Some((entry, setup)) = crate::decode::tests::oracle_entry(0) else {
        eprintln!("stream test: no oracle store, skipped");
        return;
    };
    let m = mvs_data::sound::decode_medium(&entry, "test").unwrap();
    let pcm = crate::decode::decode(MEDIUM, &m, &setup).unwrap();
    let frames = pcm.frames as usize;
    let mut s = spec(0);
    s.loops = 2;
    s.channels = pcm.channels as u8;
    if pcm.channels == 1 {
        s.pan[0] = [1.0, 0.5];
    }
    let quanta = (2 * frames).div_ceil(QUANTUM) + 2;
    let mut mem = rig(Arc::new(pcm));
    mem.apply(&[Control::Start(s)]);
    let want = mem.run(quanta);

    let (mut notes, _rx) = rtrb::RingBuffer::new(1024);
    let mut dsp = Dsp::new(&init());
    let load = ScopeLoad {
        streamed: vec![Arc::new(crate::decode::Coded::new(MEDIUM, entry.into_boxed_slice(), &setup).unwrap())],
        ..ScopeLoad::default()
    };
    dsp.hold(9, &load, &mut notes);
    s.streamed = true;
    dsp.apply(&Control::Start(s), &mut notes);
    let mut st = Rig { dsp, notes, rx: rtrb::RingBuffer::new(1).1 };
    let mut got = Vec::new();
    for _ in 0..quanta {
        // (the streamer keeps ahead: a test's quanta come faster than a device's, so each waits for its frames)
        let ch = usize::from(s.channels);
        for _ in 0..2000 {
            let slot = &st.dsp.streamer.slots[0];
            if slot.ended() || slot.rx.slots() >= (QUANTUM + 2) * ch {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
        got.extend(st.run(1));
    }
    let first = got.iter().zip(&want).position(|(a, b)| a != b);
    assert_eq!(first, None, "first difference at {first:?} of {} frames", 2 * frames);
    assert_eq!(st.dsp.ended.as_slice(), &[(VoiceId(1), 2 * frames as u64)]);
}

// ---- the mixer thread allocates nothing ----

thread_local! {
    static COUNTING: Cell<bool> = const { Cell::new(false) };
    static ALLOCS: Cell<usize> = const { Cell::new(0) };
}

struct Counting;

// Safety: forwards to the system allocator; the counters are this thread's.
unsafe impl std::alloc::GlobalAlloc for Counting {
    unsafe fn alloc(&self, l: std::alloc::Layout) -> *mut u8 {
        if COUNTING.with(Cell::get) {
            ALLOCS.with(|a| a.set(a.get() + 1));
        }
        // Safety: the caller's layout, as given.
        unsafe { std::alloc::System.alloc(l) }
    }
    unsafe fn dealloc(&self, p: *mut u8, l: std::alloc::Layout) {
        if COUNTING.with(Cell::get) {
            ALLOCS.with(|a| a.set(a.get() + 1));
        }
        // Safety: allocated by `alloc` with this layout.
        unsafe { std::alloc::System.dealloc(p, l) }
    }
}

#[global_allocator]
static GLOBAL: Counting = Counting;

/// The allocations and frees `f` makes on this thread.
pub(crate) fn allocations<R>(f: impl FnOnce() -> R) -> (R, usize) {
    ALLOCS.with(|a| a.set(0));
    COUNTING.with(|c| c.set(true));
    let r = f();
    COUNTING.with(|c| c.set(false));
    (r, ALLOCS.with(Cell::get))
}

#[test]
fn renders_without_allocating() {
    let mut r = rig(medium(3000, Some([100, 2000])));
    let mut out = vec![0.0f32; QUANTUM * 2];
    let controls: Vec<Control> = (0..40u32)
        .flat_map(|k| {
            let mut s = spec(u64::from(k) * 64);
            s.voice = VoiceId(k);
            s.loops = if k % 2 == 0 { 0 } else { 2 };
            s.cents = f64::from(k) * 10.0;
            s.sends_n = 1;
            s.sends[0] = Send { bus: AUX, gain: 0.25 };
            if k % 3 == 0 {
                s.meters[0] = Some(mvs_data::sound::MeterParams {
                    game_param: 5,
                    attack: 0.01,
                    release: 0.1,
                    min: -48.0,
                    ..Default::default()
                });
            }
            let v = Target::Voice(VoiceId(k));
            [
                Control::Start(s),
                Control::Change(ParamChange {
                    at: 700,
                    target: v,
                    param: Param::Fade,
                    value: 0.0,
                    ramp: Ramp::Linear(0.01),
                }),
                Control::Change(ParamChange { at: 700, target: v, param: Param::Stop, value: 0.0, ramp: Ramp::Set }),
                Control::Change(ParamChange {
                    at: 300,
                    target: v,
                    param: Param::Pan { ch: 0, side: 1 },
                    value: 0.3,
                    ramp: Ramp::Target(0.002),
                }),
            ]
        })
        .collect();
    let (mut notes, _rx) = rtrb::RingBuffer::new(1024);
    COUNTING.with(|c| c.set(true));
    for c in &controls {
        r.dsp.apply(c, &mut notes);
    }
    for _ in 0..20 {
        r.dsp.render(&mut out, 2, &mut notes);
        r.dsp.ended.clear();
        r.dsp.meters.clear();
    }
    COUNTING.with(|c| c.set(false));
    assert_eq!(ALLOCS.with(Cell::get), 0);
    assert_eq!(r.dsp.voices(), 0);
}

#[test]
fn cancel_drops_waiting_changes() {
    // a fade to 0 at 300 and a cancel from 200: the voice plays on at its level
    let mut r = rig(medium(1000, None));
    r.apply(&[
        Control::Start(spec(0)),
        change(300, Param::Fade, 0.0, Ramp::Set),
        change(100, Param::Fade, 0.5, Ramp::Set),
        Control::Cancel(crate::spec::Cancel { from: 200, target: Target::Voice(VoiceId(1)), param: Param::Fade }),
    ]);
    let out = r.run(2);
    assert_eq!(out[99][0], 0.099);
    assert_eq!(out[100][0], 0.05);
    assert_eq!(out[400][0], 0.2);
}

#[test]
fn joined_ramp_runs_from_its_start() {
    // a line from 0 at sample 0 to 1 at 512, joined at 256: 0.5 there
    let mut r = rig(medium(1000, None));
    let joined = Ramp::Joined { from: 0.0, start: 0, secs: 512.0 / 48_000.0, curve: Curve::Linear };
    r.apply(&[Control::Start(spec(0)), change(256, Param::Fade, 1.0, joined)]);
    let out = r.run(3);
    assert_eq!(out[256][0], 0.256 * 0.5);
    assert_eq!(out[384][0], 0.384 * 0.75);
    assert_eq!(out[600][0], 0.6);
}

/// A medium of `id` (a frame of silence).
fn medium_of(id: Id) -> Arc<Pcm> {
    Arc::new(Pcm { id, rate: crate::RATE, channels: 1, frames: 1, loop_points: None, data: vec![0.0].into() })
}

fn music_scope(event: Id, ids: &[Id]) -> crate::Cmd {
    let media = ids.iter().map(|&i| medium_of(i)).collect();
    crate::Cmd::Music(crate::MusicCmd::Scope {
        event,
        values: Box::new([]),
        flow: None,
        media: Box::new(ScopeLoad { media, ..ScopeLoad::default() }),
    })
}

#[test]
fn a_music_scope_lets_the_last_ones_media_go() {
    let init = Arc::new(init());
    let (_tx, cmds) = rtrb::RingBuffer::new(4);
    let (notes, _rx) = rtrb::RingBuffer::new(1024);
    let rtpc = Arc::new(crate::RtpcMirror::new(&init));
    let mut m = super::Mixer::new(init, cmds, notes, rtpc, 2);
    m.take(music_scope(5, &[10, 11]));
    m.take(music_scope(5, &[11, 12]));
    // (no run of the music has them: what the second scope does not ask for goes at the quantum's end)
    m.let_go();
    assert!(m.dsp.media.get(10).is_none());
    assert_eq!(m.dsp.media.count(11), 1, "a medium asked for again is held once");
    assert!(m.dsp.media.get(12).is_some());
    // no values: no stage's music
    m.take(music_scope(5, &[]));
    m.let_go();
    assert!(m.dsp.media.get(11).is_none() && m.dsp.media.get(12).is_none());
}
