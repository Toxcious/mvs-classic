//! Black Adam's force field (and `perks.rs`'s PerkForceField): C021_ForceField_C
//! ([`ForceField`], a world actor) and the Bubble perk's C021_ForceField_Perk_C ([`PerkForceField`], ForceField's
//! child that follows him), their begin hit, overlaps, weakening and end; the buffs Buff_C021_SlowReflectProjectile
//! (on a spawned actor, `ElemExt::SlowReflect`) and Buff_C021_SetHomingTowardsTarget (on him, `BuffExt::SetHoming`).
//!
//! C021_ForceField_C is the actor of C021_Special_ProjectileDestroyer_Montage's AnimNotify_ConditionalSpawner_C at
//! 0.2469 s (socket fml_un_C_pelvis_att, translation (0, -30, 65), condition C021NoBubblePerkCondition_C, not
//! attached); Buff_C021_SlowReflectProjectile_C a buff of another kit's projectile or thrown item;
//! Buff_C021_SetHomingTowardsTarget_C the ApplyBuff of C021_Special_ProjectileAttract_Montage at 0.1956 s (BuffSelf,
//! 1 s). Offsets are their ubergraphs'.
//!
//! C021_ForceField (a SpawnedActorParent child; the parent's TriggerAbilityOnSpawnIndex with TriggerAbilityIndex 1
//! is the owner's actor spawn ability at the spawn request; 'Die With Instigator' true: the World's teardown).
//! - ReceiveBeginPlay @3945: Health = MaxHealth (100), Update Color (the view's), SphereTraceMultiForObjects(the
//!   actor, the actor, Radius 500, [Pawn], ignoring the Instigator) @4152; for each hit @396 (no team test): the
//!   Instigator's BPI_IsFacingLeft (false without the interface) and its cast to BaseFighter (no hit without it);
//!   dY = (the actor - the hit actor).Y (UE); base = facing left ? (-dY, 0) : (dY, 0) @1348-1432; on the ground
//!   (the Instigator's CharacterMovement IsMovingOnGround) GetRotated2D(base, dY > 0 ? A : -A) with A = facing left
//!   ? -30 : 30 @1474-1608, in the air base itself @1794; the hit @1698-2840 (Damage 0, BaseKnockBack 2000,
//!   HitReaction 6, ArmorPierce, DestroyProjectiles, AttackDecayType 5, HitEachActorOnlyOnce, IgnoreKBI,
//!   PlayOnHitEffects false, ApplyHitPauseToDamageDealer false, the rest 0 / false); pfgDoDamage(null, the hit
//!   actor) @2937; a DoOnce @3322 posts sfx_cha_C021_barrier_hit at the first hit actor @3615. Then Timeline_0.Play
//!   @3273.
//! - Timeline_0 (0.15 s, linear 0 -> 1): Sphere's and PS_Shield's world scale = Lerp(0, Radius / 50, T) @4448-4595,
//!   then a DoOnce posts sfx_cha_C021_barrier_loop on the actor @280; finished @4309: Sphere's overlap events on,
//!   Delay(Lifetime 16) @4342 -> @15 Shrink And Destroy.
//! - Weaken(d) @4637: Health -= d; <= 0 -> Shrink And Destroy @78; else LastHitTime = GetServerWorldTimeSeconds @168
//!   and, d > 0, Shrink @239.
//! - Shrink And Destroy @4735: sfx_cha_C021_barrier_death at the Sphere @4785, Timeline_1.Play @4863 (0.2 s: Sphere
//!   = Lerp(Radius / 50, 0, T) @3731; finished: K2_DestroyActor @3879). No DoOnce: a second call posts again.
//! - Shrink @9269: Timeline_2.PlayFromStart (0.2 s: Sphere = Lerp(Radius / 50, Radius x 0.85 / 50, T) @9376-9616,
//!   the current Radius; finished @9302: Radius x= Shrink Percent Per Hit 0.85).
//! - ReceiveActorBeginOverlap @9245 -> Destroy On Enter @5169: the entrant a SpawnedActorParent (projectiles and
//!   items are); Blocked Projectile Attack Type = its ProjectileAttackType @5244; GetGameTimeSinceCreation > 0
//!   @5343; its owner's Comp_Actor_Destructible TeamIndex against the actor owner's @5551:
//!   - another team: PS_HitImpact_Directional at the entrant, RotatorFromAxisAndAngle((1, 0, 0), Get Signed Angle
//!     Between(the actor - the entrant, (0, 1, 0), (1, 0, 0))) @5771-5879, sfx_cha_C021_barrier_projectile_hit at the
//!     Sphere @5994, pfgDoDamage(null, the entrant, Damage 1, DestroyProjectiles, OnHitAudio the same event, ...)
//!     @7202, the entrant's K2_DestroyActor @7325;
//!   - the same team: only the exact class SideSpecialProjectile (C021_Homing_Projectile_C) @7735: the owner's
//!     Comp_ReflectProjectile.ReflectProjectile(the entrant, KnockBackDirectionVector = (-Velocity.Y, Velocity.Z))
//!     @7783-9176;
//!   - after either: (type 0 or 1) and GetServerWorldTimeSeconds - LastHitTime > 0.25 -> Weaken(34) @4715.
//! - ReceiveActorEndOverlap @9244: nothing.
//! - ReceiveAnyDamage @9663 (authority): the DamageCauser's BPI_GetProjectileAttackType (0 when it does not
//!   implement BPI_SpawnedActor, a fighter: @10026); type != 2 and the causer not in ActorsThatHit @9819-9946 ->
//!   ActorsThatHit += it @9956 and Weaken(34) @10006, whatever the damage. Nothing clears the set.
//!
//! Model. The actor sits in the World's actors; its step (after the fighters' and the projectiles' moves) is its
//! tick: the timelines first (Timeline_0, Played in BeginPlay inside the spawn, ticks in the spawn frame: a newly
//! spawned actor's tick functions, its components' among them, run in that frame's TG_NewlySpawned pass; a timeline
//! Played later in a step, Timeline_1 or Timeline_2 from Destroy On Enter, ticks from the next step), then the
//! Lifetime Delay, then the Sphere's overlaps at the scale the timelines left: begin detection against the list of
//! actors inside (as `actors::barrier`), with `sphere_overlap_actors`' gather for the object types the Sphere
//! overlaps (DamageDestructible, ProjectileDestructible, ItemDestructible), a projectile's HurtBox, an item's
//! Sphere, a buff's AdditionalHurtBox and Morty's C019_MortarAim Hurtbox (its DamageDestructible capsule overlaps the
//! HitBox channel, the Sphere's object type) among them. `sphere_scale` is the Sphere's world scale (the view's
//! PS_Shield). GetServerWorldTimeSeconds is the owner's world clock (its TimeSeconds, float32, no offset offline,
//! not dilated).
//! Unverified: the timelines' components tick in TG_PrePhysics, their order against the actor's own tick (the
//! Delay), against each other and against the projectiles' movement is not read (UTimelineComponent); they are taken
//! in the order Timeline_0, Timeline_1, Timeline_2, then the Delay, then the overlaps. That a component whose tick
//! Play enabled inside BeginPlay joins the spawn frame's TG_NewlySpawned pass is the 4.26 source's
//! FTickTaskLevel::AddTickFunction (bTickNewlySpawned), not read from the exe.
//! Deviation: the overlaps are tested once a frame in the actor's step: an item's move comes after it, so an item is
//! met where it was before its move of the frame; a projectile where its move left it (the engine updates the
//! overlaps inside each move). SetGenerateOverlapEvents(true) is taken to begin the overlaps already there.
//! Unverified: the Sphere's static mesh (/Engine/BasicShapes/Sphere) is taken as a sphere of radius 50 at scale 1
//! (its collision body is not read); the other side's response to GameTraceChannel3 is not tested.
//! Deviation: GetGameTimeSinceCreation > 0 is taken as a projectile moved in an earlier frame (age > 1) and an item
//! stepped in one (age > 0) or thrown: the actors' creation time is not kept.
//! Deviation: the Damage-1 pfgDoDamage on an enemy entrant is taken as its ReceiveAnyDamage and the light path's
//! PlayHitSound alone (the handler's HitByEnemy and the hit effects are not made: the K2_DestroyActor right behind it
//! ends the entrant; a World actors entrant without a destroy hook ends at the frame's list pass, not in the call).
//! The begin trace's non-fighter hits (another Pawn body: SeismicReactor_C's DamageBox) take the packet through
//! `damage_spawned_actor` (Damage 0: nothing is applied).
//! ReceiveAnyDamage is the kind's `any_damage`; the field's Damage-1 hit on a World actors entrant reaches the
//! entrant's (`world_actor_enters`). A spawned causer's type is its attack type (projectile, item, mortar aim) or its
//! class data's ProjectileAttackType (a World actors entry). TraceCollision (sphere 46 under Sphere, BlockTrace,
//! blocks ProjectileGroundTrace) is entered as a body (`field_bodies`) for the traces that ask for it.
//!
//! C021_ForceField_Perk_C (the Bubble perk's child, `perks.rs`). Buff_C021_Bubble has no graph:
//! C021BubblePerkCondition / C021NoBubblePerkCondition (pawn.rs) choose the down special's spawner,
//! C021_ForceField_Perk_C in place of C021_ForceField_C (both at 0.2469 s, translation (0, -30, 65)).
//! C021_ForceField_Perk ReceiveBeginPlay @74: the parent's ReceiveBeginPlay (C021_ForceField's whole begin: its trace
//! on Radius @4152, Timeline_0, the Lifetime Delay) and then K2_AttachToActor(GetOwner(), None, KeepWorld, KeepWorld,
//! KeepWorld, weld) @30: the field keeps the place and rotation it has relative to Black Adam's root at that moment
//! and rides him from then on. Its class data has its own values (Radius 350, MaxHealth 70, Lifetime 10).
//! The port: the relative place is taken at the spawn (the attach is in the same BeginPlay) against his actor
//! location (x, actor y) in his facing's frame; each step of the field, ahead of the parent's step, puts it back at
//! that place from his place and facing now, and its facing (the shield's yaw) is his.
//! Deviation: a turn of his is taken as his root's yaw flipping with `facing`, so the side offset mirrors at once;
//! the place is set in the field's own step, not in his move: a trace or overlap of another actor between his move
//! and the field's step finds the field where his last move left it.
//! Deviation: the weld and the attach's effect on the Sphere's overlaps while he moves (an attached component's
//! sweep) are not modelled: the overlaps are tested at the step's place, as the unattached field's.
//! Unverified (stock UE attach rule, not a native read): KeepWorld scale keeps the field's scale relative to his root,
//! so a change of his scale while it is up (Buff_Enlarge 1.4, the FFA catch-up buff) would scale the sphere and its
//! offset in the build; here both stay as at the spawn.
//! SpawnedActorParent's blast-box timer (CheckBlastBoxExit every 0.1 s, DestroyOnExitedArena? true in its data; the
//! BeginPlay loop's bounds): the unattached field never moves, this one leaves the box with him (a ring-out). Taken
//! as C021_Bolt's: checked in the actor's step, the spawn frame not counted. Die With Instigator ends it at his reset.

