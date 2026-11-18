//! Stripe's gun: Comp_C023B_TargetTracker (offsets are its ubergraph's unless a
//! function is named), C023B_SpawnedGunShot ([`GunShot`], a projectile), AnimNotify_C023B_PauseAimer,
//! AnimNotify_C023B_ShootAtTarget, and C023B_AnimBP's aim update (@7755-9220) that the four Gun montages' Aim* slot
//! tracks blend by; joined to hitEnemy, playerKnockedOut, timerTick and animUpdate.
//!
//! - Comp_C023B_TargetTracker: a pawn component: its variables in `StripeGunState`, CurrentTarget in
//!   `StripeState::target` (a fighter or a spawned actor: the saw, HitProjectile), the AnimBP's aim in `aim_back` /
//!   `aim_direction`.
//! - C023B_SpawnedGunShot_C: a SpawnedActorParent child spawned by Comp_Spawner.CompSpawnActor, so a projectile kind.
//!   It has no mesh: what shows of it is PS_C023B_Gunshot_Impact at its BeginPlay (a projectileDeath event).
//! - C023B_AimWidget_C: no sim state; a render-only marker drawn at `StripeState::target` (it lives exactly while
//!   CurrentTarget is set).
//! - AnimNotify_C023B_PauseAimer_C, AnimNotify_C023B_ShootAtTarget_C: notify states (ShootAtTarget has no NotifyEnd).
//!
//! Calls out of other files: `tracker_hit_projectile` (the saw's HitLaunch @7789), `tracker_target_ended` (a spawned
//! actor's EndPlay: the saw's end hook), `tracker_hit_enemy` (BoomBox_SpawnedActor's HitEnemyPlayer binding, dash.rs),
//! `find_target` (AnimNotify_C023B_MoveCommand @1469, move_command.rs).

use super::pawn::gun_aim;
use super::state::{stripe, stripe_of, stripe_w};
use crate::Real;
use crate::State;
use crate::actors::projectile::{MakeOpts, class_def, set_hitbox, stop_simulating};
use crate::actors::world_actor::{actor_owner, actor_place};
use crate::actors::{ActorClass, ActorRef, ProjectileClass, ProjectileExt, ProjectileId};
use crate::combat::packet::DataVec;
use crate::consts::DELTA_SECONDS;
use crate::events::{Event, NotifyRef};
use crate::fighter::common::{sound_at_for, warn_text};
use crate::fighter::match_side::SpawnRequest;
use crate::fighter::traces::{
    OBJ_HURT_BOX, TRACE_PROJECTILE_GROUND_NO_PLATFORM, TraceHit, sphere_trace_multi_for_objects, sphere_trace_single,
};
use crate::fighter::{FighterId, actor_y, facing, mesh_wx, mesh_wy};
use crate::math::ue::{SMALL_NUMBER, deg_atan2, rot_from_x, safe_normal, size2d};
use crate::math::{max, min};
use crate::name;
use crate::registry::entries::{ActorEntry, ActorHooks, ProjectileHooks};
use crate::registry::props::{num, obj};
use crate::registry::{CharHooks, NotifyHandler, Registry, RegistryError};
use crate::world::World;
use crate::world::spawning::comp_spawn_projectile;

/// The gun shot's class.
pub const GUN_SHOT: &str = "C023B_SpawnedGunShot_C";

/// TimeTargeted: the component's class default (the component is not in the character data).
const TIME_TARGETED: Real = 1.5;
/// The no-target shot's reach @3030: SelectVector((0, 600, 0), (0, -600, 0), IsFacingLeft) added to the socket (UE Y:
/// along the facing, sim x 600 x facing).
const REACH: Real = 600.0;

/// `GunShot extends Projectile` (C023B_SpawnedGunShot_C). Its own state is none: the shot stays where it spawned.
#[derive(State, Debug, Default)]
pub struct GunShot {}

// ---------------------------------------------------------------- the TargetTimer
// K2_SetTimer, K2_PauseTimerHandle and K2_UnPauseTimerHandle in the tick groups: a set or an unpause is pending until
// the frame's timer pass, which starts it without counting that frame; an active one fires in the first timer pass
// whose counted deltas pass its rate. `StripeGunState::timer`: 0 invalid (never set, or cleared), 1 pending, 2 active,
// 3 paused, 4 fired.

