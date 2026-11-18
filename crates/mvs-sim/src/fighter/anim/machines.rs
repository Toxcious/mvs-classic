//! Base_AnimBP's state machines: the transitions of MovementAndHitReact, Movement, Ground with
//! IdleSubState and RunSubState, AirState and C017_AnimBP's hover states, their active transition lists
//! (`smActive`), the entry of a state (`setGs`) and the hit reaction poses.

use super::update::{Parts, anim_fast_falling, anim_len, grounded, idle_anim, nm, nmo, parts_of, set_loco};
use super::{GroundState, RunSubState, SetLocoOpt, SmBlend, fall_anim, fall_rate};
use crate::Real;
use crate::consts;
use crate::fighter::FighterId;
use crate::fighter::{anim_is_attacking, in_hitstun, is_montage_playing};
use crate::math;
use crate::name;
use crate::names::Name;
use crate::world::World;
use indexmap::IndexMap;
use mvs_data::def::EdgeIdle;

/// `AIR_STATES`: AirState's states.
pub const AIR_STATES: [GroundState; 4] =
    [GroundState::Fall, GroundState::Jump1, GroundState::Jump2, GroundState::FastFall];

/// `RUN_SUB`: RunSubState's states.
const RUN_SUB: [GroundState; 4] =
    [GroundState::Run, GroundState::CrouchRun, GroundState::CrouchRunIn, GroundState::CrouchIntoCrouchRun];

/// `EDGE_STATES`: the IdleSubState states whose player is a BlendListByBool(OnEdge && UseEdgeIdle) (`edgeIdleOf`).
const EDGE_STATES: [GroundState; 3] = [GroundState::Idle, GroundState::IdleReset, GroundState::Crouch];

pub(crate) fn is_air(s: GroundState) -> bool {
    AIR_STATES.contains(&s)
}

fn is_run_sub(s: GroundState) -> bool {
    RUN_SUB.contains(&s)
}

/// A state's name as the GroundState string: the key of `smTimes` and an end of a `smActive` entry.
pub(crate) fn gs_name(s: GroundState) -> Name {
    match s {
        GroundState::Fall => name!("fall"),
        GroundState::Jump1 => name!("jump1"),
        GroundState::Jump2 => name!("jump2"),
        GroundState::FastFall => name!("fastFall"),
        GroundState::Wall => name!("wall"),
        GroundState::HitStunned => name!("hitStunned"),
        GroundState::Recovery => name!("recovery"),
        GroundState::Bounce => name!("bounce"),
        GroundState::HitWobble => name!("hitWobble"),
        GroundState::A2g => name!("a2g"),
        GroundState::Entry => name!("entry"),
        GroundState::Run => name!("run"),
        GroundState::RunStop => name!("runStop"),
        GroundState::Idle => name!("idle"),
        GroundState::Crouch => name!("crouch"),
        GroundState::AltEnter => name!("altEnter"),
        GroundState::AltLoop => name!("altLoop"),
        GroundState::IdleReset => name!("idleReset"),
        GroundState::RunStop2 => name!("runStop2"),
        GroundState::CrouchIn => name!("crouchIn"),
        GroundState::CrouchOut => name!("crouchOut"),
        GroundState::HoverIn => name!("hoverIn"),
        GroundState::HoverLoop => name!("hoverLoop"),
        GroundState::CrouchRun => name!("crouchRun"),
        GroundState::CrouchRunIn => name!("crouchRunIn"),
        GroundState::CrouchIntoCrouchRun => name!("crouchIntoCrouchRun"),
        GroundState::CrouchRunIntoCrouch => name!("crouchRunIntoCrouch"),
    }
}

fn run_sub_of(s: GroundState) -> RunSubState {
    match s {
        GroundState::CrouchRun => RunSubState::CrouchRun,
        GroundState::CrouchRunIn => RunSubState::CrouchRunIn,
        GroundState::CrouchIntoCrouchRun => RunSubState::CrouchIntoCrouchRun,
        _ => RunSubState::Run,
    }
}

/// `setGs`'s options.
#[derive(Clone, Copy, Debug, Default)]
pub struct GsOpt {
    pub linear: bool,
    pub rate: Option<Real>,
}

fn rate(r: Real) -> GsOpt {
    GsOpt { linear: false, rate: Some(r) }
}

const NO_OPT: GsOpt = GsOpt { linear: false, rate: None };

/// `edgeOn`: the instance's OnEdge && UseEdgeIdle: the set's edge players.
fn edge_on<'d>(w: &World, f: FighterId, p: &Parts<'d>) -> Option<&'d EdgeIdle> {
    if w.fighter(f).platforms.on_edge == Some(true) { p.ex.edge_idle.as_ref() } else { None }
}

/// `setGs`: FAnimNode_StateMachine::SetState (exe 1428634c0): the machine's elapsed time is
/// 0, the entered state's players have their cached blend weight cleared (gsRel 0 until the state's update writes
/// it: update_pose), and the state is initialised (its player back at the start) only when it has no weight in the
/// machine's active transitions at that moment (GetStateWeight <= 0) or has bAlwaysResetOnEntry. Ported for AirState
/// (both states in it; bAlwaysResetOnEntry is on Jump1 alone, data): a state still blending out, or left earlier in
/// this same update, goes on from the time its player has (smTimes). Every other entry restarts the clip (the other
/// machines: not gone through). UseEdgeIdle: the state's blend list shows its edge child (both children start at 0
/// with the state).
pub fn set_gs(w: &mut World, f: FighterId, s: GroundState, a: Name, blend: Real, opt: GsOpt) {
    let d = w.data.clone();
    let p = parts_of(&d, w, f);
    let from = w.fighter(f).sm.gs;
    let (blend, enter_w) = sm_transition(w, f, from, s, blend, !opt.linear);
    let mut time = None;
    let fi = w.fighter_mut(f);
    if is_air(from) && is_air(s) {
        let lt = fi.anim.loco_time;
        fi.sm.sm_times.insert(gs_name(from), lt);
        if enter_w > 0.0 && s != GroundState::Jump1 {
            time = fi.sm.sm_times.get(&gs_name(s)).copied();
        }
        fi.sm.sm_times.shift_remove(&gs_name(s));
    }
    fi.sm.gs_kept = time.is_some();
    fi.sm.gs = s;
    if is_run_sub(s) && p.ex.crouch_run.is_some() {
        fi.sm.run_sub = Some(run_sub_of(s));
    }
    fi.sm.gs_time = 0.0;
    fi.sm.gs_rel = 0.0;
    let mut looping = match s {
        GroundState::A2g | GroundState::HoverIn => Some(false),
        GroundState::HoverLoop => Some(true),
        _ => None,
    };
    let mut a = a;
    if EDGE_STATES.contains(&s) && p.ex.edge_idle.is_some() {
        if let Some(t) = &mut w.fighter_mut(f).sm.edge_times {
            t.clear();
        }
        if let Some(e) = edge_on(w, f, &p) {
            a = nm(&w.names, if s == GroundState::Crouch { &e.crouch } else { &e.idle });
            looping = Some(true);
        }
    }
    let o = SetLocoOpt { restart: true, linear: opt.linear, time, looping };
    set_loco(w, f, a, opt.rate.unwrap_or(1.0), Some(blend), Some(o));
}

