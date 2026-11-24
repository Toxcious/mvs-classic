//! LeBron's basketball: the projectile classes BasketBallProjectile_C (the parent graph, run
//! by BasketBallProjectile_AllyPass_C), BasketBallProjectile_Shot_C and BasketBallProjectile_Pass_C ([`Ball`], a
//! projectile's `ext`), and the team mate's buffs Buff_BasketballPass and Buff_IgnitedBasketballPass.
//!
//! `Ball`'s method overrides are called from the shared projectile code (`actors/projectile.rs`):
//! `step` ([`pass_start`]), `receiveDamage` ([`damaged`]) and `hitNonPlayer` ([`hit_non_player`]).
//!
//! The ball in his hands is a buff of the pawn (pawn.rs); a throw spawns one of these, which flies, bounces and can
//! come back into his hands. Which montage spawns which: the Pass by the four pass montages (through a
//! ProjectileAimer: the ball is spawned at the aimer's ActivateSpawned) and by Special_Pass_Bounce; the Shot by
//! Special_Shoot_High / Shoot_Fadeaway / SlamBallDown / Air_Dunk_ThrowDown. The parent class is spawned by nothing of
//! his data.
//!
//! The class data both children share: Capsule 70 / 70 (profile ProjectileNavPlatform: Platform Block), HitBox /
//! HitBox2D radius 60, bounce 1 and friction 0, gravity scale 3 (Pass 4), MaxLifetime 5, MaxNumberofBounces 4,
//! IsAShot, Health 1, TakesDamage, DestroyOnHitProjectile, DestroyOnExitedArena?, no DestroyOnHitFighter, no
//! DestroyOnBlocked; the Pass also AllyPass.
//!
//! Where its hit box is turned on and off after the spawn, which decides the catch: once inside the spawn call
//! (Comp_Spawner @12653 BPI_UpdateHitbox -> HitBoxesOn -> ActivateHitbox, behind BeginPlay and
//! BPI_SetInitialMovement, with CanBePickedUp and Activated both false) and off at the destroy. No ball graph toggles
//! HitBox or HitBox2D again, so the owner's catch needs a begin of an overlap raised while both gates are open: one
//! already counted when they open raises nothing.
//!
//! The order of a frame, as the port has it: the fighters' ticks with the mid-frame test of every projectile behind
//! each (`World::overlap_mid_frame`), then per projectile its movement component's tick (`projectile::step`) and its
//! actor tick (`base_tick` / `shot_tick`), then the test of every projectile where its move ended. In the build the
//! overlaps of a move are raised inside the move, ahead of the actor's tick of that frame, so what a latent action of
//! the tick at age N writes is seen by the mid-frame tests of the next frame (age still N) and not by the test behind
//! the move of age N: [`since`].
//!
//! Latent actions: BeginPlay runs inside the spawn call, ahead of the actor's first tick, which is in the spawn frame
//! (TG_NewlySpawned, behind its movement component's first move: `age` 1). A Delay made ahead of the actor's own tick
//! of a frame is counted by that tick, so Delay(0.0) @2369 ends in the first tick (CanBePickedUp, `PICK_AGE`) and
//! Delay(0.3) @2314 in the tick of age latentFrames(0.3) (Activated, `act_age`). The hit response's Delay(0.1) @3395 is
//! made inside a move, a fighter's tick or the spawn call, each ahead of the ball's tick of that frame, so that tick
//! counts it first (`mid`: the ball's tick of the frame is still to come in the port too).
// Unverified: assumed, not read: that the ball's first actor tick is in its spawn frame and counts the two Delays of
// BeginPlay (the native first-tick rule read for a Delay made ahead of the actor's own tick).
//!
//! Teams: a team mate's pickup @8402-8884 -> @7144 (Buff_BasketballPass or, with HotHands and a no-look pass,
//! Buff_IgnitedBasketballPass on him: [`overlap`]), the two buffs and the ball they throw back,
//! BasketBallProjectile_AllyPass (owner the team mate, Instigator LeBron: `Projectile::inst_override`).
//! CheckAllyInCollisionAtStart @7669 is a no-op here ([`begin`]).
// Not ported: AllowTeleportTo's UI and the dash to a team ball (later.rs), the Pass's homing on an ally (Homing On
// Allies? assigns a target with bIsHomingProjectile off: no move), AimWidget; the hit pause of a spawned actor
// (DestroySpawnedActor(false) waits for it with ReceivesHitPause: destroyed at once here).

use super::pawn::play_ball_receive_anim_if_not_in_attack;
use super::state::{BALL, DUNK, lebron_of, set_teleport_dunk_available};
use crate::Real;
use crate::State;
use crate::actors::projectile::{
    CollisionResponse, Projectile, ProjectileExt, class_p, hit_collision, instigator, set_platform_response,
};
use crate::actors::world_actor::actor_cls;
use crate::actors::{ActorClass, ActorRef, ProjectileClass, ProjectileId};
use crate::combat::boxes::{WorldSphere, obj as obj_type};
use crate::combat::buffs::{BuffExt, buff_of, end_buff, inst_view, spawn_buff};
use crate::combat::elem::{ElemExt, actor_buff_signal, clear_homing, elem_spawn, instigator_charge, set_elem_ext};
use crate::combat::hit::hurt::hurt_touched;
use crate::combat::hit::knockback::timer_count;
use crate::combat::hits::{apply_projectile_hit, dealer};
use crate::combat::phases::{give_vbuf, struck_pose};
use crate::combat::{BuffClass, BuffRef, HitPacket};
use crate::events::{Event, NotifyRef};
use crate::fighter::common::{actor_is_a, latent_frames};
use crate::fighter::control::{DodgeState, attack_dodged, dodge_over};
use crate::fighter::match_side::{Foreign, SpawnRequest};
use crate::fighter::moves::V2;
use crate::fighter::traces::sphere_overlap_actors;
use crate::fighter::{FighterId, actor_y, invulnerable, is_evading};
use crate::input::{Dir, get8_way};
use crate::name;
use crate::names::Name;
use crate::registry::entries::{ActorEntry, ActorHooks, BuffHooks, ProjectileHooks, ReflectHooks};
use crate::registry::props::num;
use crate::registry::signals::{ActorBuffArg, ActorBuffSignal, BuffSignal, BuffSignalArg};
use crate::registry::{BuffEntry, Registry, RegistryError};
use crate::world::World;
use crate::world::pawns::is_match_fighter;
use crate::world::spawning::destroy_projectile;
use mvs_data::value::{Props, Value};
use std::sync::Arc;

