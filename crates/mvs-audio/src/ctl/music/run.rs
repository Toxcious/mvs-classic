//! One activation of a music playlist (CAkMusicRanSeqCntr's sequence context): it chains its segments entry cue on
//! exit cue, starts the voices of their clips and keeps the fade its transitions move.

use mvs_data::sound::{AutoType, Curve, Id, MusicItem, Obj, Pt, RuleDst, RuleSrc, SyncType};

use super::iter::{Iter, StepInfo};
use super::{Shared, World, meter_of, parent_of};
use crate::ctl::curves::{FRAME_SECS, interp};
use crate::ctl::samples;
use crate::mix::auto::Auto;
use crate::spec::{Cancel, Control, MAX_SENDS, Param, ParamChange, Ramp, Send, Target, VoiceId, VoiceSpec};

/// s scheduled ahead.
const HORIZON: f64 = 4.0;
/// s before its start at which a playlist's next segment is picked.
const PREFETCH: f64 = 20.0;
/// The sound engine's sample rate (init settings), in which it keeps the beat length.
const RATE: f64 = 48_000.0;

/// A segment as the playlist keeps it: its cues and length (ms).
#[derive(Clone, Copy, Debug)]
pub(super) struct SegInfo {
    pub dur: f64,
    pub entry: f64,
    pub exit: f64,
}

pub(super) fn seg_info(w: &dyn World, id: Id) -> Option<SegInfo> {
    match w.obj(id) {
        Some(Obj::MusicSegment(s)) => Some(SegInfo { dur: s.dur, entry: s.entry, exit: s.exit }),
        _ => None,
    }
}

/// The longest pre-entry of the segments under a playlist's root items (s).
pub(super) fn pre_entry(w: &dyn World, roots: &[MusicItem]) -> f64 {
    fn walk(n: &MusicItem, w: &dyn World, pre: &mut f64) {
        if n.seg != 0 {
            *pre = pre.max(seg_info(w, n.seg).map_or(0.0, |s| s.entry) / 1000.0);
        }
        for c in &n.children {
            walk(c, w, pre);
        }
    }
    let mut pre = 0.0f64;
    for r in roots {
        walk(r, w, &mut pre);
    }
    pre
}

/// A clip's voice: its number and the second it ends at (the stop asked for, or the medium's end).
#[derive(Clone, Copy, Debug)]
struct Vox {
    id: VoiceId,
    end: f64,
}

/// A segment of the playlist playing or waiting: its position 0 at `t0` (s), the time it syncs to, its voices.
struct Played {
    seg: Id,
    o: SegInfo,
    t0: f64,
    sync: f64,
    vox: Vec<Vox>,
}

/// A step of a parameter's program (a Web Audio AudioParam event): at `at` (s) the parameter takes `value` by
/// `ramp`.
#[derive(Clone, Copy, Debug)]
pub(super) struct Step {
    at: f64,
    value: f64,
    ramp: Ramp,
}

impl Step {
    fn set(at: f64, value: f64) -> Step {
        Step { at, value, ramp: Ramp::Set }
    }
}

#[derive(Clone, Copy)]
struct FadeIn {
    t0: f64,
    dur: f64,
    curve: Curve,
}

/// A voice parameter's change as a control.
pub(super) fn change(voice: VoiceId, at: f64, param: Param, value: f64, ramp: Ramp) -> Control {
    Control::Change(ParamChange { at: samples(at), target: Target::Voice(voice), param, value, ramp })
}

/// Gives a voice's parameter a program part of which may lie before `now`, as a Web Audio AudioParam has it on its
/// timeline: the steps before `now` are run here and what they leave goes as one change at `now` (the value, or a
/// ramp under way joined part-way); the steps from `now` on go as they are.
struct Join {
    voice: VoiceId,
    param: Param,
    sim: Auto,
    now: u64,
    folded: bool,
    done: bool,
}

