//! Jerry's champagne and its corks: Champagne Attack F / B,
//! LaunchCork, CorkAmmo and its tracker, the ChampagneTimer and ChampagneGate (Tom's `TomAndJerryChampagneState`), the
//! proxy's BottleShoot / LaunchCork notifies, and JerryCork_C ([`JerryCork`], a ParentProjectilePhysics child); joined
//! to Tom's tick.
//!
//! LaunchCork @1612-1690 (the launch speed clamped and normalised 1200..5000 @1069-1133, the spawn @1583), CorkAmmo /
//! CorkAmmoTracker @2310 / SetAmmoUI_Visuals / CorkAmmoSetrMulticast (`TomAndJerryState::cork_ammo`), the
//! ChampagneTimer 0.5 s @2214 and ChampagneGate; the Champagne window and the BottleShoot / LaunchCork_F / LaunchCork_B
//! named notifies of Champagne_F / _B (the proxy's begin switch @25399-25609, the end switch @24965-24999).
//! What jerry.rs calls here: champagne_attack (ActivateObject's conditions 1 / 4 with their gate), champagne_shoot
//! (condition 3), champagne_notify, champagne_timer_function (the hit react @22531), cork_begin_play (the champagne part
//! of Jerry's BeginPlay @13402; the tick hook runs it for a Jerry it has not seen), set_cork_ammo (@31094, follower.rs).
// Deviation: ChampagneTimer is a world timer on Jerry (set in his mesh's notify, fired in the frame's timer pass); the
// port counts its `timer_frames(0.5)` in Tom's tick (the joined `tick`), so the gate opens in Tom's tick of the frame
// after the timer's: a condition read by a notify of Tom's in that frame finds it open, as in the build, when the
// pawn's ReceiveTick comes ahead of his montage's notifies, a frame late otherwise.

use super::follower::rotate;
use super::jerry::{jerry_mut, jerry_of, play_montage, update_anim_state};
use super::state::{TomAndJerryChampagneState, tj_mut, tj_of};
use crate::Real;
use crate::State;
use crate::actors::projectile::{self, ProjectileExt};
use crate::actors::world_actor::actor_p;
use crate::actors::{ActorClass, ActorId, ActorRef, Projectile, ProjectileClass, ProjectileId};
use crate::combat::packet::{DataEnum, DataVec};
use crate::events::Event;
use crate::fighter::common::{timer_frames, warn_text};
use crate::fighter::match_side::SpawnRequest;
use crate::fighter::{FighterId, facing};
use crate::math::{max, min, to_int32};
use crate::name;
use crate::names::Name;
use crate::registry::entries::{ActorEntry, ActorHooks, ProjectileHooks, ReflectHooks};
use crate::registry::props::num as pnum;
use crate::registry::{Registry, RegistryError};
use crate::world::World;
use crate::world::spawning::comp_spawn_projectile;

/// The cork's class.
pub const CORK: &str = "JerryCork_C";
const CORK_N: Name = name!("JerryCork_C");
const MONTAGE_F: Name = name!("Tom_and_Jerry_Champagne_F_Montage");
const MONTAGE_B: Name = name!("Tom_and_Jerry_Champagne_B_Montage");
/// K2_SetTimer(ChampagneTimerFunction, 0.5, false) @2214.
const CHAMPAGNE_TIMER: Real = 0.5;

/// `JerryCork extends Projectile` (JerryCork_C): the cork's own fields (the view reads `pop_tick`, `bounce_left`,
/// `trail`, `reflect_fx` and `reflected`).
#[derive(State, Debug)]
pub struct JerryCork {
    /// InstigatingTom (BeginPlay @1257: the Instigator).
    pub tom: Option<FighterId>,
    /// The DoOnce @67-102.
    pub popped: bool,
    /// AimDownProjectileMovement (the parent's CDO true).
    pub aim_down: bool,
    /// The world tick of the pop (Spin.PlayFromStart @34), -1 before it.
    pub pop_tick: i32,
    /// BounceLeft (the Spin's end yaw: 260 or -320).
    pub bounce_left: bool,
    /// ParticleSystem's visibility (hidden at the pop).
    pub trail: bool,
    /// ParticleSystem2's visibility (hidden in the template; ReflectedMulticast(true) shows it).
    pub reflect_fx: bool,
    /// IsReflected.
    pub reflected: bool,
}