const ALLY_PASS: &str = "BasketBallProjectile_AllyPass_C";
const PASS: &str = "Buff_BasketballPass_C";
const IGNITED_PASS: &str = "Buff_IgnitedBasketballPass_C";
/// Delay(0.0) @2369 -> @128 CanBePickedUp = true.
const PICK_AGE: Real = 1.0;
const NEVER: Real = 1e9;
/// A hit box's reach from a root: further than any of his hurt capsules is from his root, with the box's radius.
const REACH: Real = 600.0;

/// Delay(0.3) @2314 -> @116 Activated = true.
fn act_age() -> Real {
    latent_frames(0.3)
}

/// Delay(0.1) @3395 -> @72: the hit response's DoOnce reopens.
fn reopen() -> Real {
    latent_frames(0.1)
}

/// K2_SetTimerDelegate(0.25) @6277 / @2785, set inside the move of frame k: pending in that frame's timer tick, which
/// does not count it, then timerCount(0.25) = 15 counted ticks: it fires in frame k + 15. The count here is taken
/// down by the ball's actor tick from frame k on, so one more.
fn sound_ticks() -> i32 {
    timer_count(0.25) + 1
}

/// `Ball extends Projectile`: what the class adds.
#[derive(State, Debug)]
pub struct Ball {
    /// IsANoLookPass (BeginPlay @1537).
    pub no_look: bool,
    /// Activated written by the hit response @2972 (the Delay's write is `act_age`).
    pub hit_activated: bool,
    /// The hit response's DoOnce: -1 open; else the age of the tick that reopens it (`NEVER`: none).
    pub reopen_at: Real,
    /// BounceIncrement.
    pub bounces: i32,
    /// HitEnemy.
    pub hit_enemy: bool,
    /// AllowTeleportTo (the parent class alone writes it true).
    pub allow_teleport_to: bool,
    /// The Shot's IgnorePlatformsCheck.
    pub ignore_platforms_check: bool,
    /// The Instigator's hurt capsules its hit box overlaps (places in the box table's hurt list).
    pub inside: Vec<Real>,
    /// The same per team mate of his (by fighter id), for the ally's pickup; a hole is a team mate not met.
    pub others: Vec<Option<Vec<Real>>>,
    /// The velocity the move's pass in flight started with.
    pub pass_vx: Real,
    pub pass_vy: Real,
    /// The last fighter test came ahead of the ball's tick of the frame (mid-frame, or at the spawn).
    pub mid: bool,
    /// AudioBounce's timer, in ticks left.
    pub bounce_sound: i32,
    /// Audio's timer (the parent class's bounce body).
    pub audio_sound: i32,
}

impl Default for Ball {
    /// The field initial values.
    fn default() -> Self {
        Ball {
            no_look: false,
            hit_activated: false,
            reopen_at: -1.0,
            bounces: 0,
            hit_enemy: false,
            allow_teleport_to: false,
            ignore_platforms_check: false,
            inside: Vec::new(),
            others: Vec::new(),
            pass_vx: 0.0,
            pass_vy: 0.0,
            mid: false,
            bounce_sound: 0,
            audio_sound: 0,
        }
    }
}

/// The extension of Buff_BasketballPass and Buff_IgnitedBasketballPass (`ext`: `{ from }`).
#[derive(State, Debug, Default)]
pub struct PassState {
    /// The Instigator the buff was spawned by (the ball's thrower), none when it was spawned without one.
    pub from: Option<FighterId>,
    /// GetOwner of the buff actor's hit: `from`, else the buff's owner.
    pub by: Option<FighterId>,
    /// The buff actor's Comp_Pawn_HitBoxHandler dodge variables, for the explosion's call alone.
    pub dodge: Option<DodgeState>,
}

fn pr(w: &World, id: ProjectileId) -> &Projectile {
    w.actors.projectiles.get(id).expect("a live projectile")
}

fn pm(w: &mut World, id: ProjectileId) -> &mut Projectile {
    w.actors.projectiles.get_mut(id).expect("a live projectile")
}

fn ball(w: &World, id: ProjectileId) -> &Ball {
    match &pr(w, id).ext {
        ProjectileExt::Ball(b) => b,
        e => panic!("a basketball class without its state: {e:?}"),
    }
}

fn ball_mut(w: &mut World, id: ProjectileId) -> &mut Ball {
    ball_of(pm(w, id))
}

fn ball_of(p: &mut Projectile) -> &mut Ball {
    match &mut p.ext {
        ProjectileExt::Ball(b) => b,
        e => panic!("a basketball class without its state: {e:?}"),
    }
}

/// What a latent action of the actor's tick of age `at` wrote, as a test now sees it (see the header).
fn since(p: &Projectile, at: Real, mid: bool) -> bool {
    let age = crate::real(p.age);
    if mid { age >= at } else { age > at }
}

fn sound(w: &mut World, id: ProjectileId, ev: Name) {
    let p = pr(w, id);
    let e = Event::Sound { fighter: p.owner_id, ev, x: Some(p.x), y: Some(p.y), hit: None, pooled: false, key: None };
    w.events.push(e);
}

// ---- the Ball overrides

/// `Ball.step`'s own part, ahead of the parent's movement step: the velocity the move's pass starts with
/// (`passVx` / `passVy`). Called by `projectile::step` for a ball.
pub fn pass_start(w: &mut World, id: ProjectileId) {
    let p = pm(w, id);
    let (vx, vy) = (p.vx, p.vy);
    let b = ball_of(p);
    b.pass_vx = vx;
    b.pass_vy = vy;
}

