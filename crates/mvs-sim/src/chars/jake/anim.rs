//! Jake_AnimBP's sim part: the slot inputs of his slot mix, the Origins skin's stand-in
//! pose, the EatOverride layer's machine (the swallow's idle, fall, land, jumps and walk) and its pose shapes. pawn.rs
//! enters these as Jake's `slot_input`, `pose`, `layer_held`, `anim_update` and `pose_updated` hooks.
//!
//! Jake_AnimBP's sim part (offsets of its ubergraph; the node data from its json), the poses its graph puts under the
//! hit and hurt shapes:
//!   - the slot graph from the data (`FighterData.slotHolds.graph`, walked by `fighter::anim` `graph_slots` and
//!     `fighter::anim::slot_blend` from `slot_input` below): the forward-air aim punch's 10 slots (AimPunchBase / Up /
//!     Down / Up2 / Down2 and the five Short ones) under the TwoWayBlends of AimDistanceAlpha and AimDirection /
//!     AimDirectionInverted (@4184-7088, @3055), Weapon_Charge_Attack_U's ShortAttack / ReachAttack by ChargeAmount,
//!     the spit montages' SpitBase / Up / Down by AimDirection / AimDirectionInverted (the root's two TwoWayBlends),
//!     Horse Montage2's JumpSlot / FallingBlend under BlendListByBool_3 on IsFalling (timed: `JakePawnState::slot_mix`);
//!   - the EatOverride machine (StateMachine_1) under BlendListByBool_1 on EatOverrideBool (`eat_tick`, `eat_sample`);
//!   - ModifyBone_13 on fml_un_COM at the top of the graph (Root <- the mirror and the head ModifyBones <- ModifyBone_13
//!     <- the slot graph): BMM_Replace in component space, Rotation = MakeRotator(0, StretchDirection, 0), Alpha =
//!     StretchOn (@2898-3018): the turn `turn_boxes` gives (the same node as Base_AnimBP's hit pitch), over
//!     everything, montages too.
//!
//! Read from `JakeState::anim` (the setters of pawn.rs) and written into `JakePawnState` (the pawn record).
//!
//! Deviation: the skin is the shipped default (`FighterData.skin`, Jake): IsOriginsSkin is false, so the four
//! TwoWayBlends on it take their A side and BlendListByBool_4 its pose 1 (StateMachine_1 EatOverride). The Origins
//! machine (StateMachine, OriginsEatOverride) is not ported: `slot_input` gives it no value, and the walk refuses it on
//! Jake_S02.
//! Deviation: ApplyAdditive_1 (AdditiveShake over SpitAlly: Jake_Eat_Attack_Struggle_Montage, the held fighter's
//! struggle) takes its base pose alone in the shapes (`graph_slots`): the additive shake moves no shape.
//! Deviation: Jake_Eat_Attack_Montage has a track in OriginsSkin (Jake_S02_Eat_Attack_In / Hit) beside DefaultSlot, and
//! the OriginsSkin Slot node (bAlwaysUpdateSourcePose) lies above DefaultSlot in the graph (OverrideSlot <- OriginsSkin
//! <- DefaultSlot, the json's Source links), so the build shows the S02 skin's track on every skin. The S02 clips are not
//! converted (no box track, no drawn clip): `origins_stand_in` puts the DefaultSlot track's clip at the same place in
//! their stead.

use super::state::{JakePawnState, jake, jake_w};
use crate::Real;
use crate::consts::DELTA_SECONDS;
use crate::fighter::FighterId;
use crate::fighter::anim::box_pose::{put_pose, take_pose, turn_boxes};
use crate::fighter::anim::montages::{char_data, instance_data, state_ref};
use crate::fighter::anim::slot_blend::ZERO_W;
use crate::fighter::anim::update::{nm, shown_uid};
use crate::fighter::anim::{AnimKey, anim_direction, hit_pitch, source_held};
use crate::fighter::common::{data_of, warn_text};
use crate::fighter::moves::is_falling;
use crate::math::min;
use crate::math::ue::advance_time;
use crate::name;
use crate::names::Name;
use crate::world::World;
use mvs_data::boxes::BoxAnim;
use mvs_data::fighter::Map;

