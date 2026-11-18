//! Harley Quinn's bomb: HarleyBomb_C ([`Bomb`], a
//! world actor and a DealerActor: a rigid sphere on `mvs_collision::rigid_sphere` that sticks to a fighter, is
//! blocked, carried through tunnels (its CarryHooks) and detonates on a hit, its fuse or its owner's call).
//!
//! The shared code reaches the bomb's attachment (`actors::world_actor::attached_to`: [`attached_to`]), its place
//! ([`place`]) and its dealer fields (`combat::hits` dealer: [`dealer_parts`]).
//!
//! HarleyBomb (neutral special "Stuffie Bat": Bomb_ThrowF / ThrowU, Bomb_Air_ThrowF / ThrowU / ThrowD; the second
//! press, Special_Detonate / Special_AirDetonate, activates every live one). Source: HarleyBomb, with native reads of
//! the attach, the blast and the rigid body.
//! A SpawnedActorParent child whose ProjectileMovement is deactivated: the Capsule (60 / 60) is a simulating rigid
//! body. The ability trigger of the spawn (slot 1) is the world's (Fighter.actorSpawnAbility).
//!
//! Free flight (the rigid body's closed update, float32): the spawner's launch (the notify's direction, normalised, x
//! InitialSpeed, Y along the owner's facing, plus half the owner's velocity) is pending until the body's first
//! step; a step is v += pending, v.Z += dt x -980, v *= max(0, 1 - dt x 0.2) (linear damping), p += v dt; the
//! looping timer GravityWorkAround (GravityIncrementRate 0.05 s) runs behind the step of every third frame from the
//! spawn's: v.Z += GravityIncrementValue (-50) while v.Z > TerminalVelocity (-5000) and the bomb is not attached.
//! dt is the port's frame time (DELTA_SECONDS), where the native read's table has 1 / 60.
//!
//! ReceiveBeginPlay @2115: FlipOnAttach = the instigator faces left @2446; Delay(5.0) @3370 -> Detonate Server
//! @1494 (the fuse); Delay(0.0667) @3315 -> Delay(0.2) @118 -> StartUpDelayArm = true and the fuse loop
//! sfx_shared_class_assassin_satchel_fuse_loop @15.
//! PlayerAttachCollision (a sphere of world radius 100 x 0.2 x 2.25 = 45 that overlaps Pawn and HurtBox) begin
//! overlap @1689, for a fighter: another than the owner outside the cooldown (LastAttachmentTime -1, CoolDownTime
//! 0.25: always, since nothing is attached twice) goes to the attach at once; the owner needs StartUpDelayArm and
//! the overlap, else Delay(0.2) @775 -> the same test @173 on the fighter that overlapped last, again and again.
//! The attach @341-830 stops on a target that is invulnerable or dead (@633-760); else Turn Off Physics @937,
//! AttachedTo, sfx_cha_harley_bomb_attach @1034, the VO ability_batbomb_attach at 0.33 unless the carrier is the
//! owner @4745, the mesh's looping BatBomb_Item_Attach @1048, IsAttachedFront = (bomb.Y >= fighter.Y) ==
//! fighter.FacingLeft @1306-1404 and AttachToFighter @1479: AttachedToAlly = the same team (the thrower too) @0-238,
//! IsAttached @555, the sphere's overlap events off @566 (no second carrier), the actor on the carrier's capsule
//! centre @599 with relative Z = ally ? -(the carrier's capsule half height) - 100 : 0 @674-900,
//! DestroyOnExitedArena? = AttachedToAlly @953. The carrier's FacingChanged runs AttachToFighter again @3595 (the
//! offset re-read). The visual side is the view's (legSocket, flipOnAttach).
//! Ends, all Detonate Server @1651 (a do-once: Detonate, Detonate Client, ReleaseToPool): the fuse, ActivateObject
//! @5037, the bomb's own ExitedArena @5224 and the carrier's exit or defeat @5315 (both LeftArena = true).
//! ExitedArena @5224 is Comp_Actor_Child.ExitedArena, broadcast by BlastBoxPlayerExit HandlePlayerKnockout @905 for an
//! actor that is no fighter, from the jump-out area's end overlap (@7484 -> @1195: HandleBlastBoxExit(false, true)).
//! That handler takes the actor's root capsule alone (BPI_GetRootCapsule @0-19: ExitedArenaCapsule while
//! AttachedToAlly, else Capsule), and the Capsule is PhysicsOnly from BeginPlay @2188 (no query, so no overlap): only a
//! bomb on a team mate (ExitedArenaCapsule QueryOnly with overlap events from AttachToFighter @271-342, radius and half
//! height 32) gets it, when that capsule leaves World.blastOuter. Its location is then outside the area too, so
//! CheckIfPositionOverlaps @424 sends it on to HandlePlayerKnockout.
//! SpawnedActorParent's CheckBlastBoxTimer (set at BeginPlay with the class's DestroyOnExitedArena? true) runs
//! CheckBlastBoxExit @20219-20685 on the stored bounds with DestroyOnExitedArena? as it is then (the class's true until
//! an attach, AttachedToAlly after it, @953): outside, DestroySpawnedActor(true), which ends the bomb with no Detonate
//! (ReceiveDestroyed @3682 unbinds and stops the fuse). So a free bomb thrown off the stage goes without a blast.
//! Detonate: the centre is the carrier's location + its capsule half height, the bomb teleported there, when
//! (!LeftArena || AttachedToAlly) and LastAttachedTo is valid @47-409, else the bomb's own place;
//! SphereTraceMultiForObjects(radius BlastRadius 350; PhysicsBody, DamageDestructible, ProjectileDestructible,
//! HurtBox, ItemDestructible) @546; per fighter hit: above = (he carries it) || his Z >= the centre's @1638-1790,
//! r = (his Y - the centre's) / 350, k = the curve SatchelBlastKnockBack at |r|, launch = (k sign(r) MaxKnockBackY
//! 1000, k MaxKnockBackZ 1500, down when not above) @1809-2562; the same team while !LeftArena is skipped
//! @2719-2946; the packet: Damage = same team ? 0 : |launch| x 0.0014, BaseKnockBack = |launch| x 0.6,
//! KnockBackScaling 33, the vector (launch.Y x 0.5, launch.Z), HitPauseTime 0.1, HitStunMult 1, IgnoreFacing,
//! ForceKnockback, ArmorPierce, DestroyProjectiles, HitEachActorOnlyOnce, sfx_shared_item_rock_hit, DoLOSCheck for
//! any but the carrier; the class's On Hit Buffs (Buff_ApplyConfetti to a hit enemy).
//! Detonate Client: PS_HarleySPD_BatDollExplosion at scale 0.75 with the Team parameter, the fuse loop stopped over
//! 200 ms, sfx_cha_harley_bomb_explode.
//!
//! Stage contacts (the closed rule of PhysX's contact rows for one sphere of radius 60 against static boxes):
//! `mvs_collision::rigid_sphere` sphere_step with the bomb's settings below.
//! The surfaces are the stage's floor box and its pass-through platforms' boxes (nothing else blocks the capsule's
//! profile ProjectileNavPlatform on the Lab and Sky Arena); a platform is met on every face, and only in a frame whose
//! tick read v.Z <= 0 (ReceiveTick @5419: Platform response = v.Z > 0 ? Ignore : Block).
//! The body never sleeps (the -50 timer wakes it every third frame), so it is stepped until it attaches or ends.
//! Capsule.OnComponentHit @8558-8955 (queued for every step in which the solver applied a normal impulse, read
//! after the step and ahead of the timer): GetVelocity().Z > 150 || GetVelocity().Y > 100 posts BounceAudio
//! (sfx_cha_harley_bomb_bounce): the bounces and, moving toward UE +Y only, every frame it rolls faster than 100.
//! Nothing else reacts to a stage hit: SpawnedActorParent's Blocked @8613 is reached through BPI_Blocked alone,
//! which only ProjectileBarrier and Steven_ShieldActor call.
//! Unverified: restitution 0.65 (the combine inside PhysX is the harness's, not a read; a live trial). Deviation: an
//! apex inside a platform pops out without the single-step contact losses the DLL shows (under 12 units for 2
//! frames).
//! Unverified: two contacts at once are solved in the list's order (the floor, then the platforms). Unverified: the
//! hit event's phase (after the step, ahead of the timer) is stock 4.26's. Left out: the friction row's anchor-drift
//! bias (0.1 % on rolling frames). Not ported, warned once each: Sky Arena's arm donut (a convex of unread profile and
//! place), a bomb faster than about 4200 u/s through a platform (the rule's contact is the nearest face, as the DLL's
//! is, so it goes through; no table).
//! Morty's ship pieces are PhysX kinematic boxes rising at the ship's speed vk (the spawn notify's 1000 u/s), met on
//! the platforms' rule: the contact rows take the piece's velocity, so the bounce off a top face is
//! vz' = vk + 0.65 (vk - vz*) (vz*: after the step's gravity and damping) and the sideways speed x 0.7147, as the
//! build's PhysX DLL gives with a kinematic box (harness runs at vk 1250, 16 cases to 1e-3).
//! Deviation: the attach sphere's centre is taken at the actor (the mesh's rag doll hangs it 13.5 off at rest); the
//! carrier's exit or defeat, its turn and ExitedArenaCapsule's end overlap with the jump-out area (inside the
//! carrier's move in the build) are read at the bomb's tick, not inside the fighter's; the blast visits the fighters
//! in the sim's order, one hit each, by their hurt shapes in the play plane; the curve is held at its end keys; the
//! Delays count from the spawn frame.
//! ReceiveAnyDamage @6382 (`any_damage`; its one ported caller: C021_ForceField's Damage-1 hit):
//! IsProjectileClanking -> QueueDestroyAfterClank @6396, else Detonate Server @6408. IsProjectileClanking is written
//! by BPI_SetIsProjectileClanking @8956 alone, inside a clank, which the port does not make for this class: false.
//! Blocked @5114 (`blocked`; no parent call): Detonate Server @5194. Its one ported caller: an enemy
//! Steven_ShieldActor's BlockBox (ProjectileDestructible, HitBox: Overlap) over PlayerAttachCollision (object type
//! HitBox, ProjectileDestructible: Overlap; its overlap events off from the attach @566: `hit_shapes` none), the
//! bomb's Comp_Actor_Destructible IsProjectile (Steven's barrier blockTick, BPI_Blocked @2013).
//! A fighter's hit box over the HurtBox: `hurt_box`; the blast on actors that are no fighters: [`blast_actors`]. Not
//! ported: the clank, BPI_IncrementCoopActions.