impl Default for JerryCork {
    /// The field initial values.
    fn default() -> Self {
        JerryCork {
            tom: None,
            popped: false,
            aim_down: true,
            pop_tick: -1,
            bounce_left: false,
            trail: true,
            reflect_fx: false,
            reflected: false,
        }
    }
}

/// The cork's `aimDownTick` gate: AimDownProjectileMovement of its own (the pop clears it, ReflectedMulticast sets
/// it). Called by `actors::projectile::aim_down_on`.
pub fn aim_down_on(p: &Projectile, s: &JerryCork) -> bool {
    let _ = p;
    s.aim_down
}

/// UKismetMathLibrary NormalizeToRange (4.26): equal ends give 0 below, 1 at or above; reversed ends are swapped.
fn normalize_to_range(v: Real, a: Real, b: Real) -> Real {
    if a == b {
        return if v < a { 0.0 } else { 1.0 };
    }
    let (lo, hi) = (min(a, b), max(a, b));
    (v - lo) / (hi - lo)
}

fn clamp(v: Real, lo: Real, hi: Real) -> Real {
    if v < lo {
        lo
    } else if v > hi {
        hi
    } else {
        v
    }
}

fn lerp(a: Real, b: Real, t: Real) -> Real {
    a + t * (b - a)
}

/// The Jerry actor's owner (Tom).
fn owner_of(w: &World, j: ActorId) -> Option<FighterId> {
    w.actors.actors.get(j).map(|a| a.owner)
}

fn is_dead(w: &World, j: ActorId) -> bool {
    w.actors.actors.get(j).is_none_or(|a| a.dead)
}

// ---------------------------------------------------------------- the record of the live Jerry

/// Jerry's BeginPlay @13402 (CorkAmmo = CorkStartingAmmo) and his CDO's champagne variables, for an actor the record
/// does not belong to yet.
pub fn cork_begin_play(w: &mut World, j: ActorId) {
    let Some(o) = owner_of(w, j) else { return };
    let Some(seq) = w.actors.actors.get(j).map(|a| a.seq) else { return };
    if tj_of(w, o).is_none_or(|s| s.champagne.actor_id == seq) {
        return;
    }
    let (gate, n) = {
        let p = actor_p(w, ActorRef::Actor(j));
        let n = pnum(p, "CorkStartingAmmo");
        (p.get("ChampagneGate").and_then(mvs_data::value::Value::as_bool) != Some(false), n)
    };
    if n.is_none() {
        warn_text(w, "Jerry_SpawnedActor data missing: CorkStartingAmmo");
    }
    let Some(s) = tj_mut(w, o) else { return };
    let c = &mut s.champagne;
    c.actor_id = seq;
    c.gate = gate;
    c.gate_left = 0;
    c.start_time = 0.0;
    c.queued_facing = false;
    c.widget_visible = false;
    c.bottle_anim = name!("idle");
    c.bottle_tick = 0;
    c.pop_tick = -1;
    s.cork_ammo = n.map_or(0, to_int32);
}

/// The champagne record of the owner of `j` (after `cork_begin_play`).
fn rec(w: &mut World, j: ActorId) -> Option<(FighterId, &mut TomAndJerryChampagneState)> {
    cork_begin_play(w, j);
    let o = owner_of(w, j)?;
    Some((o, &mut tj_mut(w, o)?.champagne))
}

/// The ActivateObject gate of conditions 1, 3 and 4: ChampagneGate && CorkAmmo > 0.
fn ready(w: &mut World, j: ActorId) -> bool {
    cork_begin_play(w, j);
    let Some(o) = owner_of(w, j) else { return false };
    tj_of(w, o).is_some_and(|s| s.champagne.gate && s.cork_ammo > 0)
}

// ---------------------------------------------------------------- Jerry's champagne graph

/// ActivateObject conditions 1 / 4 (the gate), then Champagne Attack(Montage, FaceTom) @0-849: UpdateAnimState(),
/// CorkQuedFacing = FaceTom ? Tom's UE Y >= Jerry's (Tom at or left of him) : Tom's BPI_IsFacingLeft, Rotate(the same),
/// SetAmmoUI_Visuals(true) (@31552: Widget visible), PlayMontageMulticast(Montage).
pub fn champagne_attack(w: &mut World, j: ActorId, back: bool) {
    if is_dead(w, j) || !ready(w, j) {
        return;
    }
    let Some(o) = owner_of(w, j) else { return };
    update_anim_state(w, j);
    let left = if back { w.fighter(o).body.x <= jerry_of(w, j).map_or(0.0, |m| m.x) } else { facing(w, o) < 0.0 };
    if let Some((_, c)) = rec(w, j) {
        c.queued_facing = left;
    }
    rotate(w, j, left);
    if let Some((_, c)) = rec(w, j) {
        c.widget_visible = true;
    }
    play_montage(w, j, if back { MONTAGE_B } else { MONTAGE_F });
}

