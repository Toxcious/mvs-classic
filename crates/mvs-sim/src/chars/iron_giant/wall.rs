//! Iron Giant's wall: SpawnedPlatform_C017_Wall_C ([`WallPlatform`], a world
//! actor and a PlatformActor: its part is the actor's `plat`).
//!
//! SpawnedPlatform_C017_Wall: the platform Iron Giant holds out from the wall he slides on. A SpawnedPlatformParent
//! child (`world/spawned_platform.rs`: `plat` holds the parent's variables and boxes), a World.actors kind with nothing
//! drawn (the class has no mesh: its Capsule, Collision and WallbounceCollision alone).
//!
//! Spawned by C017_Wall_In's AnimNotify_Spawner (0.0833 to 0.5833 s, AttachToActor, socket None, translation
//! (-150, 0, 120), no DestroyAllActorsOnNotifyEnd): the AnimBP plays the sequence as the wall slide starts. Comp_Spawner
//! HandleActorPostSpawn @10335 K2_AttachToActor(owner, None), @10515 K2_SetActorRelativeTransform(the notify's): the
//! actor stands at the owner's actor location + the translation in the owner root's frame, whose yaw is +90 facing
//! left and -90 facing right (BaseFighter @138455 / @138974): its X is the facing, so the actor is 150 behind the Giant
//! (out from the wall he faces) and 120 over his actor location, and it turns with him.
//! Class data: DelayCollisionEnabled (Collision on 0.2 s after the parent's BeginPlay), UseWallbounceCollision false
//! (WallbounceCollision off: the parent's construction script), AlliesDropThrough and EnemiesDropThrough, Collision
//! BoxExtent (300, 32, 30) on the root with no place or turn (300 along the facing, 30 high), profile Platform; Die With
//! Instigator, DestroyOnExitedArena?. HitBoxTags name a HitBox no component carries: it deals nothing.
//!
//! ReceiveBeginPlay (ubergraph @206), ahead of the parent's: both boxes IgnoreActorWhenMoving(owner) @226 / @268 (the
//! boxes' own moves: an attached box is never swept, nothing in the port); the owner a BP_FighterCharacter @330 and a
//! BPI_BaseFighter @429 whose BPI_IsWallSliding (IsWallSliding) is true @508 -> bind WallSliding to its
//! OnIsWallSliding @568-591, the parent's ReceiveBeginPlay @637, then Delay(0.2) @683 -> @30: BPI_IsWallSliding again,
//! false -> K2_DestroyActor @15. A failed cast or a false test: K2_DestroyActor @648, then the parent's BeginPlay.
//! WallSliding(IsWallSliding) (@738): false -> K2_DestroyActor @753.
//! The parent's Delay(0.2) is registered inside its BeginPlay, ahead of this one's: in the tick both run out, the
//! Collision turns on before the recheck.

use crate::Real;
use crate::State;
use crate::actors::world_actor::{WorldActor, WorldActorState};
use crate::actors::{ActorClass, ActorId, ActorRef, WorldActorClass};
use crate::fighter::common::{actor_def, latent_frames, warn_text};
use crate::fighter::match_side::SpawnRequest;
use crate::fighter::{FighterId, actor_y, facing};
use crate::registry::entries::{ActorEntry, ActorHooks, SizeHooks, WorldActorHooks};
use crate::registry::props::{flag, num, obj, text};
use crate::registry::signals::OwnerSignal;
use crate::registry::{Registry, RegistryError};
use crate::world::World;
use crate::world::blast::{BlastBounds, blast_box_loop, check_blast_box_exit, new_blast_bounds, spawn_box};
use crate::world::spawned_platform::{SpawnedPlatform, platform_end, platform_step, scale_platform};

const WALL_PLATFORM: &str = "SpawnedPlatform_C017_Wall_C";