/// @598-688: K2_ClearAndInvalidateTimerHandle(TargetTimer), then K2_SetTimer(TargetTimerFunction, TimeTargeted, false).
fn set_target_timer(w: &mut World, f: FighterId) {
    let g = &mut stripe_w(w, f).gun;
    g.timer = 1;
    g.rate = f64::from(TIME_TARGETED);
    g.elapsed = 0.0;
}

/// SetTimerPause(Pause) @0-171: K2_IsTimerPausedHandle || K2_IsTimerActiveHandle (paused = a paused timer; active =
/// one pending, active or running; an invalid or fired handle is neither), then K2_PauseTimerHandle or
/// K2_UnPauseTimerHandle. A pause keeps what is left of an active timer and the rate of a pending one; an unpause of a
/// paused timer is pending until the frame's timer pass (an unpause of one not paused does nothing).
pub fn set_timer_pause(w: &mut World, f: FighterId, pause: bool) {
    let Some(s) = crate::chars::stripe::stripe_mut(w, f) else { return };
    let g = &mut s.gun;
    if !matches!(g.timer, 1..=3) {
        return;
    }
    if pause {
        if g.timer == 2 {
            g.rate -= g.elapsed;
        }
        if g.timer == 1 || g.timer == 2 {
            g.timer = 3;
        }
    } else if g.timer == 3 {
        g.timer = 1;
    }
}

/// gun.rs's part of the joined timerTick: the fire first, then a pending timer goes active.
pub fn timer_tick(w: &mut World, f: FighterId) {
    let g = &mut stripe_w(w, f).gun;
    if g.timer == 2 {
        g.elapsed += f64::from(DELTA_SECONDS);
        if g.elapsed > g.rate {
            g.timer = 4;
            update_target(w, f, None); // TargetTimerFunction @0: UpdateTargetMulticast(null, false); nothing unbound
        }
    } else if g.timer == 1 {
        g.timer = 2;
        g.elapsed = 0.0;
    }
}

// ---------------------------------------------------------------- the target

/// UpdateTargetMulticast(NewTarget, Add) @4018: CurrentTarget = NewTarget, the AnimInstance's
/// BPI_C023B_UpdateAimTarget(CurrentTarget, Add) (the aim update below reads it as `StripeState::target`), the aim
/// widget made / attached or destroyed (the render marker). Every call passes Add = (NewTarget != null).
fn update_target(w: &mut World, f: FighterId, t: Option<ActorRef>) {
    stripe_w(w, f).target = t;
}

/// HitEnemy(HitPlayer) @1652 / HitProjectile(Projectile) @3994: TempTarget = the new one; then the old CurrentTarget's
/// binding is taken off: a fighter (a minion too) its OnExitedArena -= OnTargetExitArena @1821, anything else (a
/// spawned actor, or none: nothing) its OnEndPlay -= OnTargetDeath @1933. CurrentTarget = TempTarget @1862,
/// UpdateTargetMulticast(TempTarget, true) @1881; @716 a fighter gets OnExitedArena += OnTargetExitArena @557, anything
/// else OnEndPlay += OnTargetDeath @796; then the TargetTimer is set again @598. A binding is kept on an old target
/// that was not CurrentTarget when the next hit came (see `tracker_target_ended` / `target_exited_arena`).
fn retarget(w: &mut World, f: FighterId, t: ActorRef) {
    let cur = stripe(w, f).target;
    let g = &mut stripe_w(w, f).gun;
    match cur {
        Some(ActorRef::Fighter(id)) => g.exit_bound.retain(|&b| b != id),
        Some(ActorRef::Projectile(id)) => g.end_bound.retain(|&b| b != id),
        _ => {}
    }
    match t {
        ActorRef::Fighter(id) => {
            if !g.exit_bound.contains(&id) {
                g.exit_bound.push(id); // AddUnique
            }
        }
        ActorRef::Projectile(id) if !g.end_bound.contains(&id) => g.end_bound.push(id),
        _ => {}
    }
    update_target(w, f, Some(t));
    set_target_timer(w, f);
}

/// The owner's Comp_Pawn_HitBoxHandler.HitEnemyPlayer, bound to HitEnemy at the tracker's BeginPlay @1061 (the joined
/// hitEnemy: the pawn's own hits). BoomBox_SpawnedActor's own binding calls it too (dash.rs).
pub fn tracker_hit_enemy(w: &mut World, f: FighterId, vic: FighterId) {
    if stripe_of(w, f).is_some() {
        retarget(w, f, ActorRef::Fighter(vic));
    }
}

