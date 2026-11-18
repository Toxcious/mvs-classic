//! Interactive music: music switch containers (the decision tree, transition rules, sync points), music random /
//! sequence containers, segments and their tracks' clips, the flow of a music switch group's values
//! (CAkMusicSwitchCtx / CAkSequenceCtx / CAkSegmentCtx over the music objects of an event's bank). Times in the
//! data are ms; inside, s on the audio clock.
//!
//! Its voices are numbered by [`Ctl::music_voice`], its random draws by [`Ctl::draw`] (logged in tests), its
//! decisions logged by [`Ctl::music_line`]; a bus it plays into is put in use by `BusCtl::touch`.
//!
//! A music switch container picks its child from its decision tree with the global switch values; when the picked
//! path changes, the container at the first level that differs owns the transition:
//!   - rule: CAkMusicTransAware::GetTransitionRule (180007aa0 / 180007b90) searches the rules from the last to the
//!     first; a side matches when its list holds -1 or the segment or one of its ancestors below the container.
//!     Rule 0 when none matches. A source with no segment (empty or finished playlist) only matches -1.
//!   - time: CAkMusicSwitchCtx::ScheduleSwitchTransition (180018c00). The sync point is not before
//!     now + max(destination look-ahead, source fade time - fade offset); Immediate takes that time, ExitMarker the
//!     source segment's next exit cue after it. The source fades out over [sync + offset - time, sync + offset];
//!     without a fade it stops at the sync point, and plays its post-exit only when the sync is its exit cue. The
//!     destination starts one look-ahead before the sync point so that its entry cue (or, with LastExitTime, the
//!     position at which the playlist was last left) falls on it; its fade-in starts at sync + offset, silent before.
//!     A transition that has not started yet is dropped by a later change (DequeueCancellableTransitions).
//!
//! A playlist chains its segments entry cue on exit cue, pre-entry and post-exit overlapping per its own rule. The
//! order comes from its item tree (AkRSIterator): a group is a sequence or a random pick, played through
//! (continuous) or one item per visit (step), each item with its own loop count.
//!
//! Transition segment (the rule's transition object, ScheduleSwitchTransition 180018c00, rule +0x50: segment id,
//! fade-in +4, fade-out +0x10 with its offset at +0x18, flags +0x1c bit 0 play pre-entry, bit 1 play post-exit): the
//! change is made of two transitions. Source -> transition segment uses the rule's source side and, as destination
//! side, the object's fade-in with the entry cue as entry and its pre-entry flag. Transition segment -> destination
//! uses, as source side, the object's fade-out with sync type ExitMarker (7) and its post-exit flag, and the rule's
//! destination side. The first sync point is not before now + max(max(destination look-ahead, fade-out time -
//! offset) - the transition segment's active duration (entry to exit cue), max(its look-ahead, source fade time -
//! offset)); the second is the transition segment's exit cue. While the segment plays, the second transition is the
//! one a later change drops; that change then leaves from the transition segment, whose rule chain is its own
//! ancestors.
//!
//! Look-ahead of a destination (CAkMusicTrack::ComputeMinSrcLookAhead 180011f40, read from the DLL's code: the
//! track's clips are AkTrackSrc of 0x1c bytes, +4 source id, +0xc clip start, +0x18 source trim offset; the source's
//! media flags are at +0xc, bit 1 prefetch, bits 2-6 the type, 1 = file): for each clip, the track's LookAheadTime
//! when its source is streamed and either has no prefetch, or the start position is past the clip's start (the
//! stream must seek), or the clip is trimmed at its beginning (it does not start where the prefetch does); 0 for a
//! source in memory and for a prefetched one played from its start. From it comes off the time between the start
//! position and the clip's start; the track's look-ahead is the largest over its clips (not under 0).
//!
//! Not ported (warned when met): the cue entry types (the only readers of a destination cue filter,
//! CAkMusicSegment::GetEntrySyncPos 18000f500), several arguments per container.
// Deviation: the look-ahead is taken in ms, the engine rounds it to audio frames.
// Deviation: the music runs on the mixer thread, which otherwise allocates nothing: playing on (its ticks, chaining
// segments, starting their voices) allocates nothing once its lists have grown, but a transition does (its new run
// with a copy of the playlist's item tree, the paths, the run and resume maps' entries).

mod iter;
mod load;
mod run;

use std::collections::BTreeMap;

