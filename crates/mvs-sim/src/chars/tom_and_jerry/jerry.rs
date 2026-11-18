//! Jerry_SpawnedActor and its TempInvincibility child: Jerry free in
//! the world ([`JerryActor`], a DealerActor with a ProjectileMovement of its own), his bounce and slide, his overlaps
//! (Tom's pickup, an ally's), his hits and hurt, ActivateObject's switch (champagne.rs, rocket.rs), the proxy's notify
//! switches (champagne.rs, tennis.rs), and the actor API the other files call. `jerryCore` and
//! `champagneJerry` (set by jerry.rs at its load) are this file's functions.

use std::sync::LazyLock;

use indexmap::IndexSet;
use mvs_collision::Ground;
use mvs_collision::capsule::{capsule_gap, capsule_sweep};
use mvs_collision::capsule_mtd::capsule_mtd;
use mvs_data::fighter::ActorDef;
use mvs_data::value::{Props, Value};

use crate::Real;
use crate::State;
use crate::actors::actor_montage::{ActorAnim, MontageEvent, montage_active, montage_play, montage_stop};
use crate::actors::follower::{FollowerMeshVis, follower_broadcast};
use crate::actors::world_actor::{WorldActorState, actor_p};
use crate::actors::{ActorClass, ActorId, ActorRef, WorldActor, WorldActorClass};
use crate::combat::boxes::{ExtraHurt, WorldSphere};
use crate::combat::buffs::remove_buff;
use crate::combat::elem::{charge_pct, elem_spawn, instigator_charge};
use crate::combat::hit::hurt_hit;
use crate::combat::hits::{
    DamageCall, HitShape, actor_hit_enemy, apply_projectile_hit, dealer, do_damage, hit_box_actor_hits,
    projectile_blocks,
};
use crate::combat::packet::DataVec;
use crate::combat::phases::{alive, give_vbuf, no_hit, victim_pose};
use crate::combat::spawned_hits::{class_rows, hit_box_types};
use crate::combat::{BuffClass, HitPacket};
use crate::consts::{DELTA_SECONDS, DT, GRAVITY_Z};
use crate::events::{Event, SoundKey};
use crate::fighter::common::{actor_class_def, data_of, latent_frames, warn_text};
use crate::fighter::control::{attack_dodged, dodge_over, game_mode_cooldown_disabled};
use crate::fighter::moves::V2;
use crate::fighter::traces::{TraceBody, body_info};
use crate::fighter::{FighterId, actor_y, cap_radius, capsule_half, invulnerable};
use crate::math::ue::{SMALL_NUMBER, clamped_to_max_size, rot_from_x, safe_normal, size};
use crate::math::{max, min, to_int32};
use crate::name;
use crate::names::Name;
use crate::registry::entries::{ActorEntry, ActorHooks, MeshHooks, SizeHooks, WorldActorHooks};
use crate::registry::props::{flag, num as pnum, obj};
use crate::registry::signals::ActorBuffSignal;
use crate::registry::{Registry, RegistryError};
use crate::world::World;
use crate::world::blast::{BlastBounds, blast_box_loop, blast_timer_exit, new_blast_bounds, spawn_box};
use crate::world::flow::reset_pass;
use crate::world::pawns::match_fighters;
use crate::world::spawned_platform::BlockKind;
use crate::world::spawning::{add_actor, after_spawned_actor, new_world_actor};
use crate::world::stage_query::{ShapeRef, Shapes, StageShapes};

use super::champagne::{
    champagne_attack, champagne_notify, champagne_shoot, champagne_timer_function, cork_begin_play, set_cork_ammo,
};
use super::follower::{self, attach_to_ally, follower_tick};
use super::pawn::{set_is_jerry_attached, trigger_jerry_cooldown};
use super::rocket::launch_rocket;
use super::state::{tj_mut, tj_of};
use super::tennis::tennis_window;

/// Jerry's class.
pub const JERRY: &str = "Jerry_SpawnedActor_C";
/// The child the rockets' ends spawn (DamageDisabled for its first 0.2 s).
pub const JERRY_TEMP_INVINCIBLE: &str = "Jerry_SpawnedActor_TempInvincibility_C";

/// `JerryActor` (Jerry_SpawnedActor_C, Jerry_SpawnedActor_TempInvincibility_C): what the view and the other files read
/// of him (`JerryFields`), his movement and his latent actions. The fields are the class's, in its order.
#[derive(State, Debug)]
pub struct JerryActor {
    /// JerryMesh's anim instance (PlayMontageMulticast through its proxy).
    pub anim: ActorAnim,
    /// Comp_Pawn_HitBoxHandler's packet: HitBoxDamage, written by UpdateHitBoxDamage (@616, @6109, @24634).
    pub hit: HitPacket,
    /// Its place (sim x, y): the root Capsule's centre.
    pub x: Real,
    pub y: Real,
    /// ProjectileMovement.Velocity (sim units per second); 0 while following.
    pub vx: Real,
    pub vy: Real,
    /// FacingLeft (RotateMulticast, follower.rs): -1 left, 1 right (the CDO's false).
    pub facing: Real,
    /// The actor's rotation (UE degrees).
    pub rot_pitch: Real,
    pub rot_yaw: Real,
    /// JerryAnimBlueprint IsFlying (CDO true).
    pub flying: bool,
    pub falling: bool,
    pub wall_sliding: bool,
    pub jumping: bool,
    pub jump_number: i32,
    pub fast_fall: bool,
    /// JerryAnimBlueprint.HitReactInt while HitReact is true, -1 while it is false.
    pub hit_react: i32,
    /// The HitShake timeline's time (s) since its PlayFromStart @22398, -1 while it is not playing.
    pub hit_shake_t: Real,
    /// JerryMaterialDynamic's ColorMap (1 at a hit @22644, 0 after Delay 0.15 @72).
    pub color_map: Real,
    /// Trail_Effect's visibility.
    pub trail: bool,
    /// WasRungOut (DieWithFollower @29119, OnExitedArena @31950).
    pub was_rung_out: bool,
    /// The ally it rides (K2_AttachToActor @2146, follower.rs), none when free.
    pub following: Option<FighterId>,
    /// IsAttacking (BPI_GetIsAttacking).
    pub is_attacking: bool,
    /// JerryMesh's visibility (follower.rs AdjustVisibility); none: hidden.
    pub mesh_vis: Option<FollowerMeshVis>,
    /// The props the montage windows show: the Champagne bottle and PS_AttackChargeUP (champagne.rs), the Rocket
    /// mesh, the TennisBall and Tenis_Rachet_Mesh.
    pub champagne: bool,
    pub charge_up: bool,
    pub rocket: bool,
    pub tennis_ball: bool,
    pub racket: bool,
    /// JerryHealthHP.
    pub hp: Real,
    /// HitBoxOn?.
    pub hit_on: bool,
    /// HitEachActorOnlyOnce, by fighter id (cleared when the hit box goes off).
    pub hit_set: IndexSet<i32>,
    /// The actors that are no fighter the hit box overlaps (`hit_box_actor_hits`).
    pub over: IndexSet<ActorRef>,
    /// Their HitEachActorOnlyOnce track (cleared with the box).
    pub once: IndexSet<ActorRef>,
    /// The object types the hit box's template answers (`hit_box_types`).
    pub types: Vec<Real>,
    /// ReflectDamageArea's SphereRadius, or ReflectDamageArea2D's CapsuleRadius (2D hit boxes).
    pub hit_r: Real,
    pub is_2d: bool,
    /// AimDownProjectileMovement.
    pub aim_down: bool,
    /// ProjectileMovement active (Deactivate at the follow).
    pub moving: bool,
    /// OpenBounce (SetVelocityMulticast @28175).
    pub open_bounce: bool,
    /// BounceCount.
    pub bounce_count: i32,
    /// The Gate of OnBounce @22936-23037.
    pub bounce_gate: bool,
    /// PickUpGate (after the Delay 0.5 @15).
    pub pick_up_gate: bool,
    /// AllyPickUpGate (at each bounce @22784).
    pub ally_pick_up_gate: bool,
    /// ResetJerry (SetResetJerryMulticast @28318).
    pub reset_jerry: bool,
    /// IgnorePlatformsCheck.
    pub ignore_platforms: bool,
    /// The DoOnce @6019.
    pub kb_once: bool,
    pub bounciness: Real,
    pub friction: Real,
    pub grav_scale: Real,
    /// SpawnedActorParentVelocity.
    pub parent_vx: Real,
    pub parent_vy: Real,
    /// ProjectileMovement.MaxSpeed (0: no clamp).
    pub max_speed: Real,
    /// ActivationConditions.
    pub conds: [bool; 5],
    /// Comp_Actor_Destructible.DamageDisabled (the TempInvincibility child's).
    pub damage_disabled: bool,
    /// Latent actions, ticks left (0: not running): Delay 0.5 @15, Delay 0.033 @123, Delay 0.15 @72, the child's
    /// Delay 0.2 @15.
    pub pick_wait: i32,
    pub dir_wait: i32,
    pub color_wait: i32,
    pub dmg_wait: i32,
    /// Ticks, the spawn frame counted.
    pub age: i32,
    /// CheckBlastBoxTimer's next expiry.
    pub blast_at: crate::Clock,
    pub blast_bounds: BlastBounds,
    /// The fighters overlapping it, by id.
    pub inside: IndexSet<i32>,
    /// EndPlay has run.
    pub ended: bool,
    /// HandleDeflection's memory between the frame's passes (PreviousHitTime / Normal).
    pub prev_hit_time: Real,
    pub prev_nx: Real,
    pub prev_ny: Real,
    pub sliding: bool,
    /// AttackID (the owner's attack count at the spawn, -1337 for the TempInvincibility child).
    pub attack_id: i32,
}