#![allow(clippy::neg_cmp_op_on_partial_ord, reason = "NaN-keeping negated tests")]

use mvs_collision::Ground;
use mvs_collision::rigid_sphere::{Body, Settings, Solver, Static};
use mvs_data::value::{Props, Value};

use super::actors::{dead_of, emitter, mid_frame_hit, owner_of, rows_of, scale_of, sound, touches};
use crate::Real;
use crate::State;
use crate::actors::world_actor::{WorldActor, WorldActorState, actor_dead, actor_p, actor_place};
use crate::actors::{ActorClass, ActorId, ActorRef, WorldActorClass};
use crate::combat::HitPacket;
use crate::combat::boxes::WorldSphere;
use crate::combat::elem::charge_pct;
use crate::combat::hits::{actor_hit_enemy, ally_hit, apply_projectile_hit, dealer, projectile_blocks};
use crate::combat::packet::{DataEnum, DataVec};
use crate::combat::phases::los_blocked;
use crate::consts::{DELTA_SECONDS, SOLID_NAV};
use crate::events::{Event, SoundKey};
use crate::fighter::common::{actor_def, actor_is_a, latent_frames, owner_team, warn_once};
use crate::fighter::control::{attack_dodged, dodge_over};
use crate::fighter::match_side::{SpawnRequest, post_vo};
use crate::fighter::moves::V2;
use crate::fighter::traces::{
    OBJ_DAMAGE_DESTRUCTIBLE, OBJ_HURT_BOX, OBJ_ITEM_DESTRUCTIBLE, OBJ_PHYSICS_BODY, OBJ_PROJECTILE_DESTRUCTIBLE,
    TraceBody, TraceHit, body_info, sphere_static, sphere_trace_multi_for_objects,
};
use crate::fighter::{FighterId, actor_y, capsule_half, facing, invulnerable};
use crate::math::to_int32;
use crate::name;
use crate::registry::ActorHooks;
use crate::registry::entries::{ActorEntry, CarryHooks, SizeHooks, WorldActorHooks};
use crate::registry::props::{flag, num, obj, text};
use crate::registry::{Registry, RegistryError};
use crate::world::World;
use crate::world::blast::{BlastBounds, blast_box_loop, check_blast_box_exit, new_blast_bounds, spawn_box};
use crate::world::pawns::{all_list, match_fighters, put_fighters};
use crate::world::spawn_spot::{capsule_c, capsule_shape, find_teleport_spot_shape, spawn_place};
use crate::world::spawning::{request_props, root_body_blockers};
use crate::world::stage_query::{PolyBlock, StageShapes, static_solid_shapes};

const BOMB: &str = "HarleyBomb_C";
/// `state`: FLY = the body simulates (in the air, bouncing, rolling or lying: it never sleeps); ATTACHED; 1 = a body
/// that is not stepped, which nothing in the sim sets (the checks hold a bomb in place with it).
const FLY: i32 = 0;
const ATTACHED: i32 = 2;
/// The Capsule's FBodyInstance: LinearDamping 0.2, AngularDamping 30 (w *= 1 - 30 dt each step), MassInKgOverride
/// 3000 (only the hit event's impulse threshold reads it); SatchelPhysicalMaterial against the stage's default
/// material.
pub const BOMB_BODY: Settings = Settings {
    linear_damping: 0.2,
    angular_damping: 30.0,
    // (SatchelPhysicalMaterial 1.0 + engine default 0.3) / 2: project combine mode Average
    restitution: 0.65,
    // (20 + 0.7) / 2, static = dynamic
    friction: 10.35,
    lock_spin: false,
    lock_y: false,
    lock_z: false,
};
const MASS: Real = 3000.0;
/// AddCollisionNotifyInfo: dropped under a squared normal impulse of 1e-4.
const HIT_IMPULSE: f64 = 0.01;
const TUNNEL_SPEED: Real = 4200.0;
/// m R^2 / I of a lone sphere (`sphereStep`'s default `k`).
const INERTIA_K: Real = 2.5;

/// SatchelBlastKnockBack: three keys, linear between them.
const CURVE: [(Real, Real); 3] = [(-0.0000783205, 1.0038089), (0.74961865, 1.0000298), (0.9999999, 0.45)];

fn blast_curve(t: Real) -> Real {
    if t <= CURVE[0].0 {
        return CURVE[0].1;
    }
    for i in 1..CURVE.len() {
        let (t1, v1) = CURVE[i];
        let (t0, v0) = CURVE[i - 1];
        if t <= t1 {
            return v0 + ((t - t0) / (t1 - t0)) * (v1 - v0);
        }
    }
    CURVE[CURVE.len() - 1].1
}

