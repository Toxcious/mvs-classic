//! Gizmo's ally attach: Comp_C023A_AllyAttach (Attach to Actor, the meter,
//! the carry turn, the ally's binds), the notifies DashToAlly / StopMoveToAlly / MoveToActorHit / ApplyBuffToAttached,
//! the buffs C023A_AttachCooldown / C023A_ShareDamageBuff / Buff_C023A_AttachShrink and the four conditions; joined to
//! boundSignal, componentTick, worldTick and timerTick. Its variables are `GizmoState::attach` (state.rs).
//!
//! The dash to the ally is a Buff_MoveToActor2 (the shared helpers' port):
//! `beginMoveToActor2`, `bindReachedActor`, `moveToActor2Of` and `moveToActor2Reached('C023A_ActorReachedLogic')`
//! are called from this file's bodies. The listener maps `onAfterAttached` / `onAfterDetached` (perks.rs's
//! Backpack the one listener) are direct calls into perks.rs ([`super::perks::backpack_attached`] /
//! [`super::perks::backpack_detached`]); `setAllyAttachComp` is pawn.rs's `ally_attach` hook calling [`ally_attach`].
//!
//! Sim space: x = -UE Y, y = UE Z. The mesh's local X is the stage depth under its yaw, so AttachOffset X (-2 x
//! radius) and CarryTurn's lerp of it are depth alone: `rel_x` is the view's; the sim places the owner on the
//! carrier's Root socket plus the offset's Z ([`follow_place`]).

use super::car::{car_of, is_car, new_driver, set_overheat_state};
use super::pawn::set_com_angle;
use super::perks::{backpack_attached, backpack_detached};
use super::{gizmo_mut, gizmo_of, gizmo_w};
use crate::Real;
use crate::actors::{ActorRef, ProjectileId};
use crate::combat::buff_class::BuffClass;
use crate::combat::buffs::{buff, buff_mut, buff_of, end_buff, inst_def_of, remove_notify_buffs, spawn_buff};
use crate::combat::hit::{apply_direct_damage, def_of, remove_hitstun_from_knock_back, stun_changed};
use crate::combat::holds::{GrappleRoot, grapple_to_root, raw_half, remove_root_grapple};
use crate::combat::{BuffRef, boxes::obj};
use crate::consts::DELTA_SECONDS;
use crate::events::{Event, NotifyRef, PopKind, UiStateOp, UiStateParams};
use crate::fighter::anim::cond;
use crate::fighter::anim::montages::start_montage;
use crate::fighter::common::{buff_def, data_of, peers};
use crate::fighter::control::data_name;
use crate::fighter::match_side::notify_props;

use crate::fighter::moves::{
    attach_parent_of, clear_gravity_override, disable_movement, net_impulse, override_gravity, recall_move,
};
use crate::fighter::tick::in_section;
use crate::fighter::traces::{TRACE_PROJECTILE_GROUND_NO_PLATFORM, capsule_trace_single, sphere_overlap_actors};
use crate::fighter::{FighterId, actor_y, cap_half, cap_radius, facing, in_hitstun, set_facing};
use crate::math::ue::{SMALL_NUMBER, deg_atan2, dist2d, safe_normal, size2d};
use crate::name;
use crate::names::Name;
use crate::registry::char_hooks::AllyAttach;
use crate::registry::entries::BuffHooks;
use crate::registry::props::{flag, num};
use crate::registry::signals::FighterSignal;
use crate::registry::{BuffEntry, CharHooks, Condition, NotifyHandler, Registry, RegistryError};
use crate::world::World;
use mvs_anim::montage::PROXY_NONE;
use mvs_data::value::Value;

/// `ATTACH_COOLDOWN`.
pub const ATTACH_COOLDOWN: &str = "C023A_AttachCooldown_C";
/// `SHARE_DAMAGE`.
pub const SHARE_DAMAGE: &str = "C023A_ShareDamageBuff_C";
/// `ATTACH_SHRINK`.
pub const ATTACH_SHRINK: &str = "Buff_C023A_AttachShrink_C";
const REACHED: &str = "C023A_ActorReachedLogic";

// Default__Comp_C023A_AllyAttach_C (the pawn's component template sets none of them).
const DASH_SPEED: Real = 5000.0; // DashToSpeedPerSecond
const COOLDOWN_TIME: Real = 9.0; // AttachCooldown
const KNOCK_OFF_DAMAGE: Real = 7.0; // KnockOffDamage
const ATTACH_TIME: Real = 15.0; // AttachTime
const TIMER_RATE: Real = 0.5; // AttachTimerItervals
const BLAST_BOX_BUFFER: Real = 300.0; // CheckBlastBoxBuffer @0
const TURN_LENGTH: Real = 0.2; // Pawn_C023A CarryTurnTimeline
const MOVE_GROUP: &str = "StateGroups::NewEnumerator13"; // StateGroups byte 12 (@624): the movement group

/// The hooks this file joins in Gizmo's CharHooks that no other file holds (`bound_signal`, `component_tick`,
/// `world_tick`); `timerTick` is composed in mod.rs.
pub fn hooks(h: &mut CharHooks) {
    h.bound_signal = Some(bound_signal);
    h.component_tick = Some(component_tick);
    h.world_tick = Some(world_tick);
}

/// `live(b)`: the buff is running.
fn live(w: &World, b: Option<BuffRef>) -> Option<BuffRef> {
    b.filter(|&b| buff(w, b).is_some_and(|x| x.frames > 0.0))
}

/// PlayAnimMontage(montage, 1.0) on the owner (PlayMontageMulticast @2512).
// Deviation: through the montage player's start, as Bugs's tunnel punch: the attack-begin latch is not the montage's.
fn play_montage(w: &mut World, f: FighterId, montage: Name) {
    start_montage(w, f, montage, &PROXY_NONE, None);
    w.fighter_mut(f).charge.begin_attack = false;
}

/// The Mesh's size scale of the template (`baseCh.meshScale || 1`; Mesh.RelativeScale3D @3081).
fn mesh_rel(w: &World, c: FighterId) -> Real {
    let s = def_of(w, w.fighter(c).base_ch).mesh_scale;
    if s == 0.0 { 1.0 } else { crate::math::narrow(s) }
}