/// ActivateObject condition 3 (the gate), then MontageSectionJump(Shoot): with Champagne_F or _B the current active
/// montage of JerryMesh, MontageSectionJumpMulticast -> Montage_JumpToSection(Shoot) @30521.
pub fn champagne_shoot(w: &mut World, j: ActorId) {
    if is_dead(w, j) || !ready(w, j) {
        return;
    }
    let playing = jerry_of(w, j)
        .and_then(|m| m.anim.m.as_ref())
        .is_some_and(|m| !m.ended && !m.blending_out() && (m.montage.name == MONTAGE_F || m.montage.name == MONTAGE_B));
    if !playing {
        return;
    }
    crate::actors::actor_montage::montage_jump_to_section(w, ActorRef::Actor(j), name!("Shoot"));
}

/// ChampagneTimerFunction: ChampagneGate = true (the timer's end, and the hit react @22531).
pub fn champagne_timer_function(w: &mut World, j: ActorId) {
    if let Some((_, c)) = rec(w, j) {
        c.gate = true;
    }
}

/// CorkAmmoSetrMulticast(StackAmount) @31094: CorkAmmo, the widget's count, the Widget visible.
pub fn set_cork_ammo(w: &mut World, j: ActorId, n: i32) {
    cork_begin_play(w, j);
    let Some(o) = owner_of(w, j) else { return };
    if let Some(s) = tj_mut(w, o) {
        s.cork_ammo = n;
        s.champagne.widget_visible = true;
    }
}

/// CorkAmmoTracker(RemoveStack) @30572 (called with true alone, @2310): not being destroyed, CorkAmmo - 1 and the
/// widget's count; at 0 or less the Widget hidden @28231.
fn cork_ammo_tracker(w: &mut World, j: ActorId) {
    if is_dead(w, j) {
        return;
    }
    let Some(o) = owner_of(w, j) else { return };
    if let Some(s) = tj_mut(w, o) {
        s.cork_ammo -= 1;
        if s.cork_ammo <= 0 {
            s.champagne.widget_visible = false;
        }
    }
}

/// The named notifies of the Champagne montages' proxy (begin: the switch @25399-25609; end: @24965-24999). True when
/// `name` is one of them.
pub fn champagne_notify(w: &mut World, j: ActorId, name: Name, begin: bool) -> bool {
    if !begin {
        if name != name!("Champagne") {
            return false;
        }
        // @24965: Champagne hidden, PS_AttackChargeUP.Deactivate().
        if let Some(m) = jerry_mut(w, j) {
            m.champagne = false;
            m.charge_up = false;
        }
        return true;
    }
    cork_begin_play(w, j);
    let Some(o) = owner_of(w, j) else { return false };
    let tick = w.flow.tick;
    if name == name!("Champagne") {
        // @26591: Champagne visible, Champagne_Idle, ChampagneStartTime = GetGameTimeInSeconds, PS_AttackChargeUP.Activate.
        let time = w.fighter(o).clock.time;
        if let Some((_, c)) = rec(w, j) {
            c.bottle_anim = name!("idle");
            c.bottle_tick = tick;
            c.start_time = time;
        }
        if let Some(m) = jerry_mut(w, j) {
            m.champagne = true;
            m.charge_up = true;
        }
        true
    } else if name == name!("BottleShoot") {
        // @26227: Champagne.PlayAnimation(Champagne_Shoot).
        if let Some((_, c)) = rec(w, j) {
            c.bottle_anim = name!("shoot");
            c.bottle_tick = tick;
        }
        true
    } else if name == name!("LaunchCork_F") {
        // @26270-26542: X = (Tom's BPI_IsFacingLeft == CorkQuedFacing) ? 1 : -1.
        let fl = facing(w, o) < 0.0;
        let queued = tj_of(w, o).is_some_and(|s| s.champagne.queued_facing);
        launch_cork(w, j, if fl == queued { 1.0 } else { -1.0 }, fl);
        true
    } else if name == name!("LaunchCork_B") {
        // @25624-26178: X = -1 when (FacingLeft ? Tom's UE Y < Jerry's : Tom's UE Y >= Jerry's), else 1.
        let fl = facing(w, o) < 0.0;
        let ge = w.fighter(o).body.x <= jerry_of(w, j).map_or(0.0, |m| m.x);
        launch_cork(w, j, if if fl { !ge } else { ge } { -1.0 } else { 1.0 }, fl);
        true
    } else {
        false
    }
}

