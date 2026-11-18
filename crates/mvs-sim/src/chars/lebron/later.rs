//! LeBron's later actors: the dunk's dash to a thrown ball DashToBasketball_C
//! ([`DashToBasketball`], a world actor), the dormant C016AimPunchSlam_C (an empty kind) and the buff
//! Buff_C016_IgnitedDunk.
//!
//! An empty kind has no spawn, so the world's spawn of it says 'spawned actor not implemented (<class>)' once and
//! makes nothing, and AnimNotify_ActivateSpawned picks no actor of it.

use super::state::{BALL, set_is_teleport_dunking, set_teleport_dunk_available};
use crate::Real;
use crate::State;
use crate::actors::projectile::ProjectileExt;
use crate::actors::world_actor::WorldActorState;
use crate::actors::{ActorClass, ActorId, ActorRef, ProjectileId, WorldActorClass};
use crate::combat::BuffClass;
use crate::combat::boxes::obj as obj_type;
use crate::combat::buffs::{buff_of, spawn_buff};
use crate::combat::elem::elem_of;
use crate::consts::DELTA_SECONDS;
use crate::fighter::common::actor_data;
use crate::fighter::match_side::SpawnRequest;
use crate::fighter::moves::recall_move;
use crate::fighter::traces::sphere_overlap_actors;
use crate::fighter::{FighterId, actor_y, facing, set_facing};
use crate::name;
use crate::registry::entries::{ActorEntry, ActorHooks, BuffEntry, BuffHooks, SizeHooks, WorldActorHooks};
use crate::registry::props::{flag, num};
use crate::registry::signals::{BuffSignal, BuffSignalArg};
use crate::registry::{Registry, RegistryError};
use crate::world::World;
use crate::world::blast::{BlastBounds, blast_box_loop, blast_timer_exit, new_blast_bounds, spawn_box};
use crate::world::stage::ComponentMove;

const DASH: &str = "DashToBasketball_C";
const AIM_SLAM: &str = "C016AimPunchSlam_C";
const IGNITED_DUNK: &str = "Buff_C016_IgnitedDunk_C";

/// `DashToBasketball` (DashToBasketball_C). Spawned attached by C016_Special_Dunk1_Montage's AnimNotify_Spawner at
/// every dunk (0.0412 s), destroyed at the notify's end (DestroyAllActorsOnNotifyEnd) or with its Instigator. No tick
/// of its own: what it does after the BeginPlay is its MoveComponentTo latent action and that action's completion.
///
/// ReceiveBeginPlay @1786 (the parent's BeginPlay first):
/// - @1824 SphereOverlapActors(the world origin, 20000, [PhysicsBody, Pawn, WorldDynamic], BasketBallProjectile_C):
///   a ball's root capsule is a PhysicsBody body. The class filter is the class and its children: every such class
///   is a [`Ball`](super::ball::Ball) and only those are, so the `ext` is the filter.
/// - @1945-2780 kept (AddUnique @2712) when its AllowTeleportTo and its Comp_Actor_Destructible.TeamIndex equals the
///   owner's @2501-2583: the ball's TeamIndex is its owner's (SpawnedActorParent @11027-11258, SetTeamIndexOnSpawn),
///   so the ball's owner's team (the AllyPass ball's owner is the team mate who threw it, its Instigator LeBron).
/// - @2948 none kept: the event ends; nothing more happens until the destroy (the 1v1 case: AllowTeleportTo is
///   written true on the AllyPass ball alone).
/// - @2958-3917 the nearest by VSizeSquared(ball - owner location), strict < against MinimumDistance (1e12): on a tie
///   the first of the overlap's order (unverified: PhysX's order, the port's is the trace gather's).
/// - @3922-4124 IgnitedBasketball = the ball's CheckHasBuff(Buff_Elemental_Fire_C), read here.
/// - @4143 DashToBallVisuals: PS_C016_TrailDunk on the owner's mesh (the render's, from `trail`).
/// - @4166-4236 EndRotation = the owner's rotation now.
/// - @4460 MoveComponentTo(the owner's root, the ball's location now, EndRotation, no ease, MoveToTime, Move): the
///   target is fixed at the call; the ball flies on.
///
/// The action as Buff_MoveCompTo's (`start_buff_move` / `buff_move_tick`): each update writes the owner's root
/// outright (`recall_move`, no sweep, velocity kept) and runs its overlap update at the written place
/// (`root_moved`); the update whose float32 time reaches MoveToTime writes the target and runs the completion
/// @15-1294.
// Unverified: the action's first update is taken in the spawn frame (TG_NewlySpawned) and each one after every
// fighter's CMC of its frame (the world's actor pass); the dash actor is spawned inside the pawn's animation update
// and attached to the pawn, and its latent pass's place against the pawn's CMC is not read.
/// A destroy ahead of the completion drops the latent action unrun (UWorld::DestroyActor): a dead actor gets no
/// `step`, so no completion; LeBron stays where the last update put him.
///
/// Fields (the view draws the trail from the owner, `dead` and `trail`): `ball`, CurrentClosest (the target fixed at
/// the start; none: none kept); `ignited`, IgnitedBasketball (the target burned at the start: Buff_Elemental_Fire on
/// it; HotHands is asked at the completion @898, not here); `mv`, the MoveComponentTo action (none: none, or ended);
/// `trail`, PS_C016_TrailDunk is on: from the BeginPlay that found a ball until the actor's destroy (ReceiveDestroyed
/// @5146 -> DestroyVisuals destroys the components; the completion leaves them); `rot`, EndRotation (the owner's
/// facing at the start).
#[derive(State, Debug)]
pub struct DashToBasketball {
    pub ball: Option<ProjectileId>,
    pub ignited: bool,
    pub mv: Option<ComponentMove>,
    pub trail: bool,
    pub rot: Real,
    /// The actor's ticks.
    pub age: i32,
    /// SpawnedActorParent's CheckBlastBoxTimer: its next expiry.
    pub blast_at: crate::Clock,
    pub blast_bounds: BlastBounds,
}

