//! Velma's gadgets: VelmaVialAttack ([`VelmaVial`]) and the SlowtoStunGrenade
//! it drops ([`SlowToStunGrenade`]: a hazard box on the floor) with Buff_SlowIntoStun ([`SlowExt`]: Buff_Ice while in
//! the box), VelmaBookProjectile ([`VelmaBook`]: homing on an ally; its writeHoming / homeAccel overrides are
//! `actors::projectile`'s arms), Velma_SPAIR_CameraFlash and Velma_AirSideSp_BookBlast ([`VelmaBurst`]), and
//! Velma_SassBubblev2 ([`VelmaSassBubble`], LeBron's sass bubble's sibling: `chars::lebron::bubble`'s box hits; its
//! ActivateObject override is `actors::projectile`'s arm).
//!
//! No pawn hook and no pawn state: VelmaGadgetState stays empty.
//!
//! VelmaVialAttack. Spawned by Velma_Special_Vial_Montage (L_Hand + (0, 0, -500), speed 0). Its Capsule simulates
//! physics. ReceiveBeginPlay @905: the parent's, then (authority) @494 SetPhysicsLinearVelocity((0,
//! RandomFloatInRange(-200, 200), 800)) @609 and SetPhysicsAngularVelocityInDegrees((RandomFloatInRange(-200, 200), 0,
//! 0)) @767 (both drawn, in that order), Init Visuals @836 (@920: sfx_cha_shaggy_scooby_snack_spawn posted on the owner
//! fighter @1005), then Delay(0.0) @850 -> @15: the Instigator cast to BaseFighter @35, its Comp_Spawner.CompSpawnActor
//! (SlowtoStunGrenade, at the owner's location - (0, 0, 100) with the owner's rotation) @339, then K2_DestroyActor
//! @479. The Delay(0.0) resumes in the actor's tick of its spawn frame (a Delay counts the spawn frame:
//! `latent_frames(0)` is 1), which comes ahead of the physics scene of that frame: the vial is destroyed before its
//! body ever steps, so the capsule body is never built here.
//! Deviation: the vial's velocity and spin are kept for the view (it lives one frame) and the two draws are taken for
//! the stream; that the actor tick of its spawn frame comes ahead of the physics step is the port's tick order (the
//! Delay rule), not read in the exe for this actor.
//! Unverified: the world Y of the velocity is taken as the sim's x with its sign.
//!
//! SlowtoStunGrenade. The vial's spawn: CompSpawnActor at (the owner's location - (0, 0, 100)); its root Capsule is
//! NoCollision. MaxLifetime 6 (MaxLifetimeTimer -> DestroySpawnedActor); Die With Instigator. The Box (extent 320, 320,
//! 110 at relative Z -50; Overlap to Pawn alone) holds the slow. ReceiveBeginPlay @1967: the parent's; Visuals @1258:
//! sfx_cha_velma_vial_loop on the actor @1294 (the rest is the view's). Then (attach) BoxTraceSingle(location - (0, 0,
//! 50) -> location - (0, 0, 150), half (100, 100, 0), ProjectileGroundTrace, no complex, ignoring self) @2259,
//! K2_AttachToActor(the hit's HitActor, KeepWorld x3, weld) @2584 (the grenade stays where it is and rides that actor
//! from then on; no hit: no attach), and @138 (authority) a HitActor that is a DestructibleObjectBP binds
//! DestroyIfPlatformDestroyed to it @406-429 (@4049 DestroySpawnedActor(false)).
//! The ride: the hit body's own move of the frame (the trace hit's floor), added at the head of the grenade's step.
//! Deviation: the ride is the floor's move of the frame added in the grenade's own step; the build's weld moves the
//! actor inside its parent's move. A HitActor with no floor of its own in the port is warned and not ridden.
//! Unverified: the delegate bound @406 is taken as DestructibleObjectBP's ObjectDestroyed (the class's bytecode is not
//! at hand; as FartBomb_SpawnedActor's GroundDestroyed); the attach parent's own destroy is taken as the stock detach
//! with KeepWorld: the grenade stays where it is.
//! Box begin overlap @3789 (authority): a BP_FighterCharacter with a Comp_Actor_Destructible, of another team than the
//! Instigator's @2884, that has no Buff_SlowIntoStun @3055: its SpawnBuffActor(Buff_SlowIntoStun, it, the Instigator,
//! 1, K2_GetTimerRemainingTimeHandle(MaxLifetimeTimer)) @3418 and, on a buff, its AssignInstigatingTrapServer(self)
//! @3581 (Trap). Box end overlap @3669: still overlapping the actor (another of its bodies) -> nothing; else Remove
//! Buff (@0-498): the actor's Buff_SlowIntoStun whose Instigator is the grenade's is destroyed. ReceiveEndPlay @2628 ->
//! @1711: Remove Buff for each fighter the Box overlaps (@789-1253); sfx_cha_velma_vial_loop_stop on the actor @1785,
//! sfx_cha_velma_vial_end at its location @1878.
//! Deviation: the overlaps are tested once a tick in the grenade's step against the fighters' root capsules (the Pawn
//! bodies; the Box's response to the HurtBox object type is not in its template and is taken as no overlap), as the
//! smoke bomb's zone: a pass through inside one tick is missed, and a begin is one tick late against an overlap that a
//! fighter's move made earlier in the frame.
//! The timer's remaining time (FTimerManager 4.26: a timer set inside the tick groups is pending until the end of that
//! frame's timer pass): MaxLifetime at the spawn frame's step, then MaxLifetime - (age - 1) x dt; the timer ends the
//! actor in the pass of the frame `timer_ticks(MaxLifetime)` after its spawn.
//! Unverified: the Box is taken as what the environmental hazard's Sphere overlaps (its object type is not in the
//! template: Custom); BPI Clean is the parent's: Blocked(null) -> DestroyOnBlocked -> Delay(0.0) -> DestroySpawnedActor.
//!
//! Buff_SlowIntoStun (BuffDuration 5, MaxStacks 1, SpeedMod 0.4: the parent's speed mod; ApplyIceTime 0.25).
//! ReceiveBeginPlay @1360: the parent's; an owner that is no BP_FighterCharacter -> K2_DestroyActor @1101 (every port
//! owner is one); else K2_SetTimerDelegate(ApplyIce, ApplyIceTime, looping) @1167. ApplyIce @10: Trap invalid @39, or
//! Trap.Box not overlapping the owner @73 -> K2_DestroyActor @1038; else with a Comp_Actor_Destructible not
//! IsInvulnerable? @345 and a Comp_State_Manager: a Buff_Ice at MaxStacks or more @762 -> nothing; else
//! SpawnBuffActor(Buff_Ice, the owner, the Instigator, 1, 0.0) @949. ReceiveEndPlay @1057: the timer cleared. The
//! timer: set inside the spawn (the grenade's step), pending until the end of that frame's timer pass; the looping
//! re-arm is ExpireTime += Rate (Buff_Fire's rule): the n-th call in the pass of the frame `timer_ticks(n x 0.25)`
//! after the spawn.
//! Unverified: Trap.Box.IsOverlappingActor is the grenade's `box_over` test at the time of the call (the root
//! capsule). Its IsInvulnerable? is the fighter's `invulnerable` (Comp_Actor_Destructible's own flag, read as the
//! port's invulnerability, as the other callers do).
//!
//! VelmaBookProjectile. Spawned by Velma_Book_ThrowF / AirBook_ThrowF (R_Hand + (0, 250, 0), speed 2000). The parent's
//! movement, life (MaxLifetime 5.5), HitBox2D (radius 100), DestroyOnHitFighter, DestroyOnHitProjectile,
//! DestroyOnCollideWithEnvironment, clank and reflect are the world's Projectile's. ReceiveBeginPlay @1172: the
//! parent's; with the owner's Comp_Actor_Destructible valid @1296: sfx_cha_velma_throw_book_loop on the actor @1306;
//! GetAllies @1442, and with one or more CheckForHomingTarget @1594 (@3392-2872: for every ally, a valid
//! BP_FighterCharacter -> HomingTargetComponent = its root, no break: the last ally). GetAllies: the owner's team, the
//! owner and a fighter waiting for its respawn left out, as the tether's CheckForHomingTarget.
//! HitEnemyPlayer @2127: PF_Post_VO_Event(Instigator, ability_generic, the hit player, 0.1), then the parent's.
//! HitAlly @3644: the parent's (nothing): no hook. The On Hit Buffs rows (Buff_Educated 7 s, Buff_DecrementEvadeCount
//! 8 s: Self, Hit and Ally, not Enemy) for a team mate the book's HitBox2D meets: pfgDoDamage's entry rows (both to the
//! Instigator) and its ally rows (both to the ally), the world's ally test (`combat::phases` projectile_ally_test).
//! On an enemy the world's hit makes the entry rows (the Instigator's).
//! ReceiveEndPlay @3880 -> @15: sfx_cha_velma_throw_book_loop_stop on the actor. Not reached: SetHomingMultiCast @3846
//! (called by VelmamSassFollowUpProjectile, mark.rs's) and MulticastProjectile @3677 (no caller).
//!
//! Velma_SPAIR_CameraFlash / Velma_AirSideSp_BookBlast. Spawned at L_Hand by Velma_Special_BookClose_Downward /
//! Velma_Special_BookBlast_Backward (speed 0; no gravity: the actor stays where it spawned); the notify's End destroys
//! every actor of the class. HitBoxOn? false; the Sphere (radius 200 / 275, tag HitBox) deals the class's HitBoxDamage
//! through the parent's handler. No HitBox2D: the sphere is tested in 3D. No HurtBox: nothing hits it.
//! ReceiveBeginPlay @701 -> @41: the parent's; SpawnEmitterAtLocation(PS_Velma_CameraFlash / none for the blast, the
//! actor's location) @144; Delay(0.07) @630 -> @15: HitBoxOn? = true and UpdateHitbox.
//! Unverified: the Delay counts the spawn frame (the notify's spawn comes ahead of the projectile pass).
//!
//! Velma_SassBubblev2. LeBron's C016_SassBubble's sibling (`chars::lebron::bubble`): the same Box template (extent 32,
//! relative scale (0.9, 0.5, 0.5), tag HitBox) grown by the same ReceiveTick @143-949 (GrowthValue 0.9), attached to
//! the owner's Root at (0, 0, 150), scale 5, speed 0.01; Die With Instigator and the notify's End. Unlike C016's its
//! HitBox sphere (radius 32) keeps the default bGenerateOverlapEvents: it deals the hit too while HitBoxOn? holds (a
//! sphere of 32 x 5 in 3D). ActivateObject @1089 (no parent call; authority, no once-gate) -> @950: Activated;
//! SetVisuals(Slam true) @961: PS_Velma_NAD_Calculator_Weak at scale 1 while VSizeXY(GrowthScale) < 10, else
//! PS_Velma_NAD_Calculator at (1.5, 1.25, 1.25) (Deviation: the event carries one scale, 1.5); Slam -> FX_Blast @2108:
//! sfx_cha_wonderwoman_charge_bracer_blast_01 while VSizeXY(GrowthScale) < 13.472, else _02, at the actor. Then HitBoxOn?
//! = true @976, UpdateHitbox @987 (the Box as it has grown, then at HitBoxScale (1, 1, 1): `box_hits`; the sphere: the
//! world's next projectile test, the same frame), Box.K2_DestroyComponent @1001, Delay(0.16) @1034 -> @30: HitBoxOn? =
//! false, UpdateHitbox.
//! Unverified: which of the two components HitBoxesOn turns on first is not read; with HitEachActorOnlyOnce a fighter
//! both meet is hit once, by the Box here.
//! The rows: Buff_ArmorPack 1 s to an ally (Hit and Ally), Buff_SpawnEvidence to an enemy. An ally under the Box gets
//! its row in `box_hits`; under the sphere in the world's ally test (the class has no HitAlly of its own).
//! Unverified: the actor's delta is taken as the world's (the growth tick).

