//! Gizmo's music and piano: C023A_MusicPickup ([`MusicPickup`]),
//! C023A_Music_Projectile ([`MusicProjectile`], riding its ally), C023A_PianoNote ([`PianoNote`]), the buffs
//! C023A_Music_Buff / C023A_Music_DeBuff ([`MusicExt`]) and C023A_Buff_Crippled. No pawn hook.
//!
//! MusicProjectile's overrides of the shared projectile code are called from `actors::projectile` / `world_actor`:
//! [`hit_xy`], [`step_override`], [`attached_to`].
//!
//! ChargingAttack's ModifyMontageWithChargeMultiplier on Piano / Piano_Attached is the shared notify code's (the
//! instigator's ChargeMultiplier, 0.6 under the Backpack perk's ChargeSpeed buff).
//!
//! The class constructors' tails are the `construct` hooks. What a constructor computes once from class data
//! and never records (the pickup's sphere radii, the note's `bubbleBase`) is read from the class data where it is used,
//! so a state loaded from a record has it too.

use crate::Real;
use crate::State;
use crate::actors::projectile::{
    Projectile, ProjectileExt, StepEnd, class_p, expiry, set_capsule_collision, set_hitbox, update_max_lifetime,
};
use crate::actors::world_actor::{actor_cls, actor_dead, actor_owner, actor_place, world_actor_place_of};
use crate::actors::{ActorClass, ActorRef, ProjectileClass, ProjectileId};
use crate::combat::buffs::{buff, buff_mut, buff_stacks, remove_stack, silenced_pop, spawn_buff};
use crate::combat::elem::{actor_inst_def, clear_homing, set_homing, spawn_actor_buff};
use crate::combat::hit::knockback::timer_count;
use crate::combat::pop_text::pop_text_on;
use crate::combat::{BuffClass, BuffExt, BuffRef, DataVec, HitPacket};
use crate::events::{Event, UiStateOp, UiStateParams};
use crate::fighter::common::{actor_components, buff_def, lookup_owner, owner_team, peers};
use crate::fighter::traces::{
    OBJ_DAMAGE_DESTRUCTIBLE, OBJ_ITEM_DESTRUCTIBLE, OBJ_PAWN, OBJ_PHYSICS_BODY, OBJ_PLATFORM, OBJ_PORTAL,
    OBJ_PROJECTILE_DESTRUCTIBLE, OBJ_WORLD_DYNAMIC, OBJ_WORLD_STATIC, sphere_overlap_actors,
};
use crate::fighter::{FighterId, actor_y, facing};
use crate::math::min;
use crate::math::ue::clamped_to_size;
use crate::name;
use crate::names::Name;
use crate::registry::entries::{ActorBuffHooks, ActorEntry, ActorHooks, BuffEntry, BuffHooks, ProjectileHooks};
use crate::registry::props::{name_of, num};
use crate::registry::signals::{BuffSignal, BuffSignalArg};
use crate::registry::{Registry, RegistryError};
use crate::world::World;
use mvs_data::value::{Props, Value};

/// `MUSIC_PICKUP`.
pub const MUSIC_PICKUP: &str = "C023A_MusicPickup_C";
/// `MUSIC_PROJECTILE`.
pub const MUSIC_PROJECTILE: &str = "C023A_Music_Projectile_C";
/// `PIANO_NOTE`.
pub const PIANO_NOTE: &str = "C023A_PianoNote_C";
/// `MUSIC_BUFF`.
pub const MUSIC_BUFF: &str = "C023A_Music_Buff_C";
/// `MUSIC_DEBUFF`.
pub const MUSIC_DEBUFF: &str = "C023A_Music_DeBuff_C";
/// `CRIPPLED`.
pub const CRIPPLED: &str = "C023A_Buff_Crippled_C";

const GATE_DELAY_SEC: Real = 0.3; // ReceiveBeginPlay's Delay(0.3) @4057 -> @31
const LINGER: Real = 0.5; // FollowDamageLingerTimer: K2_SetTimerDelegate(TurnOffLingerDamage, 0.5) @6403 / @7528
const PICK_R: Real = 54.764446; // PickUpCheck.SphereRadius (DetectPawn)
const MAGNET_R: Real = 321.08163; // MagnetCheck.SphereRadius (DetectPawn)
const FRIEND_R: Real = 200.0; // FrendlyProjectileCheck.SphereRadius (Custom, no ObjectType: WorldDynamic)
const MAX_BUFF: Real = 3.0; // CheckHasBuff(Music_Buff) BuffStacks < 3 @1125 / @3307 / @5247

/// `MusicPickup extends Projectile` (C023A_MusicPickup_C): the fields the class adds.
#[derive(State, Debug, Default)]
pub struct MusicPickup {
    /// PS_C023_Damage's visibility: on at Follow projectile (@6039), off at LingerDamageMulticast (@7330).
    pub damage_fx: bool,
    /// PickupGate: the 0.3 s after the spawn are over (@123).
    pub pickup_gate: bool,
    /// IsHomingOnAlly.
    pub is_homing_on_ally: bool,
    /// CanStartFollowingProjectile (CDO true).
    pub can_start_following: bool,
    /// CanFollowProjectile (CDO true).
    pub can_follow: bool,
    /// HommingTargetActor.
    pub target: Option<ActorRef>,
    /// The target's OnEndPlay bound to FollowProjectileDeath (@6096, unbound @6506 / @7631).
    pub end_bound: bool,
    /// A C023A_PopCorn target's HasExploded bound to PopCornLaunch (@6273, unbound @7464).
    pub pop_bound: bool,
    /// FollowDamageLingerTimer: the tick TurnOffLingerDamage fires at, -1 for none.
    pub linger_at: i32,
    /// The overlaps of the three spheres as the last test left them (a begin overlap is a new entry): fighter ids
    /// for PickUpCheck and MagnetCheck, actors for FrendlyProjectileCheck.
    pub in_pick: Vec<i32>,
    pub in_magnet: Vec<i32>,
    pub in_friend: Vec<ActorRef>,
}

/// `MusicProjectile extends Projectile` (C023A_Music_Projectile_C): `ally`, the attach parent.
#[derive(State, Debug, Default)]
pub struct MusicProjectile {
    pub ally: Option<FighterId>,
}

/// `PianoNote extends Projectile` (C023A_PianoNote_C): the Bubble timeline.
#[derive(State, Debug)]
pub struct PianoNote {
    /// Bubble's position (s).
    pub bubble: Real,
    /// The tick of the spawn (the timeline's PlayFromStart).
    pub born_at: i32,
    /// HitBox2D's radius x the root's scale (`bubbleBase`, from the constructor).
    pub bubble_base: Real,
}