#![allow(clippy::neg_cmp_op_on_partial_ord)] // NaN-keeping negated tests

use mvs_data::value::Props;

use crate::Real;
use crate::State;
use crate::actors::world_actor::{WorldActorState, actor_entry, actor_p, warn, world_actor_place_of};
use crate::actors::{ActorClass, ActorId, ActorRef, WorldActorClass};
use crate::combat::HitPacket;
use crate::combat::buff_class::BuffClass;
use crate::combat::buffs::{BuffExt, BuffRef, buff_mut};
use crate::combat::elem::{
    ElemExt, charge_pct, elem_buff, elem_buff_mut, remove_actor_buff, set_homing, spawn_actor_buff,
};
use crate::combat::hits::{DamageCall, actor_hit_enemy, ally_hit, apply_projectile_hit, dealer, projectile_blocks};
use crate::combat::packet::{DataEnum, DataVec};
use crate::combat::reflect::{can_be_reflected, reflect_projectile};
use crate::combat::spawned_hits::{damage_actor_call, damage_spawned_actor_call, projectile_any_damage};
use crate::consts::DELTA_SECONDS;
use crate::events::Event;
use crate::fighter::common::{actor_def, actor_is_a, buff_def, latent_frames, lookup_owner, owner_team};
use crate::fighter::control::{attack_dodged, dodge_over};
use crate::fighter::match_side::SpawnRequest;
use crate::fighter::moves::V2;
use crate::fighter::traces::{
    OBJ_DAMAGE_DESTRUCTIBLE, OBJ_ITEM_DESTRUCTIBLE, OBJ_PAWN, OBJ_PROJECTILE_DESTRUCTIBLE, TraceBody, TraceHit,
    body_info, ecc, sphere_overlap_actors, sphere_trace_multi_for_objects,
};
use crate::fighter::{FighterId, actor_y, invulnerable, no_hit};
use crate::math::ue::{DEG_TO_RAD, KINDA_SMALL_NUMBER, deg_acos, normal, safe_normal2d, sin_cos};
use crate::math::{max, min, to_int32};
use crate::name;
use crate::names::Name;
use crate::registry::entries::{ActorBuffHooks, ActorEntry, ActorHooks, BuffHooks, SizeHooks, WorldActorHooks};
use crate::registry::props::{flag, num, obj, text};
use crate::registry::signals::{ActorBuffArg, ActorBuffSignal};
use crate::registry::{BuffEntry, Registry, RegistryError};
use crate::world::World;
use crate::world::blast::{BlastBounds, blast_box_loop, check_blast_box_exit, new_blast_bounds, spawn_box};
use crate::world::pawns::is_match_fighter;
use crate::world::spawn_spot::spawn_place;
use crate::world::spawning::request_props;

const FIELD: &str = "C021_ForceField_C";
const PERK_FIELD: &str = "C021_ForceField_Perk_C";
const SLOW_REFLECT: &str = "Buff_C021_SlowReflectProjectile_C";
const SET_HOMING: &str = "Buff_C021_SetHomingTowardsTarget_C";
/// The field's two pfgDoDamage calls (@2937 on a begin trace hit, @7202 on an enemy entrant) pass empty
/// ElementalBuffsParam and OnHitBuffsParam (`struct{[]}`): the call's rows are none on every read, never the
/// handler's own (the field's packets are not its HitBoxDamage).
const NO_ROWS: DamageCall<'static> = DamageCall { hit_box: false, comp: None, clank: false, rows: Some(&[]) };
/// The Sphere mesh's radius at scale 1 (Radius / 50 is its scale).
const MESH_RADIUS: Real = 50.0;

/// The timelines' lengths (C021_ForceField.json Timeline_<n>_Template, CurveFloat_0 / _0_1 / _1_2: linear 0 -> 1).
fn t0_len() -> Real {
    0.15
}
fn t1_len() -> Real {
    0.2
}
fn t2_len() -> Real {
    0.2
}

fn overlap_types() -> [Real; 3] {
    [crate::real(OBJ_DAMAGE_DESTRUCTIBLE), crate::real(OBJ_PROJECTILE_DESTRUCTIBLE), crate::real(OBJ_ITEM_DESTRUCTIBLE)]
}

/// A timeline's play state: its position in seconds (float32) and whether it plays.
#[derive(State, Copy, Debug, Default, PartialEq)]
pub struct Timeline {
    pub pos: Real,
    pub on: bool,
}

/// `ForceField`: C021_ForceField_C's state. For the drawing: (`x`, `y`) the actor (the Sphere's centre),
/// `sphere_scale` the Sphere's and PS_Shield's world scale, `facing` the shield's yaw; the owner's Team colour.
#[derive(State, Debug, Default)]
pub struct ForceField {
    pub attack_id: i32,
    /// HitBoxDamage (the class data's).
    pub hit: HitPacket,
    pub x: Real,
    pub y: Real,
    pub facing: Real,
    /// The actor's steps, the spawn frame's included.
    pub age: i32,
    /// Health.
    pub health: Real,
    /// Radius (float32).
    pub radius: Real,
    /// LastHitTime: GetServerWorldTimeSeconds of the last Weaken that left it alive.
    pub last_hit_time: Real,
    /// Sphere's world scale (the template's 1 until Timeline_0's first update).
    pub sphere_scale: Real,
    /// Sphere's GenerateOverlapEvents (Timeline_0's end).
    pub overlaps_on: bool,
    /// The DoOnce of sfx_cha_C021_barrier_loop.
    pub loop_posted: bool,
    /// The Lifetime Delay's ticks left after the current one (0: none pending).
    pub life_left: i32,
    pub t0: Timeline,
    pub t1: Timeline,
    pub t2: Timeline,
    /// The actors overlapping the Sphere, in the order they began.
    pub inside: Vec<ActorRef>,
    /// The fighters (ids) the begin's hit landed on (HitEachActorOnlyOnce).
    pub hit_set: Vec<i32>,
    /// ActorsThatHit (ReceiveAnyDamage's set of causers).
    pub actors_that_hit: Vec<ActorRef>,
}

/// `PerkForceField extends ForceField`: C021_ForceField_Perk_C's state.
#[derive(State, Debug)]
pub struct PerkForceField {
    pub base: ForceField,
    /// The attach's relative place: sim x in the owner's facing frame, sim y above his actor location.
    pub rel_x: Real,
    pub rel_y: Real,
    /// Its BlastBoxBoundsMin / Max.
    pub blast_bounds: BlastBounds,
    /// CheckBlastBoxTimer's next expiry, in seconds of counted deltas.
    pub blast_at: Real,
}