use mvs_data::sound::{
    Bus, EntryType, Id, MediaRef, Meter, MusicItem, MusicRule, Obj, Pt, Rtpc, RuleDst, RuleSrc, SyncType,
};

use self::iter::StepInfo;
pub(crate) use self::load::{Reach, event_id, flow_values, load, music_bank, reached};
use self::run::{Run, seg_info};
use super::curves::{FRAME_SECS, db_to_gain};
use super::{Ctl, Out, secs};
use crate::spec::{Control, Param, Ramp, VoiceId};
use crate::{GameObj, Garbage, MusicCmd, Note};

/// Seconds between the music's own ticks.
const TICK: f64 = 0.1;
/// s before its start at which a waiting transition is committed.
const MARGIN: f64 = 0.3;

/// What the music reads of the interpreter and writes to it.
pub(crate) trait World {
    fn obj(&self, id: Id) -> Option<&Obj>;
    fn media(&self, id: Id) -> Option<&MediaRef>;
    /// A switch group's global value.
    fn switch(&self, group: Id) -> Option<Id>;
    fn bus(&self, id: Id) -> Option<&Bus>;
    fn master(&self) -> Option<Id>;
    /// The dB the Volume and MakeUpGain curves of a list give on the global values.
    fn rtpc_db(&self, list: &[Rtpc]) -> f64;
    /// A bus comes into use.
    fn touch(&mut self, bus: Id);
    fn emit(&mut self, c: Control);
    fn voice(&mut self) -> VoiceId;
    fn draw(&mut self) -> f64;
    fn line(&mut self, t: f64, node: Id, what: &str);
    fn warn(&mut self, what: &'static str, id: u64);
}

/// The rules of a music container.
fn rules_of(o: &Obj) -> &[MusicRule] {
    match o {
        Obj::MusicSwitchContainer(c) => &c.rules,
        Obj::MusicRandomSequenceContainer(c) => &c.rules,
        _ => &[],
    }
}

/// A music node's meter, when it overrides its parent's.
fn meter_of(o: &Obj) -> Option<Meter> {
    match o {
        Obj::MusicSwitchContainer(c) => c.meter,
        Obj::MusicRandomSequenceContainer(c) => c.meter,
        Obj::MusicSegment(c) => c.meter,
        _ => None,
    }
}

fn parent_of(o: &Obj) -> Option<Id> {
    o.node().and_then(|n| n.parent)
}

/// What the runs share: the playlist states of the step groups, and where the music enters its bus (the Volume /
/// MakeUpGain curves of the buses above count as the voice's own, here one gain); room for a clip automation's points
/// and steps.
#[derive(Default)]
struct Shared {
    step: StepInfo,
    bus: Id,
    /// The user aux sends of the music node's chain (bus, linear gain), read when the music starts.
    sends: Vec<(Id, f64)>,
    gain: f64,
    list: Vec<Rtpc>,
    pts: Vec<Pt>,
    steps: Vec<run::Step>,
}

/// The two sides of a transition rule.
#[derive(Clone, Copy)]
struct Sides {
    s: RuleSrc,
    d: RuleDst,
}

/// A transition waiting for its sync point; `then` is the transition out of a transition segment, pending once the
/// segment has been committed.
struct Pending {
    path: Vec<Id>,
    rule: Sides,
    t_sync: f64,
    la: f64,
    at_exit: bool,
    seg: Option<Id>,
    entry_pos: f64,
    start_pos: f64,
    run: Option<u32>,
    then: Option<Box<Pending>>,
}

/// A transition segment between two playlists.
struct Via {
    tr: mvs_data::sound::RuleTrans,
    entry: f64,
    start: f64,
    la: f64,
    active: f64,
}

/// The music's state.
#[derive(Default)]
pub struct Music {
    root: Option<Id>,
    shared: Shared,
    runs: BTreeMap<u32, Run>,
    next_run: u32,
    cur: Option<u32>,
    cur_path: Vec<Id>,
    pending: Option<Pending>,
    dying: Vec<u32>,
    /// Playlist -> where it was last left (segment, position ms).
    left: BTreeMap<Id, (Id, f64)>,
    next_tick: Option<f64>,
    /// Room for the path the switch values pick and for the source and destination chains a rule is matched on.
    path: Vec<Id>,
    chains: [Vec<Id>; 2],
}

impl Music {
    /// Play action on a music switch container.
    fn play(&mut self, w: &mut dyn World, now: f64, root: Id) {
        if self.root == Some(root) {
            return;
        }
        let Some(o) = w.obj(root) else {
            return w.warn("music node missing", u64::from(root));
        };
        let over = o.node().and_then(|n| n.bus).filter(|&b| b != 0);
        self.root = Some(root);
        let bus = over.or(w.master()).unwrap_or(0);
        let mut list = Vec::new();
        let mut b = Some(bus);
        while let Some(d) = b.and_then(|b| w.bus(b)) {
            list.extend(d.rtpc.iter().filter(|r| r.param == 0 || r.param == 7).cloned());
            b = d.parent;
        }
        w.touch(bus);
        self.shared.bus = bus;
        self.shared.sends = user_sends(w, root);
        for i in 0..self.shared.sends.len() {
            w.touch(self.shared.sends[i].0);
        }
        self.shared.gain = gain_of(w.rtpc_db(&list));
        self.shared.list = list;
        self.switch_changed(w, now);
        self.next_tick.get_or_insert(now + TICK);
    }

