//! The anim instance's update of the fighter: the pose a tick shows, the locomotion player
//! (`setLoco`), the layer's hold under a montage, the anim set switch, Direction and FallLean of Base_AnimBP's
//! BlueprintUpdateAnimation, the hit react inputs and the readers of the AnimBP's variables.

use super::blend_spaces::{fall_mix, fall_space_tick, in_space, wall_head, wall_mix, wall_rate, wall_space_tick};
use super::machines::{
    AIR_STATES, air_machine, ground_machine, hit_machine, hover_machine, set_gs, sm_advance, sm_held,
};
use super::montages::{char_data, instance_data, state_ref};
use super::sequences::seq_fade_start;
use super::slot_blend::has_players;
use super::slot_mix::{blend_slots, graph_slots};
use super::*;
use crate::chars::CharState;
use crate::consts;
use crate::fighter::Fighter;
use crate::fighter::common::warn_text;
use crate::math;
use crate::names::Names;
use mvs_data::def::SetExtra;
use mvs_data::{CharData, GameData};

// ---- the data a fighter's anim code reads

/// The fighter's character data and the anim set its machines read, with the set's class defaults kept beside it
/// (`altIdleDelay`, `crouchRunOf`, `edgeIdleOf`, `runRateLandRun` of the character's data).
pub(crate) struct Parts<'d> {
    pub cd: &'d CharData,
    pub an: &'d AnimSet,
    pub ex: &'d SetExtra,
}

/// The parts of `f.animSet` as the getter gives them now (its live hook call, [`crate::fighter::anim_set`]): taken
/// again at each place the getter is read.
pub(crate) fn parts_of<'d>(d: &'d GameData, w: &World, f: FighterId) -> Parts<'d> {
    let names = &w.names;
    let cd = char_data(d, names, w.fighter(f).ch).expect("the fighter's character data");
    match crate::fighter::anim_set(w, f) {
        SetRef::Base => Parts { cd, an: &cd.def.anim, ex: &cd.def_extra.anim },
        SetRef::Named(k) => {
            let k = names.str(k);
            let an = cd.def.anim_sets.as_ref().and_then(|s| s.get(k)).unwrap_or(&cd.def.anim);
            let ex = cd.def_extra.anim_sets.as_ref().and_then(|s| s.get(k)).unwrap_or(&cd.def_extra.anim);
            Parts { cd, an, ex }
        }
    }
}

/// The interned name of a data string; one the match did not intern is the empty name.
pub(crate) fn nm(names: &Names, s: &str) -> Name {
    names.get(s).unwrap_or_default()
}

/// A clip the set may leave unset (`undefined` in the data: the empty name).
pub(crate) fn nmo(names: &Names, s: &Option<String>) -> Name {
    s.as_deref().map_or(Name::NONE, |s| nm(names, s))
}

/// `set.looping.includes(a)`.
pub(crate) fn looping(names: &Names, an: &AnimSet, a: Name) -> bool {
    let s = names.str(a);
    an.looping.iter().any(|x| x.as_deref().unwrap_or("") == s)
}

/// `animLen`: an AnimComposite's segments end to end, else the box index's length, else 1.
pub(crate) fn anim_len(p: &Parts, a: &str) -> crate::Real {
    if let Some(c) = p.an.composites.get(a) {
        // An empty composite has no last element to read.
        let l = c.last().unwrap_or_else(|| panic!("the composite {a} has no section"));
        return crate::math::narrow(l.start) + crate::math::narrow(l.len);
    }
    p.cd.boxes.anims.get(a).map_or(1.0, |x| crate::math::narrow(x.length))
}

/// `animLen`.
pub fn anim_len_of(w: &World, f: FighterId, a: Name) -> crate::Real {
    anim_len(&parts_of(&w.data, w, f), w.names.str(a))
}

/// `Fighter.grounded`.
pub(crate) fn grounded(fi: &Fighter) -> bool {
    fi.body.on_floor
}

/// The pose (clip and time) of instance `uid` (`MontagePlayer.pose(walking)`).
pub(crate) fn inst_pose(
    d: &GameData,
    w: &World,
    f: FighterId,
    uid: u32,
    walking: Option<bool>,
) -> Option<(Name, Real)> {
    let m = instance_data(d, w, f, uid)?;
    let (si, gi, t) = state_ref(w, f, uid)?.pose(m, walking)?;
    Some((nm(&w.names, &m.slots[si].segments[gi].anim), crate::math::narrow(t)))
}

// ---- Direction and FallLean

/// `animDirection`: the direction of a velocity as Base_AnimBP takes it (GetDirection @0-341,
/// OnHitPauseTimeChanged @293-547, OnKnockedBack @0-320, OnIsBouncingChanged @154-386):
/// CalculateDirection(MakeVector(V.Z, V.Y, V.X), a zero rotator), negated when FacingLeft. UAnimInstance::
/// CalculateDirection: 0 for a vector that IsNearlyZero, else the angle in degrees between +X and the vector
/// normalised in XY (acos of its X, here the launch's up part), negative when its Y (UE Y = -x) is. So 0 is straight
/// up, 90 level, 180 straight down, positive for a launch behind the fighter's back.
///
/// In float32 as the build: GetSafeNormal2D's X (the sum `X X + Y Y`, the vector kept at a sum of 1, zero under 1e-8,
/// else times InvSqrt), FMath::Acos (acosf of the input clamped to [-1, 1]) and RadiansToDegrees (one multiply).
// Unverified: CalculateDirection and GetSafeNormal2D read from the 4.26 source, not from the exe.
pub fn anim_direction(w: &World, f: FighterId, vx: Real, vy: Real) -> Real {
    if vx.abs() <= 1e-4 && vy.abs() <= 1e-4 {
        return 0.0;
    }
    let sum = vy * vy + vx * vx;
    let cos = if sum == 1.0 {
        vy
    } else if sum < 1e-8 {
        0.0
    } else {
        vy * math::ue::inv_sqrt(sum)
    };
    let deg = math::ue::acosf(if cos < -1.0 {
        -1.0
    } else if cos < 1.0 {
        cos
    } else {
        1.0
    }) * math::ue::RAD_TO_DEG;
    if (vx > 0.0) == (w.fighter(f).body.facing_dir < 0.0) { deg } else { -deg }
}