/// The owner's actor location while attached: RelativeLocation (AttachOffset / AttachOffsetScale, Y 0) under the
/// mesh's world scale (RelativeScale3D x the actor scale), so AttachOffset x the carrier's size scale; its X is depth.
// Unverified: the socket (Root) is taken at the mesh root's place; the root bone's own animation is not followed.
fn follow_place(w: &World, a_off_z: Real, c: FighterId) -> (Real, Real) {
    let fi = w.fighter(c);
    let mesh_z = crate::combat::hits::char_data(w, fi).boxes.mesh_z;
    (
        crate::fighter::mesh_wx(w, c),
        ((crate::fighter::mesh_wy(w, c) + crate::math::narrow(mesh_z)) + (a_off_z * fi.body.size_scale)),
    )
}

// ---- Attach to Actor ----
fn attach_to(w: &mut World, f: FighterId, actor: FighterId) {
    if in_hitstun(w, f) {
        return; // BPI_HitstunActive @113 -> @5562
    }
    {
        let a = &mut gizmo_w(w, f).attach;
        a.attached = true; // @2399
        a.collision_off = true; // SetActorEnableCollision(false) @2430 (the shrink buff's actor_collision_off reads it)
    }
    // DisableMovement @2534 (MOVE_None: IsFalling false, so AnimNotify_OffGroundStopAnim leaves the attached montages
    // alone), OverrideGravity(0) @2570.
    disable_movement(w, f);
    override_gravity(w, f);
    w.fighter_mut(f).controls.block_taunt_anims = true; // BPI_SetBlockTaunt(true) @2706
    // BPI_GetRootCapsule.SetGenerateOverlapEvents(false) @2889: Deviation: not modelled (the owner's collision is off).
    gizmo_w(w, f).attach.carrying = Some(actor); // @3152
    let fa = facing(w, actor);
    set_facing(w, f, fa); // BPI_SetFacingLogic(BPI_IsFacingLeft) @3395
    // BPI_GetOriginalCapsuleHalfHeightAndRadius @3520. Unverified: taken as the pawn's template (baseCh), unscaled.
    let d = def_of(w, w.fighter(actor).base_ch);
    let (off_x, off_z) = (crate::math::narrow(d.cap_radius * -2.0), crate::math::narrow(d.cap_half * 2.0)); // @3575, @3617
    let rel = mesh_rel(w, actor);
    {
        let a = &mut gizmo_w(w, f).attach;
        a.off_x = off_x;
        a.off_z = off_z;
        a.rel_x = off_x / rel; // K2_SetActorRelativeLocation((X, 0, Z) / AttachOffsetScale) @3900
        a.rel_z = off_z / rel;
    }
    let (px, py) = follow_place(w, off_z, actor);
    grapple_to_root(w, f, GrappleRoot { x: px, y: py }); // InitiateGrapple(Mesh, Root) @3001 and the relative location @3900
    // BPI_SetAttachedToAlly(true) @4218: C023A_AnimBP reads GizmoAttach.attached.
    clense_attach_ally(w, f, actor); // @4256
    gizmo_w(w, f).attach.input_bound = true; // the owner's InputTriggered += InputeTriggered @4302
    // the ally's manager, Instigator the owner @4414, @4498
    let share = spawn_buff(w, actor, name!("C023A_ShareDamageBuff_C"), 0.0, Some(0), Some(f), true, Some(actor));
    gizmo_w(w, f).attach.share_buff = share;
    let shrink = spawn_buff(w, f, name!("Buff_C023A_AttachShrink_C"), 0.0, Some(0), Some(f), true, Some(f)); // @4588, @4672
    gizmo_w(w, f).attach.shrink_buff = shrink;
    // @4691: CarryingActor is a BP_FighterCharacter (every fighter of the port): its NewBuffSpawned @4864, FacingChanged
    // @4979 and OnExitedArena @5043 bound (ally_bound: bound_signal below).
    let tick = w.flow.tick;
    {
        let a = &mut gizmo_w(w, f).attach;
        a.ally_bound = true;
        // UpdateLocality(CarryingActor, false) @5107: the outline stencil copy, the view's.
        a.meter = 1.0; // TrackedMeter @5131
        a.timer_set = tick; // K2_SetTimer(AttachTimerTick, 0.5, looping) @5154
        a.timer_next = TIMER_RATE;
    }
    // Toggle_UI_Bar(true) @5247: the HUD shows the bar while `attached` (UpdateRunChargeProgressBar(1)).
    backpack_attached(w, f); // broadcast AfterAttachedToAlly @5262
}

fn detach(w: &mut World, f: FighterId) {
    if gizmo_w(w, f).attach.attached {
        let (x, y) = (w.fighter(f).body.x, actor_y(w, f));
        let (px, py) = check_blast_box_buffer(w, f, x, y); // CheckBlastBoxBuffer(K2_GetActorLocation) @262
        recall_move(w, f, px, py); // K2_SetActorLocation(GoodLocation) @314
    }
    gizmo_w(w, f).attach.attached = false; // @376
    remove_root_grapple(w, f); // RemoveGrapple @387, SetMovementMode(Falling) @570
    clear_gravity_override(w, f); // @610
    // K2_SetActorRotation(rot(0,0,0)) @443: Deviation: not modelled (the facing is kept).
    w.fighter_mut(f).controls.block_taunt_anims = false; // @741
    gizmo_w(w, f).attach.collision_off = false; // SetActorEnableCollision(true) @799
    // SetGenerateOverlapEvents(true) @977; BPI_SetAttachedToAlly(false) @1276 (the AnimBP reads `attached`).
    let c = gizmo_w(w, f).attach.carrying;
    // SpawnBuffActor(C023A_AttachCooldown, Owner CarryingActor, Instigator the owner, 0, AttachCooldown) on the owner's
    // manager @1499. A null CarryingActor gives the spawn no owner: nothing.
    if let Some(c) = c {
        spawn_buff(w, c, name!("C023A_AttachCooldown_C"), COOLDOWN_TIME, Some(0), Some(f), true, Some(f));
    }
    {
        let a = &mut gizmo_w(w, f).attach;
        a.ally_bound = false; // the ally's FacingChanged @1689, OnExitedArena @1753, NewBuffSpawned @1962 unbound
        a.carrying = None; // CarryingActor = null (Attach to Actor @2003, every detach)
    }
    attach_turn(w, f, 0.0, 0.0, true); // BPI_AttachTurn((0,0,0), true) @2113: CarryTurnTimeline.Stop()
    let (share, shrink) = {
        let a = &gizmo_w(w, f).attach;
        (a.share_buff, a.shrink_buff)
    };
    if let (Some(c), Some(b)) = (c, live(w, share)) {
        end_buff(w, c, b); // DamageShareBuff.K2_DestroyActor @2164
    }
    if let Some(b) = live(w, shrink) {
        end_buff(w, f, b); // ShrinkBuff.K2_DestroyActor @2200
    }
    {
        let a = &mut gizmo_w(w, f).attach;
        a.share_buff = None;
        a.shrink_buff = None;
        a.input_bound = false; // InputTriggered -= InputeTriggered @2259
        // UpdateLocality(OwningFighter, false) @2300 (the view's).
        a.timer_set = -1; // K2_ClearAndInvalidateTimerHandle(AttachTimer) @2324
    }
    // Toggle_UI_Bar(false) @2344 (the HUD hides the bar with `attached`).
    backpack_detached(w, f); // broadcast AfterDetachedFromAlly @2359
}