/// `Ball.receiveDamage` behind the parent's (ParentProjectilePhysics's damage did not destroy it, the damage not 0):
/// ReceiveAnyDamage @5025 runs the hit response @5071 whatever the parent did. A response behind the parent's destroy
/// writes fields of a ball that is gone, so it is not run then. The world's callers come behind a fighter test of the
/// same pass, so `mid` says which pass this is. True: destroyed. Called by `projectile::receive_damage` for a ball.
pub fn damaged(w: &mut World, id: ProjectileId) -> bool {
    let mid = ball(w, id).mid;
    hit_response(w, id, mid, false)
}

/// `Ball.hitNonPlayer` behind the parent's (which did not destroy it): OnHitAnything @2839 runs the hit response when
/// HitActor is a SpawnedActorParent @2867-2932. The dealer's handler broadcasts HitAnything right behind
/// HitNonPlayer, and the response is taken at this call for a SpawnedActorParent (a projectile, an item); a pawn that
/// is none (`NOT_SPAWNED_ACTOR`: the Banana Guard) takes the parent's alone. True: destroyed. Called by
/// `projectile::hit_non_player` for a ball.
pub fn hit_non_player(w: &mut World, id: ProjectileId, other_type: i32) -> bool {
    if other_type == crate::actors::projectile::NOT_SPAWNED_ACTOR {
        return false;
    }
    let mid = ball(w, id).mid;
    hit_response(w, id, mid, false)
}

/// The hit response @2942-3449 (authority). Its callers: HitEnemyPlayer @5473, ReceiveAnyDamage @5071, Blocked @7811
/// and OnHitAnything with a spawned actor @2932. True: DestroySpawnedActor(false) @3216.
///   Activated = true @2972 at every entry, ahead of the DoOnce;
///   the DoOnce @2983-3043, reopened @72 by the Delay(0.1) @3395 (the destroy's path makes no Delay: it stays shut);
///   the parent class's pre-block @3840-4215 (not a shot): AllowTeleportTo and the pawn's TeleportDunkReady follow
///     HitEnemy (the UI calls AllowTeleportDunk / UIVisuals are drawing);
///   BounceIncrement + 1 @3068-3137, at MaxNumberofBounces the destroy;
///   a shot @3246-3310: Velocity x -1 through BPI_SetInitialMovement(v, MaxSpeed, InitialSpeed), which writes the
///     velocity and the two speeds as they are; not a shot @3450-3750: (-X, -Y x 0.25, 10000);
///   Delay(0.1) @3395.
/// `mid`: the call is ahead of the ball's move and tick of this frame (a fighter's tick, or the spawn call).
/// `in_move`: the call is raised inside the move's sweep (its hit box meeting a fighter where the move ended).
/// A response that is not `mid` comes behind the port's actor tick of the frame, where the build's is ahead of it
/// (inside the move): the tick's Platform rule is run again on the velocity the response left ([`platforms`]), so the
/// next frame's move reads what the build's tick wrote.
// Unverified: the velocity a response inside the sweep reverses is the one the pass started with (the movement
// component integrates behind MoveUpdatedComponent, and skips that pass's ComputeVelocity when an event of the move
// changed Velocity: the native rule for a handler's write), so the pass's gravity is not in it; with a bounce in the
// same move it is the bounced velocity as the handler left it. Which of a move's passes raised the overlap is not
// kept. A response from a damage or from another spawned actor reverses the velocity as it stands (the port tests
// those behind the move; in the build a fighter's hit box meets the ball in the fighter's tick, ahead of the move,
// and that move's gravity is then added to the reversed velocity).
// Deviation: SpawnedActorParentVelocity (what a hit reads as the dealer's velocity) is not written here; the port's
// copy follows at the actor's next tick. Nothing reads it in between with HitEachActorOnlyOnce and one enemy.
fn hit_response(w: &mut World, id: ProjectileId, mid: bool, in_move: bool) -> bool {
    let (is_shot, max) = {
        let cp = class_p(w, pr(w, id));
        (cp.get("IsAShot").is_some_and(crate::registry::props::truthy), num(cp, "MaxNumberofBounces").unwrap_or(0.0))
    };
    ball_mut(w, id).hit_activated = true;
    {
        let b = ball(w, id);
        if b.reopen_at >= 0.0 && !since(pr(w, id), b.reopen_at, mid) {
            return false;
        }
    }
    ball_mut(w, id).reopen_at = NEVER;
    if !is_shot {
        let he = ball(w, id).hit_enemy;
        ball_mut(w, id).allow_teleport_to = he;
        // @3979 / @4162: the Instigator's
        if let Some(i) = instigator(pr(w, id)) {
            set_teleport_dunk_available(w, i, he);
        }
    }
    let b = ball_mut(w, id);
    b.bounces += 1;
    if crate::real(b.bounces) == max {
        return true;
    }
    let (pvx, pvy) = (b.pass_vx, b.pass_vy);
    let p = pm(w, id);
    if is_shot {
        p.vx = if in_move { -pvx } else { -p.vx };
        p.vy = if in_move { -pvy } else { -p.vy };
    } else {
        p.vx = -p.vx * 0.25;
        p.vy = 10000.0;
    }
    if !mid {
        platforms(p);
    }
    // The Delay is counted first by the ball's tick of this frame: behind this call for a `mid` one (age + 1; at the
    // spawn place the ticks of age 1 to the reopening), already run for the others.
    let at = (if mid { crate::real(p.age) + 1.0 } else { crate::real(p.age) }) + reopen() - 1.0;
    ball_of(p).reopen_at = at;
    false
}

// ---- the kind