use crate::Real;
use crate::State;
use crate::actors::homing::home_toward;
use crate::actors::projectile::{BuffHome, ProjectileExt, class_def, class_p, set_hitbox};
use crate::actors::world_actor::actor_p;
use crate::actors::{
    ActorClass, ActorId, ActorRef, Projectile, ProjectileClass, ProjectileId, WorldActor, WorldActorClass,
    WorldActorState,
};
use crate::chars::lebron::bubble::{Rect, box_hits};
use crate::combat::boxes::WorldSphere;
use crate::combat::buffs::{BuffExt, buff, buff_mut, buff_of, end_buff, inst_view, spawn_buff};
use crate::combat::elem::instigator_charge;
use crate::combat::hit::knockback::timer_ticks;
use crate::combat::{BuffClass, BuffRef};
use crate::consts::DELTA_SECONDS;
use crate::events::{Event, SoundKey};
use crate::fighter::body::{PlatformRef, plat};
use crate::fighter::common::{actor_data, buff_def, latent_frames, peers, warn_text};
use crate::fighter::match_side::{SpawnRequest, post_vo};
use crate::fighter::moves::V2;
use crate::fighter::traces::{TRACE_PROJECTILE_GROUND, box_trace_single};
use crate::fighter::{FighterId, actor_y, cap_radius, capsule_half, invulnerable, no_collision};
use crate::math::ue::size2d;
use crate::math::{max, to_int32};
use crate::name;
use crate::names::Name;
use crate::registry::entries::{ActorHooks, BuffHooks, HazardHooks, ProjectileHooks, SizeHooks, WorldActorHooks};
use crate::registry::props::{flag, num, num_or, obj};
use crate::registry::{ActorEntry, BuffEntry, Registry, RegistryError};
use crate::world::World;
use crate::world::spawn_spot::spawn_place;
use crate::world::spawning::{comp_spawn_actor, new_world_actor};
use crate::world::wall_bp::{WallActor, bind_ground_destroyed, wall_delegate_fns};

