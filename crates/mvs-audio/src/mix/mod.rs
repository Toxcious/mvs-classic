//! The mixer: the mixer thread's loop over quanta. Each quantum it takes the commands due, runs the interpreter
//! ([`crate::ctl::Ctl`]), applies its [`crate::spec::Control`]s to the voice pool ([`Dsp`]), renders the voices
//! (source, resampler, meters, low-pass / high-pass, gain, pan) into their buses, runs the bus tree bottom-up (HDR
//! gain, the effects, volume) and writes the top buses to the output. What the DSP says back (a voice ran out, a
//! meter's value) goes to the interpreter.
//!
//! Nothing on the mixer thread allocates: the pool, the buses, the change queue and the scratch are made up front,
//! what a command carried goes back to the game thread as a [`Note::Free`], and streamed media decode on a thread of
//! their own (`stream`).

pub(crate) mod auto;
pub(crate) mod bus;
pub(crate) mod media;
pub(crate) mod stream;
pub(crate) mod voice;

use std::sync::Arc;

use mvs_data::sound::{Id, InitBank};

use self::auto::Auto;
use self::bus::{BUS_CHANNELS, Buses};
use self::media::{Held, Media, music_owner};
use self::stream::Streamer;
use self::voice::{Scratch, Source, Voice};
use crate::ctl::{Ctl, Out, Rng};
use crate::spec::{Control, Param, ParamChange, VoiceId, VoiceSpec};
use crate::{Cmd, MusicCmd, Note, QUANTUM, RtpcMirror, Stamped};

/// A quantum of audio: `channels` planar channels of [`QUANTUM`] samples. A bus's buffer and an effect's input.
#[derive(Clone, Debug)]
pub struct Block {
    pub ch: Box<[[f32; QUANTUM]]>,
}

impl Block {
    pub fn new(channels: usize) -> Block {
        Block { ch: vec![[0.0; QUANTUM]; channels].into_boxed_slice() }
    }
    pub fn channels(&self) -> usize {
        self.ch.len()
    }
    pub fn clear(&mut self) {
        for c in self.ch.iter_mut() {
            c.fill(0.0);
        }
    }
}

/// The most voices that play at once (the pool's size).
pub const MAX_VOICES: usize = 512;
/// The most parameter changes waiting for their time.
pub const MAX_PENDING: usize = 16_384;
/// The most controls the interpreter emits in a quantum before the vector would grow.
pub const MAX_CONTROLS: usize = 8_192;

/// The most music media a release lets go of once the music no longer plays them (its list's capacity).
const MAX_RELEASED: usize = 4_096;
/// The most media the music scope in use holds (its list's capacity; past it a medium is not let go by the next scope).
const MAX_MUSIC_SCOPE: usize = 4_096;
/// The most voices reported ended in a quantum: the pool's run-outs and the starts that failed.
const MAX_ENDED: usize = MAX_VOICES + MAX_CONTROLS;

/// The DSP side of the mixer: the voice pool, the bus tree and the media it plays. It takes [`Control`]s and renders
/// quanta; the checks drive it offline with a control list.
pub struct Dsp {
    /// The audio clock, samples.
    pub(crate) now: u64,
    slots: Vec<Voice>,
    /// Slots playing, in start order.
    active: Vec<usize>,
    free: Vec<usize>,
    buses: Buses,
    /// Changes not due yet, by time (stable).
    pending: Vec<ParamChange>,
    due: Vec<ParamChange>,
    pub(crate) media: Media,
    streamer: Streamer,
    sc: Box<Scratch>,
    out: Block,
    loose: Block,
    /// The port's output volume (linear), after the top buses.
    master: Auto,
    gain: [f32; QUANTUM],
    bus_meters: Vec<(Id, f64)>,
    /// Since the last quantum: voices that ran out (and when), meters' values (game parameter, value, when).
    pub(crate) ended: Vec<(VoiceId, u64)>,
    pub(crate) meters: Vec<(Id, f64, u64)>,
}

impl Dsp {
    pub fn new(init: &InitBank) -> Dsp {
        Dsp {
            now: 0,
            slots: (0..MAX_VOICES).map(|_| Voice::empty()).collect(),
            active: Vec::with_capacity(MAX_VOICES),
            free: (0..MAX_VOICES).rev().collect(),
            buses: Buses::new(init),
            pending: Vec::with_capacity(MAX_PENDING),
            due: Vec::with_capacity(MAX_PENDING),
            media: Media::new(),
            streamer: Streamer::new(),
            sc: Box::new(Scratch::new()),
            out: Block::new(BUS_CHANNELS),
            loose: Block::new(BUS_CHANNELS),
            master: Auto::new(1.0),
            gain: [0.0; QUANTUM],
            bus_meters: Vec::with_capacity(256),
            ended: Vec::with_capacity(MAX_ENDED),
            meters: Vec::with_capacity(4 * MAX_VOICES),
        }
    }