/// ReceiveBeginPlay @1926, behind the parent's. @1246 (authority, the owner a fighter): IsANoLookPass = the owner's
/// current montage is one of NoLookMontages @1371-1537 (read by the ally's pickup @7423) and, without AllyPass, the
/// owner's Comp_State_Manager.RemoveBuff(the ball buff, 0) @1776: a Shot takes the ball out of his hands itself
/// (SlamBallDown has no RemoveBuff notify), a Pass leaves that to its montage's notify. Then the binds (the bounce
/// handlers below; OnHitByEnemy and OnKnockedBack have empty bodies) and the two Delays (the header). The Shot's own
/// BeginPlay @1603 adds a Delay(0.0) that sets a field nothing reads and, with Homing On Allies? (the Pass), looks for
/// an ally to home at (a target with no homing on).
/// CheckAllyInCollisionAtStart @2011 -> @7669 (not AllowTeleportTo, not AllyPass: the Shot) asks HitBox's
/// GetOverlappingActors @156: every hit box template is NoCollision until the spawner's ActivateHitbox, behind
/// BeginPlay, and the list holds the overlaps a begin added (OverlappingComponents), so it is empty here and the loop
/// gives nobody the pass: nothing is run. A team mate inside the box at the spawn has the begin of his overlap at
/// the activation, with CanBePickedUp still false.
fn begin(w: &mut World, id: ProjectileId) {
    let Some(f) = pr(w, id).owner else { return };
    let (no_look, ally) = {
        let cp = class_p(w, pr(w, id));
        let m = w.fighter(f).montages.montage.as_ref().map(|m| w.names.str(m.montage.name));
        let nl = m.is_some_and(|m| {
            cp.get("NoLookMontages").and_then(Value::as_arr).is_some_and(|a| a.iter().any(|v| v.as_str() == Some(m)))
        });
        (nl, cp.get("AllyPass").is_some_and(crate::registry::props::truthy))
    };
    ball_mut(w, id).no_look = no_look;
    if ally {
        return;
    }
    let live: Vec<BuffRef> = w
        .fighter(f)
        .buffs
        .buffs
        .iter()
        .filter(|b| b.cls == BALL && b.frames > 0.0)
        .map(|b| BuffRef { fighter: f, serial: b.serial })
        .collect();
    for b in live {
        end_buff(w, f, b);
    }
}

/// The handlers of ProjectileMovement.OnProjectileBounce, in the order they are bound, inside the move:
///   the component-bound event @7664 -> @6254: AudioBounce in 0.25 s (sfx_cha_C016_foley_bounce @8048), the timer set
///     again by every bounce;
///   SlowDownOnBounce @5076 (bound in BeginPlay @2025, authority): Velocity x 0.9 @5106-5170;
///   the class's OnProjectileBounce (below).
/// The timer is counted in ticks of the actor ([`sound_ticks`]): the build's fires in the world's timer tick of the
/// frame 15 after the bounce's, a frame's end where this takes the same frame's actor tick.
fn bounced(p: &mut Projectile) {
    p.vx *= 0.9;
    p.vy *= 0.9;
    let (vx, vy) = (p.vx, p.vy);
    let b = ball_of(p);
    b.bounce_sound = sound_ticks();
    b.pass_vx = vx;
    b.pass_vy = vy;
}

/// The parent class's OnProjectileBounce @2424-2838: the count, HitEnemy and AllowTeleportTo off, the pawn's
/// TeleportDunkReady off @2656, at MaxNumberofBounces DestroySpawnedActor(false) @2746, else Audio in 0.25 s (@8043 ->
/// the same sound @4220).
fn base_bounce(w: &mut World, id: ProjectileId, _nx: Real, _ny: Real, _ivx: Real, _ivy: Real) -> bool {
    bounced(pm(w, id));
    let b = ball_mut(w, id);
    b.bounces += 1;
    b.hit_enemy = false;
    b.allow_teleport_to = false;
    let n = b.bounces;
    if let Some(i) = instigator(pr(w, id)) {
        set_teleport_dunk_available(w, i, false); // @2656: the Instigator's
    }
    if crate::real(n) == num(class_p(w, pr(w, id)), "MaxNumberofBounces").unwrap_or(0.0) {
        return true;
    }
    ball_mut(w, id).audio_sound = sound_ticks();
    false
}

/// The Shot's override @1909 (no parent call): the homing flag off (a buff's homing with it: `clear_homing`; the
/// ball's own, the Pass's on an ally, is not ported). A floor or a wall is not counted for a Shot or a Pass: only the
/// hit response counts, and the lifetime or the blast box ends one that meets nobody.
fn shot_bounce(w: &mut World, id: ProjectileId, _nx: Real, _ny: Real, _ivx: Real, _ivy: Real) -> bool {
    bounced(pm(w, id));
    clear_homing(w, ActorRef::Projectile(id));
    false
}

fn timers(w: &mut World, id: ProjectileId) {
    let b = ball_mut(w, id);
    let mut fire = [false; 2];
    if b.bounce_sound > 0 {
        b.bounce_sound -= 1;
        fire[0] = b.bounce_sound == 0;
    }
    if b.audio_sound > 0 {
        b.audio_sound -= 1;
        fire[1] = b.audio_sound == 0;
    }
    for _ in fire.into_iter().filter(|&f| f) {
        sound(w, id, name!("sfx_cha_C016_foley_bounce"));
    }
}

/// The parent class's ReceiveTick @7816, behind the move: while AllowTeleportTo, the Capsule's Platform response is
/// Ignore with Velocity.Z >= 0 @7961 and Block otherwise @8002 (a write of the value it has does nothing).
fn base_platform(p: &mut Projectile) {
    if ball_of(p).allow_teleport_to {
        let r = if p.vy >= 0.0 { CollisionResponse::Ignore } else { CollisionResponse::Block };
        set_platform_response(p, r);
    }
}

fn base_tick(w: &mut World, id: ProjectileId) -> bool {
    timers(w, id);
    base_platform(pm(w, id));
    false
}

/// The Shot's ReceiveTick @1943: the parent's tick first (the call @1943; AllowTeleportTo is written true by the
/// not-a-shot branch of the hit response alone, so it writes nothing for a Shot or a Pass), then @394: with
/// GetVelocity().Z > 0 and IgnorePlatformsCheck off, the flag on @597 and the Capsule's Platform response Ignore
/// @608; with Z <= 0 and the flag on, the flag off @545 and Block @556. Written behind the move, so the next frame's
/// move reads it: a ball rises through a platform from the frame after its velocity turned upward and lands on one
/// from the frame after it turned down.
fn shot_tick(w: &mut World, id: ProjectileId) -> bool {
    base_tick(w, id);
    shot_platform(pm(w, id));
    false
}