impl Default for JerryActor {
    /// The field initial values.
    fn default() -> Self {
        JerryActor {
            anim: ActorAnim::new(name!("JerryMesh")),
            hit: HitPacket::default(),
            x: 0.0,
            y: 0.0,
            vx: 0.0,
            vy: 0.0,
            facing: 1.0,
            rot_pitch: 0.0,
            rot_yaw: 0.0,
            flying: true,
            falling: false,
            wall_sliding: false,
            jumping: false,
            jump_number: 0,
            fast_fall: false,
            hit_react: -1,
            hit_shake_t: -1.0,
            color_map: 0.0,
            trail: true,
            was_rung_out: false,
            following: None,
            is_attacking: false,
            mesh_vis: Some(FollowerMeshVis::All),
            champagne: false,
            charge_up: false,
            rocket: false,
            tennis_ball: false,
            racket: false,
            hp: 0.0,
            hit_on: false,
            hit_set: IndexSet::new(),
            over: IndexSet::new(),
            once: IndexSet::new(),
            types: Vec::new(),
            hit_r: 0.0,
            is_2d: false,
            aim_down: false,
            moving: false,
            open_bounce: false,
            bounce_count: 0,
            bounce_gate: true,
            pick_up_gate: false,
            ally_pick_up_gate: false,
            reset_jerry: false,
            ignore_platforms: false,
            kb_once: false,
            bounciness: 0.0,
            friction: 0.0,
            grav_scale: 0.0,
            parent_vx: 0.0,
            parent_vy: 0.0,
            max_speed: 0.0,
            conds: [false; 5],
            damage_disabled: false,
            pick_wait: 0,
            dir_wait: 0,
            color_wait: 0,
            dmg_wait: 0,
            age: 0,
            blast_at: f64::from(0.1_f32),
            blast_bounds: new_blast_bounds(),
            inside: IndexSet::new(),
            ended: false,
            prev_hit_time: 1.0,
            prev_nx: 0.0,
            prev_ny: 0.0,
            sliding: false,
            attack_id: 0,
        }
    }
}

/// The Jerry actor `id`, none for another actor or a gone one.
pub fn jerry_of(w: &World, id: ActorId) -> Option<&JerryActor> {
    match &w.actors.actors.get(id)?.state {
        WorldActorState::JerryActor(s) => Some(s),
        _ => None,
    }
}

/// [`jerry_of`], mutable.
pub fn jerry_mut(w: &mut World, id: ActorId) -> Option<&mut JerryActor> {
    match &mut w.actors.actors.get_mut(id)?.state {
        WorldActorState::JerryActor(s) => Some(s),
        _ => None,
    }
}

/// The Jerry actor's place (its `x`, `y`).
pub fn place(_w: &World, _a: &WorldActor, s: &JerryActor) -> Option<(Real, Real)> {
    Some((s.x, s.y))
}

/// The Jerry actor as a dealer (`DealerActor`): its place, facing and AttackID.
pub fn dealer_parts(_w: &World, _a: &WorldActor, s: &JerryActor) -> (Real, Real, Real, i32) {
    (s.x, s.y, s.facing, s.attack_id)
}

/// The Jerry actor's HitBoxDamage packet (`hit`).
pub fn hit(s: &JerryActor) -> &HitPacket {
    &s.hit
}

/// The Jerry actor's motion as a dealer: its velocity and `DealerActor.hitVelocity` (SpawnedActorParentVelocity while
/// set, else the ProjectileMovement's).
pub fn dealer_motion(_w: &World, _a: &WorldActor, s: &JerryActor) -> (Real, Real, Option<[Real; 2]>) {
    let hv = if s.parent_vx != 0.0 || s.parent_vy != 0.0 { [s.parent_vx, s.parent_vy] } else { [s.vx, s.vy] };
    (s.vx, s.vy, Some(hv))
}

// ---------------------------------------------------------------- reads

/// The Jerry state, for a live Jerry actor (a gone or foreign id is a bug of the caller).
fn jm(w: &mut World, id: ActorId) -> &mut JerryActor {
    jerry_mut(w, id).expect("a live Jerry actor")
}

fn is_dead(w: &World, id: ActorId) -> bool {
    w.actors.actors.get(id).is_none_or(|a| a.dead)
}

fn owner_of(w: &World, id: ActorId) -> Option<FighterId> {
    w.actors.actors.get(id).map(|a| a.owner)
}

/// The class's `p` (the actor's CDO).
fn pp(w: &World, id: ActorId) -> &Props {
    actor_p(w, ActorRef::Actor(id))
}

// ---------------------------------------------------------------- sounds and emitters

fn sound(w: &mut World, id: ActorId, ev: Name) {
    let Some(a) = w.actors.actors.get(id) else { return };
    let Some(j) = jerry_of(w, id) else { return };
    let e = Event::Sound {
        fighter: a.owner_id,
        ev,
        x: Some(j.x),
        y: Some(j.y),
        hit: None,
        pooled: false,
        key: Some(SoundKey { tag: name!("ak"), n: a.seq }),
    };
    w.events.push(e);
}

fn sound_at(w: &mut World, id: ActorId, ev: Name, x: Real, y: Real) {
    let Some(a) = w.actors.actors.get(id) else { return };
    let e = Event::Sound { fighter: a.owner_id, ev, x: Some(x), y: Some(y), hit: None, pooled: false, key: None };
    w.events.push(e);
}

fn emitter(w: &mut World, id: ActorId, vfx: Name, x: Real, y: Real, scale: Real) {
    let Some(a) = w.actors.actors.get(id) else { return };
    let e = Event::ProjectileDeath {
        cls: a.cls,
        fighter: a.owner_id,
        vfx: Some(vfx),
        x,
        y,
        ox: 0.0,
        rot: [0.0; 3],
        scale,
        tint: None,
        to: None,
    };
    w.events.push(e);
}

// ---------------------------------------------------------------- the graph's own events

/// IsAttackingStart, bound to Jerry's own event @1360: IsAttacking and Comp_Follower's FollowerIsAttackingStart.
fn is_attacking_start(w: &mut World, id: ActorId) {
    jm(w, id).is_attacking = true;
    follower_broadcast(w, ActorRef::Actor(id), ActorBuffSignal::FollowerIsAttackingStart);
}

/// IsAttackingEnd, bound to @1402 (FollowerIsAttackingEnd).
fn is_attacking_end(w: &mut World, id: ActorId) {
    jm(w, id).is_attacking = false;
    follower_broadcast(w, ActorRef::Actor(id), ActorBuffSignal::FollowerIsAttackingEnd);
}

fn rotation_zero(j: &mut JerryActor) {
    j.rot_pitch = 0.0;
    j.rot_yaw = 0.0;
}

/// `jerryCore.hitboxOff`: UpdateHitbox with HitBoxOn? false: the hit box off (its hit-once set goes with the
/// collision).
pub fn hitbox_off(w: &mut World, j: ActorId) {
    let s = jm(w, j);
    s.hit_on = false;
    s.hit_set.clear();
    s.over.clear();
    s.once.clear();
}

/// `jerryCore.flyingEnd` (JerryFlyingMulticast @26866): IsFlying false, Trail_Effect hidden, broadcast IsAttackingEnd.
pub fn flying_end(w: &mut World, j: ActorId) {
    let s = jm(w, j);
    s.flying = false;
    s.trail = false;
    is_attacking_end(w, j);
}

/// `champagneJerry.updateAnimState` (UpdateAnimState @31009): HitBoxOn? false, UpdateHitbox,
/// AimDownProjectileMovement false, JerryFlyingMulticast, K2_SetActorRotation(0).
pub fn update_anim_state(w: &mut World, j: ActorId) {
    hitbox_off(w, j);
    jm(w, j).aim_down = false;
    flying_end(w, j);
    rotation_zero(jm(w, j));
}

/// `setVelocityMulticast` (SetVelocityMulticast @28115): the velocity, AimDownProjectileMovement and OpenBounce true.
/// Callers: ForceReflect (tennis.rs).
pub fn set_velocity_multicast(w: &mut World, j: ActorId, vx: Real, vy: Real) {
    let s = jm(w, j);
    s.vx = vx;
    s.vy = vy;
    s.aim_down = true;
    s.open_bounce = true;
}

/// `jerryCore.destroy` (K2_DestroyActor): its EndPlay (`end`) runs in the frame's list pass.
pub fn destroy(w: &mut World, j: ActorId) {
    if let Some(a) = w.actors.actors.get_mut(j) {
        a.dead = true;
    }
}