/// `animDirectionTick`: BlueprintUpdateAnimation's tail (ubergraph 26249 -> 6940-7210), every
/// update of the AnimInstance:
///   Direction = HitPauseTime <= 0 ? GetDirection() : KnockBackAngle   (GetDirection: Fighter.GetVelocity())
///   DirectionSoft = FInterpTo_Constant(DirectionSoft, Direction, DeltaTimeX, 180)
/// FMath::FInterpConstantTo (14135cbb0): the target when (Target - Current)^2 < 1e-8, else Current + Clamp(Target -
/// Current, -Speed x DT, Speed x DT); the angles are not wrapped. DeltaTimeX carries the actor's CustomTimeDilation,
/// so a pause moves DirectionSoft by 0.015 degrees a frame at most. `paused`: the update of a frame under the pause,
/// which reads HitPauseTime > 0 up to and with the pause's last frame (the HitPauseTimer fires in the timer manager's
/// tick, after the mesh's). The layer instance's FallLean has the same update (fall_lean_tick, run here).
pub fn anim_direction_tick(w: &mut World, f: FighterId, paused: bool) {
    fall_lean_tick(w, f, paused);
    let target = if paused {
        w.fighter(f).react.knock_back_angle
    } else {
        let b = &w.fighter(f).body;
        anim_direction(w, f, b.vx, b.vy)
    };
    let dil = if paused { consts::HITPAUSE_DILATION } else { 1.0 };
    let r = &mut w.fighter_mut(f).react;
    r.direction_soft = math::ue::finterp_constant_to(r.direction_soft, target, consts::DELTA_SECONDS * dil, 180.0);
}

/// `fInterpTo`: FMath::FInterpTo (exe 14135cc10).
pub fn f_interp_to(cur: Real, target: Real, dt: Real, speed: Real) -> Real {
    math::ue::finterp_to(cur, target, dt, speed)
}

/// `fallLeanTick`: Base_AnimBP SetHorizontalAndVerticalSpeed's FallLean (@2645-2969,
/// @3675-4398), in the BaseMovementMachine layer instance, whose BlueprintUpdateAnimation runs on every mesh update
/// whatever the state:
///   YSpeedDirectional = (FacingLeft ? 1 : -1) x Velocity.Y
///   x = Velocity.Z < 0 ? -YSpeedDirectional : YSpeedDirectional
///   FallLean = FInterpTo(FallLean, Lerp(A, -A, clamp(NormalizeToRange(x, -B, B), 0, 1)), DeltaTimeX, FallLeanSpeed)
/// A = FallLeanAmount, B = BaseMaxWalkSpeed: the CMC template's MaxWalkSpeed read once at the instance's init (for
/// Iron Giant his unmodified walk speed, not the fighter's max walk). NormalizeToRange (exe 142b2a960): (Value - min)
/// / (max - min), and for an equal range Value < min ? 0 : 1. UE Y = -x: YSpeedDirectional is the speed along the
/// facing. DeltaTimeX carries the hitpause dilation.
pub fn fall_lean_tick(w: &mut World, f: FighterId, paused: bool) {
    let (amount0, speed) = {
        let p = parts_of(&w.data, w, f);
        (p.an.fall_lean_amount, p.an.fall_lean_speed)
    };
    let max_walk = char_data(&w.data, &w.names, w.fighter(f).ch).expect("the fighter's character data").def.max_walk;
    let fi = w.fighter_mut(f);
    let b = crate::math::narrow(max_walk);
    let lo = -b;
    let fwd = fi.body.vx * fi.body.facing_dir;
    let x = if fi.body.vy < 0.0 { -fwd } else { fwd };
    let n = if lo == b {
        if x < lo { 0.0 } else { 1.0 }
    } else {
        (x - math::min(lo, b)) / (math::max(lo, b) - math::min(lo, b))
    };
    let alpha = math::min(math::max(n, 0.0), 1.0);
    let amount = crate::math::narrow(amount0);
    let target = amount + (alpha * ((-amount) - amount));
    let dil = if paused { consts::HITPAUSE_DILATION } else { 1.0 };
    fi.react.fall_lean =
        f_interp_to(fi.react.fall_lean, target, consts::DELTA_SECONDS * dil, crate::math::narrow(speed));
}

// ---- the layer's hold under a montage