/// `WallPlatform` (SpawnedPlatform_C017_Wall_C).
#[derive(State, Debug, Default)]
pub struct WallPlatform {
    pub x: Real,
    pub y: Real,
    /// The relative translation: along the owner's facing, and up.
    pub rx: Real,
    pub rz: Real,
    pub age: i32,
    /// The age the BeginPlay's Delay(0.2) resumes at (0: none pending).
    pub recheck_at: Real,
    /// CheckBlastBoxTimer's next expiry.
    pub blast_at: Real,
    pub blast_bounds: BlastBounds,
}

/// The wall's place (its `x`, `y`).
pub fn place(_w: &World, _a: &WorldActor, s: &WallPlatform) -> Option<(Real, Real)> {
    Some((s.x, s.y))
}

/// `WallPlatform.place` (the origin): the owner's actor location + the translation turned by the owner's yaw.
fn origin(w: &World, f: FighterId, s: &mut WallPlatform) {
    s.x = w.fighter(f).body.x + s.rx * facing(w, f);
    s.y = actor_y(w, f) + s.rz;
}

/// `WallPlatform.place`, with the parts: the boxes at the actor.
fn follow(w: &mut World, id: ActorId) {
    let Some(a) = w.actors.actors.get(id) else { return };
    let f = a.owner;
    let (ox, oy, fc) = (w.fighter(f).body.x, actor_y(w, f), facing(w, f));
    let Some(a) = w.actors.actors.get_mut(id) else { return };
    let WorldActorState::WallPlatform(s) = &mut a.state else { return };
    s.x = ox + s.rx * fc;
    s.y = oy + s.rz;
    let (x, y) = (s.x, s.y);
    let Some(pl) = a.plat.as_mut() else { return };
    if let Some(c) = pl.collision_part() {
        c.place(x, y);
    }
    if let Some(c) = pl.wallbounce_part() {
        c.place(x, y);
    }
}

/// `wallSpawn`: the class's spawn (the kind's `spawn`).
fn spawn(w: &mut World, f: FighterId, r: &SpawnRequest) -> (WorldActorState, bool) {
    let np = crate::world::spawning::request_props(w, f, r);
    let t = obj(&np, "Transforms").and_then(|t| obj(t, "Translation"));
    let (rx, rz) = (t.and_then(|t| num(t, "X")).unwrap_or(0.0), t.and_then(|t| num(t, "Z")).unwrap_or(0.0));
    let mut s = WallPlatform {
        x: 0.0,
        y: 0.0,
        rx,
        rz,
        age: 0,
        recheck_at: 0.0,
        blast_at: 0.1,
        blast_bounds: new_blast_bounds(),
    };
    origin(w, f, &mut s);
    // Only C017_Wall_In's notify spawns the class: a spawn of another shape is refused.
    let socket = text(&np, "Socket");
    if !flag(&np, "AttachToActor")
        || matches!(socket, Some(s) if !s.is_empty() && s != "None")
        || flag(&np, "WorldTransforms")
    {
        warn_text(
            w,
            &format!("{WALL_PLATFORM}: a spawn that is not attached to the owner with socket None is not ported"),
        );
        return (WorldActorState::WallPlatform(s), true);
    }
    // ReceiveBeginPlay: not wall sliding -> K2_DestroyActor @648 (the owner is a fighter: both casts hold).
    if !w.fighter(f).wall.wall_sliding {
        return (WorldActorState::WallPlatform(s), true);
    }
    let p = actor_def(w, ActorRef::Fighter(f), r.cls).map(|d| &d.p);
    let force = p.is_some_and(|p| flag(p, "ForceSpawnPositionInsideBlastBox"));
    let doe = p.is_some_and(|p| flag(p, "DestroyOnExitedArena?"));
    // the parent's BeginPlay loop
    let dead = blast_box_loop(&spawn_box(w), &mut s.x, &mut s.y, None, force, &mut s.blast_bounds, doe);
    s.recheck_at = latent_frames(0.2); // Delay(0.2) @683, behind the parent's
    (WorldActorState::WallPlatform(s), dead)
}