/// The Platform writes of the class's tick, on the velocity as it stands: the parent's, and for a child the Shot's.
// Deviation: BasketBallProjectile_AllyPass_C takes the Shot's rule here; the build gives it the
// parent's ReceiveTick alone (no IgnorePlatformsCheck writes).
fn platforms(p: &mut Projectile) {
    base_platform(p);
    if p.class != ProjectileClass::BasketBallProjectile {
        shot_platform(p);
    }
}

fn shot_platform(p: &mut Projectile) {
    let ignoring = ball_of(p).ignore_platforms_check;
    if p.vy > 0.0 {
        if ignoring {
            return;
        }
        ball_of(p).ignore_platforms_check = true;
        set_platform_response(p, CollisionResponse::Ignore);
    } else if ignoring {
        ball_of(p).ignore_platforms_check = false;
        set_platform_response(p, CollisionResponse::Block);
    }
}

/// The begin of an overlap of its hit box (HitBox @6381 without 2D hit boxes, HitBox2D with them) and
/// HandleHitboxOverlap @8114, for the owner. No native filter keeps the owner or the instigator out: the box begins
/// to overlap his own hurt capsules as anyone's, and the graph's gate is all there is: CanBePickedUp and (Activated
/// and other == Owner, or other != Owner) @8134-8315. With other == Instigator @8345 the catch @6467: unless his
/// mesh's current montage is C016_Special_Dunk1_Montage @6696-6763, the BPI_C016 cast @6797, which fails on any pawn
/// but his (a Taz who burped the ball): pop_flow @6862, the event ends, the ball flies on; else
/// BPI_PlayBallRecieveAnimIfNotInAttack @6872, the broadcast BasketBallPickedUp @6929 (a perk's bind) and
/// SpawnBuffActor(the ball buff, him, him, 0, 0.0) @7028; then DestroySpawnedActor(true) @7112 (the Dunk1 branch goes
/// there directly, whoever the owner is).
/// The event is raised per pair of components: a hurt capsule of his that comes into the box raises one while another
/// of them has stayed inside since the spawn. A fighter's root capsule ignores the HitBox channel, so the hurt
/// capsules are the only ones. The pairs are kept (`Ball::inside`) and updated by every test of the fighter: behind
/// his own tick and behind the ball's move.
/// Any other BaseFighter @8402: of the Instigator's team, not him, not IsMinion @8477-8846 -> BasketBallPickedUp(him)
/// @8856, then @7144: SpawnBuffActor(Buff_IgnitedBasketballPass when the Instigator has Buff_C016_HotHands @7215 and
/// IsANoLookPass @7423, else Buff_BasketballPass; him, the Instigator, 0 stacks, 20.0 s) @7531 and
/// DestroySpawnedActor(true) @7112. A fighter of the other team (or a minion) passes the gate and stops there with
/// nothing done: its overlaps are not kept. The gate's Activated part is for the owner alone.
fn overlap(w: &mut World, id: ProjectileId, vic: FighterId) {
    // The World's buffer, taken out for the call (a nested call finds it empty and uses its own).
    let mut touched = std::mem::take(&mut w.scratch.touched);
    touched.clear();
    overlap_with(w, id, vic, &mut touched);
    w.scratch.touched = touched;
}

/// `overlap` with the buffer of the hurt capsules the box touches.
fn overlap_with(w: &mut World, id: ProjectileId, vic: FighterId, touched: &mut Vec<Real>) {
    let mid = w.overlap_mid_frame || pr(w, id).age == 0; // age 0: the test at the spawn place, inside the spawn call
    ball_mut(w, id).mid = mid;
    if pr(w, id).dead {
        return;
    }
    let Some(inst) = instigator(pr(w, id)) else { return };
    let mine = vic == inst;
    let (vteam, vminion, vdead) = {
        let v = w.fighter(vic);
        (v.team.team, v.life.is_minion, v.life.dead > 0.0)
    };
    if !mine && (vteam != w.fighter(inst).team.team || vminion) {
        return;
    }
    let vid = w.fighter(vic).id;
    // the pairs now: the hurt capsules of the fighter the box touches
    let (px, py) = (pr(w, id).x, pr(w, id).y);
    let (dx, dy) = (px - w.fighter(vic).body.x, py - w.fighter(vic).body.y);
    let far = !hit_collision(pr(w, id)) || vdead || dx > REACH || dx < -REACH || dy > REACH || dy < -REACH;
    // (the step's start pose at his root of now: the live read of the t49 catch, `anim::struck_boxes`)
    let pose = if far { None } else { struck_pose(w, vic) };
    if let Some(pose) = pose {
        let p = pr(w, id);
        let s =
            WorldSphere { x: p.x, y: p.y, z: p.sphere_3d.then_some(0.0), r: p.hit_radius, ..WorldSphere::default() };
        hurt_touched(w, vic, &s, &pose, touched);
        give_vbuf(w, pose);
    }
    let begun = {
        let b = ball_mut(w, id);
        let list: &mut Vec<Real> = if mine {
            &mut b.inside
        } else {
            let k = usize::try_from(vid).expect("a fighter id");
            if b.others.len() <= k {
                b.others.resize(k + 1, None);
            }
            b.others[k].get_or_insert_with(Vec::new)
        };
        let begun = touched.iter().any(|t| !list.contains(t));
        list.clone_from(touched);
        begun
    };
    if touched.is_empty() && far {
        return;
    }
    if !begun {
        return;
    }
    let (activated, gate) = {
        let p = pr(w, id);
        let b = ball(w, id);
        (b.hit_activated || since(p, act_age(), mid), since(p, PICK_AGE, mid))
    };
    if !gate || (Some(vic) == pr(w, id).owner && !activated) {
        return;
    }
    let a = ActorRef::Projectile(id);
    let arg = ActorBuffArg { by: vic, kx: 0.0, ky: 0.0 };
    if !mine {
        // @8856 (Buff_C016_KeepPossession's bind), the buff @7531, the destroy @7659 -> @7112.
        if pr(w, id).elem.is_some() {
            actor_buff_signal(w, a, ActorBuffSignal::BasketBallPickedUp, Some(&arg));
        }
        let hot = buff_of(w, inst, name!("Buff_C016_HotHands_C")).is_some() && ball(w, id).no_look;
        let cls = if hot { name!("Buff_IgnitedBasketballPass_C") } else { name!("Buff_BasketballPass_C") };
        spawn_buff(w, vic, cls, 20.0, Some(0), Some(inst), true, None);
        if !pr(w, id).dead {
            destroy_projectile(w, id, false);
        }
        return;
    }
    let in_dunk = w.fighter(vic).montages.montage.as_ref().map(|m| m.montage.name) == Some(DUNK);
    if !in_dunk {
        if lebron_of(w, vic).is_none() {
            return; // @6862
        }
        play_ball_receive_anim_if_not_in_attack(w, vic);
        // BasketBallPickedUp(him) @6929: the buffs that bound it (Buff_C016_KeepPossession).
        if pr(w, id).elem.is_some() {
            actor_buff_signal(w, a, ActorBuffSignal::BasketBallPickedUp, Some(&arg));
        }
        spawn_buff(w, vic, BALL, 0.0, Some(0), Some(vic), true, None);
    }
    destroy_projectile(w, id, false);
}