/// HitProjectile(Projectile) @3994: the saw's HitLaunch @7789 (saw.rs).
pub fn tracker_hit_projectile(w: &mut World, f: FighterId, pr: ProjectileId) {
    if stripe_of(w, f).is_some() {
        retarget(w, f, ActorRef::Projectile(pr));
    }
}

/// A spawned actor's EndPlay (AActor::EndPlay: ReceiveEndPlay, then OnEndPlay.Broadcast), for every tracker that bound
/// its OnEndPlay (@796): OnTargetDeath @0-64: CurrentTarget.OnEndPlay -= OnTargetDeath (a fighter or none: nothing
/// bound there), UpdateTargetMulticast(null, false). So an old target that was still bound clears whatever target the
/// tracker holds now. The ended actor's delegate goes with it. Caller: the saw's end hook (saw.rs), at its EndPlay.
pub fn tracker_target_ended(w: &mut World, pr: ProjectileId) {
    let all = w.pawns.clone().unwrap_or_else(|| w.players.clone()); // w.all
    for f in all {
        let Some(s) = stripe_of(w, f) else { continue };
        if !s.gun.end_bound.contains(&pr) {
            continue;
        }
        let cur = s.target;
        let g = &mut stripe_w(w, f).gun;
        g.end_bound.retain(|&b| b != pr);
        if let Some(ActorRef::Projectile(id)) = cur {
            g.end_bound.retain(|&b| b != id);
        }
        update_target(w, f, None);
    }
}

/// OnExitedArena of a fighter the tracker bound (@557) -> OnTargetExitArena @0-143: CurrentTarget cast to
/// BP_FighterCharacter (a spawned actor or none: return), its OnExitedArena -= OnTargetExitArena, then
/// UpdateTargetMulticast(null, false). The fighter that left stays bound when it was not CurrentTarget.
// Taken from CharHooks.fighter_exited_arena, in the OnExitedArena broadcast, which a minion's ring-out reaches too.
// The game mode's PlayerKnockedOut is not used: a minion's ring-out does not reach it, so a minion target would not
// be cleared until the TargetTimer.
fn target_exited_arena(w: &mut World, f: FighterId, loser: FighterId) {
    let Some(s) = stripe_of(w, f) else { return };
    if !s.gun.exit_bound.contains(&loser) {
        return;
    }
    let Some(ActorRef::Fighter(cur)) = s.target else { return };
    stripe_w(w, f).gun.exit_bound.retain(|&b| b != cur);
    update_target(w, f, None);
}

/// FindTarget @0-101: HasTarget = CurrentTarget != None, Target = CurrentTarget. Callers: FireAtTarget,
/// AnimNotify_C023B_MoveCommand @1469 (move_command.rs).
pub fn find_target(w: &mut World, f: FighterId) -> Option<ActorRef> {
    stripe_of(w, f).and_then(|s| s.target)
}

// ---------------------------------------------------------------- FireAtTarget @1979-3508

/// K2_GetActorLocation of a target / a hit actor (a fighter's actor location, a spawned actor's place, the origin for
/// what has none).
pub(super) fn loc(w: &World, a: Option<ActorRef>) -> (Real, Real) {
    match a {
        Some(ActorRef::Fighter(id)) => (w.fighter(id).body.x, actor_y(w, id)),
        Some(other) => actor_place(w, other).map_or((0.0, 0.0), |(x, y, _)| (x, y)),
        None => (0.0, 0.0),
    }
}

/// A hit actor's Comp_Actor_Destructible.TeamIndex: a fighter's team, a spawned actor's owner's; an actor without the
/// component reads None's (0).
fn team_of(w: &World, a: Option<ActorRef>) -> i32 {
    match a {
        Some(ActorRef::Fighter(id)) => w.fighter(id).team.team,
        Some(other) => actor_owner(w, other).map_or(0, |o| w.fighter(o).team.team),
        None => 0,
    }
}

/// The main mesh's GetSocketLocation(fml_un_L_uparm) @2269 / @3093 (a bare bone of the skeleton): the socket as the
/// boxes sample it (the pose of the frame).
fn muzzle(w: &World, f: FighterId) -> (Real, Real) {
    let ch = w.fighter(f).ch;
    let key = name!("socket:fml_un_L_uparm");
    let o = w.fixed.samplers.get(&ch).and_then(|b| {
        let si = *b.by_name.get(&key)?;
        b.offsets.get(usize::try_from(si).ok()?).copied()
    });
    let pose = o.and_then(|_| crate::fighter::anim::sampled_boxes(w, f));
    let (Some(o), Some(pose)) = (o, pose) else {
        warn_text(w, "C023B_SpawnedGunShot_C: socket fml_un_L_uparm not sampled (the actor location taken)");
        return (w.fighter(f).body.x, actor_y(w, f));
    };
    let o = usize::try_from(o).expect("a box offset");
    let r = (mesh_wx(w, f) + crate::real(pose[o]) * facing(w, f), mesh_wy(w, f) + crate::real(pose[o + 1]));
    crate::fighter::anim::put_pose(w, pose);
    r
}

