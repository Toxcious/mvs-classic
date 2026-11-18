//! Stripe's dash grab, skateboard, chainsaw and boombox: C023B_DashListener
//! ([`StripeListener`], a world actor), C023B_DashProjectileBlock ([`BlockExt`], a buff), BoomBox_SpawnedActor
//! ([`BoomBox`], a projectile), LongLeap / NoLongLeap, ChainsawAttackHitCondition, FaceTarget, MoveToActorHit,
//! Comp_C023B_EnemyAttach's live part; joined to hitEnemy. The perks' listeners of the attach are perks.rs's
//! (`attached_to_enemy`, its onAttachedToEnemy).
//!
//! C023B_DashListener (BeginPlay binds the owner's HitEnemyPlayer @681-704; the projectile overlap DoOnce @20-50 ->
//! BeforeLeapInitiated @112, the section jump to Launch @233, the 0.1 s hit pause @354, the overlapped actor destroyed
//! @446-538; HitEnemy -> SendAttachedDispatch @972). C023B_DashProjectileBlock, C023B_LongLeap_Condition /
//! _NoLongLeap_Condition, AnimNotify_C023B_MoveToActorHit (Stripe's own: the +-350 offsets and the [0, 8] trace);
//! ChainSaw -> Chainsaw_Cut (C023B_ChainsawAttackHitCondition, which writes PreviousAttackCancelled; OnWall is shared:
//! `combat::conditions`); AnimNotify_C023B_FaceTarget; Chainsaw_Cut's ActivateSpawned at f0 finds no actor: the shared
//! notify's no-op, nothing here; BoomBox_SpawnedActor (DThrow); Comp_C023B_EnemyAttach's live part (BeginPlay,
//! SendAttachedDispatch -> AttachedToEnemy @10; its BeginPlay @203 only sets OwningFighter, which nothing here needs).
//!
//! The DashGrab montages' Spawner (t 0.0001, 0.7368 s, Queued, AttachToActor at the owner's root with no offset,
//! DestroyAllActorsOnNotifyEnd) makes the listener, Die With Instigator; the world's notify spawn path sends
//! AfterSpawnedActor to the owner's buffs (Buff_C023B_Leap, perks.rs). Its data: LeapIgnoreProjectileTypes [2, 3, 4]
//! (Light, Radial, Transcendent); the root Capsule (no collision profile) and the HitBox sphere (radius 200, object
//! type HitBox, Custom: Overlap to WorldStatic, WorldDynamic, Pawn, Visibility, Camera, PhysicsBody, Vehicle,
//! Destructible, WallJump, WorldStaticDetection, DamageDestructible, Platform; every other channel its default: HurtBox
//! and ProjectileDestructible Ignore, Item Overlap).
//! - ReceiveBeginPlay @600: the owner's Comp_Pawn_HitBoxHandler.HitEnemyPlayer += HitEnemy @681-704; ReceiveEndPlay
//!   @746 removes it. HitEnemy @901 -> the owner's Comp_C023B_EnemyAttach.SendAttachedDispatch(HitPlayer) @972: the
//!   joined hit_enemy below, for each live listener of the owner.
//! - The HitBox's OnComponentBeginOverlap @1018 -> the leap gate (`leaps_off`), then the DoOnce @20-50 (never reset:
//!   one per listener) -> `leap(team)` and, the other's Destructible TeamIndex not the listener's @446-538,
//!   K2_DestroyActor on it. By its responses the sphere meets no fighter's hurt box and no projectile's HurtBox or
//!   HitBox (both Ignore): what it can meet is a body of the types above whose own template overlaps the HitBox channel
//!   (an item's capsule, SeismicReactor's DamageBox, C019_MortarAim's Hurtbox, the heal grenade's DamageCollision, ...).
//! - Its own hit box handler deals nothing: HitBoxOn? is true (SpawnedActorParent's default) and the HitBoxDamage
//!   carries DestroyProjectiles over 0 damage, but UpdateHitbox -> HitBoxesOn @8531 turns on only the components that
//!   carry a tag of HitBoxTags ([HitBox]: GetHitBoxes, GetComponentsByTag), and no component of the class or of
//!   SpawnedActorParent has a tag. So no pfgDoDamage and no projectile destroyed by it.
// Deviation: the begin overlap is taken at the listener's tick (its sphere at the owner's place as the frame left it),
// not at the move that made it; an actor already inside at the spawn counts (UpdateOverlaps at registration).
// Deviation: the sphere's overlaps are the trace module's bodies (the types it gathers, and the template's response to
// HitBox as `responder`); a HitBox body, and Vehicle, Destructible and WorldStaticDetection bodies, are not gathered.
// Unverified: the build's Destructible TeamIndex is taken as `actor_by`'s team (the actor's owner's).