impl Default for PianoNote {
    /// The field initial values.
    fn default() -> Self {
        PianoNote { bubble: 0.0, born_at: -1, bubble_base: 0.0 }
    }
}

/// C023A_Music_Buff's and C023A_Music_DeBuff's extension (`MusicExt`): the Instigator.
#[derive(State, Debug, Default)]
pub struct MusicExt {
    pub from: Option<FighterId>,
}

// ---------------------------------------------------------------- access

fn pr(w: &World, id: ProjectileId) -> &Projectile {
    w.actors.projectiles.get(id).expect("a live projectile")
}

fn pm(w: &mut World, id: ProjectileId) -> &mut Projectile {
    w.actors.projectiles.get_mut(id).expect("a live projectile")
}

fn pk(w: &World, id: ProjectileId) -> &MusicPickup {
    match &pr(w, id).ext {
        ProjectileExt::MusicPickup(s) => s,
        _ => panic!("a music pickup"),
    }
}

fn pk_mut(w: &mut World, id: ProjectileId) -> &mut MusicPickup {
    match &mut pm(w, id).ext {
        ProjectileExt::MusicPickup(s) => s,
        _ => panic!("a music pickup"),
    }
}

// ---------------------------------------------------------------- C023A_MusicPickup_C

/// `GATE_DELAY` (`latentFrames(0.3)`).
fn gate_delay() -> i32 {
    crate::math::to_int32(crate::fighter::common::latent_frames(GATE_DELAY_SEC))
}

/// The pickup's `ProjectileMovement.HomingAccelerationMagnitude` (float32).
fn homing_mag(w: &World, id: ProjectileId) -> Real {
    let p = pr(w, id);

    p.owner
        .and_then(|o| actor_components(w, ActorRef::Fighter(o), p.cls))
        .and_then(|c| c.get("ProjectileMovement"))
        .and_then(Value::as_obj)
        .and_then(|c| num(c, "HomingAccelerationMagnitude"))
        .unwrap_or(0.0)
}

/// The radius of the boxes UpdateHitBoxDamage turned on (HitBoxScale (1, 1, 1)) x the root's scale: HitBox /
/// HitBox2D 48, with Follow / Follow2D 80 around the same centre (both on the Capsule at its origin).
fn box_radius(w: &World, id: ProjectileId, follow: bool) -> Real {
    let p = pr(w, id);
    let cs = p.owner.and_then(|o| actor_components(w, ActorRef::Fighter(o), p.cls));
    let radius = |capsule: &str, sphere: &str| -> Real {
        let c = |k: &str, f: &str| cs.and_then(|c| c.get(k)).and_then(Value::as_obj).and_then(|c| num(c, f));
        c(capsule, "CapsuleRadius").or_else(|| c(sphere, "SphereRadius")).unwrap_or(0.0)
    };
    let r = if follow { radius("HitBoxFollow2D", "HitBoxFollow") } else { radius("HitBox2D", "HitBox") };
    r * p.scale
}

/// UpdateMaxLifetimeWithAuthority(MaxLifetime) from the actor's tick (behind its move).
fn refresh_life(w: &mut World, id: ProjectileId) {
    let life = num(class_p(w, pr(w, id)), "MaxLifetime").unwrap_or(0.0);
    update_max_lifetime(pm(w, id), life, 0);
}

fn sound(w: &mut World, id: ProjectileId, ev: Name) {
    let p = pr(w, id);
    let e = Event::Sound { fighter: p.owner_id, ev, x: Some(p.x), y: Some(p.y), hit: None, pooled: false, key: None };
    w.events.push(e);
}

/// The ally test of PickedUp @4725-5281 and of the magnet's begin overlap @2718-3341: a BPI_BaseFighter other than
/// the owner, PickupGate, of the pickup's team, with fewer than 3 Music_Buff stacks.
fn ally_wants(w: &World, id: ProjectileId, o: FighterId) -> bool {
    let Some(owner) = pr(w, id).owner else { return false };
    o != owner
        && pk(w, id).pickup_gate
        && w.fighter(o).team.team == w.fighter(owner).team.team
        && buff_stacks(w, o, MUSIC_BUFF) < MAX_BUFF
}

/// PickedUp(Actor) @4725: the ally's SpawnBuffActor(Music_Buff, the ally, Instigator GetOwner (Gizmo), 1 stack, 0.0)
/// @5362, then QueueDestroyAfterClank while clanking, else DestroySpawnedActor(false) @15. True: destroyed now.
fn picked_up(w: &mut World, id: ProjectileId, o: FighterId) -> bool {
    if !ally_wants(w, id, o) {
        return false;
    }
    let owner = pr(w, id).owner;
    spawn_buff(w, o, name!("C023A_Music_Buff_C"), 0.0, Some(1), owner, true, Some(o));
    if pr(w, id).clanking {
        pm(w, id).queue_destroy = true;
        return false;
    }
    true
}

/// MagnetUp(Actor) @5472 -> @4221: CanStartFollowingProjectile false, IsHomingOnAlly, the homing on the ally's root
/// at the class's magnitude, Velocity = ClampVectorSize(Velocity, -2000, 2000), the ally's OnEndPlay bound to
/// Stop Homming @4475 (never reached: AFighterCharacter::EndPlay does not call AActor::EndPlay, so a fighter's
/// OnEndPlay is not broadcast; a destroyed minion is an invalid homing target instead), UpdateMaxLifetimeWithAuthority(
/// MaxLifetime), UpdateCapsuleCollision(false).
fn magnet_up(w: &mut World, id: ProjectileId, o: FighterId) {
    let s = pk_mut(w, id);
    s.can_start_following = false;
    s.is_homing_on_ally = true;
    let mag = homing_mag(w, id);
    set_homing(w, ActorRef::Projectile(id), ActorRef::Fighter(o), mag);
    let p = pm(w, id);
    let c = clamped_to_size([0.0, p.vx, p.vy], -2000.0, 2000.0);
    (p.vx, p.vy) = (c[1], c[2]);
    refresh_life(w, id);
    set_capsule_collision(pm(w, id), false);
}

/// The velocity of a followed actor: a projectile's or an item's.
// Unverified: a World.actors target is taken at rest (its vx / vy are not read).
fn velocity_of(w: &World, a: ActorRef) -> (Real, Real) {
    match a {
        ActorRef::Projectile(p) => w.actors.projectiles.get(p).map_or((0.0, 0.0), |p| (p.vx, p.vy)),
        ActorRef::Item(i) => w.actors.items.get(i).map_or((0.0, 0.0), |p| (p.vx, p.vy)),
        _ => (0.0, 0.0),
    }
}

