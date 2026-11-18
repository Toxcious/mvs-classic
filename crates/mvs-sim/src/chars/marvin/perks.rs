//! Marvin's perks: C018_PerkPlatform_C (the UFO's child: ufo.rs's platform kind
//! with [`PerkUfoVars`] and the child functions below, which ufo.rs calls for an actor of this class),
//! C018PerkCannonBlast_C, C018PerkRocketExplosion_C, Buff_C018PerkHomingProjectile_C (homing.rs's actor hooks),
//! Buff_C018_Aim_C / Buff_C018_Missiles_C (read by pawn.rs's conditions), Buff_C018_Redirect_C (its listener:
//! [`redirect_reversed`]) and Buff_C018_PerkProjectileSpeed_C (its ext [`SpeedExt`]). Joined hook: `afterSpawnedActor`.

use crate::Real;
use crate::State;
use crate::actors::item::{item_movement_active, item_simulating_physics};
use crate::actors::projectile::{movement_active, set_actor_scale};
use crate::actors::world_actor::{WorldActorState, actor_p};
use crate::actors::{ActorClass, ActorId, ActorRef, ProjectileClass, WorldActorClass};
use crate::combat::buffs::end_buff;
use crate::combat::elem::{ElemExt, elem_buff, set_elem_ext, spawn_actor_buff};
use crate::combat::{BuffClass, BuffRef};
use crate::events::Event;
use crate::fighter::FighterId;
use crate::fighter::common::{buff_def, lookup_owner, warn_text};
use crate::name;
use crate::names::Name;
use crate::registry::entries::{ActorBuffHooks, ActorEntry, ActorHooks, BuffEntry, BuffHooks};
use crate::registry::props::{flag, num};
use crate::registry::signals::{ActorBuffArg, ActorBuffSignal};
use crate::registry::{Registry, RegistryError};
use crate::world::World;
use crate::world::pawns::match_fighters;
use crate::world::spawning::comp_spawn_projectile;

use super::{homing, ufo};

/// C018_PerkPlatform's class.
pub const PERK_PLATFORM: &str = "C018_PerkPlatform_C";

/// C018PerkCannonBlast's class.
pub const PERK_CANNON_BLAST: &str = "C018PerkCannonBlast_C";

/// C018PerkRocketExplosion's class.
pub const PERK_ROCKET_EXPLOSION: &str = "C018PerkRocketExplosion_C";

/// Buff_C018PerkHomingProjectile's class.
pub const PERK_HOMING_PROJECTILE: &str = "Buff_C018PerkHomingProjectile_C";

/// Buff_C018_Redirect's class.
pub const REDIRECT: &str = "Buff_C018_Redirect_C";

/// Buff_C018_PerkProjectileSpeed's class.
pub const PERK_PROJECTILE_SPEED: &str = "Buff_C018_PerkProjectileSpeed_C";

/// Buff_C018_Aim's class.
pub const AIM: &str = "Buff_C018_Aim_C";

/// Buff_C018_Missiles's class.
pub const MISSILES: &str = "Buff_C018_Missiles_C";

/// The perk platform's own variables (`PerkUfoVars`, the UFO's `child`; C018_PerkPlatform.json's defaults Blast Speed 800,
/// TimeBetweenCharges 1, LockOnGracePeriod 1, and the parent's TimeFromAimToFire 1 are read from the actor's data).
/// Timers: the world tick their timer fires in, 0 for none.
#[derive(State, Debug, Default)]
pub struct PerkUfoVars {
    /// AimActors (never cleared).
    pub aim_actors: Vec<FighterId>,
    /// HomingTarget: AimTarget kept at the fire @1058.
    pub homing_target: Option<FighterId>,
    /// The Instigator's Comp_Spawner AfterSpawnedActor bound to AfterSpawnedActor (@684-707).
    pub bound: bool,
    /// LockOnGracePeriod -> SearchForTargetsAfterGracePeriod (@252).
    pub grace_at: Real,
    /// TimeFromAimToFire -> AfterAimDelay (@171 / @400).
    pub aim_at: Real,
    /// TimeBetweenCharges -> FireBlastServer (@1857).
    pub charge_at: Real,
}

