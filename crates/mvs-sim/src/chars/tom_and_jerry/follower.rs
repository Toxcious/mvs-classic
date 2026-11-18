//! Jerry as an ally's follower: Jerry_SpawnedActor's follow (the
//! attached place, the crouch, the Delay 0.1), Comp_Follower's broadcasts and BPI_Follower's getters on Jerry's kind
//! entry (`actors::follower` FollowerHooks), RotateMulticast's sim part. `jerryCore` (the parts of Jerry's
//! core the follow calls, set by jerry.rs at its load) is jerry.rs's functions here.
//!
//! The geometry: the relative location is in the ally's root frame, whose yaw SetRotationLogic sets by his facing
//! (local X forward in sim x, Y the depth, Z up), times his scale: in the play plane Jerry stands at the ally's actor
//! location plus Z x scale (sim y); X is 0; Y is the depth (`TomAndJerryFollowState::rel_y`, the view's). As BMO's
//! (`chars/finn/bmo.rs`), the place is taken in the actor pass, behind the fighters' moves, as an attached child moves
//! with its parent.
// Deviation: the four AnimBP inputs the follower's delegates set (OnIsWallSliding / OnJumpNumberChange / OnIsJumping /
// OnIsFastFalling, JerryAnimBlueprint @2368-2583) are read off the ally each tick (`followed_move`), so they hold the
// ally's values from the attach on (the AnimBP keeps its defaults until the first write).
// Unverified: the Delay 0.1 counts Jerry's ticks from the attach's frame (`follower_tick`, called by jerry.rs's step).
// Unverified: AdjustVisibility's IsVisible() reads a mesh Buff_Invisibility left at SetVisibility(IsVisibleLocally)
// ('seers') as shown (the authority's view, the Instigator's own team, sees it).
// Unverified: Jerry's TeamIndex (PickUpJerryMulticast @31782) is taken as his owner's team.
// Not ported (drawing only): UpdateLocality @19803 (the ally's stencil value and outline on JerryMesh, the racket and
// the bottle: the view reads `following`), the relative rotations and RotateMulticast's mesh / widget / socket writes
// @16737-17836, Trail_Effect's SetVisibility(false) in JerryFlyingMulticast @27016 (jerry.rs's), BPI_IncrementCoopActions
// @1713-2109 (a stat).

use super::champagne::set_cork_ammo;
use super::jerry::{self, jerry_mut, jerry_of};
use super::state::{TomAndJerryFollowState, tj_mut, tj_of};
use crate::Real;
use crate::actors::follower::{FollowerMeshVis, FollowerShape};
use crate::actors::world_actor::actor_p;
use crate::actors::{ActorId, ActorRef};
use crate::combat::BuffClass;
use crate::combat::buffs::spawn_buff;
use crate::combat::elem::elem_spawn;
use crate::fighter::anim::update::followed_move;
use crate::fighter::common::{actor_def, char_data, latent_frames, warn_text};
use crate::fighter::moves::is_falling as fighter_falling;
use crate::fighter::{FighterId, actor_y, cap_half, cap_radius, capsule_half, facing};
use crate::math::to_int32;
use crate::name;
use crate::names::Name;
use crate::registry::entries::FollowerHooks;
use crate::registry::props::{num as prop, obj};
use crate::registry::signals::FighterSignal;
use crate::world::World;

/// The owner's record of this Jerry, none for another actor than the one it holds.
fn rec(w: &World, j: ActorId) -> Option<&TomAndJerryFollowState> {
    let a = w.actors.actors.get(j)?;
    let r = &tj_of(w, a.owner)?.follow;
    (r.jerry_id == a.seq).then_some(r)
}

/// [`rec`], to write.
fn rec_mut(w: &mut World, j: ActorId) -> Option<&mut TomAndJerryFollowState> {
    let (owner, seq) = w.actors.actors.get(j).map(|a| (a.owner, a.seq))?;
    let r = &mut tj_mut(w, owner)?.follow;
    (r.jerry_id == seq).then_some(r)
}

/// The ally's root capsule scale (scaled / unscaled half height, as SetFollowingVariables @534 takes it).
fn scale_of(w: &World, f: FighterId) -> Real {
    let u = w.fighter(f).capsule.cap_half_u;
    if u > 0.0 { cap_half(w, f) / u } else { 1.0 }
}