/// LaunchCork(LaunchVector (x, 0.5), OwnerFacingLeft) @0-2617.
fn launch_cork(w: &mut World, j: ActorId, lx: Real, owner_fl: bool) {
    cork_begin_play(w, j);
    let Some(o) = owner_of(w, j) else { return };
    let time = w.fighter(o).clock.time;
    let start = tj_of(w, o).map_or(0.0, |s| s.champagne.start_time);
    if let Some(s) = tj_mut(w, o) {
        s.champagne.gate = false;
    }
    // Lerp(1500, 3000, clamp(NormalizeToRange(now - ChampagneStartTime, 0.2, 1.0), 0, 1)) @760-929 / @2330-2499.
    let charge = lerp(1500.0, 3000.0, clamp(normalize_to_range(time - start, 0.2, 1.0), 0.0, 1.0));
    let mut speed = charge; // @2546: no clamp off an ally
    if let Some(ally) = jerry_of(w, j).and_then(|m| m.following) {
        // @92-1069 (IsValid(FollowingFighter) && IsFollowing): the ally's velocity along UE Y (sim -x), its sign turned
        // when the cork goes against the owner's facing, clamped to 1800, added, the sum clamped to 1200..5000.
        let gt = lx > 0.0;
        let same = (owner_fl && gt) || (!gt && !owner_fl);
        let v = -w.fighter(ally).body.vx;
        speed = clamp(clamp(if same { v } else { -v }, -1800.0, 1800.0) + charge, 1200.0, 5000.0);
    }
    // @1096-1227: the direction (0, X, Lerp(0.5, 0.1, NormalizeToRange(speed, 1200, 5000))), not normalised.
    let z = lerp(0.5, 0.1, normalize_to_range(speed, 1200.0, 5000.0));
    // @1278-1583: Tom's Comp_Spawner.CompSpawnActor(JerryCork_C, the world transform at Jerry's location + (0, 0, 80),
    // the direction, speed, speed), AttackID 0: Tom's attack count at the spawn (Comp_Spawner HandleActorPostSpawn
    // @2444-2747, comp_spawn_projectile's default).
    let (jx, jy) = jerry_of(w, j).map_or((0.0, 0.0), |m| (m.x, m.y));
    comp_spawn_projectile(w, o, CORK_N, jx, jy + 80.0, [Some(0.0), Some(lx), Some(z)], speed, speed, None);
    // @1756: PS_AttackChargeUP.Deactivate(); @1792: PS_TomJerry_ChampagnePop at the Champagne's Neck (the view).
    if let Some(m) = jerry_mut(w, j) {
        m.charge_up = false;
    }
    let tick = w.flow.tick;
    if let Some(s) = tj_mut(w, o) {
        s.champagne.pop_tick = tick;
    }
    if is_dead(w, j) {
        return; // IsActorBeingDestroyed @2151
    }
    if let Some(s) = tj_mut(w, o) {
        s.champagne.gate_left = to_int32(timer_frames(Some(CHAMPAGNE_TIMER)));
    }
    cork_ammo_tracker(w, j);
}

/// champagne.rs's part of the joined tick (after pawn.rs's): the record follows the live Jerry (corkBeginPlay), the
/// ChampagneTimer (Deviation above).
pub fn tick(w: &mut World, f: FighterId) {
    let Some(s) = tj_of(w, f) else { return };
    let live = match s.jerry_actor {
        Some(ActorRef::Actor(j)) => Some(j),
        _ => None,
    };
    let alive = live.filter(|&j| !is_dead(w, j));
    if let Some(j) = alive {
        cork_begin_play(w, j);
    }
    let seq = alive.and_then(|j| w.actors.actors.get(j).map(|a| a.seq));
    let Some(s) = tj_mut(w, f) else { return };
    let c = &mut s.champagne;
    if c.gate_left > 0 {
        c.gate_left -= 1;
        if c.gate_left == 0 && seq == Some(c.actor_id) {
            c.gate = true;
        }
    }
}