/// Buff_C018_PerkProjectileSpeed_C's extension on the actor (`SpeedExt`, `ElemExt::C018PerkProjectileSpeed`).
#[derive(State, Copy, PartialEq, Debug, Default)]
pub struct SpeedExt {
    /// ActiveSpeedModifier.
    pub active: Real,
    /// HitAnything / ReflectedDispatch bound.
    pub bound: bool,
}

fn with_child<R>(w: &mut World, a: ActorId, f: impl FnOnce(&mut PerkUfoVars) -> R) -> Option<R> {
    let WorldActorState::Ufo(u) = &mut w.actors.actors.get_mut(a)?.state else { return None };
    u.child.get_or_insert_with(PerkUfoVars::default);
    u.child.as_mut().map(f)
}

fn ufo_of(w: &World, a: ActorId) -> Option<&ufo::Ufo> {
    match &w.actors.actors.get(a)?.state {
        WorldActorState::Ufo(u) => Some(u),
        _ => None,
    }
}

fn ufo_mut(w: &mut World, a: ActorId) -> Option<&mut ufo::Ufo> {
    match &mut w.actors.actors.get_mut(a)?.state {
        WorldActorState::Ufo(u) => Some(u),
        _ => None,
    }
}

fn pnum(w: &World, a: ActorId, k: &str) -> Option<Real> {
    num(actor_p(w, ActorRef::Actor(a)), k)
}

/// `perkFindInitalAimTarget` (the child's FindInitalAimTarget; function @0-801): GetAllActorsOfClass(
/// Buff_C018HomingVictim_C) @16; for each, its owner's Comp_Actor_Destructible.TeamIndex != the platform's own @386 (no
/// validity guard on the owner) -> AddUnique(AimActors, owner) @579. Returns AimActors[0] @648-703 (None when empty:
/// Array_Get out of range). The array is not cleared, sorted or kept to this Marvin's marks (any Marvin's mark counts).
/// Called by ufo.rs for a perk platform.
// Unverified: GetAllActorsOfClass walks the level's actor list (about spawn order); taken as the fighters' order, then
// each one's buff list. Matters only with two marked enemies (2v2, FFA). The platform's team is its Instigator's.
pub fn perk_find_inital_aim_target(w: &mut World, a: ActorId) -> Option<FighterId> {
    let owner = w.actors.actors.get(a)?.owner;
    let team = w.fighter(owner).team.team;
    let mut found: Vec<FighterId> = Vec::new();
    for o in match_fighters(w) {
        let marked = w.fighter(o).buffs.buffs.iter().any(|b| b.class == BuffClass::BuffC018HomingVictim);
        if marked && w.fighter(o).team.team != team {
            found.push(o);
        }
    }
    with_child(w, a, |v| {
        for o in found {
            if !v.aim_actors.contains(&o) {
                v.aim_actors.push(o);
            }
        }
        v.aim_actors.first().copied()
    })
    .flatten()
}

/// `perkAfterBlastArmed` (AfterBlastArmed @2587; the parent's intro sets its timer @1405): AimTarget =
/// FindInitalAimTarget() @2610; @81 a target -> AimAtTargetVisualsMulticast(AimTarget) @125 (the child's override @2028
/// forwards to the parent's) and the TimeFromAimToFire timer -> AfterAimDelay @171; none -> the LockOnGracePeriod timer
/// -> SearchForTargetsAfterGracePeriod @252. Called by ufo.rs for a perk platform.
pub fn perk_after_blast_armed(w: &mut World, a: ActorId) {
    let t = perk_find_inital_aim_target(w, a);
    if let Some(u) = ufo_mut(w, a) {
        u.aim_target = t;
    }
    if t.is_some() {
        ufo::aim_at_target_visuals(w, a, t);
        let at = ufo::timer_at(w, Some(pnum(w, a, "TimeFromAimToFire").unwrap_or(0.0)));
        with_child(w, a, |v| v.aim_at = at);
    } else {
        let at = ufo::timer_at(w, Some(pnum(w, a, "LockOnGracePeriod").unwrap_or(0.0)));
        with_child(w, a, |v| v.grace_at = at);
    }
}