    /// Node path below the root down to the playlist the switch values pick ([] or a container last: nothing).
    fn resolve(&self, w: &mut dyn World, path: &mut Vec<Id>) {
        path.clear();
        let mut id = self.root.expect("a root");
        for _ in 0..16 {
            let Some(Obj::MusicSwitchContainer(o)) = w.obj(id) else { break };
            let several = o.args.len() != 1;
            let v = o.args.first().and_then(|&g| w.switch(g));
            let e = o.tree.iter().find(|x| Some(x.key) == v).or_else(|| o.tree.iter().find(|x| x.key == 0));
            let next = e.map(|e| e.node).filter(|&n| n != 0);
            if several {
                w.warn("music switch with several arguments", u64::from(id));
            }
            let Some(next) = next else { break };
            path.push(next);
            id = next;
        }
    }

    fn switch_changed(&mut self, w: &mut dyn World, now: f64) {
        if self.root.is_none() {
            return;
        }
        let mut np = std::mem::take(&mut self.path);
        self.resolve(w, &mut np);
        let same = |a: &[Id], b: &[Id]| a == b;
        let moved = 'moved: {
            if same(&np, self.pending.as_ref().map_or(&self.cur_path, |p| &p.path)) {
                break 'moved false;
            }
            if let Some(p) = self.pending.take() {
                self.drop_pending(w, now, &p);
                // (from a transition segment the way back to the playlist it was heading for is a transition too)
                if same(&np, &self.cur_path) && !self.cur_via() {
                    break 'moved false;
                }
            }
            self.plan(w, now, &np);
            true
        };
        self.path = np;
        if moved {
            self.tick(w, now);
        }
    }

    fn cur_via(&self) -> bool {
        self.cur.and_then(|c| self.runs.get(&c)).is_some_and(|r| r.via)
    }

    fn drop_pending(&mut self, w: &mut dyn World, now: f64, p: &Pending) {
        for key in [p.run, p.then.as_ref().and_then(|t| t.run)].into_iter().flatten() {
            if let Some(r) = self.runs.get_mut(&key) {
                r.drop_run(w, now);
            }
        }
    }

    fn make_run(&mut self, w: &mut dyn World, pid: Id) -> Option<u32> {
        let (list, rule, odd, pre) = {
            let Some(Obj::MusicRandomSequenceContainer(po)) = w.obj(pid) else { return None };
            let rule = po.rules.first().map(|r| (r.s, r.d));
            // the pre-entry of every root item's segments
            let pre = if rule.is_none_or(|r| r.1.pre) { run::pre_entry(&*w, &po.list) } else { 0.0 };
            let odd = po.rules.len() > 1
                || po.rules.first().is_some_and(|r| {
                    r.s.sync != SyncType::ExitMarker || r.s.fade.t != 0.0 || r.d.fade.t != 0.0 || r.trans.is_some()
                });
            (po.list.first().cloned(), rule, odd, pre)
        };
        if odd {
            w.warn("music playlist rules not ported", u64::from(pid));
        }
        Some(self.add_run(Run::new(pid, list, rule, pre)))
    }

    fn add_run(&mut self, r: Run) -> u32 {
        self.next_run += 1;
        self.runs.insert(self.next_run, r);
        self.next_run
    }