/// `edgeBlend`: the edge blend lists of IdleSubState's Idle / IdleReset (Anim_Edge_Idle over
/// Anim_Idle) and Crouch (Anim_Crouch_Edge over Anim_Crouch) when OnEdge changes inside the state: BlendTime 0.1 each
/// way, Linear, bResetChildOnActivation false, so the child coming in resumes the time it had (a child at weight 0 is
/// not updated; both start at 0 with the state, set_gs). Both looped.
/// Deviation: the child going out is the frozen pose of the port's one crossfade (it is updated while its weight is
/// above 0 in the build), and a switch back inside the 0.1 s is a new switch of the full time.
fn edge_blend(w: &mut World, f: FighterId) {
    let s = w.fighter(f).sm.gs;
    if !EDGE_STATES.contains(&s) {
        return;
    }
    let d = w.data.clone();
    let p = parts_of(&d, w, f);
    let Some(e) = &p.ex.edge_idle else { return };
    let on = w.fighter(f).platforms.on_edge == Some(true);
    let want = if s == GroundState::Crouch {
        if on { nm(&w.names, &e.crouch) } else { nmo(&w.names, &p.an.crouch) }
    } else if on {
        nm(&w.names, &e.idle)
    } else {
        idle_anim(w, f)
    };
    let fi = w.fighter_mut(f);
    if fi.anim.loco_anim == want {
        return;
    }
    let (la, lt, rate) = (fi.anim.loco_anim, fi.anim.loco_time, fi.anim.loco_rate);
    // the child going out stops at its time
    let times = fi.sm.edge_times.get_or_insert_with(IndexMap::new);
    times.insert(la, lt);
    let t = times.get(&want).copied().unwrap_or(0.0);
    let o = SetLocoOpt { restart: true, linear: true, time: Some(t), looping: Some(true) };
    set_loco(w, f, want, rate, Some(0.1), Some(o));
}

/// `hoverMachine`: a character AnimBP's hover states around the base layer (CharDef.anim.hover;
/// C017_AnimBP). Its machine MovementAndHover has the states Movement (the linked base layer) and Hover (the machine
/// HoverState): Movement -> Hover on IsHovering (TransitionResult_1, 0.05 s, HermiteCubic), Hover -> Movement on
/// !IsHovering (TransitionResult_2, 0.2 s, HermiteCubic). IsHovering is written by BPI_SetIsGliding (@15253) from
/// Pawn_C017's SetHoverValueMulticast and ReadHoverInput (@2518, @3173) with isGliding at each change, so it is
/// `gliding`. HoverState (reinitialised when it becomes relevant): HoverIn (not looped) -> HoverLoop (looped) at 0 s
/// once GetRelevantAnimTimeRemaining(12, 0) <= 0 (@12642). True: the tick's states are the hover's.
/// Deviation: both states are BlendSpacePlayers on X = HoverFallLean (ubergraph @2267-2972 / @2999-3755); a
/// BlendSpace1D of several samples has no decoded rule, so the pose is the sample at 0 (what the target is at rest)
/// and he does not lean while he flies sideways.
/// Deviation: leaving the hover in the air goes to Fall with the 0.2 s of Hover -> Movement (the base layer's
/// machines start again at their entry states and reach Fall a frame later, their own crossfade under that one); on
/// the ground to the Ground machine's entry state. In hitstun the hit machine takes over from here.
pub fn hover_machine(w: &mut World, f: FighterId) -> bool {
    let d = w.data.clone();
    let p = parts_of(&d, w, f);
    let Some(h) = &p.an.hover else { return false };
    let s = w.fighter(f).sm.gs;
    let on = matches!(s, GroundState::HoverIn | GroundState::HoverLoop);
    if crate::chars::iron_giant::giant_gliding(w, f) {
        if !on {
            set_gs(w, f, GroundState::HoverIn, nm(&w.names, &h.enter), 0.05, NO_OPT);
        } else if s == GroundState::HoverIn && anim_len(&p, &h.enter) - w.fighter(f).anim.loco_time <= 0.0 {
            set_gs(w, f, GroundState::HoverLoop, nm(&w.names, &h.looping), 0.0, NO_OPT);
        }
        return true;
    }
    if !on || in_hitstun(w, f) {
        return false;
    }
    if grounded(w.fighter(f)) {
        set_gs(w, f, GroundState::Entry, nmo(&w.names, &p.an.idle), 0.2, NO_OPT);
    } else {
        let (a, r) = (fall_anim(w, f), fall_rate(w, f));
        set_gs(w, f, GroundState::Fall, a, 0.2, rate(r));
    }
    true
}

/// A state's place in the machines: the outer machine's states down to it.
fn path(g: GroundState, crouch_run: bool) -> ([Name; 4], usize) {
    let n = gs_name(g);
    let none = Name::NONE;
    match g {
        GroundState::HoverIn | GroundState::HoverLoop => ([name!("Hover"), n, none, none], 2),
        GroundState::HitStunned | GroundState::Recovery | GroundState::Bounce | GroundState::HitWobble => {
            ([n, none, none, none], 1)
        }
        g if is_air(g) => ([name!("Movement"), name!("Air"), n, none], 3),
        GroundState::A2g | GroundState::Wall => ([name!("Movement"), n, none, none], 2),
        // RunSubState's states are inside Ground's Run (its machine keyed 'run' in smActive); a set without
        // UseCrouchRun never leaves its Run state, which is then Ground's Run alone.
        g if is_run_sub(g) && (g != GroundState::Run || crouch_run) => {
            ([name!("Movement"), name!("Ground"), name!("run"), n], 4)
        }
        _ => ([name!("Movement"), name!("Ground"), n, none], 3),
    }
}

/// `smTransition`: FAnimNode_StateMachine::Update_AnyThread (exe 142867fa0): a taken
/// transition appends an active transition whose duration is CrossfadeDuration x InverseAlpha(1 -
/// GetStateWeight(next)) (GetStateWeight 142856d90; HermiteCubic: x^3 * 4/3 - 2 x^2 + x * 5/3, else clamped x), so
/// going back to a state that is still blending out takes less time. Each update adds DeltaTime to every active
/// transition (the new one included), and finished ones are removed up to the last finished. The machine is the
/// outermost level at which the two states differ (MovementAndHitReact, Movement, then Ground or AirState).
/// Returns the duration and the GetStateWeight of the entered state as SetState reads it for an AirState transition
/// (`smEnterW`; 0 for another machine).
/// Deviation: the machines under the entered state are taken to start with no active transition
/// (bReinitializeOnBecomingRelevant; a machine still blending out when re-entered would keep them).
pub fn sm_transition(
    w: &mut World,
    f: FighterId,
    from: GroundState,
    to: GroundState,
    dur: Real,
    hermite: bool,
) -> (Real, Real) {
    let d = w.data.clone();
    let cr = parts_of(&d, w, f).ex.crouch_run.is_some();
    let (pf, nf) = path(from, cr);
    let (pt, nt) = path(to, cr);
    let mut i = 0;
    while i < nf && i < nt && pf[i] == pt[i] {
        i += 1;
    }
    if i >= nf || i >= nt {
        return (dur, 0.0);
    }
    let mach = if i == 0 { name!("Top") } else { pf[i - 1] };
    let active = &mut w.fighter_mut(f).sm.sm_active;
    for k in &pt[i..nt - 1] {
        active.shift_remove(k);
    }
    let list = active.entry(mach).or_default();
    let mut wt = 0.0;
    for (k, t) in list.iter().enumerate() {
        if k > 0 {
            wt *= 1.0 - t.alpha;
        } else if t.prev == pt[i] {
            wt += 1.0 - t.alpha;
        }
        if t.next == pt[i] {
            wt += t.alpha;
        }
    }
    let x = 1.0 - crate::math::min(crate::math::max(wt, 0.0), 1.0);
    let d = dur * if hermite { (x * x * x * 4.0) / 3.0 - 2.0 * x * x + (x * 5.0) / 3.0 } else { x };
    list.push(SmBlend { prev: pf[i], next: pt[i], el: 0.0, dur: d, hermite, alpha: 0.0 });
    // SetState reads GetStateWeight(next) with the new entry appended at Alpha 0, which adds nothing to wt.
    (d, if mach == name!("Air") { wt } else { 0.0 })
}