/// `Bomb` (HarleyBomb_C): the class's own fields; the `SimActor` ones are the [`WorldActor`]'s.
#[derive(State, Debug)]
pub struct Bomb {
    /// `a.p.HitBoxDamage ?? {}`.
    pub hit: HitPacket,
    pub attack_id: i32,
    /// The owner's at the spawn.
    pub facing: Real,
    /// FlipOnAttach.
    pub flip_on_attach: bool,
    pub x: Real,
    pub y: Real,
    pub vx: Real,
    pub vy: Real,
    /// SphereBody.spin.
    pub spin: Real,
    /// The launch, pending until the body's first step.
    pub pvx: Real,
    pub pvy: Real,
    /// Ticks lived, the spawn frame's included.
    pub age: i32,
    /// Simulating, held (the checks'), attached.
    pub state: i32,
    /// StartUpDelayArm.
    pub armed: bool,
    /// AttachedTo (= LastAttachedTo: nothing detaches).
    pub carrier: Option<FighterId>,
    /// AttachedToAlly.
    pub ally: bool,
    /// IsAttachedFront.
    pub front: bool,
    /// The actor's relative Z under the carrier's capsule.
    pub rel_y: Real,
    pub carrier_facing: Real,
    /// LeftArena.
    pub left_arena: bool,
    /// ExitedArenaCapsule overlaps the jump-out area (World.blastOuter).
    pub in_area: bool,
    /// GravityWorkAround's next expiry, seconds of counted timer ticks.
    pub grav_at: crate::Clock,
    /// The arena check's.
    pub blast_at: crate::Clock,
    /// Its BlastBoxBoundsMin / Max.
    pub blast_bounds: BlastBounds,
    /// The ids of the fighters overlapping the attach sphere.
    pub inside: Vec<i32>,
    /// The last overlapped fighter of the owner's path.
    pub poll_for: Option<FighterId>,
    /// The age the pending Delay(0.2) @775 ends at.
    pub poll_at: i32,
    /// The Capsule's, at the root's scale.
    pub radius: Real,
    /// Unscaled: the reads take the root's scale.
    pub attach_r: Real,
    /// ExitedArenaCapsule's radius (its half height is the same: a sphere), unscaled.
    pub exit_r: Real,
    /// SetActorEnableCollision(false) (the carry hook): no body contact, no overlap, no query.
    pub no_collision: bool,
}

impl Default for Bomb {
    /// The field initial values.
    fn default() -> Self {
        Bomb {
            hit: HitPacket::default(),
            attack_id: 0,
            facing: 0.0,
            flip_on_attach: false,
            x: 0.0,
            y: 0.0,
            vx: 0.0,
            vy: 0.0,
            spin: 0.0,
            pvx: 0.0,
            pvy: 0.0,
            age: 0,
            state: FLY,
            armed: false,
            carrier: None,
            ally: false,
            front: false,
            rel_y: 0.0,
            carrier_facing: 0.0,
            left_arena: false,
            in_area: false,
            grav_at: f64::from(0.05_f32),
            blast_at: f64::from(0.1_f32),
            blast_bounds: new_blast_bounds(),
            inside: Vec::new(),
            poll_for: None,
            poll_at: -1,
            radius: 0.0,
            attach_r: 0.0,
            exit_r: 0.0,
            no_collision: false,
        }
    }
}

impl Bomb {
    /// DestroyOnExitedArena?: the class's true until an attach, then AttachedToAlly.
    fn destroy_on_exit(&self, class_default: bool) -> bool {
        if self.state == ATTACHED { self.ally } else { class_default }
    }
}

fn bomb_of(w: &World, id: ActorId) -> Option<&Bomb> {
    match &w.actors.actors.get(id)?.state {
        WorldActorState::HarleyBomb(b) => Some(b),
        _ => None,
    }
}

fn bomb_mut(w: &mut World, id: ActorId) -> Option<&mut Bomb> {
    match &mut w.actors.actors.get_mut(id)?.state {
        WorldActorState::HarleyBomb(b) => Some(b),
        _ => None,
    }
}

/// `Bomb.attachedTo`: the fighter the bomb is stuck to, `None` for none.
pub fn attached_to(w: &World, a: &WorldActor, b: &Bomb) -> Option<FighterId> {
    let _ = (w, a);
    if b.state == ATTACHED { b.carrier } else { None }
}

/// The bomb's place (its `x`, `y`).
pub fn place(w: &World, a: &WorldActor, b: &Bomb) -> Option<(Real, Real)> {
    let _ = (w, a);
    Some((b.x, b.y))
}

/// The bomb as a dealer (`DealerActor`): its place, facing and AttackID.
pub fn dealer_parts(w: &World, a: &WorldActor, b: &Bomb) -> (Real, Real, Real, i32) {
    let _ = (w, a);
    (b.x, b.y, b.facing, b.attack_id)
}

/// `destroyOnExit`'s class default: the data's DestroyOnExitedArena?.
fn class_destroy_on_exit(w: &World, id: ActorId) -> bool {
    flag(actor_p(w, ActorRef::Actor(id)), "DestroyOnExitedArena?")
}

fn fuse_key(w: &World, id: ActorId) -> SoundKey {
    SoundKey { tag: name!("hqbomb"), n: w.actors.actors.get(id).map_or(-1, |a| a.seq) }
}

fn fighter_id(w: &World, f: FighterId) -> i32 {
    w.fighter(f).id
}

/// Detonate's numbers for a HitActor at (x, y) (@1809-3879): the launch from BlastKnockBack over the distance along
/// UE Y, up when the actor is at or above the bomb (or carries it), Damage 0 for the bomb's team (@3699).
#[derive(Clone, Copy, Debug, PartialEq)]
struct Blast {
    damage: Real,
    base_knock_back: Real,
    /// KnockBackDirectionVector.X / .Y.
    kx: Real,
    ky: Real,
}

/// `blastHit`'s numbers. UE Y = -x: launch.Y is away from the centre, as a sim x here.
fn blast_numbers(w: &World, id: ActorId, x: Real, y: Real, carried: bool, same: bool) -> Blast {
    let Some(b) = bomb_of(w, id) else { return Blast { damage: 0.0, base_knock_back: 0.0, kx: 0.0, ky: 0.0 } };
    let p = actor_p(w, ActorRef::Actor(id));
    let r = num(p, "BlastRadius").unwrap_or(0.0);
    let ky = num(p, "MaxKnockBackY").unwrap_or(0.0);
    let kz = num(p, "MaxKnockBackZ").unwrap_or(0.0);
    let above = carried || y >= b.y;
    let q = (x - b.x) / r;
    let k = blast_curve(q.abs());
    let lx = k * crate::math::sign(q) * ky;
    let ly = k * kz * if above { 1.0 } else { -1.0 };
    let mag = crate::math::ue::size([0.0, lx, ly]);
    Blast { damage: if same { 0.0 } else { mag * 0.0014 }, base_knock_back: (mag * 0.6), kx: (lx * 0.5), ky: ly }
}

/// The packet of a fighter's hit (`blastHit`'s object, as the typed packet).
fn blast_packet(n: &Blast, los: bool) -> HitPacket {
    HitPacket {
        damage: Some(n.damage),
        base_knock_back: Some(n.base_knock_back),
        knock_back_scaling: Some(33.0),
        knock_back_direction_vector: Some(DataVec { x: Some(n.kx), y: Some(n.ky), z: None }),
        attack_tag: Some(name!("None")),
        hit_reaction: Some(DataEnum::Value(0)),
        hit_pause_time: Some(0.1),
        hit_stun_mult: Some(1.0),
        on_hit_audio: Some(name!("sfx_shared_item_rock_hit")),
        ignore_facing: Some(true),
        force_knockback: Some(true),
        armor_pierce: Some(true),
        destroy_projectiles: Some(true),
        hit_each_actor_only_once: Some(true),
        do_los_check: Some(los),
        play_on_hit_effects: Some(true),
        kbi_multiplier: Some(1.0),
        ..HitPacket::default()
    }
}