    fn first(&mut self, w: &mut dyn World, key: u32, resume: Option<Id>) -> Option<Id> {
        self.runs.get_mut(&key)?.first(&self.shared.step, resume, w)
    }

    fn discard(&mut self, w: &mut dyn World, now: f64, key: Option<u32>) {
        if let Some(r) = key.and_then(|k| self.runs.get_mut(&k)) {
            r.drop_run(w, now);
        }
    }

    fn plan(&mut self, w: &mut dyn World, now: f64, np: &[Id]) {
        let mut k = 0;
        while k < np.len() && k < self.cur_path.len() && np[k] == self.cur_path[k] {
            k += 1;
        }
        if self.cur_via() && k == np.len() && k > 0 {
            k -= 1;
        }
        let owner = if k == 0 { self.root.expect("a root") } else { np[k - 1] };
        let here = self.cur.and_then(|c| self.runs.get(&c)).and_then(|r| r.at(now));
        let [mut src_chain, mut dst_chain] = std::mem::take(&mut self.chains);
        src_chain.clear();
        dst_chain.clear();
        if let Some((seg, _)) = here {
            if self.cur_via() {
                let mut id = Some(seg);
                for _ in 0..32 {
                    let Some(i) = id.filter(|&i| i != owner) else { break };
                    src_chain.push(i);
                    id = w.obj(i).and_then(parent_of);
                }
            } else {
                src_chain.push(seg);
                src_chain.extend_from_slice(&self.cur_path[k.min(self.cur_path.len())..]);
            }
        }
        let pid = np.last().copied();
        let mut run = pid.and_then(|p| self.make_run(w, p));
        // the rule is picked with the playlist's first segment; LastExitTime then resumes the segment it was left in
        let hist = run.and(pid).and_then(|p| self.left.get(&p).copied());
        let mut seg = run.and_then(|r| self.first(w, r, None));
        dst_chain.extend(seg);
        dst_chain.extend_from_slice(&np[k..]);
        let hit =
            |list: &[i64], chain: &[Id]| list.contains(&-1) || chain.iter().any(|&x| list.contains(&i64::from(x)));
        let picked = {
            let rules = w.obj(owner).map(rules_of).unwrap_or_default();
            let r = rules.iter().rev().find(|r| hit(&r.src, &src_chain) && hit(&r.dst, &dst_chain));
            r.or(rules.first()).map(|r| (Sides { s: r.s, d: r.d }, r.trans))
        };
        self.chains = [src_chain, dst_chain];
        let Some((rule, trans)) = picked else {
            self.discard(w, now, run);
            return w.warn("music container without a transition rule", u64::from(owner));
        };
        if seg.is_some_and(|s| w.obj(s).is_none()) {
            seg = None;
        }
        let mut entry_pos = seg.and_then(|s| seg_info(w, s)).map_or(0.0, |s| s.entry);
        let mut start_pos = if rule.d.pre { 0.0 } else { entry_pos };
        match (rule.d.entry, hist) {
            (EntryType::LastExitTime, Some((hseg, hpos))) => {
                let r2 = pid.and_then(|p| self.make_run(w, p));
                let s2 = r2.and_then(|r| self.first(w, r, Some(hseg)));
                if s2.is_some() {
                    self.discard(w, now, run);
                    run = r2;
                    seg = s2;
                    entry_pos = hpos;
                    start_pos = hpos;
                } else {
                    self.discard(w, now, r2);
                }
            }
            (EntryType::EntryMarker | EntryType::LastExitTime, _) => {}
            _ => w.warn("music entry type not ported", u64::from(owner)),
        }
        let la = seg.map_or(0.0, |s| look_ahead(w, s, entry_pos, start_pos));
        // a transition segment between the two
        let via = match trans {
            Some(t) => match seg_info(w, t.seg) {
                Some(so) => Some(Via {
                    tr: t,
                    entry: so.entry,
                    start: if t.pre { 0.0 } else { so.entry },
                    la: 0.0,
                    active: so.exit - so.entry,
                }),
                None => {
                    w.warn("music transition segment missing", u64::from(t.seg));
                    None
                }
            },
            None => None,
        };
        let mut via = via;
        let mut t_min = now + la.max(rule.s.fade.t - rule.s.fade.off) / 1000.0;
        if let Some(v) = &mut via {
            v.la = look_ahead(w, v.tr.seg, v.entry, v.start);
            t_min = now
                + (la.max(v.tr.fade_out.t - v.tr.fade_out.off) - v.active)
                    .max(v.la)
                    .max(rule.s.fade.t - rule.s.fade.off)
                    / 1000.0;
        }
        let (mut t_sync, mut at_exit) = (t_min, false);
        let cur = self.cur;
        match rule.s.sync {
            SyncType::ExitMarker => {
                if let Some(e) = cur.and_then(|c| self.runs.get(&c)).and_then(|r| r.exit_after(t_min)) {
                    t_sync = e;
                    at_exit = true;
                }
            }
            SyncType::NextBar | SyncType::NextBeat | SyncType::NextGrid => {
                if let Some(c) = cur
                    && let Some(r) = self.runs.get_mut(&c)
                    && let Some((t, ex)) = r.grid_after(w, &mut self.shared, now, t_min, rule.s.sync)
                {
                    t_sync = t;
                    at_exit = ex;
                }
            }
            SyncType::Immediate => {}
            _ => w.warn("music sync played as immediate", u64::from(owner)),
        }
        let mut dest =
            Pending { path: np.to_vec(), rule, t_sync, la, at_exit, seg, entry_pos, start_pos, run, then: None };
        let Some(v) = via else {
            self.pending = Some(dest);
            return;
        };
        let t = v.tr;
        let item = MusicItem { seg: t.seg, loop_count: 1, ..MusicItem::default() };
        let pre = run::pre_entry(&*w, std::slice::from_ref(&item));
        let vkey = self.add_run(Run::new(t.seg, Some(item), None, pre));
        if let Some(r) = self.runs.get_mut(&vkey) {
            r.via = true;
        }
        self.first(w, vkey, None);
        dest.rule = Sides { s: RuleSrc { fade: t.fade_out, sync: SyncType::ExitMarker, post: t.post }, d: rule.d };
        dest.t_sync = t_sync + v.active / 1000.0;
        dest.at_exit = true;
        self.pending = Some(Pending {
            path: np.to_vec(),
            rule: Sides { s: rule.s, d: RuleDst { fade: t.fade_in, entry: EntryType::EntryMarker, pre: t.pre } },
            t_sync,
            la: v.la,
            at_exit,
            seg: Some(t.seg),
            entry_pos: v.entry,
            start_pos: v.start,
            run: Some(vkey),
            then: Some(Box::new(dest)),
        });
    }

