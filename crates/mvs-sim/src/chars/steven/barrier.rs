//! The barrier and the up special's platform: StevenShieldProjectile_C, the
//! Steven_ShieldActor classes (Steven_ShieldActor_C, _Up_C, _U_SpecialPlatform_C) as [`StevenShield`] (a PlatformActor:
//! a SpawnedPlatformParent child with a wall box), their place and their hooks.
//!
//! Offsets are the ubergraphs' of StevenShieldProjectile, Steven_ShieldActor, Steven_ShieldActor_Up and
//! Steven_ShieldActor_U_SpecialPlatform. No pawn hook: every variable of these classes lives on its actor.
//!
//! ---- StevenShieldProjectile_C (the side special's disc) ----
//! A ParentProjectilePhysics child: the world's Projectile is the parent (movement: gravity 0, bShouldBounce with
//! DestroyOnCollideWithEnvironment, so no bounce and a death at the first blocking hit; the hit; DestroyOnBlocked;
//! 'Ak Death Location' sfx_cha_steven_barrierthrow_impact, posted by the projectile's destroy).
//! GetSpawnMaxLifetime @0-252 (its own): GetGameTimeInSeconds - the Instigator's Comp_Pawn_ChargeAttack.ChargeStartTime,
//! x 1.0, + 0.1: the disc flies as long as the throw was held, plus 0.1 s.
//! Custom End Play @609 (the parent binds AActor::OnEndPlay to it; it runs inside K2_DestroyActor's EndPlay), with
//! authority: the owner's Comp_Spawner CompSpawnActor(Steven_ShieldActor_C) @1202 at the disc's location + (0, 0, 150)
//! @759-801, yaw 90 when ProjectileMovement.Velocity.Y > 0 else -90 @964-1096 (UE Y is -sim x: a disc flying to -x
//! leaves a barrier that faces -x). Not in the Lab reset's pass (the reset drops the pawns and their actors).
//!
//! ---- Steven_ShieldActor and Steven_ShieldActor_Up ----
//! SpawnedPlatformParent children (`world/spawned_platform.rs`: `plat` holds the parent's variables and boxes). Their
//! boxes are the class data's, centred on the actor (no RelativeLocation): a box at the actor's yaw +-90 (or pitched 90
//! on _Up) is upright in the play plane, its X extent along sim x (pitched: its Z extent along x, X up).
//! Steven_ShieldActor (the barrier the disc leaves): Collision (profile Platform: stood on, passed from below and the
//! sides by the parent's BPI_IgnorePlatformCollision), WallBox (Custom, no ObjectType: WorldDynamic; Pawn not listed:
//! Block; WallJump Block: a wall from every side for a capsule that does not ignore it), BlockBox
//! (ProjectileDestructible), Hurtbox (object type HurtBox: the fighters' melee hit boxes overlap it),
//! FighterDetectionSphere (radius 450, Pawn overlap); WallbounceCollision NoCollision.
//! Steven_ShieldActor_Up (the up special's shield, a child): every graph of the parent and BeginPlay's own tail
//! (`spawn_up`); Collision and WallbounceCollision pitched 90 (360 wide, 40 thick), UseWallbounceCollision with
//! WallbounceAllies / WallbounceEnemies; WallBox and the detection sphere NoCollision; BlockBox and Hurtbox pitched.
//! Both run SpawnedActorParent's BeginPlay blast-box loop and its CheckBlastBoxExit timer (DestroyOnExitedArena?, the
//! parent's default): a shield placed outside the arena is destroyed in its spawn.
//!
//! ---- Steven_ShieldActor_U_SpecialPlatform ----
//! A WW_ShieldActor child (`actors/shield_actor.rs`: its spawn on the socket, BlockCollision's block loop, the parent's
//! platform and wall calls, ActivateObject). Its own: ReceiveBeginPlay (the parent's, then IsBouncePerkActive from the
//! owner's buffs), Collision's begin overlap (a BP_FighterCharacter, with authority ->
//! Comp_ShieldBounce.AddFighterToBounceQueue: presentation), BPI_GetWallBounceMultiplier (1.3 with the perk for a
//! bounced actor of another team, else 1.0; no bounce count), and the class data's MaxLifetime 0.5 (the parent's
//! timer -> Delay(DestroyActor_Delay 0) -> DestroySpawnedActor; no death sound or VFX in the data).
//! Comp_ShieldBounce: the bounce queue moves the shield's mesh components: presentation.
//!
//! Deviation: the pool (CanBePooled on Steven_ShieldActor: DestroySpawnedActor -> ReleaseToPool, every primitive's
//! collision off and hidden) is taken as a destroy: the actor leaves the list. What the pool changes at the next
//! spawn is kept: the released barrier's stored blast bounds wait in the owner's pool (`StevenBarrierState::pool`),
//! and a spawn that takes one runs no BeginPlay loop (`disc_end`).
//! Deviation: the disc's Custom End Play runs in `ProjectileHooks::destroyed` (inside the projectile's destroy, ahead of
//! the parent's death effects, which the build plays in DestroySpawnedActor before its K2_DestroyActor): same frame,
//! events reordered.
//! Deviation: the disc's barrier spawn gets Comp_Spawner HandleActorPostSpawn's AfterSpawnedActor @12116 as far as
//! Buff_Enlarge's binding (@2366 -> @2032); the buffs' other AfterSpawnedActor bindings (`BuffSignal::AfterSpawnedActor`,
//! as the notify spawn makes) are not called for it.
//! Unverified: the disc's clock is read at the spawn (make), as the parent's BeginPlay reads it; both are the spawn
//! frame.
//! Unverified: whether the build's HandleImpact still broadcasts OnProjectileBounce for the disc's first blocking hit
//! (the disc has no bounce, so the bounce sound is not reached).
//! Unverified: the loop is posted on the actor's own AkComponent and stops with the actor; the stop with the component
//! is the Wwise integration's, not read: the UI stops the loop's key.
//! Unverified: the up shield's yaw is taken as the owner's facing (Comp_Spawner's rotation for an unattached actor is
//! not read); its pitched boxes lie the same either side, only the hit VFX side would read it (_Up's is 0).
//! Unverified: the actors ahead of the projectiles in BlockBox's overlap pass, and the order of the overlap events
//! within a frame, are not read.
//! Unverified: IsBouncePerkActive of U_SpecialPlatform is read from the owner's buffs at the call (ShieldActor holds no
//! field of this class's); the perk's marker is a permanent buff, so the answer is the BeginPlay's.
//! Deviation: the detection sphere's events are taken from what changed since the barrier's last tick (in its tick,
//! behind the fighters' moves): walking = Fighter.grounded, a landing on solid ground = a base without canPassThrough
//! (the rule `landed_on_solid_ground` uses for OnLandedOnSolidGround), a new Buff_Tunneling = one not there at the last
//! tick; the fighters' IsOnSolidGround before the barrier's spawn is taken from where they stand then. The overlap is
//! the sphere against the root capsule in the play plane; only the fighters are tested (the other pawns fail the
//! casts).
//! Deviation: BlockBox's overlap is the projectile's hit circle (its hit box on) against the box in the play plane,
//! begun when it was not inside at the last tick, as `actors::shield_actor` takes WW_ShieldActor's. The BlockingActor
//! is the shield itself: a consumer that keeps it matches no fighter, so the shield's owner can be hit.
//! Deviation: the bounce getter is called from a fighter's move: the destroy and its death sound come at the actor's
//! next tick (`pending_death`); the build destroys it inside the move.
//! Deviation: the platform boxes are re-placed with no move of their own (dx / dy 0: a fighter standing on Collision
//! is not carried up by a size raise, where the build's based movement may follow the teleported base); a fighter
//! inside a box that grew is not pushed out here.