/// TraceGunShot(StartLocation, EndLocation, ActorsToIgnore) @0-2262 (both ends with UE X 0: the play plane):
/// LineTraceMultiForObjects([HurtBox 18]) @330: the first hit (by distance) whose actor's TeamIndex differs from the
/// owner's gives LocalHitEnemyLocation (that actor's location) and LocalFoundEnemy @1047-1500;
/// LineTraceSingle(channel 5, ProjectileGroundNoPlatform) @1512. SpawnLocation @1648-2156: (found || single) ?
/// ((!single || |start - single| > |start - enemy|) ? enemy : single) : end. LocalHitEnemyLocation starts at (0, 0, 0)
/// @5, so a ground hit with no enemy found is compared against the world origin and the origin is taken when the
/// ground is farther from the start than it (the build's: ported as it is).
fn trace_gun_shot(
    w: &mut World,
    f: FighterId,
    s: (Real, Real),
    e: (Real, Real),
    ignore: &[Option<ActorRef>],
) -> (Real, Real) {
    let me = Some(ActorRef::Fighter(f));
    let mut found = false;
    let (mut hx, mut hy) = (0.0, 0.0);
    let mut out: Vec<TraceHit> = Vec::new();
    sphere_trace_multi_for_objects(w, s.0, s.1, e.0, e.1, 0.0, &[crate::real(OBJ_HURT_BOX)], ignore, me, &mut out);
    let my_team = w.fighter(f).team.team;
    for h in &out {
        if team_of(w, h.actor) == my_team {
            continue;
        }
        (hx, hy) = loc(w, h.actor);
        found = true;
        break;
    }
    let g = sphere_trace_single(w, s.0, s.1, e.0, e.1, 0.0, TRACE_PROJECTILE_GROUND_NO_PLATFORM, ignore, me);
    if !found && g.is_none() {
        return e;
    }
    match g {
        Some(g) if size2d([g.x - s.0, g.y - s.1]) <= size2d([hx - s.0, hy - s.1]) => (g.x, g.y),
        _ => (hx, hy),
    }
}

/// FireAtTarget: FindTarget @1979; with a target the trace runs from the muzzle to its location, the owner and the
/// target ignored @2190-2336; without one along the facing for 600 (@3001-3206, ignore [owner, None]). Then the owner's
/// Comp_Spawner.CompSpawnActor(C023B_SpawnedGunShot_C, at the SpawnLocation, rot 0, scale 1 (world transform),
/// direction (0, 0, 0), InitialSpeed 500, MaxSpeed 500, AttackID 0) @2540 / @3390 (the no-target call passes None as
/// the parent and the instigator; the spawner's own owner is the shot's either way). The spawn's
/// TriggerAbilityOnSpawnIndex (the class data's) spends the gun's ammo (`comp_spawn_projectile`: actorSpawnAbility).
/// Caller: AnimNotify_C023B_ShootAtTarget.
pub fn fire_at_target(w: &mut World, f: FighterId) {
    if stripe_of(w, f).is_none() {
        return;
    }
    let t = find_target(w, f);
    let m = muzzle(w, f);
    let e = if t.is_some() { loc(w, t) } else { (m.0 + REACH * if facing(w, f) < 0.0 { -1.0 } else { 1.0 }, m.1) };
    let at = trace_gun_shot(w, f, m, e, &[Some(ActorRef::Fighter(f)), t]);
    // AttackID 0: Comp_Spawner @2444-2747 takes the PlayerState attack count (the default)
    comp_spawn_projectile(
        w,
        f,
        name!("C023B_SpawnedGunShot_C"),
        at.0,
        at.1,
        [Some(0.0), Some(0.0), Some(0.0)],
        500.0,
        500.0,
        None,
    );
}