    fn commit(&mut self, w: &mut dyn World, now: f64, p: Pending) {
        self.pending = p.then.map(|b| *b);
        if let Some(old) = self.cur.take() {
            let r = self.runs.get_mut(&old).expect("the playing run");
            let here = r.at(p.t_sync);
            if let (Some(h), false) = (here, r.via) {
                self.left.insert(r.id, h);
            }
            r.stop(w, now, p.t_sync, &p.rule.s, p.at_exit);
            self.dying.push(old);
        }
        self.cur = p.run;
        self.cur_path = p.path;
        // Unverified: where a `music` line is logged (a segment's start, a transition at its sync point) is guessed;
        // no voice tape has music on it yet.
        w.line(p.t_sync, p.seg.or(self.cur_path.last().copied()).unwrap_or(0), "transition");
        if let (Some(key), Some(seg)) = (p.run, p.seg) {
            let r = self.runs.get_mut(&key).expect("the committed run");
            if p.rule.d.fade.t > 0.0 {
                r.fade_in_at(
                    w,
                    now,
                    p.t_sync + p.rule.d.fade.off / 1000.0,
                    p.rule.d.fade.t / 1000.0,
                    p.rule.d.fade.curve,
                );
            }
            r.start(w, &mut self.shared, now, seg, p.t_sync - p.entry_pos / 1000.0, p.start_pos, p.t_sync);
        }
    }

    fn tick(&mut self, w: &mut dyn World, now: f64) {
        for _ in 0..2 {
            let Some(p) = &self.pending else { break };
            if p.t_sync - now > p.la / 1000.0 + MARGIN {
                break;
            }
            let p = self.pending.take().expect("a pending transition");
            self.commit(w, now, p);
        }
        if let Some(r) = self.cur.and_then(|c| self.runs.get_mut(&c)) {
            r.tick(w, &mut self.shared, now);
        }
        for i in 0..self.dying.len() {
            if let Some(r) = self.runs.get_mut(&self.dying[i]) {
                r.tick(w, &mut self.shared, now);
            }
        }
        let runs = &self.runs;
        self.dying.retain(|k| runs.get(k).is_some_and(|r| !r.dead));
        self.collect();
    }