/// HitEnemyPlayer @5362 (no parent call), behind the hit: HitEnemy, PF_Post_VO_Event(the owner, ability_pass_hit, the
/// hit fighter as context, 0.1) @5393, the hit response. Bound after the HitEnemyPlayer of a granted buff
/// (Buff_C016_DistanceShot @1292, in the parent's BeginPlay; this one at HandleActorPostSpawn @8152): a ball that
/// buff destroyed (@1069) is PendingKill when the broadcast reaches this entry, and ProcessMulticastDelegate skips it
/// (the caller's test).
fn hit_enemy(w: &mut World, id: ProjectileId, vic: FighterId) {
    ball_mut(w, id).hit_enemy = true;
    if let Some(o) = pr(w, id).owner {
        crate::fighter::match_side::post_vo(w, o, name!("ability_pass_hit"), Some(vic), 0.1);
    }
    if !pr(w, id).dead {
        let mid = ball(w, id).mid;
        if hit_response(w, id, mid, !mid) {
            destroy_projectile(w, id, false);
        }
    }
}

/// Blocked @7800: the parent's Blocked(null) (with DestroyOnBlocked its Delay(0.0) and destroy: off in the class
/// data), then the hit response.
fn blocked(w: &mut World, id: ProjectileId, _by: ActorRef) {
    if class_p(w, pr(w, id)).get("DestroyOnBlocked").is_some_and(crate::registry::props::truthy) {
        pm(w, id).blocked_destroy = true;
    }
    if !pr(w, id).dead {
        let mid = ball(w, id).mid;
        if hit_response(w, id, mid, !mid) {
            destroy_projectile(w, id, false);
        }
    }
}

/// ReceiveEndPlay @4869, however it ended: sfx_cha_C016_ball_death at its place @4930 (the Pass's class data names the
/// same event as its 'Ak Death Location', which the parent's destroy posts beside this one).
fn end(w: &mut World, id: ProjectileId) {
    sound(w, id, name!("sfx_cha_C016_ball_death"));
}

/// BasketBallProjectile Reflected @4784 (the Shot and the Pass have no graph of their own): the parent's (empty),
/// K2_PauseTimerHandle(LifetimeTimer), Delay(0.375) (latent link 4814 -> 95), K2_UnPauseTimerHandle(LifetimeTimer).
/// Nothing writes LifetimeTimer: the handle is invalid and both calls do nothing.
fn reflected(_w: &mut World, _a: ActorRef) {}

// Deviation: the World's make keeps the request's copy (the spawn place on the copy's mesh, byFighter) for the ball as
// for every class.
fn make(_w: &World, _f: FighterId, _r: &SpawnRequest) -> ProjectileExt {
    ProjectileExt::Ball(Ball::default())
}

fn base_kind() -> ProjectileHooks {
    ProjectileHooks {
        make: Some(make),
        begin: Some(begin),
        step: Some(base_tick),
        bounce_impact: Some(base_bounce),
        overlap: Some(overlap),
        hit_enemy: Some(hit_enemy),
        blocked: Some(blocked),
        end: Some(end),
        reflect: ReflectHooks { reflected: Some(reflected), ..ReflectHooks::default() },
        ..ProjectileHooks::default()
    }
}

/// The Shot's graph over the parent's: its own tick and bounce handler in place of the parent's.
fn shot_kind() -> ProjectileHooks {
    ProjectileHooks { step: Some(shot_tick), bounce_impact: Some(shot_bounce), ..base_kind() }
}

// ---- Buff_BasketballPass and Buff_IgnitedBasketballPass
//
// The team mate who picked up a ball ([`overlap`]) holds the pass for 20 s (the caller's duration; BuffParentActor's
// life timer ends it). Class data: Hit Box Damage (damage 4, base 900, scaling 12, direction (0.1, 1), IgnoreFacing,
// ForceKnockback, HitEachActorOnlyOnce), DestroyAfterSpawn? (true; the Ignited child false), BreakBuffs the other one.
// ReceiveBeginPlay @1399: the parent's, InitialExplosion @1409, then the owner's Comp_Pawn_BranchManager
// IsAttackingStart bound to ThrowBall @1494 / @1517. BPI After Status Update Multicast (BuffApplied @2919 -> SpawnVFX
// @2934, BuffRemoved -> RemoveVFX @2959): the drawing (the orbiting ball, PS_C016Basketball_Recall, the "Pass
// Received" pop text) is the render's; the voice line of SpawnPopupText is `pass_begin` here.

fn pass_state(w: &mut World, b: BuffRef) -> Option<&mut PassState> {
    match &mut crate::registry::props::buff_mut(w, b)?.ext {
        BuffExt::BasketballPass(e) => Some(e),
        _ => None,
    }
}