/// `holdsSource`: the montage holds every Slot node's source of a set of the character's
/// own AnimBP (Montage.holdsSource, from the slot nodes without bAlwaysUpdateSourcePose: FAnimNode_Slot::
/// Update_AnyThread, exe 1424945f0, updates its source only with the flag or a source weight above 0.00001) at full
/// weight.
/// A montage with tracks in only some slots of a set (the slots without one pass the update on to their source) holds
/// it when the blend nodes, as their inputs stand, update none of those: the slot graph's walk
/// (`slot_mix::graph_source_updated`) for a character whose hooks give its inputs; Morty's aim slots and Black Adam's
/// AimGrab slots are their own rules (CharHooks.layerHeld).
fn holds_source(w: &World, f: FighterId, uid: Option<u32>) -> bool {
    let Some(uid) = uid else { return false };
    let (Some(m), Some(s)) = (instance_data(&w.data, w, f, uid), state_ref(w, f, uid)) else { return false };
    if !s.full_weight(m) {
        return false;
    }
    if m.holds_source == Some(true) {
        return true;
    }
    let cd = char_data(&w.data, &w.names, w.fighter(f).ch).expect("the fighter's character data");
    let partial = cd.fighter.slot_holds.stages.iter().any(|st| st.iter().any(|n| track_in(m, n)));
    partial && super::slot_mix::graph_source_updated(w, f, m, 1.0) == Some(false)
}

/// A non-empty, non-additive track of montage `m` in `slot` (the import's test for `holds_source`).
fn track_in(m: &mvs_data::fighter::Montage, slot: &str) -> bool {
    m.slots.iter().any(|s| s.slot == slot && !s.segments.is_empty() && s.additive != Some(true))
}

/// `sourceHeld`: a shown montage at full weight holds every Slot node's source of a set of
/// the data's, the character's own rule (layerHeld) aside.
pub fn source_held(w: &World, f: FighterId) -> bool {
    let ms = &w.fighter(f).montages;
    let (m, d) = (ms.montage.as_ref().map(|s| s.montage.uid), ms.dash_montage.as_ref().map(|s| s.montage.uid));
    holds_source(w, f, m) || holds_source(w, f, d)
}

/// `layerFrozen`: the BaseMachine layer gets no update this frame (holdsSource; a
/// character's own rule for the Slot nodes under a blend input of its graph: CharHooks.layerHeld). Its movement
/// machine gets none while the carried pose is shown either (BlendListByBool_16 updates only its weighted child).
// Deviation: the carried pose's 0.05 s blend in (both children updated, the poses mixed) is not modelled: the
// machine stands from the first update with IsBeingCarried, and the carried clip is shown whole.
pub fn layer_frozen(w: &World, f: FighterId) -> bool {
    if source_held(w, f) || w.fighter(f).anim.carried.is_some() {
        return true;
    }
    w.fighter(f).hooks.char.layer_held.is_some_and(|h| h(w, f))
}

/// `relevancyRestart`: the first update that reaches the machines after one or more missed
/// ones (a hold of layerFrozen). FAnimNode_StateMachine::Update_AnyThread (exe 142867fa0) starts every machine with
/// bReinitializeOnBecomingRelevant again at its entry state when its update counter missed a frame, then runs that
/// update's transitions from there (the flags and entry states: FighterData.slotHolds.layer / .machines). AirState
/// has the flag off: it goes on from the state and time it had, so in the air an air state stands. True: the
/// machines were put at the entry states.
/// Deviation: the port holds one state for the whole stack, so AirState's own state is known only while it is the
/// shown one: after a hold that began on the ground (or in the hover) and ends in the air AirState is entered at
/// Fall, where the build goes on from the state it last had.
/// Deviation: a crossfade that was running when the hold began is dropped with the restart, AirState's own too.
fn relevancy_restart(w: &mut World, f: FighterId) -> bool {
    let (data, names) = (w.data.clone(), w.names.clone());
    let p = parts_of(&data, w, f);
    let fi = w.fighter(f);
    if !grounded(fi)
        && !fi.wall.wall_sliding
        && AIR_STATES.contains(&fi.sm.gs)
        && !p.cd.fighter.slot_holds.layer["AirState"].reinit
        && !(p.an.hover.is_some() && crate::chars::iron_giant::giant_gliding(w, f))
    {
        let sm = &mut w.fighter_mut(f).sm;
        sm.sm_active.clear();
        sm.sm_times.clear();
        return false;
    }
    let fi = w.fighter_mut(f);
    fi.sm.sm_active.clear();
    fi.sm.sm_times.clear();
    fi.sm.a2g_times.clear();
    fi.anim.fade_anim = Name::NONE;
    fi.sm.gs = GroundState::Entry;
    fi.sm.gs_time = 0.0;
    let idle = nmo(&names, &p.an.idle);
    set_loco(w, f, idle, 1.0, Some(0.0), Some(SetLocoOpt { restart: true, linear: true, ..SetLocoOpt::default() }));
    true
}

// ---- the carried pose

/// BPI_SetBeingCarried(IsCarried, IsAbove) (BaseFighter ubergraph 106724: broadcast OnIsCarried, bound by Base_AnimBP
/// @27672 to OnCarried): the layer shows the anim set's carried clip while `carried`. Callers: Buff_VelmaAttachToAlly
/// (@5818 true, @1421 false), Car_Buff and C023A_Car_Buff (true, true at the begin; false, false at the end).
/// Unverified: the carried player starts at 0 each time the pose comes on (BlendListByBool_16's reset of its child
/// on activation not read); a call while it is on keeps the time.
pub fn set_being_carried(w: &mut World, f: FighterId, carried: bool, above: bool) {
    let an = &mut w.fighter_mut(f).anim;
    an.carried = if carried { Some(Carried { above, time: an.carried.map_or(0.0, |c| c.time) }) } else { None };
}

/// The carried clip of the fighter's anim set: CarriedAbove with CarryInt 1, else CarriedBelow.
/// Unverified: the base set's clip, also for a character with sets of its own (none of them names another).
fn carried_clip(w: &World, f: FighterId, above: bool) -> Option<&str> {
    let d = &char_data(&w.data, &w.names, w.fighter(f).ch)?.fighter;
    d.anim_set.get(if above { "CarriedAbove" } else { "CarriedBelow" }).and_then(|v| v.as_str())
}