/// Follow projectile(Actor) @6717 -> @5718: HommingTargetActor, CanStartFollowingProjectile false, the homing on its
/// root, Velocity = its GetVelocity x 0.5, the linger timer cleared, UpdateHitBoxDamage([Follow, HitBox], FollowDamage,
/// HitBoxScale 1, KeepUnusedHitBoxes false) (the handler keeps the first entry's AttackTag), PS_C023_Damage shown, its
/// OnEndPlay bound to FollowProjectileDeath, UpdateMaxLifetimeWithAuthority(MaxLifetime), UpdateCapsuleCollision(false);
/// a C023A_PopCorn's HasExploded bound to PopCornLaunch; sfx_cha_C023a_attack_sing_note_spike on itself.
fn follow(w: &mut World, id: ProjectileId, t: ActorRef) {
    {
        let s = pk_mut(w, id);
        s.target = Some(t);
        s.can_start_following = false;
    }
    let mag = homing_mag(w, id);
    if let ActorRef::Projectile(_) = t {
        set_homing(w, ActorRef::Projectile(id), t, mag);
    } else {
        pm(w, id).buff_home = Some(crate::actors::homing::buff_home(t, mag));
    }
    let (tvx, tvy) = velocity_of(w, t);
    let radius = box_radius(w, id, true);
    let fd = match class_p(w, pr(w, id)).get("FollowDamage") {
        Some(Value::Obj(o)) => o.clone(),
        _ => Props::new(),
    };
    let mut hit = HitPacket::from_props(&w.names, &fd, None);
    let pop = actor_cls(w, t) == Some(name!("C023A_PopCorn_C"));
    {
        let p = pm(w, id);
        p.vx = tvx * 0.5;
        p.vy = tvy * 0.5;
        hit.attack_tag = p.hit.attack_tag;
        p.hit = hit;
        p.hit_radius = radius;
    }
    {
        let s = pk_mut(w, id);
        s.linger_at = -1;
        s.damage_fx = true;
        s.end_bound = true;
    }
    refresh_life(w, id);
    set_capsule_collision(pm(w, id), false);
    pk_mut(w, id).pop_bound = pop;
    sound(w, id, name!("sfx_cha_C023a_attack_sing_note_spike"));
}

/// FollowProjectileDeath @6380 (the target's EndPlay) and PopCornLaunch @7366 (its HasExploded, with LaunchVelocity):
/// the linger timer (0.5 s -> TurnOffLingerDamage), the OnEndPlay unbound (and HasExploded for the launch),
/// CanStartFollowingProjectile = !IsHomingOnAlly; not homing on an ally: the homing off (the launch: Velocity =
/// LaunchVelocity), UpdateMaxLifetimeWithAuthority(MaxLifetime), UpdateCapsuleCollision(true), CanFollowProjectile.
fn unfollow(w: &mut World, id: ProjectileId, launch: Option<(Real, Real)>) {
    let at = w.flow.tick + timer_count(LINGER);
    let s = pk_mut(w, id);
    s.linger_at = at;
    if launch.is_some() {
        s.pop_bound = false;
    }
    s.end_bound = false;
    s.can_start_following = !s.is_homing_on_ally;
    if s.is_homing_on_ally {
        return;
    }
    clear_homing(w, ActorRef::Projectile(id));
    if let Some((vx, vy)) = launch {
        let p = pm(w, id);
        p.vx = vx;
        p.vy = vy;
    }
    refresh_life(w, id);
    set_capsule_collision(pm(w, id), true);
    pk_mut(w, id).can_follow = true;
}

/// The zero packet of LingerDamageMulticast's UpdateHitBoxDamage @7013 (Struct_HitBoxDamage's fields in order).
fn zero_packet() -> Props {
    let vec2 = || {
        let mut v = Props::new();
        v.insert("X".to_owned(), Value::Num(0.0));
        v.insert("Y".to_owned(), Value::Num(0.0));
        Value::Obj(v)
    };
    let mut vacuum = Props::new();
    vacuum.insert("UseVacuumKnockBack".to_owned(), Value::Bool(false));
    let n = |v: Real| Value::Num(f64::from(v));
    let b = Value::Bool;
    let rows: Vec<(&str, Value)> = vec![
        ("Damage", n(0.0)),
        ("BaseKnockBack", n(0.0)),
        ("KnockBackScaling", n(0.0)),
        ("KnockBackDirectionVector", vec2()),
        ("AttackTag", Value::Str("None".to_owned())),
        ("HitReaction", n(0.0)),
        ("HitPauseTime", n(0.0)),
        ("HitStunMult", n(0.0)),
        ("OnHitAudio", Value::Null),
        ("OnHitEffectOverride", Value::Null),
        ("DirectionalKnockback", b(false)),
        ("VelocityInsteadOfFacing", b(false)),
        ("KnockBackMirror", b(false)),
        ("IgnoreFacing", b(false)),
        ("AttackAnimation", Value::Null),
        ("AttackID", n(0.0)),
        ("DoLOSCheck", b(false)),
        ("ForceKnockback", b(false)),
        ("QueueHitPauseUntilPoses", b(false)),
        ("HitOnlyOneFighter", b(false)),
        ("ArmorPierce", b(false)),
        ("IgnoreActor", Value::Null),
        ("DestroyProjectiles", b(false)),
        ("ReflectProjectiles", b(false)),
        ("VacuumKnockback", b(false)),
        ("DirectKnockbackTowardsSocket", b(false)),
        ("KnockbackTargetSocket", Value::Str("None".to_owned())),
        ("KnockbackSocketOffset", vec2()),
        ("ControllerRumble", Value::Null),
        ("PlayOnHitEffects", b(true)),
        ("ApplyHitPauseToDamageDealer", b(true)),
        ("AttackDecayType", n(0.0)),
        ("AttackDecayName", Value::Str("None".to_owned())),
        ("AdditiveKBMode", n(0.0)),
        ("KBScalingStartingDmg", n(0.0)),
        ("HitEachActorOnlyOnce", b(true)),
        ("AccountForVelocityWithVacuumKnockback", b(true)),
        ("IgnoreKBI", b(false)),
        ("KBI_Multiplier", n(1.0)),
        ("VacuumInfo", Value::Obj(vacuum)),
    ];
    rows.into_iter().map(|(k, v)| (k.to_owned(), v)).collect()
}