/// `jerryCore.stopMovement`: AimDownProjectileMovement false @1609, ProjectileMovement.Deactivate() @1620.
pub fn stop_movement(w: &mut World, j: ActorId) {
    let s = jm(w, j);
    s.aim_down = false;
    s.moving = false;
    s.vx = 0.0;
    s.vy = 0.0;
}

/// `jerryCore.damage`: JerryHealthHP -= n; the HP left. Called by follower.rs.
pub fn damage(w: &mut World, j: ActorId, n: Real) -> Real {
    let s = jm(w, j);
    s.hp -= n;
    s.hp
}

/// `jerryCore.setRungOut` (WasRungOut = true). Called by follower.rs.
pub fn set_rung_out(w: &mut World, j: ActorId) {
    jm(w, j).was_rung_out = true;
}

/// `setResetJerry` (SetResetJerryServer -> SetResetJerryMulticast @28318). Called by fishing.rs.
pub fn set_reset_jerry(w: &mut World, j: ActorId, reset: bool) {
    jm(w, j).reset_jerry = reset;
}

/// DeathVisuals(AlsoRespawn true) @28492-29118: PS_TomJerryPoof_Ghost (its Team parameter is the view's), the death VO
/// and the ghost sound at the actor.
fn death_visuals(w: &mut World, id: ActorId) {
    let Some(j) = jerry_of(w, id) else { return };
    let (x, y) = (j.x, j.y);
    emitter(w, id, name!("PS_TomJerryPoof_Ghost"), x, y, 1.0);
    sound_at(w, id, name!("vo_tomandjerry_eff_pain_jerry_death"), x, y);
    sound_at(w, id, name!("sfx_cha_tomandjerry_ghost"), x, y);
}

/// `jerryCore.hurt`: HitVisuals @22431 (its push_flows run Montage_Stop, then HitReact @22218, then the flash @22644,
/// the impact @22546, ChampagneTimerFunction @22531) and the pain VO @22002: ReceiveAnyDamage's tail @21988 and
/// FollowerDamage's.
pub fn hurt(w: &mut World, j: ActorId) {
    montage_stop(w, ActorRef::Actor(j), None); // Montage_Stop(0, null)
    if is_dead(w, j) {
        return;
    }
    // @22218: HitReact true, OnHitReactStart (bound by nothing ported), Comp_Follower FollowerHitReactStart, HitReactInt,
    // HitShake.PlayFromStart.
    follower_broadcast(w, ActorRef::Actor(j), ActorBuffSignal::FollowerHitReactStart);
    let r = w.random_int(4.0); // RandomIntegerInRange(0, 3): Min + RandomInteger(Max - Min + 1)
    let s = jm(w, j);
    s.hit_react = to_int32(r);
    s.hit_shake_t = 0.0;
    // @22644: ColorMap 1, Delay 0.15 (a Delay already running is not started again).
    s.color_map = 1.0;
    if s.color_wait == 0 {
        s.color_wait = to_int32(latent_frames(0.15));
    }
    let (x, y) = (s.x, s.y);
    emitter(w, j, name!("PS_HitImpact01"), x, y, 0.5);
    champagne_timer_function(w, j);
    sound(w, j, name!("vo_tomandjerry_eff_pain_jerry"));
}

/// ReceiveAnyDamage @21458 (the world's actor damage: dmg > 0 and CanBeHitBy).
fn any_damage(w: &mut World, id: ActorId, dmg: Real, by: ActorRef) {
    let s = jm(w, id);
    if s.following.is_some() {
        return;
    }
    s.hp -= dmg;
    if s.hp > 0.0 {
        hurt(w, id);
        return;
    }
    // @21594-21802: a causer that is a BPI_SpawnedActor with ProjectileAttackType 1, while HitBoxOn?:
    // pfgDoDamage(ReflectDamageArea, causer, HitBoxDamage, null, {[]}, {[]}).
    let (hit_on, packet) = (s.hit_on, s.hit.clone());
    if hit_on && pnum(actor_p(w, by), "ProjectileAttackType") == Some(1.0) {
        let ctx = DamageCall { hit_box: true, comp: Some(name!("ReflectDamageArea")), clank: false, rows: Some(&[]) };
        do_damage(w, ActorRef::Actor(id), by, &packet, &ctx);
    }
    death_visuals(w, id);
    destroy(w, id);
}

/// OnBounce @22784 (bound to ProjectileMovement.OnProjectileBounce @14802), inside the move, with the velocity the
/// bounce left.
fn on_bounce(w: &mut World, id: ActorId) {
    let max = pnum(pp(w, id), "BounceMaxCount").unwrap_or(0.0);
    let j = jm(w, id);
    j.ally_pick_up_gate = true;
    if j.open_bounce && crate::real(j.bounce_count) >= max {
        j.aim_down = false;
        rotation_zero(j);
        return;
    }
    if !j.bounce_gate {
        return;
    }
    j.bounce_count += 1;
    let n = crate::real(j.bounce_count);
    // n < BounceMaxCount @24397-24634 (the knockback direction from the velocity) cannot follow the increment with this
    // class's BounceMaxCount 1.
    if n < max {
        if j.hit_on {
            j.hit.knock_back_direction_vector =
                Some(DataVec { x: Some(if -j.vx >= 0.0 { 1.0 } else { -1.0 }), y: Some(1.0), z: None });
        }
        return;
    }
    j.bounciness = 0.2;
    j.friction = 5.0;
    let sp = size([0.0, j.vx, j.vy]);
    if n >= max + 1.0 {
        hitbox_off(w, id);
        let me = ActorRef::Actor(id);
        elem_spawn(w, me, BuffClass::BuffElementalIceproof, 0);
        elem_spawn(w, me, BuffClass::BuffElementalFireproof, 0);
        jm(w, id).aim_down = false;
        sound(w, id, name!("sfx_cha_tomandjerry_paddleball_bounce"));
        // @23942 K2_AttachToActor(the hit's actor) at a speed of 500 or less. Deviation: not ported (the stage's boxes are the
        // only blockers of the move here; a moving platform would carry him).
        rotation_zero(jm(w, id));
        flying_end(w, id);
        // @24029 Widget.SetVisibility(true): CorkAmmoSetrMulticast(CorkAmmo) @31094 does that and sets the count it has.
        if let Some(o) = owner_of(w, id)
            && let Some(s) = tj_of(w, o)
        {
            let n = s.cork_ammo;
            set_cork_ammo(w, id, n);
        }
        jm(w, id).bounce_gate = false;
        return;
    }
    sound(w, id, name!("sfx_cha_tomandjerry_paddleball_nojerry_bounce"));
    if sp <= 900.0 {
        let j = jm(w, id);
        j.aim_down = false;
        rotation_zero(j);
        return;
    }
    if montage_active(w, ActorRef::Actor(id)).is_none() {
        play_montage(w, id, name!("Tom_and_Jerry_Jerry_Fly_Bounce_Montage"));
    }
}

// ---------------------------------------------------------------- the movement (UProjectileMovementComponent)

/// The root Capsule (ECC_Pawn, Custom: it blocks WorldStatic and Platform, the second ignored while
/// IgnorePlatformsCheck), upright, scaled by the root's scale.
// Deviation: the actor's look-at pitch (ReceiveTick) turns the capsule in the build; the sweep keeps it upright.
#[derive(Clone, Copy)]
struct Cap {
    r: Real,
    hh: Real,
}

fn cap_of(w: &World, id: ActorId) -> Cap {
    let Some(a) = w.actors.actors.get(id) else { return Cap { r: 0.0, hh: 0.0 } };
    let g = a.size_scale.unwrap_or(1.0); // the root's scale (the sweep's and the overlaps' scaled capsule)
    let c = actor_class_def(w, a.owner, a.cls).and_then(|d| obj(&d.components, "Capsule"));
    Cap {
        r: c.and_then(|c| pnum(c, "CapsuleRadius")).unwrap_or(0.0) * g,
        hh: c.and_then(|c| pnum(c, "CapsuleHalfHeight")).unwrap_or(0.0) * g,
    }
}

/// The platforms the move meets: the world's, with IgnorePlatformsCheck (the Capsule's Platform response Ignore) the
/// stage solids of WorldStatic that block as the stage does (`isStaticSolid` of each: `traces::statics_among`); the
/// polygon solids after them.
fn platforms_of(w: &mut World, ignore: bool) -> StageShapes {
    let mut all = w.blocker_shapes(BlockKind::Nav);
    if !ignore {
        return all;
    }
    let mut out = Vec::new();
    crate::fighter::traces::statics_among(w, &all.boxes, &mut out);
    all.boxes = out;
    all.polys.retain(|q| q.is_static);
    all
}

/// One move's result (moveComp): the hit time, its normal, a start in penetration and its depth.
#[derive(Clone, Copy)]
struct Mv {
    t: Real,
    nx: Real,
    ny: Real,
    pen: bool,
    depth: Real,
}

impl Mv {
    fn new() -> Mv {
        Mv { t: 1.0, nx: 0.0, ny: 0.0, pen: false, depth: 0.0 }
    }
}

