//! The reads the bot node classes share (the helpers the tasks and nodes read, `corners` /
//! `waypoints` / `num`, the copies the characters' `bots.rs` keep of them).
//!
//! Coordinates: the Blueprints work in world Y (side) and Z (up); sim x = -Y, y = Z. MoveX(+1) is toward -Y, so the
//! pad's x is the sim direction. FacingLeft is facing +Y: facing < 0, and the forward stick is `facing`.
//! An actor no longer in its arena reads its place as (0, 0) here (`actor_place`): its last place is not kept.

use crate::Real;
use crate::actors::ActorRef;
use crate::ai::blackboard::{BbType, BbVec};
use crate::ai::cx::Cx;
use crate::ai::pad::{Btn, Pad};
use crate::ai::props;
use crate::fighter::FighterId;
use crate::input::Dir;
use crate::math::{max, min};
use crate::world::World;
use crate::world::blast::BlastCorners;
use mvs_data::value::Props;

/// FLT_MAX.
pub const FLT_MAX: Real = 3.402_823_5e38;

/// FAISystem::InvalidLocation: what GetBlackboardValueAsVector reads of an unset key.
pub const INVALID: BbVec = BbVec { x: FLT_MAX, y: FLT_MAX, z: FLT_MAX };

/// `BEHAVIORS`: EBotBehavior of NumericDifficulty 0..3.
pub const BEHAVIORS: [&str; 4] =
    ["EBotBehavior::VeryEasy", "EBotBehavior::Easy", "EBotBehavior::Medium", "EBotBehavior::Hard"];

/// `sub`: a - b in single precision.
pub fn sub(a: Real, b: Real) -> Real {
    a - b
}

/// `clamp`: Math.min(hi, Math.max(lo, v)).
pub fn clamp(v: Real, lo: Real, hi: Real) -> Real {
    min(hi, max(lo, v))
}

/// `sq`.
pub fn sq(v: Real) -> Real {
    v * v
}

/// The sim x and `y` of an actor (a fighter's `y` is its capsule centre, not `actorY`).
fn place(w: &World, a: ActorRef) -> (Real, Real) {
    match a {
        ActorRef::Fighter(f) => {
            let b = &w.fighter(f).body;
            (b.x, b.y)
        }
        ActorRef::Stage(_) | ActorRef::Buff(_) => {
            crate::unported!("bot locY of a stage actor or a buff", w, a)
        }
        ActorRef::Projectile(id) if w.actors.projectiles.get(id).is_none() => {
            // removed: its last place is kept in `gone_motion`
            w.actors.gone_motion.iter().find(|g| g.id == id).map_or((0.0, 0.0), |g| (g.x, g.y))
        }
        _ => {
            let (x, y, _) = crate::actors::world_actor::actor_place(w, a).expect("a spawned actor's place");
            (x, y)
        }
    }
}

/// `locY`: K2_GetActorLocation's world Y; None reads the zero vector.
pub fn loc_y(w: &World, a: Option<ActorRef>) -> Real {
    a.map_or(0.0, |a| -place(w, a).0)
}

/// `locZ`: GetActorLocation's Z: a crouched fighter's is lower by its capsule shrink (`actorY`).
pub fn loc_z(w: &World, a: Option<ActorRef>) -> Real {
    match a {
        None => 0.0,
        Some(ActorRef::Fighter(f)) => crate::fighter::actor_y(w, f),
        Some(a) => place(w, a).1,
    }
}

/// `loc`: the actor location as a blackboard vector (world X = 0: the spawn points' X).
pub fn loc(w: &World, a: ActorRef) -> BbVec {
    BbVec { x: 0.0, y: loc_y(w, Some(a)), z: loc_z(w, Some(a)) }
}

/// `velOf`: Actor.GetVelocity in world Y / Z: the `vx` / `vy` of any object that has them and 0 for one
/// without; each family below reads the fields its class carries.
pub fn vel(w: &World, a: Option<ActorRef>) -> (Real, Real) {
    let (vx, vy) = match a {
        None => (0.0, 0.0),
        Some(ActorRef::Fighter(f)) => {
            let b = &w.fighter(f).body;
            (b.vx, b.vy)
        }
        Some(ActorRef::Projectile(id)) => w.actors.projectiles.get(id).map_or_else(
            || w.actors.gone_motion.iter().find(|g| g.id == id).map_or((0.0, 0.0), |g| (g.vx, g.vy)),
            |p| (p.vx, p.vy),
        ),
        // Deviation: a removed item reads velocity (0, 0); its last values are not kept.
        Some(ActorRef::Item(id)) => w.actors.items.get(id).map_or((0.0, 0.0), |p| (p.vx, p.vy)),
        // Deviation: a removed world actor reads velocity (0, 0); its last values are not kept.
        Some(ActorRef::Actor(id)) => w.actors.actors.get(id).map_or((0.0, 0.0), |p| world_actor_vel(&p.state)),
        // `Ship` carries vx / vy (the ship's move). Deviation: a removed one reads (0, 0).
        Some(ActorRef::Ship(id)) => w.actors.ships.get(id).map_or((0.0, 0.0), |s| (s.vx, s.vy)),
        // `Marker` and `MortarAim` hold `vx = 0`, `vy = 0`.
        // `AimPunch` and `Accelerator` have neither: 0.
        Some(ActorRef::Marker(_) | ActorRef::Mortar(_) | ActorRef::AimPunch(_) | ActorRef::Accelerator(_)) => {
            (0.0, 0.0)
        }
        // A stage actor (every class of `stageActors`) has no vx / vy: 0. A buff
        // actor holds `vx: 0, vy: 0`.
        Some(ActorRef::Stage(_) | ActorRef::Buff(_)) => (0.0, 0.0),
    };
    (-vx, vy)
}