/// The EatOverride machine's states (`EAT_IDLE` .. `EAT_JUMP2`).
pub const EAT_IDLE: i32 = 0;
pub const EAT_FALL: i32 = 1;
pub const EAT_LAND: i32 = 2;
pub const EAT_JUMP: i32 = 3;
pub const EAT_JUMP2: i32 = 4;

/// Each state's clip (`EAT_CLIPS`, by state).
pub const EAT_CLIPS: [Name; 5] = [
    name!("Jake_Eat_Attack_Idle"),
    name!("Jake_Eat_Attack_Fall"),
    name!("Jake_Eat_Attack_Land"),
    name!("Jake_Eat_Attack_Jump"),
    name!("Jake_Eat_Attack_Jump"),
];

/// The idle's walk blend clip (`EAT_WALK`).
pub const EAT_WALK: Name = name!("Jake_Eat_Attack_Walk");

/// The inputs the machine reads (`EAT_INPUTS`).
pub const EAT_INPUTS: [&str; 5] = ["Moving", "VelocityZ", "IsFalling", "IsJumping", "CurrentJumpNumber"];

const EAT_LOOP: [bool; 5] = [true, true, false, false, false];
/// BlendListByBool_2's BlendTime per child.
const MOVING_BLEND: [Real; 2] = [0.1, 0.1];
const MAX_TRANSITIONS: usize = 3;

/// IsOriginsSkin: Comp_Pawn_SkinManager.AppliedSkin == Jake_S02, at the instance's initialization @8379-8490.
fn origins_skin(w: &World, f: FighterId) -> bool {
    data_of(w, f).fighter.skin.name == "Jake_S02"
}

fn flag(on: bool) -> Real {
    if on { 1.0 } else { 0.0 }
}

// ---------------------------------------------------------------- the slot graph's inputs

/// `slotInput` (CharHooks.slotInput): the value of one of the graph's inputs as the AnimBP holds it at this pose update.
///   AimDirection, AimDistanceAlpha, ChargeAmount, EatOverrideBool: the variables (pawn.rs's setters, the charge).
///   AimDirectionInverted = AimDirection x -1, written with it (BPI_SetAimDirection @8544 -> @3055-3097).
///   IsFalling: the variable of the update @2809. IsOriginsSkin: above.
///   The node-named alphas (their exposed inputs' handlers): TwoWayBlend / _1 / _2 / _3 = IsOriginsSkin as a float
///   (@3229, @3596, @3670, @3744); _13 / _8 = AimDirection x 2 (@4184, @6846); _12 / _7 = AimDirectionInverted x 2
///   (@4263, @6925); _10 / _6 = AimDirection x 2 - 1 (@6117, @7004); _5 / _9 = AimDirectionInverted x 2 - 1 (@6604,
///   @6725). The walk clamps an alpha to 0..1.
///   ApplyAdditive_1 / ApplyAdditive: 0, the base pose alone (the Deviation above). StateMachine_1: 0, the EatOverride
///   machine this file updates and poses (`slot_blend::SlotGraphIn::source`).
pub fn slot_input(w: &World, f: FighterId, input: Name) -> Real {
    let a = &jake(w, f).anim;
    let inv = -a.aim_direction;
    if input == name!("AimDirection") {
        a.aim_direction
    } else if input == name!("AimDirectionInverted") {
        inv
    } else if input == name!("AimDistanceAlpha") {
        a.aim_distance_alpha
    } else if input == name!("ChargeAmount") {
        a.charge_amount
    } else if input == name!("EatOverrideBool") {
        flag(a.eat_override)
    } else if input == name!("IsFalling") {
        flag(is_falling(w, f))
    } else if input == name!("IsOriginsSkin")
        || input == name!("AnimGraphNode_TwoWayBlend")
        || input == name!("AnimGraphNode_TwoWayBlend_1")
        || input == name!("AnimGraphNode_TwoWayBlend_2")
        || input == name!("AnimGraphNode_TwoWayBlend_3")
    {
        flag(origins_skin(w, f))
    } else if input == name!("AnimGraphNode_TwoWayBlend_13") || input == name!("AnimGraphNode_TwoWayBlend_8") {
        a.aim_direction * 2.0
    } else if input == name!("AnimGraphNode_TwoWayBlend_12") || input == name!("AnimGraphNode_TwoWayBlend_7") {
        inv * 2.0
    } else if input == name!("AnimGraphNode_TwoWayBlend_10") || input == name!("AnimGraphNode_TwoWayBlend_6") {
        (a.aim_direction * 2.0) - 1.0
    } else if input == name!("AnimGraphNode_TwoWayBlend_5") || input == name!("AnimGraphNode_TwoWayBlend_9") {
        (inv * 2.0) - 1.0
    } else if input == name!("AnimGraphNode_ApplyAdditive_1")
        || input == name!("AnimGraphNode_ApplyAdditive")
        || input == name!("AnimGraphNode_StateMachine_1")
    {
        0.0
    } else {
        Real::NAN
    }
}