/// TurnOffLingerDamage @3489 (authority) -> LingerDamageMulticast @6989: UpdateHitBoxDamage([HitBox], the zero packet,
/// HitBoxScale 1, KeepUnusedHitBoxes false): Follow off, HitBox on with a packet of 0; PS_C023_Damage hidden.
fn linger_off(w: &mut World, id: ProjectileId) {
    let mut hit = HitPacket::from_props(&w.names, &zero_packet(), None);
    let radius = box_radius(w, id, false);
    let p = pm(w, id);
    hit.attack_tag = p.hit.attack_tag;
    p.hit = hit;
    p.hit_radius = radius;
    pk_mut(w, id).damage_fx = false;
}

/// `popCornExploded(w, popcorn, vx, vy)`: the music pickups following the popcorn `pr` (popcorn.rs calls it).
/// C023A_PopCorn's HasExploded broadcast: PopCornLaunch(LaunchVelocity) on every pickup bound to that popcorn. The
/// velocity in sim space (x = -UE Y, y = UE Z).
pub fn pop_corn_exploded(w: &mut World, pc: ProjectileId, vx: Real, vy: Real) {
    let ids: Vec<ProjectileId> = w.actors.projectiles.iter().map(|(id, _)| id).collect();
    for id in ids {
        let hit = {
            let p = pr(w, id);
            !p.dead
                && matches!(&p.ext, ProjectileExt::MusicPickup(s) if s.pop_bound && s.target == Some(ActorRef::Projectile(pc)))
        };
        if hit {
            unfollow(w, id, Some((vx, vy)));
        }
    }
}

/// The fighters a DetectPawn sphere of the pickup meets (their root capsules, Pawn).
fn fighters_in(w: &mut World, id: ProjectileId, r: Real) -> Vec<FighterId> {
    let (x, y) = (pr(w, id).x, pr(w, id).y);
    sphere_overlap_actors(w, x, y, r, &[crate::real(OBJ_PAWN)], &[], crate::real(OBJ_WORLD_DYNAMIC))
        .into_iter()
        .filter_map(|a| if let ActorRef::Fighter(f) = a { Some(f) } else { None })
        .collect()
}

/// A fighter of the pickup's team other than its owner is in the match: the only ones the two DetectPawn spheres act
/// on (both handlers test the team and the owner first), so with none the queries are skipped.
fn has_ally(w: &World, id: ProjectileId) -> bool {
    let Some(owner) = pr(w, id).owner else { return false };
    let team = w.fighter(owner).team.team;
    peers(w).iter().any(|&o| o != owner && w.fighter(o).team.team == team)
}

/// GetComponentByClass(Comp_Actor_Destructible).IsProjectile of an actor (its class default; no graph of the port
/// writes it).
fn destructible_is_projectile(w: &World, a: ActorRef) -> bool {
    let Some(cls) = actor_cls(w, a) else { return false };
    let own = ActorRef::Fighter(lookup_owner(w, a));
    actor_components(w, own, cls)
        .and_then(|c| c.get("Comp_Actor_Destructible"))
        .and_then(Value::as_obj)
        .and_then(|c| c.get("IsProjectile"))
        .is_some_and(|v| match v {
            Value::Null => false,
            Value::Bool(b) => *b,
            Value::Num(n) => *n != 0.0 && !n.is_nan(),
            Value::Str(s) => !s.is_empty(),
            _ => true,
        })
}

/// A same-team spawned actor with a Destructible IsProjectile near enough to be met (perf: the query is skipped
/// without one).
fn friend_near(w: &World, id: ProjectileId) -> bool {
    let p = pr(w, id);
    let Some(owner) = p.owner else { return false };
    let team = w.fighter(owner).team.team;
    let reach = FRIEND_R + 400.0;
    let (x, y) = (p.x, p.y);
    for (oid, o) in w.actors.projectiles.iter() {
        if oid == id || o.dead {
            continue;
        }
        let a = ActorRef::Projectile(oid);
        if owner_team(w, a) != team {
            continue;
        }
        if (o.x - x).abs() < reach && (o.y - y).abs() < reach && destructible_is_projectile(w, a) {
            return true;
        }
    }
    for (oid, o) in w.actors.actors.iter() {
        let a = ActorRef::Actor(oid);
        if o.dead || owner_team(w, a) != team {
            continue;
        }
        let Some((ox, oy, _)) = world_actor_place_of(w, oid) else { continue };
        if (ox - x).abs() < reach && (oy - y).abs() < reach && destructible_is_projectile(w, a) {
            return true;
        }
    }
    false
}

/// FrendlyProjectileCheck's begin overlap @2202 (authority): CanStartFollowingProjectile and not IsHomingOnAlly, an
/// actor with BPI_SpawnedActor whose Destructible is of the pickup's team and IsProjectile, and CanFollowProjectile ->
/// CanFollowProjectile false, Follow projectile.
fn friend_overlap(w: &mut World, id: ProjectileId, o: ActorRef) {
    let s = pk(w, id);
    if !s.can_start_following || s.is_homing_on_ally {
        return;
    }
    let Some(owner) = pr(w, id).owner else { return };
    if owner_team(w, o) != w.fighter(owner).team.team || !destructible_is_projectile(w, o) || !pk(w, id).can_follow {
        return;
    }
    pk_mut(w, id).can_follow = false;
    follow(w, id, o);
}

/// `isFollowed`: a spawned actor (not a fighter) that is alive and has an owner.
fn is_followed(w: &World, a: ActorRef) -> bool {
    !matches!(a, ActorRef::Fighter(_) | ActorRef::Stage(_))
        && actor_cls(w, a).is_some()
        && !actor_dead(w, a)
        && actor_owner(w, a).is_some()
}

/// The pickup's constructor tail: CanStartFollowingProjectile / CanFollowProjectile from the class data (CDO true).
fn pickup_construct(w: &World, p: &mut Projectile) {
    let cp = class_p(w, p);
    let flag = |k: &str| cp.get(k).and_then(Value::as_bool).unwrap_or(true);
    p.ext = ProjectileExt::MusicPickup(MusicPickup {
        can_start_following: flag("CanStartFollowingProjectile"),
        can_follow: flag("CanFollowProjectile"),
        linger_at: -1,
        ..MusicPickup::default()
    });
}

/// ReceiveBeginPlay @3534: the parent's (NumberOfActorsAllowed 5: past the limit the oldest pickup of the owner's
/// SpawnedActors goes), the material and team colour (the view's), Delay(0.3) @4057, MeshWiggle and SpawnGrowth (the
/// mesh's: the view's), sfx_cha_C023a_attack_sing_note on itself @4145.
fn pickup_begin(w: &mut World, id: ProjectileId) {
    sound(w, id, name!("sfx_cha_C023a_attack_sing_note"));
}