/// FMath::Clamp.
fn fclamp(v: Real, lo: Real, hi: Real) -> Real {
    if v < lo {
        lo
    } else if v < hi {
        v
    } else {
        hi
    }
}

/// CheckBlastBoxBuffer(ActorLocation) -> GoodLocation: CapsuleTraceSingle from the carrier's location to the place
/// (radius: the owner's scaled radius, half height BPI_GetCapsuleHalfHeight, ProjectileGroundNoPlatform, ignoring the
/// owner), the hit's Location when it blocks, then clamped inside the blast box less 300 on each side.
fn check_blast_box_buffer(w: &mut World, f: FighterId, x: Real, y: Real) -> (Real, Real) {
    let (cx, cy) = match gizmo_w(w, f).attach.carrying {
        Some(c) => (w.fighter(c).body.x, actor_y(w, c)), // CarryingActor.K2_GetActorLocation @148
        None => (0.0, 0.0),
    };
    let me = Some(ActorRef::Fighter(f));
    let (r, half) = (cap_radius(w, f), raw_half(w, f));
    let hit = capsule_trace_single(w, cx, cy, x, y, r, half, TRACE_PROJECTILE_GROUND_NO_PLATFORM, &[me], me);
    let (px, py) = match hit {
        Some(h) if h.blocking => (h.x, h.y),
        _ => (x, y), // SelectVector @1346
    };
    let k = crate::world::blast::blast_corners(w);
    // BPI_GetBottomRightCorner / BPI_GetTopLeftCorner (UE Y, Z): Z in [BR.Z + 300, TL.Z - 300] @1447, Y in
    // [BR.Y + 300, TL.Y - 300] @1502.
    let z = fclamp(py, k.bottom_right[1] + BLAST_BOX_BUFFER, k.top_left[1] - BLAST_BOX_BUFFER);
    let ue_y = fclamp(-px, k.bottom_right[0] + BLAST_BOX_BUFFER, k.top_left[0] - BLAST_BOX_BUFFER);
    ((-ue_y), z)
}

/// Pawn_C023A BPI_AttachTurn(AttachOffset, StopTurn) @3820: StopTurn -> CarryTurnTimeline.Stop(); else Attach Offset =
/// the pin and PlayFromStart (its update @3871: RelativeLocation = VLerp(Offset x (-1, 1, 1), Offset, TurnAlpha)).
fn attach_turn(w: &mut World, f: FighterId, x: Real, z: Real, stop: bool) {
    let a = &mut gizmo_w(w, f).attach;
    if stop {
        a.turn_on = false;
        return;
    }
    a.turn_x = x;
    a.turn_z = z;
    a.turn_pos = 0.0;
    a.turn_on = true;
    turn_update(w, f);
}

fn turn_update(w: &mut World, f: FighterId) {
    let a = &mut gizmo_w(w, f).attach;
    let u = a.turn_pos / TURN_LENGTH;
    let alpha = u * u * (3.0 - 2.0 * u); // TurnAlpha (0 -> 1 over 0.2 s)
    a.rel_x = -a.turn_x + 2.0 * a.turn_x * alpha;
    a.rel_z = a.turn_z;
}

/// ClenseAttachAlly(Actor) @0-2096: every debuff buff actor among the ally's Comp_Actor_Parent children destroyed; a
/// running hitstun removed (RemoveAllHitStunSources, RemoveHitstunFromKnockBack, NetImpulse (0, 0, 1)); either one pops
/// "Cleanse!" on the owner (BPI_PopText @1971).
fn clense_attach_ally(w: &mut World, f: FighterId, c: FighterId) {
    let mut cleansed = false;
    let list: Vec<u32> = w.fighter(c).buffs.buffs.iter().map(|b| b.serial).collect();
    for serial in list {
        let r = BuffRef { fighter: c, serial };
        let Some(b) = buff(w, r) else { continue };
        if b.frames <= 0.0 {
            continue;
        }
        let cls = b.cls;
        let debuff = match inst_def_of(w, r) {
            Some(d) => flag(&d, "IsDebuff"),
            None => buff_def(w, ActorRef::Fighter(c), cls).is_some_and(|d| flag(d, "IsDebuff")),
        };
        if !debuff {
            continue;
        }
        end_buff(w, c, r); // K2_DestroyActor @588
        cleansed = true;
    }
    if in_hitstun(w, c) {
        // RemoveAllHitStunSources @891.
        // Unverified: the sources taken as the knockback's and the buff stun's flags.
        w.fighter_mut(c).buffs.buff_stunned = false;
        if w.fighter(c).attackers.stun_source {
            w.fighter_mut(c).attackers.stun_source = false;
            let pulled = w.fighter(c).moves.pull.is_some();
            stun_changed(w, c, pulled);
        }
        remove_hitstun_from_knock_back(w, c); // @978
        net_impulse(w, c, 0.0, 1.0, true, true, 0.0, false, 0.0); // @1273
        cleansed = true;
    }
    if cleansed {
        let id = w.fighter(f).id;
        w.fighter_mut(f).events.push(Event::PopText {
            fighter: id,
            kind: PopKind::GizmoAttach,
            stacks: 0.0,
            gain: None,
            params: None,
        });
    }
}