impl Join {
    /// `init`: the parameter's value before the program.
    fn new(voice: VoiceId, param: Param, init: f64, now: f64) -> Join {
        Join { voice, param, sim: Auto::new(init), now: samples(now), folded: false, done: false }
    }

    fn push(&mut self, w: &mut dyn World, s: &Step) {
        let at = samples(s.at);
        if !self.done && at < self.now {
            self.sim.change(at, s.value, s.ramp);
            self.folded = true;
            return;
        }
        self.flush(w);
        w.emit(change(self.voice, s.at, self.param, s.value, s.ramp));
    }

    fn flush(&mut self, w: &mut dyn World) {
        if std::mem::replace(&mut self.done, true) || !self.folded {
            return;
        }
        let target = Target::Voice(self.voice);
        let (v, ramp) = self.sim.onward(self.now);
        let mut put = |value, ramp| {
            w.emit(Control::Change(ParamChange { at: self.now, target, param: self.param, value, ramp }));
        };
        match ramp {
            Some((to, r @ Ramp::Joined { .. })) => put(to, r),
            Some((to, r)) => {
                put(v, Ramp::Set);
                put(to, r);
            }
            None => put(v, Ramp::Set),
        }
    }
}

pub(super) struct Run {
    /// The playlist's container.
    pub id: Id,
    items: Vec<Played>,
    seq: Iter,
    /// The playlist's own first rule (the one the chaining reads): its sides.
    rule: Option<(RuleSrc, RuleDst)>,
    /// No segment syncs at or after this.
    end_at: f64,
    kill_at: f64,
    fade_in: Option<FadeIn>,
    ended: bool,
    /// The next segment, once picked (Some(None): none).
    ahead: Option<Option<Id>>,
    /// The longest pre-entry a chained segment starts with (s).
    pre: f64,
    pub dead: bool,
    /// A transition segment's activation.
    pub via: bool,
    /// What the fade did and will do, to give a voice started later the same.
    prog: Vec<Step>,
    /// The voice lists of the segments that went, for the next ones.
    spare: Vec<Vec<Vox>>,
}

impl Run {
    /// A run of the playlist whose first root item is `list`, with its first rule's sides; `pre`: its segments'
    /// longest pre-entry ([`pre_entry`]) when its rule plays the pre-entry.
    pub(super) fn new(id: Id, list: Option<MusicItem>, rule: Option<(RuleSrc, RuleDst)>, pre: f64) -> Run {
        Run {
            id,
            items: Vec::new(),
            seq: Iter::new(list),
            rule,
            end_at: f64::INFINITY,
            kill_at: f64::INFINITY,
            fade_in: None,
            ended: false,
            ahead: None,
            pre,
            dead: false,
            via: false,
            prog: Vec::new(),
            spare: Vec::new(),
        }
    }

    /// The first segment the playlist would play (resume: the one it was left in).
    pub(super) fn first(&mut self, step: &StepInfo, resume: Option<Id>, w: &mut dyn World) -> Option<Id> {
        for _ in 0..64 {
            let r = self.seq.next(step, w)?;
            if resume.is_none_or(|x| x == r) {
                return Some(r);
            }
        }
        None
    }