/// The carried player's update: its looped time by the frame (AdvanceTime's Fmod wrap).
fn carried_tick(w: &mut World, f: FighterId) {
    let Some(c) = w.fighter(f).anim.carried else { return };
    let len = carried_clip(w, f, c.above).map_or(0.0, |a| anim_len(&parts_of(&w.data, w, f), a));
    if let Some(c) = &mut w.fighter_mut(f).anim.carried {
        math::ue::advance_time(true, consts::DT, &mut c.time, len);
    }
}

// ---- the locomotion player

/// `setLoco`. The Shaggy_HR_*_Montage assets are referenced by no asset or the exe; the
/// AnimBP plays the notify-free HR sequences, so nothing fires here. The anim set's Anim_Idle names the layer's idle
/// players: the sequence is the variable's value now (idle_anim), looped as that player is.
pub fn set_loco(w: &mut World, f: FighterId, a: Name, rate: Real, blend: Option<crate::Real>, opt: Option<SetLocoOpt>) {
    let o = opt.unwrap_or_default();
    let (loop_, idle) = {
        let p = parts_of(&w.data, w, f);
        (o.looping.unwrap_or_else(|| looping(&w.names, p.an, a)), nmo(&w.names, &p.an.idle))
    };
    let a = if a == idle { idle_anim(w, f) } else { a };
    let an = &mut w.fighter_mut(f).anim;
    if an.loco_anim != a || o.restart {
        an.loco_anim = a;
        an.loco_time = o.time.unwrap_or(0.0);
        an.loco_loop = loop_;
        an.loco_gen += 1;
        an.loco_blend = match blend {
            None => (0.1, false),
            Some(b) => (b, !o.linear),
        };
    }
    an.loco_rate = rate;
}

/// `idleAnim`: the linked base layer's Anim_Idle: the anim set's sequence unless the
/// character's AnimBP wrote another into the variable (CharHooks.idleAnim; HarleyQuinn_AnimBP SetIdleAnim).
pub fn idle_anim(w: &World, f: FighterId) -> Name {
    // `hook?.(f) ?? set.idle`: a hook's name stands, its None too (a hook always returns a string)
    if let Some(h) = w.fighter(f).hooks.char.idle_anim {
        return h(w, f);
    }
    nmo(&w.names, &parts_of(&w.data, w, f).an.idle)
}

/// `idleSwap`: the idle players (the Ground machine's entry state, Idle and IdleReset of
/// IdleSubState) read Anim_Idle at each update of the layer: a write of the variable changes the playing sequence in
/// place. The player and its position stay (no transition, no restart); the position is held inside the new
/// sequence's length.
/// Unverified: FAnimNode_SequencePlayer's update with a changed Sequence pin is not read natively; the clamp is the
/// stock node's.
fn idle_swap(w: &mut World, f: FighterId) {
    let s = w.fighter(f).sm.gs;
    if !matches!(s, GroundState::Idle | GroundState::IdleReset | GroundState::Entry) {
        return;
    }
    let a = idle_anim(w, f);
    if w.fighter(f).anim.loco_anim == a {
        return;
    }
    let (data, names) = (w.data.clone(), w.names.clone());
    let p = parts_of(&data, w, f);
    // an edge player shown (UseEdgeIdle, edgeBlend): the Anim_Idle player is the one blended out
    let edge = p.ex.edge_idle.as_ref().map(|e| nm(&names, &e.idle));
    if s != GroundState::Entry && edge == Some(w.fighter(f).anim.loco_anim) {
        return;
    }
    let len = anim_len(&p, names.str(a));
    let an = &mut w.fighter_mut(f).anim;
    an.loco_anim = a;
    an.loco_time = math::min(an.loco_time, len);
}

/// `shownMontage` (`crate::fighter::shown_montage`) as the instance's number.
pub(crate) fn shown_uid(w: &World, f: FighterId) -> Option<u32> {
    let ms = &w.fighter(f).montages;
    let s = if crate::fighter::shown_montage(w, f)? { &ms.dash_montage } else { &ms.montage };
    s.as_ref().map(|s| s.montage.uid)
}