/// UpdateMeter(Amount, PlayShake) @2999: TrackedMeter -= Amount; at or under 0 KickAttached and the exit montage.
fn update_meter(w: &mut World, f: FighterId, amount: Real) {
    let a = &mut gizmo_w(w, f).attach;
    if !a.attached {
        return;
    }
    a.meter -= amount;
    // UpdateRunChargeProgressBar(TrackedMeter) @3171, ShakeEvent @3253: the HUD draws the meter.
    if a.meter <= 0.0 {
        detach(w, f); // KickAttached @3433 -> Attach to Actor(null, false) @3471
        play_montage(w, f, name!("C023A_Lantern_Exit_Montage")); // @3447
    }
}

// ---- AtemptToReachAlly / ActorReachedLogic / StopMoveToAlly ----

/// A dash target's place: a fighter's actor location, a car's.
fn target_place(w: &World, t: ActorRef) -> (Real, Real) {
    match t {
        ActorRef::Fighter(g) => (w.fighter(g).body.x, actor_y(w, g)),
        ActorRef::Projectile(p) => w.actors.projectiles.get(p).map_or((0.0, 0.0), |p| (p.x, p.y)),
        _ => (0.0, 0.0),
    }
}

fn atempt_to_reach_ally(w: &mut World, f: FighterId, t: ActorRef) {
    let (tx, ty) = target_place(w, t);
    let fx = w.fighter(f).body.x;
    set_facing(w, f, if -tx >= -fx { -1.0 } else { 1.0 }); // BPI_SetFacingLogic(Actor.Y >= Owner.Y) @349 (true: facing left)
    let b = spawn_buff(w, f, name!("Buff_MoveToActor2_C"), 0.0, Some(0), Some(f), true, Some(f)); // @466
    gizmo_w(w, f).attach.move_buff = b; // @550
    let Some(b) = b else { return };
    if crate::combat::move_to_actor_2::move_to_actor_2_of(w, b).is_none() {
        return; // the cast @569
    }
    let dx = tx - fx;
    let dy = ty - actor_y(w, f);
    let len = size2d([dx, dy]);
    let u = safe_normal([0.0, dx, dy], SMALL_NUMBER); // GetDirectionUnitVector @768
    let (ux, uy) = (u[1], u[2]);
    // Actor's CapsuleComponent scaled half height @892. Unverified: a car is taken as having none (0).
    let hh = match t {
        ActorRef::Fighter(g) => cap_half(w, g),
        _ => 0.0,
    };
    crate::combat::move_to_actor_2::begin_move_to_actor_2(
        w,
        f,
        b,
        Some(t),
        false,
        false,
        len / DASH_SPEED,
        [0.0, hh],
        [(ux * 2500.0), (uy * 2500.0)],
    ); // @1386
    crate::combat::move_to_actor_2::bind_reached_actor(w, b, REACHED); // @1460
    set_com_angle(w, f, deg_atan2(-ux, uy), true, 0.0, 1.0); // BPI_Set_COM_Angle(DegAtan2(dir.Y, dir.Z), true, 0, 1) @2048
}

/// `moveToActor2Reached(REACHED, ...)`.
fn actor_reached(w: &mut World, f: FighterId, _b: BuffRef, dest: Option<ActorRef>) {
    if gizmo_of(w, f).is_none() {
        return;
    }
    match dest {
        Some(ActorRef::Fighter(d)) if peers(w).contains(&d) => {
            // A Pawn_C023A in the AllyDash section of C023A_DashGrab_Montage is not attached to (@197-558).
            if gizmo_of(w, d).is_some() && in_section(w, d, name!("C023A_DashGrab_Montage"), name!("AllyDash")) {
                return;
            }
            play_montage(w, f, name!("C023A_Lantern_Idle_Montage")); // @1279
            attach_to(w, f, d); // AttachToActorMulticast(ReachedActor, true) @1302
            set_com_angle(w, f, 0.0, false, 0.0, 0.0); // @1592
        }
        Some(a @ ActorRef::Projectile(car)) if is_car(w, a) => {
            let (cx, cy) = target_place(w, a);
            recall_move(w, f, cx, cy); // K2_SetActorLocation((0, car.Y, car.Z)) @805
            set_com_angle(w, f, 0.0, false, 0.0, 0.0); // @1133
            if !car_driven(w, car) {
                new_driver(w, car, f); // @1702
                play_montage(w, f, name!("C023A_Car_Overheat_Montage")); // @1747
            }
            set_overheat_state(w, car); // @1222
        }
        _ => {} // @644: neither a fighter nor the car
    }
}

/// The car's IsDriven.
fn car_driven(w: &World, car: ProjectileId) -> bool {
    w.actors.projectiles.get(car).and_then(|p| car_of(&p.ext)).is_some_and(|c| c.driven)
}

/// The car's HasOverheat.
fn car_overheated(w: &World, car: ProjectileId) -> bool {
    w.actors.projectiles.get(car).and_then(|p| car_of(&p.ext)).is_some_and(|c| c.has_overheat)
}

/// StopMoveToAlly @0-283: a live MoveToAllyBuff: ReachedActor unbound and the buff destroyed (the pointer kept).
fn stop_move_to_ally(w: &mut World, f: FighterId) {
    let mb = gizmo_w(w, f).attach.move_buff;
    let Some(b) = live(w, mb) else { return };
    let Some(e) = crate::combat::move_to_actor_2::move_to_actor_2_mut(w, b) else { return };
    e.reached = None; // ReachedActor -= ActorReachedLogic @206
    end_buff(w, f, b); // K2_DestroyActor @247
}

// ---- Pawn_C023A BPI_FindDashTarget ----