/// The same packet as the data object `actorVictimHit` takes, with the class's On Hit Buffs rows when it has any.
fn blast_props(n: &Blast, los: bool, rows: &[Props]) -> Props {
    let num = |v: Real| Value::Num(f64::from(v));
    let mut dir = Props::new();
    dir.insert("X".into(), num(n.kx));
    dir.insert("Y".into(), num(n.ky));
    let mut p = Props::new();
    p.insert("Damage".into(), num(n.damage));
    p.insert("BaseKnockBack".into(), num(n.base_knock_back));
    p.insert("KnockBackScaling".into(), num(33.0));
    p.insert("KnockBackDirectionVector".into(), Value::Obj(dir));
    p.insert("AttackTag".into(), Value::Str("None".into()));
    p.insert("HitReaction".into(), num(0.0));
    p.insert("HitPauseTime".into(), num(0.1));
    p.insert("HitStunMult".into(), num(1.0));
    p.insert("OnHitAudio".into(), Value::Str("sfx_shared_item_rock_hit".into()));
    p.insert("IgnoreFacing".into(), Value::Bool(true));
    p.insert("ForceKnockback".into(), Value::Bool(true));
    p.insert("ArmorPierce".into(), Value::Bool(true));
    p.insert("DestroyProjectiles".into(), Value::Bool(true));
    p.insert("HitEachActorOnlyOnce".into(), Value::Bool(true));
    p.insert("DoLOSCheck".into(), Value::Bool(los));
    p.insert("PlayOnHitEffects".into(), Value::Bool(true));
    p.insert("KBI_Multiplier".into(), num(1.0));
    if !rows.is_empty() {
        let mut o = Props::new();
        o.insert("OnHitBuffs".into(), Value::Arr(rows.iter().cloned().map(Value::Obj).collect()));
        p.insert("OnHitBuffs".into(), Value::Obj(o));
    }
    p
}

/// The class's `On Hit Buffs`.OnHitBuffs rows, as the data objects.
fn data_rows(w: &World, id: ActorId) -> Vec<Props> {
    actor_p(w, ActorRef::Actor(id))
        .get("On Hit Buffs")
        .and_then(Value::as_obj)
        .and_then(|o| o.get("OnHitBuffs"))
        .and_then(Value::as_arr)
        .map(|a| a.iter().filter_map(Value::as_obj).cloned().collect())
        .unwrap_or_default()
}

/// The HitActor's Comp_Actor_Destructible TeamIndex, `None` for an actor without the component. Every
/// SpawnedActorParent and ItemActorParent child has one (the parents' templates), and its TeamIndex is the owner's
/// (`owner_team`; an item's the last grabber's). Another actor has one where its class's data names it.
fn destructible_team(w: &World, o: ActorRef) -> Option<i32> {
    if let ActorRef::Item(it) = o {
        return Some(crate::actors::item::item_destructible_team(w, it));
    }
    let (owner, lookup, cls) = match o {
        ActorRef::Projectile(id) => {
            let p = w.actors.projectiles.get(id)?;
            (p.owner, p.lookup_fighter, p.cls)
        }
        ActorRef::Actor(id) => {
            let p = w.actors.actors.get(id)?;
            (Some(p.owner), None, p.cls)
        }
        ActorRef::Marker(id) => {
            let p = w.actors.markers.get(id)?;
            (Some(p.owner), None, p.cls)
        }
        ActorRef::Ship(id) => {
            let p = w.actors.ships.get(id)?;
            (Some(p.owner), None, p.cls)
        }
        ActorRef::Mortar(id) => {
            let p = w.actors.mortars.get(id)?;
            (Some(p.owner), None, p.cls)
        }
        ActorRef::AimPunch(id) => {
            let p = w.actors.aim_punches.get(id)?;
            (Some(p.owner), None, p.cls)
        }
        ActorRef::Accelerator(id) => {
            let p = w.actors.accelerators.get(id)?;
            (Some(p.owner), None, p.cls)
        }
        _ => return None,
    };
    let own = owner.or(lookup)?;
    let Some(def) = actor_def(w, ActorRef::Fighter(own), cls) else {
        warn_once(w, name!("HarleyBomb blast: no class data for a class (taken as without Comp_Actor_Destructible)"));
        return None;
    };
    let me = ActorRef::Fighter(own);
    let has = obj(&def.components, "Comp_Actor_Destructible").is_some()
        || actor_is_a(w, me, cls, name!("SpawnedActorParent_C")) == Some(true)
        || actor_is_a(w, me, cls, name!("ItemActorParent_C")) == Some(true);
    has.then(|| owner_team(w, o))
}

/// BlastObjects: PhysicsBody, DamageDestructible, ProjectileDestructible, HurtBox and ItemDestructible (@566-598).
const BLAST_TYPES: [Real; 5] = [
    OBJ_PHYSICS_BODY as Real,
    OBJ_DAMAGE_DESTRUCTIBLE as Real,
    OBJ_PROJECTILE_DESTRUCTIBLE as Real,
    OBJ_HURT_BOX as Real,
    OBJ_ITEM_DESTRUCTIBLE as Real,
];

/// Detonate's actors that are no fighters (@4150 -> @1809, the same path): the SphereTraceMultiForObjects of
/// BlastRadius at the actor over BLAST_TYPES (@566-598), once per actor. LocalSameTeam is set false once, at the
/// call's start (@36), and written only for a HitActor with a Comp_Actor_Destructible (@2719-2946: its TeamIndex
/// against the bomb's, the owner's team); such an actor of the bomb's team is skipped unless LeftArena (@2994-3032).
/// One without the component (@2799 -> @3200) is never skipped and keeps the LocalSameTeam the last actor with one
/// left (the fighters included: BaseFighter has the component): Damage 0 when that was true (@3699). DoLOSCheck is on
/// (no such actor is AttachedTo); pfgDoDamage @3879 with On Hit Buffs through `actor_victim_hit`.
/// `same`: the LocalSameTeam the fighters' pass left.
// Unverified: no ported actor carries the DestructibleEnvironment tag, so the LOS check is never turned off here.
// Deviation: the bodies are the ones the port's traces gather (ActorKind.bodies), the visit in their order, behind
// every fighter (the build's hit list mixes them by the trace's order), so the LocalSameTeam an actor without the
// component reads may be another than the build's.
fn blast_actors(w: &mut World, id: ActorId, r: Real, rows: &[Props], mut same: bool) {
    let me = ActorRef::Actor(id);
    let Some(b) = bomb_of(w, id) else { return };
    let (bx, by, left) = (b.x, b.y, b.left_arena);
    let team = w.fighter(owner_of(w, id)).team.team;
    let mut hits: Vec<TraceHit> = Vec::new();
    sphere_trace_multi_for_objects(w, bx, by, bx, by, r, &BLAST_TYPES, &[Some(me)], Some(me), &mut hits);
    let mut seen: Vec<ActorRef> = Vec::new();
    for h in hits {
        let Some(o) = h.actor else { continue };
        // (the pawns: the fighters' own pass)
        if matches!(o, ActorRef::Fighter(_)) || seen.contains(&o) {
            continue;
        }
        seen.push(o);
        let Some((ax, ay, _)) = actor_place(w, o) else { continue };
        if actor_dead(w, o) {
            continue;
        }
        if let Some(t) = destructible_team(w, o) {
            same = t == team;
            if same && !left {
                continue;
            }
        }
        if los_blocked(w, bx, by, ax, ay) {
            continue;
        }
        let n = blast_numbers(w, id, ax, ay, false, same);
        let packet = blast_props(&n, true, rows);
        crate::chars::shaggy::shockwave::actor_victim_hit(w, me, o, &packet, rows, None);
    }
}