/// The actor's tick, behind its movement's. Deviation (as FinnTreasure's): the Delay resumes here; the linger timer
/// fires here in the frame the world's timer pass fires it, ahead of that pass; the begin overlaps of the three spheres
/// are tested here once a tick against where everything then is (a fighter or actor moving into a sphere raises it in
/// its own move in the build, earlier in the frame), a new entry since the last test being a begin overlap; the
/// followed actor's EndPlay is met here (dead since the last tick), not inside its destroy.
/// FrendlyProjectileCheck's types: Overlap on every channel it lists but Pawn and Vehicle (Ignore); of those, the object
/// types `fighter/traces.rs` gathers. Deviation: WorldDynamic, Destructible, PlatformDetection, HitBox, KnockBack,
/// Cleanse, FadeCheck and WorldStaticDetection bodies are not gathered (a projectile's own hit boxes among them: it is
/// met by its root Capsule or its HurtBox alone), nor are Item, HurtBox and BlockTrace (not listed: the channels'
/// defaults).
fn pickup_step(w: &mut World, id: ProjectileId) -> bool {
    let t = w.flow.tick;
    let ally = has_ally(w, id);
    if pr(w, id).age == gate_delay() {
        // @31: Velocity (0, 0, 150), PickupGate, CheckOverlaps @3408: PickedUp for each fighter in PickUpCheck, then
        // MagnetUp for each in MagnetCheck that passes the ally test while not bIsHomingProjectile.
        {
            let p = pm(w, id);
            p.vx = 0.0;
            p.vy = 150.0;
        }
        pk_mut(w, id).pickup_gate = true;
        if ally {
            for o in fighters_in(w, id, PICK_R) {
                if picked_up(w, id, o) {
                    return true;
                }
            }
            for o in fighters_in(w, id, MAGNET_R) {
                if pr(w, id).buff_home.is_none() && ally_wants(w, id, o) {
                    magnet_up(w, id, o);
                }
            }
        }
    }
    let s = pk(w, id);
    if let Some(tg) = s.target
        && s.end_bound
        && actor_dead(w, tg)
    {
        unfollow(w, id, None);
    }
    if t == pk(w, id).linger_at {
        pk_mut(w, id).linger_at = -1;
        linger_off(w, id);
    }
    if ally {
        // PickUpCheck's begin overlap @2634 -> PickedUp; MagnetCheck's @2688: the ally test while not IsHomingOnAlly.
        let pick = fighters_in(w, id, PICK_R);
        let ids: Vec<i32> = pick.iter().map(|&o| w.fighter(o).id).collect();
        let was = std::mem::replace(&mut pk_mut(w, id).in_pick, ids);
        for o in pick {
            if !was.contains(&w.fighter(o).id) && picked_up(w, id, o) {
                return true;
            }
        }
        let mag = fighters_in(w, id, MAGNET_R);
        let ids: Vec<i32> = mag.iter().map(|&o| w.fighter(o).id).collect();
        let was = std::mem::replace(&mut pk_mut(w, id).in_magnet, ids);
        for o in mag {
            if !was.contains(&w.fighter(o).id) && !pk(w, id).is_homing_on_ally && ally_wants(w, id, o) {
                magnet_up(w, id, o);
            }
        }
    } else if !pk(w, id).in_pick.is_empty() || !pk(w, id).in_magnet.is_empty() {
        let s = pk_mut(w, id);
        s.in_pick.clear();
        s.in_magnet.clear();
    }
    if friend_near(w, id) {
        let (x, y) = (pr(w, id).x, pr(w, id).y);
        let types: [Real; 7] = [
            crate::real(OBJ_WORLD_STATIC),
            crate::real(OBJ_PHYSICS_BODY),
            crate::real(OBJ_DAMAGE_DESTRUCTIBLE),
            crate::real(OBJ_PLATFORM),
            crate::real(OBJ_PROJECTILE_DESTRUCTIBLE),
            crate::real(OBJ_PORTAL),
            crate::real(OBJ_ITEM_DESTRUCTIBLE),
        ];
        let now: Vec<ActorRef> = sphere_overlap_actors(
            w,
            x,
            y,
            FRIEND_R,
            &types,
            &[Some(ActorRef::Projectile(id))],
            crate::real(OBJ_WORLD_DYNAMIC),
        )
        .into_iter()
        .filter(|&a| is_followed(w, a))
        .collect();
        let was = std::mem::replace(&mut pk_mut(w, id).in_friend, now.clone());
        for o in now {
            if !was.contains(&o) {
                friend_overlap(w, id, o);
            }
        }
    } else if !pk(w, id).in_friend.is_empty() {
        pk_mut(w, id).in_friend.clear();
    }
    false
}

// ---------------------------------------------------------------- C023A_Music_Projectile_C

/// Spawned by Music_Buff's EvadeTriggered (@2048: the instigator's CompSpawnActor, AttachToActor true on the buff's
/// owner, socket None, a relative transform at 0 with Scale3D 4, speeds 0, AttackID 0, AssignInstigator the
/// instigator): it rides the ally's actor location (the Capsule bAbsoluteScale: its world scale 4, the HitBox2D circle
/// 42 x 4). Its graph: BeginPlay picks a mesh (the view's); BPI_SetInitialMovement @187: KnockBackDirectionVector =
/// (1, 1) for a zero velocity, else (Y, Z); DirectionalKnockback = the velocity is zero; IgnoreFacing; HitBoxOn? and
/// UpdateHitbox. The velocity Comp_Spawner hands it is Normal(direction x 0) x 0: always zero, so the direction the
/// buff computes from the evade never reaches it: (1, 1), directional. MaxLifetime 0.35 (the parent's).
/// Deviation (as Creature_WreckingBall's): the ride is taken in the projectile pass (after the fighters' moves).
fn projectile_construct(_w: &World, p: &mut Projectile) {
    // AttachToActor true: the movement is never active and the spawn is not pushed out (World.pushOutRoot).
    p.attached = true;
    // The relative Scale3D 4 on a bAbsoluteScale root: its world scale (the constructor took the spawn's).
    let k = 4.0 / (if p.scale == 0.0 { 1.0 } else { p.scale });
    p.scale = 4.0;
    p.hit_radius *= k;
    p.hurt_radius *= k;
    p.ext = ProjectileExt::MusicProjectile(MusicProjectile::default());
}