/// `findDashTarget`: BPI_FindDashTarget @1976-2029: the closest ally in reach (a fighter) or a team mate's car (a
/// projectile, `ActorRef::Projectile`); writes `last_dash_target` / `attach.dash_car`. Called by
/// C023A_AllyInRange_Condition.
///
/// The closest teammate (or car) in DashRange ahead (dot >= -0.1) or within 225: a fighter that is a player character,
/// has no C023A_AttachCooldown and whose attach chain does not reach the owner; a car not driven and not overheated.
/// LastDashTarget is written only when the list is not empty (@1831 leaves it).
// Unverified: GetDotProductTo / GetDistanceTo as stock 4.26 (the forward vector the facing; the 3D distance of the actor
// locations, depth 0). Unverified: a car's team is its owner's (the car's Comp_Actor_Destructible.TeamIndex is not read).
pub fn find_dash_target(w: &mut World, f: FighterId) -> Option<ActorRef> {
    gizmo_of(w, f)?;
    let range =
        crate::math::narrow(data_of(w, f).fighter.pawn.get("DashRange").and_then(Value::as_f64).unwrap_or(1000.0));
    let (fx, fy) = (w.fighter(f).body.x, actor_y(w, f));
    // SphereOverlapActors(own location, DashRange, [Pawn, ProjectileDestructible, DamageDestructible], BaseFighter,
    // [self, GetAttachParentActor]) @180.
    let mut list: Vec<ActorRef> = Vec::new();
    let types =
        [crate::real(obj::PAWN), crate::real(obj::PROJECTILE_DESTRUCTIBLE), crate::real(obj::DAMAGE_DESTRUCTIBLE)];
    let ignore = [Some(ActorRef::Fighter(f)), attach_parent_of(w, f)];
    for o in sphere_overlap_actors(w, fx, fy, range, &types, &ignore, -1.0) {
        if let ActorRef::Fighter(g) = o
            && peers(w).contains(&g)
        {
            list.push(o);
        }
    }
    // GetAllActorsOfClass(C023A_Car_SpawnedActor) @293: not driven and in DashRange, AddUnique @890.
    let cars: Vec<(ProjectileId, Real, Real)> = w
        .actors
        .projectiles
        .iter()
        .filter(|(id, p)| is_car(w, ActorRef::Projectile(*id)) && !p.dead)
        .map(|(id, p)| (id, p.x, p.y))
        .collect();
    for (id, x, y) in cars {
        if car_driven(w, id) {
            continue;
        }
        let o = ActorRef::Projectile(id);
        if dist(fx, fy, x, y) <= range && !list.contains(&o) {
            list.push(o);
        }
    }
    if list.is_empty() {
        return None; // @1831: TargetFound false, LastDashTarget kept
    }
    let mut best: Option<ActorRef> = None;
    let mut best_range: Real = 1e9; // LocalClosestRange @35
    let my_facing = facing(w, f);
    let my_team = w.fighter(f).team.team;
    for t in list {
        let (tx, ty) = target_place(w, t);
        let d = dist(fx, fy, tx, ty);
        let dot = if d > 1e-4 { (my_facing * (tx - fx)) / d } else { 0.0 };
        if !(dot >= -0.1 || d <= 225.0) {
            continue; // @1456 (both interfaces hold for a fighter and the car)
        }
        match t {
            ActorRef::Projectile(id) => {
                if car_overheated(w, id) {
                    continue; // @1794
                }
                let owner_team = w.actors.projectiles.get(id).and_then(|p| p.owner).map(|o| w.fighter(o).team.team);
                if owner_team != Some(my_team) {
                    continue; // @3185 (Unverified above)
                }
            }
            ActorRef::Fighter(g) => {
                if w.fighter(g).life.not_player_character {
                    continue; // @2364
                }
                if buff_of(w, g, name!("C023A_AttachCooldown_C")).is_some() {
                    continue; // CheckHasBuff @2489
                }
                if attach_chain_reaches(w, g, f) {
                    continue; // @2857-3065
                }
                if w.fighter(g).team.team != my_team {
                    continue; // @3185
                }
            }
            _ => continue,
        }
        if d < best_range {
            best_range = d;
            best = Some(t);
        }
    }
    set_last_dash_target(w, f, best); // @1976 / @2029
    best
}

fn dist(x0: Real, y0: Real, x1: Real, y1: Real) -> Real {
    dist2d([x0, y0], [x1, y1])
}

/// GetAttachParentActor up the chain until it is invalid or the owner (`attach_parent_of`: a Gizmo's CarryingActor
/// through the shrink buff's entry below, a carry, a hold, an attach buff).
// Unverified: the chain ends at an item (a carried fighter's parent): an item's own attach parent is not followed.
fn attach_chain_reaches(w: &World, t: FighterId, me: FighterId) -> bool {
    let mut p = attach_parent_of(w, t);
    let mut i = 0;
    while let Some(ActorRef::Fighter(g)) = p
        && g != me
        && i < 16
    {
        p = attach_parent_of(w, g);
        i += 1;
    }
    p == Some(ActorRef::Fighter(me))
}

/// LastDashTarget: a fighter in `GizmoState::last_dash_target`, the car in `GizmoAttach::dash_car`; read as one (a
/// destroyed car fails IsValid: none) here and by pawn.rs's `last_dash_target` hook, the same rule.
fn set_last_dash_target(w: &mut World, f: FighterId, t: Option<ActorRef>) {
    let s = gizmo_w(w, f);
    s.last_dash_target = match t {
        Some(ActorRef::Fighter(g)) => Some(g),
        _ => None,
    };
    s.attach.dash_car = match t {
        Some(ActorRef::Projectile(c)) => Some(c),
        _ => None,
    };
}

/// `lastDashTarget`: `last_dash_target`, or the live car in `attach.dash_car`.
pub fn last_dash_target(w: &World, f: FighterId) -> Option<ActorRef> {
    let s = gizmo_of(w, f)?;
    if let Some(g) = s.last_dash_target {
        return Some(ActorRef::Fighter(g));
    }
    let car = s.attach.dash_car?;
    w.actors.projectiles.get(car).filter(|p| !p.dead).map(|_| ActorRef::Projectile(car))
}

/// The component pawn.rs's `ally_attach` hook hands out (`setAllyAttachComp`'s function): `None` for a fighter that
/// is no Gizmo.
pub fn ally_attach(w: &World, f: FighterId) -> Option<AllyAttach> {
    let s = gizmo_of(w, f)?;
    Some(AllyAttach { is_attached: s.attach.attached, attach_to_actor })
}

/// `attachToActor(w, g, actor, attach)`.
fn attach_to_actor(w: &mut World, g: FighterId, actor: Option<FighterId>, attach: bool) {
    if !attach {
        detach(w, g);
    } else if let Some(a) = actor {
        attach_to(w, g, a);
    }
}

