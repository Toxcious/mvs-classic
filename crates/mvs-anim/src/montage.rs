//! Plays one AnimMontage instance: montage time, composite sections
//! and the two notify paths of the engine. The rules and their sources (FAnimMontageInstance::Advance 14284b1d0,
//! FMontageSubStepper::Advance 14284b950, UAnimSequenceBase::GetAnimNotifiesFromDeltaPositions 1428719e0,
//! UAnimInstance::TriggerAnimNotifies 142845f30) are followed here.
//!
//! The montage is the bound data's record (`mvs_data::fighter::Montage`); the instance's state is a
//! [`MontageState`] its owner keeps (a fighter's montage slots, an actor's mesh), plain data that a rollback save
//! copies. A [`MontagePlayer`] pairs the two for one call: `MontagePlayer { m, s }.advance(dt, sink, ..)`. What reads
//! the state alone (the weights, the pose, the sections) is on `MontageState`, the montage passed in.
//!
//! The sink's handlers are the owner's: they get the player itself (they reach the owner through it),
//! and a notify by its index into the montage's notifies.

pub use mvs_data::fighter::{Montage, Notify, NotifyTick, Section, Segment, Slot};

use mvs_math::js;

/// `sameName`: two FNames compare without case.
pub fn same_name(a: &str, b: &str) -> bool {
    a == b || a.to_lowercase() == b.to_lowercase()
}