use super::gun::{loc, tracker_hit_enemy};
use super::perks::{LEAP_CLASS, attached_to_enemy};
use super::state::{stripe_mut, stripe_of};
use crate::Real;
use crate::State;
use crate::actors::projectile::{ProjectileExt, set_hitbox};
use crate::actors::world_actor::actor_p;
use crate::actors::{ActorClass, ActorId, ActorRef, ProjectileClass, ProjectileId, WorldActorClass, WorldActorState};
use crate::combat::BuffClass;
use crate::combat::buffs::{BuffExt, BuffRef, buff_mut, buff_of, buff_signal};
use crate::combat::hit::knockback::timer_count;
use crate::combat::hit::set_hit_pause;
use crate::events::NotifyRef;
use crate::fighter::anim::{active_instance, slot_jump_to_section};
use crate::fighter::common::{actor_by, latent_frames, sound_at_for};
use crate::fighter::control::DodgeBy;
use crate::fighter::match_side::SpawnRequest;
use crate::fighter::moves::{note_attack_cancelled, sweep_move};
use crate::fighter::traces::{
    OBJ_DAMAGE_DESTRUCTIBLE, OBJ_ITEM, OBJ_PAWN, OBJ_PHYSICS_BODY, OBJ_PLATFORM, OBJ_WORLD_DYNAMIC, OBJ_WORLD_STATIC,
    TRACE_PROJECTILE_GROUND, TraceHit, box_trace_single, capsule_trace_multi_for_objects, sphere_overlap_actors,
};
use crate::fighter::{FighterId, facing, signal_actors, signal_bound};
use crate::math::to_int32;
use crate::name;
use crate::names::Name;
use crate::registry::entries::{ActorEntry, ActorHooks, BuffEntry, BuffHooks, ProjectileHooks, WorldActorHooks};
use crate::registry::entries::{Condition, NotifyHandler};
use crate::registry::props::arr;
use crate::registry::signals::{BuffSignal, BuffSignalArg, FighterSignal, OwnerSignal};
use crate::registry::{Registry, RegistryError};
use crate::world::World;
use crate::world::spawning::k2_destroy_actor;
use crate::world::wall_bp::{WallActor, bind_ground_destroyed, wall_delegate_fns};

/// The dash listener's class.
pub const DASH_LISTENER: &str = "C023B_DashListener_C";
/// HitBox, the object type the listener's sphere is of (the `responder` of its overlap query).
const OBJ_HIT_BOX: Real = 7.0;
/// `NO_TYPES`: the class default LeapIgnoreProjectileTypes of the listener and of the block.
const NO_TYPES: [i32; 3] = [2, 3, 4];

/// `Listener` (C023B_DashListener_C): the DoOnce of its overlap (@20-50).
#[derive(State, Debug, Default)]
pub struct StripeListener {
    pub done: bool,
}

/// `BoomBox extends Projectile` (BoomBox_SpawnedActor_C): its latent counts in frames (0: none pending).
#[derive(State, Debug, Default)]
pub struct BoomBox {
    /// The Delay(12) to GroundDestroyed.
    pub ground_due: i32,
    /// GroundDestroyed's DoOnce / HasHit.
    pub has_hit: bool,
    /// Delay(TriggerTime).
    pub detonate_due: i32,
    /// Delay(0.15).
    pub off_due: i32,
    /// Delay(0.7).
    pub kill_due: i32,
    /// What the view reads of the Detonate calls (EffectMulticast's emitter, the ShakeAnim timeline, the BaseColor
    /// write): how many there were, and the box's age at the last.
    pub detonations: i32,
    pub detonate_age: i32,
    /// The world tick of the BeginPlay (the ride starts the frame after; the ride is not ported, the field is the
    /// record's).
    pub ride_from: i32,
}