/// `smAdvance`: each update adds DeltaTime to the active transitions and updates their
/// states. Returns the weight AirState's current state `cur` is updated with (a state is updated once, by the first
/// entry that names it, the previous state at 1 - Alpha and the next at Alpha; with no entry naming it, at the
/// machine's weight, taken as 1: the port holds no weight for the machines above).
pub fn sm_advance(w: &mut World, f: FighterId, cur: Option<GroundState>) -> Real {
    let cur = cur.map(gs_name);
    let mut rel = 1.0;
    let active = &mut w.fighter_mut(f).sm.sm_active;
    for (m, list) in active.iter_mut() {
        let mut done = None;
        let mut named = *m != name!("Air") || cur.is_none();
        for (k, t) in list.iter_mut().enumerate() {
            t.el += consts::DT;
            let u = if t.dur > 0.0 { crate::math::min(t.el / t.dur, 1.0) } else { 1.0 };
            t.alpha = if t.hermite { u * u * (3.0 - 2.0 * u) } else { u };
            if u >= 1.0 {
                done = Some(k);
            }
            if !named && (Some(t.prev) == cur || Some(t.next) == cur) {
                named = true;
                rel = if Some(t.prev) == cur { 1.0 - t.alpha } else { t.alpha };
            }
        }
        if let Some(k) = done {
            list.drain(..=k);
        }
    }
    // (`retain` keeps the others' order)
    active.retain(|_, list| !list.is_empty());
    rel
}

/// `airClip`: the clip of an AirState state's player.
fn air_clip(w: &mut World, f: FighterId, p: &Parts, s: Name) -> Name {
    if s == name!("fall") {
        fall_anim(w, f)
    } else if s == name!("jump1") {
        nmo(&w.names, &p.an.jump1)
    } else if s == name!("jump2") {
        nmo(&w.names, &p.an.jump2)
    } else {
        nmo(&w.names, &p.an.fast_fall)
    }
}

/// `smHeld`: the AirState states that were left and are still named by an active
/// transition are updated with the machine (in the update their transition finishes in too): their players' times
/// go on. One that no transition names any more has no weight: its time is dropped, and its next entry starts it
/// again (set_gs). `before`: ahead of sm_advance (the times move), else after it (the drop).
pub(crate) fn sm_held(w: &mut World, f: FighterId, before: bool) {
    let d = w.data.clone();
    let p = parts_of(&d, w, f);
    // The Map's walk with its deletes: an entry removed leaves the next one at its place.
    let mut i = 0;
    while let Some((&s, &t)) = w.fighter(f).sm.sm_times.get_index(i) {
        let named = w
            .fighter(f)
            .sm
            .sm_active
            .get(&name!("Air"))
            .is_some_and(|air| air.iter().any(|e| e.prev == s || e.next == s));
        if !named {
            w.fighter_mut(f).sm.sm_times.shift_remove(&s);
            continue;
        }
        if before {
            let a = air_clip(w, f, &p, s);
            let r = if s == name!("fall") { fall_rate(w, f) } else { 1.0 };
            let mv = consts::DT * r;
            let mut v = t;
            if super::update::looping(&w.names, p.an, a) {
                // the player's FAnimationRuntime::AdvanceTime (142816220): the Fmod wrap, + End below 0
                crate::math::ue::advance_time(true, mv, &mut v, anim_len(&p, w.names.str(a)));
            } else {
                v += mv;
            }
            w.fighter_mut(f).sm.sm_times.insert(s, v);
        }
        i += 1;
    }
}

/// `a2gRate`: the PlayRate of Air to Ground's players: Anim_RunLand's is
/// SelectFloat(RunPlayRate, 1, RunRateEffectsLandRun) (@23060-23139, each update), the others' 1.
fn a2g_rate(w: &World, f: FighterId, p: &Parts, a: Name) -> Real {
    if a == nmo(&w.names, &p.an.run_land) && p.ex.run_rate_land { run_play_rate(w, f) } else { 1.0 }
}

/// `a2gPose`: Air to Ground's pose: BlendListByBool(HorizontalSpeed >= 200: Anim_RunLand,
/// 0.2 s in; else 0.1 s in to BlendListByBool(OnEdge && UseEdgeIdle, 0 s: BlendListByBool(IsCrouching:
/// Anim_Crouch_Edge, else Anim_Edge_Land; 0.05 s) (@22953), else BlendListByBool(IsCrouching: Anim_CrouchLand, else
/// Anim_IdleLand; 0.05 s)). All Linear, children not reset on activation; none of the five sequence players loops.
pub fn a2g_pose(w: &World, f: FighterId) -> Name {
    let d = w.data.clone();
    let p = parts_of(&d, w, f);
    let fi = w.fighter(f);
    if fi.body.vx.abs() >= 200.0 {
        return nmo(&w.names, &p.an.run_land);
    }
    let crouched = fi.capsule.cmc_crouched;
    if let Some(e) = edge_on(w, f, &p) {
        return nm(&w.names, if crouched { &e.crouch } else { &e.land });
    }
    nmo(&w.names, if crouched { &p.an.crouch_land } else { &p.an.idle_land })
}

/// `groundMachine`: Base_AnimBP Movement machine from Air to Ground on, and the Ground
/// machine with its IdleSubState and RunSubState. The Ground machine's first update (it is reinitialised each time its
/// state becomes relevant, in the update that enters it): FAnimNode_StateMachine::Update_AnyThread (exe 142867fa0)
/// runs the FindValidTransition loop on the first update like on any other and, with bSkipFirstUpdateTransition,
/// only resets the active transition array after it. So ProxyEntryState is left in that same update, the states it
/// passes are never shown, and the state it ends in is at full weight inside Ground: the one crossfade is that of the
/// transition that entered Ground (Air to Ground -> Ground 0.3 / 0 s, a hit state -> Movement).
/// Deviation: the other machines with the flag (AirState, Wall, IdleSubState, HitBackIntro / HitForwardIntro, C017's
/// HoverState) and Movement entered in the air still wait a frame in their entry state.
pub fn ground_machine(w: &mut World, f: FighterId) {
    let mut first = None;
    ground_states(w, f, &mut first);
    edge_blend(w, f);
    let Some(b) = first else { return };
    let fi = w.fighter_mut(f);
    fi.anim.loco_blend = b;
    fi.sm.sm_active.shift_remove(&name!("Ground"));
}

/// What `groundStates` reads once at its start.
struct Ground<'d> {
    p: Parts<'d>,
    speed: Real,
    crouch: bool,
    mont: bool,
    /// CrouchTransitionOverride (the rules do not test the clips; a child that sets it sets both).
    cto: bool,
}