use crate::Real;
use crate::State;
use crate::actors::projectile::MakeOpts;
use crate::actors::world_actor::{WorldActor, WorldActorState};
use crate::actors::{ActorClass, ActorId, ActorRef, ProjectileClass, ProjectileId, WorldActorClass};
use crate::combat::boxes::{WorldSphere, shape_over_box};
use crate::combat::buffs::buff_of;
use crate::combat::elem::{instigator_charge, projectile_like};
use crate::events::{Event, SoundKey};
use crate::fighter::common::{actor_def, latent_frames, owner_team, peers, warn_text};
use crate::fighter::match_side::SpawnRequest;
use crate::fighter::traces::{TraceBody, body_info};
use crate::fighter::{FighterId, cap_half, cap_radius, grounded, in_hitstun};
use crate::math::max;
use crate::name;
use crate::names::Name;
use crate::registry::entries::{ActorEntry, ActorHooks, DamageBox, ProjectileHooks, SizeHooks, WorldActorHooks};
use crate::registry::props::{flag, num, num_in, obj, text};
use crate::registry::{Registry, RegistryError};
use crate::world::World;
use crate::world::blast::{BlastBounds, blast_box_loop, blast_timer_exit, new_blast_bounds, spawn_box};
use crate::world::flow::reset_pass;
use crate::world::spawned_platform::{PlatformPart, SpawnedPlatform, platform_end, platform_step};
use crate::world::spawning::{add_actor, after_spawned_actor, new_world_actor};
use mvs_data::value::Props;

const BARRIER: Name = name!("Steven_ShieldActor_C");
const UP: Name = name!("Steven_ShieldActor_Up_C");
const PLATFORM: Name = name!("Steven_ShieldActor_U_SpecialPlatform_C");
const DISC: Name = name!("StevenShieldProjectile_C");
/// The ShieldBounce perk's marker (Perks/Buffs/Steven/Buff_Steven_ShieldBounce: no graph; read by BPI_CheckIfHasBuff
/// alone), given by Perk_Steven_ShieldBounce's AppliedBuffs.
const BOUNCE_PERK: Name = name!("Buff_Steven_ShieldBounce_C");
const TUNNELING: Name = name!("Buff_Tunneling_C");

/// `PlaneBox`: a box's half extents in the play plane (x, y) and in depth.
#[derive(State, Copy, Debug, Default)]
pub struct PlaneBox {
    pub hx: Real,
    pub hy: Real,
    pub hz: Real,
}

/// `hurtBox` / `blockBox`: the cache `damageBox` hands out; written when the box is asked for (zeros until then).
#[derive(State, Copy, Debug, Default)]
pub struct BoxCache {
    pub x: Real,
    pub y: Real,
    pub hx: Real,
    pub hy: Real,
    pub hz: Real,
}

/// `StevenShield` (the Steven_ShieldActor classes). Its `plat` is the actor's (`WorldActor::plat`); the WallBox is a
/// part of it at `wall_box` that `plat`'s own two parts do not count.
#[derive(State, Debug, Default)]
pub struct StevenShield {
    pub x: Real,
    pub y: Real,
    /// The actor's yaw side (+1 faces +x: yaw -90).
    pub facing: Real,
    pub age: i32,
    /// The age DestroySpawnedActor runs at after MaxLifetime.
    pub end_age: Real,
    /// CurrentHealth.
    pub health: Real,
    /// BounceCounter.
    pub bounce_counter: i32,
    /// MaxBounces met in a fighter's move: destroyed at the next tick.
    pub pending_death: bool,
    /// IsBouncePerkActive.
    pub bounce_perk: bool,
    /// FighterDetectionSphere on (Steven_ShieldActor; NoCollision on _Up).
    pub detect: bool,
    pub block: Option<PlaneBox>,
    pub hurt: Option<PlaneBox>,
    pub hurt_box: BoxCache,
    pub block_box: BoxCache,
    /// The projectiles and actors (their numbers) overlapping BlockBox.
    pub inside: Vec<i32>,
    /// The fighters (ids) in FighterDetectionSphere (their bindings).
    pub near: Vec<i32>,
    /// The fighters walking at the last tick (the port's stand-in for their events).
    pub grounded: Vec<i32>,
    /// The fighters with IsOnSolidGround, as the port tracks it (see `detect_tick`).
    pub solid: Vec<i32>,
    /// The fighters with Buff_Tunneling at the last tick.
    pub tunneling: Vec<i32>,
    /// The latest OnHitVisuals (presentation): the tick of its frame and whether it was a fighter's hit. A later hit
    /// overwrites both; never cleared.
    pub hit_tick: Option<i32>,
    pub hit_heavy: Option<bool>,
    /// The WallBox's place in `plat.parts`.
    pub wall_box: Option<u8>,
    /// SpawnedActorParent's CheckBlastBoxTimer: its next expiry.
    pub blast_at: crate::Clock,
    /// BlastBoxBoundsMin / Max, stored by the BeginPlay loop ([`blast_loop`]).
    pub blast_bounds: BlastBounds,
}

/// The StevenShield's place (its `x`, `y`).
pub fn place(_w: &World, _a: &WorldActor, s: &StevenShield) -> Option<(Real, Real)> {
    Some((s.x, s.y))
}

fn st(w: &World, id: ActorId) -> Option<&StevenShield> {
    match &w.actors.actors.get(id)?.state {
        WorldActorState::StevenShield(s) => Some(s),
        _ => None,
    }
}

fn st_mut(w: &mut World, id: ActorId) -> Option<&mut StevenShield> {
    match &mut w.actors.actors.get_mut(id)?.state {
        WorldActorState::StevenShield(s) => Some(s),
        _ => None,
    }
}

fn sound(w: &mut World, fighter: i32, ev: Name, x: Real, y: Real) {
    w.events.push(Event::Sound { fighter, ev, x: Some(x), y: Some(y), hit: None, pooled: false, key: None });
}

/// `lifeEnd`: SpawnedActorParent's MaxLifetime timer -> OnMaxLifetimeExpired -> Delay(DestroyActor_Delay, 0 here) ->
/// DestroySpawnedActor (@23967 -> 3161).
fn life_end(p: &Props) -> Real {
    crate::combat::hit::timer_ticks(num(p, "MaxLifetime"))
        + 1.0
        + latent_frames(num(p, "DestroyActor_Delay").unwrap_or(0.0))
}