impl Default for PerkForceField {
    /// The field initial values.
    fn default() -> Self {
        PerkForceField {
            base: ForceField::default(),
            rel_x: 0.0,
            rel_y: 0.0,
            blast_bounds: new_blast_bounds(),
            blast_at: 0.1,
        }
    }
}

fn ff_of(w: &World, id: ActorId) -> Option<&ForceField> {
    match &w.actors.actors.get(id)?.state {
        WorldActorState::ForceField(s) => Some(s),
        WorldActorState::PerkForceField(s) => Some(&s.base),
        _ => None,
    }
}

fn ff_mut(w: &mut World, id: ActorId) -> Option<&mut ForceField> {
    match &mut w.actors.actors.get_mut(id)?.state {
        WorldActorState::ForceField(s) => Some(s),
        WorldActorState::PerkForceField(s) => Some(&mut s.base),
        _ => None,
    }
}

fn perk_mut(w: &mut World, id: ActorId) -> Option<&mut PerkForceField> {
    match &mut w.actors.actors.get_mut(id)?.state {
        WorldActorState::PerkForceField(s) => Some(s),
        _ => None,
    }
}

/// The place, facing and AttackID of a field, for the World's dealer reads (`combat::hits` dealer, the actor place
/// getters).
pub fn dealer_parts(s: &WorldActorState) -> Option<(Real, Real, Real, i32)> {
    match s {
        WorldActorState::ForceField(f) => Some((f.x, f.y, f.facing, f.attack_id)),
        WorldActorState::PerkForceField(p) => Some((p.base.x, p.base.y, p.base.facing, p.base.attack_id)),
        _ => None,
    }
}

/// The field's HitBoxDamage (`hit`), for the World's dealer reads.
pub fn hit_of(s: &WorldActorState) -> Option<&HitPacket> {
    match s {
        WorldActorState::ForceField(f) => Some(&f.hit),
        WorldActorState::PerkForceField(p) => Some(&p.base.hit),
        _ => None,
    }
}

/// A number of the class data the graph reads (no default: a missing one warns and reads 0).
fn pnum(p: &Props, key: &str) -> Real {
    num(p, key).unwrap_or_else(|| {
        warn("C021_ForceField data missing a number");
        0.0
    })
}

fn owner_of(w: &World, id: ActorId) -> Option<FighterId> {
    w.actors.actors.get(id).map(|a| a.owner)
}

fn dead_of(w: &World, id: ActorId) -> bool {
    w.actors.actors.get(id).is_none_or(|a| a.dead)
}

fn sound(w: &mut World, f: FighterId, ev: Name, x: Real, y: Real) {
    let fighter = w.fighter(f).id;
    w.events.push(Event::Sound { fighter, ev, x: Some(x), y: Some(y), hit: None, pooled: false, key: None });
}

fn lerp(a: Real, b: Real, t: Real) -> Real {
    a + (t * (b - a))
}

/// GetRotated2D(V, deg) in float32: FMath::SinCos of the radians, (C X - S Y, S X + C Y).
fn rotated_2d(x: Real, y: Real, deg: Real) -> (Real, Real) {
    let (s, c) = sin_cos(deg * DEG_TO_RAD);
    (((c * x) - (s * y)), ((s * x) + (c * y)))
}

/// Get Signed Angle Between(A, (0, 1, 0), (1, 0, 0)) for A = (0, uy, uz) in UE axes: DegAcos(Normal(A) . B) x
/// SignOfFloat((B x Normal(A)) . Axis), the cross's X being Normal(A).Z.
fn signed_angle(uy: Real, uz: Real) -> Real {
    let [_, ny, nz] = normal([0.0, uy, uz], KINDA_SMALL_NUMBER);
    let deg = deg_acos(ny);
    if nz > 0.0 {
        deg
    } else if nz < 0.0 {
        -deg
    } else {
        0.0
    }
}

/// The begin's packet (@1698-2840) with its KnockBackDirectionVector.
fn begin_packet(kx: Real, ky: Real) -> HitPacket {
    HitPacket {
        damage: Some(0.0),
        base_knock_back: Some(2000.0),
        knock_back_scaling: Some(0.0),
        attack_tag: Some(name!("None")),
        hit_reaction: Some(DataEnum::Value(6)),
        hit_pause_time: Some(0.0),
        hit_stun_mult: Some(0.0),
        directional_knockback: Some(false),
        velocity_instead_of_facing: Some(false),
        knock_back_mirror: Some(false),
        ignore_facing: Some(false),
        attack_id: Some(0),
        do_los_check: Some(false),
        force_knockback: Some(false),
        queue_hit_pause_until_poses: Some(false),
        hit_only_one_fighter: Some(false),
        armor_pierce: Some(true),
        destroy_projectiles: Some(true),
        reflect_projectiles: Some(false),
        vacuum_knockback: Some(false),
        direct_knockback_towards_socket: Some(false),
        knockback_target_socket: Some(name!("None")),
        knockback_socket_offset: Some(DataVec { x: Some(0.0), y: Some(0.0), z: None }),
        play_on_hit_effects: Some(false),
        apply_hit_pause_to_damage_dealer: Some(false),
        attack_decay_type: Some(DataEnum::Value(5)),
        attack_decay_name: Some(name!("None")),
        additive_kb_mode: Some(DataEnum::Value(0)),
        kb_scaling_starting_dmg: Some(0.0),
        hit_each_actor_only_once: Some(true),
        account_for_velocity_with_vacuum_knockback: Some(false),
        ignore_kbi: Some(true),
        kbi_multiplier: Some(1.0),
        knock_back_direction_vector: Some(DataVec { x: Some(kx), y: Some(ky), z: None }),
        ..HitPacket::default()
    }
}

/// The Damage-1 hit Destroy On Enter builds for an enemy entrant @6072-7105 (the rest 0 / false / None).
fn enter_packet() -> HitPacket {
    HitPacket {
        damage: Some(1.0),
        base_knock_back: Some(0.0),
        knock_back_scaling: Some(0.0),
        knock_back_direction_vector: Some(DataVec { x: Some(0.0), y: Some(0.0), z: None }),
        attack_tag: Some(name!("None")),
        hit_reaction: Some(DataEnum::Value(0)),
        hit_pause_time: Some(0.0),
        hit_stun_mult: Some(0.0),
        on_hit_audio: Some(name!("sfx_cha_C021_barrier_projectile_hit")),
        armor_pierce: Some(false),
        destroy_projectiles: Some(true),
        reflect_projectiles: Some(false),
        play_on_hit_effects: Some(true),
        apply_hit_pause_to_damage_dealer: Some(true),
        attack_decay_type: Some(DataEnum::Value(0)),
        hit_each_actor_only_once: Some(true),
        account_for_velocity_with_vacuum_knockback: Some(true),
        ignore_kbi: Some(false),
        kbi_multiplier: Some(1.0),
        ..HitPacket::default()
    }
}

/// The begin's packet for a hit actor at sim x (@1348-1794).
fn begin_packet_at(w: &World, id: ActorId, x: Real) -> HitPacket {
    let (Some(s), Some(inst)) = (ff_of(w, id), owner_of(w, id)) else { return begin_packet(0.0, 0.0) };
    let left = crate::fighter::facing(w, inst) < 0.0;
    // dY in UE: (the actor - the victim).Y, UE Y = -sim x.
    let dy = (-s.x) - (-x);
    let bx = if left { -dy } else { dy };
    let a = if left { -30.0 } else { 30.0 };
    if crate::fighter::grounded(w, inst) {
        let (kx, ky) = rotated_2d(bx, 0.0, if dy > 0.0 { a } else { -a });
        begin_packet(kx, ky)
    } else {
        begin_packet(bx, 0.0)
    }
}