    /// Whether a run has the medium: a run's segments and the one it picked next, and the
    /// segment a run waiting at its sync point starts with (a transition's and the one after it: `run.hold(seg)`,
    /// `vr.hold(t.seg)`), which it holds before it plays.
    fn uses<'a>(&self, obj: &dyn Fn(Id) -> Option<&'a Obj>, m: Id) -> bool {
        if self.runs.values().any(|r| r.holds(obj, m)) {
            return true;
        }
        let mut p = self.pending.as_ref();
        while let Some(q) = p {
            if let (Some(_), Some(seg)) = (q.run, q.seg)
                && run::seg_has(obj, seg, m)
            {
                return true;
            }
            p = q.then.as_deref();
        }
        false
    }

    /// Lets go the runs nothing refers to any more.
    fn collect(&mut self) {
        let (cur, dying, pending) = (self.cur, &self.dying, &self.pending);
        let keep = |k: &u32| {
            if cur == Some(*k) || dying.contains(k) {
                return true;
            }
            let mut p = pending.as_ref();
            while let Some(q) = p {
                if q.run == Some(*k) {
                    return true;
                }
                p = q.then.as_deref();
            }
            false
        };
        self.runs.retain(|k, r| !r.dead || keep(k));
    }

    /// The Volume / MakeUpGain curves of the buses the music plays into follow the game parameters: the voices'
    /// gain moves over an audio frame.
    fn follow_gain(&mut self, w: &mut dyn World, now: f64) {
        if self.root.is_none() {
            return;
        }
        let g = gain_of(w.rtpc_db(&self.shared.list));
        if g == self.shared.gain {
            return;
        }
        self.shared.gain = g;
        for r in self.runs.values() {
            for v in r.live(now) {
                w.emit(run::change(v, now, Param::RtpcGain, g, Ramp::Linear(FRAME_SECS)));
            }
        }
    }

    fn quantum(&mut self, w: &mut dyn World, now: f64) {
        self.follow_gain(w, now);
        if let Some(t) = self.next_tick
            && now >= t
        {
            self.tick(w, now);
            self.next_tick = Some(if now - t > TICK { now + TICK } else { t + TICK });
        }
    }
}

/// The user aux sends a music node plays with: the first node of its chain that overrides the setting, else the top
/// node, holds the buses and their send volumes (the sounds' `plan` does the same for a voice). The music's analyzer
/// buses (AkMeterFX) hang on them, so without the sends the meters never run.
// Deviation: the send volumes' RTPC curves (props 39..) are not followed; the static volume is used.
fn user_sends(w: &dyn World, root: Id) -> Vec<(Id, f64)> {
    let mut cur = Some(root);
    for _ in 0..32 {
        let Some(n) = cur.and_then(|c| w.obj(c)).and_then(Obj::node) else { break };
        if n.aux_ov || n.parent.is_none() {
            let mut sends = Vec::new();
            for (i, &aid) in n.aux.iter().enumerate() {
                let sdb = n.p.get(19 + i as u16).unwrap_or(0.0);
                if aid != 0 && w.bus(aid).is_some() && sdb > -96.3 {
                    sends.push((aid, db_to_gain(sdb).min(4.0)));
                }
            }
            return sends;
        }
        cur = n.parent;
    }
    Vec::new()
}

fn gain_of(db: f64) -> f64 {
    if db == f64::NEG_INFINITY { 0.0 } else { db_to_gain(db) }
}

/// Look-ahead of a segment entered at `entry_pos` and played from `start_pos`: the part before the sync position, plus
/// what the track streams need before their first clip.
fn look_ahead(w: &dyn World, seg: Id, entry_pos: f64, start_pos: f64) -> f64 {
    const RATE: f64 = 48_000.0;
    let Some(Obj::MusicSegment(so)) = w.obj(seg) else { return entry_pos - start_pos };
    let mut look = 0.0f64;
    for &tid in &so.children {
        let Some(Obj::MusicTrack(tr)) = w.obj(tid) else { continue };
        for c in &tr.clips {
            // (the engine compares in samples; held: 0 in memory, 1 streamed, 2 streamed with prefetch)
            let smp = |ms: f64| mvs_math::js::round(ms * RATE / 1000.0);
            let rel = smp(c.play_at + c.begin_trim) - smp(start_pos);
            let src =
                if c.held == 0 || (c.held == 2 && rel >= 0.0 && smp(c.begin_trim) == 0.0) { 0.0 } else { tr.look };
            look = look.max(src - ((rel * 1000.0) / RATE).max(0.0));
        }
    }
    entry_pos - start_pos + look
}