// ---------------------------------------------------------------- StevenShieldProjectile

fn disc_loop(w: &World, id: ProjectileId) -> (i32, SoundKey) {
    let p = w.actors.projectiles.get(id).expect("a live projectile");
    (p.owner_id, SoundKey { tag: name!("stevendisc"), n: p.seq })
}

/// GetSpawnMaxLifetime @0-252: the options the projectile's constructor takes.
fn disc_opts(w: &World, f: FighterId, _r: &SpawnRequest) -> MakeOpts {
    let fm = w.fighter(f);
    MakeOpts { life: Some((fm.clock.time - fm.charge.charge_start_time) + 0.1), ..MakeOpts::default() }
}

/// ReceiveBeginPlay: the parent's, then OnProjectileBounce bound (ProjectileBounce), LoopingSFX
/// (PF_Post_Event(sfx_cha_steven_barrierthrow_loop) on itself @1333).
fn disc_begin(w: &mut World, id: ProjectileId) {
    let (fighter, key) = disc_loop(w, id);
    w.events.push(Event::SoundStart { fighter, ev: name!("sfx_cha_steven_barrierthrow_loop"), key });
}

/// ProjectileBounce @1399: PF_Post_Event(sfx_cha_steven_barrierthrow_bounce) on itself.
fn disc_bounce(w: &mut World, id: ProjectileId, _nx: Real, _ny: Real, _ivx: Real, _ivy: Real) -> bool {
    let p = w.actors.projectiles.get(id).expect("a live projectile");
    let (fighter, x, y) = (p.owner_id, p.x, p.y);
    sound(w, fighter, name!("sfx_cha_steven_barrierthrow_bounce"), x, y);
    false
}

/// The Steven_ShieldActor_C the disc leaves (Custom End Play @609): `facing` by the disc's velocity.
fn disc_end(w: &mut World, id: ProjectileId) {
    if reset_pass(w) {
        return;
    }
    let Some(p) = w.actors.projectiles.get(id) else { return };
    let Some(owner) = p.owner else { return };
    let (x, y, facing) = (p.x, p.y + 150.0, if p.vx < 0.0 { -1.0 } else { 1.0 });
    if actor_def(w, ActorRef::Fighter(owner), BARRIER).is_none() {
        warn_text(
            w,
            &format!("{}: {} is not in the owner's data: no barrier", w.names.str(DISC), w.names.str(BARRIER)),
        );
        return;
    }
    let mut s = new_shield(w, owner, BARRIER, x, y, facing);
    // Comp_Spawner CompSpawnActor @2774-3659: a barrier waiting in the owner's SpawnPool is reused by InitFromPool
    // (SpawnedActorParent @23673 -> @31-257: the transform, the attack id, CheckBlastBoxTimer restarted), which runs no
    // ReceiveBeginPlay and so no blast-box loop: it keeps the bounds it stored and only the exit timer tests it. A
    // fresh one (the pool empty) runs the loop.
    // Unverified: the rest of a reused barrier is built fresh (health, bounce counter, perk flag, the fighters'
    // grounded lists), where the build's skips its ReceiveBeginPlay @584 and keeps what the release left; and every
    // end of a side barrier is taken as a release (a K2_DestroyActor route would not pool it).
    let pooled = super::steven_of(w, owner).and_then(|st| st.barrier.pool.last().copied());
    let dead = match pooled {
        Some(b) => {
            super::steven_w(w, owner).barrier.pool.pop();
            s.blast_bounds = b;
            false
        }
        None => blast_loop(w, owner, BARRIER, &mut s),
    };
    let a =
        new_world_actor(w, BARRIER, WorldActorClass::StevenShieldActor, owner, dead, WorldActorState::StevenShield(s));
    let aid = w.actors.actors.insert(a);
    if !dead {
        spawned(w, aid);
    }
    add_actor(w, aid, instigator_charge(w, owner), true);
    // HandleActorPostSpawn's AfterSpawnedActor @12116: Buff_Enlarge's binding (@2366 -> @2032), the buffs' signal.
    after_spawned_actor(w, owner, ActorRef::Actor(aid));
}

/// The loop was posted on the actor's own AkComponent: it stops with the actor.
fn disc_stop(w: &mut World, id: ProjectileId) {
    let (fighter, key) = disc_loop(w, id);
    w.events.push(Event::SoundStop { fighter, key, fade: None, ev: None, curve: None, cut: false });
}

// ---------------------------------------------------------------- Steven_ShieldActor and Steven_ShieldActor_Up

/// A component template whose CollisionEnabled is not NoCollision (`enabled`).
fn enabled(c: Option<&Props>) -> bool {
    c.is_some_and(|c| {
        !c.get("BodyInstance")
            .and_then(mvs_data::value::Value::as_obj)
            .and_then(|b| b.get("CollisionEnabled"))
            .and_then(mvs_data::value::Value::as_str)
            .unwrap_or("QueryAndPhysics")
            .ends_with("NoCollision")
    })
}

/// `planeBox`: the template's box in the play plane; `None` (warned) for one with a place, scale or turn the port does
/// not lay there, or without an extent.
fn plane_box(w: &World, cls: Name, comp: Option<&Props>) -> Option<PlaneBox> {
    let c = comp?;
    let e = obj(c, "BoxExtent")?;
    let rr = obj(c, "RelativeRotation");
    let (yaw, roll, pitch) = (
        rr.and_then(|r| num(r, "Yaw")).unwrap_or(0.0),
        rr.and_then(|r| num(r, "Roll")).unwrap_or(0.0),
        rr.and_then(|r| num(r, "Pitch")).unwrap_or(0.0),
    );
    if c.get("RelativeLocation").is_some()
        || c.get("RelativeScale3D").is_some()
        || yaw != 0.0
        || roll != 0.0
        || (pitch != 0.0 && pitch.abs() != 90.0)
    {
        warn_text(
            w,
            &format!("{}: a box with a place, scale or turn the port does not lay in the play plane", w.names.str(cls)),
        );
        return None;
    }
    let up = pitch.abs() == 90.0;
    let g = |k: &str| num(e, k).unwrap_or(0.0);
    Some(PlaneBox { hx: if up { g("Z") } else { g("X") }, hy: if up { g("X") } else { g("Z") }, hz: g("Y") })
}