/// `originsStandIn` (CharHooks.poseUpdated, after `graph_slots` composed the shown montage's pose): the OriginsSkin
/// track the walk took (the header's Deviation) has no box track; the DefaultSlot track's clip at its time stands in
/// for it (the fighter's anim / anim time: the shapes and the drawn pose). A clip the index has is left as it is.
pub fn origins_stand_in(w: &mut World, f: FighterId) {
    let Some(uid) = shown_uid(w, f) else { return };
    let an = &w.fighter(f).anim;
    if data_of(w, f).boxes.anims.contains_key(w.names.str(an.anim)) {
        return;
    }
    let (Some(m), Some(s)) = (instance_data(&w.data, w, f, uid), state_ref(w, f, uid)) else { return };
    let slot_pose = |slot: &str| {
        let (si, gi, t) = s.pose_in(m, slot)?;
        Some((nm(&w.names, &m.slots[si].segments[gi].anim), t))
    };
    let Some(o) = slot_pose("OriginsSkin") else { return };
    // The shown time went through the pose's float (`SlotPose.time`): compared at that width.
    if o.0 != an.anim || crate::math::narrow(o.1) != an.anim_time {
        return;
    }
    let Some(d) = slot_pose("DefaultSlot") else { return };
    let an = &mut w.fighter_mut(f).anim;
    an.anim = d.0;
    an.anim_time = crate::math::narrow(d.1);
}

// ---------------------------------------------------------------- the EatOverride machine
// StateMachine_1 (StateMachineIndexInClass 1; MaxTransitionsPerFrame 3, bSkipFirstUpdateTransition and
// bReinitializeOnBecomingRelevant: entry Idle, FighterData.slotHolds.machines), its states' players (PlayRate 1) and
// transitions in each state's order (the rules' graphs; crossfades HermiteCubic):
//   Idle: BlendListByBool_2 on Moving (BlendTime [0.1, 0.1]; true -> pose 0) over Jake_Eat_Attack_Walk / _Idle (both
//     looped). -> Jump 0 s: IsJumping && CurrentJumpNumber == 1 (TR_11 @4918); -> Fall 0.1 s: VelocityZ <= 0 &&
//     IsFalling (TR_12 @7215).
//   Fall: Jake_Eat_Attack_Fall (looped). -> Jump2 0 s: IsJumping && CurrentJumpNumber == 2 (TR_13 @7316); -> Land 0 s:
//     !IsFalling (TR_14).
//   Land: Jake_Eat_Attack_Land. -> Jump 0 s: IsJumping && CurrentJumpNumber == 1 (TR_15 @2577); -> Idle 0.15 s: the
//     state's time remaining <= 0.15 (TR_16 @1688).
//   Jump: Jake_Eat_Attack_Jump. -> Land 0 s: !IsJumping && !IsFalling (TR_17 @606); -> Jump2 0 s: IsJumping &&
//     CurrentJumpNumber == 2 (TR_18 @1058); -> Fall 0.1 s: time remaining <= 0.1 (TR_19 @1486).
//   Jump2: Jake_Eat_Attack_Jump. -> Land 0 s: !IsJumping && !IsFalling (TR_20 @2092); -> Fall 0.1 s: time remaining
//     <= 0.1 (TR_21 @2217).
// The inputs: Moving = VSize(Velocity) >= 10 (@7779-7924), VelocityZ (@7943-8063), IsFalling (@2809), IsJumping /
// CurrentJumpNumber (Fighter's OnIsJumping / OnJumpNumberChange: `cmd_jumped` / `jumps`, as the AirState machine reads
// them, `fighter::anim::followed_move`).