/// C023B_DashProjectileBlock_C's extension (`{ closed }`): the DoOnce of its OnHitByEnemy.
#[derive(State, Debug, Default)]
pub struct BlockExt {
    pub closed: bool,
}

// ---------------------------------------------------------------- the leap gate (listener and block)

/// The test both classes make on the other actor (@1018-1443 / @1147-1572): its Comp_Actor_Destructible valid, and
/// !LeapIgnoreProjectileTypes.Contains(its BPI_GetProjectileAttackType (0 when the BPI_SpawnedActor cast fails)) AND
/// its Destructible's IsProjectile. `actor_by` gives the Destructible's IsProjectile (false for a fighter or an actor
/// with no Destructible) and the attack type.
fn leaps_off(by: &DodgeBy, ignore: &[i32]) -> bool {
    by.is_projectile && !ignore.contains(&by.attack_type)
}

/// The DoOnce's body (@50-395, the same in both classes): broadcast BeforeLeapInitiated(the other's Destructible
/// TeamIndex) @112 (Buff_C023B_Leap binds it: the owner's buffs get the signal); the owner's
/// Comp_Pawn_MontageNavigation.NotifyChangeSection(Launch, null) @233 (JumpToSectionLogic with no montage:
/// Montage_JumpToSection(Launch, the current active montage)); the owner's Comp_HitPause.SetHitPauseTime(0.1) @354.
/// The DashGrab montages name the section `launch` (t 1.1833); section names compare without case, as FName does.
fn leap(w: &mut World, f: FighterId, team: i32) {
    buff_signal(w, f, BuffSignal::BeforeLeapInitiated, BuffSignalArg { team: Some(team), ..BuffSignalArg::default() });
    if let Some(m) = active_instance(w, f) {
        slot_jump_to_section(w, f, m, name!("Launch"));
    }
    set_hit_pause(w, f, crate::real(timer_count(0.1)), 0.1);
}

// ---------------------------------------------------------------- C023B_DashListener

/// The overlapped object types the trace module gathers (Vehicle, Destructible and WorldStaticDetection it does not
/// gather: their bodies are left out, the Deviation above).
fn listener_types() -> [Real; 7] {
    [OBJ_WORLD_STATIC, OBJ_WORLD_DYNAMIC, OBJ_PAWN, OBJ_PHYSICS_BODY, OBJ_DAMAGE_DESTRUCTIBLE, OBJ_PLATFORM, OBJ_ITEM]
        .map(crate::real)
}

fn listener_spawn(_w: &mut World, _f: FighterId, _r: &SpawnRequest) -> (WorldActorState, bool) {
    (WorldActorState::StripeListener(StripeListener::default()), false)
}

/// The owner's `dashListener` is this actor (the kind's `spawn`).
fn listener_spawned(w: &mut World, id: ActorId) {
    let Some(a) = w.actors.actors.get(id) else { return };
    let o = a.owner;
    if let Some(s) = stripe_mut(w, o) {
        s.dash_listener = Some(ActorRef::Actor(id));
    }
}

fn listener_end(w: &mut World, id: ActorId) {
    let Some(a) = w.actors.actors.get(id) else { return };
    let o = a.owner;
    if let Some(s) = stripe_mut(w, o)
        && s.dash_listener == Some(ActorRef::Actor(id))
    {
        s.dash_listener = None;
    }
}