/// The `StevenShield` constructor's fields (its SpawnedPlatform is made by [`spawned`] once the World holds the
/// actor): facing is the actor's yaw side (+1 faces +x). Placed at (x, y), still (no movement).
fn new_shield(w: &World, owner: FighterId, cls: Name, x: Real, y: Real, facing: Real) -> StevenShield {
    let mut s = StevenShield {
        x,
        y,
        facing,
        blast_at: f64::from(0.1_f32),
        blast_bounds: new_blast_bounds(),
        ..StevenShield::default()
    };
    if let Some(d) = actor_def(w, ActorRef::Fighter(owner), cls) {
        let comp = |k: &str| obj(&d.components, k);
        s.block = if enabled(comp("BlockBox")) { plane_box(w, cls, comp("BlockBox")) } else { None };
        s.hurt = if enabled(comp("Hurtbox")) { plane_box(w, cls, comp("Hurtbox")) } else { None };
        s.detect = enabled(comp("FighterDetectionSphere"))
            && comp("FighterDetectionSphere").and_then(|c| num(c, "SphereRadius")).is_some_and(|r| r != 0.0);
        // SpawnedActorParent's MaxLifetime timer -> OnMaxLifetimeExpired -> Delay(DestroyActor_Delay, 0 here) ->
        // DestroySpawnedActor (@23967 -> 3161).
        s.end_age = life_end(&d.p);
        // ReceiveBeginPlay @584: the parent's, CurrentHealth = MaxHealth @594, BounceCounter = 0, IsBouncePerkActive
        // = the owner's BPI_CheckIfHasBuff(Buff_Steven_ShieldBounce) (the enter animation: presentation).
        s.health = num(&d.p, "MaxHealth").unwrap_or(0.0);
    }
    s.bounce_perk = buff_of(w, owner, BOUNCE_PERK).is_some();
    // The fighters' IsOnSolidGround at the spawn (see `detect_tick`).
    for i in 0..peers(w).len() {
        let f = peers(w)[i];
        let id = w.fighter(f).id;
        if grounded(w, f) {
            s.grounded.push(id);
            if !base_passes_through(w, f) {
                s.solid.push(id);
            }
        }
    }
    s
}

/// SpawnedActorParent's BeginPlay blast-box loop (`world::blast::blast_box_loop`) on a new shield at its place, run
/// by the parent's BeginPlay ahead of the class's own part (Steven_ShieldActor @584, _Up @612 call it first): the
/// bounds stored; outside the jump-out area with DestroyOnExitedArena? (the SpawnedActorParent default, kept by both
/// classes) DestroySpawnedActor(true): true, the shield is destroyed in its spawn. Neither class has
/// ForceSpawnPositionInsideBlastBox.
fn blast_loop(w: &World, owner: FighterId, cls: Name, s: &mut StevenShield) -> bool {
    let p = actor_def(w, ActorRef::Fighter(owner), cls).map(|d| &d.p);
    let force = p.is_some_and(|p| flag(p, "ForceSpawnPositionInsideBlastBox"));
    let doe = p.is_some_and(|p| flag(p, "DestroyOnExitedArena?"));
    blast_box_loop(&spawn_box(w), &mut s.x, &mut s.y, None, force, &mut s.blast_bounds, doe)
}

/// `f.base?.canPassThrough === undefined`'s opposite: the fighter's floor is a SpawnedPlatformParent's box that answers
/// canPassThrough.
fn base_passes_through(w: &World, f: FighterId) -> bool {
    match w.fighter(f).platforms.base {
        Some(crate::fighter::body::PlatformRef::Spawned(a, i)) => {
            crate::world::spawned_platform::spawned_has_pass_through(w, a, i)
        }
        _ => false,
    }
}

/// The rest of the `StevenShield` constructor once the World holds the actor (the kind's `spawned`): its
/// SpawnedPlatform with the half extents of Collision and WallbounceCollision, the WallBox as a part of its own (its
/// own template, its ignore list the actor's FightersIgnoringCollision, no share in the parent's
/// BPI_IgnorePlatformCollision, no DelayCollisionEnabled), the boxes placed at the actor.
fn spawned(w: &mut World, id: ActorId) {
    let Some(a) = w.actors.actors.get(id) else { return };
    let (owner, cls) = (a.owner, a.cls);
    let (x, y) = st(w, id).map_or((0.0, 0.0), |s| (s.x, s.y));
    let Some(d) = actor_def(w, ActorRef::Fighter(owner), cls) else { return };
    let comp = |k: &str| obj(&d.components, k);
    let col = if enabled(comp("Collision")) { plane_box(w, cls, comp("Collision")) } else { None };
    let wb = if flag(&d.p, "UseWallbounceCollision") && enabled(comp("WallbounceCollision")) {
        plane_box(w, cls, comp("WallbounceCollision"))
    } else {
        None
    };
    let wall = if enabled(comp("WallBox")) { plane_box(w, cls, comp("WallBox")) } else { None };
    let wall_part = wall.zip(comp("WallBox")).map(|(b, c)| {
        let mut p = PlatformPart::new(name!("WallBox"), false, b.hx, b.hy, c);
        p.takes_ignore = false;
        // no DelayCollisionEnabled reaches it
        p.on = true;
        p.place(x, y);
        p
    });
    let mut pl = SpawnedPlatform::new(w, owner, cls, col.map(|b| (b.hx, b.hy)), wb.map(|b| (b.hx, b.hy)));
    if let Some(c) = pl.collision_part() {
        c.place(x, y);
    }
    if let Some(c) = pl.wallbounce_part() {
        c.place(x, y);
    }
    let wall_box = wall_part.map(|p| pl.push(p));
    if let Some(a) = w.actors.actors.get_mut(id) {
        a.plat = Some(pl);
    }
    if let Some(s) = st_mut(w, id) {
        s.wall_box = wall_box;
    }
}

/// DestroySpawnedActor(false) of the actor's own graphs (health, bounces, lifetime): the release; the death effects
/// are its EndPlay's (`shield_end`).
fn destroy_shield(w: &mut World, id: ActorId) {
    if let Some(a) = w.actors.actors.get_mut(id) {
        a.dead = true;
    }
}

/// ReceiveEndPlay, however the actor ended (its own destroys, _Up's dedup, Die With Instigator, the Lab reset): the
/// parent's (SpawnedPlatformParent @483 -> SpawnedActorParent @15507; the pool's release @23788 the same): the 'Ak
/// Death Location' sound (sfx_cha_steven_barrierthrow_end) and DeathVFX (none in the class data) at the actor unless
/// DestroyVFXBlocked (written only by BPI_SetDestroyEffectsBlocked, which no caller makes on these classes: false);
/// then the game state's platform lists. The outro (PS_StevenShield_Outro) is presentation. The parent's EndPlay
/// begins with DestroyChildrenWithIgnore @15512 (its buff children: the World runs `end_actor_children` ahead of every
/// World.actors end).
fn shield_end(w: &mut World, id: ActorId) {
    let Some(s) = st(w, id) else { return };
    let (x, y, wall, bounds) = (s.x, s.y, s.wall_box, s.blast_bounds);
    let Some(a) = w.actors.actors.get(id) else { return };
    let (fighter, cls, owner) = (a.owner_id, a.cls, a.owner);
    // ReleaseToPool @22789 -> the owner's Comp_Spawner AddActorToPool (AddUnique on the class's array).
    if cls == BARRIER && super::steven_of(w, owner).is_some() {
        super::steven_w(w, owner).barrier.pool.push(bounds);
    }
    let p = crate::actors::world_actor::actor_p(w, ActorRef::Actor(id));
    let ev = text(p, "Ak Death Location").filter(|e| !e.is_empty()).map(str::to_owned);
    let vfx = flag(p, "DeathVFX");
    if let Some(ev) = ev {
        let ev = w.lit(&ev);
        sound(w, fighter, ev, x, y);
    }
    if vfx {
        warn_text(w, &format!("{}: DeathVFX not posted", w.names.str(cls)));
    }
    platform_end(w, id);
    if let (Some(i), Some(pl)) = (wall, crate::world::spawned_platform::plat_mut(w, id)) {
        pl.parts[usize::from(i)].dead = true;
    }
}