    /// The audio clock, samples.
    pub fn now(&self) -> u64 {
        self.now
    }

    /// Holds a decoded medium for the voices (`owner`: what lets it go, [`Dsp::release`]).
    pub fn hold(&mut self, owner: u64, load: &crate::ctl::scope::ScopeLoad, notes: &mut rtrb::Producer<Note>) {
        self.media.add(owner, load, notes);
    }

    /// Lets go what was held under `owner`.
    pub fn release(&mut self, owner: u64, notes: &mut rtrb::Producer<Note>) {
        self.media.remove(owner, notes);
    }

    /// Sets the output volume (linear) over a frame.
    pub fn set_volume(&mut self, v: f64) {
        self.master.change(self.now, v, crate::spec::Ramp::Linear(crate::ctl::curves::FRAME_SECS));
    }

    /// Takes a control.
    pub fn apply(&mut self, c: &Control, notes: &mut rtrb::Producer<Note>) {
        match c {
            Control::Start(s) => self.start(s, notes),
            Control::Change(ch) => {
                if self.pending.len() == self.pending.capacity() {
                    let _ = notes.push(Note::Warn { what: "parameter changes overflow", id: ch.at });
                    // (a stop is never lost: a value change makes room for it)
                    if ch.param != Param::Stop || !self.make_room(ch) {
                        return;
                    }
                }
                let at = self.pending.partition_point(|p| p.at <= ch.at);
                self.pending.insert(at, *ch);
            }
            Control::Cancel(c) => {
                self.pending.retain(|p| !(p.target == c.target && p.param == c.param && p.at >= c.from));
            }
        }
    }

    /// The change list is full and a stop comes: drops a value change for it, one of the stopped voice's due at or
    /// after the stop (moot: the voice is gone by then), else the last value change of all. False when every change
    /// waiting is a stop.
    fn make_room(&mut self, stop: &ParamChange) -> bool {
        let moot =
            self.pending.iter().rposition(|p| p.target == stop.target && p.param != Param::Stop && p.at >= stop.at);
        match moot.or_else(|| self.pending.iter().rposition(|p| p.param != Param::Stop)) {
            Some(i) => {
                self.pending.remove(i);
                true
            }
            None => false,
        }
    }

    fn start(&mut self, s: &VoiceSpec, notes: &mut rtrb::Producer<Note>) {
        let Some(slot) = self.free.pop() else {
            let _ = notes.push(Note::Warn { what: "voice pool full", id: u64::from(s.voice.0) });
            self.never_started(s, notes);
            return;
        };
        let (src, rate, channels, frames) = match self.media.get(s.medium) {
            Some(Held::Pcm(p)) => (Source::Mem(p.clone()), p.rate, usize::from(p.channels), p.frames),
            Some(Held::Stream(c)) => {
                let ch = usize::from(c.channels);
                let opened = (ch > 0).then(|| self.streamer.open(c, s.offset, s.loops, s.loop_points)).flatten();
                match opened {
                    Some(k) => (Source::Stream(k), c.rate, ch, c.frames),
                    None => {
                        let _ = notes.push(Note::Warn { what: "no stream slot free", id: u64::from(s.medium) });
                        (Source::None, 0, 0, 0)
                    }
                }
            }
            None => {
                let _ = notes.push(Note::Warn { what: "medium not loaded", id: u64::from(s.medium) });
                (Source::None, 0, 0, 0)
            }
        };
        if matches!(src, Source::None) {
            self.free.push(slot);
            self.never_started(s, notes);
            return;
        }
        self.slots[slot].start(s, src, rate, channels, frames, &self.buses);
        self.active.push(slot);
    }

    /// Reports a voice that could not start as ended at its start (within the list's capacity).
    fn never_started(&mut self, s: &VoiceSpec, notes: &mut rtrb::Producer<Note>) {
        if self.ended.len() < self.ended.capacity() {
            self.ended.push((s.voice, s.at.max(self.now)));
        } else {
            let _ = notes.push(Note::Warn { what: "ended voices overflow", id: u64::from(s.voice.0) });
        }
    }