/// SetFollowingVariables (fn @0-810): with a valid FollowingFighter, Follow_Z_Offset = -(the root capsule's scaled
/// half height, crouch-aware - 80) @228, Follow_Y_Offset = its scaled radius @356, CrouchHeight = -(scale x the CMC's
/// CrouchedHalfHeight - 40) @783.
fn set_following_variables(w: &mut World, j: ActorId, f: FighterId) {
    let z = -(capsule_half(w, f) - 80.0);
    let y = cap_radius(w, f);
    // The CMC value is unscaled: the base definition's, not the size-scaled copy's.
    let crouched = char_data(w, w.fighter(f).base_ch).def.crouched_half;
    let crouch = -((scale_of(w, f) * crate::math::narrow(crouched)) - 40.0);
    if let Some(r) = rec_mut(w, j) {
        r.follow_z = z;
        r.follow_y = y;
        r.crouch_height = crouch;
    }
}

/// The attached place: the ally's actor location plus the relative Z times his scale (X 0; Y the depth).
fn place(w: &mut World, j: ActorId) {
    let Some(f) = jerry_of(w, j).and_then(|s| s.following) else { return };
    let Some(rel_z) = rec(w, j).map(|r| r.rel_z) else { return };
    let (x, y) = (w.fighter(f).body.x, (actor_y(w, f) + (rel_z * scale_of(w, f))));
    if let Some(s) = jerry_mut(w, j) {
        s.x = x;
        s.y = y;
    }
}

/// `rotate` (Rotate -> RotateServer -> RotateMulticast @16718): FacingLeft, the actor's rotation (the rest is drawing:
/// the widget's place while following @16737-16814, JerryMesh's yaw, the AnimBP's FacingLeft, the racket and the
/// bottle on the other wrist @16866-17836). Called by jerry.rs, champagne.rs (champagneJerry.rotate) and
/// tennis.rs.
pub fn rotate(w: &mut World, j: ActorId, facing_left: bool) {
    if let Some(s) = jerry_mut(w, j) {
        s.facing = if facing_left { -1.0 } else { 1.0 };
    }
}

/// The relative location of a facing: (0, FacingLeft ? -Y : Y, `z`).
fn set_relative(w: &mut World, j: ActorId, left: bool, crouch_aware: bool) {
    let Some(r) = rec_mut(w, j) else { return };
    r.rel_y = if left { -r.follow_y } else { r.follow_y };
    r.rel_z = if crouch_aware && r.is_crouching { r.crouch_height } else { r.follow_z };
}

/// UpdateFacing(FacingLeft) @17898 (FacingChanged, and the Delay's @900): the relative location (0, FacingLeft ? -Y :
/// Y, IsCrouching ? CrouchHeight : Z) @18101, the relative yaw (-90 / 90) @18194, RotateServer(FacingLeft) @18224.
fn update_facing(w: &mut World, j: ActorId, left: bool) {
    set_relative(w, j, left, true);
    place(w, j);
    rotate(w, j, left);
}

/// CrouchChange(IsCrouching) @18248: the relative location with Jerry's own FacingLeft @18451.
fn crouch_change(w: &mut World, j: ActorId) {
    let left = jerry_of(w, j).is_some_and(|s| s.facing < 0.0);
    set_relative(w, j, left, true);
    place(w, j);
}