/// MusicProjectile's `attachedTo`: its ally.
pub fn attached_to(p: &Projectile, m: &MusicProjectile) -> Option<FighterId> {
    let _ = p;
    m.ally
}

/// MusicProjectile's `hitX` / `hitY`: its ally's actor location while attached.
pub fn hit_xy(w: &World, p: &Projectile) -> (Real, Real) {
    match &p.ext {
        ProjectileExt::MusicProjectile(MusicProjectile { ally: Some(a) }) => (w.fighter(*a).body.x, actor_y(w, *a)),
        _ => (p.x, p.y),
    }
}

/// MusicProjectile's `step` override: it rides its ally, the parent's expiry runs on.
pub fn step_override(w: &mut World, id: ProjectileId) -> Option<StepEnd> {
    if pr(w, id).dead {
        return None;
    }
    let ally = match &pr(w, id).ext {
        ProjectileExt::MusicProjectile(m) => m.ally,
        _ => None,
    };
    {
        let p = pm(w, id);
        p.prev_x = p.x;
        p.prev_y = p.y;
        p.age += 1;
    }
    if let Some(a) = ally {
        let (x, y, fc) = (w.fighter(a).body.x, actor_y(w, a), facing(w, a));
        let p = pm(w, id);
        p.x = x;
        p.y = y;
        p.facing = fc;
    }
    expiry(w, id)
}

/// The music projectile's `begin`.
fn projectile_begin(w: &mut World, id: ProjectileId) {
    let p = pm(w, id);
    p.hit.knock_back_direction_vector = Some(DataVec { x: Some(1.0), y: Some(1.0), z: None });
    p.hit.directional_knockback = Some(true);
    p.hit.ignore_facing = Some(true);
    set_hitbox(p, true);
}

// ---------------------------------------------------------------- C023A_PianoNote_C

/// The Bubble timeline's ScaleAlpha curve (CurveFloat_0, RCIM_Cubic): [time, value, arrive, leave]; the timeline is
/// 1 s long, not looped. FRichCurve::Eval as the kicked arc curve of Tom and Jerry.
const BUBBLE_KEYS: [[Real; 4]; 2] = [[0.0, 0.0, 0.0, 0.0], [0.5, 1.0, 0.3017271, 0.30172667]];

fn bubble_curve(t: Real) -> Real {
    let (a, b) = (BUBBLE_KEYS[0], BUBBLE_KEYS[1]);
    if t <= a[0] {
        return a[1];
    }
    if t >= b[0] {
        return b[1];
    }
    let gap = b[0] - a[0];
    let u = (t - a[0]) / gap;
    let p1 = a[1] + (a[3] * gap) / 3.0;
    let p2 = b[1] - (b[2] * gap) / 3.0;
    let v = 1.0 - u;
    v * v * v * a[1] + 3.0 * v * v * u * p1 + 3.0 * v * u * u * p2 + u * u * u * b[1]
}

/// Spawned by the Piano montages' Spawner notifies. BeginPlay: the team colour (the view's), Bubble.PlayFromStart;
/// Bubble's update @16: HitBox and HitBox2D relative scale = VLerp(0.5, 1.5, ScaleAlpha). Its life: MaxLifetime 0.35
/// (InitialLifeSpan 0.55 behind it), gravity -1 with AimDownProjectileMovement (the parent's).
// Unverified: the timeline ticks with the actor from the frame after the spawn (as DeathBlinkWarning's), position +
// delta in float32; in the spawn frame the hit circle is the spawn's (HitBoxScale: the parent's UpdateHitbox ->
// ActivateHitbox behind BeginPlay, which sets the boxes' relative scale after PlayFromStart's update).
fn piano_construct(w: &World, p: &mut Projectile) {
    let base = p
        .owner
        .and_then(|o| actor_components(w, ActorRef::Fighter(o), p.cls))
        .and_then(|c| c.get("HitBox2D"))
        .and_then(Value::as_obj)
        .and_then(|c| num(c, "CapsuleRadius"))
        .unwrap_or(0.0)
        * p.scale;
    p.ext = ProjectileExt::PianoNote(PianoNote { bubble_base: base, ..PianoNote::default() });
}

/// The note's `begin`.
fn piano_begin(w: &mut World, id: ProjectileId) {
    let t = w.flow.tick;
    if let ProjectileExt::PianoNote(n) = &mut pm(w, id).ext {
        n.born_at = t;
    }
}

/// The note's `step`: `bubbleTick` from the tick after the spawn.
fn piano_step(w: &mut World, id: ProjectileId) -> bool {
    let t = w.flow.tick;
    let (bubble, born, base) = match &pr(w, id).ext {
        ProjectileExt::PianoNote(n) => (n.bubble, n.born_at, n.bubble_base),
        _ => return false,
    };
    if t <= born || bubble >= 1.0 {
        return false;
    }
    let b = min(bubble + (1.0 / 60.0), 1.0);
    let p = pm(w, id);
    if let ProjectileExt::PianoNote(n) = &mut p.ext {
        n.bubble = b;
    }
    p.hit_radius = base * (0.5 + bubble_curve(b));
    false
}

// ---------------------------------------------------------------- the buffs

/// The buff entry's Instigator (SpawnBuffActor's), kept for the graphs that read GetInstigator.
fn ext_from(w: &World, b: BuffRef) -> Option<FighterId> {
    match &buff(w, b)?.ext {
        BuffExt::GizmoMusic(e) => e.from,
        _ => None,
    }
}

/// The class data's MaxStacks (`d.MaxStacks ?? 1`).
fn max_stacks(w: &World, f: FighterId, cls: Name) -> Real {
    buff_def(w, ActorRef::Fighter(f), cls).and_then(|d| num(d, "MaxStacks")).unwrap_or(1.0)
}

/// C023A_Music_Buff MulticastUI(Count) @2432 (not a dedicated server, a UI broker for the owner): Count != 0 ->
/// AddStateForPlayer MusicDebuff (StateType 2, DisplayType 0, Count / MaxStacks, the DodgeBurst icon), else
/// RemoveStateForPlayer MusicDebuff.
fn music_buff_ui(w: &mut World, f: FighterId, count: Real, max: Real) {
    let op = if count != 0.0 {
        UiStateOp::Add(UiStateParams {
            id: name!("MusicDebuff"),
            state_type: 2,
            display_type: 0,
            current_value: count,
            max_value: max,
            timer: -1.0,
            icon_material: name!("MI_State_DodgeBurst"),
            active: true,
            pop_text: Name::NONE,
            auto_remove_time: -1.0,
            subicon: None,
        })
    } else {
        UiStateOp::Remove(name!("MusicDebuff"))
    };
    w.fighter_mut(f).controls.ui_state_ops.push(op);
}