/// The owner's notify handlers (`NotifySink`), for an owner whose montage key is `K`.
pub trait NotifySink<K> {
    fn begin(&mut self, p: &mut MontagePlayer<'_, K>, idx: usize);
    fn end(&mut self, p: &mut MontagePlayer<'_, K>, idx: usize);
    fn instant(&mut self, p: &mut MontagePlayer<'_, K>, idx: usize);
    /// The sink has `rootStep` (a forward step of the position inside Advance, before that step's notifies).
    fn has_root_step(&self) -> bool {
        false
    }

    fn root_step(&mut self, _p: &mut MontagePlayer<'_, K>, _a: f64, _b: f64) {}
    /// The sink has `passEnd`.
    fn has_pass_end(&self) -> bool {
        false
    }

    fn pass_end(&mut self, _p: &mut MontagePlayer<'_, K>, _idx: usize) {}
}

/// A step of Advance: [a, b, lo, clamp] (`Step`).
pub type Step = [f64; 4];

/// What an Advance leaves for the queued dispatch (`Advanced`).
#[derive(Debug, Clone, PartialEq)]
pub struct Advanced {
    pub ranges: Vec<Step>,
    pub gone: bool,
}

/// `started`: false before the first Advance, 'jump' when a handler set the position since the last step, else true.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Started {
    #[default]
    No,
    Yes,
    Jump,
}

/// The montage clock's tolerance.
const EPS: f64 = 1e-7;

/// `upTo`: the upper bound of a step, exact in float32 when b is a section-end clamp, else with the clock's EPS.
#[inline]
fn up_to(t: f64, b: f64, clamp: f64) -> bool {
    if clamp != 0.0 { js::fround(t) <= js::fround(b) } else { t <= b + EPS }
}

/// `blendTicks`: weight updates (one per frame) a blend of `t` seconds takes to complete.
pub fn blend_ticks(t: impl Into<f64>) -> f64 {
    let t: f64 = t.into();
    if t > 0.0 { (t * 60.0 - 1e-6).ceil() } else { 0.0 }
}

/// Math.min on float32 (NaN wins, -0 below +0): exact through the double.
#[inline]
fn min32(a: f32, b: f32) -> f32 {
    js::min(f64::from(a), f64::from(b)) as f32
}

/// Math.max on float32 (NaN wins, +0 above -0).
#[inline]
fn max32(a: f32, b: f32) -> f32 {
    js::max(f64::from(a), f64::from(b)) as f32
}

/// A montage time as the build holds it (FAnimMontageInstance Position, the asset's section and segment times:
/// float). The state keeps it widened: a time the player wrote narrows exactly.
#[inline]
fn f(x: f64) -> f32 {
    x as f32
}

/// [`alpha_to_blend_option`] in float32, as the instance's own blend runs (FAlphaBlend's alpha is a float).
fn alpha_to_blend_option32(option: Option<&str>, x: f32, warn: &dyn Fn(&str)) -> f32 {
    match option {
        None | Some("Linear") => x,
        Some("Cubic") => min32(max32(x * x * (3.0 - 2.0 * x), 0.0), 1.0),
        Some("CircularOut") => max32(1.0 - (x - 1.0) * (x - 1.0), 0.0).sqrt(),
        Some(o) => {
            warn(&format!("montage blend option not ported: {o}"));
            x
        }
    }
}

/// `alphaToBlendOption`: FAlphaBlend::AlphaToBlendOption (exe 142816450) for Linear, Cubic and CircularOut; another
/// option is taken as Linear and named to `warn` (the owner's `warnOnce`). In double, for the view's own blends; the
/// instance's [`MontageState::shown_weight`] runs it in float32.
pub fn alpha_to_blend_option(option: Option<&str>, x: f64, warn: &dyn Fn(&str)) -> f64 {
    match option {
        None | Some("Linear") => x,
        Some("Cubic") => js::min(js::max(x * x * (3.0 - 2.0 * x), 0.0), 1.0),
        Some("CircularOut") => js::max(1.0 - (x - 1.0) * (x - 1.0), 0.0).sqrt(),
        Some(o) => {
            warn(&format!("montage blend option not ported: {o}"));
            x
        }
    }
}

/// `MontageProxy`: the play's PlayMontageCallbackProxy, by the values below.
pub type MontageProxy = u8;
/// No proxy (`''`).
pub const PROXY_NONE: MontageProxy = 0;
/// `'attack'`: TriggerCurrentAttack's play.
pub const PROXY_ATTACK: MontageProxy = 1;
/// `'branchAttack'`: a branch's play.
pub const PROXY_BRANCH_ATTACK: MontageProxy = 2;
/// `'evade'`: the dodge's play.
pub const PROXY_EVADE: MontageProxy = 3;
/// `'evadeDrop'`: the platform drop dodge's play.
pub const PROXY_EVADE_DROP: MontageProxy = 4;

/// A montage instance's state (`MontagePlayer`'s fields but its montage): `montage` names the montage the owner
/// plays (the sim's `Name`), which the owner resolves to its data for each call. `Clone` copies field by field, so a
/// save into a state of the same shape reuses `active`.
#[derive(Debug, PartialEq)]
pub struct MontageState<K> {
    pub montage: K,
    pub pos: f64,
    pub ended: bool,
    /// A Set in insertion order: the notify indices whose states are on.
    pub active: Vec<usize>,
    pub in_advance: bool,
    pub terminated: bool,
    pub blend_out_fired: bool,
    pub proxy: MontageProxy,
    pub interrupt_fired: bool,
    pub play_rate: f64,
    pub walking: bool,
    pub started: Started,
    pub age: f64,
    pub at_end: bool,
    pub blend_left: f64,
    pub blend_time: f64,
}

impl<K: Clone> Clone for MontageState<K> {
    fn clone(&self) -> Self {
        MontageState { montage: self.montage.clone(), active: self.active.clone(), ..*self }
    }

    fn clone_from(&mut self, o: &Self) {
        self.montage.clone_from(&o.montage);
        self.active.clone_from(&o.active);
        let MontageState {
            montage: _,
            active: _,
            pos,
            ended,
            in_advance,
            terminated,
            blend_out_fired,
            proxy,
            interrupt_fired,
            play_rate,
            walking,
            started,
            age,
            at_end,
            blend_left,
            blend_time,
        } = *o;
        self.pos = pos;
        self.ended = ended;
        self.in_advance = in_advance;
        self.terminated = terminated;
        self.blend_out_fired = blend_out_fired;
        self.proxy = proxy;
        self.interrupt_fired = interrupt_fired;
        self.play_rate = play_rate;
        self.walking = walking;
        self.started = started;
        self.age = age;
        self.at_end = at_end;
        self.blend_left = blend_left;
        self.blend_time = blend_time;
    }
}

impl<K> MontageState<K> {
    /// `new MontagePlayer(m, slotHint)` of montage `montage`: `ground` true for 'ground'.
    pub fn new(montage: K, ground: bool) -> MontageState<K> {
        MontageState {
            montage,
            pos: 0.0,
            ended: false,
            active: Vec::new(),
            in_advance: false,
            terminated: false,
            blend_out_fired: false,
            proxy: PROXY_NONE,
            interrupt_fired: false,
            play_rate: 1.0,
            walking: ground,
            started: Started::No,
            age: 0.0,
            at_end: false,
            blend_left: -1.0,
            blend_time: 0.0,
        }
    }
}

/// One call's player: the montage's data and the instance's state.
pub struct MontagePlayer<'a, K> {
    pub m: &'a Montage,
    pub s: &'a mut MontageState<K>,
}

/// A slot's track as `segAt` / `poseOf` see it: the slot's index, or none (an empty list).
pub type SegRef = (usize, usize);

impl<K> MontageState<K> {
    pub fn has(&self, i: usize) -> bool {
        self.active.contains(&i)
    }

    /// `fullWeight`: montage weight 1 (the blend-in is over and no blend-out has started).
    pub fn full_weight(&self, m: &Montage) -> bool {
        !self.blending_out() && self.age * (1.0 / 60.0) >= m.blend_in - 1e-6
    }

    /// `blendingOut`.
    pub fn blending_out(&self) -> bool {
        self.blend_left >= 0.0 || self.terminated
    }

    /// `weight`: the instance's blend weight 0..1 for a layer.
    pub fn weight(&self, m: &Montage) -> f64 {
        if self.terminated || self.ended {
            return 0.0;
        }
        // (tick counts: whole numbers, exact in float32; the weight a float as the build's blend)
        let age = f(self.age);
        let in_ticks = f(blend_ticks(m.blend_in));
        if self.blend_left < 0.0 {
            return f64::from(if in_ticks > 0.0 { min32(age / in_ticks, 1.0) } else { 1.0 });
        }
        let out_ticks = f(blend_ticks(self.blend_time));
        if !(out_ticks > 0.0) {
            return 0.0;
        }
        let left = f(self.blend_left);
        let at = max32(age - (out_ticks - left), 0.0);
        let from = if in_ticks > 0.0 { min32(at / in_ticks, 1.0) } else { 1.0 };
        f64::from(from * min32(left / out_ticks, 1.0))
    }

    /// `shownWeight`: the share of this instance's pose in the Slot node's output while its own blend-out runs (`warn`:
    /// the owner's `warnOnce`).
    pub fn shown_weight(&self, m: &Montage, warn: &dyn Fn(&str)) -> f64 {
        if self.terminated || self.ended {
            return 0.0;
        }
        if self.blend_left < 0.0 {
            return 1.0;
        }
        let n = f(blend_ticks(self.blend_time));
        if !(n > 0.0) {
            return 0.0;
        }
        f64::from(
            1.0 - alpha_to_blend_option32(
                m.blend_out_option.as_deref(),
                min32(max32((n - f(self.blend_left)) / n, 0.0), 1.0),
                warn,
            ),
        )
    }

    /// `slotFor`: DefaultSlot, else the first slot with Ground (walking) or Air in its name, else the first.
    fn slot_for(&self, m: &Montage, walking: bool) -> Option<usize> {
        let slots = &m.slots;
        let word = if walking { "ground" } else { "air" };
        slots
            .iter()
            .position(|s| s.slot == "DefaultSlot")
            .or_else(|| slots.iter().position(|s| s.slot.to_ascii_lowercase().contains(word)))
            .or(if slots.is_empty() { None } else { Some(0) })
    }

    /// `kickSplit`: both kick slots carry a track.
    pub fn kick_split(&self, m: &Montage) -> bool {
        let slots = &m.slots;
        let has = |w: &str| slots.iter().any(|s| s.slot.to_ascii_lowercase().contains(w));
        !slots.iter().any(|s| s.slot == "DefaultSlot") && has("ground") && has("air")
    }

    /// `sectionIndexAt`: the section at t, with the tolerance of the section-end cross test.
    pub fn section_index_at(&self, m: &Montage, t: f64) -> usize {
        let mut idx = 0;
        for (i, s) in m.sections.iter().enumerate() {
            if t >= s.t - EPS {
                idx = i;
            }
        }
        idx
    }

    /// `sectionEnd`.
    pub fn section_end(&self, m: &Montage, idx: usize) -> f64 {
        let s = &m.sections;
        let mut end = m.length;
        for o in s {
            if o.t > s[idx].t + EPS && o.t < end {
                end = o.t;
            }
        }
        end
    }

    /// `sectionStart`.
    pub fn section_start(&self, m: &Montage, name: &str) -> Option<f64> {
        m.sections.iter().find(|x| same_name(&x.name, name)).map(|s| s.t)
    }

    /// `poseIn`: the pose of one named slot's track: (slot, segment, time), None without a track for it.
    pub fn pose_in(&self, m: &Montage, slot: &str) -> Option<(usize, usize, f64)> {
        let si = m.slots.iter().position(|x| x.slot == slot)?;
        self.pose_of(m, Some(si))
    }

    /// `pose`: the current animation (slot, segment) and its time for the pose; `walking` None takes the
    /// player's own.
    pub fn pose(&self, m: &Montage, walking: Option<bool>) -> Option<(usize, usize, f64)> {
        self.pose_of(m, self.slot_for(m, walking.unwrap_or(self.walking)))
    }

    fn pose_of(&self, m: &Montage, slot: Option<usize>) -> Option<(usize, usize, f64)> {
        let si = slot?;
        let gi = self.seg_at(&m.slots[si].segments)?;
        Some((si, gi, self.seg_time(&m.slots[si].segments[gi])))
    }

    /// `segAt`: the track's segment at the position (its index), None for an empty track.
    pub fn seg_at(&self, segs: &[Segment]) -> Option<usize> {
        if segs.is_empty() {
            return None;
        }
        let mut seg = 0;
        for (i, s) in segs.iter().enumerate() {
            if self.pos >= s.start - EPS {
                seg = i;
            }
        }
        Some(seg)
    }

    /// `segTime`: the time in the segment's anim.
    // Unverified: float32 in a fixed order of operations (FAnimSegment::ConvertTrackPosToAnimPos unread in the exe);
    // the loop wrap is `%` (fmodf, exact).
    pub fn seg_time(&self, seg: &Segment) -> f64 {
        let (a0, a1) = (f(seg.a0), f(seg.a1));
        let span = (a1 - a0).abs();
        let mut local = (f(self.pos) - f(seg.start)) * f(seg.rate);
        if seg.loops > 1.0 && span > 0.0 {
            local %= span;
        }
        local = min32(max32(local, 0.0), span);
        f64::from(if a1 >= a0 { a0 + local } else { a0 - local })
    }

    /// `stopBlend` (FAnimMontageInstance::Stop FUN_142865260 with a blend time): (left, time).
    pub fn stop_blend(&self, t: impl Into<f64>) -> (f64, f64) {
        let t: f64 = t.into();
        if self.blend_left >= 0.0 && !(t < self.blend_time) {
            return (self.blend_left, self.blend_time);
        }
        (blend_ticks(t), t)
    }

    /// Montage_Stop's blend (FUN_1428412a0, spec 07 items 17 and 21): an instance already stopped (its automatic
    /// blend-out running) is not reached, so its blend-out keeps its time; else `stop_blend`. Only Montage_PlayInternal's
    /// group stop shortens a running blend-out (`stop_blend`).
    pub fn montage_stop_blend(&self, t: impl Into<f64>) -> (f64, f64) {
        if self.blend_left >= 0.0 {
            return (self.blend_left, self.blend_time);
        }
        self.stop_blend(t)
    }

    fn is_bp(&self, m: &Montage, i: usize) -> bool {
        m.notifies[i].tick == NotifyTick::BranchingPoint
    }

    fn active_of(&self, m: &Montage, bp: bool) -> Vec<usize> {
        let mut v: Vec<usize> = self.active.iter().copied().filter(|&i| self.is_bp(m, i) == bp).collect();
        v.sort_unstable();
        v
    }

    /// The next section's start of section `si` (`next && next !== 'None' ? sectionStart(next) : null`).
    fn next_start(&self, m: &Montage, si: usize) -> Option<f64> {
        match &m.sections[si].next {
            Some(n) if !n.is_empty() && n != "None" => self.section_start(m, n),
            _ => None,
        }
    }

    /// `keptAtEnd`: a branching-point marker at the section end in float32 keeps the step there.
    fn kept_at_end(&self, m: &Montage, lo: f64, end: f64) -> bool {
        if !(end < m.length) {
            return false;
        }
        let fe = js::fround(end);
        for n in &m.notifies {
            if n.tick != NotifyTick::BranchingPoint {
                continue;
            }
            let t0 = js::max(n.t, 0.0);
            if t0 > lo && js::fround(t0) == fe {
                return true;
            }
            if n.state && t0 + n.dur > lo && js::fround(t0 + n.dur) == fe {
                return true;
            }
        }
        false
    }

    /// `inside` of branchingPoints' sync: the position q is inside state i.
    fn inside(&self, m: &Montage, i: usize, q: f64) -> bool {
        let n = &m.notifies[i];
        let t0 = js::max(n.t, 0.0);
        q > t0 + EPS && q <= t0 + n.dur + EPS
    }
}

fn add<K>(s: &mut MontageState<K>, i: usize) {
    if !s.has(i) {
        s.active.push(i);
    }
}

fn delete<K>(s: &mut MontageState<K>, i: usize) {
    if let Some(k) = s.active.iter().position(|&x| x == i) {
        s.active.remove(k);
    }
}

/// An instance as the player's algorithm sees it: the montage, the state (which the host may find anew on each
/// call, so a handler can move it) and the owner's notify handlers. [`Play`] is the algorithm over it.
pub trait Host<'m, K> {
    fn m(&self) -> &'m Montage;
    fn s(&mut self) -> &mut MontageState<K>;
    fn begin(&mut self, idx: usize);
    fn end(&mut self, idx: usize);
    fn instant(&mut self, idx: usize);
    /// The sink has `rootStep` (a forward step of the position inside Advance, before that step's notifies).
    fn has_root_step(&self) -> bool {
        false
    }

    fn root_step(&mut self, _a: f64, _b: f64) {}
    /// The sink has `passEnd`.
    fn has_pass_end(&self) -> bool {
        false
    }

    fn pass_end(&mut self, _idx: usize) {}
}

/// The `MontagePlayer` methods that run notify handlers, over any [`Host`].
pub trait Play<'m, K>: Host<'m, K> {
    /// `advance`: one frame (weight update, Advance unless `skip`, the queued dispatch). False once the montage is
    /// over.
    fn advance(&mut self, dt: impl Into<f64>, skip: bool, dilated: bool) -> bool {
        match self.advance_only(dt, skip, dilated) {
            Some(h) => self.dispatch_advanced(&h),
            None => false,
        }
    }

    /// `advanceOnly`: the weight update and Advance, with its branching-point notifies.
    fn advance_only(&mut self, dt: impl Into<f64>, skip: bool, dilated: bool) -> Option<Advanced> {
        let dt: f64 = dt.into();
        let m = self.m();
        if self.s().ended {
            return None;
        }
        let gone = self.s().terminated;
        let mut ranges: Vec<Step> = Vec::new();
        if !gone {
            if !dilated {
                let s = self.s();
                s.age += 1.0;
                if s.blend_left > 0.0 {
                    s.blend_left -= 1.0;
                }
            }
            if !skip {
                self.s().in_advance = true;
                let ok = self.s().at_end || self.step_move(dt, &mut ranges);
                self.s().in_advance = false;
                if !ok {
                    return None;
                }
                if self.s().blend_left == 0.0 {
                    // Terminate: BranchingPointNotifyEnd for the active states, last first.
                    self.s().terminated = true;
                    for i in self.s().active_of(m, true).into_iter().rev() {
                        delete(self.s(), i);
                        self.end(i);
                        if self.s().ended {
                            return None;
                        }
                    }
                }
            }
        }
        Some(Advanced { ranges, gone })
    }

    /// `dispatchAdvanced`: the queued dispatch of the steps an `advance_only` made.
    fn dispatch_advanced(&mut self, h: &Advanced) -> bool {
        if self.s().ended {
            return false;
        }
        if !self.dispatch(&h.ranges) {
            return false;
        }
        if h.gone {
            self.s().ended = true;
            return false;
        }
        true
    }

    fn stop(&mut self) {
        if !self.s().ended {
            self.finish();
        }
    }

    /// `endQueued`: every queued state ends (in list order).
    fn end_queued(&mut self) {
        let ns = &self.m().notifies;
        let mut i = 0;
        while i < ns.len() && !self.s().active.is_empty() {
            if self.s().has(i) && ns[i].tick != NotifyTick::BranchingPoint {
                delete(self.s(), i);
                self.end(i);
            }
            i += 1;
        }
    }

    /// `endBranching`: BranchingPointNotifyEnd for the active states, last first.
    fn end_branching(&mut self) {
        let ns = &self.m().notifies;
        let mut i = ns.len();
        while i > 0 && !self.s().active.is_empty() {
            i -= 1;
            if self.s().has(i) && ns[i].tick == NotifyTick::BranchingPoint {
                delete(self.s(), i);
                self.end(i);
            }
        }
    }

    #[doc(hidden)]
    fn finish(&mut self) {
        self.s().ended = true;
        let ns = &self.m().notifies;
        let mut list = self.s().active.clone();
        // (Array.prototype.sort with x.t - y.t: stable, a NaN difference as equal)
        list.sort_by(|&x, &y| cmp_num(ns[x].t - ns[y].t));
        for i in list {
            delete(self.s(), i);
            self.end(i);
        }
    }

    /// `move`: the position steps of Advance, with section links. False if a branching-point handler stopped the
    /// montage.
    #[doc(hidden)]
    fn step_move(&mut self, dt: f64, ranges: &mut Vec<Step>) -> bool {
        let m = self.m();
        let mut a = self.s().pos;
        let mut lo = a + EPS;
        if self.s().started == Started::No {
            a = -1.0; // notifies at t <= 0 fire on the first tick
            lo = -1.0;
        } else if self.s().started == Started::Jump {
            lo = a;
        }
        self.s().started = Started::Yes;
        // FMontageSubStepper: the step's move in float32, DeltaTime x (PlayRate x RateScale), onto the Position
        let rate = f(m.rate_scale) * f(self.s().play_rate);
        let mut p = f64::from(f(self.s().pos) + f(dt) * rate);
        let mut guard = 0;
        loop {
            let (end, ns) = {
                let s = self.s();
                let si = s.section_index_at(m, js::max(s.pos, 0.0));
                (s.section_end(m, si), s.next_start(m, si))
            };
            let cross = p >= end - EPS && guard <= 8;
            let b = if cross { end } else { p };
            let clamp = if cross { 1.0 } else { 0.0 };
            ranges.push([a, b, lo, clamp]);
            if !self.branching_points(lo, b, clamp) {
                return false;
            }
            if self.s().pos != b {
                return true; // a handler moved the position: no section handling this frame
            }
            if ns.is_none() && self.s().blend_left < 0.0 {
                // Automatic blend-out. Without it: the end of the last section stops the instance.
                let trigger = f(if m.blend_out_trigger_time >= 0.0 { m.blend_out_trigger_time } else { m.blend_out });
                if if m.auto_blend_out { (f(end) - f(b)) / rate <= max32(trigger, 1e-4) } else { cross } {
                    let s = self.s();
                    s.blend_time = m.blend_out;
                    s.blend_left = blend_ticks(m.blend_out);
                }
            }
            if !cross {
                return true;
            }
            if self.s().kept_at_end(m, lo, end) {
                // No link: the step stopped on a marker at the end.
                if !(p - end > EPS) {
                    return true;
                }
                a = end;
                lo = end + EPS;
                guard += 1;
                continue;
            }
            let Some(ns) = ns else {
                self.s().at_end = true;
                return true;
            };
            let over = f(p) - f(end);
            self.s().pos = ns;
            a = ns;
            lo = ns;
            p = f64::from(f(ns) + over);
            guard += 1;
        }
    }

    /// branchingPoints' `sync`: the active states matched to the position (UpdateActiveStateBranchingPoints).
    #[doc(hidden)]
    fn sync(&mut self, q: f64, skip: &[usize]) -> bool {
        let m = self.m();
        for i in self.s().active_of(m, true).into_iter().rev() {
            if skip.contains(&i) || self.s().inside(m, i, q) {
                continue;
            }
            delete(self.s(), i);
            self.end(i);
            if self.s().ended {
                return false;
            }
        }
        for i in 0..m.notifies.len() {
            let n = &m.notifies[i];
            if n.tick != NotifyTick::BranchingPoint
                || !n.state
                || skip.contains(&i)
                || self.s().has(i)
                || !self.s().inside(m, i, q)
            {
                continue;
            }
            add(self.s(), i);
            self.begin(i);
            if self.s().ended {
                return false;
            }
        }
        true
    }

    /// `branchingPoints`: the branching points of one step (a, b]: the position stops at each marker in time order.
    #[doc(hidden)]
    fn branching_points(&mut self, lo: f64, b: f64, clamp: f64) -> bool {
        let ns = &self.m().notifies;
        // (t, notify, kind: 0 begin, 1 end, 2 instant)
        let mut marks: Vec<(f64, usize, u8)> = Vec::new();
        let mut any = false;
        for (i, n) in ns.iter().enumerate() {
            if n.tick != NotifyTick::BranchingPoint {
                continue;
            }
            any = true;
            let t0 = js::max(n.t, 0.0);
            if !n.state {
                if t0 > lo && up_to(t0, b, clamp) {
                    marks.push((t0, i, 2));
                }
                continue;
            }
            let t1 = t0 + n.dur;
            if t0 > lo && up_to(t0, b, clamp) {
                marks.push((t0, i, 0));
            }
            if t1 > lo && up_to(t1, b, clamp) {
                marks.push((t1, i, 1));
            }
        }
        if !any {
            self.to(b);
            return true;
        }
        marks.sort_by(|x, y| cmp_num(x.0 - y.0));
        let mut here: Vec<usize> = Vec::new();
        let mut k = 0;
        while k < marks.len() {
            let q = marks[k].0;
            let mut e = k;
            while e < marks.len() && marks[e].0 - q <= EPS {
                e += 1;
            }
            here.clear();
            here.extend(marks[k..e].iter().map(|x| x.1));
            self.to(js::min(q, b));
            if !self.sync(q, &here) {
                return false;
            }
            while k < e {
                let (_, i, kind) = marks[k];
                if kind == 2 {
                    self.instant(i);
                } else if kind == 0 {
                    if !self.s().has(i) {
                        add(self.s(), i);
                        self.begin(i);
                    }
                } else if self.s().has(i) {
                    delete(self.s(), i);
                    self.end(i);
                }
                if self.s().ended {
                    return false;
                }
                if self.s().pos != js::min(q, b) {
                    return true;
                }
                k += 1;
            }
        }
        if marks.is_empty() || b - marks[marks.len() - 1].0 > EPS {
            self.to(b);
            if !self.sync(b, &[]) {
                return false;
            }
        }
        self.to(b);
        true
    }

    /// `to`: the position to q inside Advance; a forward step is offered to the sink first.
    #[doc(hidden)]
    fn to(&mut self, q: f64) {
        if q > self.s().pos && self.has_root_step() {
            let a = self.s().pos;
            self.root_step(a, q);
        }
        self.s().pos = q;
    }

    /// `dispatch`: TriggerAnimNotifies for the steps of this frame. False if a handler stopped the montage.
    #[doc(hidden)]
    fn dispatch(&mut self, ranges: &[Step]) -> bool {
        let m = self.m();
        let ns = &m.notifies;
        let mut want: Vec<usize> = Vec::new();
        let mut instants: Vec<usize> = Vec::new();
        for (i, n) in ns.iter().enumerate() {
            if n.tick == NotifyTick::BranchingPoint {
                continue;
            }
            let t0 = js::max(n.t, 0.0);
            if n.state {
                let t1 = t0 + n.dur;
                if ranges.iter().any(|&[a, b, _, clamp]| up_to(t0, b, clamp) && t1 > a + 2.0 * EPS) {
                    want.push(i);
                }
            } else if ranges.iter().any(|&[_, b, lo, clamp]| t0 > lo && up_to(t0, b, clamp)) {
                instants.push(i);
            }
        }
        // the old list and the states this step queues that it lacks, taken before any handler runs
        let old = self.s().active_of(m, false);
        let begins: Vec<usize> = want.iter().copied().filter(|&i| !self.s().has(i)).collect();
        for i in instants {
            self.instant(i);
            // Deviation: the instants after one that stopped the montage are dropped.
            if self.s().ended {
                break;
            }
        }
        for i in old {
            if want.contains(&i) {
                continue;
            }
            if self.s().has(i) {
                delete(self.s(), i);
                self.end(i);
            } else if self.s().ended && self.has_pass_end() {
                self.pass_end(i);
            }
        }
        for i in begins {
            if self.s().has(i) {
                continue;
            }
            add(self.s(), i);
            self.begin(i);
        }
        !self.s().ended
    }
}

impl<'m, K, H: Host<'m, K> + ?Sized> Play<'m, K> for H {}

/// A [`MontagePlayer`] with a [`NotifySink`] as a [`Host`].
struct WithSink<'p, 'a, K> {
    p: &'p mut MontagePlayer<'a, K>,
    sink: &'p mut dyn NotifySink<K>,
}

impl<'a, K> Host<'a, K> for WithSink<'_, 'a, K> {
    fn m(&self) -> &'a Montage {
        self.p.m
    }
    fn s(&mut self) -> &mut MontageState<K> {
        self.p.s
    }
    fn begin(&mut self, idx: usize) {
        self.sink.begin(self.p, idx);
    }
    fn end(&mut self, idx: usize) {
        self.sink.end(self.p, idx);
    }
    fn instant(&mut self, idx: usize) {
        self.sink.instant(self.p, idx);
    }
    fn has_root_step(&self) -> bool {
        self.sink.has_root_step()
    }
    fn root_step(&mut self, a: f64, b: f64) {
        self.sink.root_step(self.p, a, b);
    }
    fn has_pass_end(&self) -> bool {
        self.sink.has_pass_end()
    }
    fn pass_end(&mut self, idx: usize) {
        self.sink.pass_end(self.p, idx);
    }
}

impl<'a, K> MontagePlayer<'a, K> {
    /// `advance` with a sink (see [`Play::advance`]).
    pub fn advance(&mut self, dt: impl Into<f64>, sink: &mut dyn NotifySink<K>, skip: bool, dilated: bool) -> bool {
        WithSink { p: self, sink }.advance(dt, skip, dilated)
    }