/// The rest of the spawn once the World holds the actor (the kind's `spawned`): the constructor's SpawnedPlatform
/// with the half extents (X, Z) of the class's Collision and WallbounceCollision, then `follow` with its parts.
fn spawned(w: &mut World, id: ActorId) {
    let Some(a) = w.actors.actors.get(id) else { return };
    let (f, cls) = (a.owner, a.cls);
    let half = |k: &str| {
        let d = actor_def(w, ActorRef::Fighter(f), cls)?;
        let e = obj(obj(&d.components, k)?, "BoxExtent")?;
        Some((num(e, "X").unwrap_or(0.0), num(e, "Z").unwrap_or(0.0)))
    };
    let (c, wb) = (half("Collision"), half("WallbounceCollision"));
    let pl = SpawnedPlatform::new(w, f, cls, c, wb);
    if let Some(a) = w.actors.actors.get_mut(id) {
        a.plat = Some(pl);
    }
    follow(w, id);
}

// Unverified: the Delay counts the spawn frame (the actor ticks as newly spawned), as the parent's (`platform_step`).
/// `wallTick`.
fn step(w: &mut World, id: ActorId) {
    let Some(a) = w.actors.actors.get_mut(id) else { return };
    let WorldActorState::WallPlatform(s) = &mut a.state else { return };
    s.age += 1;
    platform_step(w, id);
    follow(w, id);
    let Some(a) = w.actors.actors.get(id) else { return };
    let f = a.owner;
    let doe = actor_def(w, ActorRef::Fighter(f), a.cls).is_some_and(|d| flag(&d.p, "DestroyOnExitedArena?"));
    let sliding = w.fighter(f).wall.wall_sliding;
    let Some(a) = w.actors.actors.get_mut(id) else { return };
    let WorldActorState::WallPlatform(s) = &mut a.state else { return };
    // Delay(0.2) -> @30: BPI_IsWallSliding false -> K2_DestroyActor @15.
    if s.recheck_at != 0.0 && crate::real(s.age) == s.recheck_at {
        s.recheck_at = 0.0;
        if !sliding {
            a.dead = true;
            return;
        }
    }
    // SpawnedActorParent CheckBlastBoxExit on its 0.1 s looping timer (DestroyOnExitedArena?), as Projectile.expiry.
    // (its expiry stored as a float32 sum, not `blast_timer_exit`'s double one)
    if doe && crate::consts::clock(crate::real(s.age - 1)) > f64::from(s.blast_at) {
        s.blast_at += 0.1;
        if check_blast_box_exit(s.x, s.y, &s.blast_bounds) {
            a.dead = true;
        }
    }
}

/// `size.scale`: Collision and WallbounceCollision are the root's children with no place or scale of their own: their
/// extents at the root's scale.
fn scale(w: &mut World, id: ActorId, k: Real, old: Real) {
    scale_platform(w, id, k / old);
    follow(w, id);
}

/// WallSliding, bound to the owner's OnIsWallSliding at BeginPlay: false -> K2_DestroyActor @753.
fn owner_signal(w: &mut World, id: ActorId, _f: FighterId, sig: OwnerSignal, v: bool) {
    if sig == OwnerSignal::WallSliding
        && !v
        && let Some(a) = w.actors.actors.get_mut(id)
    {
        a.dead = true;
    }
}

/// This file's registry entries (SpawnedPlatform_C017_Wall_C's kind): the parent's end and
/// BPI_IgnorePlatformCollision / BPI_IgnoreWallbounceCollision (the class overrides neither).
pub fn register(r: &mut Registry) -> Result<(), RegistryError> {
    let hooks = WorldActorHooks {
        spawn: Some(spawn),
        spawned: Some(spawned),
        size: Some(SizeHooks { scale: Some(scale), ..SizeHooks::default() }),
        step: Some(step),
        end: Some(platform_end),
        ignore_platform_collision: Some(crate::world::spawned_platform::ignore_platform_collision),
        ignore_wallbounce_collision: Some(crate::world::spawned_platform::ignore_wallbounce_collision),
        owner_signal: Some(owner_signal),
        ..WorldActorHooks::default()
    };
    r.actors.enter(
        WALL_PLATFORM,
        ActorEntry::new(ActorClass::World(WorldActorClass::SpawnedPlatformC017Wall), ActorHooks::World(hooks)),
    )
}