// ---------------------------------------------------------------- JerryCork

fn proj(w: &mut World, p: ProjectileId) -> Option<&mut Projectile> {
    w.actors.projectiles.get_mut(p)
}

fn cork(w: &mut World, p: ProjectileId) -> Option<&mut JerryCork> {
    match &mut w.actors.projectiles.get_mut(p)?.ext {
        ProjectileExt::JerryCork(c) => Some(c),
        _ => None,
    }
}

fn sound(w: &mut World, p: ProjectileId, ev: Name) {
    let Some(pr) = w.actors.projectiles.get(p) else { return };
    let e =
        Event::Sound { fighter: pr.owner_id, ev, x: Some(pr.x), y: Some(pr.y), hit: None, pooled: false, key: None };
    w.events.push(e);
}

/// The DoOnce @67 -> @102-628 -> @15: ProjectileGravityScale 8, AimDownProjectileMovement false, HitBoxOn? false and
/// UpdateHitbox, BounceLeft = Velocity.Y > 0, Velocity (0, Velocity.Y > 0 ? -400 : 400, 1500), both particle systems
/// hidden, UpdateMaxLifetimeWithAuthority(0.35), Spin.PlayFromStart. The lifetime from the age the call finds: one
/// tick ahead for the spawn's hit test (age 0, ahead of the cork's first move), none behind its move.
fn pop(w: &mut World, p: ProjectileId) {
    if cork(w, p).is_none_or(|c| c.popped) {
        return;
    }
    let mid = w.overlap_mid_frame;
    let tick = w.flow.tick;
    let Some(pr) = proj(w, p) else { return };
    projectile::set_gravity_scale(pr, 8.0);
    projectile::set_hitbox(pr, false);
    let left = pr.vx < 0.0; // UE Velocity.Y > 0
    pr.vx = if left { 400.0 } else { -400.0 };
    pr.vy = 1500.0;
    // a mover's mid-frame hit runs ahead of the cork's move
    let ahead = if mid || pr.age == 0 { 1 } else { 0 };
    projectile::update_max_lifetime(pr, 0.35, ahead);
    if let ProjectileExt::JerryCork(c) = &mut pr.ext {
        c.popped = true;
        c.aim_down = false;
        c.bounce_left = left;
        c.trail = false;
        c.reflect_fx = false;
        c.pop_tick = tick;
    }
}

fn cork_make(_w: &World, _f: FighterId, _r: &SpawnRequest) -> ProjectileExt {
    ProjectileExt::JerryCork(JerryCork::default())
}

/// UserConstructionScript: KnockBackDirectionVector = the Instigator's BPI_IsFacingLeft ? (1, 0.5) : (-1, 0.5)
/// (VelocityInsteadOfFacing takes the velocity's direction in its place); ReceiveBeginPlay @1247: the parent's,
/// InstigatingTom, BounceEvent bound to OnProjectileBounce.
fn cork_begin(w: &mut World, p: ProjectileId) {
    let owner = w.actors.projectiles.get(p).and_then(|pr| pr.owner);
    let fl = owner.is_some_and(|o| facing(w, o) < 0.0);
    let Some(pr) = proj(w, p) else { return };
    pr.hit.knock_back_direction_vector = Some(DataVec { x: Some(if fl { 1.0 } else { -1.0 }), y: Some(0.5), z: None });
    if let ProjectileExt::JerryCork(c) = &mut pr.ext {
        c.tom = owner;
    }
}

/// BounceEvent @2348 -> the pop.
fn cork_bounce_impact(w: &mut World, p: ProjectileId, nx: Real, ny: Real, ivx: Real, ivy: Real) -> bool {
    let _ = (nx, ny, ivx, ivy);
    pop(w, p);
    false
}

/// HitEnemyPlayer @2063: the parent's, sfx_cha_tomandjerry_cork_hit, the pop.
fn cork_hit_enemy(w: &mut World, p: ProjectileId, v: FighterId) {
    let _ = v;
    sound(w, p, name!("sfx_cha_tomandjerry_cork_hit"));
    pop(w, p);
}

/// HitNonPlayer @2161: the parent's (none of DestroyOnHitProjectile / Heavy / DestroyOnHitDestructibleEnvironment in
/// the class data: it does nothing), sfx_cha_tomandjerry_cork_impact, the pop.
fn cork_hit_non_player(w: &mut World, p: ProjectileId, by: ActorRef) {
    let _ = by;
    sound(w, p, name!("sfx_cha_tomandjerry_cork_impact"));
    pop(w, p);
}