/// SearchForTargetsAfterGracePeriod (@2634): AimTarget = FindInitalAimTarget() @2657; a target -> the visuals and the
/// TimeFromAimToFire timer @354-400; none -> OnMaxLifetimeExpired @458. No Destroyed test.
fn search_after_grace(w: &mut World, a: ActorId) {
    let t = perk_find_inital_aim_target(w, a);
    if let Some(u) = ufo_mut(w, a) {
        u.aim_target = t;
    }
    if t.is_some() {
        ufo::aim_at_target_visuals(w, a, t);
        let at = ufo::timer_at(w, Some(pnum(w, a, "TimeFromAimToFire").unwrap_or(0.0)));
        with_child(w, a, |v| v.aim_at = at);
    } else {
        ufo::on_max_lifetime_expired(w, a);
    }
}

/// `perkFireBlast` (FireBlastServer @764): not Destroyed @793 and the Instigator's Comp_Spawner valid (every
/// fighter's) ->
/// - the attack count: GetInstigator() cast to BPI_PlayerState @933: Pawn_C018 does not implement it, so 0 @1919;
/// - HomingTarget = AimTarget @1058;
/// - CompSpawnActor(C018PerkCannonBlast_C, attach false, owner null, socket None, world transform true, at (0,
///   RocketSpawnPoint Y, Z) with rotation 0 and scale 0.5, direction Normal((0, 1, 0)), speed and max Blast Speed (800),
///   attack id 0) @1443: its AfterSpawnedActor (the binding below) comes inside the call;
/// - FireRocketVisualMulticast @1569;
/// - RemoveItem(AimActors, AimTarget) @1588; AimActors[0] valid @1703 -> AimTarget = it @1801, ReloadRocketMulticast
///   @1820 (the parent's reload: its own Delay to ReadyToFire, which this chain does not wait for), the
///   TimeBetweenCharges timer -> FireBlastServer @1857 and the perk countdown sound @15; else OnMaxLifetimeExpired
///   @2013;
/// - then (push_flow 1947) sfx_cha_C018_ufo_rocket_launch @1947, after either branch.
///
/// CurrentCannonAmmo and ReadyToFire are neither read nor written. Called by ufo.rs for a perk platform.
// Unverified: the direction (0, 1, 0) is a world direction; `comp_spawn_projectile` mirrors it by the owner's facing;
// the homing buff steers the rocket from its first tick either way. The scale 0.5 is set on the actor after the spawn
// (`comp_spawn_projectile` takes the transform's scale as 1): the spawn-place overlap test and the AfterSpawnedActor
// binding see the full-size capsule. The `owner null` argument: the spawner's owner is taken.
pub fn perk_fire_blast(w: &mut World, a: ActorId) {
    let Some(u) = ufo_of(w, a) else { return };
    if u.destroyed {
        return;
    }
    let target = u.aim_target;
    let Some(f) = w.actors.actors.get(a).map(|x| x.owner) else { return };
    with_child(w, a, |v| v.homing_target = target);
    let at = ufo::rocket_spawn_point(w, a);
    let speed = pnum(w, a, "Blast Speed").unwrap_or(800.0);
    // AttackID 0 from the failed BPI_PlayerState cast (@1919): Comp_Spawner's HandleActorPostSpawn puts the owner's
    // attack count in place of a 0, which is `comp_spawn_projectile`'s default
    let pr = comp_spawn_projectile(
        w,
        f,
        name!("C018PerkCannonBlast_C"),
        at.0,
        at.1,
        [Some(0.0), Some(1.0), Some(0.0)],
        speed,
        speed,
        None,
    );
    if let Some(pr) = pr
        && let Some(p) = w.actors.projectiles.get_mut(pr)
        && !p.dead
    {
        let s = p.scale * 0.5;
        set_actor_scale(p, s);
    }
    ufo::fire_rocket_visual(w, a);
    let next = with_child(w, a, |v| {
        v.aim_actors.retain(|o| Some(*o) != target);
        v.aim_actors.first().copied()
    })
    .flatten();
    let fighter = w.fighter(f).id;
    let (x, y) = ufo_of(w, a).map_or((0.0, 0.0), |u| (u.x, u.y));
    if let Some(n) = next {
        if let Some(u) = ufo_mut(w, a) {
            u.aim_target = Some(n);
        }
        ufo::reload_rocket(w, a);
        let at = ufo::timer_at(w, Some(pnum(w, a, "TimeBetweenCharges").unwrap_or(0.0)));
        with_child(w, a, |v| v.charge_at = at);
        w.events.push(Event::Sound {
            fighter,
            ev: name!("sfx_cha_C018_ufo_rocket_countdown_alarm_perk_version"),
            x: Some(x),
            y: Some(y),
            hit: None,
            pooled: false,
            key: None,
        });
    } else {
        ufo::on_max_lifetime_expired(w, a);
    }
    w.events.push(Event::Sound {
        fighter,
        ev: name!("sfx_cha_C018_ufo_rocket_launch"),
        x: Some(x),
        y: Some(y),
        hit: None,
        pooled: false,
        key: None,
    });
}