impl Default for DashToBasketball {
    /// The field initial values.
    fn default() -> Self {
        DashToBasketball {
            ball: None,
            ignited: false,
            mv: None,
            trail: false,
            rot: 1.0,
            age: 0,
            blast_at: f64::from(0.1_f32),
            blast_bounds: new_blast_bounds(),
        }
    }
}

fn dash_mut(w: &mut World, id: ActorId) -> Option<&mut DashToBasketball> {
    match &mut w.actors.actors.get_mut(id)?.state {
        WorldActorState::DashToBasketball(s) => Some(s),
        _ => None,
    }
}

/// `dashBegin`: the BeginPlay's ball search and the MoveComponentTo call, into the new actor's state.
// @1824's types are [3, 2, 1] (PhysicsBody, Pawn, WorldDynamic). With the class filter only a ball can be kept, and a
// ball is found through its root capsule, a PhysicsBody; the port's balls have no Pawn or WorldDynamic body, so the
// query asks PhysicsBody alone: the same actors.
fn dash_begin(w: &mut World, f: FighterId, d: &mut DashToBasketball, move_to_time: Option<Real>) {
    let team = w.fighter(f).team.team;
    let found = sphere_overlap_actors(w, 0.0, 0.0, 20000.0, &[crate::real(obj_type::PHYSICS_BODY)], &[], -1.0);
    let mut best: Option<ProjectileId> = None;
    let mut min: Real = 1e12; // MinimumDistance @2958
    let (fx, fy) = (w.fighter(f).body.x, actor_y(w, f));
    for o in found {
        let ActorRef::Projectile(pid) = o else { continue };
        let Some(p) = w.actors.projectiles.get(pid) else { continue };
        let ProjectileExt::Ball(ball) = &p.ext else { continue };
        if p.dead {
            continue;
        }
        let Some(owner) = p.owner else { continue };
        if !ball.allow_teleport_to || w.fighter(owner).team.team != team {
            continue; // @2583
        }
        let dx = p.x - fx;
        let dy = p.y - fy;
        let dist = (dx * dx) + (dy * dy); // VSizeSquared @3395
        #[allow(clippy::neg_cmp_op_on_partial_ord, reason = "`!(d < min)` (Less_FloatFloat @3432)")]
        if !(dist < min) {
            continue;
        }
        min = dist;
        best = Some(pid);
    }
    let Some(best) = best else { return }; // @2948
    let (bx, by) = {
        let p = w.actors.projectiles.get(best).expect("a live projectile");
        (p.x, p.y)
    };
    d.ball = Some(best);
    d.ignited = elem_of(w, ActorRef::Projectile(best), BuffClass::BuffElementalFire).is_some(); // @3973-4124
    d.trail = true; // DashToBallVisuals @4143
    d.rot = facing(w, f); // EndRotation @4236
    let time = move_to_time.unwrap_or(0.2);
    d.mv = ComponentMove::call(None, fx, fy, bx, by, false, false, time, 0); // @4460
}