pub const VIAL: &str = "VelmaVialAttack_C";
pub const SLOW_TO_STUN: &str = "SlowtoStunGrenade_C";
pub const SLOW_INTO_STUN: &str = "Buff_SlowIntoStun_C";
pub const BOOK: &str = "VelmaBookProjectile_C";
pub const BOOK_BLAST: &str = "Velma_AirSideSp_BookBlast_C";
pub const CAMERA_FLASH: &str = "Velma_SPAIR_CameraFlash_C";
pub const SASS_BUBBLE: &str = "Velma_SassBubblev2_C";
const SLOW_N: Name = name!("Buff_SlowIntoStun_C");
const ICE_N: Name = name!("Buff_Ice_C");
const GRENADE_N: Name = name!("SlowtoStunGrenade_C");
const DESTROY_IF_PLATFORM_DESTROYED: &str = "SlowtoStunGrenade_C.DestroyIfPlatformDestroyed";

const BOX_HX: Real = 320.0;
const BOX_HZ: Real = 110.0;
const BOX_DZ: Real = -50.0;

/// The Sass bubble's Box, as chars/lebron/bubble.rs's: extent 32, relative scale (0.9, 0.5, 0.5), 50 forward a tick.
const BOX_EXTENT: Real = 32.0;
const BOX_SCALE_X: Real = 0.9;
const BOX_SCALE_YZ: Real = 0.5;
const BOX_FORWARD: Real = 50.0;

/// `VelmaVial` (VelmaVialAttack_C): its place and the Capsule's linear velocity (Y, Z of the build) and spin.
#[derive(State, Debug, Default)]
pub struct VelmaVial {
    pub x: Real,
    pub y: Real,
    pub vx: Real,
    pub vy: Real,
    /// Its angular velocity about X, degrees per second.
    pub spin: Real,
}

/// `SlowToStunGrenade` (SlowtoStunGrenade_C).
#[derive(State, Debug, Default)]
pub struct SlowToStunGrenade {
    pub x: Real,
    pub y: Real,
    /// Steps taken (0 in the spawn frame's).
    pub age: i32,
    /// The age of the step whose frame's timer pass the MaxLifetimeTimer ends it in (infinity for no life).
    pub life_at: Real,
    /// The fighters (ids) the Box overlaps.
    pub inside: Vec<i32>,
    /// The attach parent's floor it rides.
    pub floor: Option<PlatformRef>,
    /// BPI Clean's Blocked: its Delay(0.0) is pending, taken at the head of the next step.
    pub blocked_destroy: bool,
}

/// Buff_SlowIntoStun's extension (`SlowExt`): the grenade that slows it (`trap`), the Instigator (`from`), the tick of
/// the timer's set (`start`), the calls so far (`n`) and the timer's rate.
#[derive(State, Debug, Default)]
pub struct SlowExt {
    pub trap: Option<ActorRef>,
    pub from: Option<FighterId>,
    pub start: Real,
    pub n: Real,
    pub rate: Real,
}

/// `VelmaBook` (VelmaBookProjectile_C): ProjectileMovement.HomingTargetComponent's actor (its root) and
/// HomingAccelerationMagnitude.
#[derive(State, Debug, Default)]
pub struct VelmaBook {
    pub home_to: Option<FighterId>,
    pub home_mag: Real,
}

/// `VelmaBurst` (Velma_SPAIR_CameraFlash_C, Velma_AirSideSp_BookBlast_C): the age the Delay(0.07) turns the hit box on
/// at.
#[derive(State, Debug)]
pub struct VelmaBurst {
    pub on_at: i32,
}

impl Default for VelmaBurst {
    fn default() -> Self {
        Self { on_at: -1 }
    }
}