/// `eatRelevant`: the machine is relevant (updated) this frame: BlendListByBool_1 (BlendTime 0 0: its active child
/// alone) on EatOverrideBool, BlendListByBool_4's pose 1 on the default skin, and the Slot nodes above: DefaultSlot,
/// OriginsSkin, JumpSlot and FallingBlend have bAlwaysUpdateSourcePose, the rest hold their source under a full-weight
/// montage of one of the data's slot sets (`source_held`: OverrideSlot, the spit, charge and aim punch slots).
pub fn eat_relevant(w: &World, f: FighterId) -> bool {
    jake(w, f).anim.eat_override && !origins_skin(w, f) && !source_held(w, f)
}

/// The box tracks' clip table of Jake's data (the lengths and rates of the players).
type Anims<'a> = &'a Map<BoxAnim>;

/// What the transitions read of the fighter this update.
struct Inputs {
    jumping: bool,
    jump_number: i32,
    vy: Real,
    falling: bool,
}

/// GetRelevantAnimTimeRemaining: the sequence's length less the player's time.
fn remaining(anims: Anims, names: &crate::names::Names, p: &JakePawnState, st: usize) -> Real {
    let len = anims.get(names.str(EAT_CLIPS[st])).map_or(0.0, |a| a.length);
    crate::math::narrow(len) - p.eat_times[st]
}

/// The first transition of the state whose rule passes: (target, crossfade time), or none.
fn transition(
    anims: Anims,
    names: &crate::names::Names,
    p: &JakePawnState,
    st: i32,
    i: &Inputs,
) -> Option<(i32, Real)> {
    let (jumping, jn, falling) = (i.jumping, i.jump_number, i.falling);
    let rem = |s: i32| remaining(anims, names, p, usize::try_from(s).expect("an eat state"));
    match st {
        EAT_IDLE => {
            if jumping && jn == 1 {
                return Some((EAT_JUMP, 0.0));
            }
            if i.vy <= 0.0 && falling {
                return Some((EAT_FALL, 0.1));
            }
            None
        }
        EAT_FALL => {
            if jumping && jn == 2 {
                return Some((EAT_JUMP2, 0.0));
            }
            if !falling {
                return Some((EAT_LAND, 0.0));
            }
            None
        }
        EAT_LAND => {
            if jumping && jn == 1 {
                return Some((EAT_JUMP, 0.0));
            }
            if rem(st) <= 0.15 {
                return Some((EAT_IDLE, 0.15));
            }
            None
        }
        EAT_JUMP => {
            if !jumping && !falling {
                return Some((EAT_LAND, 0.0));
            }
            if jumping && jn == 2 {
                return Some((EAT_JUMP2, 0.0));
            }
            if rem(st) <= 0.1 {
                return Some((EAT_FALL, 0.1));
            }
            None
        }
        _ => {
            if !jumping && !falling {
                return Some((EAT_LAND, 0.0));
            }
            if rem(st) <= 0.1 {
                return Some((EAT_FALL, 0.1));
            }
            None
        }
    }
}

/// A state entered from weight 0 is initialized: its players at 0 (and Idle's list at INDEX_NONE). One still fading
/// out (the last transition's source) keeps its players.
fn enter(p: &mut JakePawnState, st: i32) {
    if st == p.fade_from && p.fade_t < p.fade_dur {
        return;
    }
    p.eat_times[usize::try_from(st).expect("an eat state")] = 0.0;
    if st == EAT_IDLE {
        p.walk_time = 0.0;
        p.walk_last = -1;
        p.walk_w = 0.0;
    }
}