/// InitialExplosion @3133: ExplosionVisuals @3152 (SpawnEmitterAtLocation(PS_C016_PERK_Explosion, the owner's
/// location, scale 1.65) @3222, its Team the owner's BPI_GetColorValue @3390-3487; then sfx_cha_C016_ball_explode at
/// the owner's location @85), then @2974 SphereOverlapActors(the buff's location, 300, [3, 2, 8, 11, 16] @3002,
/// BP_FighterCharacter, no ignore) @3034 and per result @164-1162: valid, with a Comp_Actor_Destructible, of another
/// team than the owner @597-679 -> Hit Box Damage.ForceKnockback = true @952 and the buff's own
/// Comp_Pawn_HitBoxHandler.pfgDoDamage(null, the result, Hit Box Damage, no buffs) @1039 (a DealerActor credited to
/// the Instigator). No authority or invulnerability test of its own (the hit's own dodge path).
// Unverified: the buff actor's location is taken as its owner's (actor location). Unverified: SphereOverlapActors'
// order is the fighters' order (traces).
fn initial_explosion(w: &mut World, f: FighterId, b: BuffRef, cls: Name) {
    let (fx, fy, fid) = (w.fighter(f).body.x, actor_y(w, f), w.fighter(f).id);
    let vfx = Event::ProjectileDeath {
        cls,
        fighter: fid,
        vfx: Some(name!("PS_C016_PERK_Explosion")),
        x: fx,
        y: fy,
        ox: 0.0,
        rot: [0.0; 3],
        scale: 1.65,
        tint: None,
        to: None,
    };
    let snd = Event::Sound {
        fighter: fid,
        ev: name!("sfx_cha_C016_ball_explode"),
        x: Some(fx),
        y: Some(fy),
        hit: None,
        pooled: false,
        key: None,
    };
    w.fighter_mut(f).events.push(vfx);
    w.fighter_mut(f).events.push(snd);
    let mut packet = inst_view(w, b)
        .obj("Hit Box Damage")
        .map_or_else(HitPacket::default, |p| HitPacket::from_props(&w.names, p, None));
    packet.force_knockback = Some(true);
    let types = [obj_type::PHYSICS_BODY, obj_type::PAWN, obj_type::DAMAGE_DESTRUCTIBLE]
        .into_iter()
        .chain([obj_type::PROJECTILE_DESTRUCTIBLE, obj_type::ITEM_DESTRUCTIBLE])
        .map(crate::real)
        .collect::<Vec<_>>();
    let found = sphere_overlap_actors(w, fx, fy, 300.0, &types, &[], -1.0);
    for a in found {
        let ActorRef::Fighter(vic) = a else { continue };
        if !is_match_fighter(w, vic) || w.fighter(vic).team.team == w.fighter(f).team.team {
            continue;
        }
        if crate::combat::phases::no_hit_by(w, f, vic) {
            continue;
        }
        let attack_id = packet.attack_id.unwrap_or(0);
        if invulnerable(w, vic) {
            // AttackingActor: the buff actor
            let tick = w.flow.tick;
            attack_dodged(
                w,
                ActorRef::Buff(b),
                vic,
                attack_id,
                packet.base_knock_back.unwrap_or(0.0),
                tick,
                None,
                false,
            );
            continue;
        }
        dodge_over(w, ActorRef::Buff(b), vic);
        let mut d = dealer(w, ActorRef::Buff(b));
        d.attack_id = attack_id;
        // HitLocation of a null HitCollision: the victim's capsule. The packet's IgnoreFacing X is UE +Y: world -x.
        let k = packet.knock_back_direction_vector.unwrap_or_default();
        let at = V2 { x: w.fighter(vic).body.x, y: actor_y(w, vic) };
        let dir = V2 { x: -k.x.unwrap_or(0.0), y: k.y.unwrap_or(0.0) };
        apply_projectile_hit(w, &d, vic, &packet, &[], at, Some(dir), &[], 0.0);
    }
    // the handler of this call is gone with it
    if let Some(e) = pass_state(w, b) {
        e.dodge = None;
    }
}

/// ThrowBall @1559 (the owner's IsAttackingStart): an owner that is BP_FighterCharacter and not BPI_IsEvading @1753;
/// BPI_Get8WayDirection(true) @1917 (the stick against the facing): Up / FwdUp -> (0, 1, 1), Down / FwdDown ->
/// (0, 1, -1), any other (0, 1, 0) @2231-2525; the owner's Comp_Spawner CompSpawnActor(BasketBallProjectile_AllyPass_C,
/// not attached, socket Snout for EFighter 6 (ReinDog: 'creature') else Spine_Socket, not a world transform, the
/// relative identity, that direction, speeds 2600 / 2600, AttackID 0 (the owner's attack count), AssignInstigator the
/// buff's Instigator) @2634; DestroyAfterSpawn? -> K2_DestroyActor @2838. The spawn is the owner's spawn request (the
/// world's spawn pass of this frame, as a notify's). True: destroyed.
/// The ball's charge is the Instigator's (AllyPass ReceiveChargeValuesFromInstigator; SpawnedActorParent
/// GetChargeValuesFromInstigator @11459-11695 reads GetInstigator's Comp_Pawn_ChargeAttack: LeBron, not the mate).
// Unverified: no trace reaches a team mate holding the pass (a 2v2); the spawn request (literal properties and an
// Instigator) is written from the decode and has not run.
fn throw_ball(w: &mut World, f: FighterId, b: BuffRef) -> bool {
    if is_evading(w, f) {
        return false;
    }
    let (ix, iy, facing) = {
        let fi = w.fighter(f);
        (fi.controls.inp.x, fi.controls.inp.y, fi.body.facing_dir)
    };
    let dir = get8_way(ix, iy, facing);
    let z = match dir {
        Dir::Up | Dir::FwdUp => 1.0,
        Dir::Down | Dir::FwdDown => -1.0,
        _ => 0.0,
    };
    let socket = if w.fighter(f).ch == name!("creature") { "Snout" } else { "Spine_Socket" };
    let mut d = Props::new();
    d.insert("X".into(), Value::Num(0.0));
    d.insert("Y".into(), Value::Num(1.0));
    d.insert("Z".into(), Value::Num(z));
    let mut p = Props::new();
    p.insert("Socket".into(), Value::Str(socket.into()));
    p.insert("InitialMovementDirection".into(), Value::Obj(d));
    p.insert("InitialSpeed".into(), Value::Num(2600.0));
    p.insert("MaxSpeed".into(), Value::Num(2600.0));
    let from = match crate::registry::props::buff(w, b).map(|x| &x.ext) {
        Some(BuffExt::BasketballPass(e)) => e.from,
        _ => None,
    };
    let charge = instigator_charge(w, from.unwrap_or(f));
    let r = SpawnRequest {
        cls: name!("BasketBallProjectile_AllyPass_C"),
        n: NotifyRef { montage: Name::NONE, index: 0 },
        attack_id: w.fighter(f).decay.attack_count,
        foreign: Foreign::None,
        copy: None,
        charge,
        aim: None,
        props: Some(Arc::new(p)),
        instigator: from,
        origin: None,
    };
    w.fighter_mut(f).requests.spawn_requests.push(r);
    inst_view(w, b).get("DestroyAfterSpawn?").is_some_and(crate::registry::props::truthy)
}