/// `attachToAlly`: PickUpJerryMulticast(OtherActor) @31595 and the DoOnce @1469-1504 into the follow @1515-3768
/// (the ally overlap @16530-16665 -> @8309, authority). Called by jerry.rs (its overlap and attachTo for a fighter
/// other than Tom).
/// - @31595-31864: a BaseFighter, not IsMinion, of Jerry's TeamIndex (Unverified above).
/// - The DoOnce (Temp_bool_IsClosed_Variable_1, never reset): a Jerry follows once (`following` stays set to his end).
/// - @1515-1590: FollowingFighter (BP_FighterCharacter: every player); @1609-1620 the movement off; @1690 the cork
///   ammo (CorkAllyReloadAmmo); @2146 the attach (SnapToTarget location, KeepWorld rotation and scale); @2209
///   JerryFlyingMulticast; @2223 IsFollowing; @2234 SetFollowingVariables; @2298-2623 the place and the relative
///   location (0, IsFacingLeft ? -Y : Y, Z: not the crouch height), RotateServer @2746; @2769-2972 the hit box off and
///   the proof buffs on himself (SpawnBuffActor(Iceproof / Fireproof, self, Instigator, 0 stacks, 0 s)); @3142
///   UpdateFollower and the four bindings; @3543 SpawnBuffActor(Buff_Cheese_C, the ally, GetOwner() = Tom, 0 stacks,
///   Buff Duration) and @3680 (Buff_SpeedUp_C, ..., SpeedBuffDuration) through Jerry's own Comp_State_Manager (so no
///   fighter broadcasts NewBuffSpawned / BuffStacksAdded); @3768 Delay(0.1).
pub fn attach_to_ally(w: &mut World, j: ActorId, ally: FighterId) {
    let Some(a) = w.actors.actors.get(j) else { return };
    let (owner, seq, dead) = (a.owner, a.seq, a.dead);
    let Some(s) = jerry_of(w, j) else { return };
    if dead || w.fighter(ally).life.is_minion || w.fighter(ally).team.team != w.fighter(owner).team.team {
        return;
    }
    if s.following.is_some() {
        return; // the DoOnce
    }
    let Some(t) = tj_mut(w, owner) else {
        warn_text(w, "Jerry_SpawnedActor: the follow needs its owner to be a Tom and Jerry");
        return;
    };
    // A new actor's follow variables are at the class defaults: the record is started over for this one.
    t.follow = TomAndJerryFollowState { jerry_id: seq, ..TomAndJerryFollowState::default() };
    if let Some(s) = jerry_mut(w, j) {
        s.following = Some(ally); // FollowingFighter @1590
    }
    jerry::stop_movement(w, j);
    let p = actor_p(w, ActorRef::Actor(j));
    let (ammo, cheese_secs, speed_secs) = (
        prop(p, "CorkAllyReloadAmmo").unwrap_or(0.0),
        prop(p, "Buff Duration").unwrap_or(0.0),
        prop(p, "SpeedBuffDuration").unwrap_or(0.0),
    );
    set_cork_ammo(w, j, to_int32(ammo)); // @1690
    jerry::flying_end(w, j); // @2209
    set_following_variables(w, j, ally); // @2234
    let left = facing(w, ally) < 0.0; // BPI_IsFacingLeft @2429
    set_relative(w, j, left, false);
    place(w, j); // @2298-2623
    rotate(w, j, left); // RotateServer @2746
    elem_spawn(w, ActorRef::Actor(j), BuffClass::BuffElementalIceproof, 0); // @2800
    elem_spawn(w, ActorRef::Actor(j), BuffClass::BuffElementalFireproof, 0); // @2896
    jerry::hitbox_off(w, j); // @2769, UpdateHitbox @2972
    // @2986-3132: FollowingFighter valid and JerryMesh's anim instance a JerryAnimBlueprint (the kind's mesh): the
    // AnimBP's Fighter (UpdateFollower @3142) is `following`; then the four bindings.
    if let Some(r) = rec_mut(w, j) {
        r.bound = true;
    }
    copy_inputs(w, j, ally);
    spawn_buff(w, ally, name!("Buff_Cheese_C"), cheese_secs, Some(0), Some(owner), true, None); // @3543
    spawn_buff(w, ally, name!("Buff_SpeedUp_C"), speed_secs, Some(0), Some(owner), true, None); // @3680
    if let Some(r) = rec_mut(w, j) {
        r.delay = to_int32(latent_frames(0.1)); // @3768
    }
}

/// JerryAnimBlueprint's follower inputs: BlueprintUpdateAnimation @646-1476 (IsFalling = the Fighter's CMC
/// IsFalling(); IsCrouching / IsMovingOnGround / the speeds are the view's) and the four delegates (Deviation above).
fn copy_inputs(w: &mut World, j: ActorId, f: FighterId) {
    let falling = fighter_falling(w, f); // @735
    let m = followed_move(w, f);
    if let Some(s) = jerry_mut(w, j) {
        s.falling = falling;
        s.wall_sliding = m.wall_sliding;
        s.jump_number = m.jump_number;
        s.jumping = m.jumping;
        s.fast_fall = m.fast_falling;
    }
}

/// `followerTick`: jerry.rs's step while `following` (its ReceiveTick @18482 runs up to @19077, then this):
/// ReceiveTick's tail @19077-19377 (the ally's CMC IsCrouching against IsCrouching: on a change IsCrouching = it,
/// CrouchChange @8363), the attached place (behind the ally's move of the frame), the AnimBP's inputs (its update
/// after the tick), then the actor's latent action: the Delay 0.1 @3768 -> @722 (SetFollowingVariables, then
/// UpdateFacing(the ally's IsFacingLeft) @900).
pub fn follower_tick(w: &mut World, j: ActorId) {
    let Some(f) = jerry_of(w, j).and_then(|s| s.following) else { return };
    let Some(r) = rec(w, j) else { return };
    if w.actors.actors.get(j).is_none_or(|a| a.dead) {
        return;
    }
    let crouched = w.fighter(f).capsule.cmc_crouched;
    if r.is_crouching != crouched {
        if let Some(r) = rec_mut(w, j) {
            r.is_crouching = crouched;
        }
        crouch_change(w, j);
    }
    place(w, j);
    copy_inputs(w, j, f);
    let fire = rec_mut(w, j).is_some_and(|r| {
        if r.delay > 0 {
            r.delay -= 1;
            r.delay == 0
        } else {
            false
        }
    });
    if fire {
        set_following_variables(w, j, f);
        update_facing(w, j, facing(w, f) < 0.0);
    }
}