/// One player's advance by dt (PlayRate 1 x the sequence's RateScale), FAnimationRuntime::AdvanceTime: a looped one
/// wraps (FMath::Fmod), another stops at its end.
fn advance(anims: Anims, names: &crate::names::Names, clip: Name, t: Real, dt: Real, looped: bool) -> Real {
    let Some(a) = anims.get(names.str(clip)) else { return t };
    let mut time = t;
    advance_time(looped, dt * crate::math::narrow(a.rate_scale), &mut time, crate::math::narrow(a.length));
    time
}

/// Idle's list on Moving by dt (FAlphaBlend, `slot_blend`'s `step_list` rule): the active child, the switch, the step;
/// the first update after the state's initialization snaps.
fn walk_list(p: &mut JakePawnState, moving: bool, dt: Real) {
    let child: i32 = if moving { 0 } else { 1 };
    if child != p.walk_last {
        let wc = if child == 0 { p.walk_w } else { 1.0 - p.walk_w };
        let t = if p.walk_last < 0 {
            0.0
        } else {
            MOVING_BLEND[usize::try_from(child).expect("a list child")] * min((1.0 - wc).abs(), 1.0)
        };
        p.walk_last = child;
        p.walk_from = p.walk_w;
        p.walk_dur = t;
        p.walk_rem = t;
    }
    p.walk_rem = if p.walk_rem > dt { p.walk_rem - dt } else { 0.0 };
    let to = if child == 0 { 1.0 } else { 0.0 };
    p.walk_w = if p.walk_dur > 0.0 { to + (p.walk_from - to) * (p.walk_rem / p.walk_dur) } else { to };
}

/// A state's players by dt (a state at a weight above 0: the current one, and the one the crossfade leaves).
fn state_update(anims: Anims, names: &crate::names::Names, p: &mut JakePawnState, st: i32, dt: Real, moving: bool) {
    let k = usize::try_from(st).expect("an eat state");
    if st != EAT_IDLE {
        p.eat_times[k] = advance(anims, names, EAT_CLIPS[k], p.eat_times[k], dt, EAT_LOOP[k]);
        return;
    }
    walk_list(p, moving, dt);
    if p.walk_w > ZERO_W {
        p.walk_time = advance(anims, names, EAT_WALK, p.walk_time, dt, true);
    }
    if 1.0 - p.walk_w > ZERO_W {
        p.eat_times[k] = advance(anims, names, EAT_CLIPS[k], p.eat_times[k], dt, true);
    }
}

/// `eatTick`: the machine's update by dt: the restart after a missed update, the transitions (on the players' times of
/// the update before; up to MaxTransitionsPerFrame, each from the state the last one entered), then the states'
/// players.
// Deviation: one crossfade is kept (the last transition's); a transition taken while another runs drops the older
// one's source from the update (the build keeps every active transition's states updated). Render only: the shapes
// take the state alone (`eat_sample`).
fn eat_tick(w: &mut World, f: FighterId, dt: Real) {
    let fi = w.fighter(f);
    let inp =
        Inputs { jumping: fi.moves.cmd_jumped, jump_number: fi.jump.jumps, vy: fi.body.vy, falling: is_falling(w, f) };
    // VSize >= 10 (by the squares)
    let moving = fi.body.vx * fi.body.vx + fi.body.vy * fi.body.vy >= 100.0;
    let data = w.data.clone();
    let cd = char_data(&data, &w.names, fi.ch).expect("the fighter's character data");
    let anims = &cd.boxes.anims;
    let names = w.names.clone();
    let p = &mut jake_w(w, f).pawn;
    let first = p.eat_first;
    if first {
        p.fade_from = -1;
        p.fade_dur = 0.0;
        p.fade_t = 0.0;
        p.eat = EAT_IDLE;
        enter(p, EAT_IDLE);
        p.eat_first = false;
    }
    for _ in 0..MAX_TRANSITIONS {
        let Some(t) = transition(anims, &names, p, p.eat, &inp) else { break };
        let from = p.eat;
        enter(p, t.0);
        p.eat = t.0;
        p.fade_from = if t.1 > 0.0 { from } else { -1 };
        p.fade_dur = t.1;
        p.fade_t = 0.0;
    }
    if first {
        // bSkipFirstUpdateTransition: the first update's transitions leave no crossfade
        p.fade_from = -1;
        p.fade_dur = 0.0;
    }
    if p.fade_from >= 0 {
        p.fade_t += dt;
        if p.fade_t >= p.fade_dur {
            p.fade_from = -1;
        } else if p.fade_from != p.eat {
            let from = p.fade_from;
            state_update(anims, &names, p, from, dt, moving);
        }
    }
    let st = p.eat;
    state_update(anims, &names, p, st, dt, moving);
}