/// The kind's `spawn`: the parent's BeginPlay blast-box loop at the owner's location first; one that destroys the
/// actor leaves the rest of BeginPlay to run, and the MoveComponentTo it starts never updates.
fn dash_spawn(w: &mut World, f: FighterId, r: &SpawnRequest) -> (WorldActorState, bool) {
    let data = w.data.clone();
    let cls = w.names.str(r.cls);
    let a = actor_data(w, &data, ActorRef::Fighter(f), r.cls)
        .unwrap_or_else(|| panic!("{cls}: no actor data for its spawn (the importer gives every spawned class)"));
    let mut d = DashToBasketball::default();
    let (mut x, mut y) = (w.fighter(f).body.x, actor_y(w, f));
    let force = flag(&a.p, "ForceSpawnPositionInsideBlastBox");
    let sb = spawn_box(w);
    let dead =
        blast_box_loop(&sb, &mut x, &mut y, None, force, &mut d.blast_bounds, flag(&a.p, "DestroyOnExitedArena?"));
    dash_begin(w, f, &mut d, num(&a.p, "MoveToTime"));
    (WorldActorState::DashToBasketball(d), dead)
}

/// The latent action's update of this frame; the completion @15-1294 at its end.
fn dash_step(w: &mut World, id: ActorId) {
    let Some(a) = w.actors.actors.get(id) else { return };
    let (f, cls) = (a.owner, a.cls);
    let data = w.data.clone();
    let doe = actor_data(w, &data, ActorRef::Fighter(f), cls).is_some_and(|d| flag(&d.p, "DestroyOnExitedArena?"));
    // CheckBlastBoxTimer at the actor's place: attached with no transform, the owner's location.
    let (ox, oy) = (w.fighter(f).body.x, actor_y(w, f));
    let Some(s) = dash_mut(w, id) else { return };
    let frames = crate::real(s.age);
    s.age += 1;
    if blast_timer_exit(doe, &mut s.blast_at, &s.blast_bounds, frames, ox, oy) {
        if let Some(a) = w.actors.actors.get_mut(id) {
            a.dead = true;
        }
        return;
    }
    let Some(mut m) = s.mv else { return };
    if w.fighter(f).life.dead > 0.0 {
        return;
    }
    let done = m.update(DELTA_SECONDS);
    let (mx, my) = (m.x, m.y);
    if let Some(s) = dash_mut(w, id) {
        s.mv = Some(m);
    }
    recall_move(w, f, mx, my);
    // The write leaves the capsule where the target puts it (a ball less than his half height over a floor puts it in
    // the floor); the next CMC move starts in penetration and resolves it first (ResolvePenetrationImpl).
    w.fighter_mut(f).capsule.cap_resized = true;
    crate::combat::phases::root_moved(w, f);
    if !done {
        return;
    }
    let (rot, ignited, ball) = {
        let Some(s) = dash_mut(w, id) else { return };
        s.mv = None;
        (s.rot, s.ignited, s.ball)
    };
    // @35 the owner's K2_SetActorRotation(EndRotation).
    // Deviation: the port's turn is the facing setter, which also broadcasts FacingChanged; the build's bare
    // SetActorRotation does not run SetRotationLogic. No change while the dunk holds his facing.
    if facing(w, f) != rot {
        set_facing(w, f, rot);
    }
    // @87-137 K2_TeleportTo on the dash actor's own SceneComponent root: nothing reads its place (no sim effect).
    spawn_buff(w, f, BALL, 0.0, Some(0), Some(f), true, Some(f)); // @246 SpawnBuffActor(Buff_C016_Ball_Conditional_C, owner, owner, 0, 0.0)
    set_teleport_dunk_available(w, f, false); // @429
    set_is_teleport_dunking(w, f, true); // @566
    // @790 BPI_IncrementCoopActions: a statistic, not kept.
    // @898-1171 HotHands (asked now) and IgnitedBasketball (read at the start): IgnitedDunk, 1 stack, 0.0 s (no timer).
    if buff_of(w, f, name!("Buff_C016_HotHands_C")).is_some() && ignited {
        spawn_buff(w, f, name!("Buff_C016_IgnitedDunk_C"), 0.0, Some(1), Some(f), true, Some(f));
    }
    if let Some(b) = ball
        && w.actors.projectiles.get(b).is_some_and(|p| !p.dead)
    {
        crate::world::spawning::destroy_projectile(w, b, false); // @1255-1294 K2_DestroyActor (no BasketBallPickedUp)
    }
}