// ---------------------------------------------------------------- C023B_SpawnedGunShot
// Its data: MaxLifetime 0.1, HitBoxOn? false (on in BeginPlay below), DestroyedOnHit false, Die With Instigator,
// DestroyOnExitedArena?; HitBoxDamage 4 / 1000 / 7 / (1, 1), AttackTag Hit, HitPauseTime 0.1, HitEachActorOnlyOnce; its
// hit box the Sphere tagged HitBox (radius 90) on the root Capsule (bAbsoluteScale, no collision profile).
// - EnableProjectileMovement is not set (SpawnedActorParent's CDO false): its BeginPlay deactivates the movement
//   (@3777-3792), and the spawn's direction is zero: the shot stays where it was spawned. `ridden`: its root has no
//   collision profile, which nothing reads while it never moves (no platform warning).
// - Its rotation is the spawn's rot 0 (no tick of its own turns it): a facing hit's X takes ComingFromLeft false (actor
//   yaw 0; `projectile::yaw_x` returns -1 for it).

fn shot_make(_w: &World, _f: FighterId, _r: &SpawnRequest) -> ProjectileExt {
    ProjectileExt::GunShot(GunShot {})
}

fn shot_make_opts(_w: &World, _f: FighterId, _r: &SpawnRequest) -> MakeOpts {
    MakeOpts { ridden: true, ..MakeOpts::default() }
}

fn shot_construct(w: &World, p: &mut crate::actors::Projectile) {
    stop_simulating(p);
    // The Sphere's radius (attached to the absolute-scale Capsule: world scale 1).
    let d = class_def(w, p);
    let r = d.and_then(|d| obj(&d.components, "Sphere")).and_then(|c| num(c, "SphereRadius")).unwrap_or(0.0);
    let scale = d.and_then(|d| obj(&d.p, "HitBoxScale")).and_then(|v| num(v, "X")).unwrap_or(1.0);
    p.hit_radius = r * scale;
}

/// ReceiveBeginPlay @55-2312, after the parent's: FacingLeft = the owner's BPI_IsFacingLeft; d = |owner - shot| (actor
/// locations); base = Lerp(1100, 525, clamp(NormalizeToRange(d, 800, 3000), 0, 1)); u = unit(shot - (owner + (0, 0,
/// 150))) (UE (Y, Z)); a = DegAtan2(-u.Y, -u.Z) + 180, side = a in [45, 135] (InRange inclusive), other = a in
/// [225, 315]. KnockBackDirectionVector = (d <= 200 ? (FacingLeft ? 1 : -1) : side || other ? (side ? 1 : -1) : 0,
/// d <= 200 || side || other ? 1 : u.Z); BaseKnockBack = that Y < 0 ? Lerp(base, base x 0.65, |Y|) : base
/// @1521-1557; HitBoxOn? = true, UpdateHitbox @1593-1604. Then SpawnEmitterAtLocation(PS_C023B_Gunshot_Impact, the
/// shot, FindLookAtRotation(owner + (0, 0, 150), shot)) with its Team parameter @1812-2126, and
/// PF_Post_Event_At_Location(sfx_cha_C023b_gun_ricco) at the Capsule @2234.
fn shot_begin(w: &mut World, id: ProjectileId) {
    let Some(pr) = w.actors.projectiles.get(id) else { return };
    let (Some(o), px, py) = (pr.owner, pr.x, pr.y) else { return };
    let left = facing(w, o) < 0.0;
    let (ox, oy) = (w.fighter(o).body.x, actor_y(w, o));
    let d = size2d([px - ox, py - oy]);
    let alpha = min(max((d - 800.0) / (3000.0 - 800.0), 0.0), 1.0);
    let base = 1100.0 + (525.0 - 1100.0) * alpha;
    // UE (Y, Z) of shot - (owner + 150 up)
    let (d_y, d_z) = (-(px - ox), py - (oy + 150.0));
    let [_, u_y, u_z] = safe_normal([0.0, d_y, d_z], SMALL_NUMBER);
    let a = deg_atan2(-u_y, -u_z) + 180.0;
    let side = (45.0..=135.0).contains(&a);
    let other = (225.0..=315.0).contains(&a);
    let near = d <= 200.0;
    let h = if near {
        if left { 1.0 } else { -1.0 }
    } else if side || other {
        if side { 1.0 } else { -1.0 }
    } else {
        0.0
    };
    let v = if near || side || other { 1.0 } else { u_z };
    let bkb = if v < 0.0 { base + (base * 0.65 - base) * v.abs() } else { base };
    let pr = w.actors.projectiles.get_mut(id).expect("a live shot");
    pr.hit.base_knock_back = Some(bkb);
    pr.hit.knock_back_direction_vector = Some(DataVec { x: Some(h), y: Some(v), z: None });
    set_hitbox(pr, true);
    // FindLookAtRotation(from, to) 142f6cad0: MakeFromX(to - from), then FMatrix::Rotator.
    let rot = rot_from_x([0.0, d_y, d_z]);
    let fighter = w.fighter(o).id;
    w.events.push(Event::ProjectileDeath {
        cls: name!("C023B_SpawnedGunShot_C"),
        fighter,
        vfx: Some(name!("PS_C023B_Gunshot_Impact")),
        x: px,
        y: py,
        ox: 0.0,
        rot,
        scale: 1.0,
        tint: None,
        to: None,
    });
    sound_at_for(w, Some(o), name!("sfx_cha_C023b_gun_ricco"), px, py);
}