/// The way from the box to the capsule at (x, y) (its nearest points), normalised into `mv.nx` / `ny`. A polygon
/// solid: from its point nearest the capsule's centre to the segment's point nearest that.
fn contact_normal(mv: &mut Mv, b: ShapeRef, x: Real, y: Real, r: Real, hh: Real) {
    let s = max(hh - r, 0.0);
    let b = match b {
        ShapeRef::Box(g) => g,
        ShapeRef::Poly(q) => {
            let (qx, qy) = q.closest(x, y);
            let (nx, ny) = (x - qx, max(y - s, min(qy, y + s)) - qy);
            let l = (nx * nx + ny * ny).sqrt();
            mv.nx = if l > 0.0 { nx / l } else { 0.0 };
            mv.ny = if l > 0.0 { ny / l } else { 1.0 };
            return;
        }
    };
    let nx = x - max(crate::math::narrow(b.left), min(x, crate::math::narrow(b.right)));
    let mut ny = 0.0;
    if y - s > crate::math::narrow(b.top) {
        ny = y - s - crate::math::narrow(b.top);
    } else if y + s < crate::math::narrow(b.bottom) {
        ny = y + s - crate::math::narrow(b.bottom);
    }
    let l = (nx * nx + ny * ny).sqrt();
    mv.nx = if l > 0.0 { nx / l } else { 0.0 };
    mv.ny = if l > 0.0 { ny / l } else { 1.0 };
}

/// The shapes of the move: the ground, then the platforms, then the polygon solids.
fn box_at<'a>(g0: &'a Ground, ps: Shapes<'a>, i: usize) -> ShapeRef<'a> {
    let n = ps.boxes.len();
    if i == 0 {
        ShapeRef::Box(g0)
    } else if i <= n {
        ShapeRef::Box(&ps.boxes[i - 1])
    } else {
        ShapeRef::Poly(&ps.polys[i - 1 - n])
    }
}

/// Whether the capsule at (x, y), its radius grown by `infl`, overlaps the shape (a polygon solid: its segment within
/// that of it).
fn overlaps(b: ShapeRef, x: Real, y: Real, r: Real, hh: Real, infl: Real) -> bool {
    match b {
        ShapeRef::Box(g) => capsule_gap(x, y, r, hh, g) < f64::from(r + infl),
        ShapeRef::Poly(q) => {
            let s = max(hh - r, 0.0);
            q.seg_dist2(x, y - s, x, y + s) < (r + infl) * (r + infl)
        }
    }
}

/// The capsule's least way out of the shape (capsuleMtd; a polygon solid's own).
fn mtd_of(b: ShapeRef, x: Real, y: Real, r: Real, h: Real) -> mvs_collision::capsule_mtd::Mtd {
    match b {
        ShapeRef::Box(g) => capsule_mtd(g, x, y, r, h, 0.0, 1.0),
        ShapeRef::Poly(q) => q.mtd(x, y, r, h, 0.0, 1.0),
    }
}

/// The capsule's sweep a -> b against the shape (capsuleSweep; a polygon solid's own).
#[allow(clippy::too_many_arguments, reason = "a sweep and its shape")]
fn sweep_of(b: ShapeRef, ax: Real, ay: Real, bx: Real, by: Real, r: Real, hh: Real) -> Option<f64> {
    match b {
        ShapeRef::Box(g) => capsule_sweep(ax, ay, bx, by, r, hh, g),
        ShapeRef::Poly(q) => q.capsule_sweep(ax, ay, bx, by, r, hh).map(f64::from),
    }
}

/// One swept move of the root capsule (UPrimitiveComponent::MoveComponentImpl, as Projectile.moveComp): a start in
/// penetration is a hit at time 0 (the 2D capsule's least way out, capsuleMtd), dropped when the move goes out of the
/// box unless `never`; else the exact sweep (capsuleSweep) to the first box, pulled back (PullBackHit). True: a hit.
#[allow(clippy::too_many_arguments, reason = "the parameters")]
fn move_comp(
    j: &mut JerryActor,
    mv: &mut Mv,
    cap: Cap,
    g0: &Ground,
    ps: Shapes,
    dx: Real,
    dy: Real,
    never: bool,
) -> bool {
    *mv = Mv::new();
    let len = (dx * dx + dy * dy).sqrt();
    if len <= 4e-4 {
        return false;
    }
    let Cap { r, hh } = cap;
    let h = max(hh - r, 0.0);
    let (x0, y0) = (j.x, j.y);
    let mut best = 3.4e38;
    let mut skip = vec![false; ps.boxes.len() + ps.polys.len() + 1];
    for (i, sk) in skip.iter_mut().enumerate() {
        let b = box_at(g0, ps, i);
        if !overlaps(b, x0, y0, r, hh, 0.0) {
            continue;
        }
        let m = mtd_of(b, x0, y0, r, h);
        let d = m.nx * f64::from(dx) + m.ny * f64::from(dy);
        if !never && d > 0.0 {
            *sk = true;
            continue;
        }
        if d < best {
            best = d;
            mv.pen = true;
            mv.nx = crate::math::narrow(m.nx);
            mv.ny = crate::math::narrow(m.ny);
            mv.depth = crate::math::narrow(m.depth);
        }
    }
    if mv.pen {
        mv.t = 0.0;
        return true;
    }
    let mut t_min = 2.0;
    let mut hit: Option<usize> = None;
    for (i, &sk) in skip.iter().enumerate() {
        if sk {
            continue;
        }
        let b = box_at(g0, ps, i);
        let Some(t) = sweep_of(b, x0, y0, x0 + dx, y0 + dy, r, hh) else {
            continue;
        };
        if t >= t_min {
            continue;
        }
        if t == 0.0 {
            // touching at the start: a hit only when the move goes into the box
            contact_normal(mv, b, x0, y0, r, hh);
            if mv.nx * dx + mv.ny * dy >= 0.0 {
                continue;
            }
        }
        t_min = t;
        hit = Some(i);
    }
    let Some(hi) = hit else {
        j.x = x0 + dx;
        j.y = y0 + dy;
        return false;
    };
    contact_normal(
        mv,
        box_at(g0, ps, hi),
        x0 + dx * crate::math::narrow(t_min),
        y0 + dy * crate::math::narrow(t_min),
        r,
        hh,
    );
    let back = min(max(0.1, 0.1 / len), 1.0 / len) + 0.001;
    let t = max(crate::math::narrow(t_min - f64::from(back)), 0.0);
    j.x = x0 + dx * t;
    j.y = y0 + dy * t;
    mv.t = t;
    true
}

/// Whether the capsule, its radius grown by `infl`, overlaps a box at (x, y).
fn blocked(cap: Cap, g0: &Ground, ps: Shapes, x: Real, y: Real, infl: Real) -> bool {
    (0..=ps.boxes.len() + ps.polys.len()).any(|i| overlaps(box_at(g0, ps, i), x, y, cap.r, cap.hh, infl))
}

#[allow(clippy::too_many_arguments, reason = "the parameters")]
fn sweep_out(j: &mut JerryActor, mv: &mut Mv, cap: Cap, g0: &Ground, ps: Shapes, dx: Real, dy: Real) -> bool {
    let (x, y) = (j.x, j.y);
    move_comp(j, mv, cap, g0, ps, dx, dy, false);
    j.x != x || j.y != y || dx * dx + dy * dy <= 16e-8
}

/// ResolvePenetrationImpl (Projectile.resolvePenetration's rule).
#[allow(clippy::too_many_arguments, reason = "the parameters")]
fn resolve_penetration(
    j: &mut JerryActor,
    mv: &mut Mv,
    cap: Cap,
    g0: &Ground,
    ps: Shapes,
    ax: Real,
    ay: Real,
    dx: Real,
    dy: Real,
) -> bool {
    if ax == 0.0 && ay == 0.0 {
        return false;
    }
    if !blocked(cap, g0, ps, j.x + ax, j.y + ay, 0.1) {
        j.x += ax;
        j.y += ay;
        return true;
    }
    let mut moved = sweep_out(j, mv, cap, g0, ps, ax, ay);
    if !moved && mv.pen {
        let k = (if mv.depth > 0.0 { mv.depth } else { 0.125 }) + 0.125;
        let (sx, sy) = (mv.nx * k, mv.ny * k);
        if (sx != ax || sy != ay) && (ax + sx != 0.0 || ay + sy != 0.0) {
            moved = sweep_out(j, mv, cap, g0, ps, ax + sx, ay + sy);
        }
    }
    if !moved && (dx != 0.0 || dy != 0.0) {
        moved = sweep_out(j, mv, cap, g0, ps, ax + dx, ay + dy);
        if !moved && dx * ax + dy * ay > 0.0 {
            moved = sweep_out(j, mv, cap, g0, ps, dx, dy);
        }
    }
    moved
}

/// SafeMoveUpdatedComponent (Projectile.safeMove's rule): `mv` holds the hit.
#[allow(clippy::too_many_arguments, reason = "the parameters")]
fn safe_move(j: &mut JerryActor, mv: &mut Mv, cap: Cap, g0: &Ground, ps: Shapes, dx: Real, dy: Real) -> bool {
    let hit = move_comp(j, mv, cap, g0, ps, dx, dy, true);
    if !hit || !mv.pen {
        return hit;
    }
    let (nx, ny, depth) = (mv.nx, mv.ny, mv.depth);
    let k = (if depth > 0.0 { depth } else { 0.125 }) + 0.125;
    if resolve_penetration(j, mv, cap, g0, ps, nx * k, ny * k, dx, dy) {
        return move_comp(j, mv, cap, g0, ps, dx, dy, false);
    }
    mv.t = 0.0;
    mv.nx = nx;
    mv.ny = ny;
    mv.pen = true;
    mv.depth = depth;
    true
}