/// Detonate (a do-once through `dead`): the blast, the client's effects.
fn detonate(w: &mut World, id: ActorId) {
    let me = ActorRef::Actor(id);
    if dead_of(w, id) {
        return;
    }
    if let Some(a) = w.actors.actors.get_mut(id) {
        a.dead = true;
    }
    let f = owner_of(w, id);
    let Some(b) = bomb_of(w, id) else { return };
    let (carrier, attached, ally, left, attack_id) =
        (b.carrier, b.state == ATTACHED, b.ally, b.left_arena, b.attack_id);
    if (!left || ally)
        && let Some(c) = carrier
    {
        let (cx, cy) = (w.fighter(c).body.x, actor_y(w, c) + capsule_half(w, c));
        if let Some(b) = bomb_mut(w, id) {
            b.x = cx;
            b.y = cy;
        }
    }
    let (bx, by) = bomb_of(w, id).map_or((0.0, 0.0), |b| (b.x, b.y));
    let p = actor_p(w, me);
    let r = num(p, "BlastRadius").unwrap_or(0.0);
    let rows = rows_of(w, id, "On Hit Buffs");
    let data = data_rows(w, id);
    // LocalSameTeam @36
    let mut local_same = false;
    let team = w.fighter(f).team.team;
    let all = all_list(w);
    for &o in &all {
        if !touches(w, o, bx, by, r, false) {
            continue;
        }
        let carried = attached && Some(o) == carrier;
        let same = w.fighter(o).team.team == team;
        local_same = same;
        if same && !left {
            continue;
        }
        let (ox, oy) = (w.fighter(o).body.x, actor_y(w, o));
        let los_free = carried || !los_blocked(w, bx, by, ox, oy);
        if !los_free {
            continue;
        }
        if projectile_blocks(w, &dealer(w, me), o) {
            continue;
        }
        let n = blast_numbers(w, id, ox, oy, carried, same);
        let pkt = blast_packet(&n, !carried);
        if invulnerable(w, o) {
            // pfgDoDamage @3879 with the packet as built for him (BaseKnockBack @3779): the dodge, its burst and, with
            // the template's UseDelayedDodgeText false, the DODGED text at once.
            if !same {
                let tick = w.flow.tick;
                attack_dodged(w, me, o, attack_id, n.base_knock_back, tick, None, false);
            }
            continue;
        }
        dodge_over(w, me, o);
        mid_frame_hit(w, o);
        let d = dealer(w, me);
        let ok = apply_projectile_hit(
            w,
            &d,
            o,
            &pkt,
            &rows,
            V2 { x: ox, y: oy },
            Some(V2 { x: n.kx, y: n.ky }),
            &[],
            charge_pct(w, me),
        );
        if ok && !same {
            let d = dealer(w, me);
            actor_hit_enemy(w, &d, o);
        }
        if same && o != f {
            // NotifyAllyOfHit (@21344): a team mate, not the Instigator
            let d = dealer(w, me);
            ally_hit(w, &d, o, &pkt);
        }
    }
    put_fighters(w, all);
    blast_actors(w, id, r, &data, local_same);
    let (cls, fid) = (w.actors.actors.get(id).expect("a live actor").cls, fighter_id(w, f));
    emitter(w, cls, fid, name!("PS_HarleySPD_BatDollExplosion"), bx, by, 0.75, 0.0, [0.0; 3]);
    let armed = bomb_of(w, id).is_some_and(|b| b.armed);
    if armed {
        let key = fuse_key(w, id);
        w.events.push(Event::SoundStop { fighter: fid, key, fade: Some(0.2), ev: None, curve: None, cut: false });
    }
    if let Some(b) = bomb_mut(w, id) {
        b.armed = false;
    }
    sound(w, fid, name!("sfx_cha_harley_bomb_explode"), bx, by);
}

/// AttachToFighter: the offset under the carrier's capsule, read at the call.
fn attach_to(w: &mut World, id: ActorId, o: FighterId) {
    let f = owner_of(w, id);
    let ally = w.fighter(o).team.team == w.fighter(f).team.team;
    let rel_y = if ally { -capsule_half(w, o) - 100.0 } else { 0.0 };
    let (cf, ox, oy) = (facing(w, o), w.fighter(o).body.x, actor_y(w, o));
    let Some(b) = bomb_mut(w, id) else { return };
    b.ally = ally;
    b.rel_y = rel_y;
    b.carrier_facing = cf;
    b.x = ox;
    b.y = oy + rel_y;
}

/// ExitedArenaCapsule (a sphere: half height = radius) against the jump-out area, in the play plane.
fn in_jump_out_area(w: &World, id: ActorId) -> bool {
    let Some(b) = bomb_of(w, id) else { return false };
    let o = w.stage.blast_outer;
    let r = b.exit_r * scale_of(w, id);
    let dx = crate::math::max(crate::math::max(o.left - b.x, 0.0), b.x - o.right);
    let dy = crate::math::max(crate::math::max(o.bottom - b.y, 0.0), b.y - o.top);
    (dx * dx + dy * dy).sqrt() <= r
}

/// The attach @341-830 for an overlapping fighter.
fn try_attach(w: &mut World, id: ActorId, o: FighterId) {
    if bomb_of(w, id).is_none_or(|b| b.state == ATTACHED) {
        return;
    }
    if invulnerable(w, o) || w.fighter(o).life.dead > 0.0 {
        return;
    }
    let f = owner_of(w, id);
    let (ox, of) = (w.fighter(o).body.x, facing(w, o));
    let Some(b) = bomb_mut(w, id) else { return };
    b.front = (b.x <= ox) == (of < 0.0);
    b.state = ATTACHED;
    b.carrier = Some(o);
    (b.vx, b.vy, b.pvx, b.pvy, b.spin) = (0.0, 0.0, 0.0, 0.0, 0.0);
    b.poll_at = -1;
    b.inside.clear();
    let (bx, by) = (b.x, b.y);
    let fid = fighter_id(w, f);
    sound(w, fid, name!("sfx_cha_harley_bomb_attach"), bx, by);
    if o != f {
        post_vo(w, f, name!("ability_batbomb_attach"), Some(o), 0.33);
    }
    attach_to(w, id, o);
    // ExitedArenaCapsule's collision on (AttachToFighter @271-342, ahead of the attach @599): its overlaps are updated
    // where the actor is then, the move to the carrier @599-900 updates them again: what it overlaps after both.
    if bomb_of(w, id).is_some_and(|b| b.ally) {
        let inside = in_jump_out_area(w, id);
        if let Some(b) = bomb_mut(w, id) {
            b.in_area = inside;
        }
    }
}

/// The owner's path of the overlap handler @173.
fn owner_test(w: &mut World, id: ActorId) {
    let Some(b) = bomb_of(w, id) else { return };
    let (o, armed, age) = (b.poll_for, b.armed, b.age);
    let there = o.is_some_and(|o| b.inside.contains(&fighter_id(w, o)));
    if let (Some(o), true, true) = (o, armed, there) {
        if let Some(b) = bomb_mut(w, id) {
            b.poll_at = -1;
        }
        try_attach(w, id, o);
    } else if bomb_of(w, id).is_some_and(|b| b.poll_at < 0)
        && let Some(b) = bomb_mut(w, id)
    {
        b.poll_at = age + to_int32(latent_frames(0.2));
    }
}

/// The NOWHERE floor of a bomb with its collision off: a box far below everything.
fn nowhere() -> Static {
    Static {
        g: Ground { left: 0.0, right: 0.0, top: -1e9, bottom: -1e9 - 1.0 },
        kinematic: None,
        static_solid: false,
        poly: None,
    }
}