    /// Mixes one quantum into `out` (interleaved, `channels` per frame, [`QUANTUM`] frames).
    pub fn render(&mut self, out: &mut [f32], channels: usize, notes: &mut rtrb::Producer<Note>) {
        let now = self.now;
        let end = now + QUANTUM as u64;
        let k = self.pending.partition_point(|p| p.at < end);
        self.due.clear();
        self.due.extend(self.pending.drain(..k));
        self.buses.take(now, &self.due);
        self.buses.clear();
        self.loose.clear();
        self.out.clear();
        for &i in &self.active {
            self.slots[i].render(
                now,
                &self.due,
                &mut self.buses,
                &mut self.streamer,
                &mut self.sc,
                &mut self.loose,
                &mut self.meters,
            );
        }
        self.bus_meters.clear();
        self.buses.run(now, &self.due, &mut self.out, &mut self.bus_meters);
        for &(gp, v) in &self.bus_meters {
            if self.meters.len() < self.meters.capacity() {
                self.meters.push((gp, v, now));
            }
        }
        bus::add(&mut self.out, &self.loose);
        let fixed = self.master.fill(
            now,
            &[],
            crate::spec::Target::Bus(0),
            crate::spec::Param::BusVolume,
            |v| v,
            &mut self.gain,
        );
        write(&self.out, fixed, &self.gain, out, channels);
        self.retire(notes);
        self.streamer.wake();
        self.now = end;
    }

    /// Frees the slots of the voices that ended in the last quantum.
    fn retire(&mut self, notes: &mut rtrb::Producer<Note>) {
        let mut k = 0;
        while k < self.active.len() {
            let i = self.active[k];
            let v = &mut self.slots[i];
            let Some((at, ran_out)) = v.ended else {
                k += 1;
                continue;
            };
            if ran_out && self.ended.len() < self.ended.capacity() {
                self.ended.push((v.spec.voice, at));
            }
            // a meter that ends counts once more at its minimum
            for (m, p) in v.meters.iter_mut().zip(&v.spec.meters) {
                if m.take().is_some()
                    && let Some(p) = p
                    && self.meters.len() < self.meters.capacity()
                {
                    self.meters.push((p.game_param, p.min, at));
                }
            }
            match std::mem::replace(&mut v.src, Source::None) {
                Source::Mem(p) => {
                    if !self.media.holds(&p) {
                        media::free(Held::Pcm(p), notes);
                    }
                }
                Source::Stream(slot) => self.streamer.close(slot),
                Source::None => {}
            }
            self.active.remove(k);
            self.free.push(i);
        }
    }

    /// Voices playing.
    pub fn voices(&self) -> usize {
        self.active.len()
    }
}

/// Writes the stereo mix to the output's channels: left and right to the first two, silence on the rest; a mono
/// output takes their mean.
// Unverified: a device with more than two channels gets the front pair only.
fn write(mix: &Block, fixed: Option<f32>, gain: &[f32; QUANTUM], out: &mut [f32], channels: usize) {
    let (l, r) = (&mix.ch[0], &mix.ch[1]);
    for i in 0..QUANTUM {
        let g = fixed.unwrap_or(gain[i]);
        let f = &mut out[i * channels..(i + 1) * channels];
        if channels == 1 {
            f[0] = (l[i] + r[i]) * 0.5 * g;
            continue;
        }
        f[0] = l[i] * g;
        f[1] = r[i] * g;
        f[2..].fill(0.0);
    }
}

/// Whether a load carries medium `m`.
fn asks(load: &crate::ctl::scope::ScopeLoad, m: Id) -> bool {
    load.media.iter().any(|p| p.id == m) || load.streamed.iter().any(|s| s.id == m)
}

/// The mixer thread's state.
pub struct Mixer {
    pub(crate) cmds: rtrb::Consumer<Stamped>,
    pub(crate) notes: rtrb::Producer<Note>,
    pub(crate) ctl: Ctl,
    pub(crate) dsp: Dsp,
    /// Output channels.
    pub(crate) channels: usize,
    /// The audio clock, samples.
    pub(crate) now: u64,
    /// The last sim tick whose commands a quantum takes (offline: the pumped tick; live: all).
    pub(crate) until: u32,
    controls: Vec<Control>,
    /// Music media released while a run of the music had them (owner, medium): let go once none has.
    released: Vec<(u64, Id)>,
    /// The media the last music scope asked for (owner, medium).
    music_scope: Vec<(u64, Id)>,
}

impl Mixer {
    /// Runs the streamer on the mixer's thread (offline: a render then hangs on no thread's timing).
    pub fn inline_streams(&mut self) {
        self.dsp.streamer = Streamer::inline();
    }