/// C023A_Music_Buff_C begin: ReceiveBeginPlay @254: an owner that is a BP_FighterCharacter (else nothing at all) binds
/// its EvadeTriggered (@363); on the authority the owner's DamageManager KnockedBack (@115) and MulticastUI(
/// min(CurrentStackAmount, MaxStacks)) @225.
fn music_begin(w: &mut World, f: FighterId, b: BuffRef, from: Option<FighterId>, _dur: Option<Real>) {
    if let Some(x) = buff_mut(w, b) {
        x.ext = BuffExt::GizmoMusic(MusicExt { from });
    }
    if w.fighter(f).life.not_player_character {
        return;
    }
    let max = max_stacks(w, f, name!("C023A_Music_Buff_C"));
    let stacks = buff(w, b).map_or(0.0, |x| x.stacks);
    music_buff_ui(w, f, min(stacks, max), max);
}

/// EvadeTriggered(Direction) @2250 -> @432: the direction by Enum_InputDirection8Way (Up (0, 0, 1), Down (0, 0, -1),
/// Fwd / Back (0, 1, 0), Neutral / Default 0, FwdUp / BackUp (0, 1, 1), FwdDown / BackDown (0, 1, -1)) x (0, -1, 1)
/// when the owner and the instigator face apart; the instigator's Comp_Spawner CompSpawnActor(Music_Projectile,
/// attached to the owner) @2048; RemoveStack(1) @2178. A spawn on a None instigator does nothing.
const EVADE_DIRS: [[Real; 2]; 10] = [
    [0.0, 1.0],
    [0.0, -1.0],
    [1.0, 0.0],
    [1.0, 0.0],
    [0.0, 0.0],
    [0.0, 0.0],
    [1.0, 1.0],
    [1.0, -1.0],
    [1.0, 1.0],
    [1.0, -1.0],
];

fn evade_triggered(w: &mut World, f: FighterId, b: BuffRef, dir: Real) {
    if let Some(by) = ext_from(w, b) {
        let d = EVADE_DIRS.iter().zip(0..).find(|&(_, i)| crate::real(i) == dir).map_or([0.0, 0.0], |(d, _)| *d);
        let y = if facing(w, by) != facing(w, f) { -d[0] } else { d[0] };
        let (x, ay, ff) = (w.fighter(f).body.x, actor_y(w, f), facing(w, f));
        // AttackID 0: Comp_Spawner @2444-2747 takes the attack count (the default)
        let pr = crate::world::spawning::comp_spawn_projectile(
            w,
            by,
            name!("C023A_Music_Projectile_C"),
            x,
            ay,
            [Some(0.0), Some(y), Some(d[1])],
            0.0,
            0.0,
            None,
        );
        if let Some(id) = pr
            && let ProjectileExt::MusicProjectile(m) = &mut pm(w, id).ext
        {
            m.ally = Some(f);
            pm(w, id).facing = ff;
        }
    }
    remove_stack(w, f, b, 1);
}

/// C023A_Music_Buff_C on (evadeTriggered, knockedBack): OnKnockedBack @3475: RemoveStack(1).
fn music_on(w: &mut World, f: FighterId, b: BuffRef, s: BuffSignal, a: &BuffSignalArg) -> bool {
    if w.fighter(f).life.not_player_character {
        return false;
    }
    match s {
        BuffSignal::EvadeTriggered => evade_triggered(w, f, b, a.dir.unwrap_or(4.0)),
        BuffSignal::KnockedBack => remove_stack(w, f, b, 1),
        _ => {}
    }
    false
}

/// C023A_Music_Buff_C stacks: AddStack @3290: the parent's, sfx_cha_C023a_piano_note_end on the owner,
/// MulticastUI(CurrentStackAmount); RemoveStack @3407: the parent's, the UI.
fn music_stacks(w: &mut World, f: FighterId, b: BuffRef, added: bool) -> bool {
    if w.fighter(f).life.not_player_character {
        return false;
    }
    if added {
        let id = w.fighter(f).id;
        w.fighter_mut(f).events.push(Event::Sound {
            fighter: id,
            ev: name!("sfx_cha_C023a_piano_note_end"),
            x: None,
            y: None,
            hit: None,
            pooled: false,
            key: None,
        });
    }
    let max = max_stacks(w, f, name!("C023A_Music_Buff_C"));
    let stacks = buff(w, b).map_or(0.0, |x| x.stacks);
    music_buff_ui(w, f, stacks, max);
    false
}

/// C023A_Music_Buff_C end: ReceiveEndPlay @3431: MulticastUI(0) (a RemoveStack that ends it sends its own remove too:
/// one op here).
fn music_end(w: &mut World, f: FighterId, _b: BuffRef) {
    if !w.fighter(f).life.not_player_character {
        music_buff_ui(w, f, 0.0, 0.0);
    }
}

/// C023A_Music_DeBuff_C begin.
fn debuff_begin(w: &mut World, _f: FighterId, b: BuffRef, from: Option<FighterId>, _dur: Option<Real>) {
    if let Some(x) = buff_mut(w, b) {
        x.ext = BuffExt::GizmoMusic(MusicExt { from });
    }
}

/// C023A_Music_DeBuff_C maxStacks: BPI On Max Stacks @964: the owner's SpawnBuffActor(C023A_Buff_Silenced, the owner,
/// the Instigator, 0 stacks, 8.0 s) @1169 (Silenced breaks this buff), then K2_DestroyActor @10.
fn debuff_max_stacks(w: &mut World, f: FighterId, b: BuffRef) -> bool {
    let from = ext_from(w, b);
    spawn_buff(w, f, name!("C023A_Buff_Silenced_C"), 8.0, Some(0), from, true, Some(f));
    true
}

/// C023A_Music_DeBuff_C on a spawned actor: BPI On Max Stacks @964-1169 with an owner that has a
/// Comp_State_Manager: SpawnBuffActor(C023A_Buff_Silenced_C, the owner, the Instigator, 0, 8.0) (the actor's buff list
/// runs the sweep: Silenced breaks the debuff), then K2_DestroyActor @10.
// The debuff has an actor entry, so the spawn runs.
// Unverified: the Instigator is not kept on the actor's entry (the Silenced on an actor reads none); the owner's
// Comp_State_Manager is not tested (every SpawnedActorParent child has one; a carrier without one gets nothing in the
// build, the debuff staying at MaxStacks).
fn debuff_actor_max_stacks(w: &mut World, a: ActorRef, _i: u32) -> bool {
    spawn_actor_buff(w, a, name!("C023A_Buff_Silenced_C"), 0, 8.0, true, None);
    true
}