/// OnHitVisuals(FighterHit) (presentation): PS_HitImpact_TEMPLATE (scale 1) for a fighter's hit, else
/// PS_HitImpact_Directional (scale 2), at the actor + GetVFXPosition. Steven_ShieldActor's: (0, 0, 150) for a fighter's
/// hit, else (0, +-150, 150) by the mesh's yaw (a barrier facing +x puts it at sim x + 150); _Up's answers (0, 0, 0).
fn hit_visuals(w: &mut World, id: ActorId, fighter_hit: bool) {
    let tick = w.flow.tick;
    let Some(a) = w.actors.actors.get(id) else { return };
    let (cls, fighter) = (a.cls, a.owner_id);
    let Some(s) = st_mut(w, id) else { return };
    s.hit_tick = Some(tick);
    s.hit_heavy = Some(fighter_hit);
    let (x, y, facing) = (s.x, s.y, s.facing);
    let up = cls == UP;
    let ox = if up || fighter_hit { 0.0 } else { 150.0 * facing };
    let oy = if up { 0.0 } else { 150.0 };
    w.events.push(Event::ProjectileDeath {
        cls,
        fighter,
        vfx: Some(if fighter_hit { name!("PS_HitImpact_TEMPLATE") } else { name!("PS_HitImpact_Directional") }),
        x: x + ox,
        y: y + oy,
        ox: 0.0,
        rot: [0.0, 0.0, 0.0],
        scale: if fighter_hit { 1.0 } else { 2.0 },
        tint: None,
        to: None,
    });
}

/// ShouldFighterIgnoreCollision: hasBuff(Buff_Tunneling) || (IsOnSolidGround && of the barrier's team).
/// CheckForCollisionChange: Deactivate (WallBox and Collision ignored by the capsule) or Reactivate.
/// The write to Collision is the parent's BPI_IgnorePlatformCollision's again at the fighter's next pawn tick
/// (`handle_platform_collision` runs ahead of every move): only WallBox's lasts, which is what the port writes.
fn check_collision(w: &mut World, id: ActorId, f: FighterId, solid: bool) {
    let tun = buff_of(w, f, TUNNELING).is_some();
    let team = w.fighter(f).team.team;
    set_wall_ignored(w, id, f, |t| tun || (solid && team == t));
}

/// The WallBox ignores the fighter (`setIgnored`), by the platform's team.
fn set_wall_ignored(w: &mut World, id: ActorId, f: FighterId, v: impl FnOnce(i32) -> bool) {
    let fid = w.fighter(f).id;
    let Some(wall) = st(w, id).and_then(|s| s.wall_box) else { return };
    let Some(pl) = crate::world::spawned_platform::plat_mut(w, id) else { return };
    let ignore = v(pl.team);
    pl.parts[usize::from(wall)].set_ignored(fid, ignore);
}

fn has(l: &[i32], id: i32) -> bool {
    l.contains(&id)
}

fn put(l: &mut Vec<i32>, id: i32, v: bool) {
    let i = l.iter().position(|&x| x == id);
    match (v, i) {
        (true, None) => l.push(id),
        (false, Some(i)) => {
            l.remove(i);
        }
        _ => {}
    }
}

/// FighterDetectionSphere (radius 450, Pawn overlap): its begin overlap runs CheckForCollisionChange(the fighter) and
/// binds the fighter's OnLandedOnSolidGround, OnMovementModeChange and Comp_State_Manager.NewBuffSpawned (a
/// Buff_Tunneling only); each runs CheckForCollisionChange. Its end overlap: ReactivateCollisionForFighter and the
/// unbinds. BaseFighter's order: OnLanded broadcasts OnLandedOnSolidGround @113707 ahead of IsOnSolidGround = true
/// @113727, and the CMC's Landed comes ahead of the switch to Walking; leaving Walking broadcasts OnMovementModeChange
/// @127894 ahead of IsOnSolidGround = false @128043. So a landing on solid ground ends with the check on true, one
/// elsewhere on false; a take-off checks the value it had on the ground: a team mate who jumps keeps passing until
/// his next landing.
fn detect_tick(w: &mut World, id: ActorId, r: Real) {
    let Some(s) = st(w, id) else { return };
    let (ax, ay) = (s.x, s.y);
    for i in 0..peers(w).len() {
        let f = peers(w)[i];
        let fid = w.fighter(f).id;
        let g = grounded(w, f);
        let Some(s) = st(w, id) else { return };
        let was_g = has(&s.grounded, fid);
        let mut solid = has(&s.solid, fid);
        let was_in = has(&s.near, fid);
        let was_tun = has(&s.tunneling, fid);
        let mut event = false;
        if g && !was_g {
            if !base_passes_through(w, f) {
                solid = true; // OnLanded @113727
            }
            event = true;
        } else if !g && was_g {
            event = true; // OnMovementModeChange, then IsOnSolidGround = false
        }
        let tun = buff_of(w, f, TUNNELING).is_some();
        let (fx, fy) = (w.fighter(f).body.x, w.fighter(f).body.y);
        let hh = max(cap_half(w, f) - cap_radius(w, f), 0.0);
        let dy = max((ay - fy).abs() - hh, 0.0);
        let dx = ax - fx;
        let rr = r + cap_radius(w, f);
        let in_now = w.fighter(f).life.dead == 0.0 && dx * dx + dy * dy <= rr * rr;
        if in_now && !was_in {
            check_collision(w, id, f, solid);
        } else if !in_now && was_in {
            set_wall_ignored(w, id, f, |_| false);
        } else if in_now && (event || (tun && !was_tun)) {
            check_collision(w, id, f, solid);
        }
        if !g && was_g {
            solid = false;
        }
        let Some(s) = st_mut(w, id) else { return };
        put(&mut s.near, fid, in_now);
        put(&mut s.grounded, fid, g);
        put(&mut s.solid, fid, solid);
        put(&mut s.tunneling, fid, tun);
    }
}