impl Ground<'_> {
    fn remaining(&self, w: &World, f: FighterId) -> Real {
        let a = &w.fighter(f).anim;
        anim_len(&self.p, w.names.str(a.loco_anim)) - a.loco_time
    }

    /// Entering IdleSubState (reinitialised): CrouchCheck -> Crouch if crouched, still and no base montage, with
    /// UseCrouchInFromRun -> Crouch In (0.05) instead; else Idle (0.2). T22 first (TR_118 @5809-6166): CrouchCheck ->
    /// CrouchRunIntoCrouch (0) on the T19 expression, whose last term (!UseCrouchInFromRun || (UseCrouchRun &&
    /// GetCurrentStateName(RunSubState) == CrouchRun)) reads the run machine's state as it was left (run_sub;
    /// Unverified: a machine that is not relevant is taken to return the state it was left in). Its clip
    /// AnimCrounchRunIntoCrouch unset (every set but Velma's and Marvin's): T21 -> Crouch (0) in the same update.
    /// Deviation: that order is the layer copy's (BaseMovementMachine). A set that runs as a LinkedAnimGraph runs the
    /// AnimGraph copy, whose IdleSubState takes Crouch (0.2) first (TR_21 @11699-12056): Tom and Jerry's no-Jerry set
    /// cuts here where the build blends 0.2 s. An anim set does not record which copy it runs.
    fn idle_sub(&self, w: &mut World, f: FighterId) {
        let an = self.p.an;
        let cr = self.p.ex.crouch_run.as_ref();
        let still = self.crouch && self.speed == 0.0 && !self.mont;
        let run_sub = w.fighter(f).sm.run_sub;
        if still && (!an.crouch_in_from_run || (cr.is_some() && run_sub == Some(RunSubState::CrouchRun))) {
            match cr.and_then(|c| c.out_of.as_ref()) {
                Some(o) => set_gs(w, f, GroundState::CrouchRunIntoCrouch, nm(&w.names, o), 0.0, NO_OPT),
                None => set_gs(w, f, GroundState::Crouch, nmo(&w.names, &an.crouch), 0.0, NO_OPT),
            }
        } else if still {
            if an.crouch_in_from_run && an.crouch_in.is_some() {
                set_gs(w, f, GroundState::CrouchIn, nmo(&w.names, &an.crouch_in), 0.05, NO_OPT);
            } else {
                set_gs(w, f, GroundState::Crouch, nmo(&w.names, &an.crouch), 0.2, NO_OPT);
            }
        } else {
            set_gs(w, f, GroundState::Idle, nmo(&w.names, &an.idle), 0.2, NO_OPT);
        }
    }

    /// Idle / Alternate Idle Enter / Alternate Idle Loop -> Crouch In (0) on IsCrouching && CrouchTransitionOverride,
    /// -> Crouch (0.1) on IsCrouching without it.
    fn to_crouch(&self, w: &mut World, f: FighterId) {
        if self.cto {
            set_gs(w, f, GroundState::CrouchIn, nmo(&w.names, &self.p.an.crouch_in), 0.0, NO_OPT);
        } else {
            set_gs(w, f, GroundState::Crouch, nmo(&w.names, &self.p.an.crouch), 0.1, NO_OPT);
        }
    }

    /// Ground -> Run (`blend`): RunSubState is reinitialised in its empty CrouchCheck, which it leaves in the same
    /// update by T7 -> CrouchIntoCrouchRun (UseCrouchRun && IsCrouching && AnimCrounchIntoCrouchRun set), T3 ->
    /// CrouchRun (UseCrouchRun && IsCrouching), else T4 -> Run (constant true), all 0 s. The one crossfade is Ground's.
    /// Unverified: CrouchCheck taken to pass on in the entering update (bSkipFirstUpdateTransition).
    fn run_entry(&self, w: &mut World, f: FighterId, blend: Real) {
        match &self.p.ex.crouch_run {
            Some(cr) if self.crouch => match &cr.into {
                Some(i) => set_gs(w, f, GroundState::CrouchIntoCrouchRun, nm(&w.names, i), blend, NO_OPT),
                None => {
                    let r = crouch_run_play_rate(w, f);
                    set_gs(w, f, GroundState::CrouchRun, nm(&w.names, &cr.anim), blend, rate(r));
                }
            },
            _ => self.run(w, f, blend),
        }
    }

    fn run(&self, w: &mut World, f: FighterId, blend: Real) {
        let r = run_play_rate(w, f);
        set_gs(w, f, GroundState::Run, nmo(&w.names, &self.p.an.run), blend, rate(r));
    }

    fn crouch_run(&self, w: &mut World, f: FighterId, blend: Real) {
        let cr = self.p.ex.crouch_run.as_ref().expect("a set with UseCrouchRun");
        let r = crouch_run_play_rate(w, f);
        set_gs(w, f, GroundState::CrouchRun, nm(&w.names, &cr.anim), blend, rate(r));
    }
}