/// C023A_Buff_Silenced_C on a spawned actor: BeginPlay @705 finds no Comp_Pawn_AbilityManager on the owner
/// (AdjustBlockCooldowns does nothing), MulticastUI @10 fails for a non-fighter (no HUD state), and the PopText
/// @69-645 is made at the owner's location, as for a fighter, the event naming the actor's owner fighter.
// The class has an actor entry.
pub fn silenced_actor_begin(w: &mut World, a: ActorRef, i: u32, _from: Option<FighterId>) -> bool {
    let (x, y, _) = actor_place(w, a).unwrap_or((0.0, 0.0, false));
    let audio = actor_inst_def(w, a, i).and_then(|d| name_of(w, &d, "Params Audio"));
    let owner = lookup_owner(w, a);
    let g = silenced_pop(audio, x, y);
    pop_text_on(w, owner, &g);
    false
}

/// C023A_Music_DeBuff_C status: BPI After Status Update Multicast @29: not a dedicated server, a BP_FighterCharacter
/// owner with a UI broker: BuffRemoved or a count <= 0 -> RemoveStateForPlayer MusicDebuff, else AddStateForPlayer
/// MusicDebuff (StateType 1, DisplayType 0, the count / MaxStacks, the Silenced icon). BlockerBuffs Crippled and
/// Silenced (`combat/buffs.rs` spawn_buff).
fn debuff_status(w: &mut World, f: FighterId, _c: BuffClass, stacks: Real, _applied: bool, removed: bool) {
    if w.fighter(f).life.not_player_character {
        return;
    }
    if removed || stacks <= 0.0 {
        w.fighter_mut(f).controls.ui_state_ops.push(UiStateOp::Remove(name!("MusicDebuff")));
    } else {
        let max = max_stacks(w, f, name!("C023A_Music_DeBuff_C"));
        w.fighter_mut(f).controls.ui_state_ops.push(UiStateOp::Add(UiStateParams {
            id: name!("MusicDebuff"),
            state_type: 1,
            display_type: 0,
            current_value: stacks,
            max_value: max,
            timer: -1.0,
            icon_material: name!("MI_State_Silenced"),
            active: true,
            pop_text: Name::NONE,
            auto_remove_time: -1.0,
            subicon: None,
        }));
    }
}

/// C023A_Buff_Crippled_C begin: ReceiveBeginPlay @70: the owner's BPI_SetBlockEvadeInvulnerability(true), then on the
/// authority MulticastUI(true) @251 -> @433: AddStateForPlayer Crippled (StateType 1, DisplayType 1, -1 / -1, the Block
/// icon). Nothing in the build spawns it (xref: no SpawnBuffActor names the class) and Gizmo's data has no defaults
/// for it.
// Deviation: BlockEvadeInvulnerability (BaseFighter's flag, read by the evade's invulnerability) has no port field:
// the block itself is not ported.
fn crippled_begin(w: &mut World, f: FighterId, _b: BuffRef, _from: Option<FighterId>, _dur: Option<Real>) {
    w.fighter_mut(f).controls.ui_state_ops.push(UiStateOp::Add(UiStateParams {
        id: name!("Crippled"),
        state_type: 1,
        display_type: 1,
        current_value: -1.0,
        max_value: -1.0,
        timer: -1.0,
        icon_material: name!("MI_State_Block"),
        active: true,
        pop_text: Name::NONE,
        auto_remove_time: -1.0,
        subicon: None,
    }));
}

/// C023A_Buff_Crippled_C end: ReceiveEndPlay @272: SetBlockEvadeInvulnerability(false), MulticastUI(false) ->
/// RemoveStateForPlayer Crippled.
fn crippled_end(w: &mut World, f: FighterId, _b: BuffRef) {
    w.fighter_mut(f).controls.ui_state_ops.push(UiStateOp::Remove(name!("Crippled")));
}

/// This file's registry entries, in music.rs's order: the buffs, then the three projectiles.
pub fn register(r: &mut Registry) -> Result<(), RegistryError> {
    r.buffs.enter(
        MUSIC_BUFF,
        BuffEntry::new(
            BuffClass::C023AMusicBuff,
            BuffHooks {
                begin: Some(music_begin),
                on: Some(music_on),
                stacks: Some(music_stacks),
                end: Some(music_end),
                ..BuffHooks::default()
            },
        ),
    )?;
    r.buffs.enter(
        MUSIC_DEBUFF,
        BuffEntry {
            actor: Some(ActorBuffHooks { max_stacks: Some(debuff_actor_max_stacks), ..ActorBuffHooks::default() }),
            ..BuffEntry::new(
                BuffClass::C023AMusicDeBuff,
                BuffHooks {
                    begin: Some(debuff_begin),
                    max_stacks: Some(debuff_max_stacks),
                    status: Some(debuff_status),
                    ..BuffHooks::default()
                },
            )
        },
    )?;
    r.buffs.enter(
        CRIPPLED,
        BuffEntry::new(
            BuffClass::C023ABuffCrippled,
            BuffHooks { begin: Some(crippled_begin), end: Some(crippled_end), ..BuffHooks::default() },
        ),
    )?;
    let pickup = ProjectileHooks {
        construct: Some(pickup_construct),
        begin: Some(pickup_begin),
        step: Some(pickup_step),
        ..ProjectileHooks::default()
    };
    r.actors.enter(
        MUSIC_PICKUP,
        ActorEntry::new(ActorClass::Projectile(ProjectileClass::C023AMusicPickup), ActorHooks::Projectile(pickup)),
    )?;
    let projectile = ProjectileHooks {
        construct: Some(projectile_construct),
        begin: Some(projectile_begin),
        ..ProjectileHooks::default()
    };
    r.actors.enter(
        MUSIC_PROJECTILE,
        ActorEntry::new(
            ActorClass::Projectile(ProjectileClass::C023AMusicProjectile),
            ActorHooks::Projectile(projectile),
        ),
    )?;
    let piano = ProjectileHooks {
        construct: Some(piano_construct),
        begin: Some(piano_begin),
        step: Some(piano_step),
        ..ProjectileHooks::default()
    };
    r.actors.enter(
        PIANO_NOTE,
        ActorEntry::new(ActorClass::Projectile(ProjectileClass::C023APianoNote), ActorHooks::Projectile(piano)),
    )
}