/// BlockBox's begin overlap @1617, with authority: the other actor's Comp_Actor_Destructible of another team and
/// (IsProjectile || BPI_Projectiles) @1651-1920 -> BPI_Blocked(self) on a BPI_SpawnedActor @2013, then
/// OnHitVisuals(false) @2051. No DestroyOnBlocked, heavy or meter test of its own.
/// An actor of World.actors is met by its HitBox-type components that overlap ProjectileDestructible
/// (`hit_shapes`: a dealer's hit box; a sphere against the box in the play plane, a box against it as two boxes),
/// with the graph's IsProjectile-or-BPI_Projectiles test and the team test; its Blocked is the kind's. The numbers
/// share the world's count, so one list holds both.
fn block_tick(w: &mut World, id: ActorId) {
    let Some(s) = st(w, id) else { return };
    let Some(b) = s.block else { return };
    // the ones gone
    let live: Vec<i32> = s.inside.iter().copied().filter(|&n| overlap_alive(w, n)).collect();
    if let Some(s) = st_mut(w, id) {
        s.inside = live;
    }
    block_actors(w, id, b);
    let mut i = 0;
    while let Some(pid) = w.actors.projectiles.nth(i) {
        i += 1;
        let Some(p) = w.actors.projectiles.get(pid) else { continue };
        if p.dead || w.actors.actors.get(id).is_none_or(|a| a.dead) {
            continue;
        }
        let seq = p.seq;
        let Some(s) = st(w, id) else { return };
        let at = s.inside.iter().position(|&n| n == seq);
        let mut over = false;
        if crate::actors::projectile::hit_collision(p) {
            let (hx, hy) = crate::actors::projectile::hit_xy(w, p);
            let dx = max((hx - s.x).abs() - b.hx, 0.0);
            let dy = max((hy - s.y).abs() - b.hy, 0.0);
            over = dx * dx + dy * dy <= p.hit_radius * p.hit_radius;
        }
        if !over {
            if let (Some(at), Some(s)) = (at, st_mut(w, id)) {
                s.inside.remove(at);
            }
            continue;
        }
        if at.is_some() {
            continue;
        }
        if let Some(s) = st_mut(w, id) {
            s.inside.push(seq);
        }
        let team = crate::world::spawned_platform::plat_of(w, id).map_or(-1, |p| p.team);
        if owner_team(w, ActorRef::Projectile(pid)) == team {
            continue;
        }
        crate::combat::hits::blocked(w, ActorRef::Projectile(pid), ActorRef::Actor(id));
        hit_visuals(w, id, false);
    }
}

/// `actorAlive` or a live projectile of that number: a number the overlap list keeps.
fn overlap_alive(w: &World, n: i32) -> bool {
    if w.actors.projectiles.iter().any(|(_, p)| p.seq == n && !p.dead) {
        return true;
    }
    w.actors.actors.iter().any(|(_, x)| {
        x.seq == n
            && !x.dead
            && crate::combat::spawned_hits::world_hooks(w, x.cls).is_some_and(|k| k.hit_shapes.is_some())
    })
}

fn block_actors(w: &mut World, id: ActorId, b: PlaneBox) {
    let _ = b;
    let mut i = 0;
    while let Some(oid) = w.actors.actors.nth(i) {
        i += 1;
        if w.actors.actors.get(id).is_none_or(|a| a.dead) {
            return;
        }
        let Some(o) = w.actors.actors.get(oid) else { continue };
        if o.dead || oid == id {
            continue;
        }
        let (oseq, ocls, oowner) = (o.seq, o.cls, o.owner);
        let Some(k) = crate::combat::spawned_hits::world_hooks(w, ocls).and_then(|k| k.hit_shapes) else { continue };
        let mut shapes: Vec<WorldSphere> = Vec::new();
        let have = k(w, oid, &mut shapes);
        let Some(s) = st(w, id) else { return };
        let at = s.inside.iter().position(|&n| n == oseq);
        let mut over = false;
        let bx = if have && !shapes.is_empty() { box_at(w, id, true) } else { None };
        if let Some(bx) = bx {
            over = shapes.iter().any(|s| shape_over_box(s, &bx));
        }
        if !over {
            if let (Some(at), Some(s)) = (at, st_mut(w, id)) {
                s.inside.remove(at);
            }
            continue;
        }
        if at.is_some() {
            continue;
        }
        if let Some(s) = st_mut(w, id) {
            s.inside.push(oseq);
        }
        let team = crate::world::spawned_platform::plat_of(w, id).map_or(-1, |p| p.team);
        if w.fighter(oowner).team.team == team || !projectile_like(w, ActorRef::Actor(oid)) {
            continue;
        }
        crate::combat::hits::blocked(w, ActorRef::Actor(oid), ActorRef::Actor(id));
        hit_visuals(w, id, false);
    }
}

/// `ActorKind.hitBoxOn`: a projectile's hit box that lives inside one call (C016_SassBubble's Box, Velma_SassBubblev2's:
/// `chars/lebron/bubble.rs`) against BlockBox, as `block_tick` meets a hit circle: two boxes in the play plane, the
/// pair listed in `inside` (the step's next pass ends it: that projectile's hit circle is off), the team test, then
/// BPI_Blocked and OnHitVisuals(false). The bubble is IsProjectile.
fn shield_box_on(w: &mut World, id: ActorId, pr: ProjectileId, _comp: Name, s: &WorldSphere) {
    let Some(p) = w.actors.projectiles.get(pr).filter(|p| !p.dead) else { return };
    let seq = p.seq;
    if st(w, id).is_none_or(|t| t.inside.contains(&seq)) {
        return;
    }
    let Some(bx) = box_at(w, id, true) else { return };
    if !shape_over_box(s, &bx) {
        return;
    }
    if let Some(t) = st_mut(w, id) {
        t.inside.push(seq);
    }
    let team = crate::world::spawned_platform::plat_of(w, id).map_or(-1, |p| p.team);
    if owner_team(w, ActorRef::Projectile(pr)) == team {
        return;
    }
    crate::combat::hits::blocked(w, ActorRef::Projectile(pr), ActorRef::Actor(id));
    hit_visuals(w, id, false);
}

/// `shieldTick`.
fn shield_tick(w: &mut World, id: ActorId) {
    let Some(s) = st_mut(w, id) else { return };
    s.age += 1;
    platform_step(w, id);
    let Some(s) = st(w, id) else { return };
    if s.pending_death || crate::real(s.age) >= s.end_age {
        destroy_shield(w, id);
        return;
    }
    let Some(a) = w.actors.actors.get(id) else { return };
    let (owner, cls, scale) = (a.owner, a.cls, a.size_scale.unwrap_or(1.0));
    // SpawnedActorParent's CheckBlastBoxExit on its 0.1 s looping timer (DestroyOnExitedArena?), the spawn frame not
    // counted, as `actors::shield_actor`'s: only a size raise (`shield_move_z`) moves the shield.
    let doe = actor_def(w, ActorRef::Fighter(owner), cls).is_some_and(|d| flag(&d.p, "DestroyOnExitedArena?"));
    let Some(s) = st_mut(w, id) else { return };
    let (frames, detect) = (crate::real(s.age - 1), s.detect);
    if blast_timer_exit(doe, &mut s.blast_at, &s.blast_bounds, frames, s.x, s.y) {
        destroy_shield(w, id);
        return;
    }
    let sphere = actor_def(w, ActorRef::Fighter(owner), cls)
        .and_then(|d| num_in(&d.components, "FighterDetectionSphere", "SphereRadius"))
        .unwrap_or(0.0);
    let r = sphere * scale;
    if detect && r > 0.0 {
        detect_tick(w, id, r);
    }
    block_tick(w, id);
}