// ---------------------------------------------------------------- the AnimBP's update

/// `animUpdate` (CharHooks.animUpdate after the montages: the variables and players the graph's evaluation of this
/// frame reads): BlueprintUpdateAnimation's StretchDirection = GetDirection() while CalculateStretchAngle (@2828-2870:
/// the Base_AnimBP rule, `anim_direction`), then the EatOverride machine while it is relevant (`eat_relevant`; a frame
/// it is not, its next update is a first update: the restart at Idle). `dilation`: the hit pause's (the instance's
/// delta).
/// HeadScale: written by the pawn's HeadScale event (@2741-2848: its vector) from BaseFighter's BPI_SetHeadScale
/// @110502-110539 (Conv_FloatToVector of Comp_State_Manager UpdatePlayerStatus's head size), whose value is the
/// fighter's `pose.head_scale` (`combat::buffs` writes it where the build makes the call): copied here, ahead of the
/// drawing that reads it.
pub fn anim_update(w: &mut World, f: FighterId, dilation: Real, pre: bool) {
    if pre {
        return;
    }
    if jake(w, f).anim.calculate_stretch_angle {
        let (vx, vy) = (w.fighter(f).body.vx, w.fighter(f).body.vy);
        let d = anim_direction(w, f, vx, vy);
        jake_w(w, f).anim.stretch_direction = d;
    }
    let h = w.fighter(f).pose.head_scale.unwrap_or(1.0);
    let j = jake_w(w, f);
    if j.head_scale[0] != h || j.head_scale[1] != h || j.head_scale[2] != h {
        j.head_scale = [h, h, h];
    }
    if eat_relevant(w, f) {
        eat_tick(w, f, DELTA_SECONDS * dilation);
    } else {
        jake_w(w, f).pawn.eat_first = true;
    }
}

/// `layerHeld` (CharHooks.layerHeld): BlendListByBool_1 (BlendTime 0 0) on EatOverrideBool updates its active child
/// alone, so while it holds the linked layer (pose 1) gets no update: the BaseMachine layer is held, and restarts when
/// it is relevant again (`layer_frozen`, `relevancy_restart`).
pub fn layer_held(w: &World, f: FighterId) -> bool {
    jake(w, f).anim.eat_override
}

// ---------------------------------------------------------------- the shapes

/// A Float32Array store.
#[allow(clippy::cast_possible_truncation, reason = "a Float32Array store rounds to float32")]
fn f32_of(x: Real) -> f32 {
    x
}

/// `eatSample`: the machine's pose (its current state's clip; Idle's two players by the list's weight) into `out`.
/// False: a clip has no box track (a warning; `out` is left as it was).
// Deviation: the crossfade of a transition is not in the shapes (the state entered stands alone), as the BaseMachine
// layer's crossfades are not (`fighter::anim`).
fn eat_sample(w: &World, f: FighterId, out: &mut [f32]) -> bool {
    let p = &jake(w, f).pawn;
    let ch = w.fighter(f).ch;
    let clip = EAT_CLIPS[usize::try_from(p.eat).expect("an eat state")];
    let mut buf = take_pose(w);
    buf.resize(out.len(), 0.0);
    let t = p.eat_times[usize::try_from(p.eat).expect("an eat state")];
    if !crate::combat::boxes::sample_of(w, ch, w.names.str(clip), t, &mut buf) {
        warn_text(
            w,
            &format!("jake EatOverride: {} has no box track: the shapes keep the clip under it", w.names.str(clip)),
        );
        put_pose(w, buf);
        return false;
    }
    if p.eat == EAT_IDLE && p.walk_w > ZERO_W {
        let mut walk = take_pose(w);
        walk.resize(out.len(), 0.0);
        if !crate::combat::boxes::sample_of(w, ch, w.names.str(EAT_WALK), p.walk_time, &mut walk) {
            warn_text(
                w,
                &format!(
                    "jake EatOverride: {} has no box track: the shapes keep the clip under it",
                    w.names.str(EAT_WALK)
                ),
            );
            put_pose(w, walk);
            put_pose(w, buf);
            return false;
        }
        let wt = p.walk_w;
        for k in 0..buf.len() {
            buf[k] = f32_of(crate::real(buf[k]) + (crate::real(walk[k]) - crate::real(buf[k])) * wt);
        }
        put_pose(w, walk);
    }
    out.copy_from_slice(&buf);
    put_pose(w, buf);
    true
}