/// The interpreter as the music sees it.
struct CtlWorld<'a, 'o> {
    ctl: &'a mut Ctl,
    out: &'a mut Out<'o>,
    now: u64,
}

impl World for CtlWorld<'_, '_> {
    fn obj(&self, id: Id) -> Option<&Obj> {
        self.ctl.banks.obj(id)
    }
    fn media(&self, id: Id) -> Option<&MediaRef> {
        self.ctl.banks.media(id)
    }
    fn switch(&self, group: Id) -> Option<Id> {
        self.ctl.syncs.switch(group, GameObj::GLOBAL)
    }
    fn bus(&self, id: Id) -> Option<&Bus> {
        self.ctl.init.buses.get(&id)
    }
    fn master(&self) -> Option<Id> {
        self.ctl.master
    }
    fn rtpc_db(&self, list: &[Rtpc]) -> f64 {
        self.ctl.rtpc_of(list, GameObj::GLOBAL).0
    }
    fn touch(&mut self, bus: Id) {
        self.ctl.buses.touch(bus, self.now, &self.ctl.syncs, self.out);
    }
    fn emit(&mut self, c: Control) {
        self.out.controls.push(c);
    }
    fn voice(&mut self) -> VoiceId {
        self.ctl.music_voice()
    }
    fn draw(&mut self) -> f64 {
        self.ctl.draw(self.out)
    }
    fn line(&mut self, t: f64, node: Id, what: &str) {
        self.ctl.music_line(t, node, what, self.out);
    }
    fn warn(&mut self, what: &'static str, id: u64) {
        self.ctl.warn(what, id, self.out);
    }
}

/// Runs `f` on the music with the interpreter behind it.
fn with(ctl: &mut Ctl, now: u64, out: &mut Out, f: impl FnOnce(&mut Music, &mut dyn World, f64)) {
    let mut m = std::mem::take(&mut ctl.music);
    f(&mut m, &mut CtlWorld { ctl, out, now }, secs(now));
    ctl.music = m;
}

/// Takes a music command: the bank of a preload or a scope joined the loaded ones on the game side
/// (`Cmd::Banks` before it), the media were taken by the mixer before this; what the command carried goes back to
/// the game thread.
pub fn cmd(cmd: MusicCmd, out: &mut Out) {
    let (load, values, flow) = match cmd {
        MusicCmd::Preload { values, media, .. } => (Some(media), values, None),
        MusicCmd::Scope { values, flow, media, .. } => (Some(media), values, flow),
        MusicCmd::Release { values, media, .. } => {
            let _ = out.notes.push(Note::Free(Garbage::MusicValues(media)));
            (None, values, None)
        }
    };
    if let Some(load) = load {
        let _ = out.notes.push(Note::Free(Garbage::Scope(load)));
    }
    let _ = out.notes.push(Note::Free(Garbage::MusicValues(values)));
    if let Some(f) = flow {
        let _ = out.notes.push(Note::Free(Garbage::MusicFlow(f)));
    }
}

/// Whether a run of the music (playing, waiting or fading out) has the medium.
pub fn uses(ctl: &Ctl, m: Id) -> bool {
    let obj = |id| ctl.banks.obj(id);
    ctl.music.uses(&obj, m)
}

/// Plays a music hierarchy node a Play action names, on an object.
pub fn play(ctl: &mut Ctl, node: Id, obj: GameObj, now: u64, out: &mut Out) {
    let _ = obj;
    with(ctl, now, out, |m, w, t| m.play(w, t, node));
}

/// A global switch changed.
pub fn switch_moved(ctl: &mut Ctl, now: u64, out: &mut Out) {
    with(ctl, now, out, |m, w, t| m.switch_changed(w, t));
}

/// Moves the music for the quantum at `now`: segment ends, transitions, switch changes.
pub fn quantum(ctl: &mut Ctl, now: u64, out: &mut Out) {
    if ctl.music.root.is_none() {
        return;
    }
    with(ctl, now, out, |m, w, t| m.quantum(w, t));
}

#[cfg(test)]
mod tests;