/// `perkTimers`: the child's timers, in the world's timer pass (none loops). AfterAimDelay is the parent's (@21884): not
/// Destroyed -> FireBlastServer (the child's). Called by ufo.rs for a perk platform.
pub fn perk_timers(w: &mut World, a: ActorId) {
    let tick = crate::real(w.flow.tick);
    let Some((grace, aim, charge)) = with_child(w, a, |v| (v.grace_at, v.aim_at, v.charge_at)) else { return };
    if grace > 0.0 && tick >= grace {
        with_child(w, a, |v| v.grace_at = 0.0);
        search_after_grace(w, a);
    }
    if aim > 0.0 && tick >= aim {
        with_child(w, a, |v| v.aim_at = 0.0);
        if ufo_of(w, a).is_some_and(|u| !u.destroyed) {
            perk_fire_blast(w, a);
        }
    }
    if charge > 0.0 && tick >= charge {
        with_child(w, a, |v| v.charge_at = 0.0);
        perk_fire_blast(w, a);
    }
}

/// `perkBegin` (ReceiveBeginPlay @749): the parent's, then (authority @473) the Instigator's Comp_Spawner
/// AfterSpawnedActor bound to AfterSpawnedActor @684-707. Nothing unbinds it: the binding dies with the actor. Called by
/// ufo.rs for a perk platform.
pub fn perk_begin(w: &mut World, a: ActorId) {
    with_child(w, a, |v| v.bound = true);
}

/// AfterSpawnedActor(SpawnedActor) (@2048), for each live perk platform of this Marvin that bound it:
/// IsSoftRef(the actor, C018PerkCannonBlast) @2048 (the exact class) -> the Instigator's Comp_State_Manager
/// .SpawnBuffActor(Buff_C018PerkHomingProjectile_C, the actor, the Instigator, 0 stacks, 0.0, BreakIfBlocked true, false)
/// @2382 (swept on the actor's own buffs: SweepBuffState reads its Owner argument's Comp_Actor_Parent) -> the cast -> its
/// ServerHomingInfo(HomingTarget) @2541. The joined hook (`perks.rs`'s `afterSpawnedActor`).
pub fn after_spawned_actor(w: &mut World, f: FighterId, a: ActorRef) {
    let ActorRef::Projectile(pr) = a else { return };
    if w.actors.projectiles.get(pr).is_none_or(|p| p.cls != name!("C018PerkCannonBlast_C")) {
        return;
    }
    let mut i = 0;
    while let Some(&o) = w.fighter(f).items.spawned_actors.get(i) {
        i += 1;
        let ActorRef::Actor(id) = o else { continue };
        let Some(x) = w.actors.actors.get(id) else { continue };
        if x.class != WorldActorClass::C018PerkPlatform || x.dead {
            continue;
        }
        let Some(target) = ufo_of(w, id).and_then(|u| u.child.as_ref()).filter(|c| c.bound).map(|c| c.homing_target)
        else {
            continue;
        };
        // spawn_actor_buff gives the serial; ServerHomingInfo takes the entry's place in the list.
        // The build writes the buff it spawned.
        if let Some(s) = spawn_actor_buff(w, a, name!("Buff_C018PerkHomingProjectile_C"), 0, 0.0, true, Some(f))
            && let Some(b) = crate::combat::elem::buff_place(w, a, s)
        {
            homing::server_homing_info(w, a, b, target);
        }
    }
}

/// The Redirect's listener (`onProjectileReversed`'s; ReceiveBeginPlay @212 binds the owner's ProjectileReversed to
/// OnProjectileReversed @143-166, no unbind: the binding dies with the buff, which DestroyBuffWhenOwnerDefeated ends at
/// a defeat): OnProjectileReversed(Actor) @227: PrintString of its name @264 (a development print: nothing in the game);
/// the owner's Comp_State_Manager.SpawnBuffActor(Buff_C018_PerkProjectileSpeed_C, the actor, the owner, Actor.GetOwner()
/// == the owner ? 1 : 3 stacks, 0.0, BreakIfBlocked true, false) @561: the count is by the actor's owner, not by team.
/// One run per live Redirect buff of `f`. Called by reverse.rs's [`super::reverse::projectile_reversed`].
pub fn redirect_reversed(w: &mut World, f: FighterId, a: ActorRef) {
    let n = w.fighter(f).buffs.buffs.iter().filter(|b| b.class == BuffClass::BuffC018Redirect).count();
    for _ in 0..n {
        let stacks = if lookup_owner(w, a) == f { 1 } else { 3 };
        perk_projectile_speed(w, a, stacks, f);
    }
}

