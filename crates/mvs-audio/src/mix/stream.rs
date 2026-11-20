//! Streamed voices: a thread of its own decodes their media ahead of them (lewton allocates per packet, the mixer
//! thread may not) into one ring per stream slot, the voice's source frames in order, its loops already laid out.
//! The mixer reads a slot's ring as the voice plays. Offline (the checks) the streamer runs inline instead, on the
//! mixer's thread at each open and after each quantum, so a render does not hang on a thread's timing.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::thread::JoinHandle;

use crate::decode::{Coded, Stream};
use crate::spec::MAX_CHANNELS;

/// Streamed voices that can play at once (every medium of a second or longer streams).
pub const MAX_STREAMS: usize = 64;
/// A slot's ring, samples (interleaved): about 0.34 s of stereo at the mixer's rate.
const RING: usize = 32_768;
/// Frames the streamer decodes in one go.
const CHUNK: usize = 1024;

/// What the mixer asks of the streamer through the ring (a close goes by the slot's own word: [`Shared::close`]).
enum Req {
    Open { slot: usize, epoch: u32, coded: Arc<Coded>, from: u32, loops: u16, loop_points: Option<[u32; 2]> },
}

/// What a slot's two sides share.
struct Shared {
    /// The generation the mixer asked to close: a word, not a request on the ring, so that no close is lost.
    close: AtomicU32,
    /// The last generation the streamer let go of.
    closed: AtomicU32,
    /// The streamer has written the stream's last frame.
    ended: AtomicBool,
}

/// The mixer's side of a slot.
pub(crate) struct Slot {
    pub(crate) rx: rtrb::Consumer<f32>,
    shared: Arc<Shared>,
    epoch: u32,
    busy: bool,
    pub(crate) channels: usize,
}

impl Slot {
    /// Whether the stream's last frame is in the ring.
    pub(crate) fn ended(&self) -> bool {
        self.shared.ended.load(Ordering::Acquire)
    }
}

/// The mixer's side of the streamer.
pub(crate) struct Streamer {
    reqs: rtrb::Producer<Req>,
    pub(crate) slots: Vec<Slot>,
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
    /// Offline: the streamer's side, run on the mixer's thread.
    inline: Option<Box<Inline>>,
}

/// The streamer's side when it runs inline.
struct Inline {
    reqs: rtrb::Consumer<Req>,
    sides: Vec<Side>,
    planar: Vec<Vec<f32>>,
}

impl Streamer {
    /// The streamer on a thread of its own.
    pub(crate) fn new() -> Streamer {
        Streamer::make(false)
    }

    /// The streamer run on the mixer's thread (offline: decoding allocates).
    pub(crate) fn inline() -> Streamer {
        Streamer::make(true)
    }

    fn make(inline: bool) -> Streamer {
        let (reqs, req_rx) = rtrb::RingBuffer::new(4 * MAX_STREAMS);
        let stop = Arc::new(AtomicBool::new(false));
        let mut slots = Vec::with_capacity(MAX_STREAMS);
        let mut sides = Vec::with_capacity(MAX_STREAMS);
        for _ in 0..MAX_STREAMS {
            let (tx, rx) = rtrb::RingBuffer::new(RING);
            let shared =
                Arc::new(Shared { close: AtomicU32::new(0), closed: AtomicU32::new(0), ended: AtomicBool::new(false) });
            slots.push(Slot { rx, shared: shared.clone(), epoch: 0, busy: false, channels: 0 });
            sides.push(Side { tx, shared, epoch: 0, open: None });
        }
        if inline {
            let inline = Inline { reqs: req_rx, sides, planar: vec![vec![0.0f32; CHUNK]; MAX_CHANNELS] };
            return Streamer { reqs, slots, stop, thread: None, inline: Some(Box::new(inline)) };
        }
        let s = stop.clone();
        let thread = std::thread::Builder::new()
            .name("audio-streamer".into())
            .spawn(move || run(req_rx, sides, &s))
            .expect("audio: the streamer thread starts");
        Streamer { reqs, slots, stop, thread: Some(thread), inline: None }
    }

    /// Opens a stream on a free slot: the voice's source from medium frame `from`, `loops` times (0: until closed)
    /// over its loop region. None when no slot is free.
    pub(crate) fn open(
        &mut self,
        coded: &Arc<Coded>,
        from: u32,
        loops: u16,
        loop_points: Option<[u32; 2]>,
    ) -> Option<usize> {
        let slot = self
            .slots
            .iter()
            .position(|s| !s.busy && s.shared.closed.load(Ordering::Acquire) == s.epoch && self.reqs.slots() > 0)?;
        let s = &mut self.slots[slot];
        // (what the closed stream wrote after the mixer stopped reading)
        while s.rx.pop().is_ok() {}
        s.epoch = s.epoch.wrapping_add(1);
        s.busy = true;
        s.channels = usize::from(coded.channels);
        s.shared.ended.store(false, Ordering::Release);
        let req = Req::Open { slot, epoch: s.epoch, coded: coded.clone(), from, loops, loop_points };
        let _ = self.reqs.push(req);
        self.wake();
        Some(slot)
    }

    pub(crate) fn close(&mut self, slot: usize) {
        let s = &mut self.slots[slot];
        if !s.busy {
            return;
        }
        s.busy = false;
        s.shared.close.store(s.epoch, Ordering::Release);
    }