/// ReceiveBeginPlay (InitialExplosion), then the state manager's BuffApplied status update: SpawnVFX @0-166 away from
/// a dedicated server, for an owner whose Instigator (the pawn itself) IsLocallyControlled, SpawnPopupText, whose
/// PF_Post_VO_Event(GetInstigator (LeBron), ability_pass_hit, the owner as context, 1.0) @640 is posted here (the pop
/// text is the render's). No Instigator: Lib_Audio posts nothing.
// Unverified: IsLocallyControlled is taken as true for every pawn, as offline (a bot's AI controller is local,
// APlayerController::IsLocalController); online the build posts the line on the team mate's own machine alone.
fn pass_begin(w: &mut World, f: FighterId, b: BuffRef, from: Option<FighterId>, _duration: Option<Real>) {
    let Some(x) = crate::registry::props::buff_mut(w, b) else { return };
    let cls = x.cls;
    x.ext = BuffExt::BasketballPass(PassState { from, by: Some(from.unwrap_or(f)), dodge: None });
    initial_explosion(w, f, b, cls);
    if let Some(from) = from {
        crate::fighter::match_side::post_vo(w, from, name!("ability_pass_hit"), Some(f), 1.0);
    }
}

fn pass_on(w: &mut World, f: FighterId, b: BuffRef, s: BuffSignal, _a: &BuffSignalArg) -> bool {
    s == BuffSignal::IsAttackingStart && throw_ball(w, f, b)
}

/// Buff_IgnitedBasketballPass: the parent's BeginPlay (above), then (authority) the owner's Comp_Spawner
/// AfterSpawnedActor bound to SpawnedActor @54 / @148. SpawnedActor @194: a BasketBallProjectile_C or a child of it
/// (ClassIsChildOf @223 or IsSoftRef @261) gets SpawnBuffActor(Buff_Elemental_Fire, the actor, its Instigator, 5
/// stacks, 0.0) @551, the new buff's DestroyAfterApplied = false @714, then K2_DestroyActor @747 (whatever the spawn
/// gave). Its ThrowBall is the parent's with DestroyAfterSpawn? false: the spawn's AfterSpawnedActor ends it.
fn ignited_on(w: &mut World, f: FighterId, b: BuffRef, s: BuffSignal, a: &BuffSignalArg) -> bool {
    if s == BuffSignal::IsAttackingStart {
        return throw_ball(w, f, b);
    }
    if s != BuffSignal::AfterSpawnedActor {
        return false;
    }
    let Some(actor) = a.actor else { return false };
    let Some(cls) = actor_cls(w, actor) else { return false };
    if actor_is_a(w, ActorRef::Fighter(f), cls, name!("BasketBallProjectile_C")) != Some(true) {
        return false;
    }
    if let Some(serial) = elem_spawn(w, actor, BuffClass::BuffElementalFire, 5) {
        set_elem_ext(w, actor, serial, ElemExt::DestroyAfterApplied(false));
    }
    true
}

/// This file's registry entries (the four ball classes, the two pass buffs). foreignOwner (Taz's burp: a ball he ate
/// is spawned again with him as its owner, and as its Instigator: the burp passes none and Comp_Spawner @9238 takes
/// the owner then): the two graphs read of the owner and the Instigator only what any fighter answers or what fails
/// on a Taz in the build as here: the owner's RemoveBuff(the ball buff) @1776 (he has none), his mesh's montage for
/// IsANoLookPass @1371, the team colour, the VO, the Destructible TeamIndex and GetAllies; the BPI_C016 casts on the
/// Instigator (@2656 / @3979 / @4162; the catch's @6797) fail on a Taz (`lebron_of`). So he never catches the ball:
/// outside Dunk1 the failed cast @6862 ends the catch with no buff and no destroy, and it flies on through him. The
/// parent class is spawned by nothing in the build (DashToBasketball and the buffs only search or cast for it): its
/// entry is the graph BasketBallProjectile_AllyPass_C runs.
pub fn register(r: &mut Registry) -> Result<(), RegistryError> {
    let enter = |r: &mut Registry, cls: &'static str, class: ProjectileClass, k: ProjectileHooks, foreign: bool| {
        let mut e = ActorEntry::new(ActorClass::Projectile(class), ActorHooks::Projectile(k));
        e.foreign_owner = foreign;
        r.actors.enter(cls, e)
    };
    enter(r, "BasketBallProjectile_C", ProjectileClass::BasketBallProjectile, base_kind(), false)?;
    enter(r, "BasketBallProjectile_Shot_C", ProjectileClass::BasketBallProjectileShot, shot_kind(), true)?;
    enter(r, "BasketBallProjectile_Pass_C", ProjectileClass::BasketBallProjectilePass, shot_kind(), true)?;
    enter(r, ALLY_PASS, ProjectileClass::BasketBallProjectileAllyPass, base_kind(), true)?;
    let pass = BuffHooks { begin: Some(pass_begin), on: Some(pass_on), ..BuffHooks::default() };
    r.buffs.enter(PASS, BuffEntry::new(BuffClass::BuffBasketballPass, pass))?;
    let ignited = BuffHooks { begin: Some(pass_begin), on: Some(ignited_on), ..BuffHooks::default() };
    r.buffs.enter(IGNITED_PASS, BuffEntry::new(BuffClass::BuffIgnitedBasketballPass, ignited))
}