/// LimitVelocity (142996e60): GetClampedToMaxSize(MaxSpeed) when MaxSpeed > 0.
fn limit_velocity(j: &mut JerryActor) {
    (j.vx, j.vy) = clamp_speed(j, j.vx, j.vy);
}

/// GetClampedToMaxSize(MaxSpeed) of (0, x, y) when MaxSpeed > 0 (the InvSqrt scale).
fn clamp_speed(j: &JerryActor, x: Real, y: Real) -> (Real, Real) {
    if j.max_speed.is_nan() || j.max_speed <= 0.0 {
        return (x, y);
    }
    let [_, cx, cy] = clamped_to_max_size([0.0, x, y], j.max_speed);
    (cx, cy)
}

/// ComputeVelocity: gravity (ProjectileGravityScale x the world's), then LimitVelocity to MaxSpeed.
fn compute_velocity(j: &JerryActor, vx: Real, vy: Real, dt: Real) -> (Real, Real) {
    clamp_speed(j, vx, vy - GRAVITY_Z * j.grav_scale * dt)
}

/// HandleImpact with bShouldBounce (Projectile.bounceImpact's rule: ComputeBounceResult, then OnProjectileBounce, the
/// class's OnBounce, with the velocity it left; MinFrictionFraction 0, no bBounceAngleAffectsFriction, stop threshold
/// 0), then Velocity = LimitVelocity(Velocity) (Projectile.bounceImpact's order).
fn bounce_impact(w: &mut World, id: ActorId, nx: Real, ny: Real) {
    let j = jm(w, id);
    let vn = j.vx * nx + j.vy * ny;
    if vn <= 0.0 {
        let (tx, ty) = (j.vx - vn * nx, j.vy - vn * ny);
        let mut fr = j.friction;
        if j.sliding {
            let q = -vn / size([0.0, tx, ty]);
            // FMath::Clamp of a NaN gives the upper bound
            fr *= if q < 0.0 {
                0.0
            } else if q < 1.0 {
                q
            } else {
                1.0
            };
        }
        let keep = min(max(1.0 - fr, 0.0), 1.0);
        let b = max(j.bounciness, 0.0);
        j.vx = tx * keep - b * vn * nx;
        j.vy = ty * keep - b * vn * ny;
    }
    on_bounce(w, id);
    limit_velocity(jm(w, id));
}

/// HandleSliding (Projectile.slideMove's rule): the time still left, -1 when its own hit ended the loop.
#[allow(clippy::too_many_arguments, reason = "the parameters")]
fn slide_move(
    w: &mut World,
    id: ActorId,
    mv: &mut Mv,
    cap: Cap,
    g0: &Ground,
    ps: Shapes,
    nx: Real,
    ny: Real,
    sub: Real,
) -> Real {
    let j = jm(w, id);
    let (dx, dy) = (j.vx * sub, j.vy * sub);
    if safe_move(j, mv, cap, g0, ps, dx, dy) {
        let (ht, pen) = (mv.t, mv.pen);
        bounce_impact(w, id, mv.nx, mv.ny);
        return if is_dead(w, id) || !jm(w, id).moving || pen { -1.0 } else { sub * (1.0 - ht) };
    }
    let j = jm(w, id);
    let (ax, ay) = compute_velocity(j, j.vx, j.vy, sub);
    let (fx, fy) = (ax - j.vx, ay - j.vy);
    let fnn = fx * nx + fy * ny;
    if fnn < 0.0 {
        let (wx, wy) = (j.vx + fx - fnn * nx, j.vy + fy - fnn * ny);
        let ws = size([0.0, wx, wy]);
        let fr = min(-fnn * j.friction, ws);
        // T - GetSafeNormal(T) x fr (HandleSliding 142993dd0)
        let [_, ux, uy] = safe_normal([0.0, wx, wy], SMALL_NUMBER);
        j.vx = wx - ux * fr;
        j.vy = wy - uy * fr;
    } else {
        j.vx = ax;
        j.vy = ay;
    }
    0.0
}

/// UProjectileMovementComponent::TickComponent with bShouldBounce, one frame (Projectile.bounceMove's passes: up to 4,
/// the impacts, HandleDeflection and the slide).
fn bounce_move(w: &mut World, id: ActorId) {
    let cap = cap_of(w, id);
    let g0 = w.stage.ground;
    let mut mv = Mv::new();
    let mut remaining = DT;
    let mut impacts = 0;
    let mut it = 0;
    while remaining >= 1e-6 && it < 4 && jm(w, id).moving && !is_dead(w, id) {
        it += 1;
        let ignore = jm(w, id).ignore_platforms;
        let mut ps = platforms_of(w, ignore);
        let tick = remaining;
        remaining = 0.0;
        let j = jm(w, id);
        let (ox, oy) = (j.vx, j.vy);
        let (ax, ay) = compute_velocity(j, ox, oy, tick);
        let dx = ox * tick + (ax - ox) * 0.5 * tick;
        let dy = oy * tick + (ay - oy) * 0.5 * tick;
        j.vx = ax;
        j.vy = ay;
        if !safe_move(j, &mut mv, cap, &g0, ps.view(), dx, dy) {
            j.prev_hit_time = 1.0;
            j.sliding = false;
            continue;
        }
        let (ht, nx, ny, pen) = (mv.t, mv.nx, mv.ny, mv.pen);
        if ht > 1e-4 {
            let (ax, ay) = compute_velocity(j, ox, oy, tick * ht);
            j.vx = ax;
            j.vy = ay;
        } else {
            j.vx = ox;
            j.vy = oy;
        }
        impacts += 1;
        let mut sub = tick * (1.0 - ht);
        bounce_impact(w, id, nx, ny);
        if !jm(w, id).moving || is_dead(w, id) || pen {
            break;
        }
        let ignore = jm(w, id).ignore_platforms;
        ps = platforms_of(w, ignore);
        let j = jm(w, id);
        let repeat = j.prev_hit_time < 1.0 && ht <= 1e-4;
        let along = j.prev_nx * nx + j.prev_ny * ny;
        // GetSafeNormal(Velocity) . Normal <= 0.01 (HandleDeflection 142993540)
        let [_, ux, uy] = safe_normal([0.0, j.vx, j.vy], SMALL_NUMBER);
        j.sliding = (repeat && along >= 0.999845) || ux * nx + uy * ny <= 0.01;
        if j.sliding {
            if repeat && along <= 0.0 {
                j.vx = 0.0;
                j.vy = 0.0;
            } else {
                let vn = j.vx * nx + j.vy * ny;
                j.vx -= vn * nx;
                j.vy -= vn * ny;
            }
            if sub > 1e-4 {
                sub = slide_move(w, id, &mut mv, cap, &g0, ps.view(), nx, ny, sub);
            }
            if sub < 0.0 || !jm(w, id).moving || is_dead(w, id) {
                break;
            }
        }
        let j = jm(w, id);
        j.prev_hit_time = ht;
        j.prev_nx = nx;
        j.prev_ny = ny;
        if sub >= 1e-6 {
            remaining += sub;
            if impacts <= 1 {
                it -= 1;
            }
        }
    }
}

// ---------------------------------------------------------------- overlaps

/// Whether a capsule overlaps Jerry's Capsule or Box (BoxExtent Y 32, Z 70 at the root).
fn overlaps_capsule(w: &World, id: ActorId, x: Real, y: Real, r: Real, hh: Real) -> bool {
    let (Some(j), Some(a)) = (jerry_of(w, id), w.actors.actors.get(id)) else { return false };
    let Cap { r: jr, hh: jh } = cap_of(w, id);
    let dy = max((y - j.y).abs() - max(hh - r, 0.0) - max(jh - jr, 0.0), 0.0);
    if ((x - j.x) * (x - j.x) + dy * dy).sqrt() < r + jr {
        return true;
    }
    let Some(e) =
        actor_class_def(w, a.owner, a.cls).and_then(|d| obj(&d.components, "Box")).and_then(|b| obj(b, "BoxExtent"))
    else {
        return false;
    };
    let (ey, ez) = (pnum(e, "Y").unwrap_or(0.0), pnum(e, "Z").unwrap_or(0.0));
    let bx = Ground {
        left: f64::from(j.x - ey),
        right: f64::from(j.x + ey),
        bottom: f64::from(j.y - ez),
        top: f64::from(j.y + ez),
    };
    capsule_gap(x, y, r, hh, &bx) < f64::from(r)
}