// ---- Buff_C016_IgnitedDunk

/// Buff_C016_IgnitedDunk (Buff_c016_IgnitedDunk): given by the dash's completion alone (1 stack, 0.0 s: no life
/// timer), taken by Dunk1's RemoveBuff state at its end (RemoveOnEnd) or with the owner's defeat
/// (DestroyBuffWhenOwnerDefeated). BeginPlay @10 (authority, the owner's Comp_Pawn_HitBoxHandler valid) binds its
/// HitEnemyPlayer to OnIgniteHit @178-252, EndPlay @313 unbinds it: the HitEnemy signal of the owner's own hit boxes
/// (the fighter hit path), not his projectiles' or the shockwave's. OnIgniteHit @625-770: the owner's
/// Comp_State_Manager.SpawnBuffActor(Buff_Fire_C, the hit fighter, the owner, 4 stacks, 0.0 s, BreakIfBlocked true,
/// SpawnOnAllies false) @770. C016IgniteDunkEnabledCondition (pawn.rs) reads its presence for the fire wall.
fn ignited_on(w: &mut World, f: FighterId, _b: crate::combat::BuffRef, s: BuffSignal, arg: &BuffSignalArg) -> bool {
    if s == BuffSignal::HitEnemy
        && let Some(v) = arg.victim
    {
        spawn_buff(w, v, name!("Buff_Fire_C"), 0.0, Some(4), Some(f), true, Some(f));
    }
    false
}

/// This file's registry entries: the dash, the dormant aim slam (an empty kind) and the buff.
pub fn register(r: &mut Registry) -> Result<(), RegistryError> {
    // Nothing of the dash reads the scale: its query has a fixed radius about the origin, its move is the owner's
    // root, and its own root (a SceneComponent) has no shape.
    let dash = WorldActorHooks {
        spawn: Some(dash_spawn),
        step: Some(dash_step),
        size: Some(SizeHooks::default()),
        ..WorldActorHooks::default()
    };
    r.actors
        .enter(DASH, ActorEntry::new(ActorClass::World(WorldActorClass::DashToBasketball), ActorHooks::World(dash)))?;
    // Dormant: spawned by C016_GroundAimSlam_Montage and activated by C016_Special_Dunk_DownSlam_Montage, and his data
    // names neither montage anywhere but as its own entry (no start table row, no branch, no jump).
    r.actors.enter(
        AIM_SLAM,
        ActorEntry::new(
            ActorClass::World(WorldActorClass::C016AimPunchSlam),
            ActorHooks::World(WorldActorHooks::default()),
        ),
    )?;
    // (FireWallHazard_C, which Dunk1's AnimNotify_ConditionalSpawner spawns behind C016IgniteDunkEnabledCondition, is
    // a shared class: `actors/firewall.rs`.)
    let ignited = BuffHooks { on: Some(ignited_on), ..BuffHooks::default() };
    r.buffs.enter(IGNITED_DUNK, BuffEntry::new(BuffClass::BuffC016IgnitedDunk, ignited))
}