/// ReceiveAnyDamage @626: only a DamageCauser that is a BP_FighterCharacter; OnHitVisuals(true) @705, CurrentHealth -=
/// Damage @720-766 (no clamp), at <= 0 DestroySpawnedActor(false) @841. Its Comp_Actor_Destructible has
/// IsInvulnerableToProjectiles: the fighters' melee boxes meet its Hurtbox (`damage_box`).
fn shield_damage(w: &mut World, id: ActorId, dmg: Real, causer: ActorRef) {
    if w.actors.actors.get(id).is_none_or(|a| a.dead) || !matches!(causer, ActorRef::Fighter(_)) {
        return;
    }
    hit_visuals(w, id, true);
    let Some(s) = st_mut(w, id) else { return };
    s.health -= dmg;
    if s.health <= 0.0 {
        destroy_shield(w, id);
    }
}

/// The box in the play plane (an axis-aligned box: the actor's yaw is +-90, _Up's boxes pitched 90), written into the
/// cache behind `hurtBox` / `blockBox`. Both Hurtbox (HurtBox) and BlockBox (ProjectileDestructible)
/// overlap the HitBox channel: Comp_Pawn_HitBoxHandler @6187-6648 runs pfgDoDamage per overlapping component with no
/// victim-component filter, the hit-once record per actor, so a melee hit over either is one hit.
fn box_at(w: &mut World, id: ActorId, block: bool) -> Option<WorldSphere> {
    if w.actors.actors.get(id).is_none_or(|a| a.dead) {
        return None;
    }
    let s = st_mut(w, id)?;
    let h = if block { s.block } else { s.hurt }?;
    let (x, y) = (s.x, s.y);
    let c = if block { &mut s.block_box } else { &mut s.hurt_box };
    *c = BoxCache { x, y, hx: h.hx, hy: h.hy, hz: h.hz };
    Some(WorldSphere {
        x,
        y,
        z: Some(0.0),
        r: 0.0,
        bx: Some([h.hx, 0.0, 0.0, 0.0, h.hy, 0.0, 0.0, 0.0, h.hz]),
        ..WorldSphere::default()
    })
}

fn hurt_at(w: &mut World, id: ActorId) -> Option<WorldSphere> {
    box_at(w, id, false)
}

fn block_at(w: &mut World, id: ActorId) -> Option<WorldSphere> {
    box_at(w, id, true)
}

const DAMAGE_BOXES: &[DamageBox] =
    &[DamageBox { comp: "Hurtbox", at: hurt_at }, DamageBox { comp: "BlockBox", at: block_at }];

/// BPI_GetWallBounceMultiplier(BouncedActor) (Steven_ShieldActor's, _Up's too): a fighter in hitstun
/// (BPI_HitstunActive) counts a bounce @139-208, and at MaxBounces (4) DestroySpawnedActor(false) @287; then 1.3
/// (PerkBounceMultiplier) with IsBouncePerkActive for a bounced actor of another team @302-594, else 1.0.
fn bounce_mult(w: &mut World, id: ActorId, _part: u32, f: FighterId) -> Real {
    let stun = in_hitstun(w, f);
    let p = crate::actors::world_actor::actor_p(w, ActorRef::Actor(id));
    let (max_bounces, perk_mult) = (num(p, "MaxBounces").unwrap_or(0.0), num(p, "PerkBounceMultiplier").unwrap_or(1.0));
    let team = crate::world::spawned_platform::plat_of(w, id).map_or(-1, |p| p.team);
    let other = w.fighter(f).team.team != team;
    let Some(s) = st_mut(w, id) else { return 1.0 };
    if stun && !s.pending_death {
        s.bounce_counter += 1;
        if crate::real(s.bounce_counter) >= max_bounces {
            s.pending_death = true;
        }
    }
    if s.bounce_perk && other { perk_mult } else { 1.0 }
}

/// Steven_ShieldActor_Up's ReceiveBeginPlay: the parent's, then the owner's BPI_Steven DecrementUpShieldAmmo @721 when
/// the cast succeeds, then every other Steven_ShieldActor_Up of the owner's Comp_Spawner (Get Spawned Actor
/// (Steven_ShieldActor_Up_C) @829: the filter of SpawnedActors by the class, `spawner_filtered`, then that list) with
/// the same owner is K2_DestroyActor'd @316 (its EndPlay's death effects: `shield_end`). Spawned not attached: the
/// socket (fml_un_C_spine_a with (0, 0, 300) on the ground montage, fml_un_C_prop1 on the air one) at the spawn.
fn spawn_up(w: &mut World, f: FighterId, r: &SpawnRequest) -> (WorldActorState, bool) {
    let np = crate::world::spawning::request_props(w, f, r);
    let at = crate::world::spawn_spot::spawn_place(w, f, &np);
    let mut s = new_shield(w, f, r.cls, at.x, at.y, crate::fighter::facing(w, f));
    // the parent's BeginPlay loop; the class's own part runs on behind a destroy there
    let dead = blast_loop(w, f, r.cls, &mut s);
    super::pawn::decrement_up_shield_ammo(w, f);
    if let Some(h) = w.fighter(f).hooks.char.spawner_filtered {
        h(w, f, UP);
    }
    // The filter's list (FilterArray IsA: the class or a child; no class derives from _Up), each with this owner.
    let list = w.fighter(f).items.spawned_actors.clone();
    for x in list {
        if let ActorRef::Actor(aid) = x
            && w.actors.actors.get(aid).is_some_and(|a| a.cls == UP && !a.dead)
        {
            crate::actors::world_actor::mark_spawned_dead(w, x);
        }
    }
    (WorldActorState::StevenShield(s), dead)
}