/// OnHitAnything @2250: the parent's (its entry statement), sfx_cha_tomandjerry_cork_impact, the pop.
fn cork_hit_anything(w: &mut World, p: ProjectileId, victim: ActorRef) {
    let _ = victim;
    sound(w, p, name!("sfx_cha_tomandjerry_cork_impact"));
    pop(w, p);
}

fn proj_of(a: ActorRef) -> Option<ProjectileId> {
    match a {
        ActorRef::Projectile(p) => Some(p),
        _ => None,
    }
}

/// MulticastReflect @1479-1517: the parent's with ShouldPause = NewOwner != InstigatingTom.
fn cork_multicast_pause(w: &World, a: ActorRef, by: FighterId, pause: bool) -> bool {
    let _ = pause;
    let tom = proj_of(a).and_then(|p| w.actors.projectiles.get(p)).and_then(|pr| match &pr.ext {
        ProjectileExt::JerryCork(c) => c.tom,
        _ => None,
    });
    Some(by) != tom
}

/// @1545-1620: NewOwner a Pawn_TomAndJerry -> @1429.
fn cork_multicast(w: &mut World, a: ActorRef, by: FighterId, pause: bool) {
    let _ = pause;
    let Some(p) = proj_of(a) else { return };
    if tj_of(w, by).is_none() {
        return;
    }
    // @1429-1459: IsReflected, UpdateMaxLifetimeWithAuthority(5.0), ReflectedMulticast(true) @1625-1937: the root
    // Capsule's world scale 1.25, gravity 0, Bounciness 1 (the class's already), AimDownProjectileMovement,
    // sfx_cha_tomandjerry_tennis_return and _tennis_ball_reflected, ParticleSystem2 visible.
    let Some(pr) = proj(w, p) else { return };
    if let ProjectileExt::JerryCork(c) = &mut pr.ext {
        c.reflected = true;
        c.aim_down = true;
        c.reflect_fx = true;
    }
    projectile::update_max_lifetime(pr, 5.0, 1);
    projectile::set_actor_scale(pr, 1.25);
    projectile::set_gravity_scale(pr, 0.0);
    sound(w, p, name!("sfx_cha_tomandjerry_tennis_return"));
    sound(w, p, name!("sfx_cha_tomandjerry_tennis_ball_reflected"));
    let Some(pr) = proj(w, p) else { return };
    // @1391-1341: bIsHomingProjectile false, Bounciness 1.
    projectile::write_homing(pr, false);
    // @658-1122 (authority): the hit's packet, X = Normal(Velocity).Y > 0 ? 1 : -1 (sim: vx < 0), and UpdateHitBoxDamage
    // (the boxes on). IgnoreFacing (UE: X along +Y, so the victim goes along the cork's travel): the port's
    // projectileHitDir multiplies X by yawX (the travel side while the look-at's yaw follows the velocity), so X = 1
    // gives sign(vx).
    let h = &mut pr.hit;
    h.damage = Some(7.0);
    h.base_knock_back = Some(1250.0);
    h.knock_back_scaling = Some(10.0);
    h.knock_back_direction_vector = Some(DataVec { x: Some(1.0), y: Some(0.5), z: None });
    h.velocity_instead_of_facing = Some(false);
    h.ignore_facing = Some(true);
    h.additive_kb_mode = Some(DataEnum::Value(0));
    projectile::reflect_boxes_on(pr);
}

/// This file's registry entries (JerryCork_C).
pub fn register(r: &mut Registry) -> Result<(), RegistryError> {
    let kind = ProjectileHooks {
        make: Some(cork_make),
        begin: Some(cork_begin),
        bounce_impact: Some(cork_bounce_impact),
        hit_enemy: Some(cork_hit_enemy),
        hit_non_player: Some(cork_hit_non_player),
        hit_anything: Some(cork_hit_anything),
        reflect: ReflectHooks {
            multicast_pause: Some(cork_multicast_pause),
            multicast: Some(cork_multicast),
            ..ReflectHooks::default()
        },
        ..ProjectileHooks::default()
    };
    let entry = ActorEntry {
        calls_parent_hit_anything: true,
        ..ActorEntry::new(ActorClass::Projectile(ProjectileClass::JerryCork), ActorHooks::Projectile(kind))
    };
    r.actors.enter(CORK, entry)?;
    Ok(())
}