// ---- the hooks ----

/// `boundSignal`: the owner's DamageTaken / InputeTriggered / OnDeath and the carried ally's FacingChanged /
/// OnExitedArena / NewBuffSpawned / the share buff's OnTakeDamage.
#[allow(clippy::too_many_arguments, reason = "the hook's parameters")]
pub fn bound_signal(
    w: &mut World,
    f: FighterId,
    from: FighterId,
    sig: FighterSignal,
    _flag: bool,
    num_: Real,
    tag: Name,
    by: Option<FighterId>,
) {
    let Some(s) = gizmo_of(w, f) else { return };
    let a = &s.attach;
    if from == f {
        match sig {
            FighterSignal::TakeAnyDamage => {
                // DamageTaken @2818 (the owner's OnTakeAnyDamage, bound @1784)
                if a.attached {
                    update_meter(w, f, num_ / KNOCK_OFF_DAMAGE);
                }
            }
            FighterSignal::InputTriggered => {
                // InputeTriggered (bound @4302 while attached): InputType 2 or 3
                if !a.input_bound || (num_ != 2.0 && num_ != 3.0) {
                    return;
                }
                detach(w, f); // AttachToActorMulticast(null, false) @114
                net_impulse(w, f, 0.0, 1500.0, true, true, 0.0, false, 0.0); // NetImpulse((0, 0, 1500), true, true, 0) @437
            }
            FighterSignal::ExitedArena => {
                // OnDeath @3842 (bound to the owner's OnExitedArena @1943)
                detach(w, f); // @3914
                play_montage(w, f, name!("C023A_Lantern_Exit_Montage")); // @3930
            }
            _ => {}
        }
        return;
    }
    if a.carrying != Some(from) {
        return;
    }
    // C023A_ShareDamageBuff OnTakeDamage (its own binding on the ally's OnTakeAnyDamage): ApplyDamage(Instigator,
    // Damage x 2 x DamageTransferPercent, DamageCauser) @132.
    // Unverified: run here, for the share buff this component spawned.
    if sig == FighterSignal::TakeAnyDamage
        && let Some(sb) = live(w, a.share_buff)
    {
        let pct = match inst_def_of(w, sb) {
            Some(d) => num(&d, "DamageTransferPercent").unwrap_or(0.25),
            None => buff_def(w, ActorRef::Fighter(from), name!("C023A_ShareDamageBuff_C"))
                .and_then(|d| num(d, "DamageTransferPercent"))
                .unwrap_or(0.25),
        };
        apply_direct_damage(w, f, (num_ * 2.0) * pct, by);
    }
    let Some(s) = gizmo_of(w, f) else { return };
    let a = &s.attach;
    if !a.ally_bound {
        return;
    }
    match sig {
        FighterSignal::Facing => {
            // CarryTurn @99-145: BPI_AttachTurn(AttachOffset / AttachOffsetScale, false)
            let rel = mesh_rel(w, from);
            let (ox, oz) = (a.off_x, a.off_z);
            attach_turn(w, f, ox / rel, oz / rel, false);
        }
        FighterSignal::ExitedArena => {
            // OnCarryingActorExitArena @76-92
            detach(w, f);
            play_montage(w, f, name!("C023A_Lantern_Exit_Montage"));
        }
        FighterSignal::NewBuffSpawned => {
            // OnCarryingActorBuffSpawned @15-112: AddUnique the class, Delay(0)
            let a = &mut gizmo_w(w, f).attach;
            if !a.buff_queue.contains(&tag) {
                a.buff_queue.push(tag);
            }
            a.queue_due = true;
        }
        _ => {}
    }
}

/// The Delay(0.0) @112 resume @167: the newest class checked on the ally; a live buff of the movement group detaches.
fn queue_resume(w: &mut World, f: FighterId) {
    let (attached, carrying, cls) = {
        let a = &gizmo_w(w, f).attach;
        (a.attached, a.carrying, a.buff_queue.last().copied())
    };
    let Some(c) = carrying.filter(|_| attached) else {
        gizmo_w(w, f).attach.buff_queue.clear(); // Array_Clear @933 / @975
        return;
    };
    let b = cls.and_then(|cls| buff_of(w, c, cls)); // BPI_CheckIfHasBuff @458
    // The live buff actor's StateInfo (the cast @545, @624), its class default without an instance record.
    if let Some(b) = b {
        let cls = buff(w, b).map_or(Name::NONE, |x| x.cls);
        let group = match inst_def_of(w, b) {
            Some(d) => state_group(&d),
            None => buff_def(w, ActorRef::Fighter(c), cls).and_then(state_group),
        };
        if group.as_deref() == Some(MOVE_GROUP) {
            detach(w, f); // AttachToActorMulticast(null, false) @700
        }
    }
    let a = &mut gizmo_w(w, f).attach;
    a.buff_queue.pop(); // Array_Remove(LastIndex) @775
    if !a.buff_queue.is_empty() {
        a.queue_due = true; // goto 112: a new Delay(0), the next frame's pass
    }
}

/// `StateInfo.Group` of a buff's values.
fn state_group(d: &mvs_data::value::Props) -> Option<String> {
    d.get("StateInfo").and_then(Value::as_obj).and_then(|s| s.get("Group")).and_then(Value::as_str).map(str::to_owned)
}

/// Pawn_C023A CarryTurnTimeline's tick (dilation: the hit pause's).
pub fn component_tick(w: &mut World, f: FighterId, dilation: Real) {
    let Some(s) = gizmo_mut(w, f) else { return };
    let a = &mut s.attach;
    if !a.turn_on {
        return;
    }
    a.turn_pos += DELTA_SECONDS * dilation;
    if a.turn_pos >= TURN_LENGTH {
        a.turn_pos = TURN_LENGTH;
        a.turn_on = false;
    }
    turn_update(w, f);
}