fn listener_step(w: &mut World, id: ActorId) {
    let Some(a) = w.actors.actors.get(id) else { return };
    if matches!(&a.state, WorldActorState::StripeListener(l) if l.done) {
        return;
    }
    let (o, scale) = (a.owner, a.size_scale.unwrap_or(1.0));
    let team = w.fighter(o).team.team;
    let ignore: Vec<i32> = {
        let p = actor_p(w, ActorRef::Actor(id));
        if p.contains_key("LeapIgnoreProjectileTypes") {
            arr(p, "LeapIgnoreProjectileTypes")
                .iter()
                .filter_map(|v| v.as_f64())
                .map(crate::math::dbl::to_int32)
                .collect()
        } else {
            NO_TYPES.to_vec()
        }
    };
    let (ox, oy) = (w.fighter(o).body.x, crate::fighter::actor_y(w, o));
    let found =
        sphere_overlap_actors(w, ox, oy, 200.0 * scale, &listener_types(), &[Some(ActorRef::Fighter(o))], OBJ_HIT_BOX);
    for a in found {
        let fighter = if let ActorRef::Fighter(x) = a { Some(x) } else { None };
        let by = actor_by(w, a, fighter);
        if !leaps_off(&by, &ignore) {
            continue;
        }
        if let Some(WorldActorState::StripeListener(l)) = w.actors.actors.get_mut(id).map(|x| &mut x.state) {
            l.done = true;
        }
        leap(w, o, by.team);
        if by.team != team {
            k2_destroy_actor(w, a); // @446-538
        }
        return;
    }
}

/// The owner's live listeners (his spawned actors of the class, not dead).
fn live_listeners(w: &World, f: FighterId) -> usize {
    w.fighter(f)
        .items
        .spawned_actors
        .iter()
        .filter(|a| {
            let ActorRef::Actor(id) = a else { return false };
            w.actors.actors.get(*id).is_some_and(|x| !x.dead && matches!(x.state, WorldActorState::StripeListener(_)))
        })
        .count()
}

// ---------------------------------------------------------------- C023B_DashProjectileBlock

// The DashGrab montages' ApplyBuff (t 0.0561, 0.662 s, BuffDuration 0, DestroyBuffsOnNotifyEnd). Its data: BuffDuration
// 5, no mods; LeapIgnoreProjectileTypes (the class default, [2, 3, 4] as the listener's).
// - ReceiveBeginPlay @421: the owner's Comp_Pawn_DamageManager valid -> HasProjectileArmor = true @617 (PF_Post_Event
//   (null) @670 posts nothing). Authority: DamageManager.HitByEnemy and HitByAlly += OnHitByEnemy @896-1105.
// - OnHitByEnemy(Attacker) @1147: the leap gate on the Attacker (the hit's DamageDealer: the projectile for a
//   projectile's hit) -> the DoOnce (@15-50, one per buff actor) -> leap(team). No destroy here. The HitByAlly binding
//   (DamageHitByAlly: DamageAndLaunch @8047, a team mate's projectile on Stripe, CanBeHitBy refusing it) reaches the
//   same handler: the leap with the team mate's TeamIndex, which Buff_C023B_Leap takes as LaunchNextLeap.
// - ReceiveEndPlay @1602: HasProjectileArmor = false (a plain write), both unbound.

fn block_begin(w: &mut World, f: FighterId, b: BuffRef, _from: Option<FighterId>, _dur: Option<Real>) {
    w.fighter_mut(f).invuln.projectile_armor = true;
    if let Some(x) = buff_mut(w, b) {
        x.ext = BuffExt::DashProjectileBlock(BlockExt::default());
    }
}

fn block_on(w: &mut World, f: FighterId, b: BuffRef, sig: BuffSignal, a: &BuffSignalArg) -> bool {
    if !matches!(sig, BuffSignal::HitByEnemy | BuffSignal::DamageHitByAlly) {
        return false;
    }
    let Some(by) = &a.by else { return false };
    let Some(x) = buff_mut(w, b) else { return false };
    if !matches!(x.ext, BuffExt::DashProjectileBlock(_)) {
        x.ext = BuffExt::DashProjectileBlock(BlockExt::default());
    }
    let BuffExt::DashProjectileBlock(e) = &mut x.ext else { return false };
    if e.closed || !leaps_off(by, &NO_TYPES) {
        return false;
    }
    e.closed = true;
    leap(w, f, by.team);
    false
}

fn block_end(w: &mut World, f: FighterId, _b: BuffRef) {
    w.fighter_mut(f).invuln.projectile_armor = false;
}

// ---------------------------------------------------------------- the conditions