    pub fn advance_only(
        &mut self,
        dt: impl Into<f64>,
        sink: &mut dyn NotifySink<K>,
        skip: bool,
        dilated: bool,
    ) -> Option<Advanced> {
        WithSink { p: self, sink }.advance_only(dt, skip, dilated)
    }

    pub fn dispatch_advanced(&mut self, h: &Advanced, sink: &mut dyn NotifySink<K>) -> bool {
        WithSink { p: self, sink }.dispatch_advanced(h)
    }

    /// `jumpToSection` (Montage_JumpToSection): the position only.
    pub fn jump_to_section(&mut self, name: &str) -> bool {
        self.s.jump_to_section(self.m, name)
    }

    /// `jumpToSectionEnd` (Montage_JumpToSectionsEnd): just before the section end.
    pub fn jump_to_section_end(&mut self, idx: usize) {
        self.s.jump_to_section_end(self.m, idx)
    }

    pub fn stop(&mut self, sink: &mut dyn NotifySink<K>) {
        WithSink { p: self, sink }.stop()
    }

    /// `halt`: FAnimMontageInstance::Stop with bInterrupt, the states left on.
    pub fn halt(&mut self) {
        self.s.ended = true;
    }

    pub fn drift(&mut self, dt: impl Into<f64>) {
        self.s.drift(self.m, dt)
    }