// ---------------------------------------------------------------- Buff_C018_PerkProjectileSpeed
// A Buff_SlowProjectile child with its own GetSpeedModifier (1.0 + CurrentStackAmount x SpeedReduction 0.1: a speed-up,
// 1.1 for one stack, 1.3 for three), AddStack and RemoveStack (the parent's, then on authority RemoveSpeedBuff and
// ApplySpeedBuff again: @261 / @285 -> @222 -> @10). Class defaults: CanStack, MaxStacks 10, SizeModsPerStack (0.15,
// 0.15, 0.15), EffectGravity false (the parent's).
// ReceiveBeginPlay (the parent's @770): the owner no SpawnedActorParent -> K2_DestroyActor @751; else BuffParentActor's
// (CurrentStackAmount = SpawnedStackCount @1207), then ApplySpeedBuff @879: an active ProjectileMovement -> Velocity and
// MaxSpeed x GetSpeedModifier @147 / @287, ActiveSpeedModifier = it @359; else the root Capsule simulating physics @619
// -> its physics linear velocity x it @868 and ActiveSpeedModifier = it @972; else nothing (ActiveSpeedModifier stays
// 0). Then (authority, a Comp_Pawn_HitBoxHandler @1643-1777) the handler's HitAnything and the owner's ReflectedDispatch
// bound @1862-1949: both K2_DestroyActor (@2543 / @2562).
// ReceiveEndPlay @10: RemoveSpeedBuff @267: Velocity x (1 / ActiveSpeedModifier) @110, MaxSpeed = GetMaxSpeed() x
// (1 / ActiveSpeedModifier) @301 (the reciprocal a float32 before the products), whatever the component does now.
// Not here: the VFX (presentation), Comp_RunAlongEdgeTrace's Move Speed Multiplier (no ported actor has it), the gravity
// part (EffectGravity false).
// Deviation: an item's ProjectileMovement velocity while it lies (physics on) is not kept apart from its body's: the
// end's velocity write is skipped for an item that does not fly; with ActiveSpeedModifier 0 (neither branch applied)
// RemoveSpeedBuff's 1 / 0 is not written.
// Deviation: a stack added to a live buff runs AddStack -> UpdateCurrentStates -> UpdatePlayerStatus -> ApplySizeBuffs
// (BuffParentActor @311-382, Comp_State_Manager @968): the actor's size by the new count. Not done here: the stack path
// of `spawn_actor_buff` does not run it (the size stays the spawn's).

/// The ProjectileMovement state the speed buff reads of a projectile or an item.
struct Pmc {
    vx: Real,
    vy: Real,
    max_speed: Real,
    active: bool,
    physics: bool,
}

/// A SpawnedActorParent the port knows (the cast @30 / @800): a projectile or an item.
fn pmc(w: &World, a: ActorRef) -> Option<Pmc> {
    match a {
        ActorRef::Projectile(id) => {
            let p = w.actors.projectiles.get(id)?;
            Some(Pmc { vx: p.vx, vy: p.vy, max_speed: p.max_speed, active: movement_active(p), physics: false })
        }
        ActorRef::Item(id) => {
            let it = w.actors.items.get(id)?;
            Some(Pmc {
                vx: it.vx,
                vy: it.vy,
                max_speed: it.max_speed,
                active: item_movement_active(w, id),
                physics: item_simulating_physics(w, id),
            })
        }
        _ => None,
    }
}

fn set_v(w: &mut World, a: ActorRef, vx: Real, vy: Real) {
    match a {
        ActorRef::Projectile(id) => {
            if let Some(p) = w.actors.projectiles.get_mut(id) {
                (p.vx, p.vy) = (vx, vy);
            }
        }
        ActorRef::Item(id) => {
            if let Some(it) = w.actors.items.get_mut(id) {
                (it.vx, it.vy) = (vx, vy);
            }
        }
        _ => {}
    }
}