/// `groundStates`: UseEdgeIdle (Tom and Jerry's two sets alone) and
/// CrouchTransitionOverride / UseCrouchInFromRun (Morty, Velma and Marvin: through Crouch In and Crouch Out);
/// UseCrouchRun (Velma and Marvin alone) adds RunSubState's CrouchRun, CrouchRunIn and CrouchIntoCrouchRun inside
/// Ground Run and IdleSubState's CrouchRunIntoCrouch (the BaseMovementMachine layer copy, machines 13 and 14);
/// without it RunSubState stays in Run. Transitions in priority order, up to MaxTransitionsPerFrame 3.
/// Deviation: a chain of transitions in one frame crossfades with its last transition's duration; IdleSubState's
/// empty CrouchCheck entry state is taken to pass on in the same frame.
fn ground_states(w: &mut World, f: FighterId, first: &mut Option<(Real, bool)>) {
    let d = w.data.clone();
    let p = parts_of(&d, w, f);
    let fi = w.fighter(f);
    let an = p.an;
    let g = Ground {
        speed: fi.body.vx.abs(),
        crouch: fi.capsule.cmc_crouched,
        mont: is_montage_playing(w, f),
        cto: an.crouch_override && an.crouch_in.is_some() && an.crouch_out.is_some(),
        p,
    };
    let (speed, crouch, mont, cto) = (g.speed, g.crouch, g.mont, g.cto);
    let cr = g.p.ex.crouch_run.as_ref();
    let names = w.names.clone();
    let clip = |s: &Option<String>| nmo(&names, s);
    let mut n = 0;
    while n < 3 {
        let s = w.fighter(f).sm.gs;
        if is_air(s) || s == GroundState::Wall {
            // Air -> Air to Ground (0) on (!IsFalling || IsMovingOnGround) && !IsAttacking; Wall -> Air to Ground (0.1).
            if anim_is_attacking(w, f) {
                return;
            }
            w.fighter_mut(f).sm.a2g_times.clear();
            let a = a2g_pose(w, f);
            let r = a2g_rate(w, f, &g.p, a);
            set_gs(w, f, GroundState::A2g, a, if s == GroundState::Wall { 0.1 } else { 0.0 }, rate(r));
            return;
        }
        if s == GroundState::A2g {
            // The blend lists switch live; a child resumes its own time.
            let a = a2g_pose(w, f);
            let la = w.fighter(f).anim.loco_anim;
            if a != la {
                // UseEdgeIdle: a change of the edge list's side is its 0 s (Deviation: the crouch list on the side
                // coming in is taken at its own child already, the nested lists' weights are not kept apart).
                let e = g.p.ex.edge_idle.as_ref();
                let side = |x: Name| e.is_some_and(|e| x == nm(&names, &e.crouch) || x == nm(&names, &e.land));
                let run_land = clip(&an.run_land);
                let into = if a == run_land {
                    0.2
                } else if la == run_land {
                    0.1
                } else if side(a) != side(la) {
                    0.0
                } else {
                    0.05
                };
                let t = w.fighter(f).sm.a2g_times.get(&a).copied().unwrap_or(0.0);
                let r = a2g_rate(w, f, &g.p, a);
                let o = SetLocoOpt { restart: true, linear: true, time: Some(t), looping: Some(false) };
                set_loco(w, f, a, r, Some(into), Some(o));
            } else {
                let r = a2g_rate(w, f, &g.p, a);
                w.fighter_mut(f).anim.loco_rate = r;
            }
            // Air to Ground -> Ground: 0.3 s when the land is done and slow, 0 s when done and fast. Ground
            // reinitialises in ProxyEntryState (Anim_Idle) and has its first update now.
            if g.remaining(w, f) > 0.0 {
                return;
            }
            set_gs(w, f, GroundState::Entry, clip(&an.idle), if speed < 200.0 { 0.3 } else { 0.0 }, NO_OPT);
        } else if s == GroundState::Entry {
            // Ground's first update: the entering transition's crossfade is kept, Ground's own are dropped, and the
            // machine has its own MaxTransitionsPerFrame (this one is its first).
            if first.is_none() {
                *first = Some(w.fighter(f).anim.loco_blend);
            }
            if w.fighter(f).sm.dash_stop_blend {
                set_gs(w, f, GroundState::RunStop, clip(&an.run_stop), 0.2, NO_OPT);
            } else {
                g.idle_sub(w, f);
            }
            n = 0;
        } else if is_run_sub(s) {
            // Ground Run's own transitions first, from whichever RunSubState state is current: T7 Run -> Idle (0.2) on
            // OnEdge && UseEdgeIdle && WalkHorizontalSpeed < 0.01 (@13906-14016) ahead of all (no RunStop at an edge).
            if edge_on(w, f, &g.p).is_some() && speed < 0.01 {
                g.idle_sub(w, f);
            } else if mont {
                set_gs(w, f, GroundState::Idle, clip(&an.idle), 0.0, NO_OPT);
            } else if speed < 0.01 && crouch {
                g.idle_sub(w, f);
            } else if speed < 0.01 {
                // Run -> RunStop2 (0.05) before Run -> RunStop (0.05): Anim_RunStop_2 set and the Run state's time
                // fraction in [StopSplitFraction, StopSplitFraction + 0.5] (StopSplitFraction 0.5 on every child).
                let a = &w.fighter(f).anim;
                let frac = a.loco_time / anim_len(&g.p, names.str(a.loco_anim));
                if an.run_stop2.is_some() && (0.5..=1.0).contains(&frac) {
                    set_gs(w, f, GroundState::RunStop2, clip(&an.run_stop2), 0.05, NO_OPT);
                } else {
                    set_gs(w, f, GroundState::RunStop, clip(&an.run_stop), 0.05, NO_OPT);
                }
            } else if s == GroundState::Run {
                // Run -> CrouchRun (0.1) on UseCrouchRun && IsCrouching && !CrouchTransitionOverride, -> CrouchRunIn
                // (0, Anim_CrouchIn) with it (the rules read the bool alone).
                if cr.is_some() && crouch && !an.crouch_override {
                    g.crouch_run(w, f, 0.1);
                } else if cr.is_some() && crouch {
                    set_gs(w, f, GroundState::CrouchRunIn, clip(&an.crouch_in), 0.0, NO_OPT);
                } else {
                    w.fighter_mut(f).anim.loco_rate = run_play_rate(w, f);
                    return;
                }
            } else if s == GroundState::CrouchRun {
                // CrouchRun -> Run (0.1) on !IsCrouching.
                if !crouch {
                    g.run(w, f, 0.1);
                } else {
                    w.fighter_mut(f).anim.loco_rate = crouch_run_play_rate(w, f);
                    return;
                }
            } else if s == GroundState::CrouchRunIn {
                // CrouchRunIn -> Run (0.05) on !IsCrouching, -> CrouchRun (0.05) once 0.05 s remain.
                if !crouch {
                    g.run(w, f, 0.05);
                } else if g.remaining(w, f) <= 0.05 {
                    g.crouch_run(w, f, 0.05);
                } else {
                    return;
                }
            } else {
                // CrouchIntoCrouchRun -> Run (0.1) on !IsCrouching, -> CrouchRun (0) at its end.
                if !crouch {
                    g.run(w, f, 0.1);
                } else if g.remaining(w, f) <= 0.0 {
                    g.crouch_run(w, f, 0.0);
                } else {
                    return;
                }
            }
        } else if s == GroundState::RunStop {
            if speed > 0.0 {
                g.run_entry(w, f, 0.2);
            } else if crouch {
                g.idle_sub(w, f);
            } else if g.remaining(w, f) <= crate::math::narrow(an.run_stop_blend_time) {
                // RunStop -> Idle when the remaining time reaches RunStopBlendTime (a 0.3 s Linear crossfade).
                g.idle_sub(w, f);
                w.fighter_mut(f).anim.loco_blend = (0.3, false);
            } else {
                return;
            }
        } else if s == GroundState::RunStop2 {
            // RunStop2 -> Idle (0) on IsCrouching, -> Run (0.2) on speed > 0, -> Idle when the remaining time reaches
            // 0.3 (a literal; a 0.3 s HermiteCubic crossfade).
            if crouch {
                g.idle_sub(w, f);
            } else if speed > 0.0 {
                g.run_entry(w, f, 0.2);
            } else if g.remaining(w, f) <= 0.3 {
                g.idle_sub(w, f);
                w.fighter_mut(f).anim.loco_blend = (0.3, true);
            } else {
                return;
            }
        } else if speed > 0.0 && !mont {
            // Idle (IdleSubState) -> Run (0) on HorizontalSpeed > 0 && !IsMontagePlaying.
            g.run_entry(w, f, 0.0);
        } else if s == GroundState::Idle {
            // Idle -> Crouch, -> IdleReset, -> Crouch In, -> Alternate Idle Enter, in that order; the last two on
            // (!OnEdge || !UseEdgeIdle) too (@4354-4526, @4010-4325).
            let edge = edge_on(w, f, &g.p).is_some();
            let (gs_time, frac) = {
                let fi = w.fighter(f);
                let a = &fi.anim;
                (fi.sm.gs_time, a.loco_time / anim_len(&g.p, names.str(a.loco_anim)))
            };
            if crouch && !cto {
                g.to_crouch(w, f);
            } else if mont {
                set_gs(w, f, GroundState::IdleReset, clip(&an.idle), 0.0, NO_OPT);
            } else if crouch && !edge {
                g.to_crouch(w, f);
            } else if !edge && gs_time > crate::math::narrow(g.p.ex.alt_idle_delay) && frac > 0.5 {
                // the set's AlternateIdle_TimeDelay
                set_gs(w, f, GroundState::AltEnter, clip(&an.alt_enter), 0.1, NO_OPT);
            } else {
                return;
            }
        } else if s == GroundState::AltEnter {
            if g.remaining(w, f) <= 0.0 {
                set_gs(w, f, GroundState::AltLoop, clip(&an.alt_loop), 0.2, NO_OPT);
            } else if mont {
                set_gs(w, f, GroundState::IdleReset, clip(&an.idle), 0.0, NO_OPT);
            } else if crouch {
                g.to_crouch(w, f);
            } else {
                return;
            }
        } else if s == GroundState::AltLoop {
            if mont {
                set_gs(w, f, GroundState::IdleReset, clip(&an.idle), 0.0, NO_OPT);
            } else if crouch {
                g.to_crouch(w, f);
            } else {
                return;
            }
        } else if s == GroundState::IdleReset {
            if !mont {
                set_gs(w, f, GroundState::Idle, clip(&an.idle), 0.0, NO_OPT);
            } else {
                return;
            }
        } else if s == GroundState::Crouch {
            // Crouch -> Crouch Out (0) with CrouchTransitionOverride and (!OnEdge || !UseEdgeIdle) (@4719-4920), -> Idle
            // (0.1) without CrouchTransitionOverride (at an edge with it the state holds).
            if crouch {
                return;
            }
            if cto {
                if edge_on(w, f, &g.p).is_some() {
                    return;
                }
                set_gs(w, f, GroundState::CrouchOut, clip(&an.crouch_out), 0.0, NO_OPT);
            } else {
                set_gs(w, f, GroundState::Idle, clip(&an.idle), 0.1, NO_OPT);
            }
        } else if s == GroundState::CrouchIn {
            // Crouch In -> Crouch Out (0) on !IsCrouching, -> Crouch (0.1) at its end. Both players are in the sync
            // group CrouchTransition; with a 0 s crossfade only the entered one is updated, so it starts at 0.
            if !crouch {
                set_gs(w, f, GroundState::CrouchOut, clip(&an.crouch_out), 0.0, NO_OPT);
            } else if g.remaining(w, f) <= 0.0 {
                set_gs(w, f, GroundState::Crouch, clip(&an.crouch), 0.1, NO_OPT);
            } else {
                return;
            }
        } else if s == GroundState::CrouchOut {
            // Crouch Out -> Crouch In (0) on IsCrouching, -> Idle (0.1) at its end.
            if crouch {
                set_gs(w, f, GroundState::CrouchIn, clip(&an.crouch_in), 0.0, NO_OPT);
            } else if g.remaining(w, f) <= 0.0 {
                set_gs(w, f, GroundState::Idle, clip(&an.idle), 0.1, NO_OPT);
            } else {
                return;
            }
        } else if s == GroundState::CrouchRunIntoCrouch {
            // CrouchRunIntoCrouch -> Crouch Out (0) on !IsCrouching && CrouchTransitionOverride && (!OnEdge ||
            // !UseEdgeIdle) (@6195-6396), -> Idle (0.1) on !IsCrouching && !CrouchTransitionOverride, -> Crouch (0) at
            // its end.
            if !crouch && an.crouch_override && edge_on(w, f, &g.p).is_none() {
                set_gs(w, f, GroundState::CrouchOut, clip(&an.crouch_out), 0.0, NO_OPT);
            } else if !crouch && !an.crouch_override {
                set_gs(w, f, GroundState::Idle, clip(&an.idle), 0.1, NO_OPT);
            } else if g.remaining(w, f) <= 0.0 {
                set_gs(w, f, GroundState::Crouch, clip(&an.crouch), 0.0, NO_OPT);
            } else {
                return;
            }
        } else {
            return;
        }
        n += 1;
    }
}