/// One frame of the simulating body: the tick's platform response, the step, the hit event.
fn fly(w: &mut World, id: ActorId) {
    let Some(b) = bomb_mut(w, id) else { return };
    // ReceiveTick's read, ahead of the step and of the pending launch
    let up = b.vy > 0.0;
    b.vx += b.pvx;
    b.vy += b.pvy;
    (b.pvx, b.pvy) = (0.0, 0.0);
    let (no_collision, vy) = (b.no_collision, b.vy);
    if !w.stage.stage_platforms.is_empty() && vy.abs() > TUNNEL_SPEED {
        warn_once(w, name!("HarleyBomb: faster than 4200 u/s over platforms: it can pass one (no table for it)"));
    }
    if w.data.stage.name.starts_with("Map_M011") {
        warn_once(w, name!("HarleyBomb: Sky Arena's arm donut collider is not ported (profile and place unread)"));
    }
    // The stage solids that block PhysicsBody are met as the floor, rising or not (with the actor's collision off the
    // body meets nothing). The Platform channel's boxes (the stage platforms, Morty's ship pieces) only on a tick that
    // starts with vz <= 0: ReceiveTick's SetCollisionResponseToChannel(20, Block) @5438-5609, Ignore while rising.
    let mut list: Vec<Static> = Vec::new();
    let floor = if no_collision {
        nowhere()
    } else {
        let mut solids = StageShapes::default();
        static_solid_shapes(w, SOLID_NAV, name!("HarleyBomb_C body"), &mut solids);
        list.extend(solids.boxes.iter().map(|&g| sphere_static(w, g)));
        if !up {
            for p in w.stage.stage_platforms.iter() {
                let g = Ground {
                    left: f64::from(p.left),
                    right: f64::from(p.right),
                    bottom: f64::from(p.bottom),
                    top: f64::from(p.top),
                };
                list.push(sphere_static(w, g));
            }
            // Morty's ship pieces (object type Platform, Block for PhysicsBody) on the same rule, as kinematic boxes
            // rising with the ship (`ship_statics`).
            let mut i = 0;
            while let Some(sh) = w.actors.ships.nth(i) {
                i += 1;
                crate::chars::morty::ship_statics(w, sh, |pc, _| pc.nav, &mut list);
            }
        }
        // (the polygon solids after the boxes)
        list.extend(solids.polys.iter().map(PolyBlock::rigid));
        sphere_static(w, w.stage.ground)
    };
    let Some(b) = bomb_mut(w, id) else { return };
    let mut body = Body {
        x: f64::from(b.x),
        y: f64::from(b.y),
        vx: f64::from(b.vx),
        vy: f64::from(b.vy),
        spin: f64::from(b.spin),
        radius: f64::from(b.radius),
        ..Body::default()
    };
    let hit = Solver::default().sphere_step(&mut body, &floor, Some(&list), DELTA_SECONDS, &BOMB_BODY, INERTIA_K);
    (b.x, b.y, b.vx, b.vy, b.spin) = (
        crate::math::narrow(body.x),
        crate::math::narrow(body.y),
        crate::math::narrow(body.vx),
        crate::math::narrow(body.vy),
        crate::math::narrow(body.spin),
    );
    // OnComponentHit @8558: the velocity after the step; UE Y = -x.
    if hit * f64::from(MASS) >= HIT_IMPULSE && (b.vy > 150.0 || -b.vx > 100.0) {
        let (x, y, fid) = (b.x, b.y, fighter_id(w, owner_of(w, id)));
        sound(w, fid, name!("sfx_cha_harley_bomb_bounce"), x, y);
    }
}

/// `1 x RelativeScale3D.X` of a component template, 1 without one.
fn scale_x(c: Option<&Props>) -> Real {
    c.and_then(|c| obj(c, "RelativeScale3D")).and_then(|s| num(s, "X")).unwrap_or(1.0)
}

/// HurtBox's sphere radius at the root's scale: a SphereComponent with no SphereRadius in its template (the native
/// default 32) under HarleyQuinn_BatBomb_SkelMesh, so at the mesh's RelativeScale3D 2.25: 72. `None` for a class
/// without it or with its body off.
fn hurt_radius(w: &World, id: ActorId) -> Option<(Real, crate::fighter::traces::BodyInfo)> {
    let a = w.actors.actors.get(id)?;
    let c = actor_def(w, ActorRef::Fighter(a.owner), a.cls).map(|d| &d.components);
    let hb = c.and_then(|c| obj(c, "HurtBox"));
    let info = body_info(hb, None)?;
    if !info.on {
        return None;
    }
    let hb = hb?;
    let mesh = text(hb, "AttachParent").and_then(|n| c.and_then(|c| obj(c, n)));
    Some((num(hb, "SphereRadius").unwrap_or(32.0) * scale_x(Some(hb)) * scale_x(mesh) * scale_of(w, id), info))
}

/// HurtBox as a body of the traces (`ActorKind.bodies`); object type ProjectileDestructible (profile
/// ProjectileDestructible, QueryAndPhysics), which no graph turns, so it answers for the actor's life, carried or
/// not. What meets it: the object queries for ProjectileDestructible (the Earth Smash pillar's overlap,
/// C021_ForceField's). The other bodies answer no query of the port: the Capsule is PhysicsOnly from BeginPlay @2188
/// and NoCollision at the attach @4889, ExitedArenaCapsule (QueryOnly from AttachToFighter @271; Custom with no
/// serialized object type: the shape default's WorldDynamic, as BaseFighter's Capsule) and PlayerAttachCollision
/// (HitBox) are of object types no query gathers.
// Deviation: the centre is taken at the actor, as the attach sphere's (the mesh hangs 13.5 off the AttachPoint at
// rest, turned with the body); the mesh's own rag doll bodies (Ragdoll, PhysicsBody, until @4998) are not entered
// (no physics asset is read; no query of the port asks for PhysicsBody).
fn bodies(w: &World, id: ActorId, out: &mut Vec<TraceBody>) {
    let Some(b) = bomb_of(w, id) else { return };
    if b.no_collision {
        return;
    }
    let Some((r, info)) = hurt_radius(w, id) else { return };
    out.push(TraceBody {
        ax: b.x,
        ay: b.y,
        bx: b.x,
        by: b.y,
        hx: 0.0,
        hy: 0.0,
        r,
        z: Real::NAN,
        object_type: info.kind,
        profile: info.profile,
        component: name!("HurtBox"),
        actor: Some(ActorRef::Actor(id)),
        platform: -1,
        blocks: info.blocks,
        poly: None,
        floor: None,
    });
}

/// The same HurtBox sphere for a fighter's hit box (profile ProjectileDestructible: the HitBox channel overlaps it).
fn hurt_box(w: &World, id: ActorId) -> Option<WorldSphere> {
    let b = bomb_of(w, id)?;
    if dead_of(w, id) || b.no_collision {
        return None;
    }
    let (r, _) = hurt_radius(w, id)?;
    Some(WorldSphere { x: b.x, y: b.y, z: Some(0.0), r, ..WorldSphere::default() })
}

/// PlayerAttachCollision's sphere in the play plane (its depth not read).
fn hit_shapes(w: &World, id: ActorId, out: &mut Vec<WorldSphere>) -> bool {
    let Some(b) = bomb_of(w, id) else { return false };
    if dead_of(w, id) || b.state == ATTACHED || b.no_collision {
        return false;
    }
    out.push(WorldSphere { x: b.x, y: b.y, r: b.attach_r * scale_of(w, id), ..WorldSphere::default() });
    true
}

// ---- the calls a buff's graph makes on the bomb (CarryHooks: Buff_ProjectileThroughTunnel's, Buff_C015HitProjectile's)

fn carry_id(a: ActorRef) -> ActorId {
    match a {
        ActorRef::Actor(id) => id,
        _ => unreachable!("a HarleyBomb carry hook is called with its own actor"),
    }
}

/// IsSimulatingPhysics() of the root (the Capsule while free; Turn Off Physics at the attach @937); its
/// ProjectileMovement is deactivated.
fn carry_moving(w: &World, a: ActorRef) -> bool {
    bomb_of(w, carry_id(a)).is_some_and(|b| b.state == FLY)
}