fn set_max_speed(w: &mut World, a: ActorRef, m: Real) {
    match a {
        ActorRef::Projectile(id) => {
            if let Some(p) = w.actors.projectiles.get_mut(id) {
                p.max_speed = m;
            }
        }
        ActorRef::Item(id) => {
            if let Some(it) = w.actors.items.get_mut(id) {
                it.max_speed = m;
            }
        }
        _ => {}
    }
}

fn speed_class() -> Name {
    name!("Buff_C018_PerkProjectileSpeed_C")
}

/// The class defaults of the speed buff.
fn speed_reduction(w: &World, a: ActorRef) -> Real {
    let own = ActorRef::Fighter(lookup_owner(w, a));
    buff_def(w, own, speed_class()).and_then(|d| num(d, "SpeedReduction")).unwrap_or(0.1)
}

/// GetSpeedModifier: 1.0 + Multiply_IntFloat(CurrentStackAmount, SpeedReduction).
fn speed_modifier(stacks: i32, reduction: Real) -> Real {
    1.0 + (crate::real(stacks) * reduction)
}

fn set_active(w: &mut World, a: ActorRef, serial: u32, ext: SpeedExt) {
    set_elem_ext(w, a, serial, ElemExt::C018PerkProjectileSpeed(ext));
}

fn ext_of(w: &World, a: ActorRef, serial: u32) -> Option<SpeedExt> {
    match crate::combat::elem::elem_of(w, a, BuffClass::BuffC018PerkProjectileSpeed)? {
        b if b.serial == serial => match b.ext {
            ElemExt::C018PerkProjectileSpeed(e) => Some(e),
            _ => Some(SpeedExt::default()),
        },
        _ => None,
    }
}

/// ApplySpeedBuff(ProjectileMovement, the actor).
fn apply_speed(w: &mut World, a: ActorRef, serial: u32, stacks: i32) {
    let m = speed_modifier(stacks, speed_reduction(w, a));
    let Some(p) = pmc(w, a) else { return };
    let mut e = ext_of(w, a, serial).unwrap_or_default();
    if p.active {
        set_v(w, a, p.vx * m, p.vy * m);
        set_max_speed(w, a, p.max_speed * m);
        e.active = m;
        set_active(w, a, serial, e);
    } else if matches!(a, ActorRef::Item(_)) && p.physics {
        set_v(w, a, p.vx * m, p.vy * m);
        e.active = m;
        set_active(w, a, serial, e);
    }
}

/// RemoveSpeedBuff(ProjectileMovement).
fn remove_speed(w: &mut World, a: ActorRef, serial: u32) {
    let act = ext_of(w, a, serial).map_or(0.0, |e| e.active);
    if act == 0.0 {
        return;
    }
    let inv = 1.0 / act;
    let Some(p) = pmc(w, a) else { return };
    if matches!(a, ActorRef::Projectile(_)) || p.active {
        set_v(w, a, p.vx * inv, p.vy * inv);
    }
    set_max_speed(w, a, p.max_speed * inv);
}

/// The Redirect's SpawnBuffActor on the reversed actor `a`. A live buff of the class takes the new stacks in the sweep
/// (no new buff); its AddStack override then runs RemoveSpeedBuff and ApplySpeedBuff with the new count.
fn perk_projectile_speed(w: &mut World, a: ActorRef, stacks: i32, by: FighterId) {
    let class = BuffClass::BuffC018PerkProjectileSpeed;
    let live = crate::combat::elem::elem_of(w, a, class).map(|b| b.serial);
    spawn_actor_buff(w, a, speed_class(), stacks, 0.0, true, Some(by));
    let Some(serial) = live else { return };
    let Some(b) = crate::combat::elem::elem_of(w, a, class).filter(|b| b.serial == serial) else { return };
    let n = b.stacks;
    if pmc(w, a).is_none() {
        return;
    }
    remove_speed(w, a, serial);
    apply_speed(w, a, serial, n);
}

/// Buff_C018_PerkProjectileSpeed's fighter `begin`: ended at once (the owner is no SpawnedActorParent @751; no caller
/// gives one).
fn speed_begin(w: &mut World, f: FighterId, b: BuffRef, _from: Option<FighterId>, _duration: Option<Real>) {
    end_buff(w, f, b);
}