/// GetUseLeap of the owner's Buff_C023B_Leap: its LaunchNextLeap; none without the buff.
fn use_leap(w: &World, f: FighterId) -> Option<bool> {
    let s = stripe_of(w, f)?;
    buff_of(w, f, LEAP_CLASS)?;
    Some(s.perks.launch_next_leap)
}

/// C023B_LongLeap_Condition Check: a BaseFighter, BPI_CheckIfHasBuff(Buff_C023B_Leap) -> cast -> GetUseLeap; any
/// failure (no buff included) false.
fn long_leap(w: &mut World, f: FighterId) -> bool {
    use_leap(w, f) == Some(true)
}

/// C023B_NoLongLeap_Condition Check: NOT GetUseLeap; any failure true.
fn no_long_leap(w: &mut World, f: FighterId) -> bool {
    use_leap(w, f) != Some(true)
}

/// C023B_ChainsawAttackHitCondition Check: the cast to Pawn_C023B fails -> false, nothing written. Else v = LastDecay
/// == 'Chainsaw' AND LandedAttackIDs.Contains(BPI_GetPlayerAttackCount) (a failed PlayerState interface gives 0);
/// BranchManager.PreviousAttackCancelled = v; returns v.
fn chainsaw_hit(w: &mut World, f: FighterId) -> bool {
    let Some(s) = stripe_of(w, f) else { return false };
    let d = &w.fighter(f).decay;
    let v = s.last_decay == name!("Chainsaw") && d.landed_attack_ids.contains(&d.attack_count);
    note_attack_cancelled(w, f, v)
}

// ---------------------------------------------------------------- the notifies

/// AnimNotify_C023B_FaceTarget Received_NotifyBegin: BPI_SetFacingLogic(BPI_IsFacingLeft) @288, his own facing again.
/// SetRotationLogic sets the same rotation and broadcasts FacingChanged whatever the change (@138804 / @139018);
/// `set_facing` returns at once for an unchanged value, so the broadcast is made here.
fn face_target_begin(w: &mut World, f: FighterId, _n: NotifyRef, _key: i32) {
    let left = facing(w, f) < 0.0;
    signal_actors(w, f, OwnerSignal::Facing, left);
    signal_bound(w, f, FighterSignal::Facing, left, 0.0, Name::NONE, None);
}

/// AnimNotify_C023B_MoveToActorHit Received_NotifyBegin (authority): left = BPI_IsFacingLeft @219; the owner's root
/// capsule (BPI_GetRootCapsule @386: its CapsuleRadius / CapsuleHalfHeight properties, unscaled: `cap_radius_u` /
/// `cap_half_u`, also while a size buff scales the pawn); L = its HitBoxHandler.MostRecentHitActor's location @651; end
/// = L + (0, left ? -350 : 350, 0) @757 (UE Y: sim x + 350 facing left, - 350 facing right);
/// CapsuleTraceSingleForObjects(L, end, radius, half height / 1.5, [WorldStatic, DamageDestructible], ignore self)
/// @847; the point: the hit's Location or end @1658, its X (depth) 0 @1759; BPI_SetActorLocation(point, sweep,
/// teleport) @1810 (`sweep_move`). Returns false.
/// Unverified: a HitActor that is no fighter or projectile reads (0, 0) (`loc`); none: the origin (the null read's zero
/// vector).
fn move_to_actor_hit_begin(w: &mut World, f: FighterId, _n: NotifyRef, _key: i32) {
    if stripe_of(w, f).is_none() {
        return;
    }
    let left = facing(w, f) < 0.0;
    let h = w.fighter(f).decay.recent_hit_actor;
    let (hx, hy) = loc(w, h);
    let ex = hx + if left { 350.0 } else { -350.0 };
    let (r, half) = (w.fighter(f).capsule.cap_radius_u, w.fighter(f).capsule.cap_half_u);
    let types = [crate::real(OBJ_WORLD_STATIC), crate::real(OBJ_DAMAGE_DESTRUCTIBLE)];
    let mut hits: Vec<TraceHit> = Vec::new();
    capsule_trace_multi_for_objects(
        w,
        hx,
        hy,
        ex,
        hy,
        r,
        half / 1.5,
        &types,
        &[],
        Some(ActorRef::Fighter(f)),
        &mut hits,
    );
    let (px, py) = hits.first().map_or((ex, hy), |t| (t.x, t.y));
    sweep_move(w, f, px, py);
}