/// `VelmaSassBubble` (Velma_SassBubblev2_C).
#[derive(State, Debug)]
pub struct VelmaSassBubble {
    /// The Box's world scale along the owner's forward, across the stage's depth and up, grown each tick.
    pub box_x: Real,
    pub box_y: Real,
    pub box_z: Real,
    /// Its relative X (f x 50, set by each growing tick).
    pub box_fwd: Real,
    /// VSizeXY(GrowthScale).
    pub growth_len: Real,
    /// ParticleSystem1's world scale (the bubble drawn).
    pub ps_scale: Real,
    /// Box.K2_DestroyComponent @1001.
    pub box_gone: bool,
    /// The age the Delay(0.16) turns the hit box off at.
    pub off_at: i32,
}

impl Default for VelmaSassBubble {
    fn default() -> Self {
        Self {
            box_x: 0.0,
            box_y: 0.0,
            box_z: 0.0,
            box_fwd: 0.0,
            growth_len: 0.0,
            ps_scale: 0.0,
            box_gone: false,
            off_at: -1,
        }
    }
}

/// The vial's place (its `x`, `y`).
pub fn vial_place(w: &World, a: &WorldActor, s: &VelmaVial) -> Option<(Real, Real)> {
    let _ = (w, a);
    Some((s.x, s.y))
}

/// The grenade's place (its `x`, `y`).
pub fn grenade_place(w: &World, a: &WorldActor, s: &SlowToStunGrenade) -> Option<(Real, Real)> {
    let _ = (w, a);
    Some((s.x, s.y))
}

/// The book's writeHoming override, behind the parent's: off drops its target (`VelmaBook.writeHoming`).
pub fn book_write_homing(p: &mut Projectile, on: bool) {
    if let ProjectileExt::VelmaBook(b) = &mut p.ext
        && !on
    {
        b.home_to = None;
    }
}

/// The book's homeAccel override: toward its ally's capsule (`VelmaBook.homeAccel`), as the shared homing gives it: a
/// knocked-out player's pawn is not destroyed and stays a valid target where it was knocked out, until its respawn
/// moves it; a destroyed minion is no target.
pub fn book_home_accel(w: &World, p: &Projectile) -> Option<V2> {
    let ProjectileExt::VelmaBook(b) = &p.ext else { return None };
    let t = b.home_to?;
    if b.home_mag <= 0.0 {
        return None;
    }
    home_toward(w, p.x, p.y, &BuffHome { to: ActorRef::Fighter(t), mag: b.home_mag })
}

/// The sass bubble's ActivateObject override (no parent call): Activated alone (`VelmaSassBubble.activate`).
pub fn bubble_activate(p: &mut Projectile) {
    p.activated = true;
}

// ---- VelmaVialAttack

/// `rangeF`: RandomFloatInRange(lo, hi), one draw of the world's stream: Min + (Max - Min) x FRand, float32.
fn range_f(w: &mut World, lo: Real, hi: Real) -> Real {
    let r = w.random_float();
    lo + ((hi - lo) * r)
}

fn vial_spawn(w: &mut World, f: FighterId, r: &SpawnRequest) -> (WorldActorState, bool) {
    let np = crate::world::spawning::request_props(w, f, r);
    let at = spawn_place(w, f, &np);
    // (0, Y, 800) in world space, not turned by the facing.
    let vx = range_f(w, -200.0, 200.0);
    let spin = range_f(w, -200.0, 200.0);
    let fighter = w.fighter(f).id;
    w.events.push(Event::Sound {
        fighter,
        ev: name!("sfx_cha_shaggy_scooby_snack_spawn"),
        x: None,
        y: None,
        hit: None,
        pooled: false,
        key: None,
    });
    (WorldActorState::VelmaVial(VelmaVial { x: at.x, y: at.y, vx, vy: 800.0, spin }), false)
}

fn vial_step(w: &mut World, a: ActorId) {
    // Delay(0.0) -> @15: GetInstigator is the owner (Comp_Spawner's spawn): a fighter.
    let f = w.actors.actors.get(a).expect("a live world actor").owner;
    let (x, y) = (w.fighter(f).body.x, actor_y(w, f) - 100.0);
    spawn_grenade(w, f, x, y);
    if let Some(v) = w.actors.actors.get_mut(a) {
        v.dead = true;
    }
}

/// The vial's hurtBox: none (Comp_Actor_Destructible IsProjectile, no HurtBox component).
fn vial_hurt_box(_w: &World, _a: ActorId) -> Option<WorldSphere> {
    None
}

// ---- SlowtoStunGrenade

fn gr(w: &World, id: ActorId) -> &SlowToStunGrenade {
    match &w.actors.actors.get(id).expect("a live world actor").state {
        WorldActorState::SlowToStunGrenade(s) => s,
        _ => panic!("a SlowtoStunGrenade_C state"),
    }
}

fn gr_mut(w: &mut World, id: ActorId) -> &mut SlowToStunGrenade {
    match &mut w.actors.actors.get_mut(id).expect("a live world actor").state {
        WorldActorState::SlowToStunGrenade(s) => s,
        _ => panic!("a SlowtoStunGrenade_C state"),
    }
}

fn gr_owner(w: &World, id: ActorId) -> FighterId {
    w.actors.actors.get(id).expect("a live world actor").owner
}

fn gr_dead(w: &World, id: ActorId) -> bool {
    w.actors.actors.get(id).is_none_or(|a| a.dead)
}

fn gr_kill(w: &mut World, id: ActorId) {
    if let Some(a) = w.actors.actors.get_mut(id) {
        a.dead = true;
    }
}

fn grenade_key(w: &World, id: ActorId) -> SoundKey {
    SoundKey { tag: name!("velmaVial"), n: w.actors.actors.get(id).expect("a live world actor").seq }
}

/// K2_GetTimerRemainingTimeHandle(MaxLifetimeTimer), float.
pub fn remaining(w: &World, id: ActorId) -> Real {
    let life = num_or(actor_p(w, ActorRef::Actor(id)), "MaxLifetime", 0.0);
    let age = gr(w, id).age;
    if age == 0 { life } else { life - crate::real(age - 1) * DELTA_SECONDS }
}