    pub fn new(
        init: Arc<InitBank>,
        cmds: rtrb::Consumer<Stamped>,
        notes: rtrb::Producer<Note>,
        rtpc: Arc<RtpcMirror>,
        channels: usize,
    ) -> Mixer {
        // the random draw has no seed: a run draws from the clock
        let seed =
            std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_nanos() as u64);
        let ctl = Ctl::new(init.clone(), rtpc.clone(), Rng::Seeded(seed));
        let dsp = Dsp::new(&init);
        Mixer {
            cmds,
            notes,
            ctl,
            dsp,
            channels,
            now: 0,
            until: u32::MAX,
            controls: Vec::with_capacity(MAX_CONTROLS),
            released: Vec::with_capacity(MAX_RELEASED),
            music_scope: Vec::with_capacity(MAX_MUSIC_SCOPE),
        }
    }

    /// Takes a command: the media a scope, the music or a lazy event's post carries are held for the voices before
    /// the interpreter has it; the output volume is the mixer's own.
    fn take(&mut self, cmd: Cmd) {
        let now = self.now;
        match &cmd {
            Cmd::SetVolume(v) => {
                self.dsp.set_volume(*v);
                return;
            }
            Cmd::Media { owner, media } => self.dsp.media.add_pcms(*owner, media, &mut self.notes),
            // (a scope set again replaces its media)
            Cmd::Scope { name, load } => {
                self.dsp.release(*name, &mut self.notes);
                self.dsp.hold(*name, load, &mut self.notes);
            }
            Cmd::Unscope { name } => self.dsp.release(*name, &mut self.notes),
            Cmd::Music(MusicCmd::Preload { event, media, .. }) => {
                let owner = music_owner(*event);
                // (asked for again: kept)
                self.released.retain(|&(o, m)| o != owner || !asks(media, m));
                self.dsp.hold(owner, media, &mut self.notes);
            }
            // (the music scope becomes these media; what the last scope asked for and this one
            // does not goes at once, or once the music has played it out)
            Cmd::Music(MusicCmd::Scope { event, media, .. }) => {
                let owner = music_owner(*event);
                for k in 0..self.music_scope.len() {
                    let (o, m) = self.music_scope[k];
                    if !(o == owner && asks(media, m)) {
                        self.release_music(o, m);
                    }
                }
                self.music_scope.clear();
                for m in media.media.iter().map(|p| p.id).chain(media.streamed.iter().map(|s| s.id)) {
                    if self.music_scope.len() < MAX_MUSIC_SCOPE {
                        self.music_scope.push((owner, m));
                    }
                }
                self.released.retain(|&(o, m)| o != owner || !asks(media, m));
                self.dsp.hold(owner, media, &mut self.notes);
            }
            // (what the values reach goes at once, or once the music has played it out)
            Cmd::Music(MusicCmd::Release { event, media, .. }) => {
                let owner = music_owner(*event);
                for &m in media {
                    self.release_music(owner, m);
                }
            }
            _ => {}
        }
        let mut out = Out { controls: &mut self.controls, notes: &mut self.notes };
        self.ctl.apply(cmd, now, &mut out);
    }

    /// Releases a music medium held under `owner`: let go once no run of the music has it (at once past the list's
    /// capacity).
    fn release_music(&mut self, owner: u64, m: Id) {
        if self.released.len() < MAX_RELEASED {
            self.released.push((owner, m));
        } else {
            self.dsp.media.remove_one(owner, m, &mut self.notes);
        }
    }

    /// Lets the released music media go that no run of the music has (after the quantum's voices took theirs).
    fn let_go(&mut self) {
        let Mixer { released, ctl, dsp, notes, .. } = self;
        released.retain(|&(owner, m)| {
            let kept = crate::ctl::music::uses(ctl, m);
            if !kept {
                dsp.media.remove_one(owner, m, notes);
            }
            kept
        });
    }

    fn flush(&mut self) {
        for c in &self.controls {
            self.dsp.apply(c, &mut self.notes);
        }
        self.controls.clear();
    }

    /// Mixes the next quanta into `out` (interleaved, the output's channels; a multiple of [`QUANTUM`] frames).
    pub fn render(&mut self, out: &mut [f32]) {
        let ch = self.channels;
        for q in out.chunks_exact_mut(QUANTUM * ch) {
            while let Ok(s) = self.cmds.peek() {
                if s.tick > self.until {
                    break;
                }
                let Ok(s) = self.cmds.pop() else { break };
                self.take(s.cmd);
            }
            let mut o = Out { controls: &mut self.controls, notes: &mut self.notes };
            self.ctl.quantum(self.now, &mut o);
            self.flush();
            self.dsp.render(q, ch, &mut self.notes);
            for k in 0..self.dsp.ended.len() {
                let (v, at) = self.dsp.ended[k];
                let mut o = Out { controls: &mut self.controls, notes: &mut self.notes };
                self.ctl.ended(v, at, &mut o);
            }
            self.dsp.ended.clear();
            for k in 0..self.dsp.meters.len() {
                let (gp, v, at) = self.dsp.meters[k];
                let mut o = Out { controls: &mut self.controls, notes: &mut self.notes };
                self.ctl.meter(gp, v, at, &mut o);
            }
            self.dsp.meters.clear();
            self.flush();
            self.let_go();
            self.now = self.dsp.now;
        }
    }
}

#[cfg(test)]
pub(crate) mod tests;

#[cfg(test)]
mod level_tests;