/// ReceiveActorBeginOverlap @15743 with a fighter.
// Deviation: the build raises it inside the move that makes it (the fighter's or Jerry's); here the overlaps are tested
// as they stand at Jerry's tick, behind his move, and a begin is a fighter that was not inside at the last test.
fn fighter_overlaps(w: &mut World, id: ActorId) {
    for f in w.all_ids() {
        let (fx, fy, fr, fh) = (w.fighter(f).body.x, actor_y(w, f), cap_radius(w, f), capsule_half(w, f));
        let fid = w.fighter(f).id;
        let on = alive(w, f) && !is_dead(w, id) && overlaps_capsule(w, id, fx, fy, fr, fh);
        let Some(j) = jerry_mut(w, id) else { return };
        if !on {
            j.inside.shift_remove(&fid);
            continue;
        }
        if j.inside.contains(&fid) {
            continue;
        }
        j.inside.insert(fid);
        let (pick, ally, free) = (j.pick_up_gate, j.ally_pick_up_gate, j.following.is_none());
        if Some(f) == owner_of(w, id) {
            if pick {
                pickup(w, id); // @15820-15863
            }
        } else if ally && free {
            attach_to_ally(w, id, f); // @16530-16665 -> @8309 PickUpJerryMulticast -> @31595 (follower.rs)
        }
    }
}

/// Tom's pickup @15848: SetResetJerryServer(true) (ResetJerry @28318), then @940: the follower's Buff_Cheese removed
/// (Comp_State_Manager RemoveBuff @1128), K2_DestroyActor @1248 (the EndPlay reattaches him).
fn pickup(w: &mut World, id: ActorId) {
    let j = jm(w, id);
    j.reset_jerry = true;
    if let Some(f) = j.following {
        remove_buff(w, f, Some(name!("Buff_Cheese_C")), None);
    }
    destroy(w, id);
}

// ---------------------------------------------------------------- the tick

/// ReceiveTick @18482 (ahead of the movement's tick: an actor ticks before its components): the parent's ReceiveTick
/// first (SpawnedActorParentVelocity = GetVelocity() @8771), then with AimDownProjectileMovement the look-at of the
/// velocity as the rotation (pitch - 90, yaw), then rising: the Platform response Ignore; falling: the DoOnce @6019
/// (BaseKnockBack 1250, KnockBackScaling 14, UpdateHitBoxDamage) and the Platform response Block.
fn receive_tick(j: &mut JerryActor) {
    j.parent_vx = j.vx;
    j.parent_vy = j.vy;
    if !j.aim_down {
        return;
    }
    // FindLookAtRotation(location, location + Velocity) (MakeFromX, FMatrix::Rotator): (location + V) - location in
    // UE (X 0, Y = -x, Z = y); then MakeRotator(0, pitch - 90, yaw)
    let (ly, lz) = (-j.x, j.y);
    let (uy, uz) = ((ly + -j.vx) - ly, (lz + j.vy) - lz);
    let r = rot_from_x([0.0, uy, uz]);
    j.rot_pitch = r[0] + -90.0;
    j.rot_yaw = r[1];
    if j.vy >= 0.0 {
        j.ignore_platforms = true;
        return;
    }
    if !j.kb_once {
        j.kb_once = true;
        j.hit.base_knock_back = Some(1250.0);
        j.hit.knock_back_scaling = Some(14.0);
    }
    j.ignore_platforms = false;
}

/// The latent actions of the frame.
// Unverified: counted from the spawn frame's tick, behind the tick's own logic.
fn latent(w: &mut World, id: ActorId) {
    let j = jm(w, id);
    let mut proof = false;
    if j.pick_wait > 0 {
        j.pick_wait -= 1;
        if j.pick_wait == 0 {
            // @15: PickUpGate true, ProjectileGravityScale 8.
            j.pick_up_gate = true;
            j.grav_scale = 8.0;
        }
    }
    if j.dir_wait > 0 {
        j.dir_wait -= 1;
        if j.dir_wait == 0 {
            if j.attack_id == -1337 {
                // @171-312: HitBoxOn? false, UpdateHitbox, the proof buffs.
                proof = true;
            } else {
                // @389-616: KnockBackDirectionVector (UE Velocity.Y >= 0 ? 1 : -1, 1), UpdateHitBoxDamage.
                j.hit.knock_back_direction_vector =
                    Some(DataVec { x: Some(if -j.vx >= 0.0 { 1.0 } else { -1.0 }), y: Some(1.0), z: None });
            }
        }
    }
    if proof {
        hitbox_off(w, id);
        elem_spawn(w, ActorRef::Actor(id), BuffClass::BuffElementalIceproof, 0);
        elem_spawn(w, ActorRef::Actor(id), BuffClass::BuffElementalFireproof, 0);
    }
    let j = jm(w, id);
    if j.color_wait > 0 {
        j.color_wait -= 1;
        if j.color_wait == 0 {
            j.color_map = 0.0; // @72
        }
    }
    if j.dmg_wait > 0 {
        j.dmg_wait -= 1;
        if j.dmg_wait == 0 {
            j.damage_disabled = false; // the child's @15
        }
    }
}

/// HitShake (0.4 s): its end @31962 -> @22068: HitReact false, OnHitReactEnded, Comp_Follower FollowerHitReactEnd,
/// JerryMesh back at (0, 0, -80).
// Unverified: the timeline is advanced in the actor's tick.
const HIT_SHAKE_LEN: Real = 0.4;

fn hit_shake_tick(w: &mut World, id: ActorId) {
    let j = jm(w, id);
    if j.hit_shake_t < 0.0 {
        return;
    }
    j.hit_shake_t += DELTA_SECONDS;
    if j.hit_shake_t < HIT_SHAKE_LEN {
        return;
    }
    j.hit_shake_t = -1.0;
    j.hit_react = -1;
    follower_broadcast(w, ActorRef::Actor(id), ActorBuffSignal::FollowerHitReactEnd);
}

fn step(w: &mut World, id: ActorId) {
    let doe = flag(pp(w, id), "DestroyOnExitedArena?");
    // (the root's mobility: the WorldActor's)
    let movable = w.actors.actors.get(id).is_some_and(|a| a.mobility.is_none());
    let j = jm(w, id);
    j.age += 1;
    receive_tick(j);
    if j.following.is_none() && j.moving && movable {
        bounce_move(w, id);
    }
    if is_dead(w, id) {
        return;
    }
    fighter_overlaps(w, id);
    if is_dead(w, id) {
        return;
    }
    if jm(w, id).following.is_some() {
        follower_tick(w, id);
    } else {
        let j = jm(w, id);
        j.falling = j.vy != 0.0; // JerryAnimBlueprint's free branch: IsFalling = VerticalSpeed != 0 (Unverified: TryGetPawnOwner)
    }
    latent(w, id);
    hit_shake_tick(w, id);
    // SpawnedActorParent CheckBlastBoxTimer (0.1 s, looping, with DestroyOnExitedArena?).
    let j = jm(w, id);
    let (x, y, age) = (j.x, j.y, j.age);
    if blast_timer_exit(doe, &mut j.blast_at, &j.blast_bounds, crate::real(age - 1), x, y) {
        destroy(w, id);
    }
}

/// The hit box (HitBoxTags HitBox: ReflectDamageArea, the sphere at the root; with 2D hit boxes ReflectDamageArea2D,
/// a capsule along the depth: a circle in the play plane) against the enemies' hurt shapes, as a projectile's.
// Deviation: its begin overlaps with projectiles (DestroyProjectiles), items and other actors are
// `hit_box_actor_hits` behind the fighter pass, at Jerry's tick, in the play plane.
fn hits(w: &mut World, id: ActorId) {
    let me = ActorRef::Actor(id);
    let (Some(a), Some(j)) = (w.actors.actors.get(id), jerry_of(w, id)) else { return };
    if !j.hit_on || a.dead || j.following.is_some() {
        if let Some(j) = jerry_mut(w, id) {
            j.over.clear();
        }
        return;
    }
    let sphere = WorldSphere {
        x: j.x,
        y: j.y,
        z: if j.is_2d { None } else { Some(0.0) },
        r: j.hit_r * a.size_scale.unwrap_or(1.0),
        ..WorldSphere::default()
    };
    let (f, attack_id, hit) = (a.owner, j.attack_id, j.hit.clone());
    let team = w.fighter(f).team.team;
    for vic in match_fighters(w) {
        let vid = w.fighter(vic).id;
        let Some(j) = jerry_of(w, id) else { return };
        if w.fighter(vic).team.team == team || !alive(w, vic) || no_hit(w, vic) || j.hit_set.contains(&vid) {
            continue;
        }
        let Some(vp) = victim_pose(w, vic) else { continue };
        let met = hurt_hit(w, vic, std::slice::from_ref(&sphere), &vp).is_some();
        give_vbuf(w, vp);
        if !met {
            continue;
        }
        if projectile_blocks(w, &dealer(w, me), vic) {
            continue;
        }
        if invulnerable(w, vic) {
            let tick = w.flow.tick;
            attack_dodged(w, me, vic, attack_id, hit.base_knock_back.unwrap_or(0.0), tick, None, false);
            continue;
        }
        dodge_over(w, me, vic);
        if hit.hit_each_actor_only_once.unwrap_or(false)
            && let Some(j) = jerry_mut(w, id)
        {
            j.hit_set.insert(vid);
        }
        let rows = class_rows(w, me);
        let d = dealer(w, me);
        // the dealer's current packet (the knockback direction and the falling knockback are written as it flies)
        let Some(j) = jerry_of(w, id) else { return };
        let cur = j.hit.clone();
        let (px, py) = (j.x, j.y);
        if apply_projectile_hit(w, &d, vic, &cur, &rows, V2 { x: px, y: py }, None, &[], charge_pct(w, me)) {
            actor_hit_enemy(w, &d, vic);
        }
    }
    let Some(j) = jerry_mut(w, id) else { return };
    let (types, packet) = (j.types.clone(), j.hit.clone());
    let (mut over, mut once) = (std::mem::take(&mut j.over), std::mem::take(&mut j.once));
    let shape = HitShape::Sphere { x: j.x, y: j.y, r: sphere.r };
    hit_box_actor_hits(w, me, &packet, &types, Some(shape), &mut over, &mut once);
    if let Some(j) = jerry_mut(w, id) {
        j.over = over;
        j.once = once;
    }
}