/// What `hitMachine` gives: 'none', 'hit' or 'entered' (a hit state -> Movement this update).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum HitMachine {
    None,
    Hit,
    Entered,
}

/// `animBounceType`: Base_AnimBP's IsBouncing (OnIsBouncingChanged, bound to
/// BP_FighterCharacter's OnIsBouncing at its BeginPlay, statement 27480): the last broadcast's type. BaseFighter's own
/// broadcasts (BFBounce, the hit pause end) carry its IsBouncing (`bouncing` / `bounceType`); Buff_Flatten's 3 and 0 do
/// not (`anim_bounce`). Its 0 is "not bouncing" for the machine's rules (NotBouncing).
pub fn anim_bounce_type(w: &World, f: FighterId) -> Real {
    let h = &w.fighter(f).hit;
    h.anim_bounce.unwrap_or(if h.bouncing { crate::real(h.bounce_type) } else { 0.0 })
}

/// `animBouncing`.
pub fn anim_bouncing(w: &World, f: FighterId) -> bool {
    let h = &w.fighter(f).hit;
    match h.anim_bounce {
        Some(b) => b > 0.0,
        None => h.bouncing,
    }
}

/// `hitMachine`: Base_AnimBP MovementAndHitReact machine (no skip on the first update):
/// Movement -> HitStunned (0) on Is Hit Stunned, -> Bounce (0) on HitPauseTime > 0 && IsBouncing; HitStunned ->
/// Movement (0) on !stunned && !IsFalling, -> Bounce, -> Recovery (0) on !stunned && IsFalling; Recovery
/// (Anim_HitReact_Recover, not looped) -> Bounce, -> Movement (0.05 s) on remaining <= 0.05 || !IsFalling ||
/// IsWallSliding (the live layer copy TR_184 @21985-22162; the AnimGraph copy lacks the last term); Bounce ->
/// HitStunned (0) on stunned && no hit pause, -> Recovery on !stunned && no pause && IsFalling, -> HitWobble on
/// !IsBouncing, -> Movement (0.2 s) on !stunned && no pause && !IsFalling; HitWobble -> Movement (0.2 s), -> Recovery,
/// -> Bounce, -> HitStunned on IsNeutralHit (Has_N_HR), -> HitStunned on stunned && no pause. Movement -> HitWobble and
/// HitStunned / Recovery -> HitWobble have no rule. Movement reinitialises on entry (Ground's ProxyEntryState; on the
/// ground its first update is in that frame, ground_machine). Is Hit Stunned is a variable the hitstun handler's
/// dispatch writes (Base_AnimBP OnIsHitStunned @0-60), and the rules read it at the update: the ends called inside a
/// tick are seen by that frame's update when it comes later in the frame, which is the live read here.
pub fn hit_machine(w: &mut World, f: FighterId) -> HitMachine {
    let d = w.data.clone();
    let p = parts_of(&d, w, f);
    let fi = w.fighter(f);
    let stunned = in_hitstun(w, f);
    let pause = fi.react.hit_pause_time > 0.0;
    let falling = !grounded(fi);
    let bouncing = anim_bouncing(w, f);
    let bounce = pause && bouncing;
    let to_movement = |w: &mut World, blend: Real| {
        set_gs(w, f, GroundState::Entry, nmo(&w.names, &p.an.idle), blend, NO_OPT);
        HitMachine::Entered
    };
    let recovery = |w: &mut World| set_gs(w, f, GroundState::Recovery, nmo(&w.names, &p.an.recover), 0.0, NO_OPT);
    for _ in 0..3 {
        let s = w.fighter(f).sm.gs;
        match s {
            GroundState::HitStunned => {
                if !stunned && !falling {
                    return to_movement(w, 0.0);
                }
                if bounce {
                    enter_bounce(w, f);
                } else if !stunned && falling {
                    recovery(w);
                } else {
                    hit_pose(w, f);
                    return HitMachine::Hit;
                }
            }
            GroundState::Recovery => {
                let fi = w.fighter(f);
                let left = anim_len(&p, w.names.str(fi.anim.loco_anim)) - fi.anim.loco_time;
                if bounce {
                    enter_bounce(w, f);
                } else if left <= 0.05 || !falling || fi.wall.wall_sliding {
                    return to_movement(w, 0.05);
                } else {
                    return HitMachine::Hit;
                }
            }
            GroundState::Bounce => {
                if stunned && !pause {
                    enter_hit_stunned(w, f);
                } else if !stunned && !pause && falling {
                    recovery(w);
                } else if !bouncing {
                    let fi = w.fighter_mut(f);
                    fi.sm.gs = GroundState::HitWobble;
                    fi.sm.gs_time = 0.0;
                    fi.react.wobble_times = (0.0, 0.0);
                    wobble_pose(w, f, true);
                } else if !stunned && !pause && !falling {
                    return to_movement(w, 0.2);
                } else {
                    // The blend list follows IsBouncing live (BlendTime 0 for every pose). Each of the six types has
                    // its own sequence player and bResetChildOnActivation is false on the node, so a player that was
                    // shown before in this visit of the state goes on from the time it was left at (a child without
                    // weight is not updated: FAnimNode_BlendListBase::Update_AnyThread, stock 4.26), and one shown for
                    // the first time starts at 0 (the state's entry initialised all six).
                    let t = anim_bounce_type(w, f);
                    if t != w.fighter(f).pose.bounce_pose {
                        let fi = w.fighter_mut(f);
                        let (bp, lt) = (fi.pose.bounce_pose, fi.anim.loco_time);
                        fi.pose.bounce_times.insert(super::montages::int(bp), lt);
                        fi.pose.bounce_pose = t;
                        let time = fi.pose.bounce_times.get(&super::montages::int(t)).copied().unwrap_or(0.0);
                        let a = bounce_anim(w, f);
                        let o = SetLocoOpt { restart: true, linear: false, time: Some(time), looping: Some(true) };
                        set_loco(w, f, a, 1.0, Some(0.0), Some(o));
                    }
                    return HitMachine::Hit;
                }
            }
            GroundState::HitWobble => {
                if !stunned && !pause && !falling {
                    return to_movement(w, 0.2);
                }
                if !stunned && falling && !pause {
                    recovery(w);
                } else if bouncing {
                    enter_bounce(w, f);
                } else if w.fighter(f).react.hit_neutral || (stunned && !pause) {
                    enter_hit_stunned(w, f);
                } else {
                    wobble_pose(w, f, false);
                    return HitMachine::Hit;
                }
            }
            _ => {
                if stunned {
                    enter_hit_stunned(w, f);
                } else if bounce {
                    enter_bounce(w, f);
                } else {
                    return HitMachine::None;
                }
            }
        }
    }
    HitMachine::Hit
}