/// Buff_C018_PerkProjectileSpeed's `actor.begin` (ApplySpeedBuff): `true` destroys the buff (a failed cast).
fn speed_actor_begin(w: &mut World, a: ActorRef, b: u32, _from: Option<FighterId>) -> bool {
    let Some(buff) = elem_buff(w, a, b) else { return false };
    let (serial, stacks) = (buff.serial, buff.stacks);
    if pmc(w, a).is_none() {
        warn_text(
            w,
            "Buff_C018_PerkProjectileSpeed_C: the owner is no spawned actor the port knows (destroyed as a failed cast)",
        );
        return true;
    }
    let own = ActorRef::Fighter(lookup_owner(w, a));
    if buff_def(w, own, speed_class()).is_some_and(|d| flag(d, "EffectGravity")) {
        warn_text(w, "Buff_C018_PerkProjectileSpeed_C: EffectGravity is not ported (the gravity scale is left alone)");
    }
    set_active(w, a, serial, SpeedExt { active: 0.0, bound: true });
    apply_speed(w, a, serial, stacks);
    false
}

/// Buff_C018_PerkProjectileSpeed's `actor.on` (HitAnything / ReflectedDispatch): destroys the actor, bound by the begin.
fn speed_actor_on(w: &mut World, a: ActorRef, b: u32, s: ActorBuffSignal, _arg: Option<&ActorBuffArg>) -> bool {
    matches!(s, ActorBuffSignal::HitAnything | ActorBuffSignal::Reflected)
        && matches!(
            elem_buff(w, a, b).map(|x| x.ext),
            Some(ElemExt::C018PerkProjectileSpeed(SpeedExt { bound: true, .. }))
        )
}

/// Buff_C018_PerkProjectileSpeed's `actor.end` (RemoveSpeedBuff).
fn speed_actor_end(w: &mut World, a: ActorRef, b: u32) {
    let Some(serial) = elem_buff(w, a, b).map(|x| x.serial) else { return };
    remove_speed(w, a, serial);
}

/// This file's registry entries: the perk platform, cannon blast and rocket (ufo.rs's kinds, parents the base
/// classes; the perk blast's BPI_ReverseMoveDirection is the base's, so no `reverse_move` of its own), then the buffs.
pub fn register(r: &mut Registry) -> Result<(), RegistryError> {
    r.actors.enter(PERK_PLATFORM, ufo::platform_entry(WorldActorClass::C018PerkPlatform, &[ufo::PLATFORM]))?;
    r.actors.enter(
        PERK_CANNON_BLAST,
        ActorEntry {
            parents: &[ufo::CANNON_BLAST],
            foreign_owner: true,
            ..ActorEntry::new(
                ActorClass::Projectile(ProjectileClass::C018PerkCannonBlast),
                ActorHooks::Projectile(ufo::cannon_blast_hooks()),
            )
        },
    )?;
    r.actors.enter(
        PERK_ROCKET_EXPLOSION,
        ufo::rocket_explosion_entry(WorldActorClass::C018PerkRocketExplosion, &[ufo::ROCKET_EXPLOSION]),
    )?;
    r.buffs.enter(
        PERK_HOMING_PROJECTILE,
        BuffEntry {
            actor: Some(homing::projectile_actor()),
            ..BuffEntry::new(BuffClass::BuffC018PerkHomingProjectile, BuffHooks::default())
        },
    )?;
    r.buffs.enter(AIM, BuffEntry::new(BuffClass::BuffC018Aim, BuffHooks::default()))?;
    r.buffs.enter(MISSILES, BuffEntry::new(BuffClass::BuffC018Missiles, BuffHooks::default()))?;
    r.buffs.enter(REDIRECT, BuffEntry::new(BuffClass::BuffC018Redirect, BuffHooks::default()))?;
    let speed = ActorBuffHooks {
        begin: Some(speed_actor_begin),
        on: Some(speed_actor_on),
        end: Some(speed_actor_end),
        ..ActorBuffHooks::default()
    };
    r.buffs.enter(
        PERK_PROJECTILE_SPEED,
        BuffEntry {
            actor: Some(speed),
            ..BuffEntry::new(
                BuffClass::BuffC018PerkProjectileSpeed,
                BuffHooks { begin: Some(speed_begin), ..BuffHooks::default() },
            )
        },
    )
}