/// The begin's hit on one fighter of the trace (the seismic reactor's pattern).
fn begin_hit(w: &mut World, id: ActorId, vic: FighterId) {
    let me = ActorRef::Actor(id);
    let vid = w.fighter(vic).id;
    let Some(s) = ff_of(w, id) else { return };
    if s.hit_set.contains(&vid) {
        return;
    }
    let attack_id = s.attack_id;
    let Some(inst) = owner_of(w, id) else { return };
    let p = begin_packet_at(w, id, w.fighter(vic).body.x);
    if let Some(s) = ff_mut(w, id) {
        s.hit_set.push(vid);
    }
    if w.fighter(vic).team.team != w.fighter(inst).team.team {
        if !crate::combat::phases::no_hit_by(w, inst, vic) && !projectile_blocks(w, &dealer(w, me), vic) {
            if invulnerable(w, vic) {
                let tick = w.flow.tick;
                attack_dodged(w, me, vic, attack_id, 2000.0, tick, None, false);
            } else {
                dodge_over(w, me, vic);
                // pfgDoDamage(null HitBox, ...) @2937: HitLocation is the victim's capsule. The packet's X along the
                // actor's facing (FacingFlipOnSpawn: the Instigator's), as the graph's choice of base expects.
                let at = V2 { x: w.fighter(vic).body.x, y: actor_y(w, vic) };
                let d = dealer(w, me);
                if apply_projectile_hit(w, &d, vic, &p, &[], at, None, &[], charge_pct(w, me)) {
                    let d = dealer(w, me);
                    actor_hit_enemy(w, &d, vic);
                }
            }
        }
    } else if !no_hit(w, vic) && !invulnerable(w, vic) {
        let d = dealer(w, me);
        ally_hit(w, &d, vic, &p);
    }
}

/// ReceiveBeginPlay's trace and hits, then Timeline_0.Play.
fn field_begin(w: &mut World, id: ActorId) {
    let me = ActorRef::Actor(id);
    let (Some(s), Some(owner)) = (ff_of(w, id), owner_of(w, id)) else { return };
    let (x, y, r) = (s.x, s.y, s.radius);
    let mut hits: Vec<TraceHit> = Vec::new();
    sphere_trace_multi_for_objects(
        w,
        x,
        y,
        x,
        y,
        r,
        &[crate::real(OBJ_PAWN)],
        &[Some(ActorRef::Fighter(owner))],
        Some(me),
        &mut hits,
    );
    let mut posted = false;
    for h in hits {
        let Some(o) = h.actor else { continue };
        let vic = match o {
            ActorRef::Fighter(f) if is_match_fighter(w, f) => Some(f),
            _ => None,
        };
        if let Some(vic) = vic {
            begin_hit(w, id, vic);
        } else if let ActorRef::Projectile(pid) = o {
            // A spawned actor's Pawn body (SeismicReactor_C's DamageBox, one per actor: no TrackActorsHitByTag record
            // needed): pfgDoDamage on it (Damage 0, so no ApplyDamage; its HitByEnemy / HitByAlly alone).
            let px = w.actors.projectiles.get(pid).map_or(0.0, |p| p.x);
            let p = begin_packet_at(w, id, px);
            damage_spawned_actor_call(w, me, pid, &p, &NO_ROWS);
        } else {
            warn("C021_ForceField_C: the begin trace met a Pawn body of an actor that is no projectile (not hit)");
        }
        // The DoOnce @3322 is pushed at @641, once the Instigator's casts passed, and runs when the hit's flow pops:
        // the first trace hit of any kind posts it, at its HitActor (a fighter's root: its actor height).
        if !posted {
            posted = true;
            let ev = name!("sfx_cha_C021_barrier_hit");
            let (sx, sy) = match vic {
                Some(v) => (w.fighter(v).body.x, actor_y(w, v)),
                None => crate::actors::world_actor::actor_place(w, o).map_or((h.ix, h.iy), |(px, py, _)| (px, py)),
            };
            sound(w, owner, ev, sx, sy);
        }
    }
    if let Some(s) = ff_mut(w, id) {
        s.t0.pos = 0.0;
        s.t0.on = true;
    }
}

fn shrink_and_destroy(w: &mut World, id: ActorId) {
    let (Some(s), Some(owner)) = (ff_of(w, id), owner_of(w, id)) else { return };
    let (x, y) = (s.x, s.y);
    sound(w, owner, name!("sfx_cha_C021_barrier_death"), x, y);
    if let Some(s) = ff_mut(w, id) {
        s.t1.on = true; // Play: from where it stands
    }
}

fn weaken(w: &mut World, id: ActorId, d: Real) {
    let Some(owner) = owner_of(w, id) else { return };
    let now = w.fighter(owner).clock.time;
    let Some(s) = ff_mut(w, id) else { return };
    s.health -= d;
    if s.health <= 0.0 {
        shrink_and_destroy(w, id);
        return;
    }
    s.last_hit_time = now;
    if d > 0.0 {
        s.t2.pos = 0.0; // PlayFromStart
        s.t2.on = true;
    }
}

/// @7361: (type 0 or 1) and GetServerWorldTimeSeconds - LastHitTime > 0.25 -> Weaken(34) @4715.
fn weaken_after_enter(w: &mut World, id: ActorId, ty: i32) {
    let Some(owner) = owner_of(w, id) else { return };
    let Some(s) = ff_of(w, id) else { return };
    if (ty == 0 || ty == 1) && (w.fighter(owner).clock.time - s.last_hit_time) > 0.25 {
        weaken(w, id, 34.0);
    }
}

/// The entrant's place, dead flag, attack type, team and "created before this frame" (GetGameTimeSinceCreation > 0,
/// see the Deviation above; a mortar aim steps in its spawn frame, as a projectile moves) for a projectile, an item
/// or a mortar aim.
struct Entrant {
    x: Real,
    y: Real,
    vx: Real,
    vy: Real,
    dead: bool,
    ty: i32,
    team: i32,
    created: bool,
    cls: Name,
}

fn entrant(w: &World, o: ActorRef) -> Option<Entrant> {
    match o {
        ActorRef::Projectile(id) => {
            let p = w.actors.projectiles.get(id)?;
            Some(Entrant {
                x: p.x,
                y: p.y,
                vx: p.vx,
                vy: p.vy,
                dead: p.dead,
                ty: p.attack_type,
                team: owner_team(w, o),
                created: p.age > 1,
                cls: p.cls,
            })
        }
        ActorRef::Item(id) => {
            let it = w.actors.items.get(id)?;
            Some(Entrant {
                x: it.x,
                y: it.y,
                vx: it.vx,
                vy: it.vy,
                dead: it.dead,
                ty: crate::actors::item::item_attack_type(w, id),
                // (a stage item no one has picked up: no owner, its team the Destructible's default -1)
                team: it.destructible_team(w),
                created: it.age > 0 || it.thrown,
                cls: it.cls,
            })
        }
        ActorRef::Mortar(id) => {
            let m = w.actors.mortars.get(id)?;
            Some(Entrant {
                x: m.x,
                y: m.y,
                vx: 0.0,
                vy: 0.0,
                dead: m.dead,
                ty: m.attack_type,
                team: w.fighter(m.owner).team.team,
                created: m.age > 1,
                cls: m.cls,
            })
        }
        _ => None,
    }
}

/// The impact and the sound of an enemy entrant at (x, y).
fn enter_impact(w: &mut World, id: ActorId, owner: FighterId, x: Real, y: Real) {
    let Some(s) = ff_of(w, id) else { return };
    let (fx, fy) = (s.x, s.y);
    let ang = signed_angle((-fx) - (-x), fy - y);
    let fighter = w.fighter(owner).id;
    w.events.push(Event::ProjectileDeath {
        cls: name!("C021_ForceField_C"),
        fighter,
        vfx: Some(name!("PS_HitImpact_Directional")),
        x,
        y,
        ox: 0.0,
        rot: [0.0, 0.0, -ang],
        scale: 1.0,
        tint: None,
        to: None,
    });
    sound(w, owner, name!("sfx_cha_C021_barrier_projectile_hit"), fx, fy);
}