/// The vial's spawn (`spawnGrenade`): the owner's CompSpawnActor.
pub fn spawn_grenade(w: &mut World, f: FighterId, x: Real, y: Real) {
    let data = w.data.clone();
    let Some(d) = actor_data(w, &data, ActorRef::Fighter(f), GRENADE_N) else {
        warn_text(w, &format!("spawned actor not exported ({SLOW_TO_STUN})"));
        return;
    };
    let life = num_or(&d.p, "MaxLifetime", 0.0);
    let s = SlowToStunGrenade {
        x,
        y,
        life_at: if life > 0.0 { timer_ticks(Some(life)) } else { Real::INFINITY },
        ..SlowToStunGrenade::default()
    };
    let a = new_world_actor(
        w,
        GRENADE_N,
        WorldActorClass::SlowtoStunGrenade,
        f,
        false,
        WorldActorState::SlowToStunGrenade(s),
    );
    let id = w.actors.actors.insert(a);
    let charge = instigator_charge(w, f);
    comp_spawn_actor(w, id, charge, f); // addActor, the BeginPlay below, afterSpawnedActor
}

/// ReceiveBeginPlay's loop sound, trace, attach and bind @1991-2623 -> @138-470 (see the header).
fn grenade_begin_play(w: &mut World, id: ActorId) {
    let (fighter, key) = (w.actors.actors.get(id).expect("a live world actor").owner_id, grenade_key(w, id));
    w.events.push(Event::SoundStart { fighter, ev: name!("sfx_cha_velma_vial_loop"), key });
    let (x, y) = (gr(w, id).x, gr(w, id).y);
    let me = ActorRef::Actor(id);
    let hit = box_trace_single(w, x, y - 50.0, x, y - 150.0, 100.0, 0.0, 100.0, TRACE_PROJECTILE_GROUND, &[], Some(me));
    let Some(h) = hit else { return }; // K2_AttachToActor(None): nothing; the cast fails
    if let Some(r) = h.floor {
        gr_mut(w, id).floor = Some(r);
    } else if h.actor.is_some() {
        warn_text(
            w,
            &format!("{SLOW_TO_STUN}: the ground trace met {}: the ride on it is not ported", w.names.str(h.component)),
        );
    }
    bind_ground_destroyed(w, Some(&h), me, DESTROY_IF_PLATFORM_DESTROYED);
}

/// DestroyIfPlatformDestroyed -> @4049 DestroySpawnedActor(false): the actor ends in the frame's list pass.
fn destroy_if_platform_destroyed(w: &mut World, listener: ActorRef, _from: WallActor) {
    if let ActorRef::Actor(id) = listener {
        gr_kill(w, id);
    }
}

/// The Box over a fighter's root capsule (an upright capsule: its segment within its radius of the box).
fn box_over(w: &World, id: ActorId, o: FighterId) -> bool {
    if w.fighter(o).life.dead > 0.0 || no_collision(w, o) {
        return false;
    }
    let k = w.actors.actors.get(id).and_then(|a| a.size_scale).unwrap_or(1.0); // the Box under the root, at its scale
    let g = gr(w, id);
    let (by, hx, hz) = (g.y + BOX_DZ * k, BOX_HX * k, BOX_HZ * k);
    let cr = cap_radius(w, o);
    let s = max(capsule_half(w, o) - cr, 0.0);
    let cy = actor_y(w, o);
    let dx = max((w.fighter(o).body.x - g.x).abs() - hx, 0.0);
    let dy = max(max(cy - s - (by + hz), 0.0), by - hz - (cy + s));
    (dx * dx + dy * dy).sqrt() <= cr
}

/// Remove Buff(actor): its Buff_SlowIntoStun (the first live one) whose Instigator is the grenade's.
fn remove_slow(w: &mut World, id: ActorId, o: FighterId) {
    let owner = gr_owner(w, id);
    if let Some(b) = buff_of(w, o, SLOW_N)
        && buff(w, b).is_some_and(|x| matches!(&x.ext, BuffExt::SlowIntoStun(e) if e.from == Some(owner)))
    {
        end_buff(w, o, b);
    }
}

fn grenade_begin(w: &mut World, id: ActorId, o: FighterId) {
    let owner = gr_owner(w, id);
    if w.fighter(o).team.team == w.fighter(owner).team.team || buff_of(w, o, SLOW_N).is_some() {
        return;
    }
    let left = remaining(w, id);
    let Some(b) = spawn_buff(w, o, SLOW_N, left, Some(1), Some(owner), true, Some(o)) else { return };
    if let Some(x) = buff_mut(w, b)
        && x.frames > 0.0
        && let BuffExt::SlowIntoStun(e) = &mut x.ext
    {
        e.trap = Some(ActorRef::Actor(id)); // AssignInstigatingTrapServer(self)
    }
}

fn grenade_overlaps(w: &World, a: ActorRef, x: Real, y: Real, r: Real) -> bool {
    let ActorRef::Actor(id) = a else { return false };
    if gr_dead(w, id) {
        return false;
    }
    let k = w.actors.actors.get(id).and_then(|a| a.size_scale).unwrap_or(1.0);
    let g = gr(w, id);
    let dx = max((x - g.x).abs() - BOX_HX * k, 0.0);
    let dy = max((y - (g.y + BOX_DZ * k)).abs() - BOX_HZ * k, 0.0);
    (dx * dx + dy * dy).sqrt() <= r
}

fn grenade_clean(w: &mut World, a: ActorRef) -> bool {
    if let ActorRef::Actor(id) = a
        && flag(actor_p(w, a), "DestroyOnBlocked")
    {
        gr_mut(w, id).blocked_destroy = true;
    }
    true
}