fn carry_at(w: &World, a: ActorRef) -> V2 {
    bomb_of(w, carry_id(a)).map_or(V2 { x: 0.0, y: 0.0 }, |b| V2 { x: b.x, y: b.y })
}

/// K2_SetActorLocation(teleport): the body moves with it, its velocity kept; on a carrier, the next attach place
/// reads the carrier again (the bomb's step), the buff's tick behind it places it once more.
fn carry_place(w: &mut World, a: ActorRef, x: Real, y: Real) {
    if let Some(b) = bomb_mut(w, carry_id(a)) {
        b.x = x;
        b.y = y;
    }
}

/// GetVelocity(): the root's physics velocity (the deactivated ProjectileMovement's Velocity is taken as 0).
fn carry_velocity(w: &World, a: ActorRef) -> V2 {
    bomb_of(w, carry_id(a)).map_or(V2 { x: 0.0, y: 0.0 }, |b| V2 { x: b.vx, y: b.vy })
}

/// SetPhysicsLinearVelocity on the simulating root; nothing on an attached one.
fn carry_set_velocity(w: &mut World, a: ActorRef, vx: Real, vy: Real) {
    if let Some(b) = bomb_mut(w, carry_id(a))
        && b.state == FLY
    {
        b.vx = vx;
        b.vy = vy;
    }
}

/// SetActorEnableCollision: every component's collision (the Capsule's contacts, PlayerAttachCollision,
/// ExitedArenaCapsule, the HurtBox).
fn carry_collision(w: &mut World, a: ActorRef, _buff: u32, on: bool) {
    if let Some(b) = bomb_mut(w, carry_id(a)) {
        b.no_collision = !on;
    }
}

/// MaxLifetime 0 (SpawnedActorParent's, not saved by the class): no MaxLifetimeTimer to pause.
fn carry_pause_life(_w: &mut World, _a: ActorRef, _buff: u32, _on: bool) {}

// ---- the kind

/// The kind's `spawn`: the body's launch, the spawn push-out, then the parent's BeginPlay blast-box loop.
fn spawn(w: &mut World, f: FighterId, r: &SpawnRequest) -> (WorldActorState, bool) {
    let data = w.data.clone();
    let cls = w.names.str(r.cls);
    let a = crate::fighter::common::actor_data(w, &data, ActorRef::Fighter(f), r.cls)
        .unwrap_or_else(|| panic!("{cls}: no actor data for its spawn (the importer gives every spawned class)"));
    let np = request_props(w, f, r);
    let mut at = spawn_place(w, f, &np);
    // The spawn with collision handling 2 (Comp_Spawner @9054 / @9531, AdjustIfPossibleButAlwaysSpawn):
    // FindTeleportSpot moves the root Capsule (ProjectileNavPlatform, 60 / 60: a sphere) out of what it blocks (the
    // stage block and World.blockers('nav'): `root_body_blockers`) before BeginPlay; an attached spawn is set back by
    // Comp_Spawner (@10335-10581).
    let cap = obj(&a.components, "Capsule");
    let attached = np.get("AttachToActor").is_some_and(crate::registry::props::truthy);
    let boxes = if attached {
        None
    } else {
        root_body_blockers(w, cap.and_then(|c| obj(c, "BodyInstance")), r.cls, name!("spawn push-out"))
    };
    if let Some(boxes) = boxes {
        let shape = capsule_shape(cap.map(capsule_c), scale_x(cap), 0.0, 1.0);
        let spot = find_teleport_spot_shape(boxes.view(), at.x, at.y, &shape);
        at = spot;
    }
    let d = obj(&np, "InitialMovementDirection");
    let sp = num(&np, "InitialSpeed").unwrap_or(0.0);
    let dy = d.and_then(|d| num(d, "Y")).unwrap_or(0.0);
    let dz = d.and_then(|d| num(d, "Z")).unwrap_or(0.0);
    // InitialSpeed: Velocity.GetSafeNormal() x InitialSpeed
    let [_, ny, nz] = crate::math::ue::safe_normal([0.0, dy, dz], crate::math::ue::SMALL_NUMBER);
    let fac = facing(w, f);
    let (fvx, fvy) = (w.fighter(f).body.vx, w.fighter(f).body.vy);
    // HandleActorPostSpawn: the launch plus half the owner's velocity.
    let vx = (ny * sp) * fac + fvx / 2.0;
    let vy = (nz * sp) + fvy / 2.0;
    let pa = obj(&a.components, "PlayerAttachCollision");
    let pa_mesh = pa.and_then(|p| text(p, "AttachParent")).and_then(|n| obj(&a.components, n));
    let ex = obj(&a.components, "ExitedArenaCapsule");
    let ex_parent = ex.and_then(|p| text(p, "AttachParent")).and_then(|n| obj(&a.components, n));
    let mut b = Bomb {
        hit: HitPacket::from_props(&w.names, obj(&a.p, "HitBoxDamage").unwrap_or(&Props::new()), None),
        attack_id: r.attack_id,
        facing: fac,
        flip_on_attach: fac < 0.0,
        x: at.x,
        y: at.y,
        pvx: vx,
        pvy: vy,
        radius: cap.and_then(|c| num(c, "CapsuleRadius")).unwrap_or(0.0),
        attach_r: pa.and_then(|p| num(p, "SphereRadius")).unwrap_or(0.0) * scale_x(pa) * scale_x(pa_mesh),
        exit_r: ex.and_then(|p| num(p, "CapsuleRadius")).unwrap_or(32.0) * scale_x(ex) * scale_x(ex_parent),
        ..Bomb::default()
    };
    // The parent's BeginPlay loop; its DestroySpawnedActor(true) is a silent K2_DestroyActor (ReceiveDestroyed @3682
    // only unbinds): no detonation.
    let doe = flag(&a.p, "DestroyOnExitedArena?");
    let sb = spawn_box(w);
    let gone = blast_box_loop(
        &sb,
        &mut b.x,
        &mut b.y,
        None,
        flag(&a.p, "ForceSpawnPositionInsideBlastBox"),
        &mut b.blast_bounds,
        doe,
    );
    (WorldActorState::HarleyBomb(b), gone)
}