/// `updatePose`.
pub fn update_pose(w: &mut World, f: FighterId) {
    let (data, names) = (w.data.clone(), w.names.clone());
    let hooks = w.fighter(f).hooks.char;
    let sm = shown_uid(w, f);
    let mp = sm.and_then(|u| inst_pose(&data, w, f, u, None));
    let shaggy = matches!(w.fighter(f).cs, CharState::Shaggy(_));
    if shaggy {
        // Shaggy_AnimBP's own layers: the rage shirt, the kick slots.
        let len = anim_len(&parts_of(&data, w, f), "Shaggy_RageShirt_Idle");
        let fi = w.fighter_mut(f);
        let rage = fi.rage.rage_enabled;
        if let CharState::Shaggy(sh) = &mut fi.cs {
            if rage {
                // its looped sequence player: FAnimationRuntime::AdvanceTime
                math::ue::advance_time(true, consts::DT * 0.75, &mut sh.rage_shirt_time, len);
            }
            // Deviation: a slot montage's blend in / out is taken as hiding the additive at once (the port
            // crossfades the posed result instead of weighting the slot over the additive pose).
            sh.rage_shirt = if rage && mp.is_none() { sh.rage_shirt_time } else { -1.0 };
        }
    }
    if let Some((a, t)) = mp {
        let an = &mut w.fighter_mut(f).anim;
        an.anim = a;
        an.anim_time = t;
    }
    if let Some((mw, bw)) = crate::chars::morty::morty_slot_weights(w, f) {
        match sm {
            Some(u) => blend_slots(w, f, u, mw, bw),
            None => crate::chars::morty::morty_slot_mix(w, f).clear(),
        }
    }
    if let Some(input) = hooks.slot_input {
        // its AnimBP's slot graph from the data (graph_slots)
        match sm {
            Some(u) => graph_slots(w, f, Some(u), input),
            None => slot_mix_of(w, f).slot_mix.clear(),
        }
    }
    if let Some(h) = hooks.pose_updated {
        h(w, f);
    }
    if shaggy {
        let mm = w.fighter(f).montages.montage.as_ref().map(|s| (s.montage.uid, s.walking));
        let kick_w = match &w.fighter(f).cs {
            CharState::Shaggy(sh) => sh.kick_w,
            _ => 1.0,
        };
        let other_w = mm.map_or(0.0, |(_, walking)| if walking { 1.0 - kick_w } else { kick_w });
        let op = match mm {
            Some((u, walking))
                if Some(u) == sm
                    && other_w > 0.0
                    && instance_data(&data, w, f, u)
                        .is_some_and(|m| state_ref(w, f, u).is_some_and(|s| s.kick_split(m))) =>
            {
                inst_pose(&data, w, f, u, Some(!walking))
            }
            _ => None,
        };
        if let CharState::Shaggy(sh) = &mut w.fighter_mut(f).cs {
            sh.kick_mix = op.map(|(anim, time)| crate::chars::shaggy::state::KickMix { anim, time, w: other_w });
        }
    }
    {
        let s = &mut w.fighter_mut(f).sm;
        if s.dash_stop_delay >= 0.0 {
            s.dash_stop_delay -= consts::DT;
            if s.dash_stop_delay <= 0.0 {
                s.dash_stop_delay = -1.0;
                s.dash_stop_blend = true;
            }
        }
    }
    // Shaggy_AnimBP's slots, top down: KickGround / KickAir (bAlwaysUpdateSourcePose), DashSlot (not), DefaultSlot
    // (bAlwaysUpdateSourcePose), then the BaseMachine layer. FAnimNode_Slot updates its source when
    // bAlwaysUpdateSourcePose or the source weight is relevant, so the layer's machines run under every montage
    // except a DashSlot one at full weight (dash, dash turn, claps, Fall_Smash_Land, intros, two taunts), under which
    // they are not updated at all (no transitions, players held). (FAnimNode_Slot::Update_AnyThread, exe 1424945f0:
    // bAlwaysUpdateSourcePose || SourceWeight > 0.00001.)
    let frozen = layer_frozen(w, f);
    let mut restarted = false;
    if frozen {
        w.fighter_mut(f).charge.layer_missed = true;
    } else if w.fighter(f).charge.layer_missed {
        w.fighter_mut(f).charge.layer_missed = false;
        restarted = relevancy_restart(w, f);
    }
    if !frozen && hooks.idle_anim.is_some() {
        idle_swap(w, f);
    }
    let a0 = &w.fighter(f).anim;
    let (gen0, anim0, time0, rate0, loop0) = (a0.loco_gen, a0.loco_anim, a0.loco_time, a0.loco_rate, a0.loco_loop);
    // Entered: a hit state -> Movement this update. On the ground Movement's entry state Ground has its first update
    // in it (ground_machine).
    // Deviation: in the air the machines go on a frame later.
    let hm = if frozen || hover_machine(w, f) { HitMachine::Hit } else { hit_machine(w, f) };
    let fi = w.fighter(f);
    if hm == HitMachine::None || (hm == HitMachine::Entered && grounded(fi) && !fi.wall.wall_sliding) {
        let la = fi.anim.loco_anim;
        if grounded(fi) {
            ground_machine(w, f);
        } else if fi.wall.wall_sliding {
            wall_machine(w, f, la);
        } else {
            air_machine(w, f);
        }
    }
    let cd = super::montages::char_data(&data, &names, w.fighter(f).ch).expect("the fighter's character data");
    let fi = w.fighter_mut(f);
    // AnimNotify_SetAnimIsAttacking's write of this frame's dispatch: the machines above did not see it (the dispatch
    // is behind the graph update), the next update does.
    if fi.charge.anim_attack_off == 1 {
        fi.charge.anim_attack_off = 2;
    }
    // relevancy_restart: the machines' first update ends with no active transition (bSkipFirstUpdateTransition), but
    // for a hit state, entered by MovementAndHitReact, which has the flag off.
    let hit_state = matches!(
        fi.sm.gs,
        GroundState::HitStunned | GroundState::Recovery | GroundState::Bounce | GroundState::HitWobble
    );
    let hr_skip = || cd.fighter.slot_holds.layer["MovementAndHitReact"].skip_first;
    if restarted && !(hit_state && !hr_skip()) {
        fi.sm.sm_active.clear();
        fi.anim.loco_blend = (0.0, false);
    }
    // `f.animSet` is a getter (its hook's live call): each read below takes it again.
    if !frozen && parts_of(&data, w, f).an.fall_space.is_some() {
        fall_space_tick(w, f);
    }
    if !frozen && parts_of(&data, w, f).an.wall_spaces.is_some() {
        wall_space_tick(w, f);
    }
    let len = anim_len(&parts_of(&data, w, f), names.str(w.fighter(f).anim.loco_anim));
    let tick = w.fighter(f).clock.tick;
    w.fighter_mut(f).anim.pose_at = Some(tick);
    if !frozen {
        w.fighter_mut(f).anim.layer_at = Some(tick);
        // The relevant weight of the state the machines ended in, written by its update (AirState's alone is
        // followed: a state of another machine reads 1).
        if !w.fighter(f).sm.sm_times.is_empty() {
            sm_held(w, f, true);
        }
        let gs = w.fighter(f).sm.gs;
        let rel = sm_advance(w, f, if AIR_STATES.contains(&gs) { Some(gs) } else { None });
        w.fighter_mut(f).sm.gs_rel = rel;
        if !w.fighter(f).sm.sm_times.is_empty() {
            sm_held(w, f, false);
        }
        if w.fighter(f).anim.loco_gen != gen0 && !restarted {
            seq_fade_start(w, f, anim0, time0, rate0, loop0);
        }
        let fi = w.fighter_mut(f);
        let an = &mut fi.anim;
        an.seq_from = an.loco_time;
        let mv = consts::DT * an.loco_rate;
        an.seq_delta = mv;
        fi.sm.gs_time += consts::DT;
        if fi.sm.gs == GroundState::A2g {
            fi.sm.a2g_times.insert(an.loco_anim, an.loco_time + mv);
        }
        if an.loco_loop {
            // the sequence player's FAnimationRuntime::AdvanceTime (142816220): the Fmod wrap, + End below 0
            math::ue::advance_time(true, mv, &mut an.loco_time, len);
        } else {
            // Deviation: a sequence that does not loop runs on past its end ; the build's
            // AdvanceTime holds it at the end. Its readers clamp to the length (loco_clip) or test the time left, which
            // is <= 0 either way.
            an.loco_time += mv;
        }
    }
    carried_tick(w, f);
    if let Some((a, _)) = mp {
        w.fighter_mut(f).anim.anim_key = AnimKey::Montage(a);
        return; // the montage plays over the state machines
    }
    let (anim, time) = loco_clip(w, f);
    let carried = w.fighter(f).anim.carried.is_some();
    let an = &mut w.fighter_mut(f).anim;
    // (the carried clip is keyed by its name, as a montage's: the view crossfades on a change of key)
    an.anim_key = if carried { AnimKey::Montage(anim) } else { AnimKey::Gen(an.loco_gen) };
    an.anim = anim;
    an.anim_time = time;
    let p = parts_of(&data, w, f);
    if carried {
        // the carried clip alone: no blend space of the machine's under it
        if p.an.fall_space.is_some() || p.an.wall_spaces.is_some() {
            slot_mix_of(w, f).slot_mix.clear();
        }
        return;
    }
    if p.an.fall_space.is_some() || p.an.wall_spaces.is_some() {
        if hooks.slot_mix.is_none_or(|h| h(w, f).is_none()) {
            let space = if p.an.fall_space.is_some() { "FallBlendSpace" } else { "BlendSpace_WallIdle" };
            panic!("{}: {space} needs a slot mix (CharHooks.slotMix)", names.str(w.fighter(f).ch));
        }
        let mut out = std::mem::take(&mut slot_mix_of(w, f).slot_mix);
        if p.an.wall_spaces.is_some() && w.fighter(f).sm.gs == GroundState::Wall {
            wall_mix(w, f, &mut out);
        } else {
            fall_mix(w, f, &mut out);
        }
        slot_mix_of(w, f).slot_mix = out;
    }
    // A slot graph with a SequencePlayer or a state machine over the locomotion (C023A_AnimBP's Driving under
    // IsDriving, C023B_AnimBP's Bite_Loop under IsAttached): walked with no montage shown too (with one, graph_slots
    // above has posed it); it writes the mix only when one of them shows.
    // Deviation: a fall or wall blend space mix under it is taken as its heaviest clip there.
    if let Some(input) = hooks.slot_input
        && sm.is_none()
        && has_players(&p.cd.fighter.slot_holds.graph)
    {
        graph_slots(w, f, None, input);
    }
}