/// The `vx` / `vy` of a world actor's class: the classes whose velocity moves (a body or a movement component of
/// their own). Every other class reads 0: the dealer actors hold `vx = 0` / `vy = 0`, the rest
/// (ShieldActor, ProjectileBarrier, the listeners, aimers, zones ...) have neither field.
fn world_actor_vel(s: &crate::actors::WorldActorState) -> (Real, Real) {
    use crate::actors::WorldActorState as S;
    match s {
        // SmokeGrenade.
        S::SmokeGrenade(a) => (a.vx, a.vy),
        // SnackBox.
        S::SnackBox(a) => (a.vx, a.vy),
        // KillPie: `vy` alone (ProjectileMovement's velocity Z); no vx, so 0.
        S::KillPie(a) => (0.0, a.vy),
        // Harley's Bomb: the body's while it simulates, 0 once attached.
        S::HarleyBomb(a) => (a.vx, a.vy),
        // JakeHorse (JakeHorsePlatform has none).
        S::JakeHorse(a) => (a.vx, a.vy),
        // PolymorphGrenade.
        S::PolymorphGrenade(a) => (a.vx, a.vy),
        // Evidence and VelmaVial.
        S::Evidence(a) => (a.vx, a.vy),
        S::VelmaVial(a) => (a.vx, a.vy),
        // JerryActor.
        S::JerryActor(a) => (a.vx, a.vy),
        // BouncePad.
        S::BouncePad(a) => (a.vx, a.vy),
        _ => (0.0, 0.0),
    }
}

/// `facingLeft`: facing +Y.
pub fn facing_left(w: &World, f: FighterId) -> bool {
    w.fighter(f).body.facing_dir < 0.0
}

/// `fighter(o)`: `o instanceof Fighter` (a destroyed minion too).
pub fn fighter(a: Option<ActorRef>) -> Option<FighterId> {
    match a {
        Some(ActorRef::Fighter(f)) => Some(f),
        _ => None,
    }
}

/// A cast to BaseFighter of a fighter still in the match (`World.all`).
pub fn fighter_in_match(w: &World, a: Option<ActorRef>) -> Option<FighterId> {
    fighter(a).filter(|f| w.pawns.as_ref().unwrap_or(&w.players).contains(f))
}

/// `valid(a)`: IsValid: a destroyed actor is not.
pub fn valid(w: &World, a: Option<ActorRef>) -> bool {
    a.is_some_and(|a| !crate::actors::world_actor::actor_dead(w, a))
}

/// `actor(c, prop)`: GetBlackboardValueAsActor of a key selector: `None` for a key of another type.
pub fn actor(cx: &Cx, prop: &str) -> Option<ActorRef> {
    let k = cx.key(prop);
    if cx.bot.bb.ty(k) == Some(BbType::Object) { cx.bot.bb.obj(k) } else { None }
}

/// GetBlackboardValueAsVector of a key selector: `None` for an unset key or a key of another type.
pub fn vec_opt(cx: &Cx, prop: &str) -> Option<BbVec> {
    let k = cx.key(prop);
    if cx.bot.bb.ty(k) == Some(BbType::Vector) { cx.bot.bb.vec(k) } else { None }
}

/// `vec(c, prop)`: GetBlackboardValueAsVector: an unset key reads FAISystem::InvalidLocation (FLT_MAX).
pub fn vec(cx: &Cx, prop: &str) -> BbVec {
    vec_opt(cx, prop).unwrap_or(INVALID)
}

/// `num(c, prop)`: GetBlackboardValueAsFloat: 0 for a key of another type.
pub fn num(cx: &Cx, prop: &str) -> Real {
    let k = cx.key(prop);
    if cx.bot.bb.ty(k) == Some(BbType::Float) { cx.bot.bb.num(k) } else { 0.0 }
}