/// `worldTick`: the attach: the owner follows the carrier's Root socket (the attach chain's update after the parent
/// moved) and turns with it (the rotation is relative to the mesh after the attach).
// Deviation: the facing follows at once.
pub fn world_tick(w: &mut World, f: FighterId) {
    let Some(s) = gizmo_of(w, f) else { return };
    let Some(c) = s.attach.carrying.filter(|_| s.attach.attached) else { return };
    let off_z = s.attach.off_z;
    let cf = facing(w, c);
    if facing(w, f) != cf {
        set_facing(w, f, cf);
    }
    let (px, py) = follow_place(w, off_z, c);
    if px != w.fighter(f).body.x || py != actor_y(w, f) {
        recall_move(w, f, px, py);
    }
}

/// attach.rs's part of the joined timerTick (after pawn.rs's): the latent pass (the Delay(0) of
/// OnCarryingActorBuffSpawned), then the AttachTimer (K2_SetTimer looping, 0.5): AttachTimerTick ->
/// UpdateMeter(AttachTimerItervals / AttachTime, false).
pub fn timer_tick(w: &mut World, f: FighterId) {
    let Some(s) = gizmo_mut(w, f) else { return };
    if s.attach.queue_due {
        s.attach.queue_due = false;
        queue_resume(w, f);
    }
    let tick = w.flow.tick;
    let a = &gizmo_w(w, f).attach;
    if a.timer_set >= 0 && crate::consts::clock(crate::real(tick - a.timer_set)) > f64::from(a.timer_next) {
        let next = a.timer_next + TIMER_RATE;
        gizmo_w(w, f).attach.timer_next = next;
        update_meter(w, f, TIMER_RATE / ATTACH_TIME);
    }
}

// ---------------------------------------------------------------- the notifies

/// AnimNotify_C023A_DashToAlly_C begin: BPI_GetLastDashTarget valid -> detach when attached, AtemptToReachAlly(target).
fn dash_to_ally_begin(w: &mut World, f: FighterId, _n: NotifyRef, _key: i32) {
    if gizmo_of(w, f).is_none() {
        return;
    }
    let Some(t) = last_dash_target(w, f) else { return }; // @121, IsValid @167
    if gizmo_w(w, f).attach.attached {
        detach(w, f); // @322 -> @451
    }
    atempt_to_reach_ally(w, f, t); // @582
}

/// C023A_StopMoveToAlly_C begin: attached -> detach; then StopMoveToAlly.
fn stop_move_to_ally_begin(w: &mut World, f: FighterId, _n: NotifyRef, _key: i32) {
    if gizmo_of(w, f).is_none() {
        return;
    }
    if gizmo_w(w, f).attach.attached {
        detach(w, f); // @93 -> @222
    }
    stop_move_to_ally(w, f); // @353
}

/// AnimNotify_C023A_MoveToActorHit_C begin: not attached -> BPI_SetActorLocation(MostRecentHitActor's location +
/// (facing left ? -r : r) in Y, + half height / 2 in Z), from its root capsule (BPI_ElementalSystem BPI_GetRootCapsule).
// Unverified: a HitActor that is no fighter is taken as having no root capsule (0, 0); none: the origin (the null
// reads' zeros).
fn move_to_actor_hit_begin(w: &mut World, f: FighterId, _n: NotifyRef, _key: i32) {
    let Some(s) = gizmo_of(w, f) else { return };
    if s.attach.attached {
        return; // @93-224
    }
    let left = facing(w, f) < 0.0; // @359
    let h = w.fighter(f).decay.recent_hit_actor; // the HitBoxHandler's MostRecentHitActor
    let (mut hx, mut hy, mut r, mut hh) = (0.0, 0.0, 0.0, 0.0);
    match h {
        Some(ActorRef::Fighter(g)) if peers(w).contains(&g) => {
            hx = w.fighter(g).body.x;
            hy = actor_y(w, g);
            r = cap_radius(w, g);
            hh = cap_half(w, g);
        }
        Some(ActorRef::Projectile(p)) => {
            if let Some(p) = w.actors.projectiles.get(p) {
                hx = p.x;
                hy = p.y;
            }
        }
        _ => {}
    }
    let ue_y = (-hx) + if left { -r } else { r }; // SelectFloat @1115, @1262
    recall_move(w, f, -ue_y, (hh / 2.0) + hy); // @1216, BPI_SetActorLocation @1359
}

/// AnimNotify_C023A_ApplyBuffToAttached_C begin: BPI_CheckIsAttached -> SpawnBuffActor(Buff, AttachedActor, Owner,
/// Stacks, BuffDuration) on the owner's manager; with DestroyBuffsOnNotifyEnd the buff is filed under this notify
/// (AddNotifySpawnedBuffs @482).
fn apply_buff_to_attached_begin(w: &mut World, f: FighterId, n: NotifyRef, _key: i32) {
    let Some(s) = gizmo_of(w, f) else { return };
    let Some(c) = s.attach.carrying.filter(|_| s.attach.attached) else { return };
    let p = notify_props(w, f, n);
    let Some(cls) = p.get("Buff").and_then(Value::as_str).filter(|c| !c.is_empty()) else { return };
    let cls = data_name(w, cls);
    let duration = num(p, "BuffDuration").unwrap_or(0.0);
    let stacks = num(p, "Stacks").unwrap_or(0.0);
    let destroy = crate::registry::props::truthy(p.get("DestroyBuffsOnNotifyEnd").unwrap_or(&Value::Null));
    #[allow(clippy::cast_possible_truncation, reason = "a notify's Stacks is an integer")]
    let nb = spawn_buff(w, c, cls, duration, Some(stacks as i32), Some(f), true, Some(f)); // @283
    if let Some(nb) = nb
        && destroy
    {
        if let Some(x) = buff_mut(w, nb) {
            x.notify = Some(n);
        }
        let a = &mut gizmo_w(w, f).attach;
        if !a.notify_holders.contains(&c) {
            a.notify_holders.push(c);
        }
    }
}

/// AnimNotify_C023A_ApplyBuffToAttached_C end: RemoveNotifySpawnedBuffs(self, Stacks) (every buff filed, on whichever
/// fighter it is).
fn apply_buff_to_attached_end(w: &mut World, f: FighterId, n: NotifyRef, _key: i32) {
    if gizmo_of(w, f).is_none() {
        return;
    }
    let destroy =
        crate::registry::props::truthy(notify_props(w, f, n).get("DestroyBuffsOnNotifyEnd").unwrap_or(&Value::Null));
    if !destroy {
        return;
    }
    let hs = std::mem::take(&mut gizmo_w(w, f).attach.notify_holders);
    for h in hs {
        remove_notify_buffs(w, h, n);
    }
}