fn destroy_on_enter(w: &mut World, id: ActorId, ent: ActorRef) {
    let Some(owner) = owner_of(w, id) else { return };
    let Some(e) = entrant(w, ent) else {
        match ent {
            ActorRef::Actor(a) => world_actor_enters(w, id, a),
            ActorRef::Fighter(f) if is_match_fighter(w, f) => {}
            _ => warn("C021_ForceField_C: an overlap with an actor the port does not take as a SpawnedActorParent"),
        }
        return;
    };
    if !e.created || e.dead {
        return;
    }
    let me = ActorRef::Actor(id);
    if e.team != w.fighter(owner).team.team {
        enter_impact(w, id, owner, e.x, e.y);
        // pfgDoDamage(Damage 1, DestroyProjectiles @6652) @7202: the IsProjectile branch goes on to DamageAndLaunch
        // and the actor's ReceiveAnyDamage (the class's own, else ParentProjectilePhysics'), the field its
        // DamageCauser; then K2_DestroyActor @7325 whatever it did.
        match ent {
            ActorRef::Projectile(pid) => {
                let ct = num(actor_p(w, me), "ProjectileAttackType").map_or(0, to_int32);
                projectile_any_damage(w, pid, 1.0, ct, me);
                if w.actors.projectiles.get(pid).is_some_and(|p| !p.dead) {
                    crate::world::spawning::destroy_projectile(w, pid, false);
                }
            }
            ActorRef::Mortar(mid) => {
                // C019_MortarAim: no ReceiveAnyDamage and no health, and SpawnedActorParent's HitByEnemy handler does
                // nothing: the damage leaves nothing; K2_DestroyActor ends it (its EndPlay @11168 destroys a beam
                // component of the view's).
                if let Some(m) = w.actors.mortars.get_mut(mid) {
                    m.dead = true;
                }
            }
            ActorRef::Item(iid) => {
                crate::actors::item::item_receive_damage(w, iid, 1.0, false);
                if w.actors.items.get(iid).is_some_and(|i| !i.dead) {
                    crate::world::items::destroy_item(w, iid);
                }
            }
            _ => unreachable!("an entrant of another kind"),
        }
    } else {
        let side = obj(actor_p(w, me), "SideSpecialProjectile").and_then(|s| text(s, "AssetPathName"));
        let Some(side) = side else { return };
        let short = side.rsplit_once('.').map_or(side, |(_, b)| b);
        if short != w.names.str(e.cls) || matches!(ent, ActorRef::Mortar(_)) {
            return;
        }
        // KnockBackDirectionVector = (-Velocity.Y, Velocity.Z): sim (vx, vy). Reflect timing: behind. The World
        // actors step runs after the projectile's move and its reflect tick of the frame (the entrant is a
        // projectile: C021_Homing_Projectile_C).
        reflect_projectile(w, owner, ent, V2 { x: e.vx, y: e.vy }, None);
    }
    weaken_after_enter(w, id, e.ty);
}

/// Destroy On Enter for an actor of the World's actors: the cast to SpawnedActorParent @5169 (the class data's
/// parents; a class without them warns), ProjectileAttackType @5244 (the class data's, or SpawnedActorParent's CDO,
/// which leaves it unset: 0), GetGameTimeSinceCreation > 0 @5343 (spawned before this frame: the frame's actor
/// number), the owners' teams @5551. An entrant spawned inside the Sphere begins its overlap inside
/// FinishSpawningActor (right after BeginPlay), at creation time 0, and stays inside: never destroyed by it (the
/// blackadam vs harley trace's bomb). Another team: the impact and the sound as for a projectile, then
/// pfgDoDamage(null, the entrant, Damage 1, DestroyProjectiles) @7202: `damage_actor`, whose DamageAndLaunch runs
/// the class's ReceiveAnyDamage (HarleyBomb_C detonates, HarleyJackInTheBox_C explodes) unless its
/// Comp_Actor_Destructible has DamageDisabled (WW_ShieldActor_C / _U_Special_C, C016ProjectileBarrier_C, Rick's
/// portals: CanBeHitBy fails, no damage); a damageable class with no any-damage hook warns. Then its K2_DestroyActor
/// @7325 whatever the hit did (the kind's destroy, else dead: its end in the frame's list pass). The same team: only
/// the exact class SideSpecialProjectile is reflected (a projectile class): pop_flow, no Weaken.
/// pfgDoDamage's gates ahead of DamageAndLaunch are passed by every class that meets the Sphere: none has
/// IsInvulnerableToProjectiles, a reflecting handler or an InvulnerabilityCount, and the field's
/// BPI_CanProjectileClank (HitBoxOn? and CanProjectileClank, false in its data) rules the clank out @20004-20541.
// Deviation: the entrant's ReceiveEndPlay runs at the frame's list pass for a class without a destroy hook (inside
// the call in the build); a World actors entry spawned later in a frame than the field's step (by another actor's
// step) is taken as created before the next frame (the build: GetGameTimeSinceCreation 0 at its spawn, so never
// destroyed by this begin overlap).
fn world_actor_enters(w: &mut World, id: ActorId, a: ActorId) {
    let Some(owner) = owner_of(w, id) else { return };
    let Some(ent) = w.actors.actors.get(a) else { return };
    let (eo, ecls, eseq) = (ent.owner, ent.cls, ent.seq);
    let sap = actor_is_a(w, ActorRef::Fighter(eo), ecls, name!("SpawnedActorParent_C"));
    if sap.is_none() {
        warn("C021_ForceField_C: an entrant has no class parents in the data: not taken as a SpawnedActorParent");
    }
    if sap != Some(true) {
        return;
    }
    let er = ActorRef::Actor(a);
    let ty = num(actor_p(w, er), "ProjectileAttackType").map_or(0, to_int32);
    // Deviation: an entrant listed after the field that its own spawn-frame step moves into the Sphere is met on the
    // next frame and destroyed; the build's begin overlap fires inside that move at creation time 0 and skips it.
    if eseq >= w.scratch.frame_actor_seq || w.fighter(eo).team.team == w.fighter(owner).team.team {
        return;
    }
    let Some((x, y, _)) = world_actor_place_of(w, a) else {
        warn("C021_ForceField_C: an entrant has no location the port keeps (not taken)");
        return;
    };
    enter_impact(w, id, owner, x, y);
    let hooks = actor_entry(w, er).and_then(|e| match e.hooks {
        ActorHooks::World(h) => Some(h),
        _ => None,
    });
    let comps = actor_def(w, ActorRef::Fighter(eo), ecls).map(|d| &d.components);
    let flag_of = |c: &str, k: &str| comps.and_then(|cs| obj(cs, c)).and_then(|c| c.get(k)).and_then(|v| v.as_bool());
    let damageable = flag_of("Comp_Pawn_DamageManager", "CanTakeDamage") != Some(false)
        && flag_of("Comp_Actor_Destructible", "DamageDisabled") != Some(true);
    if damageable && hooks.is_none_or(|h| h.any_damage.is_none()) {
        warn("an entrant's ReceiveAnyDamage not ported (C021_ForceField's Damage-1 hit)");
    }
    // Unverified: BPI_GetRootCapsule (a BPI_Elemental_System cast @21107) is taken as answered with the entrant's
    // location.
    damage_actor_call(w, ActorRef::Actor(id), a, &enter_packet(), Some(V2 { x, y }), &NO_ROWS); // no HitBox (@7202 null)
    if w.actors.actors.get(a).is_some_and(|x| !x.dead) {
        match hooks.and_then(|h| h.destroy) {
            Some(d) => d(w, a),
            None => {
                if let Some(x) = w.actors.actors.get_mut(a) {
                    x.dead = true;
                }
            }
        }
    }
    weaken_after_enter(w, id, ty);
}

fn field_overlaps(w: &mut World, id: ActorId) {
    let Some(s) = ff_of(w, id) else { return };
    if !s.overlaps_on || dead_of(w, id) {
        return;
    }
    let (x, y) = (s.x, s.y);
    let r = MESH_RADIUS * s.sphere_scale;
    let now = if r > 0.0 {
        sphere_overlap_actors(w, x, y, r, &overlap_types(), &[Some(ActorRef::Actor(id))], -1.0)
    } else {
        Vec::new()
    };
    let Some(s) = ff_mut(w, id) else { return };
    s.inside.retain(|o| now.contains(o));
    for o in now {
        let Some(s) = ff_mut(w, id) else { return };
        if s.inside.contains(&o) {
            continue;
        }
        s.inside.push(o);
        destroy_on_enter(w, id, o);
        if dead_of(w, id) {
            return;
        }
    }
}

/// One timeline tick: the position + DeltaSeconds (float32), clamped at the length; true when it got there.
fn advance(t: &mut Timeline, len: Real) -> bool {
    t.pos += DELTA_SECONDS;
    if t.pos >= len {
        t.pos = len;
        t.on = false;
        return true;
    }
    false
}

fn field_step(w: &mut World, id: ActorId) {
    let Some(owner) = owner_of(w, id) else { return };
    let p = actor_p(w, ActorRef::Actor(id));
    let lifetime = pnum(p, "Lifetime");
    let k = pnum(p, "Shrink Percent Per Hit");
    let Some(s) = ff_mut(w, id) else { return };
    s.age += 1;
    let mut post_loop = None;
    if s.t0.on {
        let done = advance(&mut s.t0, t0_len());
        s.sphere_scale = lerp(0.0, s.radius / 50.0, s.t0.pos / t0_len());
        if !s.loop_posted {
            s.loop_posted = true;
            post_loop = Some((s.x, s.y));
        }
        if done {
            s.overlaps_on = true;
            // Delay(Lifetime) made in this frame counts this frame (a Delay made on an object ahead of its tick):
            // the countdown below takes this frame's tick off, so Shrink And Destroy runs latentFrames(Lifetime) - 1
            // frames on.
            s.life_left = to_int32(max(1.0, latent_frames(lifetime)));
        }
    }
    if let Some((x, y)) = post_loop {
        sound(w, owner, name!("sfx_cha_C021_barrier_loop"), x, y);
    }
    let Some(s) = ff_mut(w, id) else { return };
    if s.t1.on {
        let done = advance(&mut s.t1, t1_len());
        s.sphere_scale = lerp(s.radius / 50.0, 0.0, s.t1.pos / t1_len());
        if done {
            // K2_DestroyActor @3879
            if let Some(a) = w.actors.actors.get_mut(id) {
                a.dead = true;
            }
            return;
        }
    }
    if s.t2.on {
        let done = advance(&mut s.t2, t2_len());
        s.sphere_scale = lerp(s.radius / 50.0, (s.radius * k) / 50.0, s.t2.pos / t2_len());
        if done {
            s.radius *= k;
        }
    }
    if s.life_left > 0 {
        s.life_left -= 1;
        if s.life_left == 0 {
            shrink_and_destroy(w, id);
        }
    }
    field_overlaps(w, id);
}