/// Its Comp_Pawn_HitBoxHandler.HitEnemyMulticast (ComponentBoundEvent) @2333: PF_Post_Event(sfx_cha_C023b_gun_hit) on
/// the shot.
fn shot_hit_enemy(w: &mut World, id: ProjectileId, _vic: FighterId) {
    let Some(pr) = w.actors.projectiles.get(id) else { return };
    let (o, x, y) = (pr.owner, pr.x, pr.y);
    if let Some(o) = o {
        sound_at_for(w, Some(o), name!("sfx_cha_C023b_gun_hit"), x, y);
    }
}

// ---------------------------------------------------------------- joined hooks

/// gun.rs's part of the joined hitEnemy (before bomb.rs's and dash.rs's): the tracker's HitEnemyPlayer binding.
pub fn hit_enemy(w: &mut World, f: FighterId, victim: FighterId) {
    tracker_hit_enemy(w, f, victim);
}

/// BlueprintUpdateAnimation's aim part, after the montage update (not `first`): `gun_aim` (the rule; the slot graph's
/// walk reads it live through pawn.rs `slot_input`), kept in `aim_direction` (GunAimDirection) / `aim_back` for the
/// view.
pub fn anim_update(w: &mut World, f: FighterId, _dilation: Real, first: bool) {
    if first {
        return;
    }
    let (dir, back) = gun_aim(w, f);
    let s = stripe_w(w, f);
    s.aim_direction = dir;
    s.aim_back = back;
}

/// The hooks this file joins in Stripe's CharHooks (`fighter_exited_arena`).
pub fn hooks(h: &mut CharHooks) {
    h.fighter_exited_arena = Some(target_exited_arena);
}

// ---------------------------------------------------------------- the notifies

/// AnimNotify_C023B_PauseAimer: Received_NotifyBegin @93 the owner's Comp_C023B_TargetTracker.SetTimerPause(true).
fn pause_begin(w: &mut World, f: FighterId, _n: NotifyRef, _key: i32) {
    set_timer_pause(w, f, true);
}

/// Received_NotifyEnd @93 SetTimerPause(false) (an owner without the component: a call on None, nothing).
fn pause_end(w: &mut World, f: FighterId, _n: NotifyRef, _key: i32) {
    set_timer_pause(w, f, false);
}

/// AnimNotify_C023B_ShootAtTarget: Received_NotifyBegin @93 the owner's TargetTracker.FireAtTarget (no NotifyEnd).
fn shoot_begin(w: &mut World, f: FighterId, _n: NotifyRef, _key: i32) {
    fire_at_target(w, f);
}

/// This file's registry entries (C023B_SpawnedGunShot_C, the notifies).
pub fn register(r: &mut Registry) -> Result<(), RegistryError> {
    r.actors.enter(
        GUN_SHOT,
        ActorEntry::new(
            ActorClass::Projectile(ProjectileClass::C023BSpawnedGunShot),
            ActorHooks::Projectile(ProjectileHooks {
                make: Some(shot_make),
                make_opts: Some(shot_make_opts),
                construct: Some(shot_construct),
                begin: Some(shot_begin),
                hit_enemy: Some(shot_hit_enemy),
                ..ProjectileHooks::default()
            }),
        ),
    )?;
    r.notifies.enter(
        "AnimNotify_C023B_PauseAimer_C",
        NotifyHandler { begin: Some(pause_begin), end: Some(pause_end), ..NotifyHandler::default() },
    )?;
    r.notifies.enter(
        "AnimNotify_C023B_ShootAtTarget_C",
        NotifyHandler { begin: Some(shoot_begin), ..NotifyHandler::default() },
    )
}