// ---------------------------------------------------------------- BoomBox_SpawnedActor

// C023B_DThrow_Montage's Spawner (t 0.1058, unattached, Transforms (0, 100, -50), no speed) and ActivateSpawned
// (ActivateLastObject, t 0.3769). Its data: HitBoxOn? false, HitBoxTags [HitBox], HitBoxDamage 5 / 1400 / 17.5 /
// (0, 1), HitReaction 4, HitPauseTime 0, HitEachActorOnlyOnce, NumberOfActorsAllowed 1, TriggerTime 0.05,
// EnableProjectileMovement, MaxLifetime 2, Supports2DHitboxes; ProjectileMovement bShouldBounce, Bounciness 0;
// Comp_Actor_Destructible IsProjectile; Fireproof / Iceproof passives.
// - ReceiveBeginPlay @1291: the owner facing left -> actor scale Y x -1 (the drawing's); the mesh's dynamic material
//   (the drawing's); BoxTraceSingle(location - (0, 0, 50), - (0, 0, 150), half (100, 100, 0), ProjectileGroundTrace,
//   ignore self) @1975 -> K2_AttachToActor(the hit actor, KeepWorld) @2300 and, a DestructibleObjectBP, its
//   ObjectDestroyed += GroundDestroyed @2604; then Delay(12) @2645 -> GroundDestroyed @165. MaxLifetime 2 ends the
//   box long before the 12 s Delay, which is kept all the same.
// Deviation: the attach is not ported (the ground trace is made for the ObjectDestroyed bind alone): the attach
// rides the box on the hit body's own move (a moving stage platform, a stage solid, a spawned box, a ship), and the
// port has no per-frame move of a floor to add (the `TraceHit.floor` delta); a box thrown onto a moving floor
// falls through the frame's move of it.
// - GroundDestroyed @1125: DoOnce: HasHit = true, Mesh.PlayAnimation(null), Delay(TriggerTime) @1216 -> Detonate @109.
// - Detonate @2716: EffectMulticast @3204 (PS_C023B_BoomBox_Explosion on the Mesh at (0, 0, 50) with its Team
//   parameter, PF_Post_Event_At_Location(sfx_cha_C023b_dthrow_boombox_explode) at the Mesh); HitBoxOn? = true,
//   UpdateHitbox; Delay(0.15) @2760 -> HitBoxOn? = false, UpdateHitbox @124; Delay(0.7) @2815 -> K2_DestroyActor @150.
//   No gate: a second call posts the effect again and turns the hit box on again; a Delay already pending is not
//   started again (the latent manager keeps the first).
// - ActivateObject @3863 (the parent's first) -> Detonate. ReceiveAnyDamage @3063: MulticastDamageTaken (the
//   MouseTrap_SnapFast animation, the drawing's) and Detonate.
// - HitEnemyPlayer @3740 (no parent call): the owner's Comp_C023B_TargetTracker.HitEnemy(HitPlayer, null) (gun.rs
//   `tracker_hit_enemy`).
// - ReceiveTick @180: not HasHit -> a loop over OverlappingInvulnerableFighters, which nothing fills: nothing.
// The dealing shapes: tag HitBox: Capsule1 (radius 100, half height 180, pitch 90: along the depth, at (-10, 0, 56.4))
// and the HitBox sphere (130 at (-10, 0, 76.4)); tag HitBox2D (2D): Capsule1 and HitBox2D (radius 130, along the
// depth, at (-10, 0, 76)). On the play plane both sets are the 130 circle 76.4 up (Capsule1's circle lies inside it).
// Deviation: the -10 depth offset is dropped (as the play plane's 2D circles) and Capsule1 is taken as its section on
// the plane.

/// The hit circle's height over the actor's place (`hitY`: `actors::projectile::hit_xy`).
pub const HIT_Z: Real = 76.44884;
/// The Mesh's RelativeLocation Z under the Capsule (the actor's rotation is a yaw: the Z is the world's, at the actor's
/// scale): PF_Post_Event_At_Location's place (Mesh.K2_GetComponentLocation @3607).
const MESH_Z: Real = -39.033_134;