/// The actor's bodies for the traces: Sphere (GameTraceChannel3, QueryOnly) and TraceCollision (sphere 46 under
/// Sphere, BlockTrace, blocks ProjectileGroundTrace), both at the actor and at Sphere's world scale; Capsule has no
/// collision.
fn field_bodies(w: &World, id: ActorId, out: &mut Vec<TraceBody>) {
    let (Some(a), Some(s)) = (w.actors.actors.get(id), ff_of(w, id)) else { return };
    let Some(comps) = actor_def(w, ActorRef::Fighter(a.owner), a.cls).map(|d| &d.components) else { return };
    let tc = obj(comps, "TraceCollision").and_then(|c| num(c, "SphereRadius")).unwrap_or(0.0);
    for (comp, cname, r) in [("Sphere", name!("Sphere"), MESH_RADIUS), ("TraceCollision", name!("TraceCollision"), tc)]
    {
        let Some(info) = body_info(obj(comps, comp), None) else { continue };
        if !info.on || !(r > 0.0) {
            continue;
        }
        out.push(TraceBody {
            ax: s.x,
            ay: s.y,
            bx: s.x,
            by: s.y,
            hx: 0.0,
            hy: 0.0,
            r: (r * s.sphere_scale.abs()),
            z: 0.0,
            object_type: info.kind,
            profile: info.profile,
            component: cname,
            actor: Some(ActorRef::Actor(id)),
            platform: -1,
            blocks: info.blocks,
            poly: None,
            floor: None,
        });
    }
}

/// ReceiveAnyDamage @9663.
fn field_any_damage(w: &mut World, id: ActorId, _dmg: Real, causer: ActorRef) {
    // A World actors causer whose class data leaves ProjectileAttackType unset has SpawnedActorParent's CDO value, 0;
    // a causer without BPI_SpawnedActor (a fighter) is 0 @10026.
    let ty = match causer {
        ActorRef::Fighter(_) => 0,
        ActorRef::Actor(a) if w.actors.actors.contains(a) => crate::actors::world_actor::instance_attack_type(w, a)
            .unwrap_or_else(|| num(actor_p(w, causer), "ProjectileAttackType").map_or(0, to_int32)),
        ActorRef::Projectile(p) => w.actors.projectiles.get(p).map_or(0, |p| p.attack_type),
        ActorRef::Item(i) => {
            if w.actors.items.contains(i) {
                crate::actors::item::item_attack_type(w, i)
            } else {
                0
            }
        }
        ActorRef::Mortar(m) => w.actors.mortars.get(m).map_or(0, |m| m.attack_type),
        _ => num(actor_p(w, causer), "ProjectileAttackType").map_or(0, to_int32),
    };
    let Some(s) = ff_mut(w, id) else { return };
    if ty == 2 || s.actors_that_hit.contains(&causer) {
        return;
    }
    s.actors_that_hit.push(causer);
    weaken(w, id, 34.0);
}

/// The state of either class from the request (its actor data is the owner's of `r.cls`).
fn new_field(w: &mut World, f: FighterId, r: &SpawnRequest) -> ForceField {
    let data = w.data.clone();
    let cls = w.names.str(r.cls);
    let a = crate::fighter::common::actor_data(w, &data, ActorRef::Fighter(f), r.cls)
        .unwrap_or_else(|| panic!("{cls}: no actor data for its spawn (the importer gives every spawned class)"));
    let np = request_props(w, f, r);
    let at = spawn_place(w, f, &np);
    ForceField {
        attack_id: r.attack_id,
        hit: HitPacket::from_props(&w.names, obj(&a.p, "HitBoxDamage").unwrap_or(&Props::new()), None),
        x: at.x,
        y: at.y,
        facing: crate::fighter::facing(w, f),
        health: pnum(&a.p, "MaxHealth"),
        radius: pnum(&a.p, "Radius"),
        sphere_scale: 1.0,
        ..ForceField::default()
    }
}

/// The kind's `spawn`: the state and ReceiveBeginPlay @3945's call of the parent's first, SpawnedActorParent's
/// blast-box loop @9247 on the place (DestroyOnExitedArena? the parent's true): outside the jump-out area the field is
/// destroyed in its spawn. Its begin (which needs the actor's id) is `field_spawned`. The field never moves, so its
/// CheckBlastBoxExit timer never finds it outside the bounds the loop stored: neither is kept.
// Deviation: a field destroyed at its spawn runs no begin here (as the perk field's, `perk_spawn`).
fn field_spawn(w: &mut World, f: FighterId, r: &SpawnRequest) -> (WorldActorState, bool) {
    let mut s = new_field(w, f, r);
    let p = actor_p_of(w, f, r.cls);
    let (force, doe) = (flag(p, "ForceSpawnPositionInsideBlastBox"), flag(p, "DestroyOnExitedArena?"));
    let mut bounds = new_blast_bounds();
    let dead = blast_box_loop(&spawn_box(w), &mut s.x, &mut s.y, None, force, &mut bounds, doe);
    (WorldActorState::ForceField(s), dead)
}

/// The rest of the kind's `spawn`: ReceiveBeginPlay.
fn field_spawned(w: &mut World, id: ActorId) {
    field_begin(w, id);
}

// ------------------------------------------------------------------ C021_ForceField_Perk

/// The perk field's `spawn`: the parent's BeginPlay blast-box loop on the place.
// Deviation: a field the loop finds outside the box (dead at the spawn) runs no begin here (the World runs no
// `spawned` part for a dead spawn); the begin's trace would hit all the same. No stage spawns it
// outside.
fn perk_spawn(w: &mut World, f: FighterId, r: &SpawnRequest) -> (WorldActorState, bool) {
    let base = new_field(w, f, r);
    let mut s = PerkForceField { base, ..PerkForceField::default() };
    let p = actor_p_of(w, f, r.cls);
    let (force, doe) = (flag(p, "ForceSpawnPositionInsideBlastBox"), flag(p, "DestroyOnExitedArena?"));
    let sb = spawn_box(w);
    let dead = blast_box_loop(&sb, &mut s.base.x, &mut s.base.y, None, force, &mut s.blast_bounds, doe);
    (WorldActorState::PerkForceField(s), dead)
}

fn actor_p_of(w: &World, f: FighterId, cls: Name) -> &Props {
    static NONE: std::sync::LazyLock<Props> = std::sync::LazyLock::new(Props::new);
    actor_def(w, ActorRef::Fighter(f), cls).map_or(&NONE, |d| &d.p)
}

/// The begin, then the attach's relative place.
fn perk_spawned(w: &mut World, id: ActorId) {
    field_begin(w, id);
    let Some(f) = owner_of(w, id) else { return };
    let (fx, fy, facing) = (w.fighter(f).body.x, actor_y(w, f), crate::fighter::facing(w, f));
    let Some(s) = perk_mut(w, id) else { return };
    s.rel_x = (s.base.x - fx) * facing;
    s.rel_y = s.base.y - fy;
}

fn perk_step(w: &mut World, id: ActorId) {
    let Some(f) = owner_of(w, id) else { return };
    let (fx, fy, facing) = (w.fighter(f).body.x, actor_y(w, f), crate::fighter::facing(w, f));
    let doe = flag(actor_p(w, ActorRef::Actor(id)), "DestroyOnExitedArena?");
    let Some(s) = perk_mut(w, id) else { return };
    // follow
    s.base.x = fx + s.rel_x * facing;
    s.base.y = fy + s.rel_y;
    s.base.facing = facing;
    if doe && crate::consts::clock(crate::real(s.base.age)) > f64::from(s.blast_at) {
        s.blast_at += 0.1;
        if check_blast_box_exit(s.base.x, s.base.y, &s.blast_bounds) {
            if let Some(a) = w.actors.actors.get_mut(id) {
                a.dead = true;
            }
            return;
        }
    }
    field_step(w, id);
}