/// Base_AnimBP Movement machine: Air -> Wall on IsWallSliding (0.05 s). The Wall machine reinitialises on becoming
/// relevant: Wall Enter (Anim_WallEnter = Shaggy_Wall_Grab, not looped; UseWallBlendSpace false for Shaggy) -> Wall
/// Idle (Anim_WallIdle = Shaggy_Wall_Pose, looped) once GetRelevantAnimTimeRemainingFraction < 0.1 (0.3 s). With
/// UseWallBlendSpace (AnimSet.wallSpaces, C018) each state plays its blend space instead (BlendListByBool_21 / _22
/// pose 0, never switched after the first update: the bool is set at the instance's initialisation), its clip the
/// heaviest row (wall_head) and the state known by the space its clip is a sample of; the relevant time is the
/// heaviest sample's, as wall_space_tick keeps it.
/// Wall -> WallJump (0 s) on IsJumping; WallJump (never shown) -> Air (0 s) has a constant true rule, so the chain
/// ends in Air the same frame: AirState restarts at Fall, and Fall -> Jump1 follows once the slide ends
/// (JumpingAndJump1). While both IsWallSliding and IsJumping hold, Air -> Wall -> WallJump -> Air repeats each frame.
fn wall_machine(w: &mut World, f: FighterId, la: Name) {
    let (data, names) = (w.data.clone(), w.names.clone());
    let p = parts_of(&data, w, f);
    w.fighter_mut(f).sm.gs = GroundState::Wall;
    let ws = p.an.wall_spaces.as_ref();
    let entering = match ws {
        Some(ws) => in_space(&ws.enter, names.str(la)),
        None => la == nmo(&names, &p.an.wall_enter),
    };
    if entering {
        let len = anim_len(&p, names.str(la));
        if (len - w.fighter(f).anim.loco_time) / len < 0.1 {
            match ws {
                Some(ws) => {
                    let (a, r) = (wall_head(w, f, &ws.idle), wall_rate(w, f, &ws.idle));
                    let o = SetLocoOpt { looping: Some(ws.idle.looping), ..SetLocoOpt::default() };
                    set_loco(w, f, a, r, Some(0.3), Some(o));
                }
                None => set_loco(w, f, nmo(&names, &p.an.wall_idle), 1.0, Some(0.3), None),
            }
        }
    } else {
        let idle = match ws {
            Some(ws) => in_space(&ws.idle, names.str(la)),
            None => la == nmo(&names, &p.an.wall_idle),
        };
        if !idle {
            match ws {
                Some(ws) => {
                    let (a, r) = (wall_head(w, f, &ws.enter), wall_rate(w, f, &ws.enter));
                    let o = SetLocoOpt { looping: Some(ws.enter.looping), ..SetLocoOpt::default() };
                    set_loco(w, f, a, r, Some(0.05), Some(o));
                }
                None => set_loco(w, f, nmo(&names, &p.an.wall_enter), 1.0, Some(0.05), None),
            }
        }
    }
    if w.fighter(f).moves.cmd_jumped {
        let (a, r) = (fall_anim(w, f), fall_rate(w, f));
        set_gs(w, f, GroundState::Fall, a, 0.0, GsOpt { linear: false, rate: Some(r) });
    }
}