fn boom_make(_w: &World, _f: FighterId, _r: &SpawnRequest) -> ProjectileExt {
    ProjectileExt::BoomBox(BoomBox::default())
}

fn bb(w: &mut World, id: ProjectileId) -> Option<&mut BoomBox> {
    match &mut w.actors.projectiles.get_mut(id)?.ext {
        ProjectileExt::BoomBox(b) => Some(b),
        _ => None,
    }
}

/// Detonate @2716. EffectMulticast @3204: SpawnEmitterAttached(PS_C023B_BoomBox_Explosion, Mesh, (0, 0, 50)) with Team
/// (the view's, on `detonations`: it follows the box and goes with it) and the explode sound at the Mesh @3657; then
/// ShakeAnimEvent @3735 -> @15: ShakeAnim.PlayFromStart and BoomBox_Mat's BaseColor (the view's, on `detonate_age`).
fn detonate(w: &mut World, id: ProjectileId) {
    let Some(p) = w.actors.projectiles.get(id) else { return };
    let (o, x, y, scale, age) = (p.owner, p.x, p.y, p.scale, p.age);
    if let Some(b) = bb(w, id) {
        b.detonations += 1;
        b.detonate_age = age;
    }
    sound_at_for(w, o, name!("sfx_cha_C023b_dthrow_boombox_explode"), x, y + (MESH_Z * scale));
    if let Some(p) = w.actors.projectiles.get_mut(id) {
        set_hitbox(p, true);
    }
    if let Some(b) = bb(w, id) {
        if b.off_due == 0 {
            b.off_due = to_int32(latent_frames(0.15));
        }
        if b.kill_due == 0 {
            b.kill_due = to_int32(latent_frames(0.7));
        }
    }
}

/// GroundDestroyed @1125: the DoOnce.
fn ground_destroyed(w: &mut World, id: ProjectileId) {
    let trigger = crate::registry::props::num(
        crate::actors::projectile::class_p(w, w.actors.projectiles.get(id).expect("a live projectile")),
        "TriggerTime",
    )
    .unwrap_or(0.05);
    if let Some(b) = bb(w, id) {
        if b.has_hit {
            return;
        }
        b.has_hit = true;
        b.detonate_due = to_int32(latent_frames(trigger));
    }
}

/// GroundDestroyed's bind name (`wall_delegate_fns`): GroundDestroyed @3058 -> @1125.
const GROUND_DESTROYED: &str = "BoomBox_SpawnedActor_C.GroundDestroyed";

fn ground_destroyed_bound(w: &mut World, a: ActorRef, _wall: WallActor) {
    if let ActorRef::Projectile(id) = a
        && w.actors.projectiles.get(id).is_some()
    {
        ground_destroyed(w, id);
    }
}

fn boom_begin(w: &mut World, id: ProjectileId) {
    // the ground trace @1975 and its ObjectDestroyed bind @2604 (attachGround)
    if let Some(p) = w.actors.projectiles.get(id) {
        let (x, y) = (p.x, p.y);
        let me = ActorRef::Projectile(id);
        let hit =
            box_trace_single(w, x, y - 50.0, x, y - 150.0, 100.0, 0.0, 100.0, TRACE_PROJECTILE_GROUND, &[], Some(me));
        bind_ground_destroyed(w, hit.as_ref(), me, GROUND_DESTROYED);
    }
    if let Some(b) = bb(w, id) {
        b.ground_due = to_int32(latent_frames(12.0));
    }
    let tick = w.flow.tick;
    if let Some(b) = bb(w, id) {
        b.ride_from = tick;
    }
}