// ------------------------------------------------------------------ Buff_C021_SlowReflectProjectile
//
// A BuffParentActor child on a spawned actor (the actor-side path of `combat::elem`). ReceiveBeginPlay @2658 ->
// @2310: the owner no SpawnedActorParent -> nothing (it idles: on a fighter too, no caller puts it there); its
// BPI_GetProjectileAttackType 1 (Heavy) or its Comp_Actor_Destructible.IsProjectile false -> K2_DestroyActor @2643
// (whose EndPlay below destroys the owner: HasBeenReflected is false); else IsProjectile = true @2583 and @1205:
// AdjustedCompDestructible, the binds (the owner's HitByEnemy -> OnHitByEnemyServer, its ReflectedDispatch ->
// OnReflectedServer), FindScale @1366, AdditionalHurtBox = AddComponent(sphere) with radius Scale +
// HurtboxAdditionalBuffer (150) @1608 attached to the root @1715, CreateVFX(Scale) (the view's).
// - OnHitByEnemyServer @1803: the attacker a BP_FighterCharacter with a Comp_ReflectProjectile: ReflectHit's
//   KnockBackDirectionVector = (n.X, -n.Y), n = Normal2D(the hit's), and ReflectProjectile(the owner, ReflectHit)
//   @2246.
// - OnReflectedServer @1787: HasBeenReflected = true, K2_DestroyActor @30.
// - ReceiveEndPlay @390: RemoveBuff(Buff_Slow30PercentProjectile) on the owner's state manager @628; not reflected
//   -> the owner's DestroySpawnedActor(false) @1163; the VFX and AdditionalHurtBox destroyed @796-1030.
// - TakenDamageModCondition: the attacker is a BP_FighterCharacter (every fighter): its TakenDamageMods (Damage
//   Multiply 0: the hit's damage to the actor is 0) apply (`combat::elem` actor_taken_damage).
// FindScale @0-1954: over the owner's SceneComponents, those of an object type in CheckedCollisionTypes: a sphere's
// GetScaledSphereRadius, a box's largest scaled extent, a capsule's scaled radius and half height; the largest.
// Deviation: the components' shapes are read from the data's fields (SphereRadius, BoxExtent, CapsuleRadius /
// CapsuleHalfHeight; the root Capsule the port's; a HurtBox without its radius the native 32 as Projectile's); a
// component with none of them and no mesh is skipped (warned); the world scale is the relative scales down the
// attach chain from the actor's root scale (bAbsoluteScale breaks it). C021_Homing_Projectile's HitBox2D (capsule
// 100 / 750) gives 750: a sphere of 900.
// Unverified: the order of OnHitByEnemyServer against the owner class's own HitByEnemy handler (the World's hit-by
// call, first) is the bind order, not read.
// Not ported: ReverseInsteadOfStop @3411 (no caller in the build).

const SHAPE_SKIP: [&str; 2] = ["StaticMesh", "SkeletalMesh"];

fn vec3(c: Option<&Props>, k: &str, d: Real) -> [Real; 3] {
    let v = c.and_then(|c| obj(c, k));
    [
        v.and_then(|v| num(v, "X")).unwrap_or(d),
        v.and_then(|v| num(v, "Y")).unwrap_or(d),
        v.and_then(|v| num(v, "Z")).unwrap_or(d),
    ]
}

fn world_scale(comps: &Props, name: &str, root: Real, depth: i32) -> [Real; 3] {
    let c = obj(comps, name);
    let rel = vec3(c, "RelativeScale3D", 1.0);
    let parent = c.and_then(|c| text(c, "AttachParent"));
    if c.is_some_and(|c| crate::actors::projectile::truthy(c.get("bAbsoluteScale"))) {
        return rel;
    }
    let Some(parent) = parent.filter(|p| obj(comps, p).is_some() && depth <= 8) else {
        return rel.map(|v| v * root);
    };
    let ps = world_scale(comps, parent, root, depth + 1);
    [(rel[0] * ps[0]), (rel[1] * ps[1]), (rel[2] * ps[2])]
}

fn find_scale(w: &World, a: ActorRef, d: Option<&Props>) -> Real {
    let (cls, scale, cap) = match a {
        ActorRef::Projectile(id) => {
            let Some(p) = w.actors.projectiles.get(id) else { return 0.0 };
            (
                p.cls,
                p.scale,
                (crate::actors::projectile::capsule_radius(p), crate::actors::projectile::capsule_half_height(p)),
            )
        }
        ActorRef::Item(id) => {
            let Some(i) = w.actors.items.get(id) else { return 0.0 };
            (i.cls, i.scale, (i.capsule_radius(), i.capsule_half_height()))
        }
        _ => return 0.0,
    };
    let Some(comps) = actor_def(w, ActorRef::Fighter(lookup_owner(w, a)), cls).map(|x| &x.components) else {
        return 0.0;
    };
    let types: Vec<i32> = d
        .and_then(|d| d.get("CheckedCollisionTypes"))
        .and_then(|v| v.as_arr())
        .map(|l| l.iter().filter_map(|n| n.as_str().and_then(ecc)).collect())
        .unwrap_or_default();
    let mut m: Real = 0.0;
    for (name, c) in comps.iter() {
        let Some(c) = c.as_obj() else { continue };
        let info = if c.get("BodyInstance").is_some() { body_info(Some(c), None) } else { None };
        let Some(info) = info else { continue };
        if !types.contains(&info.kind) || SHAPE_SKIP.iter().any(|k| c.get(*k).is_some()) {
            continue;
        }
        let s = world_scale(comps, name, scale, 0);
        let mn = min(min(s[0].abs(), s[1].abs()), s[2].abs());
        if name == "Capsule" && c.get("AttachParent").is_none() {
            m = max(max(m, cap.0), cap.1);
        } else if c.get("SphereRadius").is_some() || name == "HurtBox" {
            m = max(m, num(c, "SphereRadius").unwrap_or(32.0) * mn);
        } else if c.get("BoxExtent").is_some() {
            let e = vec3(Some(c), "BoxExtent", 32.0);
            m = max(max(max(m, e[0] * s[0]), e[1] * s[1]), e[2] * s[2]);
        } else if c.get("CapsuleRadius").is_some() || c.get("CapsuleHalfHeight").is_some() {
            let r = num(c, "CapsuleRadius").unwrap_or(22.0);
            let h = num(c, "CapsuleHalfHeight").unwrap_or(44.0);
            m = max(max(m, r * mn), h * mn);
        } else {
            warn("Buff_C021_SlowReflectProjectile_C: FindScale: a component has no shape the port reads (skipped)");
        }
    }
    m
}

/// The Normal2D node (Buff_C021_SlowReflectProjectile @1968): zero at a squared size of 1e-8 or under, else times
/// InvSqrt (no unit-length shortcut).
fn normal_2d(x: Real, y: Real) -> (Real, Real) {
    let [nx, ny] = safe_normal2d([x, y]);
    (nx, ny)
}

fn slow_reflect_def(w: &World, a: ActorRef) -> Option<&Props> {
    buff_def(w, ActorRef::Fighter(lookup_owner(w, a)), name!("Buff_C021_SlowReflectProjectile_C"))
}

fn slow_reflect_begin(w: &mut World, a: ActorRef, i: u32, _from: Option<FighterId>) -> bool {
    // the failed cast: it idles
    let (dead, ty, is_projectile) = match a {
        ActorRef::Projectile(id) => {
            let Some(p) = w.actors.projectiles.get(id) else { return false };
            let isp = actor_def(w, ActorRef::Fighter(lookup_owner(w, a)), p.cls)
                .and_then(|d| obj(&d.components, "Comp_Actor_Destructible"))
                .is_some_and(|c| crate::actors::projectile::truthy(c.get("IsProjectile")));
            (p.dead, p.attack_type, isp)
        }
        ActorRef::Item(id) => {
            let Some(it) = w.actors.items.get(id) else { return false };
            let dead = it.dead;
            (dead, crate::actors::item::item_attack_type(w, id), crate::actors::item::item_is_projectile(w, id))
        }
        _ => return false,
    };
    if dead {
        return true;
    }
    if ty == 1 || !is_projectile {
        return true;
    }
    let d = slow_reflect_def(w, a);
    let buffer = d.and_then(|d| num(d, "HurtboxAdditionalBuffer")).unwrap_or(0.0);
    let r = find_scale(w, a, d) + buffer;
    if let Some(b) = elem_buff_mut(w, a, i) {
        b.ext = ElemExt::SlowReflect { r, refl: false };
    }
    false
}

fn slow_reflect_hurt_sphere(w: &World, a: ActorRef, i: u32) -> Real {
    match elem_buff(w, a, i).map(|b| b.ext) {
        Some(ElemExt::SlowReflect { r, .. }) => r,
        _ => 0.0,
    }
}

fn slow_reflect_taken(_w: &World, _a: ActorRef, _i: u32, _att: FighterId) -> bool {
    true
}