/// ReceiveEndPlay @15719 (the parent's first: the world's list pass ends the buff children) -> @7302.
fn end(w: &mut World, id: ActorId) {
    let Some(j) = jerry_mut(w, id) else { return };
    if j.ended {
        return;
    }
    j.ended = true;
    let (reset, following, rung) = (j.reset_jerry, j.following.is_some(), j.was_rung_out);
    let Some(o) = owner_of(w, id) else { return };
    let has_state = tj_of(w, o).is_some();
    if let Some(s) = tj_mut(w, o)
        && s.jerry_actor == Some(ActorRef::Actor(id))
    {
        s.jerry_actor = None;
    }
    if reset || game_mode_cooldown_disabled(w, o) || (following && !rung) {
        // @7704 (the cast to BPI_TomAndJerry), then PS_TomJerryPoof00 (scale 2) and slingshot_reappear at the
        // Instigator's root GetSocketLocation(jerry_un_C_spine_a): the root is his CapsuleComponent, which has no
        // sockets, so its centre.
        if has_state && !reset_pass(w) {
            set_is_jerry_attached(w, o, true);
        }
        let (ox, oy) = (w.fighter(o).body.x, actor_y(w, o));
        emitter(w, id, name!("PS_TomJerryPoof00"), ox, oy, 2.0);
        sound_at(w, id, name!("sfx_cha_tomandjerry_slingshot_reappear"), ox, oy);
    } else if has_state && !reset_pass(w) {
        trigger_jerry_cooldown(w, o); // @8271
    }
}

/// ActivateObject: the ActivationConditions switch @15038-15718 (the first true one acts; 0 does nothing).
fn activate_object(w: &mut World, id: ActorId) {
    let c = jm(w, id).conds;
    if is_dead(w, id) || c[0] {
        return;
    }
    if c[1] {
        champagne_attack(w, id, false); // the gate ChampagneGate && CorkAmmo > 0 is champagne.rs's
    } else if c[2] {
        launch_rocket(w, id);
    } else if c[3] {
        champagne_shoot(w, id);
    } else if c[4] {
        champagne_attack(w, id, true);
    }
}

fn set_condition(w: &mut World, id: ActorId, n: i32, v: bool) {
    if let Some(j) = jerry_mut(w, id)
        && let Ok(i) = usize::try_from(n)
        && i < j.conds.len()
    {
        j.conds[i] = v;
    }
}

/// The PlayMontageMulticast proxy's OnNotifyBegin @29408: not being destroyed, IsAttackingStart, then the begin switch
/// @25231 (the Champagne and cork names are champagne.rs's).
fn notify_begin(w: &mut World, a: ActorRef, name: Name, _montage: Name) {
    let ActorRef::Actor(id) = a else { return };
    if is_dead(w, id) {
        return;
    }
    is_attacking_start(w, id);
    if champagne_notify(w, id, name, true) {
        return;
    }
    // TennisBall @26796, TennisRacket @26761 (and the ball's begin overlap @15868: tennis.rs tests it after each ball move).
    if tennis_window(w, id, name, true) {
        return;
    }
    if name == name!("RocketVisuals") {
        jm(w, id).rocket = true; // @26831
    }
}

/// OnNotifyEnd @30346: not being destroyed, IsAttackingEnd, then the end switch @24740.
fn notify_end(w: &mut World, a: ActorRef, name: Name, _montage: Name) {
    let ActorRef::Actor(id) = a else { return };
    if is_dead(w, id) {
        return;
    }
    is_attacking_end(w, id);
    if champagne_notify(w, id, name, false) {
        return;
    }
    // TennisBall @25071-25226 (hidden, the owner's TriggerAbilityActual(3), then @8387), TennisRacket @25036.
    if tennis_window(w, id, name, false) {
        return;
    }
    if name == name!("RocketVisuals") {
        jm(w, id).rocket = false; // @22749
    }
}

/// The root Capsule where a hit shape meets it (the world's actor damage through ReceiveAnyDamage).
fn pawn_body(w: &World, id: ActorId) -> Option<ExtraHurt> {
    if is_dead(w, id) {
        return None;
    }
    let j = jerry_of(w, id)?;
    let Cap { r, hh } = cap_of(w, id);
    let s = max(hh - r, 0.0);
    Some(ExtraHurt { x: j.x, y0: j.y - s, y1: j.y + s, r })
}

/// The bodies a trace meets: the Capsule (ECC_Pawn), ProjectileHurtBox (ProjectileDestructible, scale 1.2), Box.
#[allow(clippy::too_many_arguments, reason = "the parameters")]
fn add_body(
    out: &mut Vec<TraceBody>,
    id: ActorId,
    x: Real,
    y: Real,
    comp: Name,
    c: Option<&Props>,
    r: Real,
    half: Real,
    hx: Real,
    hy: Real,
) {
    let Some(info) = body_info(c, None).filter(|i| i.on) else { return };
    let s = max(half - r, 0.0);
    out.push(TraceBody {
        ax: x,
        ay: y - s,
        bx: x,
        by: y + s,
        hx,
        hy,
        r,
        z: Real::NAN,
        object_type: info.kind,
        profile: info.profile,
        component: comp,
        actor: Some(ActorRef::Actor(id)),
        platform: -1,
        blocks: info.blocks,
        poly: None,
        floor: None,
    });
}

fn bodies(w: &World, id: ActorId, out: &mut Vec<TraceBody>) {
    let (Some(a), Some(j)) = (w.actors.actors.get(id), jerry_of(w, id)) else { return };
    if a.dead {
        return;
    }
    let Some(d) = actor_class_def(w, a.owner, a.cls) else { return };
    let cs = &d.components;
    let cap = cap_of(w, id);
    let (x, y) = (j.x, j.y);
    add_body(out, id, x, y, name!("Capsule"), obj(cs, "Capsule"), cap.r, cap.hh, 0.0, 0.0);
    let g = a.size_scale.unwrap_or(1.0);
    let hb = obj(cs, "ProjectileHurtBox");
    let sc = hb.and_then(|h| obj(h, "RelativeScale3D")).and_then(|v| pnum(v, "X")).unwrap_or(1.0) * g;
    if let Some(h) = hb {
        let (hr, hh) = (pnum(h, "CapsuleRadius").unwrap_or(0.0) * sc, pnum(h, "CapsuleHalfHeight").unwrap_or(0.0) * sc);
        add_body(out, id, x, y, name!("ProjectileHurtBox"), Some(h), hr, hh, 0.0, 0.0);
    }
    if let Some(e) = obj(cs, "Box").and_then(|b| obj(b, "BoxExtent")) {
        let (ey, ez) = (pnum(e, "Y").unwrap_or(0.0) * g, pnum(e, "Z").unwrap_or(0.0) * g);
        add_body(out, id, x, y, name!("Box"), obj(cs, "Box"), 0.0, 0.0, ey, ez);
    }
}

/// The TempInvincibility child's Comp_Actor_Destructible values now (`DamageDisabled`).
fn destructible(w: &World, id: ActorId) -> Option<&Props> {
    static OFF: LazyLock<Props> =
        LazyLock::new(|| Props::from_iter([("DamageDisabled".to_string(), Value::Bool(false))]));
    static ON: LazyLock<Props> =
        LazyLock::new(|| Props::from_iter([("DamageDisabled".to_string(), Value::Bool(true))]));
    Some(if jerry_of(w, id)?.damage_disabled { &ON } else { &OFF })
}

/// The proxy's OnCompleted / OnBlendOut / OnInterrupted: pop_flows (nothing).
fn montage_event(_w: &mut World, _a: ActorRef, _e: MontageEvent, _m: Name) {}

/// The class's kind: Jerry is made by [`spawn_jerry`], so it has no `spawn`.
fn kind() -> WorldActorHooks {
    WorldActorHooks {
        // capsule_of, hits and bodies read the scale; the view draws JerryMesh at it
        size: Some(SizeHooks::default()),
        set_condition: Some(set_condition),
        activate: Some(activate_object),
        step: Some(step),
        hits: Some(hits),
        any_damage: Some(any_damage),
        pawn_body: Some(pawn_body),
        end: Some(end),
        bodies: Some(bodies),
        fighter_signal: Some(follower::follower_signal),
        ..WorldActorHooks::default()
    }
}