/// `followerSignal`: the four bindings (@3187-3453) on the ally `f` (the kind's `fighterSignal`): the actor's own
/// record says it bound them. Entered by jerry.rs.
#[allow(clippy::too_many_arguments, reason = "the hook's parameters")]
pub fn follower_signal(
    w: &mut World,
    j: ActorId,
    f: FighterId,
    sig: FighterSignal,
    flag: bool,
    num: Real,
    _name: Name,
    _other: Option<FighterId>,
) {
    let dead = w.actors.actors.get(j).is_none_or(|a| a.dead);
    if dead || jerry_of(w, j).and_then(|s| s.following) != Some(f) {
        return;
    }
    if !rec(w, j).is_some_and(|r| r.bound) {
        return;
    }
    match sig {
        FighterSignal::Facing => update_facing(w, j, flag), // UpdateFacing @17898
        FighterSignal::AdjustedVisibility => adjust_visibility(w, j, num),
        FighterSignal::ExitedArena => {
            // DieWithFollower @29119: WasRungOut = true, K2_DestroyActor @29130.
            jerry::set_rung_out(w, j);
            jerry::destroy(w, j);
        }
        FighterSignal::TakeAnyDamage => {
            // FollowerDamage(Damage) @28338: JerryHealthHP -= Damage; at 0 or less K2_DestroyActor @21948 (no
            // DeathVisuals), else the hurt tail @21988.
            if jerry::damage(w, j, num) <= 0.0 {
                jerry::destroy(w, j);
            } else {
                jerry::hurt(w, j);
            }
        }
        _ => {}
    }
}

/// AdjustVisibility(InvisibilityCount) @29145: 0 or less and JerryMesh hidden -> SetVisibility(true) @29254; above 0
/// and shown -> SetVisibility(false) @29345 (Unverified above on 'seers').
fn adjust_visibility(w: &mut World, j: ActorId, count: Real) {
    let Some(s) = jerry_mut(w, j) else { return };
    if count <= 0.0 {
        if s.mesh_vis.is_none() {
            s.mesh_vis = Some(FollowerMeshVis::All);
        }
    } else if s.mesh_vis.is_some() {
        s.mesh_vis = None;
    }
}

fn id_of(a: ActorRef) -> Option<ActorId> {
    match a {
        ActorRef::Actor(id) => Some(id),
        _ => None,
    }
}

fn mesh(w: &mut World, a: ActorRef, v: FollowerMeshVis) {
    if let Some(s) = id_of(a).and_then(|id| jerry_mut(w, id)) {
        s.mesh_vis = Some(v);
    }
}

fn is_attacking(w: &World, a: ActorRef) -> bool {
    id_of(a).and_then(|id| jerry_of(w, id)).is_some_and(|s| s.is_attacking)
}

fn is_flying(w: &World, a: ActorRef) -> bool {
    id_of(a).and_then(|id| jerry_of(w, id)).is_some_and(|s| s.flying)
}

fn is_falling(w: &World, a: ActorRef) -> bool {
    id_of(a).and_then(|id| jerry_of(w, id)).is_some_and(|s| s.falling)
}

/// The overlap shapes: the root Capsule (ECC_Pawn, query on; its data's half height and radius, 80 / 45) at the
/// actor's place; ProjectileHurtBox is ProjectileDestructible, the rest HitBox or custom: none of the zones' Pawn /
/// HurtBox types.
fn shapes(w: &World, a: ActorRef, out: &mut Vec<FollowerShape>) {
    let Some(id) = id_of(a) else { return };
    let (Some(act), Some(s)) = (w.actors.actors.get(id), jerry_of(w, id)) else { return };
    let cap = actor_def(w, ActorRef::Fighter(act.owner), act.cls).and_then(|d| obj(&d.components, "Capsule"));
    let (Some(r), Some(h)) =
        (cap.and_then(|c| prop(c, "CapsuleRadius")), cap.and_then(|c| prop(c, "CapsuleHalfHeight")))
    else {
        warn_text(w, &format!("{}: no Capsule size in its data (BPI_Follower shapes)", w.names.str(act.cls)));
        return;
    };
    let g = act.size_scale.unwrap_or(1.0); // the root's scale, as the sweep's capsule
    out.push(FollowerShape { x: s.x, y: s.y, r: r * g, half: h * g });
}

/// `jerryFollower`: BPI_Follower's getters and Comp_Follower on Jerry's kind entry (jerry.rs).
pub const JERRY_FOLLOWER: FollowerHooks =
    FollowerHooks { comp: true, mesh, is_attacking, is_flying, is_falling, shapes };