fn grenade_step(w: &mut World, id: ActorId) {
    if gr(w, id).blocked_destroy {
        gr_kill(w, id);
        return;
    }
    // The attach parent's move of the frame (see the header). The spawn frame's is already in the spawn place, so the
    // first step does not ride.
    let ride = gr(w, id).floor.map(|r| plat(w, r)).filter(|p| !p.dead).map(|p| (p.dx, p.dy));
    if let Some((dx, dy)) = ride
        && gr(w, id).age > 0
    {
        let g = gr_mut(w, id);
        g.x += dx;
        g.y += dy;
    }
    for o in w.all_ids() {
        let on = box_over(w, id, o);
        let oid = w.fighter(o).id;
        let k = gr(w, id).inside.iter().position(|x| *x == oid);
        if on && k.is_none() {
            gr_mut(w, id).inside.push(oid);
            grenade_begin(w, id, o);
        } else if !on && let Some(k) = k {
            gr_mut(w, id).inside.remove(k);
            remove_slow(w, id, o);
        }
    }
    // The MaxLifetimeTimer's pass of this frame comes after the actors' ticks: the actor ends in it.
    if crate::real(gr(w, id).age) >= gr(w, id).life_at {
        gr_kill(w, id);
    }
    gr_mut(w, id).age += 1;
}

fn grenade_end(w: &mut World, id: ActorId) {
    for o in w.all_ids() {
        if box_over(w, id, o) {
            remove_slow(w, id, o);
        }
    }
    let (fighter, key) = (w.actors.actors.get(id).expect("a live world actor").owner_id, grenade_key(w, id));
    w.events.push(Event::SoundStop {
        fighter,
        key,
        fade: None,
        ev: Some(name!("sfx_cha_velma_vial_loop_stop")),
        curve: None,
        cut: false,
    });
    let (x, y) = (gr(w, id).x, gr(w, id).y);
    w.events.push(Event::Sound {
        fighter,
        ev: name!("sfx_cha_velma_vial_end"),
        x: Some(x),
        y: Some(y),
        hit: None,
        pooled: false,
        key: None,
    });
}

// ---- Buff_SlowIntoStun

fn slow_begin(w: &mut World, f: FighterId, b: BuffRef, from: Option<FighterId>, d: Option<Real>) {
    let _ = d;
    let rate = inst_view(w, b).num("ApplyIceTime").unwrap_or(0.25);
    let start = crate::real(w.fighter(f).clock.tick);
    if let Some(x) = buff_mut(w, b) {
        x.ext = BuffExt::SlowIntoStun(SlowExt { trap: None, from, start, n: 1.0, rate });
    }
}

fn slow_timer(w: &mut World, f: FighterId, b: BuffRef) -> bool {
    let tick = crate::real(w.fighter(f).clock.tick);
    let Some(BuffExt::SlowIntoStun(e)) = buff(w, b).map(|x| &x.ext) else { return false };
    let (start, n, rate, trap, from) = (e.start, e.n, e.rate, e.trap, e.from);
    if tick < start + timer_ticks(Some(n * rate)) {
        return false;
    }
    if let Some(x) = buff_mut(w, b)
        && let BuffExt::SlowIntoStun(e) = &mut x.ext
    {
        e.n += 1.0;
    }
    let Some(ActorRef::Actor(g)) = trap else { return true };
    if gr_dead(w, g) || !box_over(w, g, f) {
        return true;
    }
    if invulnerable(w, f) {
        return false;
    }
    let max_stacks = buff_def(w, ActorRef::Fighter(f), ICE_N).and_then(|d| num(d, "MaxStacks")).unwrap_or(0.0);
    if let Some(ice) = buff_of(w, f, ICE_N)
        && buff(w, ice).is_some_and(|x| x.stacks >= max_stacks)
    {
        return false;
    }
    spawn_buff(w, f, ICE_N, 0.0, Some(1), from, true, Some(f));
    false
}

// ---- VelmaBookProjectile

fn book_loop(w: &World, id: ProjectileId) -> SoundKey {
    SoundKey { tag: name!("velmaBook"), n: pr(w, id).seq }
}

fn pr(w: &World, id: ProjectileId) -> &Projectile {
    w.actors.projectiles.get(id).expect("a live projectile")
}

fn pr_mut(w: &mut World, id: ProjectileId) -> &mut Projectile {
    w.actors.projectiles.get_mut(id).expect("a live projectile")
}

fn book_make(_w: &World, _f: FighterId, _r: &SpawnRequest) -> ProjectileExt {
    ProjectileExt::VelmaBook(VelmaBook::default())
}

/// The constructor's tail: the homing magnitude of the ProjectileMovement template, when it is a homing one.
fn book_construct(w: &World, p: &mut Projectile) {
    let mv = class_def(w, p).and_then(|d| obj(&d.components, "ProjectileMovement"));
    let home_mag = match mv {
        Some(m) if flag(m, "bIsHomingProjectile") => num(m, "HomingAccelerationMagnitude").unwrap_or(0.0),
        _ => 0.0,
    };
    if let ProjectileExt::VelmaBook(b) = &mut p.ext {
        b.home_mag = home_mag;
    }
}

fn book_begin(w: &mut World, id: ProjectileId) {
    let (f, fighter, key) = (pr(w, id).owner.expect("a book has an owner"), pr(w, id).owner_id, book_loop(w, id));
    w.events.push(Event::SoundStart { fighter, ev: name!("sfx_cha_velma_throw_book_loop"), key });
    // CheckForHomingTarget: the last ally (GetAllies: the owner's team, a fighter waiting for its respawn left out).
    let mut to = None;
    for &o in peers(w) {
        if o != f && w.fighter(o).team.team == w.fighter(f).team.team && w.fighter(o).life.dead <= 0.0 {
            to = Some(o);
        }
    }
    if to.is_some()
        && let ProjectileExt::VelmaBook(b) = &mut pr_mut(w, id).ext
    {
        b.home_to = to;
    }
}

fn book_hit_enemy(w: &mut World, id: ProjectileId, vic: FighterId) {
    let f = pr(w, id).owner.expect("a book has an owner");
    post_vo(w, f, name!("ability_generic"), Some(vic), 0.1);
}