    /// Whether the run has a medium: a clip of its segments' tracks or of the segment picked next:
    /// what its segments asked of the host and it has not let go.
    pub(super) fn holds<'a>(&self, obj: &dyn Fn(Id) -> Option<&'a Obj>, m: Id) -> bool {
        let next = self.ahead.flatten();
        !self.dead && self.items.iter().map(|i| i.seg).chain(next).any(|seg| seg_has(obj, seg, m))
    }

    /// The voices of the run that have not ended by `now`.
    pub(super) fn live(&self, now: f64) -> impl Iterator<Item = VoiceId> + '_ {
        self.items.iter().flat_map(|i| i.vox.iter()).filter(move |v| v.end > now).map(|v| v.id)
    }

    /// Plays a segment with its position 0 at `t0` (s), from position `from` (ms): its tracks' clips, each a voice from
    /// its offset to the end of the clip. `sync` is the time the segment's entry cue syncs to.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn start(
        &mut self,
        w: &mut dyn World,
        sh: &mut Shared,
        now: f64,
        seg: Id,
        t0: f64,
        from: f64,
        sync: f64,
    ) {
        let Some(Obj::MusicSegment(o)) = w.obj(seg) else {
            return w.warn("music segment missing", u64::from(seg));
        };
        let info = SegInfo { dur: o.dur, entry: o.entry, exit: o.exit };
        let tracks = o.children.len();
        let vox = self.spare.pop().unwrap_or_default();
        self.items.push(Played { seg, o: info, t0, sync, vox });
        self.seq.played(&mut sh.step);
        w.line(t0, seg, "segment");
        let at = self.items.len() - 1;
        for k in 0..tracks {
            let Some(Obj::MusicSegment(o)) = w.obj(seg) else { break };
            let tid = o.children[k];
            let Some(Obj::MusicTrack(tr)) = w.obj(tid) else { continue };
            for ci in 0..tr.clips.len() {
                self.start_clip(w, sh, now, at, tid, ci, t0, from);
            }
        }
    }

    // A clip lasts source duration + end trim - begin trim from PlayAt + begin trim (CAkMusicTrack::AddPlaylistItem
    // 180011d80, CAkSubTrackCtx::ScheduleSequencerCmds 18000a5e0); its source starts at (position in the clip + begin
    // trim) modulo the source duration and loops per the track's Loop property.
    #[allow(clippy::too_many_arguments)]
    fn start_clip(
        &mut self,
        w: &mut dyn World,
        sh: &mut Shared,
        now: f64,
        at: usize,
        tid: Id,
        ci: usize,
        t0: f64,
        from: f64,
    ) {
        let Some(Obj::MusicTrack(tr)) = w.obj(tid) else { return };
        let c = tr.clips[ci];
        let a = c.play_at + c.begin_trim;
        let b = c.play_at + c.src_dur + c.end_trim;
        if b <= from {
            return;
        }
        let s = from.max(a);
        let when = t0 + s / 1000.0;
        let offset = (c.begin_trim + s - a) / 1000.0;
        let dur = (b - s) / 1000.0;
        // Voice volume: Volume (0) and MakeUpGain (6) up the hierarchy, and the loudness normalisation gain when the
        // nearest node overriding the analysis flags (else the top node) asks for it.
        let p = &tr.node.p;
        let (mut vol, mut makeup) = (p.get(0).unwrap_or(0.0), p.get(6).unwrap_or(0.0));
        let loops = p.get(58).unwrap_or(1.0);
        let mut flags = None;
        let mut top = None;
        let mut n = Some(tid);
        for d in 0..32 {
            let Some(id) = n else { break };
            let Some(node) = w.obj(id).and_then(Obj::node) else { break };
            if d > 0 {
                vol += node.p.get(0).unwrap_or(0.0);
                makeup += node.p.get(6).unwrap_or(0.0);
            }
            if flags.is_none() && node.hdr & 2 != 0 {
                flags = Some(node.hdr);
            }
            top = Some(node.hdr);
            n = node.parent;
        }
        // Automation (CAkMusicPBI::SetAutomationValue 180004b20): Volume is a linear multiplier of 1 + value, FadeIn /
        // FadeOut a multiplier of the value itself; point times are seconds from the clip's audible start.
        // The two automations a clip can have drive two voice multipliers the music has no other use for, the
        // distance gain (the music is not positioned) and the mute gain.
        let x0 = (s - a) / 1000.0;
        let mut slots: [Option<(usize, Param, f64, f64)>; 2] = [None; 2];
        let (mut odd_kind, mut too_many) = (false, false);
        for (k, au) in tr.auto.iter().enumerate().filter(|(_, x)| x.clip as usize == ci && !x.pts.is_empty()) {
            let add = match au.kind {
                AutoType::Volume => 1.0,
                AutoType::FadeIn | AutoType::FadeOut => 0.0,
                _ => {
                    odd_kind = true;
                    continue;
                }
            };
            let Some(free) = slots.iter().position(Option::is_none) else {
                too_many = true;
                continue;
            };
            let param = if free == 0 { Param::Dist } else { Param::Mute };
            slots[free] = Some((k, param, add, auto_start(&au.pts, x0) + add));
        }
        let media = w.media(c.src).map(|m| {
            let norm = if flags.or(top).unwrap_or(0) & 4 != 0 { m.norm.unwrap_or(0.0) } else { 0.0 };
            (m.rate, m.frames, downmix(m.channels, m.mask), norm)
        });
        if odd_kind || too_many {
            w.warn("music clip automation not ported", u64::from(c.src));
        }
        let Some((rate, m_frames, mix, norm)) = media else {
            return w.warn("music medium missing", u64::from(c.src));
        };
        let Some((channels, pan)) = mix else {
            return w.warn("music channel layout not ported", u64::from(c.src));
        };
        let late = (now - when).max(0.0);
        let until = (when + dur).min(self.kill_at);
        if when + late >= until {
            return;
        }
        // The track loops per its Loop property (CAkPBI::_InitPlay 18002be90 takes CAkSoundBase::Loop 18003d8a0: prop
        // 58, once when absent, 0 = endless) and the clip's duration ends it. The bank sets 0 on exactly the five
        // tracks whose clip reaches past its source (they start up to 1.78 s before it does); their media have no loop
        // points, so the whole file loops.
        let rate = f64::from(rate.max(1));
        let len = (c.src_dur / 1000.0).min(f64::from(m_frames) / rate);
        if len <= 0.0 {
            return w.warn("music medium empty", u64::from(c.src));
        }
        if loops != 0.0 && loops != 1.0 {
            w.warn("music track loop count not ported", u64::from(tid));
        }
        let looped = loops == 0.0;
        let start = (((offset + late) % len) + len) % len;
        let frames = |secs: f64| (secs * rate).round().clamp(0.0, f64::from(m_frames)) as u32;
        let voice = w.voice();
        let mut sends = [Send::default(); MAX_SENDS];
        for (slot, &(bus, gain)) in sends.iter_mut().zip(&sh.sends) {
            *slot = Send { bus, gain };
        }
        let spec = VoiceSpec {
            voice,
            at: samples(when + late),
            medium: c.src,
            offset: frames(start),
            loops: if looped { 0 } else { 1 },
            loop_points: looped.then(|| [0, frames(len)]),
            bus: sh.bus,
            vol,
            makeup,
            norm,
            rtpc_gain: sh.gain,
            channels,
            pan,
            dist: slots[0].map_or(1.0, |x| x.3),
            sends_n: sh.sends.len().min(MAX_SENDS) as u8,
            sends,
            streamed: true,
            ..VoiceSpec::default()
        };
        w.emit(Control::Start(spec));
        // Each automation is one timeline from the voice's audible start in the clip, as a Web Audio gain node has
        // it: a voice that starts late joins it where it is.
        for (k, param, add, v0) in slots.into_iter().flatten() {
            let Some(Obj::MusicTrack(tr)) = w.obj(tid) else { break };
            sh.pts.clear();
            sh.pts.extend_from_slice(&tr.auto[k].pts);
            sh.steps.clear();
            if param == Param::Mute {
                // (the mute gain starts at 1: the voice's distance gain starts at the value in its spec)
                sh.steps.push(Step::set(when, v0));
            }
            auto_steps(&sh.pts, add, x0, t0 + a / 1000.0, when, &mut sh.steps);
            let mut j = Join::new(voice, param, v0, now);
            for s in &sh.steps {
                j.push(w, s);
            }
            j.flush(w);
        }
        self.replay_fade(w, voice, now);
        w.emit(change(voice, until, Param::Stop, 0.0, Ramp::Set));
        self.items[at].vox.push(Vox { id: voice, end: until });
    }

    /// Gives a voice started now the fade the run's voices have, where it is now.
    fn replay_fade(&self, w: &mut dyn World, voice: VoiceId, now: f64) {
        // (the run's shared fade gain starts at 1)
        let mut j = Join::new(voice, Param::Fade, 1.0, now);
        for s in &self.prog {
            j.push(w, s);
        }
        j.flush(w);
    }

    /// Chains the next segments onto the exit cue of the last one; a segment that has played out goes.
    pub(super) fn tick(&mut self, w: &mut dyn World, sh: &mut Shared, now: f64) {
        while self.items.len() > 1 && now > self.items[0].t0 + self.items[0].o.dur / 1000.0 + 0.5 {
            let mut i = self.items.remove(0);
            cut(w, now, &mut i, 0.0);
            self.recycle(i);
        }
        while let Some(last) = self.items.last() {
            let lead = last.t0 + last.o.exit / 1000.0 - self.pre - now;
            if lead <= PREFETCH && self.ahead.is_none() && !self.ended && self.end_at == f64::INFINITY {
                self.ahead = Some(self.seq.next(&sh.step, w));
            }
            if lead > HORIZON || !self.chain(w, sh, now) {
                break;
            }
        }
        let last_end = self.items.iter().map(|i| i.t0 + i.o.dur / 1000.0).fold(0.0, f64::max);
        if (self.end_at < f64::INFINITY || self.ended) && now > self.kill_at.min(last_end) + 0.2 {
            self.cut_all(w, now, 0.0);
            self.drop_run(w, now);
        }
    }

    fn recycle(&mut self, mut i: Played) {
        i.vox.clear();
        self.spare.push(i.vox);
    }

    /// The playlist's next segment, its entry cue on the last one's exit cue.
    fn chain(&mut self, w: &mut dyn World, sh: &mut Shared, now: f64) -> bool {
        let Some(last) = self.items.last() else { return false };
        if self.ended {
            return false;
        }
        let t_exit = last.t0 + last.o.exit / 1000.0;
        if t_exit >= self.end_at {
            return false;
        }
        let seg = match self.ahead.take() {
            Some(a) => a,
            None => self.seq.next(&sh.step, w),
        };
        let Some((seg, o)) = seg.and_then(|s| seg_info(w, s).map(|o| (s, o))) else {
            self.ended = true;
            return false;
        };
        let post = self.rule.map(|(s, d)| (s.post, d.pre));
        if let Some((post, _)) = post
            && !post
        {
            let last = self.items.len() - 1;
            cut(w, now, &mut self.items[last], t_exit);
        }
        let from = if post.is_some_and(|(_, pre)| !pre) { o.entry } else { 0.0 };
        self.start(w, sh, now, seg, t_exit - o.entry / 1000.0, from, t_exit);
        true
    }

    /// Stops what the run plays and mutes it: a dropped run is silent at once.
    pub(super) fn drop_run(&mut self, w: &mut dyn World, now: f64) {
        self.dead = true;
        self.cut_all(w, now, 0.0);
    }

    fn cut_all(&mut self, w: &mut dyn World, now: f64, t: f64) {
        for i in &mut self.items {
            cut(w, now, i, t);
        }
    }

    /// The segment the playlist is in at `t` and the position in it (ms).
    pub(super) fn at(&self, t: f64) -> Option<(Id, f64)> {
        for (k, i) in self.items.iter().enumerate().rev() {
            if k > 0 && t < i.sync {
                continue;
            }
            let pos = (t - i.t0) * 1000.0;
            return (pos < i.o.dur).then_some((i.seg, pos));
        }
        None
    }

    pub(super) fn exit_after(&self, t: f64) -> Option<f64> {
        self.items.iter().map(|i| i.t0 + i.o.exit / 1000.0).find(|&e| e >= t)
    }

    // Sync point of a NextBar / NextBeat / NextGrid source rule (CAkScheduleWindow::FindSyncPoint 18000b790 over
    // CAkMusicSegment::GetExitSyncPos 18000f590, called from ScheduleSwitchTransition at 1800195a2 / 1800196e2 /
    // 180019798 with the entry cue skipped for the entry types EntryMarker and LastExitTime): the sync point is the
    // first bar / beat / grid line of the source segment strictly after the earliest sync time (at least a sample),
    // counted in samples from the segment's entry cue (grid lines from entry cue + grid offset). A line on the exit
    // cue counts, one past it does not: the search then moves to the playlist's next segment, where the entry cue is
    // skipped too (its first line is one bar / beat / grid period in), and so on; after the playlist's last segment
    // the sync is the later of the earliest sync time and that segment's exit cue. The meter (CAkMusicNode::
    // GetMusicGrid 18000e090) is the segment's when it overrides its parent's, else the nearest overriding
    // ancestor's; SetMusicNodeParams (18000ea60) keeps it in samples: beat = ceil(60 / tempo x 4 / beat value x
    // 48000), bar = beat x beats per bar, grid = ceil(48 x period ms), grid offset = ceil(48 x offset ms).
    /// The first line strictly after `t` of the segment playing at `t`, not past its exit cue; else of the segments
    /// that follow, chained here when the playlist has not been scheduled that far.
    pub(super) fn grid_after(
        &mut self,
        w: &mut dyn World,
        sh: &mut Shared,
        now: f64,
        t: f64,
        kind: SyncType,
    ) -> Option<(f64, bool)> {
        let mut k = self.items.len().checked_sub(1)?;
        while k > 0 && t < self.items[k].sync {
            k -= 1;
        }
        let mut n = 0;
        while n < 16 {
            if k == self.items.len() && !self.chain(w, sh, now) {
                break;
            }
            let i = &self.items[k];
            let (seg, t0, o) = (i.seg, i.t0, i.o);
            let mut m = None;
            let mut p = Some(seg);
            for _ in 0..32 {
                let Some(id) = p else { break };
                let Some(ob) = w.obj(id) else { break };
                m = meter_of(ob);
                if m.is_some() {
                    break;
                }
                p = parent_of(ob);
            }
            let m = m?;
            let beat = ((240.0 / (m.tempo * f64::from(m.beat_value))) * RATE - 1e-6).ceil();
            let per = match kind {
                SyncType::NextBar => beat * f64::from(m.beats),
                SyncType::NextBeat => beat,
                _ => (m.grid_period * RATE / 1000.0 - 1e-6).ceil(),
            };
            if per <= 0.0 {
                return None;
            }
            let smp = |ms: f64| mvs_math::js::round(ms * RATE / 1000.0);
            let active = smp(o.exit - o.entry);
            let min = smp((t - t0) * 1000.0 - o.entry).max(0.0);
            if min <= active {
                let mut line =
                    if kind == SyncType::NextGrid { (m.grid_offset * RATE / 1000.0 - 1e-6).ceil() } else { 0.0 };
                if line < min + 1.0 {
                    line += ((min + 1.0 - line) / per).ceil() * per;
                }
                if line <= active {
                    return Some((t0 + (o.entry + (line * 1000.0) / RATE) / 1000.0, line == active));
                }
            }
            k += 1;
            n += 1;
        }
        // Unverified: whether a sync on the last exit cue plays the post-exit is not decoded; it does not here.
        let last = self.items.last()?;
        Some((t.max(last.t0 + last.o.exit / 1000.0), false))
    }

    fn level(&self, t: f64) -> f64 {
        let Some(f) = self.fade_in else { return 1.0 };
        if t <= f.t0 {
            0.0
        } else if t >= f.t0 + f.dur {
            1.0
        } else {
            interp(f.curve, (t - f.t0) / f.dur, 0.0, 1.0)
        }
    }

    /// Adds steps to the fade's program and gives them to the run's live voices. With `cancel`, what the fade had
    /// from the first step on is dropped first, in the program and in the mixer's queue of each voice (Web Audio's
    /// cancelScheduledValues on the run's one fade gain).
    fn program(&mut self, w: &mut dyn World, now: f64, steps: &[Step], cancel: bool) {
        let Some(first) = steps.first() else { return };
        if cancel {
            self.prog.retain(|s| s.at < first.at);
        }
        self.prog.extend_from_slice(steps);
        for v in self.live(now) {
            if cancel {
                w.emit(Control::Cancel(Cancel {
                    from: samples(first.at),
                    target: Target::Voice(v),
                    param: Param::Fade,
                }));
            }
            // (each program here begins with a set: the value before it does not matter)
            let mut j = Join::new(v, Param::Fade, 1.0, now);
            for s in steps {
                j.push(w, s);
            }
            j.flush(w);
        }
    }

    /// The transition runs along the rule's curve, from `from` to `to`.
    // One curve, exact at each sample.
    #[allow(clippy::too_many_arguments)]
    fn ramp(&mut self, w: &mut dyn World, now: f64, t0: f64, dur: f64, curve: Curve, from: f64, to: f64) {
        if curve == Curve::Constant {
            // every step of a Constant curve is at `from`
            self.program(w, now, &[Step::set(t0, from)], true);
            return;
        }
        let steps = [Step::set(t0, from), Step { at: t0, value: to, ramp: Ramp::Curve { secs: dur, curve } }];
        self.program(w, now, &steps, true);
    }

    pub(super) fn fade_in_at(&mut self, w: &mut dyn World, now: f64, t0: f64, dur: f64, curve: Curve) {
        self.fade_in = Some(FadeIn { t0, dur, curve });
        self.program(w, now, &[Step::set(0.0, 0.0)], false);
        self.ramp(w, now, t0, dur, curve, 0.0, 1.0);
    }

    /// Source side of a transition whose sync point is `t_sync`.
    pub(super) fn stop(&mut self, w: &mut dyn World, now: f64, t_sync: f64, s: &RuleSrc, at_exit: bool) {
        self.end_at = t_sync;
        // segments chained ahead that would sync at or after the transition never play
        while self.items.len() > 1 && self.items[self.items.len() - 1].sync >= t_sync {
            let mut i = self.items.pop().expect("an item");
            cut(w, now, &mut i, 0.0);
            self.recycle(i);
        }
        if s.fade.t > 0.0 {
            let f0 = t_sync + (s.fade.off - s.fade.t) / 1000.0;
            let from = self.level(f0);
            self.ramp(w, now, f0, s.fade.t / 1000.0, s.fade.curve, from, 0.0);
            self.kill_at = f0 + s.fade.t / 1000.0;
        } else if !(s.post && at_exit) {
            self.program(w, now, &[Step::set(t_sync, 0.0)], true);
            self.kill_at = t_sync;
        }
        if self.kill_at < f64::INFINITY {
            let k = self.kill_at;
            self.cut_all(w, now, k);
        }
    }
}