/// `locoClip`: the state machines' clip and its time as they are after this tick's update
/// (the time held to this clip's own length); an AnimComposite plays its segments back to back (no blend between
/// them).
pub fn loco_clip(w: &World, f: FighterId) -> (Name, Real) {
    let p = parts_of(&w.data, w, f);
    let an = &w.fighter(f).anim;
    // the carried pose in place of the machine's (layer_frozen)
    if let Some(c) = an.carried
        && let Some(a) = carried_clip(w, f, c.above)
    {
        return (nm(&w.names, a), c.time);
    }
    let la = w.names.str(an.loco_anim);
    let t = math::min(an.loco_time, anim_len(&p, la));
    match p.an.composites.get(la) {
        Some(comp) => {
            let n = crate::math::narrow;
            let seg = comp
                .iter()
                .find(|s| t < n(s.start) + n(s.len))
                .or(comp.last())
                .expect("an AnimComposite with a segment");
            (nm(&w.names, &seg.anim), math::min(t - n(seg.start), n(seg.len)))
        }
        None => (an.loco_anim, t),
    }
}

/// `animSetSwitched`: a character AnimBP that blends two instances of the base graph
/// (c016_AnimBP's BlendListByBool @14 over the LinkedAnimLayer of Base_AnimBP_C016 and the LinkedAnimGraph of
/// Base_AnimBP_NoBall_C016, input HasBall || "Is In Menu" @3619-3657). The pawn's code calls this where its graph
/// writes the node's input (OnSetHasBall @7110), after its own state changed, with the set from before; nothing
/// happens when the hook names the same set. The node: the weights go linearly to the new child over BlendTime[new]
/// (0.1 s both ways), and a child at weight 0 is not updated, so the instance shown stops and the other one starts at
/// its entry states (the Ground machine's entry on the ground, Fall in the air). The blend is the port's one
/// crossfade (linear, BlendTime of the new set).
/// Unverified: that the instance becoming relevant starts at its entry states is taken from the stock 4.26 nodes
/// (FAnimNode_LinkedAnimGraph, a machine with bReinitializeOnBecomingRelevant); not read in the exe.
/// Deviation: one set of machine state stands for the two instances, the graph's variables included; the instance
/// left behind is the frozen pose of the crossfade and its last clip's sequence notifies for the blend's time. A
/// switch back inside the 0.1 s is a new switch of the full time from the entry states.
pub fn anim_set_switched(w: &mut World, f: FighterId, from: SetRef) {
    let to = crate::fighter::anim_set(w, f);
    if to == from {
        return;
    }
    let (data, names) = (w.data.clone(), w.names.clone());
    let cd = char_data(&data, &names, w.fighter(f).ch).expect("the fighter's character data");
    let name = match to {
        SetRef::Named(k) if cd.def.anim_sets.as_ref().is_some_and(|s| s.contains_key(names.str(k))) => names.str(k),
        _ => "base",
    };
    let Some(time) = cd.def.anim_set_blend.as_ref().and_then(|b| b.blend_time.get(name)).copied() else {
        warn_text(w, &format!("animSetSwitched: no blend time for the set {name} (CharDef.animSetBlend)"));
        return;
    };
    let p = parts_of(&data, w, f);
    let fi = w.fighter_mut(f);
    let a = &fi.anim;
    let (anim0, time0, rate0, loop0) = (a.loco_anim, a.loco_time, a.loco_rate, a.loco_loop);
    fi.sm.sm_active.clear();
    fi.sm.sm_times.clear();
    fi.sm.a2g_times.clear();
    let g = grounded(fi);
    fi.sm.gs = if g { GroundState::Entry } else { GroundState::Fall };
    fi.sm.gs_time = 0.0;
    let (a, r) = if g { (nmo(&names, &p.an.idle), 1.0) } else { (nmo(&names, &p.an.fall), FALL_RATE) };
    set_loco(
        w,
        f,
        a,
        r,
        Some(crate::math::narrow(time)),
        Some(SetLocoOpt { restart: true, linear: true, ..SetLocoOpt::default() }),
    );
    seq_fade_start(w, f, anim0, time0, rate0, loop0);
}

/// `FALL_RATE`: AirState Fall's sequence player PlayRate.
pub const FALL_RATE: Real = 1.5;