/// `wobblePose`: HitWobble: TwoWayBlend(A = Anim_HitReact_Back, B = Anim_HitReact_Front,
/// Alpha = FacingVelocityAlpha, no interpolation), both not looped. FacingVelocityAlpha (SetHorizontalAndVerticalSpeed,
/// every update) = (FacingLeft && Velocity.Y >= 0) || (!FacingLeft && Velocity.Y <= 0): 1 when not moving against the
/// facing. Only the weighted child updates, so each player keeps its own time; the state's entry restarts both.
pub fn wobble_pose(w: &mut World, f: FighterId, enter: bool) {
    let d = w.data.clone();
    let p = parts_of(&d, w, f);
    let fi = w.fighter(f);
    let front = if fi.body.facing_dir < 0.0 { fi.body.vx <= 0.0 } else { fi.body.vx >= 0.0 };
    let a = nmo(&w.names, if front { &p.an.hr_front } else { &p.an.hr_back });
    if !enter && a == fi.anim.loco_anim {
        return;
    }
    let fi = w.fighter_mut(f);
    if !enter {
        let lt = fi.anim.loco_time;
        if front {
            fi.react.wobble_times.1 = lt;
        } else {
            fi.react.wobble_times.0 = lt;
        }
    }
    let wt = fi.react.wobble_times;
    let time = if enter {
        0.0
    } else if front {
        wt.0
    } else {
        wt.1
    };
    let o = SetLocoOpt { restart: true, linear: false, time: Some(time), looping: Some(false) };
    set_loco(w, f, a, 1.0, Some(0.0), Some(o));
}

/// `enterHitStunned`.
pub fn enter_hit_stunned(w: &mut World, f: FighterId) {
    let fi = w.fighter_mut(f);
    fi.sm.gs = GroundState::HitStunned;
    fi.sm.gs_time = 0.0;
    fi.react.hr_branch = Name::NONE;
    fi.react.hr_times.clear();
    hit_pose(w, f);
}

/// `bounceAnim`: Bounce: BlendListByEnum(IsBouncing = Enum_BounceType; EnumToPoseIndex [0, 5,
/// 4, 3, 0, 2, 1, 0]): WallBack (1) Anim_HitReact_WallBounceBack, WallFront (2) WallBounceFront, FloorBack (3) and
/// CeilingBack (5) FloorBounceBack, FloorFront (4, the default pose) and CeilingFront (6) FloorBounceFront. All looped.
pub fn bounce_anim(w: &World, f: FighterId) -> Name {
    let d = w.data.clone();
    let p = parts_of(&d, w, f);
    let an = p.an;
    let t = anim_bounce_type(w, f);
    let a = if t == 1.0 {
        &an.wall_bounce_back
    } else if t == 2.0 {
        &an.wall_bounce_front
    } else if t == 3.0 || t == 5.0 {
        &an.floor_bounce_back
    } else {
        &an.floor_bounce_front
    };
    nmo(&w.names, a)
}

/// `enterBounce`.
pub fn enter_bounce(w: &mut World, f: FighterId) {
    let t = anim_bounce_type(w, f);
    let fi = w.fighter_mut(f);
    fi.pose.bounce_pose = t;
    fi.pose.bounce_times.clear();
    let a = bounce_anim(w, f);
    set_gs(w, f, GroundState::Bounce, a, 0.0, NO_OPT);
    w.fighter_mut(f).anim.loco_loop = true;
}

/// The hit react branch's key (`'n' + HitReactInt`, `'b'`, `'f'`).
fn hr_key(neutral: bool, int: i32, kb_facing: bool) -> Name {
    if neutral {
        match int {
            0 => name!("n0"),
            1 => name!("n1"),
            2 => name!("n2"),
            3 => name!("n3"),
            _ => panic!("HitReactInt {int} out of RandomInteger(MaxNeutralHR 4)"),
        }
    } else if kb_facing {
        name!("b")
    } else {
        name!("f")
    }
}

/// `hitPose`: HitStunned's pose: BlendListByBool(Has_N_HR && IsNeutralHit || IsBuffStunned)
/// -> BlendListByInt(HitReactInt) over Anim_HitReact_N_01..04 (not looped); else BlendListByBool(KB_Facing) -> the
/// HitBackIntro machine (Anim_HitReact_FlyBack_Intro -> Anim_HitReact_FlyBack, looped, when the intro has <= 0.1 s
/// left, 0.1 s) or HitForwardIntro. All blend list switches are 0 s with bResetChildOnActivation false, so only the
/// shown child updates: a neutral pose switched back to resumes at its held time (the state's entry resets them all);
/// the intro machines reinitialise on becoming relevant. IsBuffStunned (HitstunBuffActive, read in OnIsHitStunned): a
/// buff with a stun duration. Each side's intro machine sits under one more blend list on the intro clip being unset
/// (Base_AnimBP ubergraph @21001 / @20942, the layer's copy @25227 / @25168): pose 0 (true) is SequencePlayer_30 on
/// Anim_HitReact_FlyBack / SequencePlayer_31 on Anim_HitReact_FlyForward (both bLoopAnimation, StartPosition 0), pose
/// 1 the machine (BlendTime 0, bResetChildOnActivation false). So a character without an intro clip plays the fly loop
/// from the hit on; that player is no machine: like a neutral pose it is held while its side is not shown and goes on
/// from there in the same visit of the state.
pub fn hit_pose(w: &mut World, f: FighterId) {
    let d = w.data.clone();
    let p = parts_of(&d, w, f);
    let an = p.an;
    let fi = w.fighter(f);
    let neutral = fi.react.hit_neutral || fi.moves.pull.is_some() || fi.buffs.buff_stunned;
    let kb = fi.react.kb_facing;
    let hri = fi.react.hit_react_int;
    let branch = hr_key(neutral, hri, kb);
    let held = |b: Name| {
        matches!(b, x if x == name!("n0") || x == name!("n1") || x == name!("n2") || x == name!("n3"))
            || (b == name!("b") && an.fly_back_intro.is_none())
            || (b == name!("f") && an.fly_forward_intro.is_none())
    };
    let fly = |w: &World| nmo(&w.names, if kb { &an.fly_back } else { &an.fly_forward });
    if branch != fi.react.hr_branch {
        let fi = w.fighter_mut(f);
        let old = fi.react.hr_branch;
        if !old.is_none() && held(old) {
            let lt = fi.anim.loco_time;
            fi.react.hr_times.insert(old, lt);
        }
        fi.react.hr_branch = branch;
        let intro = if neutral {
            None
        } else if kb {
            an.fly_back_intro.as_ref()
        } else {
            an.fly_forward_intro.as_ref()
        };
        fi.react.hr_intro = intro.is_some();
        let time = Some(fi.react.hr_times.get(&branch).copied().unwrap_or(0.0));
        if !neutral && intro.is_none() {
            let o = SetLocoOpt { restart: true, linear: false, time, looping: Some(true) };
            set_loco(w, f, fly(w), 1.0, Some(0.0), Some(o));
            return;
        }
        let a = match intro {
            Some(i) => nm(&w.names, i),
            None => usize::try_from(hri)
                .ok()
                .and_then(|i| an.hr_n.get(i))
                .and_then(|a| a.as_deref())
                .map_or(Name::NONE, |a| nm(&w.names, a)),
        };
        let o = SetLocoOpt { restart: true, linear: false, time, looping: Some(false) };
        set_loco(w, f, a, 1.0, Some(0.0), Some(o));
        return; // the intro machine skips its first update's transitions
    }
    let left = anim_len(&p, w.names.str(fi.anim.loco_anim)) - fi.anim.loco_time;
    if fi.react.hr_intro && !neutral && left <= 0.1 {
        w.fighter_mut(f).react.hr_intro = false;
        let o = SetLocoOpt { restart: true, linear: false, time: None, looping: Some(true) };
        set_loco(w, f, fly(w), 1.0, Some(0.1), Some(o));
    }
}