/// Stops the voices of a segment at `t` (s; at once for a time that has passed): those that would run past it.
// A clip's voice ends by a Stop at its time, max(that time, now); a voice that has ended is not stopped again.
fn cut(w: &mut dyn World, now: f64, i: &mut Played, t: f64) {
    let at = t.max(now);
    for v in &mut i.vox {
        if v.end > at {
            v.end = at;
            w.emit(change(v.id, at, Param::Stop, 0.0, Ramp::Set));
        }
    }
}

/// A medium mixed to stereo: its channels and the gains of each to the left and right outputs. Up to two channels it
/// plays as it is (a mono source on both sides); a longer layout goes through the down-mix matrix of its speakers
/// (direct speaker assignment of the 2D music).
fn downmix(channels: u16, mask: Option<u32>) -> Option<(u8, [[f64; 2]; crate::spec::MAX_CHANNELS])> {
    use crate::ctl::pan::{STEREO_MIX, VORBIS_ORDER};
    let mut pan = [[0.0; 2]; crate::spec::MAX_CHANNELS];
    match channels {
        1 => pan[0] = [1.0, 1.0],
        2 => {
            pan[0] = [1.0, 0.0];
            pan[1] = [0.0, 1.0];
        }
        n => {
            let order = VORBIS_ORDER.get(usize::from(n)).filter(|o| !o.is_empty())?;
            let mask = mask.unwrap_or(0);
            // the speakers of the mask, in bit order
            let mut bits = [0usize; STEREO_MIX.len()];
            let mut nb = 0;
            for b in 0..STEREO_MIX.len() {
                if mask >> b & 1 != 0 {
                    bits[nb] = b;
                    nb += 1;
                }
            }
            if nb != usize::from(n) || mask >> STEREO_MIX.len() != 0 {
                return None;
            }
            for (i, &c) in order.iter().enumerate() {
                pan[i] = STEREO_MIX[bits[usize::from(c)]];
            }
        }
    }
    Some((u8::try_from(channels).ok()?, pan))
}