/// `setHitReactInfo`: DamageAndLaunch, before its SetHitPauseTime (@9984, 10701, then
/// @10735 -> 3589 -> 4974): HitReactionMulticast -> AddHitStunFromKnockBack (past the zero test) stores
/// KnockBackVelocity = KnockbackLaunchVelocity, then HitDispatchMulticast -> SetHitReactInfo(HitVelocity =
/// |KnockbackLaunchVelocity|): IsNeutralHit = HitVelocity < 1500, then HitReactInt = RandomInteger(MaxNeutralHR 4),
/// stepped on (wrapping) when it repeats the last one. `store` false: a hit HitReactionMulticast's zero test skips
/// (Comp_Pawn_DamageManager @2025-2456) runs no AddHitStunFromKnockBack, so KnockBackVelocity keeps the last one;
/// HitDispatchMulticast (@10701) still runs.
pub fn set_hit_react_info(w: &mut World, f: FighterId, vx: Real, vy: Real, store: bool) {
    let fi = w.fighter_mut(f);
    if store {
        fi.react.hit_stun_kb_vx = vx;
        fi.react.hit_stun_kb_vy = vy;
    }
    fi.react.hit_neutral = math::ue::size([0.0, vx, vy]) < 1500.0;
    let r = math::to_int32(fi.rng.int(4.0));
    let h = fi.react.hit_react_int;
    fi.react.hit_react_int = if r == h { if h + 1 > 3 { 0 } else { h + 1 } } else { r };
}

/// `stateName`: a debug label of what the fighter does.
pub fn state_name(w: &World, f: FighterId) -> String {
    let fi = w.fighter(f);
    let d = fi.life.dead;
    if d != 0.0 && !d.is_nan() {
        return "dead".into();
    }
    if fi.hit.hitpause != 0.0 && !fi.hit.hitpause.is_nan() {
        return "hitpause".into();
    }
    if fi.hit.hitstun != 0.0 && !fi.hit.hitstun.is_nan() {
        return "hitstun".into();
    }
    match &fi.montages.montage {
        Some(m) => {
            let n = w.names.str(m.montage.name);
            let n = n.strip_prefix("Shaggy_").unwrap_or(n);
            n.strip_suffix("_Montage").unwrap_or(n).to_owned()
        }
        None if grounded(fi) => "ground".into(),
        None => "air".into(),
    }
}

/// `FollowedMove`: what an actor's AnimBP keeps of the four BaseFighter movement delegates
/// it binds on the fighter it follows (JerryAnimBlueprint @2368-2560, BMOAnimBlueprint @1834-2026): OnIsWallSliding,
/// OnJumpNumberChange, OnIsJumping, OnIsFastFalling.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct FollowedMove {
    pub wall_sliding: bool,
    pub jump_number: i32,
    pub jumping: bool,
    pub fast_falling: bool,
}

/// `animFastFalling`: the AnimBP's IsFastFalling, the last OnIsFastFalling value.
/// Deviation: the wall jump @9336, ResetFighterLogic @1069, Pawn_C017's hover @6362 and JakeStretch @28788 clear
/// IsFastFalling with no broadcast, so the build's copy stays true there until the next one; the port's goes false.
pub fn anim_fast_falling(w: &World, f: FighterId) -> bool {
    let ff = &w.fighter(f).fast_fall;
    ff.fast_falling && ff.anim_fast_fall_off != Some(true)
}

/// `followedMove`.
/// Deviation: OnJumpNumberChange is also sent with JumpImpulsesReceived (BaseFighter @2217, @21396), which this does
/// not keep apart from CurrentJumpNumber.
pub fn followed_move(w: &World, f: FighterId) -> FollowedMove {
    let fi = w.fighter(f);
    FollowedMove {
        wall_sliding: fi.wall.wall_sliding,
        jump_number: fi.jump.jumps,
        jumping: fi.moves.cmd_jumped,
        fast_falling: anim_fast_falling(w, f),
    }
}

/// CharHooks.slotMix's mix, written (a character whose code reads it has the hook).
pub(crate) fn slot_mix_of(w: &mut World, f: FighterId) -> &mut SlotMix {
    let ch = w.fighter(f).ch;
    if crate::registry::char_hooks::slot_mix_mut(w, f).is_none() {
        panic!("{}: CharHooks.slotInput needs a slot mix (CharHooks.slotMix)", w.names.str(ch));
    }
    crate::registry::char_hooks::slot_mix_mut(w, f).expect("the slot mix")
}

#[cfg(test)]
mod tests {
    use super::f_interp_to;

    /// The oracle's `fInterpTo` on the same inputs (a probe of the oracle).
    #[test]
    fn f_interp_to_matches_the_oracle() {
        let cases: [(f64, f64, f64, f64, f64); 5] = [
            (0.0, 10.0, 0.0166668, 8.0, 1.3333439826965332),
            (3.5, -2.25, 0.0000833, 180.0, 3.4137845039367676),
            (1.0, 1.00001, 0.0166668, 5.0, 1.00001),
            (-7.0, 4.0, 0.0166668, 0.0, 4.0),
            (2.0, 9.0, 0.5, 4.0, 9.0),
        ];
        for (cur, target, dt, speed, want) in cases {
            assert_eq!(
                f64::from(f_interp_to(
                    mvs_sim::math::narrow(cur),
                    mvs_sim::math::narrow(target),
                    mvs_sim::math::narrow(dt),
                    mvs_sim::math::narrow(speed)
                ))
                .to_bits(),
                f64::from(mvs_sim::math::narrow(want)).to_bits(),
                "{cur} {target} {dt} {speed}"
            );
        }
    }
}