/// `airMachine`: Movement machine into Air and the AirState machine (Fall = Anim_Fall_01 at
/// PlayRate 1.5, looped, with UseFallBlendspace false, else FallBlendSpace: fall_anim; Jump1 / Jump2 =
/// BlendSpace_Jump_01 / 02, not looped; FastFall = Anim_FastFall, not looped). JumpingAndJumpN = Jumping &&
/// CurrentJumpNumber == N. CanFastFall = IsFastFalling && !BaseAnyMontagePlaying && !BaseMontageIsPlaying &&
/// !IsMontagePlaying && !IsAttacking. The transition loop has no guard against going back: FastFall -> Fall
/// (remaining < 0.05) -> FastFall (CanFastFall) in one update, both at 0 s: FastFall is not initialised (it has weight 1
/// through the entry that left it), both entries finish in this update and the pose is FastFall's alone, on from where
/// it was. So nothing a reader of the tick's end sees has changed: the switch key and the crossfade that was running
/// stay. Kept only when AirState had no transition of its own running.
/// Deviation: Fall is initialised and updated once at the machine's weight in such an update (its player restarts; a
/// notify of its clip's first 0.025 s would be queued). The port neither draws nor updates it.
/// Unverified: the player of a state updated at weight 0 is taken to move like any other (the proxy's loop over the
/// tick records is not read natively, 4.26 source).
pub fn air_machine(w: &mut World, f: FighterId) {
    let fi = w.fighter_mut(f);
    let (s, gen0, blend0) = (fi.sm.gs, fi.anim.loco_gen, fi.anim.loco_blend);
    let quiet = is_air(s) && !fi.sm.sm_active.contains_key(&name!("Air"));
    fi.sm.gs_kept = false;
    air_rules(w, f);
    let fi = w.fighter_mut(f);
    if quiet && fi.sm.gs_kept && fi.sm.gs == s {
        fi.anim.loco_gen = gen0;
        fi.anim.loco_blend = blend0;
    }
}

fn fall(w: &mut World, f: FighterId, blend: Real) {
    let (a, r) = (fall_anim(w, f), fall_rate(w, f));
    set_gs(w, f, GroundState::Fall, a, blend, rate(r));
}

/// `airRules`.
fn air_rules(w: &mut World, f: FighterId) {
    let d = w.data.clone();
    let p = parts_of(&d, w, f);
    let an = p.an;
    let fi = w.fighter(f);
    let s = fi.sm.gs;
    // IsAnyMontagePlaying (exe FUN_14283fce0): the AnimInstance has an instance, of any group, stopped ones that
    // still blend out included.
    let ms = &fi.montages;
    let any_montage = ms.montage.is_some()
        || ms.dash_montage.is_some()
        || !ms.fading.is_empty()
        || ms.grouped.as_ref().is_some_and(|g| !g.0.is_empty());
    let moving_up = fi.body.vy > 0.0;
    if !is_air(s) {
        // Into Movement's Air: from Air to Ground on MovingUp || IsFalling (0.1 s); from Ground (!IsMovingOnGround,
        // 0.2 s) or Wall (!IsWallSliding, 0.2 s) through the Conduit: -> Air 0 s when VerticalSpeed >= 50 && MovingUp
        // && IsFalling, else 0.2 s on IsFalling. Entering Air initialises AirState at its entry state Fall, whose
        // first update skips the transitions (bSkipFirstUpdateTransition).
        // Deviation: the chain takes the last crossfade; AirState restarting at Fall on each entry is from
        // FAnimNode_StateMachine::SetState semantics.
        let blend = if s == GroundState::A2g {
            0.1
        } else if fi.body.vy.abs() >= 50.0 && moving_up {
            0.0
        } else {
            0.2
        };
        fall(w, f, blend);
        return;
    }
    let can_fast_fall =
        anim_fast_falling(w, f) && !any_montage && !is_montage_playing(w, f) && !anim_is_attacking(w, f);
    let jumping = fi.moves.cmd_jumped;
    // GetRelevantAnimTimeRemaining (exe 142856260): FLT_MAX while the state's player has no cached blend weight above
    // 0. SetState clears it (set_gs: gs_rel 0), so a state's remaining rule cannot fire in the update that entered it;
    // the state's update writes it (update_pose), 0 for a state left and entered again in one update at 0 s, whose
    // rule then stays false for the next update too.
    let remaining = |w: &World| {
        let fi = w.fighter(f);
        if fi.sm.gs_rel > 0.0 {
            anim_len(&p, w.names.str(fi.anim.loco_anim)) - fi.anim.loco_time
        } else {
            Real::INFINITY
        }
    };
    let clip = |w: &World, a: &Option<String>| nmo(&w.names, a);
    for _ in 0..3 {
        let fi = w.fighter(f);
        let (t, jumps, attacking) = (fi.sm.gs, fi.jump.jumps, anim_is_attacking(w, f));
        match t {
            GroundState::Fall => {
                if can_fast_fall {
                    set_gs(w, f, GroundState::FastFall, clip(w, &an.fast_fall), 0.0, NO_OPT);
                } else if jumping && jumps == 2 {
                    set_gs(w, f, GroundState::Jump2, clip(w, &an.jump2), 0.04, NO_OPT);
                } else if jumping && jumps == 1 {
                    set_gs(w, f, GroundState::Jump1, clip(w, &an.jump1), 0.04, NO_OPT);
                } else {
                    return;
                }
            }
            GroundState::Jump1 => {
                if remaining(w) <= 0.0 {
                    fall(w, f, 0.3);
                } else if can_fast_fall {
                    set_gs(w, f, GroundState::FastFall, clip(w, &an.fast_fall), 0.0, NO_OPT);
                } else if any_montage || w.fighter(f).wall.wall_sliding {
                    fall(w, f, 0.2);
                } else if jumping && jumps == 2 {
                    set_gs(w, f, GroundState::Jump2, clip(w, &an.jump2), 0.04, NO_OPT);
                } else {
                    return;
                }
            }
            GroundState::Jump2 => {
                if can_fast_fall {
                    set_gs(w, f, GroundState::FastFall, clip(w, &an.fast_fall), 0.0, NO_OPT);
                } else if remaining(w) <= 0.0 {
                    fall(w, f, 0.3);
                } else {
                    return;
                }
            }
            // TR_107 @10098-10465 (TR_10 @9365-9732 the same): remaining < 0.05 || IsJumping || MovingUp || IsAttacking ||
            // BaseAnyMontagePlaying || BaseMontageIsPlaying || IsMontagePlaying || !IsFastFalling. BaseMontageIsPlaying
            // (Montage_IsPlaying(null) @6828: an instance not stopped) implies BaseAnyMontagePlaying.
            GroundState::FastFall
                if remaining(w) < 0.05
                    || jumping
                    || moving_up
                    || attacking
                    || any_montage
                    || is_montage_playing(w, f)
                    || !anim_fast_falling(w, f) =>
            {
                fall(w, f, 0.0);
            }
            _ => return,
        }
    }
}

/// `runPlayRate`: RunPlayRate = max(HorizontalSpeed / RunBaseSpeed / min(mesh scale), 0.6),
/// RunBaseSpeed the instance's own class default (the set's).
pub fn run_play_rate(w: &World, f: FighterId) -> Real {
    let d = w.data.clone();
    let p = parts_of(&d, w, f);
    math::max(
        w.fighter(f).body.vx.abs()
            / crate::math::narrow(p.an.run_base_speed)
            / crate::math::narrow(p.cd.boxes.mesh_scale),
        0.6,
    )
}

/// `crouchRunPlayRate`: CrouchRunPlayRate = FMax(HorizontalSpeed / CrouchRunBaseSpeed /
/// FMin(mesh scale), 0.4) (SetHorizontalAndVerticalSpeed @2296-2618, after RunPlayRate): CrouchRun's player alone
/// reads it.
pub fn crouch_run_play_rate(w: &World, f: FighterId) -> Real {
    let d = w.data.clone();
    let p = parts_of(&d, w, f);
    let cr = p.ex.crouch_run.as_ref().expect("a set with UseCrouchRun");
    math::max(
        w.fighter(f).body.vx.abs() / crate::math::narrow(cr.base_speed) / crate::math::narrow(p.cd.boxes.mesh_scale),
        0.4,
    )
}