// ---------------------------------------------------------------- the buffs

/// C023A_AttachCooldown_C begin: the state widget on its owner (AddStateForPlayer @858).
fn cooldown_begin(w: &mut World, f: FighterId, _b: BuffRef, _from: Option<FighterId>, _dur: Option<Real>) {
    w.fighter_mut(f).controls.ui_state_ops.push(UiStateOp::Add(UiStateParams {
        id: name!("CarryC023a"),
        state_type: 2,
        display_type: 0,
        current_value: -1.0,
        max_value: -1.0,
        timer: 9.0,
        icon_material: name!("MI_State_Attach"),
        active: true,
        pop_text: Name::NONE,
        auto_remove_time: -1.0,
        subicon: None,
    }));
}

/// C023A_AttachCooldown_C end: RemoveStateForPlayer @210.
fn cooldown_end(w: &mut World, f: FighterId, _b: BuffRef) {
    w.fighter_mut(f).controls.ui_state_ops.push(UiStateOp::Remove(name!("CarryC023a")));
}

/// Buff_C023A_AttachShrink_C begin: BPI_DisableDash(true) at its BeginPlay.
fn shrink_begin(w: &mut World, f: FighterId, _b: BuffRef, _from: Option<FighterId>, _dur: Option<Real>) {
    crate::fighter::moves::bpi_disable_dash(w, f, true);
}

/// Buff_C023A_AttachShrink_C end: (false) at its EndPlay.
fn shrink_end(w: &mut World, f: FighterId, _b: BuffRef) {
    crate::fighter::moves::bpi_disable_dash(w, f, false);
}

/// Buff_C023A_AttachShrink_C actorCollisionOff: the owner's SetActorEnableCollision(false) of Attach to Actor (@2430
/// .. @799) is carried on this entry (`attach.collision_off`).
fn shrink_collision_off(w: &World, f: FighterId, _b: BuffRef) -> bool {
    gizmo_of(w, f).is_some_and(|s| s.attach.collision_off)
}

/// Buff_C023A_AttachShrink_C attachParent: Attach to Actor's InitiateGrapple(the carrier's Mesh, Root) @3001 ..
/// RemoveGrapple @387: the owner's attach parent is CarryingActor while attached (GetAttachParentActor). Carried on
/// this entry: the buff lives from @4588 to @2200, inside the attach.
fn shrink_attach_parent(w: &World, f: FighterId, _b: BuffRef) -> Option<FighterId> {
    let a = &gizmo_of(w, f)?.attach;
    if a.attached { a.carrying } else { None }
}

// ---------------------------------------------------------------- the conditions

/// `attached`: BPI_CheckIsAttached (a failed cast: false).
fn attached(w: &World, f: FighterId) -> bool {
    gizmo_of(w, f).is_some_and(|s| s.attach.attached)
}

/// C023A_AllyInRange_Condition_C: BPI_FindDashTarget's TargetFound (a fighter that is no Pawn_C023A: false).
fn ally_in_range(w: &mut World, f: FighterId) -> bool {
    gizmo_of(w, f).is_some() && find_dash_target(w, f).is_some()
}

/// C023A_AttachedCondition_C: BPI_CheckIsAttached.
fn attached_condition(w: &mut World, f: FighterId) -> bool {
    attached(w, f)
}

/// C023A_NotAttachedCondition_C: Not(C023A_AttachedCondition's Check).
fn not_attached_condition(w: &mut World, f: FighterId) -> bool {
    !attached(w, f)
}

/// C023A_OnGroundOrAttachedCondition_C: PlayerOnGroundCondition's Check OR BPI_CheckIsAttached.
fn on_ground_or_attached(w: &mut World, f: FighterId) -> bool {
    cond(w, f, name!("PlayerOnGroundCondition_C")) || attached(w, f)
}

/// This file's registry entries, in attach.rs's order: the ReachedActor binder, the notifies, the buffs, the
/// conditions.
pub fn register(r: &mut Registry) -> Result<(), RegistryError> {
    crate::combat::move_to_actor_2::move_to_actor_2_reached(r, REACHED, actor_reached)?;
    let begin =
        |f: fn(&mut World, FighterId, NotifyRef, i32)| NotifyHandler { begin: Some(f), ..NotifyHandler::default() };
    r.notifies.enter("AnimNotify_C023A_DashToAlly_C", begin(dash_to_ally_begin))?;
    r.notifies.enter("C023A_StopMoveToAlly_C", begin(stop_move_to_ally_begin))?;
    r.notifies.enter("AnimNotify_C023A_MoveToActorHit_C", begin(move_to_actor_hit_begin))?;
    r.notifies.enter(
        "AnimNotify_C023A_ApplyBuffToAttached_C",
        NotifyHandler {
            begin: Some(apply_buff_to_attached_begin),
            end: Some(apply_buff_to_attached_end),
            ..NotifyHandler::default()
        },
    )?;
    r.buffs.enter(
        ATTACH_COOLDOWN,
        BuffEntry::new(
            BuffClass::C023AAttachCooldown,
            BuffHooks { begin: Some(cooldown_begin), end: Some(cooldown_end), ..BuffHooks::default() },
        ),
    )?;
    r.buffs.enter(SHARE_DAMAGE, BuffEntry::new(BuffClass::C023AShareDamageBuff, BuffHooks::default()))?;
    r.buffs.enter(
        ATTACH_SHRINK,
        BuffEntry::new(
            BuffClass::BuffC023AAttachShrink,
            BuffHooks {
                begin: Some(shrink_begin),
                end: Some(shrink_end),
                actor_collision_off: Some(shrink_collision_off),
                attach_parent: Some(shrink_attach_parent),
                ..BuffHooks::default()
            },
        ),
    )?;
    r.conditions.enter("C023A_AllyInRange_Condition_C", Condition::Fn(ally_in_range))?;
    r.conditions.enter("C023A_AttachedCondition_C", Condition::Fn(attached_condition))?;
    r.conditions.enter("C023A_NotAttachedCondition_C", Condition::Fn(not_attached_condition))?;
    r.conditions.enter("C023A_OnGroundOrAttachedCondition_C", Condition::Fn(on_ground_or_attached))
}