/// `playerTeam(f, last)`: PlayerDataManager.GetByFighter(f) then GetTeamIndex: a player's team. A minion has no
/// PlayerData (no PlayerState): GetTeamIndex on None is skipped, so the out value is what it held, `last` (the
/// caller's previous read; 0 at the start of a function's call, as its locals start).
/// Unverified: that the call on None leaves an out parameter as it was (no clear) is UE 4.26 source, not read in
/// the exe.
pub fn player_team(w: &World, f: FighterId, last: i32) -> i32 {
    let fr = w.fighter(f);
    if fr.life.is_minion { last } else { fr.team.team }
}

/// `byDifficulty(c, m)`: a per-difficulty table (a TMap keyed by EBotBehavior) lerped by the controller's
/// NumericDifficulty (VeryEasy 0 .. Hard 3).
pub fn by_difficulty(cx: &Cx, p: &Props, k: &str) -> Real {
    let t = BEHAVIORS.map(|b| props::map_find(p, k, b).unwrap_or(0.0));
    let nd = cx.bot.difficulty;
    // t[i]: an index that is no whole number 0..3 reads undefined, 0
    let at = |i: Real| [0.0, 1.0, 2.0, 3.0].iter().position(|&x| x == i).map_or(0.0, |k| t[k]);
    let (lo, hi) = (at(nd.floor()), at(nd.ceil()));
    lo + (hi - lo) * (nd - nd.floor())
}

/// `stickFor(d, f)`: BotAttack_BTT's stick for an Enum_InputDirection8Way, `f` the facing.
pub fn stick_for(d: Dir, f: Real) -> (Real, Real) {
    let x = match d {
        Dir::Fwd | Dir::FwdUp | Dir::FwdDown => f,
        Dir::Back | Dir::BackUp | Dir::BackDown => -f,
        _ => 0.0,
    };
    let y = match d {
        Dir::Up | Dir::FwdUp | Dir::BackUp => 1.0,
        Dir::Down | Dir::FwdDown | Dir::BackDown => -1.0,
        _ => 0.0,
    };
    (x, y)
}

/// `releaseJumpIfHeld`: BPI_IsButtonPressed(Jump) -> JumpReleased.
pub fn release_jump_if_held(pad: &mut Pad) {
    if pad.is_held(Btn::Jump) {
        pad.release(Btn::Jump);
    }
}

/// `corners(c)`: the one blast box (BPI_GetBlastBoxes): BPI_GetTopLeftCorner / BPI_GetBottomRightCorner as world
/// Y, Z, asked on every call (the FreeForAll shrink moves them).
pub fn corners(w: &World) -> BlastCorners {
    crate::world::blast::blast_corners(w)
}

/// A BotWaypoint actor of the level: its UE location, Edge and RecoveryWaypoint.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Waypoint {
    pub x: Real,
    pub y: Real,
    pub z: Real,
    pub edge: bool,
    pub recovery: Real,
}

/// `waypoints(c)`: the level's BotWaypoint actors (GetAllActorsOfClass), in the stage data's order.
pub fn waypoints(w: &World) -> impl Iterator<Item = Waypoint> + '_ {
    w.data.stage.waypoints.iter().map(|p| Waypoint {
        x: crate::math::narrow(p.loc[0]),
        y: crate::math::narrow(p.loc[1]),
        z: crate::math::narrow(p.loc[2]),
        edge: p.edge,
        recovery: crate::math::narrow(p.recovery),
    })
}

/// A property's EActionDirection / Enum_InputDirection8Way number as a [`Dir`] (`p.InputDirection ?? 0`).
pub fn dir_of(n: Real) -> Dir {
    match [0.0, 1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0, 9.0].iter().position(|&x| x == n) {
        Some(0) => Dir::Up,
        Some(1) => Dir::Down,
        Some(2) => Dir::Fwd,
        Some(3) => Dir::Back,
        Some(4) => Dir::Neutral,
        Some(5) => Dir::Default,
        Some(6) => Dir::FwdUp,
        Some(7) => Dir::FwdDown,
        Some(8) => Dir::BackUp,
        Some(9) => Dir::BackDown,
        _ => panic!("a bot task's direction property {n} is no direction of the build's enum"),
    }
}

/// `list[n]` for a whole `n` a Random* call gave (an index past the end gives None from `get`).
#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss, reason = "a Random* result is a small whole number")]
pub fn index_of(n: Real) -> usize {
    if (0.0..1e9).contains(&n) { n as usize } else { usize::MAX }
}

/// The sim place of an actor (a fighter's `y` is its capsule centre): `x`, `y` of the object.
pub fn sim_xy(w: &World, a: ActorRef) -> (Real, Real) {
    place(w, a)
}