    /// Wakes the streamer (once a quantum: the rings were read); inline, runs it.
    pub(crate) fn wake(&mut self) {
        if let Some(t) = &self.thread {
            t.thread().unpark();
        }
        if let Some(i) = &mut self.inline {
            step(&mut i.reqs, &mut i.sides, &mut i.planar);
        }
    }
}

impl Drop for Streamer {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        if let Some(t) = self.thread.take() {
            t.thread().unpark();
            let _ = t.join();
        }
    }
}

/// The streamer's side of a slot.
struct Side {
    tx: rtrb::Producer<f32>,
    shared: Arc<Shared>,
    /// The generation of the last open.
    epoch: u32,
    open: Option<Open>,
}

struct Open {
    stream: Stream,
    loops: u16,
    /// The loop region, end exclusive.
    region: [u32; 2],
    /// Frames written, and the most to write: `loops` medium lengths (a count of plays is a length of plays).
    written: u64,
    total: u64,
}

fn run(mut reqs: rtrb::Consumer<Req>, mut sides: Vec<Side>, stop: &AtomicBool) {
    let mut planar = vec![vec![0.0f32; CHUNK]; MAX_CHANNELS];
    while !stop.load(Ordering::Acquire) {
        step(&mut reqs, &mut sides, &mut planar);
        std::thread::park_timeout(std::time::Duration::from_millis(2));
    }
}

/// Takes the opens asked for and the closes, and fills the open streams' rings.
fn step(reqs: &mut rtrb::Consumer<Req>, sides: &mut [Side], planar: &mut [Vec<f32>]) {
    while let Ok(r) = reqs.pop() {
        match r {
            Req::Open { slot, epoch, coded, from, loops, loop_points } => {
                let id = coded.id;
                let side = &mut sides[slot];
                side.epoch = epoch;
                side.open = match Stream::open(coded, from) {
                    Ok(stream) => {
                        let frames = stream.frames;
                        // clamped to the medium, the whole medium where empty (as a decoded medium's)
                        // The loop count and the loop region are exact to the sample, as a decoded medium's.
                        let [a, b] = loop_points.unwrap_or([0, frames]);
                        let (a, b) = (a.min(frames), b.min(frames));
                        let region = if b > a { [a, b] } else { [0, frames] };
                        let total = if loops > 1 { u64::from(frames) * u64::from(loops) } else { u64::MAX };
                        Some(Open { stream, loops, region, written: 0, total })
                    }
                    Err(e) => {
                        eprintln!("audio: stream {id}: {e}");
                        side.shared.ended.store(true, Ordering::Release);
                        None
                    }
                };
            }
        }
    }
    for side in sides {
        // (a close of the generation opened: after its open, which the ring brought first)
        let close = side.shared.close.load(Ordering::Acquire);
        if close == side.epoch && side.shared.closed.load(Ordering::Acquire) != close {
            side.open = None;
            side.shared.closed.store(close, Ordering::Release);
        }
        fill(side, planar);
    }
}

/// Writes what fits of an open stream into its ring.
fn fill(side: &mut Side, planar: &mut [Vec<f32>]) {
    let Some(o) = &mut side.open else { return };
    let ch = o.stream.channels;
    loop {
        let room = side.tx.slots() / ch.max(1);
        if room == 0 || side.shared.ended.load(Ordering::Acquire) {
            return;
        }
        let looping = o.loops == 0 || (o.loops > 1 && o.written < o.total);
        let stop_at = if looping { o.region[1] } else { o.stream.frames };
        if o.stream.pos >= stop_at {
            if looping {
                o.stream.seek(o.region[0]);
                continue;
            }
            side.shared.ended.store(true, Ordering::Release);
            return;
        }
        let left_total = o.total.saturating_sub(o.written);
        let n = room.min(CHUNK).min((stop_at - o.stream.pos) as usize).min(left_total.min(CHUNK as u64) as usize);
        if n == 0 {
            side.shared.ended.store(true, Ordering::Release);
            return;
        }
        let got = {
            let mut outs: [&mut [f32]; MAX_CHANNELS] = Default::default();
            for (o, p) in outs.iter_mut().zip(planar.iter_mut()) {
                *o = &mut p[..n];
            }
            o.stream.read(&mut outs[..ch])
        };
        if got == 0 {
            side.shared.ended.store(true, Ordering::Release);
            return;
        }
        let Ok(chunk) = side.tx.write_chunk_uninit(got * ch) else { return };
        let planar = &*planar;
        chunk.fill_from_iter((0..got).flat_map(|f| planar[..ch].iter().map(move |p| p[f])));
        o.written += got as u64;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every slot opened and closed at once, then again: each close reaches the streamer (a word per slot, not the
    /// request ring) and the slots come back.
    #[test]
    fn closes_free_the_slots() {
        let Some((entry, setup)) = crate::decode::tests::oracle_entry(0) else {
            eprintln!("stream slots test: no oracle store, skipped");
            return;
        };
        let coded = Arc::new(Coded::new(1, entry.into_boxed_slice(), &setup).unwrap());
        let mut s = Streamer::new();
        for round in 0..3 {
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
            let mut opened = Vec::new();
            while opened.len() < MAX_STREAMS {
                match s.open(&coded, 0, 1, None) {
                    Some(k) => opened.push(k),
                    None => {
                        assert!(
                            std::time::Instant::now() < deadline,
                            "round {round}: {} slots came back",
                            opened.len()
                        );
                        s.wake();
                        std::thread::sleep(std::time::Duration::from_millis(1));
                    }
                }
            }
            for k in opened {
                s.close(k);
            }
            s.wake();
        }
    }
}