/// `isLoco`: a part of the slot mix that is the locomotion clip (`mix_slots`: a slot with no track passes its source
/// on), which is the machine's while it is on the graph.
fn is_loco(w: &World, f: FighterId, anim: Name, time: Real) -> bool {
    let a = &w.fighter(f).anim;
    anim == a.loco_anim && time == a.loco_time
}

/// `poseShapes` (CharHooks.pose): `s` holds the fighter's anim's clip at its anim time (not yet at the actor's scale).
/// While the EatOverride machine is on the graph (EatOverrideBool, the default skin) the locomotion part of the pose is
/// the machine's: with no montage shown the whole pose, under a shown montage each part of the slot mix that is the
/// locomotion clip (the walk ends at StateMachine_1 there: `SlotGraphIn::source`; the linked layer is off the graph).
/// The slot mix is mixed here as `sample_boxes` does it, and Base_AnimBP's pitch (`hit_pitch`) taken while the layer
/// is the pose. Then ModifyBone_13 while StretchOn: the turn by StretchDirection over the whole pose. False: neither
/// lies over the clip (`sample_boxes`' own path).
// Deviation: the boxes bind (data/boxes) bakes the COM bone's Replace (its own rotation put away) into the clips of
// the BaseMachine layer only (as for the hit pitch): a stretch montage's clip keeps its own fml_un_COM rotation under
// the turn.
pub fn pose_shapes(w: &World, f: FighterId, s: &mut [f32]) -> bool {
    let j = jake(w, f);
    let a = &j.anim;
    let machine = a.eat_override && !origins_skin(w, f);
    if !machine && !a.stretch_on {
        return false;
    }
    let fi = w.fighter(f);
    // A montage's pose: the shown one, or the one this tick's pose update took (animKey 'm') that a stop has taken off
    // since.
    let shown = crate::fighter::shown_montage(w, f).is_some() || matches!(fi.anim.anim_key, AnimKey::Montage(_));
    if machine && (!shown || is_loco(w, f, fi.anim.anim, fi.anim.anim_time)) {
        eat_sample(w, f, s);
    }
    let mix = &j.pawn.slot_mix.slot_mix;
    if !mix.is_empty() {
        let ch = fi.ch;
        let mut m = take_pose(w);
        m.resize(s.len(), 0.0);
        for sp in mix {
            if machine && is_loco(w, f, sp.anim, sp.time) {
                if !eat_sample(w, f, &mut m) {
                    continue;
                }
            } else if !crate::combat::boxes::sample_of(w, ch, w.names.str(sp.anim), sp.time, &mut m) {
                continue;
            }
            for k in 0..s.len() {
                s[k] = f32_of(crate::real(s[k]) + (crate::real(m[k]) - crate::real(s[k])) * sp.w);
            }
        }
        put_pose(w, m);
    }
    // ModifyBone_13 (StretchOn) replaces the COM above the hit pitch
    let pitch = if machine || a.stretch_on { None } else { hit_pitch(w, f) };
    if let Some(pitch) = pitch {
        turn_boxes(w, f, s, pitch);
    }
    if a.stretch_on {
        turn_boxes(w, f, s, a.stretch_direction);
    }
    true
}