    pub fn end_queued(&mut self, sink: &mut dyn NotifySink<K>) {
        WithSink { p: self, sink }.end_queued()
    }

    pub fn end_branching(&mut self, sink: &mut dyn NotifySink<K>) {
        WithSink { p: self, sink }.end_branching()
    }
}

impl<K> MontageState<K> {
    /// `jumpToSection` (Montage_JumpToSection): the position only.
    pub fn jump_to_section(&mut self, m: &Montage, name: &str) -> bool {
        let Some(t) = self.section_start(m, name) else { return false };
        self.pos = t;
        self.at_end = false;
        if self.started != Started::No {
            self.started = Started::Jump;
        }
        true
    }

    /// `jumpToSectionEnd` (Montage_JumpToSectionsEnd): just before the section end.
    pub fn jump_to_section_end(&mut self, m: &Montage, idx: usize) {
        self.pos = f64::from(f(self.section_end(m, idx)) - 1e-4);
        self.at_end = false;
        if self.started != Started::No {
            self.started = Started::Jump;
        }
    }

    /// `drift`: one Advance of a halted instance (the position with its section links and no events).
    pub fn drift(&mut self, m: &Montage, dt: impl Into<f64>) {
        let dt: f64 = dt.into();
        if self.at_end {
            return;
        }
        let rate = f(m.rate_scale) * f(self.play_rate);
        let mut p = f64::from(f(self.pos) + f(dt) * rate);
        let mut lo = self.pos + EPS;
        let mut guard = 0;
        loop {
            let si = self.section_index_at(m, js::max(self.pos, 0.0));
            let end = self.section_end(m, si);
            if !(p >= end - EPS && guard <= 8) {
                self.pos = p;
                return;
            }
            if self.kept_at_end(m, lo, end) {
                self.pos = end;
                if !(p - end > EPS) {
                    return;
                }
                lo = end + EPS;
                guard += 1;
                continue;
            }
            let Some(ns) = self.next_start(m, si) else {
                self.pos = end;
                self.at_end = true;
                return;
            };
            let over = f(p) - f(end);
            self.pos = ns;
            lo = ns;
            p = f64::from(f(ns) + over);
            guard += 1;
        }
    }
}

/// A JavaScript sort comparator's number as an Ordering (negative Less, positive Greater, zero or NaN Equal).
fn cmp_num(d: f64) -> std::cmp::Ordering {
    if d < 0.0 {
        std::cmp::Ordering::Less
    } else if d > 0.0 {
        std::cmp::Ordering::Greater
    } else {
        std::cmp::Ordering::Equal
    }
}