fn book_end(w: &mut World, id: ProjectileId) {
    let (fighter, key) = (pr(w, id).owner_id, book_loop(w, id));
    w.events.push(Event::SoundStop {
        fighter,
        key,
        fade: None,
        ev: Some(name!("sfx_cha_velma_throw_book_loop_stop")),
        curve: None,
        cut: false,
    });
}

// ---- Velma_SPAIR_CameraFlash / Velma_AirSideSp_BookBlast

/// `makeBurst`, both classes.
fn burst_make(_w: &World, _f: FighterId, _r: &SpawnRequest) -> ProjectileExt {
    ProjectileExt::VelmaBurst(VelmaBurst::default())
}

/// The constructor's tail: the Sphere's radius at the root's scale and HitBoxScale, no hurt shape.
fn burst_construct(w: &World, p: &mut Projectile) {
    let r =
        class_def(w, p).and_then(|d| obj(&d.components, "Sphere")).and_then(|s| num(s, "SphereRadius")).unwrap_or(0.0);
    let hbs = obj(class_p(w, p), "HitBoxScale").and_then(|h| num(h, "X")).unwrap_or(1.0);
    p.hit_radius = r * p.scale * hbs;
    p.hurt_radius = 0.0;
}

fn burst_on_at(w: &mut World, id: ProjectileId) {
    let at = pr(w, id).age + to_int32(latent_frames(0.07));
    if let ProjectileExt::VelmaBurst(b) = &mut pr_mut(w, id).ext {
        b.on_at = at;
    }
}

/// `burst('PS_Velma_CameraFlash').begin`.
fn flash_begin(w: &mut World, id: ProjectileId) {
    let p = pr(w, id);
    let (cls, fighter, x, y) = (p.cls, p.owner_id, p.x, p.y);
    w.events.push(Event::ProjectileDeath {
        cls,
        fighter,
        vfx: Some(name!("PS_Velma_CameraFlash")),
        x,
        y,
        ox: 0.0,
        rot: [0.0; 3],
        scale: 1.0,
        tint: None,
        to: None,
    });
    burst_on_at(w, id);
}

/// `burst(null).begin`.
fn blast_begin(w: &mut World, id: ProjectileId) {
    burst_on_at(w, id);
}

/// The burst's step, both classes.
fn burst_step(w: &mut World, id: ProjectileId) -> bool {
    let p = pr(w, id);
    if let ProjectileExt::VelmaBurst(b) = &p.ext
        && p.age == b.on_at
    {
        set_hitbox(pr_mut(w, id), true);
    }
    false
}

// ---- Velma_SassBubblev2

fn bb(w: &World, id: ProjectileId) -> &VelmaSassBubble {
    match &pr(w, id).ext {
        ProjectileExt::VelmaSassBubble(s) => s,
        _ => panic!("a Velma_SassBubblev2_C"),
    }
}

fn bb_mut(w: &mut World, id: ProjectileId) -> &mut VelmaSassBubble {
    match &mut pr_mut(w, id).ext {
        ProjectileExt::VelmaSassBubble(s) => s,
        _ => panic!("a Velma_SassBubblev2_C"),
    }
}

fn bubble_make(_w: &World, _f: FighterId, _r: &SpawnRequest) -> ProjectileExt {
    ProjectileExt::VelmaSassBubble(VelmaSassBubble::default())
}

/// The constructor's tail: the HitBox sphere's radius at the root's scale and HitBoxScale.
fn bubble_construct(w: &World, p: &mut Projectile) {
    let r =
        class_def(w, p).and_then(|d| obj(&d.components, "HitBox")).and_then(|s| num(s, "SphereRadius")).unwrap_or(0.0);
    let hbs = obj(class_p(w, p), "HitBoxScale").and_then(|h| num(h, "X")).unwrap_or(1.0);
    p.hit_radius = r * p.scale * hbs;
}

fn bubble_begin(w: &mut World, id: ProjectileId) {
    let scale = pr(w, id).scale;
    let s = bb_mut(w, id);
    s.box_x = scale * BOX_SCALE_X;
    s.box_y = scale * BOX_SCALE_YZ;
    s.box_z = s.box_y;
    s.ps_scale = scale * 0.175;
}

/// ReceiveTick @143 (the parent's first), while not Activated (chars/lebron/bubble.rs's growth, the same graph); then
/// the Delay(0.16)'s resume.
fn bubble_step(w: &mut World, id: ProjectileId) -> bool {
    let (age, activated) = (pr(w, id).age, pr(w, id).activated);
    let off = bb(w, id).off_at;
    if off >= 0 && age == off {
        bb_mut(w, id).off_at = -1;
        set_hitbox(pr_mut(w, id), false);
    }
    if activated {
        return false;
    }
    let g = num_or(class_p(w, pr(w, id)), "GrowthValue", 0.0);
    let k = 1.0 + (DELTA_SECONDS * g);
    let s = bb_mut(w, id);
    s.box_x *= k;
    s.box_y *= k;
    s.box_z *= k;
    s.growth_len = size2d([s.box_x, s.box_y]);
    s.box_fwd = k * BOX_FORWARD;
    s.ps_scale *= 1.0 + ((DELTA_SECONDS * g) * 0.5);
    false
}

/// `box()`: the Box as it has grown.
fn bubble_box(w: &World, id: ProjectileId) -> Rect {
    let (p, s) = (pr(w, id), bb(w, id));
    Rect {
        x: p.x + s.box_fwd * p.scale * p.facing,
        y: p.y,
        hx: BOX_EXTENT * s.box_x,
        hy: BOX_EXTENT * s.box_z,
        hd: BOX_EXTENT * s.box_y,
    }
}