/// The kind's entry of `class` (`kind` with its follower and mesh hooks).
fn entry(class: WorldActorClass, hooks: WorldActorHooks) -> ActorEntry {
    ActorEntry {
        follower: Some(follower::JERRY_FOLLOWER),
        mesh: Some(MeshHooks {
            montage_event: Some(montage_event),
            notify_begin: Some(notify_begin),
            notify_end: Some(notify_end),
            ..MeshHooks::default()
        }),
        ..ActorEntry::new(ActorClass::World(class), ActorHooks::World(hooks))
    }
}

/// This file's registry entries (Jerry, his TempInvincibility child).
pub fn register(r: &mut Registry) -> Result<(), RegistryError> {
    r.actors.enter(JERRY, entry(WorldActorClass::JerrySpawnedActor, kind()))?;
    let child = ActorEntry {
        parents: &[JERRY],
        ..entry(
            WorldActorClass::JerrySpawnedActorTempInvincibility,
            WorldActorHooks { destructible: Some(destructible), ..kind() },
        )
    };
    r.actors.enter(JERRY_TEMP_INVINCIBLE, child)?;
    Ok(())
}

// ---------------------------------------------------------------- the actor API the other files call

const JERRY_N: Name = name!("Jerry_SpawnedActor_C");
const JERRY_TEMP_N: Name = name!("Jerry_SpawnedActor_TempInvincibility_C");

/// The JerryActor of the class `p` / `components` data at the spawn (the constructor).
#[allow(clippy::too_many_arguments, reason = "the constructor's parameters")]
fn construct(
    w: &World,
    f: FighterId,
    d: &ActorDef,
    x: Real,
    y: Real,
    vx: Real,
    vy: Real,
    attack_id: i32,
    max_speed: Real,
) -> JerryActor {
    static EMPTY: LazyLock<Props> = LazyLock::new(Props::new);
    let comps = &d.components;
    let mut hit = HitPacket::from_props(&w.names, obj(&d.p, "HitBoxDamage").unwrap_or(&EMPTY), None);
    if hit.knock_back_direction_vector.is_none() {
        hit.knock_back_direction_vector = Some(DataVec { x: Some(1.0), y: Some(1.0), z: None });
    }
    let is_2d = flag(&d.p, "Supports2DHitboxes") && data_of(w, f).def.use_2d;
    let hit_r = if is_2d {
        obj(comps, "ReflectDamageArea2D").and_then(|c| pnum(c, "CapsuleRadius"))
    } else {
        obj(comps, "ReflectDamageArea").and_then(|c| pnum(c, "SphereRadius"))
    }
    .unwrap_or(0.0);
    if hit_r == 0.0 {
        warn_text(w, "Jerry_SpawnedActor_C: no ReflectDamageArea size in the data");
    }
    let pm = obj(comps, "ProjectileMovement");
    let pmn = |k: &str| pm.and_then(|p| pnum(p, k));
    if !pm.is_some_and(|p| flag(p, "bShouldBounce"))
        || pmn("BounceVelocityStopSimulatingThreshold").is_some_and(|t| t != 0.0)
    {
        warn_text(
            w,
            "Jerry_SpawnedActor_C: a ProjectileMovement other than the bounce with no stop threshold is not ported",
        );
    }
    JerryActor {
        hit,
        hp: pnum(&d.p, "JerryHealthHP").unwrap_or(0.0),
        hit_on: flag(&d.p, "HitBoxOn?"),
        hit_r,
        types: hit_box_types(obj(comps, if is_2d { "ReflectDamageArea2D" } else { "ReflectDamageArea" })),
        is_2d,
        aim_down: flag(&d.p, "AimDownProjectileMovement"),
        moving: true,
        bounciness: pmn("Bounciness").unwrap_or(0.6),
        friction: pmn("Friction").unwrap_or(0.2),
        grav_scale: pmn("ProjectileGravityScale").unwrap_or(1.0),
        damage_disabled: obj(comps, "Comp_Actor_Destructible").is_some_and(|c| flag(c, "DamageDisabled")),
        // SetInitialMovementMulticast @20742: ProjectileMovement.Velocity and SpawnedActorParentVelocity.
        x,
        y,
        vx,
        vy,
        parent_vx: vx,
        parent_vy: vy,
        max_speed,
        pick_wait: to_int32(latent_frames(0.5)),
        dir_wait: to_int32(latent_frames(0.033)),
        attack_id,
        ..JerryActor::default()
    }
}

/// `spawnJerry`: a Jerry_SpawnedActor (or its TempInvincibility child) for `f` at (x, y) with the velocity (vx, vy), as
/// the callers' CompSpawnActor makes him (AttackID 0: the owner's attack count, Comp_Spawner @2444-2747; the
/// TempInvincibility spawns pass -1337); the new actor, or none. `max_speed`: SetInitialMovementMulticast's MaxSpeed (the
/// slingshot's 3500, SlingShot_Aim_02 @1387). Called by slingshot.rs, rocket.rs and kicked.rs.
#[allow(clippy::too_many_arguments, reason = "the parameters")]
pub fn spawn_jerry(
    w: &mut World,
    f: FighterId,
    x: Real,
    y: Real,
    vx: Real,
    vy: Real,
    temp_invincible: bool,
    max_speed: Real,
) -> Option<ActorId> {
    let (cls, class) = if temp_invincible {
        (JERRY_TEMP_N, WorldActorClass::JerrySpawnedActorTempInvincibility)
    } else {
        (JERRY_N, WorldActorClass::JerrySpawnedActor)
    };
    let Some(d) = actor_class_def(w, f, cls) else {
        warn_text(w, &format!("spawned actor not implemented ({})", w.names.str(cls)));
        return None;
    };
    let attack_id = if temp_invincible { -1337 } else { w.fighter(f).decay.attack_count };
    let mut s = construct(w, f, d, x, y, vx, vy, attack_id, max_speed);
    let (force, doe) = {
        let p = &actor_class_def(w, f, cls)?.p;
        (flag(p, "ForceSpawnPositionInsideBlastBox"), flag(p, "DestroyOnExitedArena?"))
    };
    // SpawnedActorParent's BeginPlay: the blast-box loop (NumberOfActorsAllowed, the oldest of the class in the owner's
    // SpawnedActors past the limit, comes with add_actor).
    let sb = spawn_box(w);
    let dead = blast_box_loop(&sb, &mut s.x, &mut s.y, None, force, &mut s.blast_bounds, doe);
    let charge = instigator_charge(w, f);
    let actor = new_world_actor(w, cls, class, f, dead, WorldActorState::JerryActor(s));
    let id = w.actors.actors.insert(actor);
    add_actor(w, id, charge, true); // its number, Comp_Spawner's SpawnedActors, GrantBuffsFromInstigator
    if let Some(t) = tj_mut(w, f) {
        t.jerry_actor = Some(ActorRef::Actor(id));
    }
    if dead {
        return Some(id);
    }
    // Jerry's BeginPlay: CorkAmmo = CorkStartingAmmo (champagne.rs), the Delay 0.5 (the constructor), JerryMesh visible,
    // SetIsJerryAttached(false) @14221, the relative rotation 0, the materials (the view's), OnBounce and HitByEnemy
    // bound, the Delay 0.033 (the constructor), the IsAttackingEnd broadcast.
    cork_begin_play(w, id);
    if tj_of(w, f).is_some() {
        set_is_jerry_attached(w, f, false);
    }
    is_attacking_end(w, id);
    // The child's: Delay 0.2 -> DamageDisabled false.
    if temp_invincible {
        jm(w, id).dmg_wait = to_int32(latent_frames(0.2));
    }
    after_spawned_actor(w, f, ActorRef::Actor(id)); // CompSpawnActor's HandleActorPostSpawn @12116, behind the BeginPlay
    Some(id)
}

/// `playMontage` (PlayMontageMulticast(Montage) @21037 on JerryMesh, through its proxy: the notify switches): true when
/// it started. Called by champagne.rs, kicked.rs, rocket.rs and tennis.rs.
pub fn play_montage(w: &mut World, j: ActorId, name: Name) -> bool {
    montage_play(w, ActorRef::Actor(j), name, 1.0, 0.0, true)
}

/// `activate`: BPI_SetActivationConditions rows 1..4 then BPI_ActivateObject (the switch @15038-15718).
/// `conditions[n - 1]` is condition n's value (row 0 is left as it was).
pub fn activate(w: &mut World, j: ActorId, conditions: &[bool]) {
    let s = jm(w, j);
    for n in 1..s.conds.len() {
        s.conds[n] = conditions.get(n - 1).copied().unwrap_or(false);
    }
    activate_object(w, j);
}

/// `attachTo`: Jerry taken by a fighter: Tom's pickup (the overlap @15848: ResetJerry, the actor ends, its EndPlay
/// reattaches him) or an ally's (follower.rs attach_to_ally). Called by the overlap and FishingLure's return
/// (fishing.rs).
pub fn attach_to(w: &mut World, j: ActorId, f: FighterId) {
    if is_dead(w, j) {
        return;
    }
    if Some(f) == owner_of(w, j) {
        pickup(w, j);
    } else {
        attach_to_ally(w, j, f);
    }
}