fn slow_reflect_on(w: &mut World, a: ActorRef, i: u32, s: ActorBuffSignal, arg: Option<&ActorBuffArg>) -> bool {
    if !matches!(elem_buff(w, a, i).map(|b| b.ext), Some(ElemExt::SlowReflect { .. })) {
        return false; // not bound
    }
    if s == ActorBuffSignal::Reflected {
        if let Some(b) = elem_buff_mut(w, a, i)
            && let ElemExt::SlowReflect { refl, .. } = &mut b.ext
        {
            *refl = true;
        }
        return true;
    }
    if s == ActorBuffSignal::HitByEnemy
        && let Some(arg) = arg
        && matches!(a, ActorRef::Projectile(_) | ActorRef::Item(_))
    {
        let (nx, ny) = normal_2d(arg.kx, arg.ky);
        // Reflect timing: the signal comes from the World's melee tests. From the fighter pass (overlaps): ahead of
        // the actor's tick of the frame. From the projectile hit test: behind for a projectile (its reflect tick ran
        // with its move), ahead for an item (the items move after it).
        reflect_projectile(w, arg.by, a, V2 { x: nx, y: if ny == 0.0 { 0.0 } else { -ny } }, None);
    }
    false
}

fn slow_reflect_end(w: &mut World, a: ActorRef, i: u32) {
    let refl = matches!(elem_buff(w, a, i).map(|b| b.ext), Some(ElemExt::SlowReflect { refl: true, .. }));
    remove_actor_buff(w, a, BuffClass::BuffSlow30PercentProjectile);
    if refl {
        return;
    }
    match a {
        ActorRef::Projectile(id) if w.actors.projectiles.get(id).is_some_and(|p| !p.dead) => {
            crate::world::spawning::destroy_projectile(w, id, false);
        }
        ActorRef::Item(id) if w.actors.items.get(id).is_some_and(|p| !p.dead) => {
            crate::world::items::destroy_item(w, id);
        }
        _ => {}
    }
}

// ------------------------------------------------------------------ Buff_C021_SetHomingTowardsTarget
//
// ReceiveBeginPlay @2769 (authority): GetAllActorsOfClass(ParentProjectilePhysics) @2825, for each @15: the same
// Comp_Actor_Destructible TeamIndex as the buff's owner -> next @254; not (CanBeReflected and type 0 or 2) ->
// K2_DestroyActor of the buff @1579 and the loop goes on; else Projectiles += it, SpawnBuffActor(
// Buff_AlterProjectileSpeed, it, the owner, 0, DeBuffsDuration, true) @807, ProjectileHomingMulticast (homing,
// 20000, the owner's root) @2950-3134, UpdateMaxLifetimeWithAuthority(DeBuffsDuration) @1071, SpawnBuffActor(
// Buff_C021_SlowReflectProjectile, it, the Instigator, 0, DeBuffsDuration) @1195, sfx_cha_C021_projectile_attract on
// the buff @1305. Then GetAllActorsOfClass(ItemActorParent) @2916, for each IsThrown and of another team @1833: the
// same buffs, homing and sound (@2050-2444), no lifetime and no CanBeReflected test.
// Model: the World's projectiles and items are the two lists (in their order; Unverified: GetAllActorsOfClass's order
// is the actor iterator's). The sound's place is the owner's (the buff actor is at its owner). Of the projectiles only
// ParentProjectilePhysics' children are taken (the data's SuperStruct chain, actor_is_a): Morty's C019_CookGrenade_C
// family and C019_WhipProjectile_C, LassoProjectileFastV3_C and the other SpawnedActorParent-only classes are not
// found; a class with no chain in the data warns and is not taken.
// Deviation: the scan runs in the frame's buff timer pass, not inside the ApplyBuff notify: the buff kind has no
// world at its begin; the projectiles' moves of the frame come between.

fn homing_pass(w: &mut World, f: FighterId, a: ActorRef, dur: Real, life: bool) {
    spawn_actor_buff(w, a, name!("Buff_AlterProjectileSpeed_C"), 0, dur, true, None);
    if crate::actors::world_actor::actor_dead(w, a) {
        return;
    }
    set_homing(w, a, ActorRef::Fighter(f), 20000.0);
    if life
        && let ActorRef::Projectile(id) = a
        && let Some(p) = w.actors.projectiles.get_mut(id)
    {
        crate::actors::projectile::update_max_lifetime(p, dur, 1);
    }
    spawn_actor_buff(w, a, name!("Buff_C021_SlowReflectProjectile_C"), 0, dur, true, None);
    let (x, y) = (w.fighter(f).body.x, actor_y(w, f));
    sound(w, f, name!("sfx_cha_C021_projectile_attract"), x, y);
}

fn homing_scan(w: &mut World, f: FighterId) -> bool {
    let dur = buff_def(w, ActorRef::Fighter(f), name!("Buff_C021_SetHomingTowardsTarget_C"))
        .and_then(|d| num(d, "DeBuffsDuration"))
        .unwrap_or_else(|| {
            warn("Buff_C021_SetHomingTowardsTarget_C data missing: DeBuffsDuration");
            0.0
        });
    let team = w.fighter(f).team.team;
    let mut destroy = false;
    let prs: Vec<_> = w.actors.projectiles.ids().collect();
    for pid in prs {
        let Some(p) = w.actors.projectiles.get(pid) else { continue };
        if p.dead {
            continue;
        }
        let a = ActorRef::Projectile(pid);
        let (cls, t) = (p.cls, p.attack_type);
        let ppp = actor_is_a(w, ActorRef::Fighter(lookup_owner(w, a)), cls, name!("ParentProjectilePhysics_C"));
        if ppp.is_none() {
            warn("Buff_C021_SetHomingTowardsTarget_C: a projectile class has no class parents in the data: not found");
        }
        if ppp != Some(true) || owner_team(w, a) == team {
            continue;
        }
        if !(can_be_reflected(w, a) && (t == 0 || t == 2)) {
            destroy = true;
            continue;
        }
        homing_pass(w, f, a, dur, true);
    }
    let items: Vec<_> = w.actors.items.ids().collect();
    for iid in items {
        let Some(it) = w.actors.items.get(iid) else { continue };
        if it.dead || !it.thrown || !crate::actors::item::is_owned(w, iid) {
            continue;
        }
        if it.owner.is_some_and(|o| w.fighter(o).team.team == team) {
            continue;
        }
        homing_pass(w, f, ActorRef::Item(iid), dur, false);
    }
    destroy
}

fn set_homing_timer(w: &mut World, f: FighterId, b: BuffRef) -> bool {
    let Some(e) = buff_mut(w, b) else { return false };
    if matches!(e.ext, BuffExt::SetHoming { done: true }) {
        return false;
    }
    e.ext = BuffExt::SetHoming { done: true };
    homing_scan(w, f)
}

/// This file's registry entries (the two field classes, the two buffs).
pub fn register(r: &mut Registry) -> Result<(), RegistryError> {
    let field = WorldActorHooks {
        spawn: Some(field_spawn),
        spawned: Some(field_spawned),
        // Nothing reads it: Sphere and PS_Shield take world scales (SetWorldScale3D @3837 / @4554 / @9616, Radius / 50
        // x the timelines) and TraceCollision rides Sphere; BeginPlay's trace has its own Radius.
        size: Some(SizeHooks::default()),
        step: Some(field_step),
        bodies: Some(field_bodies),
        any_damage: Some(field_any_damage),
        ..WorldActorHooks::default()
    };
    r.actors
        .enter(FIELD, ActorEntry::new(ActorClass::World(WorldActorClass::C021ForceField), ActorHooks::World(field)))?;
    let perk = WorldActorHooks { spawn: Some(perk_spawn), spawned: Some(perk_spawned), step: Some(perk_step), ..field };
    r.actors.enter(
        PERK_FIELD,
        ActorEntry::new(ActorClass::World(WorldActorClass::C021ForceFieldPerk), ActorHooks::World(perk)),
    )?;
    let slow = ActorBuffHooks {
        begin: Some(slow_reflect_begin),
        hurt_sphere: Some(slow_reflect_hurt_sphere),
        taken_cond: Some(slow_reflect_taken),
        on: Some(slow_reflect_on),
        end: Some(slow_reflect_end),
        ..ActorBuffHooks::default()
    };
    r.buffs.enter(
        SLOW_REFLECT,
        BuffEntry {
            actor: Some(slow),
            ..BuffEntry::new(BuffClass::BuffC021SlowReflectProjectile, BuffHooks::default())
        },
    )?;
    r.buffs.enter(
        SET_HOMING,
        BuffEntry::new(
            BuffClass::BuffC021SetHomingTowardsTarget,
            BuffHooks { timer: Some(set_homing_timer), ..BuffHooks::default() },
        ),
    )
}