/// The bodies the traces meet besides the platform parts: BlockBox (ProjectileDestructible) and Hurtbox (HurtBox),
/// with their collision on.
fn shield_bodies(w: &World, id: ActorId, out: &mut Vec<TraceBody>) {
    let (Some(s), Some(a)) = (st(w, id), w.actors.actors.get(id)) else { return };
    let comps = actor_def(w, ActorRef::Fighter(a.owner), a.cls).map(|d| &d.components);
    for (comp, h) in [(name!("BlockBox"), s.block), (name!("Hurtbox"), s.hurt)] {
        let Some(h) = h else { continue };
        let Some(info) = body_info(comps.and_then(|c| obj(c, w.names.str(comp))), None) else { continue };
        if !info.on {
            continue;
        }
        out.push(TraceBody {
            ax: s.x,
            ay: s.y,
            bx: s.x,
            by: s.y,
            hx: h.hx,
            hy: h.hy,
            r: 0.0,
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
}

/// Comp_SizeManager.SetSizeScale on the shield (`actorSizeUpdate`): Buff_Enlarge's spawned-actor pass (@2366 the
/// Steven_ShieldActor cast -> @2032, no SizeCanScale or PolymorphScaleUp? test) and PolymorphGrenade's blast (@2858 ->
/// @2039, no PolymorphScaleUp? or team test) give it Buff_Polymorph_ProjectileEnlarge_WithHieghtChange (its
/// Comp_Actor_Destructible has no IsProjectile): SetActorScale3D(1.7), absolute, and the actor raised by
/// |44 x 1.7 - 44| (the root Capsule has no CapsuleHalfHeight of its own: the component's 44). Every box is the
/// root's child with no place, scale or turn of its own (`plane_box`), so each extent scales with the actor: BlockBox,
/// Hurtbox, Collision, WallbounceCollision, WallBox and FighterDetectionSphere (read with the scale in `shield_tick`).
fn shield_scale(w: &mut World, id: ActorId, s: Real, old: Real) {
    let r = s / old;
    if let Some(s) = st_mut(w, id) {
        for h in [&mut s.block, &mut s.hurt].into_iter().flatten() {
            h.hx *= r;
            h.hy *= r;
            h.hz *= r;
        }
    }
    if let Some(pl) = crate::world::spawned_platform::plat_mut(w, id) {
        for part in &mut pl.parts {
            part.hx *= r;
            part.hy *= r;
        }
    }
    place_shield(w, id);
}

fn place_shield(w: &mut World, id: ActorId) {
    let Some((x, y)) = st(w, id).map(|s| (s.x, s.y)) else { return };
    if let Some(pl) = crate::world::spawned_platform::plat_mut(w, id) {
        for part in &mut pl.parts {
            part.place(x, y);
            // a still box (a based fighter follows dx / dy every frame)
            part.dx = 0.0;
            part.dy = 0.0;
        }
    }
}

fn shield_capsule_half(w: &World, id: ActorId) -> Real {
    let Some(a) = w.actors.actors.get(id) else { return 44.0 };
    actor_def(w, ActorRef::Fighter(a.owner), a.cls)
        .and_then(|d| num_in(&d.components, "Capsule", "CapsuleHalfHeight"))
        .unwrap_or(44.0)
}

fn shield_move_z(w: &mut World, id: ActorId, dz: Real) {
    if let Some(s) = st_mut(w, id) {
        s.y += dz;
    }
    place_shield(w, id);
}

/// Steven_ShieldActor's kind: the owner's team, buffs, facing (_Up's yaw) and BPI_Steven behind the cast are read, any
/// pawn (`foreign_owner`). Neither class overrides the parent's two platform calls.
fn shield_hooks() -> WorldActorHooks {
    WorldActorHooks {
        size: Some(SizeHooks {
            scale: Some(shield_scale),
            capsule_half: Some(shield_capsule_half),
            move_z: Some(shield_move_z),
        }),
        bodies: Some(shield_bodies),
        step: Some(shield_tick),
        end: Some(shield_end),
        any_damage: Some(shield_damage),
        damage_box: DAMAGE_BOXES,
        hit_box_on: Some(shield_box_on),
        wall_bounce_multiplier: Some(bounce_mult),
        ignore_platform_collision: Some(crate::world::spawned_platform::ignore_platform_collision),
        ignore_wallbounce_collision: Some(crate::world::spawned_platform::ignore_wallbounce_collision),
        ..WorldActorHooks::default()
    }
}

// ---------------------------------------------------------------- Steven_ShieldActor_U_SpecialPlatform

/// `platformStepLife`: the shield's step, then MaxLifetime (the parent's timer -> Delay(DestroyActor_Delay 0) ->
/// DestroySpawnedActor).
fn platform_step_life(w: &mut World, id: ActorId) {
    crate::actors::shield_actor::shield_step(w, id);
    let Some(a) = w.actors.actors.get(id) else { return };
    let age = crate::actors::shield_actor::shield_of(a).map_or(0, |s| s.age);
    let end = life_end(crate::actors::world_actor::actor_p(w, ActorRef::Actor(id)));
    if !a.dead && crate::real(age) >= end {
        destroy_shield(w, id);
    }
}

/// BPI_GetWallBounceMultiplier: 1.3 with the perk for a bounced actor of another team, else 1.0; no bounce count.
fn platform_bounce_mult(w: &mut World, id: ActorId, _part: u32, f: FighterId) -> Real {
    let Some(a) = w.actors.actors.get(id) else { return 1.0 };
    let team = crate::world::spawned_platform::plat_of(w, id).map_or(-1, |p| p.team);
    if buff_of(w, a.owner, BOUNCE_PERK).is_some() && w.fighter(f).team.team != team {
        num(crate::actors::world_actor::actor_p(w, ActorRef::Actor(id)), "PerkBounceMultiplier").unwrap_or(1.0)
    } else {
        1.0
    }
}

/// This file's registry entries (StevenShieldProjectile_C and the Steven_ShieldActor classes).
pub fn register(r: &mut Registry) -> Result<(), RegistryError> {
    // reads f.time / chargeStartTime and the owner's data: any pawn
    let disc = ProjectileHooks {
        make_opts: Some(disc_opts),
        begin: Some(disc_begin),
        bounce_impact: Some(disc_bounce),
        destroyed: Some(disc_end),
        end: Some(disc_stop),
        ..ProjectileHooks::default()
    };
    let mut e =
        ActorEntry::new(ActorClass::Projectile(ProjectileClass::StevenShieldProjectile), ActorHooks::Projectile(disc));
    e.foreign_owner = true;
    r.actors.enter("StevenShieldProjectile_C", e)?;
    let _ = DISC;

    let mut barrier =
        ActorEntry::new(ActorClass::World(WorldActorClass::StevenShieldActor), ActorHooks::World(shield_hooks()));
    barrier.foreign_owner = true;
    r.actors.enter("Steven_ShieldActor_C", barrier)?;

    let mut up = ActorEntry::new(
        ActorClass::World(WorldActorClass::StevenShieldActorUp),
        ActorHooks::World(WorldActorHooks { spawn: Some(spawn_up), spawned: Some(spawned), ..shield_hooks() }),
    );
    up.foreign_owner = true;
    r.actors.enter("Steven_ShieldActor_Up_C", up)?;

    // the shield kind of `actors::shield_actor`, with its own step and bounce getter
    let mut platform = ActorEntry::new(
        ActorClass::World(WorldActorClass::StevenShieldActorUSpecialPlatform),
        ActorHooks::World(WorldActorHooks {
            step: Some(platform_step_life),
            wall_bounce_multiplier: Some(platform_bounce_mult),
            ..crate::actors::shield_actor::hooks()
        }),
    );
    platform.foreign_owner = true;
    r.actors.enter("Steven_ShieldActor_U_SpecialPlatform_C", platform)?;
    let _ = PLATFORM;
    Ok(())
}