/// A clip automation's value where a voice starts in the clip (`x0`, s from its audible start): between two points
/// it follows the first point's curve.
fn auto_start(p: &[Pt], x0: f64) -> f64 {
    let mut v0 = p[0].y;
    for i in 0..p.len() - 1 {
        if x0 >= p[i].x {
            v0 = if x0 >= p[i + 1].x {
                p[i + 1].y
            } else {
                let span = p[i + 1].x - p[i].x;
                interp(p[i].curve, (x0 - p[i].x) / if span == 0.0 { 1.0 } else { span }, p[i].y, p[i + 1].y)
            };
        }
    }
    v0
}

/// A clip automation as steps, the first at `when` (`out` gets them): `x0` is the voice's start in the clip (s), `tc`
/// the clip's audible start; between two points the value follows the first point's curve (the evaluator the
/// transitions' fades use).
// A span from the clip's start on is one curve, exact at each sample; one begun part-way, or after an earlier
// event than its first point, steps once per audio frame along the curve.
fn auto_steps(p: &[Pt], add: f64, x0: f64, tc: f64, when: f64, out: &mut Vec<Step>) {
    // the time of the last event
    let mut tp = when;
    for i in 1..p.len() {
        if p[i].x <= x0 {
            continue;
        }
        let kind = p[i - 1].curve;
        let end = tc + p[i].x;
        let to = p[i].y + add;
        match kind {
            Curve::Constant => out.push(Step::set(end, to)),
            Curve::Linear => out.push(Step { at: tp, value: to, ramp: Ramp::Linear(end - tp) }),
            _ => {
                let xa = x0.max(p[i - 1].x);
                let span = p[i].x - p[i - 1].x;
                if xa == p[i - 1].x && (tp - (tc + xa)).abs() < 1e-9 {
                    out.push(Step { at: tp, value: to, ramp: Ramp::Curve { secs: end - tp, curve: kind } });
                } else {
                    // a curved span: one step per audio frame along the shape
                    let n = ((p[i].x - xa) / FRAME_SECS - 1e-6).ceil().max(1.0);
                    let mut t_prev = tp;
                    for k in 1..=n as u32 {
                        let x = xa + ((p[i].x - xa) * f64::from(k)) / n;
                        let v = interp(kind, (x - p[i - 1].x) / span, p[i - 1].y, p[i].y) + add;
                        out.push(Step { at: t_prev, value: v, ramp: Ramp::Linear(tc + x - t_prev) });
                        t_prev = tc + x;
                    }
                }
            }
        }
        tp = end;
    }
}

/// Whether a segment's tracks have a clip of medium `m`.
pub(super) fn seg_has<'a>(obj: &dyn Fn(Id) -> Option<&'a Obj>, seg: Id, m: Id) -> bool {
    obj(seg).is_some_and(|o| {
        o.children().iter().any(|&t| matches!(obj(t), Some(Obj::MusicTrack(tr)) if tr.clips.iter().any(|c| c.src == m)))
    })
}