fn boom_step(w: &mut World, id: ProjectileId) -> bool {
    let Some(b) = bb(w, id) else { return false };
    let mut ground = false;
    if b.ground_due > 0 {
        b.ground_due -= 1;
        ground = b.ground_due == 0;
    }
    if ground {
        ground_destroyed(w, id);
    }
    let Some(b) = bb(w, id) else { return false };
    let mut det = false;
    if b.detonate_due > 0 {
        b.detonate_due -= 1;
        det = b.detonate_due == 0;
    }
    if det {
        detonate(w, id);
    }
    let Some(b) = bb(w, id) else { return false };
    let mut off = false;
    if b.off_due > 0 {
        b.off_due -= 1;
        off = b.off_due == 0;
    }
    let mut kill = false;
    if b.kill_due > 0 {
        b.kill_due -= 1;
        kill = b.kill_due == 0;
    }
    if off && let Some(p) = w.actors.projectiles.get_mut(id) {
        set_hitbox(p, false);
    }
    kill
}

fn boom_hit_enemy(w: &mut World, id: ProjectileId, vic: FighterId) {
    if let Some(o) = w.actors.projectiles.get(id).and_then(|p| p.owner) {
        tracker_hit_enemy(w, o, vic);
    }
}

fn boom_any_damage(w: &mut World, id: ProjectileId, _dmg: Real, _by: ActorRef) -> bool {
    detonate(w, id);
    false
}

fn boom_activate(w: &mut World, id: ProjectileId) {
    detonate(w, id);
}

// ---------------------------------------------------------------- the pawn's hooks

/// dash.rs's part of the joined hitEnemy (after gun.rs's and bomb.rs's): each live listener of the owner makes the
/// SendAttachedDispatch (Comp_C023B_EnemyAttach -> AttachedToEnemy(HitPlayer) @10 -> perks.rs's listeners).
pub fn hit_enemy(w: &mut World, f: FighterId, victim: FighterId) {
    for _ in 0..live_listeners(w, f) {
        attached_to_enemy(w, f, victim);
    }
}

/// This file's registry entries (C023B_DashListener_C, BoomBox_SpawnedActor_C, C023B_DashProjectileBlock_C, the
/// notifies and conditions).
pub fn register(r: &mut Registry) -> Result<(), RegistryError> {
    let listener = WorldActorHooks {
        spawn: Some(listener_spawn),
        spawned: Some(listener_spawned),
        // nothing drawn, no body (its sphere is the HitBox type, which no trace gathers)
        bodies: Some(|_, _, _| {}),
        step: Some(listener_step),
        end: Some(listener_end),
        ..WorldActorHooks::default()
    };
    r.actors.enter(
        DASH_LISTENER,
        ActorEntry::new(ActorClass::World(WorldActorClass::C023BDashListener), ActorHooks::World(listener)),
    )?;
    r.actors.enter(
        "BoomBox_SpawnedActor_C",
        ActorEntry::new(
            ActorClass::Projectile(ProjectileClass::BoomBoxSpawnedActor),
            ActorHooks::Projectile(ProjectileHooks {
                make: Some(boom_make),
                begin: Some(boom_begin),
                step: Some(boom_step),
                activate: Some(boom_activate),
                any_damage: Some(boom_any_damage),
                hit_enemy: Some(boom_hit_enemy),
                ..ProjectileHooks::default()
            }),
        ),
    )?;
    r.buffs.enter("C023B_DashProjectileBlock_C", {
        let mut e = BuffEntry::new(
            BuffClass::C023BDashProjectileBlock,
            BuffHooks { begin: Some(block_begin), on: Some(block_on), end: Some(block_end), ..BuffHooks::default() },
        );
        e.damage_hit_by_ally = true;
        e
    })?;
    r.conditions.enter("C023B_LongLeap_Condition_C", Condition::Fn(long_leap))?;
    r.conditions.enter("C023B_NoLongLeap_Condition_C", Condition::Fn(no_long_leap))?;
    r.conditions.enter("C023B_ChainsawAttackHitCondition_C", Condition::Fn(chainsaw_hit))?;
    r.notifies.enter(
        "AnimNotify_C023B_FaceTarget_C",
        NotifyHandler { begin: Some(face_target_begin), ..NotifyHandler::default() },
    )?;
    r.notifies.enter(
        "AnimNotify_C023B_MoveToActorHit_C",
        NotifyHandler { begin: Some(move_to_actor_hit_begin), ..NotifyHandler::default() },
    )?;
    wall_delegate_fns(r, &[(GROUND_DESTROYED, ground_destroyed_bound)])
}