/// `scaledBox()`: the Box behind SetRelativeScale3D(HitBoxScale); none for an equal relative scale.
fn bubble_scaled_box(w: &World, id: ProjectileId) -> Option<Rect> {
    let (p, s) = (pr(w, id), bb(w, id));
    let h = obj(class_p(w, p), "HitBoxScale");
    let axis = |k: &str| h.and_then(|h| num(h, k)).unwrap_or(1.0);
    let (sx, sz) = ((p.scale * axis("X")), (p.scale * axis("Z")));
    let sy = p.scale * axis("Y");
    if sx == s.box_x && sy == s.box_y && sz == s.box_z {
        return None;
    }
    Some(Rect {
        x: p.x + s.box_fwd * p.scale * p.facing,
        y: p.y,
        hx: BOX_EXTENT * sx,
        hy: BOX_EXTENT * sz,
        hd: BOX_EXTENT * sy,
    })
}

/// The bubble kind's activate (the blast, the box hits, the Delay(0.16)).
fn bubble_kind_activate(w: &mut World, id: ProjectileId) {
    let f = pr(w, id).owner.expect("a bubble has an owner");
    let len = bb(w, id).growth_len;
    let weak = len < 10.0;
    let (cls, fighter, px, py) = (pr(w, id).cls, pr(w, id).owner_id, pr(w, id).x, pr(w, id).y);
    let (fx, fy) = (w.fighter(f).body.x, actor_y(w, f));
    w.events.push(Event::ProjectileDeath {
        cls,
        fighter,
        vfx: Some(if weak { name!("PS_Velma_NAD_Calculator_Weak") } else { name!("PS_Velma_NAD_Calculator") }),
        x: fx,
        y: fy + 150.0,
        ox: 0.0,
        rot: [0.0; 3],
        scale: if weak { 1.0 } else { 1.5 },
        tint: None,
        to: None,
    });
    let ev = if len < 13.472 {
        name!("sfx_cha_wonderwoman_charge_bracer_blast_01")
    } else {
        name!("sfx_cha_wonderwoman_charge_bracer_blast_02")
    };
    w.events.push(Event::Sound { fighter, ev, x: Some(px), y: Some(py), hit: None, pooled: false, key: None });
    set_hitbox(pr_mut(w, id), true);
    if !bb(w, id).box_gone {
        let b = bubble_box(w, id);
        box_hits(w, id, b);
        if let Some(scaled) = bubble_scaled_box(w, id)
            && !pr(w, id).dead
        {
            box_hits(w, id, scaled);
        }
        bb_mut(w, id).box_gone = true;
    }
    // A Delay already running is not started again (the latent action manager's one per UUID).
    if bb(w, id).off_at < 0 {
        let at = pr(w, id).age + to_int32(latent_frames(0.16));
        bb_mut(w, id).off_at = at;
    }
}

fn burst_entry(cls: ProjectileClass, begin: fn(&mut World, ProjectileId)) -> ActorEntry {
    ActorEntry::new(
        ActorClass::Projectile(cls),
        ActorHooks::Projectile(ProjectileHooks {
            make: Some(burst_make),
            construct: Some(burst_construct),
            begin: Some(begin),
            step: Some(burst_step),
            ..ProjectileHooks::default()
        }),
    )
}

/// This file's registry entries: VelmaVialAttack_C, SlowtoStunGrenade_C, VelmaBookProjectile_C,
/// Velma_SPAIR_CameraFlash_C, Velma_AirSideSp_BookBlast_C, Velma_SassBubblev2_C, Buff_SlowIntoStun_C.
pub fn register(r: &mut Registry) -> Result<(), RegistryError> {
    r.actors.enter(
        VIAL,
        ActorEntry::new(
            ActorClass::World(WorldActorClass::VelmaVialAttack),
            ActorHooks::World(WorldActorHooks {
                size: Some(SizeHooks::default()),
                spawn: Some(vial_spawn),
                step: Some(vial_step),
                hurt_box: Some(vial_hurt_box),
                ..WorldActorHooks::default()
            }),
        ),
    )?;
    let mut grenade = ActorEntry::new(
        ActorClass::World(WorldActorClass::SlowtoStunGrenade),
        ActorHooks::World(WorldActorHooks {
            size: Some(SizeHooks::default()),
            begin_play: Some(grenade_begin_play),
            step: Some(grenade_step),
            end: Some(grenade_end),
            ..WorldActorHooks::default()
        }),
    );
    grenade.hazard = Some(HazardHooks { overlaps: grenade_overlaps, clean: grenade_clean });
    r.actors.enter(SLOW_TO_STUN, grenade)?;
    wall_delegate_fns(r, &[(DESTROY_IF_PLATFORM_DESTROYED, destroy_if_platform_destroyed)])?;
    r.actors.enter(
        BOOK,
        ActorEntry::new(
            ActorClass::Projectile(ProjectileClass::VelmaBookProjectile),
            ActorHooks::Projectile(ProjectileHooks {
                make: Some(book_make),
                construct: Some(book_construct),
                begin: Some(book_begin),
                hit_enemy: Some(book_hit_enemy),
                end: Some(book_end),
                ..ProjectileHooks::default()
            }),
        ),
    )?;
    r.actors.enter(CAMERA_FLASH, burst_entry(ProjectileClass::VelmaSPAIRCameraFlash, flash_begin))?;
    r.actors.enter(BOOK_BLAST, burst_entry(ProjectileClass::VelmaAirSideSpBookBlast, blast_begin))?;
    r.actors.enter(
        SASS_BUBBLE,
        ActorEntry::new(
            ActorClass::Projectile(ProjectileClass::VelmaSassBubblev2),
            ActorHooks::Projectile(ProjectileHooks {
                make: Some(bubble_make),
                construct: Some(bubble_construct),
                begin: Some(bubble_begin),
                step: Some(bubble_step),
                activate: Some(bubble_kind_activate),
                ..ProjectileHooks::default()
            }),
        ),
    )?;
    r.buffs.enter(
        SLOW_INTO_STUN,
        BuffEntry::new(
            BuffClass::BuffSlowIntoStun,
            BuffHooks { begin: Some(slow_begin), timer: Some(slow_timer), ..BuffHooks::default() },
        ),
    )
}