/// The bomb's tick.
fn step(w: &mut World, id: ActorId) {
    let f = owner_of(w, id);
    let k = scale_of(w, id);
    let doe = class_destroy_on_exit(w, id);
    let p = actor_p(w, ActorRef::Actor(id));
    let rate = num(p, "GravityIncrementRate").unwrap_or(0.05);
    let terminal = num(p, "TerminalVelocity").unwrap_or(0.0);
    let increment = num(p, "GravityIncrementValue").unwrap_or(0.0);
    let fid = fighter_id(w, f);
    let key = fuse_key(w, id);
    let Some(b) = bomb_mut(w, id) else { return };
    b.age += 1;
    let age = b.age;
    // The actor's latent actions.
    if crate::real(age) == latent_frames(0.0667) + latent_frames(0.2) {
        b.armed = true;
        w.events.push(Event::SoundStart {
            fighter: fid,
            ev: name!("sfx_shared_class_assassin_satchel_fuse_loop"),
            key,
        });
    }
    if crate::real(age) >= latent_frames(5.0) {
        detonate(w, id);
        return;
    }
    let Some(b) = bomb_mut(w, id) else { return };
    if b.poll_at >= 0 && age >= b.poll_at {
        b.poll_at = -1;
        owner_test(w, id);
    }
    let Some(b) = bomb_of(w, id) else { return };
    let (state, carrier) = (b.state, b.carrier);
    if let (ATTACHED, Some(c)) = (state, carrier) {
        if w.fighter(c).life.dead > 0.0 {
            if let Some(b) = bomb_mut(w, id) {
                b.left_arena = true;
            }
            detonate(w, id);
            return;
        }
        if bomb_of(w, id).is_some_and(|b| facing(w, c) != b.carrier_facing) {
            attach_to(w, id, c);
        }
        let (cx, cy) = (w.fighter(c).body.x, actor_y(w, c));
        let Some(b) = bomb_mut(w, id) else { return };
        b.x = cx;
        b.y = cy + b.rel_y;
        // The jump-out area's end overlap with ExitedArenaCapsule (on a team mate only): HandleBlastBoxExit(false,
        // true) -> HandlePlayerKnockout -> Comp_Actor_Child.ExitedArena @905 -> @5224.
        if b.ally && !b.no_collision {
            let was = b.in_area;
            let now = in_jump_out_area(w, id);
            let Some(b) = bomb_mut(w, id) else { return };
            b.in_area = now;
            if was && !now {
                b.left_arena = true;
                detonate(w, id);
                return;
            }
        }
    } else if state == FLY {
        fly(w, id);
    }
    // The timers, behind the step.
    let Some(b) = bomb_mut(w, id) else { return };
    let t = crate::consts::clock(crate::real(age - 1));
    if t > b.grav_at {
        b.grav_at += f64::from(rate);
        if b.state == FLY && b.vy > terminal {
            b.vy += increment;
        }
    }
    if t > b.blast_at {
        b.blast_at += f64::from(0.1_f32);
        // CheckBlastBoxExit's DestroySpawnedActor(true): no Detonate, no blast (the fuse loop stops in `end`).
        if b.destroy_on_exit(doe) && check_blast_box_exit(b.x, b.y, &b.blast_bounds) {
            if let Some(a) = w.actors.actors.get_mut(id) {
                a.dead = true;
            }
            return;
        }
    }
    if b.state == ATTACHED || b.no_collision {
        return;
    }
    // PlayerAttachCollision's overlaps after the move.
    for o in match_fighters(w) {
        let oid = fighter_id(w, o);
        let Some(b) = bomb_of(w, id) else { return };
        let (bx, by, r) = (b.x, b.y, b.attach_r * k);
        let at = b.inside.iter().position(|&i| i == oid);
        if !touches(w, o, bx, by, r, true) {
            if let (Some(at), Some(b)) = (at, bomb_mut(w, id)) {
                b.inside.remove(at);
            }
            continue;
        }
        if at.is_some() {
            continue;
        }
        if let Some(b) = bomb_mut(w, id) {
            b.inside.push(oid);
        }
        if o != f {
            try_attach(w, id, o);
        } else {
            if let Some(b) = bomb_mut(w, id) {
                b.poll_for = Some(o);
            }
            owner_test(w, id);
        }
        if bomb_of(w, id).is_some_and(|b| b.state == ATTACHED) {
            return;
        }
    }
}

/// However it ended (the owner's defeat: Die With Instigator): the fuse loop goes with the actor.
fn end(w: &mut World, id: ActorId) {
    let armed = bomb_of(w, id).is_some_and(|b| b.armed);
    if armed {
        let (fid, key) = (fighter_id(w, owner_of(w, id)), fuse_key(w, id));
        w.events.push(Event::SoundStop { fighter: fid, key, fade: Some(0.0), ev: None, curve: None, cut: false });
    }
    if let Some(b) = bomb_mut(w, id) {
        b.armed = false;
    }
}

/// This file's registry entries (HarleyBomb_C's kind and carry hooks).
pub fn register(r: &mut Registry) -> Result<(), RegistryError> {
    let hooks = WorldActorHooks {
        spawn: Some(spawn),
        // The Capsule's sphere, the attach sphere and the HurtBox at the root's scale (the view reads it too).
        // Unverified: the rigid body's mass stays MASS (PhysX's scaled shape with the body's mass as set).
        size: Some(SizeHooks {
            scale: Some(|w, id, s, old| {
                if let Some(b) = bomb_mut(w, id) {
                    b.radius *= s / old;
                }
            }),
            ..SizeHooks::default()
        }),
        bodies: Some(bodies),
        hurt_box: Some(hurt_box),
        hit_shapes: Some(hit_shapes),
        blocked: Some(|w, id, _by| detonate(w, id)),
        activate: Some(detonate),
        any_damage: Some(|w, id, _damage, _by| detonate(w, id)),
        step: Some(step),
        end: Some(end),
        ..WorldActorHooks::default()
    };
    let carry = CarryHooks {
        moving: carry_moving,
        team: owner_team,
        at: carry_at,
        place: carry_place,
        velocity: carry_velocity,
        set_velocity: carry_set_velocity,
        collision: carry_collision,
        pause_life: carry_pause_life,
        tick: None,
    };
    let entry = ActorEntry {
        carry: Some(carry),
        ..ActorEntry::new(ActorClass::World(WorldActorClass::HarleyBomb), ActorHooks::World(hooks))
    };
    r.actors.enter(BOMB, entry)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_curve_holds_its_end_keys_and_runs_linear_between() {
        assert_eq!(blast_curve(-1.0), CURVE[0].1);
        assert_eq!(
            blast_curve(0.0),
            (CURVE[0].1 + ((0.0 - CURVE[0].0) / (CURVE[1].0 - CURVE[0].0)) * (CURVE[1].1 - CURVE[0].1))
        );
        assert_eq!(blast_curve(1.0), CURVE[2].1);
        assert_eq!(blast_curve(5.0), CURVE[2].1);
        // between the second and third keys the value falls from 1 to 0.45
        let mid = blast_curve(0.87);
        assert!(mid < 1.0 && mid > 0.45, "{mid}");
        // NaN falls through to the last key, as the loop
        assert_eq!(blast_curve(Real::NAN), CURVE[2].1);
    }

    #[test]
    fn a_new_bomb_starts_where_the_ts_class_does() {
        let b = Bomb::default();
        assert_eq!(b.poll_at, -1);
        assert_eq!(b.state, FLY);
        assert_eq!(b.grav_at, f64::from(0.05_f32));
        assert_eq!(b.blast_at, f64::from(0.1_f32));
        assert!(b.destroy_on_exit(true));
    }

    #[test]
    fn the_destroy_flag_follows_the_attach() {
        let mut b = Bomb { state: ATTACHED, ally: false, ..Bomb::default() };
        assert!(!b.destroy_on_exit(true));
        b.ally = true;
        assert!(b.destroy_on_exit(false));
    }

    #[test]
    fn blast_numbers_leave_a_packet_of_the_sides_launch() {
        let n = Blast { damage: 2.0, base_knock_back: 3.0, kx: 4.0, ky: 5.0 };
        let p = blast_packet(&n, false);
        assert_eq!(p.knock_back_direction_vector, Some(DataVec { x: Some(4.0), y: Some(5.0), z: None }));
        assert_eq!(p.do_los_check, Some(false));
        let d = blast_props(&n, true, &[]);
        assert_eq!(d.get("DoLOSCheck"), Some(&Value::Bool(true)));
        assert!(d.get("OnHitBuffs").is_none());
        let row = Props::new();
        let d = blast_props(&n, true, &[row]);
        assert!(d.get("OnHitBuffs").is_some());
    }

    #[test]
    fn a_component_scale_defaults_to_one() {
        assert_eq!(scale_x(None), 1.0);
        let mut c = Props::new();
        let mut s = Props::new();
        s.insert("X".into(), Value::Num(2.25));
        c.insert("RelativeScale3D".into(), Value::Obj(s));
        assert_eq!(scale_x(Some(&c)), 2.25);
    }

    #[test]
    fn the_nowhere_floor_is_below_everything() {
        let s = nowhere();
        assert!(s.g.top < -1e8 && s.g.bottom < s.g.top);
    }
}
