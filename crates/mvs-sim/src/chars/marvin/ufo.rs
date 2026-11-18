//! Marvin's UFO: C018_Platform_C ([`Ufo`], a World.actors SpawnedPlatformParent
//! child with ShipMesh's montages and the `edgeBox` of Comp_EdgeTrace), C018CannonBlast_C (a plain projectile with its
//! own kind) and C018RocketExplosion_C ([`RocketExplosion`], a DealerActor). C018_PerkPlatform_C, C018PerkCannonBlast_C
//! and C018PerkRocketExplosion_C (perks.rs) enter the same kinds: [`platform_entry`], [`cannon_blast_hooks`],
//! [`rocket_explosion_entry`]; the perk platform's own functions (its `PlatformChild`) are perks.rs's, called from
//! here by the actor's class. The live UFO is `MarvinState::ufo` (state.rs); the UFO's own variables are on [`Ufo`].
//!
//! The UFO's parts are the actor's `solids`: Collision, CollisionMidLeft, CollisionMidRight (`UfoPart`s, the first the
//! `main` one the fighters' BPI_IgnorePlatformCollision call reaches) and WallbounceCollision (the parent's, when the
//! class has it), in that order; `SpawnedPlatform::collision` is none, so [`edge_box`] is the
//! first
//! part.
//!
//! A Delay (latent): its `*_at` field is the actor's age when it resumes (`delay`: age + latent frames), run in the
//! step's latent pass behind ReceiveTick; a Delay node that is pending ignores a new call (its field is set only while
//! 0). A K2_SetTimerDelegate timer (`life_at`, `aim_ready_at`, `aim_fire_at`): the World tick it fires in, 0 for none,
//! run in the kind's `late`.
// Deviation: Delays due in the same tick run in the fixed order of the latent pass in `step` (the build: the order they
// were registered; the intro @11841 is registered ahead of the collision @11687, the order kept here).

#![allow(clippy::neg_cmp_op_on_partial_ord)] // `!(x > 0)` is true for a NaN

use indexmap::IndexSet;
use mvs_data::value::Props;

use super::pawn::set_cannon;
use super::perks;
use super::state::marvin_of;
use crate::Real;
use crate::State;
use crate::actors::actor_montage::{ActorAnim, MontageEvent, montage_play, montage_play_side};
use crate::actors::world_actor::{WorldActorState, actor_p};
use crate::actors::{ActorClass, ActorId, ActorRef, ProjectileClass, ProjectileId, WorldActor, WorldActorClass};
use crate::combat::HitPacket;
use crate::combat::boxes::WorldSphere;
use crate::combat::elem::{charge_pct, instigator_charge};
use crate::combat::hit::timer_ticks;
use crate::combat::hits::{
    HitShape, actor_hit_enemy, apply_projectile_hit, dealer, hit_box_actor_hits, projectile_blocks,
};
use crate::combat::phases::{alive, give_vbuf, los_blocked, no_hit, victim_pose};
use crate::combat::spawned_hits::{class_rows, hit_box_types};
use crate::consts as C;
use crate::events::{Event, SoundKey, UiStateOp, UiStateParams};
use crate::fighter::common::{actor_data, actor_def, latent_frames, warn_text};
use crate::fighter::control::{attack_dodged, begin_alternate_cooldown, dodge_over};
use crate::fighter::match_side::SpawnRequest;
use crate::fighter::moves::V2;
use crate::fighter::traces::{
    OBJ_WORLD_STATIC, TraceBody, TraceHit, body_info, capsule_trace_multi_for_objects, solids::QUERY_WORLDSTATIC,
};
use crate::fighter::{
    FighterId, actor_y, cap_radius, capsule_half, facing, grounded, invulnerable, signal_actors, signal_bound,
};
use crate::math::{max, min, round};
use crate::name;
use crate::names::Name;
use crate::registry::entries::{
    ActorEntry, ActorHooks, DamageBox, MeshHooks, ProjectileHooks, SizeHooks, WorldActorHooks,
};
use crate::registry::props::{all_fighters, flag, name_of, num, obj, text};
use crate::registry::signals::{FighterSignal, OwnerSignal};
use crate::registry::{Registry, RegistryError};
use crate::world::World;
use crate::world::blast::{BlastBounds, blast_box_loop, blast_timer_exit_rounded, new_blast_bounds, spawn_box};
use crate::world::pawns::match_fighters;
use crate::world::spawn_spot::spawn_place;
use crate::world::spawned_platform::{
    PlatformPart, SpawnedPlatform, ignore_wallbounce_collision as platform_ignore_wallbounce, platform_step,
};
use crate::world::spawning::{add_actor, comp_spawn_actor, comp_spawn_projectile, new_world_actor, request_props};

/// C018_Platform's class.
pub const PLATFORM: &str = "C018_Platform_C";

/// C018CannonBlast's class.
pub const CANNON_BLAST: &str = "C018CannonBlast_C";

/// C018RocketExplosion's class.
pub const ROCKET_EXPLOSION: &str = "C018RocketExplosion_C";

const UI_ID: Name = name!("UFO");
const IDLE_LOOP: Name = name!("sfx_cha_C018_ufo_idle_loop");

/// The actor's own AkComponent as the voices' key (`akKey`: `'ak' + a.id`).
fn ak_key(seq: i32) -> SoundKey {
    SoundKey { tag: name!("ak"), n: seq }
}

// C019MarkerCameraFocus (KeepOnCamera) from BeginPlay to EndPlay. Deviation: the camera takes the platform's place,
// where the focus actor stays at the place before FindValidSpawnPosition's move.
/// `Ufo` (C018_Platform_C and C018_PerkPlatform_C): `anim` is ShipMesh's montages (`actor_montage::anim_of`).
#[derive(State, Debug)]
pub struct Ufo {
    pub x: Real,
    pub y: Real,
    /// Ticks, the spawn frame counted.
    pub age: i32,
    /// `anim = new ActorAnim('ShipMesh')`.
    pub anim: ActorAnim,
    /// CheckBlastBoxTimer's next expiry (s of counted ticks).
    pub blast_at: Real,
    pub blast_bounds: BlastBounds,
    /// The perk platform's own variables (`child`), `None` for C018_Platform.
    pub child: Option<perks::PerkUfoVars>,
    /// CurrentHealth.
    pub health: Real,
    /// Destroyed.
    pub destroyed: bool,
    /// AimTarget.
    pub aim_target: Option<FighterId>,
    pub aim_target_x: Real,
    pub aim_target_y: Real,
    /// AimerLocation.
    pub aimer_x: Real,
    pub aimer_y: Real,
    /// CurrentCannonAmmo.
    pub ammo: Real,
    /// ReadyToFire.
    pub ready_to_fire: bool,
    pub ui_state: Real,
    /// AimOn.
    pub aim_on: bool,
    /// 'Facing Left' (the ship mesh's turn).
    pub facing_left: bool,
    /// RotationDeltaTime (the turn's blend, presentation).
    pub rdt: Real,
    pub ship_hidden: bool,
    pub rocket_hidden: bool,
    pub beam_on: bool,
    /// The actor's collision (SetActorEnableCollision).
    pub collision_on: bool,
    /// SpawnedActorParent CollisionDisableCount (BPI_SetCollisionEnabled).
    pub disable_count: i32,
    /// The three sensors and their counters (ImpassibleObjectOverlapCount, ...MidLeft, ...MidRight).
    pub in_mid: bool,
    pub in_l: bool,
    pub in_r: bool,
    pub count_mid: i32,
    pub count_l: i32,
    pub count_r: i32,
    /// The boxes' own collision (Increment / Decrement Collision).
    pub col_on: bool,
    pub mid_l_on: bool,
    pub mid_r_on: bool,
    /// The fighters (ids) standing on one of the three boxes at the last step.
    pub standing: Vec<i32>,
    /// A capsule's bounce off one of the three boxes since the last step (`wall_bounce_multiplier`).
    pub bounced: bool,
    // Delays (ages; 0: none pending).
    /// Delay(0.2) @11841 -> 615.
    pub intro_at: Real,
    /// Delay(TimeTillCollisionsActive) @11687 -> 1924.
    pub collision_at: Real,
    /// Delay(DelayBeforeReload) @21608 -> 109.
    pub reload_at: Real,
    /// Delay(DelayBeforeReload) @16199 -> 144.
    pub ui_reload_at: Real,
    /// UpdateFireReadyVisualsMulticast's Current Value.
    pub ui_value: Real,
    /// Delay(0.4) @20251 -> 1940.
    pub destroyed_sound_at: Real,
    /// Delay(OutOfHealthDestroyDelay) @2226 -> 93.
    pub destroy_at: Real,
    /// The parent's OnMaxLifetimeExpired Delay(DestroyActor_Delay) -> DestroySpawnedActor.
    pub life_end_at: Real,
    /// Delay(0.3) @21075 -> 1795.
    pub portal_despawn_at: Real,
    /// Delay(0.5) @20954 -> 1666.
    pub despawn_sound_at: Real,
    // Timers (World ticks; 0: none).
    /// MaxLifetimeTimer -> OnMaxLifetimeExpired.
    pub life_at: Real,
    /// TimeTillAimReady -> AfterBlastArmed (@1428).
    pub aim_ready_at: Real,
    /// TimeFromAimToFire -> AfterAimDelay (AfterBlastArmed's).
    pub aim_fire_at: Real,
}

impl Default for Ufo {
    fn default() -> Self {
        Ufo {
            x: 0.0,
            y: 0.0,
            age: 0,
            anim: ActorAnim::new(name!("ShipMesh")),
            blast_at: 0.1,
            blast_bounds: new_blast_bounds(),
            child: None,
            health: 0.0,
            destroyed: false,
            aim_target: None,
            aim_target_x: 0.0,
            aim_target_y: 0.0,
            aimer_x: 0.0,
            aimer_y: 0.0,
            ammo: 0.0,
            ready_to_fire: false,
            ui_state: 0.0,
            aim_on: false,
            facing_left: false,
            rdt: 0.0,
            ship_hidden: false,
            rocket_hidden: false,
            beam_on: false,
            collision_on: false,
            disable_count: 0,
            in_mid: false,
            in_l: false,
            in_r: false,
            count_mid: 0,
            count_l: 0,
            count_r: 0,
            col_on: true,
            mid_l_on: true,
            mid_r_on: true,
            standing: Vec::new(),
            bounced: false,
            intro_at: 0.0,
            collision_at: 0.0,
            reload_at: 0.0,
            ui_reload_at: 0.0,
            ui_value: 0.0,
            destroyed_sound_at: 0.0,
            destroy_at: 0.0,
            life_end_at: 0.0,
            portal_despawn_at: 0.0,
            despawn_sound_at: 0.0,
            life_at: 0.0,
            aim_ready_at: 0.0,
            aim_fire_at: 0.0,
        }
    }
}

/// `RocketExplosion` (C018RocketExplosion_C and C018PerkRocketExplosion_C).
// Deviation: the HitBox is tested at the actor's hit pass in the play plane (a sphere of the capsule's radius).
// Deviation: the actor's yaw is 0 (UCS rot(0,0,0)), which faces neither side in the play plane: the shot's facing
// stands in (its knockback's X is 0).
#[derive(State, Debug)]
pub struct RocketExplosion {
    /// HitBoxDamage.
    pub hit: HitPacket,
    pub attack_id: i32,
    /// The shot's facing.
    pub facing: Real,
    /// HitBoxOn?.
    pub hit_on: bool,
    /// The HitBox sphere's radius (the root's scale is taken at the read).
    pub r: Real,
    /// HitEachActorOnlyOnce, by fighter id.
    pub hit_set: IndexSet<i32>,
    /// The actors the HitBox overlaps (`hit_box_actor_hits`).
    pub over: IndexSet<ActorRef>,
    /// Their HitEachActorOnlyOnce track.
    pub once: IndexSet<ActorRef>,
    /// The gathered object types the HitBox answers.
    pub types: Vec<Real>,
    /// The age Delay(0.2) fires at.
    pub off_at: Real,
    /// The age DestroySpawnedActor runs at after MaxLifetime.
    pub end_age: Real,
    /// Ticks, the spawn frame counted.
    pub age: i32,
    pub blast_at: Real,
    pub blast_bounds: BlastBounds,
    pub x: Real,
    pub y: Real,
}

impl Default for RocketExplosion {
    fn default() -> Self {
        RocketExplosion {
            hit: HitPacket::default(),
            attack_id: 0,
            facing: 0.0,
            hit_on: false,
            r: 0.0,
            hit_set: IndexSet::new(),
            over: IndexSet::new(),
            once: IndexSet::new(),
            types: Vec::new(),
            off_at: 0.0,
            end_age: 0.0,
            age: 0,
            blast_at: 0.1,
            blast_bounds: new_blast_bounds(),
            x: 0.0,
            y: 0.0,
        }
    }
}

// ---- access

fn ufo(w: &World, id: ActorId) -> Option<&Ufo> {
    match &w.actors.actors.get(id)?.state {
        WorldActorState::Ufo(u) => Some(u),
        _ => None,
    }
}

fn ufo_mut(w: &mut World, id: ActorId) -> Option<&mut Ufo> {
    match &mut w.actors.actors.get_mut(id)?.state {
        WorldActorState::Ufo(u) => Some(u),
        _ => None,
    }
}

/// The UFO, its SpawnedPlatform and the root's scale (`u.k`).
fn with<R>(w: &mut World, id: ActorId, f: impl FnOnce(&mut Ufo, &mut SpawnedPlatform, Real) -> R) -> Option<R> {
    let a = w.actors.actors.get_mut(id)?;
    let k = a.size_scale.unwrap_or(1.0);
    let WorldActorState::Ufo(u) = &mut a.state else { return None };
    let pl = a.plat.as_mut()?;
    Some(f(u, pl, k))
}

fn owner_of(w: &World, id: ActorId) -> Option<FighterId> {
    w.actors.actors.get(id).map(|a| a.owner)
}

fn is_perk(w: &World, id: ActorId) -> bool {
    w.actors.actors.get(id).is_some_and(|a| a.class == WorldActorClass::C018PerkPlatform)
}

/// The root's scale (`u.k`).
fn size_k(w: &World, id: ActorId) -> Real {
    w.actors.actors.get(id).and_then(|a| a.size_scale).unwrap_or(1.0)
}

/// A number of the actor's class data (`u.p`).
fn pnum(w: &World, id: ActorId, k: &str) -> Option<Real> {
    num(actor_p(w, ActorRef::Actor(id)), k)
}

/// A montage the class data names, as its name.
fn montage_name(w: &World, id: ActorId, k: &str) -> Name {
    name_of(w, actor_p(w, ActorRef::Actor(id)), k).unwrap_or(Name::NONE)
}

fn play(w: &mut World, id: ActorId, k: &str) {
    let m = montage_name(w, id, k);
    if m != Name::NONE {
        montage_play(w, ActorRef::Actor(id), m, 1.0, 0.0, true);
    }
}

fn play_side(w: &mut World, id: ActorId, k: &str) {
    let m = montage_name(w, id, k);
    if m != Name::NONE {
        montage_play_side(w, ActorRef::Actor(id), m, 1.0, 0.0);
    }
}

/// K2_SetTimerDelegate(d, rate, false): the World tick it fires in, 0 for no Rate or 0 (no timer; a negative Rate fires the next tick). Also perks.rs's.
pub fn timer_at(w: &World, rate: Option<Real>) -> Real {
    let k = timer_ticks(rate);
    if k == Real::INFINITY { 0.0 } else { crate::real(w.flow.tick) + k }
}

/// `delay(u, s)`: the age a Delay(s) resumes at.
fn delay(u: &Ufo, s: Real) -> Real {
    crate::real(u.age) + latent_frames(s)
}

fn sound(w: &mut World, id: ActorId, ev: Name) {
    let Some(a) = w.actors.actors.get(id) else { return };
    let (seq, f) = (a.seq, a.owner);
    let Some(u) = ufo(w, id) else { return };
    let (x, y) = (u.x, u.y);
    let fighter = w.fighter(f).id;
    w.events.push(Event::Sound {
        fighter,
        ev,
        x: Some(x),
        y: Some(y),
        hit: None,
        pooled: false,
        key: Some(ak_key(seq)),
    });
}

/// PF_Post_Event_At_Location(ev, the Capsule's location, ..., null instigator): a pooled AkComponent at the place.
fn sound_at(w: &mut World, id: ActorId, ev: Name, dy: Real) {
    let Some(f) = owner_of(w, id) else { return };
    let Some(u) = ufo(w, id) else { return };
    let (x, y) = (u.x, u.y + dy);
    let fighter = w.fighter(f).id;
    w.events.push(Event::Sound { fighter, ev, x: Some(x), y: Some(y), hit: None, pooled: true, key: None });
}

// ---- the parts

/// A component's box in the play plane: half extents (X, Z) and place (local X, Z), from the class data.
struct PlaneBox {
    hx: Real,
    hy: Real,
    ox: Real,
    oy: Real,
}

/// `planeBox`: `k` the root's scale over the extents and the place.
fn plane_box(comp: Option<&Props>, k: Real) -> PlaneBox {
    let e = comp.and_then(|c| obj(c, "BoxExtent"));
    let l = comp.and_then(|c| obj(c, "RelativeLocation"));
    PlaneBox {
        hx: e.and_then(|e| num(e, "X")).unwrap_or(32.0) * k,
        hy: e.and_then(|e| num(e, "Z")).unwrap_or(32.0) * k,
        ox: round(l.and_then(|l| num(l, "X")).unwrap_or(0.0)) * k,
        oy: l.and_then(|l| num(l, "Z")).unwrap_or(0.0) * k,
    }
}

/// A capsule of the class data in the play plane: radius r, the core's half length (half height - r), its centre's
/// place, and whether it lies along x (pitch 90: Hurtbox2).
struct PlaneCapsule {
    r: Real,
    half: Real,
    ox: Real,
    oy: Real,
    flat: bool,
}

/// `planeCapsule`: `k` the root's scale.
fn plane_capsule(comp: Option<&Props>, k: Real) -> Option<PlaneCapsule> {
    let c = comp?;
    let r = num(c, "CapsuleRadius").filter(|r| *r != 0.0)?;
    let l = obj(c, "RelativeLocation");
    let half = max(num(c, "CapsuleHalfHeight").unwrap_or(r) - r, 0.0);
    let pitch = obj(c, "RelativeRotation").and_then(|o| num(o, "Pitch")).unwrap_or(0.0);
    Some(PlaneCapsule {
        r: r * k,
        half: half * k,
        ox: round(l.and_then(|l| num(l, "X")).unwrap_or(0.0)) * k,
        oy: l.and_then(|l| num(l, "Z")).unwrap_or(0.0) * k,
        flat: pitch.abs() == 90.0,
    })
}

/// The class's component of the owner's data.
fn comp_box(w: &World, f: FighterId, cls: Name, comp: &str, k: Real) -> PlaneBox {
    plane_box(actor_def(w, ActorRef::Fighter(f), cls).and_then(|d| obj(&d.components, comp)), k)
}

/// The four platform boxes' component names, in the parts' order.
const PART_COMPS: [&str; 4] = ["Collision", "CollisionMidLeft", "CollisionMidRight", "WallbounceCollision"];

/// A `UfoPart`: a platform box of the class (Platform object type, 180 x 48 in the play plane). `main`: Collision,
/// which the fighters' BPI_IgnorePlatformCollision call reaches (the class's override writes all three, so the two Mid
/// boxes take no call of their own).
fn make_part(w: &World, f: FighterId, cls: Name, i: usize) -> PlatformPart {
    let comp = actor_def(w, ActorRef::Fighter(f), cls).and_then(|d| obj(&d.components, PART_COMPS[i]));
    let b = plane_box(comp, 1.0);
    let none = Props::new();
    let mut p =
        PlatformPart::new(w.names.get(PART_COMPS[i]).unwrap_or(Name::NONE), false, b.hx, b.hy, comp.unwrap_or(&none));
    p.takes_ignore = i == 0;
    p
}

/// The boxes where the actor stands, with their collision (the actor's, and each box's own from the sensors). The
/// platform does not move after its spawn: dx / dy stay 0 (a still base).
fn place_parts(w: &mut World, id: ActorId) {
    let Some(a) = w.actors.actors.get(id) else { return };
    let (f, cls, k) = (a.owner, a.cls, a.size_scale.unwrap_or(1.0));
    let boxes: Vec<PlaneBox> = PART_COMPS.iter().map(|c| comp_box(w, f, cls, c, k)).collect();
    with(w, id, |u, pl, _| {
        let own = [u.col_on, u.mid_l_on, u.mid_r_on];
        let wb = pl.wallbounce.map(usize::from);
        for (i, b) in boxes.iter().enumerate() {
            let on = match i {
                0..=2 => own[i],
                _ => true,
            };
            let idx = if i < 3 { Some(i) } else { wb };
            let Some(pt) = idx.and_then(|j| pl.parts.get_mut(j)) else { continue };
            pt.place(u.x + b.ox, u.y + b.oy);
            pt.dx = 0.0;
            pt.dy = 0.0;
            pt.on = u.collision_on && on;
        }
    });
}

/// `partsOn`.
fn parts_on(u: &Ufo, pl: &mut SpawnedPlatform) {
    let c = u.collision_on;
    for (i, on) in [u.col_on, u.mid_l_on, u.mid_r_on].into_iter().enumerate() {
        if let Some(p) = pl.parts.get_mut(i) {
            p.on = c && on;
        }
    }
    if let Some(i) = pl.wallbounce
        && let Some(p) = pl.parts.get_mut(usize::from(i))
    {
        p.on = c;
    }
}

fn parts_on_of(w: &mut World, id: ActorId) {
    with(w, id, |u, pl, _| parts_on(u, pl));
}

/// BPI_SetCollisionEnabled (SpawnedActorParent @5242): CollisionDisableCount; the actor's collision follows it.
fn set_collision(u: &mut Ufo, on: bool) {
    if on {
        if u.disable_count > 0 {
            u.disable_count -= 1;
            if u.disable_count == 0 {
                u.collision_on = true;
            }
        }
    } else {
        u.disable_count += 1;
        if u.disable_count == 1 {
            u.collision_on = false;
        }
    }
}

/// `edgeBox`: the part Comp_EdgeTrace registers as the UFO's polygon (its Collision, a `UfoPart` of its own:
/// `SpawnedPlatform.collision` is none there). Called by `fighter::traces::edges::build`.
// (Collision_GEN_VARIABLE keeps SpawnedPlatformParent's TraceEdges tag; the two Mid boxes and WallbounceCollision carry
// none.)
pub fn edge_box<'a>(w: &'a World, a: &'a WorldActor, s: &'a Ufo) -> Option<&'a PlatformPart> {
    let _ = (w, s);
    a.plat.as_ref()?.parts.first()
}

/// The UFO's place (its `x`, `y`).
pub fn place(w: &World, a: &WorldActor, s: &Ufo) -> Option<(Real, Real)> {
    let _ = (w, a);
    Some((s.x, s.y))
}

/// BPI_CanBeReflected-free read: the RocketExplosion's place (its `x`, `y`).
pub fn rocket_place(w: &World, a: &WorldActor, s: &RocketExplosion) -> Option<(Real, Real)> {
    let _ = (w, a);
    Some((s.x, s.y))
}

/// The RocketExplosion as a dealer (`DealerActor`): its place, facing and AttackID.
pub fn rocket_dealer_parts(w: &World, a: &WorldActor, s: &RocketExplosion) -> (Real, Real, Real, i32) {
    let _ = (w, a);
    (s.x, s.y, s.facing, s.attack_id)
}

/// The RocketExplosion's HitBoxDamage packet (`hit`).
pub fn rocket_hit(s: &RocketExplosion) -> &HitPacket {
    &s.hit
}

/// BPI_IsCannonReadyToFire's read of the platform (`UfoFields`): CurrentCannonAmmo > 0 and ReadyToFire of `a`, a live
/// C018_Platform_C or C018_PerkPlatform_C. Called by pawn.rs.
pub fn ready_to_fire(w: &World, a: ActorRef) -> bool {
    let ActorRef::Actor(id) = a else { return false };
    ufo(w, id).is_some_and(|u| u.ammo > 0.0 && u.ready_to_fire)
}

// ---- spawn (BeginPlay)

/// FindValidSpawnPosition (function @0-4314; LocalObjectChecks [WorldStatic], LocalRadius 325, LocalHalfHeight 725): a
/// capsule overlap at the actor (@155); none: no move. Else four sweeps of the same capsule over 2 x 725, in order down
/// (@493), up (@2145), toward +x (@2605) and toward -x (@3065); each sweep's hits in order: the hit's Location strictly
/// inside the stored blast bounds (@1133-1721), not a BP_FighterCharacter (@1991), and a capsule of 320 / 720 (5 less)
/// at the Location that meets nothing (@3878): the place. No such hit: no move.
/// K2_SetActorLocation(MakeVector(0, Y, Z)) @4040: the place, no sweep.
fn find_valid_spawn_position(w: &mut World, id: ActorId) {
    const R: Real = 325.0;
    const HH: Real = 725.0;
    const L: Real = 2.0 * HH;
    let Some(u) = ufo(w, id) else { return };
    let (x, y, b) = (u.x, u.y, u.blast_bounds);
    let own = Some(ActorRef::Actor(id));
    let types = [crate::real(OBJ_WORLD_STATIC)];
    let mut hits: Vec<TraceHit> = Vec::new();
    let mut hits2: Vec<TraceHit> = Vec::new();
    capsule_trace_multi_for_objects(w, x, y, x, y, R, HH, &types, &[], own, &mut hits);
    if hits.is_empty() {
        return;
    }
    let sweeps = [(x, y + L, x, y - L), (x, y - L, x, y + L), (x - L, y, x + L, y), (x + L, y, x - L, y)];
    for (x0, y0, x1, y1) in sweeps {
        capsule_trace_multi_for_objects(w, x0, y0, x1, y1, R, HH, &types, &[], own, &mut hits);
        let list = hits.clone();
        for h in &list {
            if !(h.y > b.bottom && h.y < b.top && h.x > b.left && h.x < b.right) {
                continue;
            }
            if matches!(h.actor, Some(ActorRef::Fighter(_))) {
                continue;
            }
            capsule_trace_multi_for_objects(w, h.x, h.y, h.x, h.y, R - 5.0, HH - 5.0, &types, &[], own, &mut hits2);
            if !hits2.is_empty() {
                continue;
            }
            if let Some(u) = ufo_mut(w, id) {
                u.x = h.x;
                u.y = h.y;
            }
            return;
        }
    }
}

/// `ufoSpawn`: CompSpawnActor(C018_Platform_C) from the up special's notify (a spawn request): at the request's place.
/// The construction (BPI_SetCollisionEnabled false @42) and ReceiveBeginPlay's parent part (@11131: the blast box clamp
/// with ForceSpawnPositionInsideBlastBox, its bounds, MaxLifetimeTimer); the rest, which reads the actor in the world
/// (the SpawnedPlatform and its boxes, BPI_SetCannon, FindValidSpawnPosition), is [`spawned`].
fn spawn(w: &mut World, f: FighterId, r: &SpawnRequest) -> (WorldActorState, bool) {
    let np = request_props(w, f, r);
    let at = spawn_place(w, f, &np);
    let data = w.data.clone();
    let none = Props::new();
    let p = actor_data(w, &data, ActorRef::Fighter(f), r.cls).map_or(&none, |d| &d.p);
    let mut u = Ufo {
        x: at.x,
        y: at.y,
        health: num(p, "MaxHealth").unwrap_or(0.0),
        ready_to_fire: flag(p, "ReadyToFire"),
        facing_left: flag(p, "Facing Left"),
        ..Ufo::default()
    };
    set_collision(&mut u, false); // the construction script @42
    if r.cls == name!("C018_PerkPlatform_C") {
        u.child = Some(perks::PerkUfoVars::default());
    }
    let sb = spawn_box(w);
    let dead = blast_box_loop(
        &sb,
        &mut u.x,
        &mut u.y,
        None,
        flag(p, "ForceSpawnPositionInsideBlastBox"),
        &mut u.blast_bounds,
        flag(p, "DestroyOnExitedArena?"),
    );
    if !dead {
        u.life_at = timer_at(w, Some(num(p, "MaxLifetime").unwrap_or(0.0)));
    }
    (WorldActorState::Ufo(u), dead)
}

/// What the constructor and BeginPlay do once the actor is in the world: the platform's SpawnedPlatform and its
/// boxes, then (@11131) BPI_SetCannon(self) on the Instigator @11515, authority FindValidSpawnPosition and the move,
/// the portal sound at PS_C018_SpaceshipArrival (ShipMesh (0, -210, -240) + (0, 0, -200): y - 440) @11608, ShipMesh
/// hidden @11807 and the intro Delay(0.2) @11841, then Delay(TimeTillCollisionsActive) @11687, the child's begin.
// Unverified: the Instigator's OnBFDefeated bound @4175 (-> @21130: not Destroyed -> OnMaxLifetimeExpired) is read in
// the step.
fn spawned(w: &mut World, id: ActorId) {
    let Some(a) = w.actors.actors.get(id) else { return };
    let (f, cls) = (a.owner, a.cls);
    let wb = comp_box(w, f, cls, "WallbounceCollision", 1.0);
    let mut pl = SpawnedPlatform::new(w, f, cls, None, Some((wb.hx, wb.hy)));
    let parts: Vec<PlatformPart> = (0..3).map(|i| make_part(w, f, cls, i)).collect();
    pl.parts.splice(0..0, parts);
    pl.wallbounce = pl.wallbounce.map(|i| i + 3);
    let missing = actor_def(w, ActorRef::Fighter(f), cls).is_none_or(|d| {
        ["Collision", "CollisionMidLeft", "CollisionMidRight"].iter().any(|c| obj(&d.components, c).is_none())
    });
    if missing {
        warn_text(w, &format!("{}: a platform box is not in the class data", w.names.str(cls)));
    }
    if let Some(a) = w.actors.actors.get_mut(id) {
        a.plat = Some(pl);
    }
    if marvin_of(w, f).is_some() {
        set_cannon(w, f, Some(ActorRef::Actor(id)));
    }
    find_valid_spawn_position(w, id);
    sound_at(w, id, name!("sfx_cha_C018_ufo_portal_spawn"), -440.0);
    let intro = pnum(w, id, "TimeTillCollisionsActive").unwrap_or(0.0);
    if let Some(u) = ufo_mut(w, id) {
        u.ship_hidden = true;
        u.intro_at = delay(u, 0.2);
        u.collision_at = delay(u, intro);
    }
    place_parts(w, id);
    if is_perk(w, id) {
        perks::perk_begin(w, id);
    }
}

// ---- the platform's functions

/// `aimAtTargetVisuals` (AimAtTargetVisualsMulticast(Target) @18034): AimTarget = Target, AimOn, RotationDeltaTime 1,
/// the beam shown, IdleMontage through a proxy. Called by perks.rs.
pub fn aim_at_target_visuals(w: &mut World, id: ActorId, target: Option<FighterId>) {
    if let Some(u) = ufo_mut(w, id) {
        u.aim_target = target;
        u.aim_on = true;
        u.rdt = 1.0;
        u.beam_on = true;
    }
    play(w, id, "IdleMontage");
}

/// `findInitalAimTarget` (C018_Platform's own): its Instigator.
pub fn find_inital_aim_target(w: &World, id: ActorId) -> Option<FighterId> {
    owner_of(w, id)
}

/// `afterBlastArmed` (C018_Platform's own): FindInitalAimTarget, AimAtTargetVisualsMulticast(it), then
/// K2_SetTimerDelegate(AfterAimDelay, TimeFromAimToFire).
pub fn after_blast_armed(w: &mut World, id: ActorId) {
    let t = find_inital_aim_target(w, id);
    aim_at_target_visuals(w, id, t);
    let at = timer_at(w, Some(pnum(w, id, "TimeFromAimToFire").unwrap_or(0.0)));
    if let Some(u) = ufo_mut(w, id) {
        u.aim_fire_at = at;
    }
}

/// `rocketSpawnPoint`: RocketSpawnPoint.K2_GetComponentLocation(): ShipMesh (0, -210, -240) + (0, 0, -340),
/// MakeVector(0, Y, Z) @14376. Called by perks.rs.
// Unverified: the ship mesh's own turn (the RLerp of 'Facing Left' @2893) is taken as a yaw, which leaves the point's
// height and lateral place as they are.
pub fn rocket_spawn_point(w: &World, id: ActorId) -> (Real, Real) {
    let Some(u) = ufo(w, id) else { return (0.0, 0.0) };
    (u.x, (u.y - 580.0 * size_k(w, id)))
}

/// `fireBlast` (FireBlastServer, C018_Platform's own, @13770-15089): not Destroyed; the Instigator's Comp_Spawner
/// CompSpawnActor(C018CannonBlast_C) at RocketSpawnPoint, rotation constant, direction Normal(AimerLocation - the point,
/// 1e-4) mirrored in Y unless the Instigator faces left (@14536-14586; `comp_spawn_projectile` mirrors it back by the
/// owner's facing), speed 'Blast Speed'; the attack id 0 from the failed BPI_PlayerState cast (Comp_Spawner's
/// HandleActorPostSpawn puts the owner's attack count in place of 0: the default). Then CurrentCannonAmmo - 1: at <= 0
/// OnMaxLifetimeExpired, else UpdateFireReadyVisualsMulticast(ammo); ReadyToFire false, FireRocketVisualMulticast. The
/// aimer is the tick's: an ActivateObject before AfterBlastArmed aims at the world's origin (AimTarget None).
pub fn fire_blast(w: &mut World, id: ActorId) {
    let Some(u) = ufo(w, id) else { return };
    if u.destroyed {
        return;
    }
    let Some(f) = owner_of(w, id) else { return };
    let (ax, ay) = (u.aimer_x, u.aimer_y);
    let at = rocket_spawn_point(w, id);
    let mut dy = -(ax - at.0);
    let mut dz = ay - at.1;
    if !(facing(w, f) < 0.0) {
        dy = -dy;
    }
    [_, dy, dz] = crate::math::ue::normal([0.0, dy, dz], crate::math::ue::KINDA_SMALL_NUMBER);
    let speed = pnum(w, id, "Blast Speed").unwrap_or(0.0);
    comp_spawn_projectile(
        w,
        f,
        name!("C018CannonBlast_C"),
        at.0,
        at.1,
        [Some(0.0), Some(dy), Some(dz)],
        speed,
        speed,
        None,
    );
    let ammo = match ufo_mut(w, id) {
        Some(u) => {
            u.ammo -= 1.0;
            u.ammo
        }
        None => return,
    };
    if ammo <= 0.0 {
        on_max_lifetime_expired(w, id);
    } else {
        update_fire_ready_visuals(w, id, ammo);
    }
    if let Some(u) = ufo_mut(w, id) {
        u.ready_to_fire = false;
    }
    fire_rocket_visual(w, id);
}

/// UpdateFireReadyVisualsMulticast(Current Value) (@15630): the UI state 'UFO' (CurrentValue -1, MaxValue -1, Timer
/// DelayBeforeReload, inactive), then Delay(DelayBeforeReload) -> @144 the state again with Current Value,
/// CannonFireCount and active.
fn update_fire_ready_visuals(w: &mut World, id: ActorId, value: Real) {
    let d = pnum(w, id, "DelayBeforeReload").unwrap_or(0.0);
    add_ui_state(w, id, -1.0, -1.0, d, false);
    if let Some(u) = ufo_mut(w, id) {
        u.ui_value = value;
        if u.ui_reload_at == 0.0 {
            u.ui_reload_at = delay(u, d);
        }
    }
}

fn add_ui_state(w: &mut World, id: ActorId, current: Real, max: Real, timer: Real, active: bool) {
    let Some(f) = owner_of(w, id) else { return };
    if let Some(u) = ufo_mut(w, id) {
        u.ui_state = current;
    }
    w.fighter_mut(f).controls.ui_state_ops.push(UiStateOp::Add(UiStateParams {
        id: UI_ID,
        state_type: 2,
        display_type: 0,
        current_value: current,
        max_value: max,
        timer,
        icon_material: name!("MI_State_Rocket"),
        active,
        pop_text: Name::NONE,
        auto_remove_time: -1.0,
        subicon: None,
    }));
}

/// `fireRocketVisual` (FireRocketVisualMulticast @17602): AimOn = false, FireRocketMontage through a proxy. Called by
/// perks.rs.
pub fn fire_rocket_visual(w: &mut World, id: ActorId) {
    if let Some(u) = ufo_mut(w, id) {
        u.aim_on = false;
    }
    play(w, id, "FireRocketMontage");
}

/// `reloadRocket` (ReloadRocketMulticast @21184): LoadRocketMontage through a proxy, Delay(DelayBeforeReload) -> @109
/// ReadyToFire and AimAtTargetVisualsMulticast(AimTarget). Called by perks.rs.
// Deviation: C018_Ship_Reload_Montage's AnimNotify_SetSubmeshVisibility (2.83 s: the rocket shown) has no actor handler
// (warned by `actor_montage`); the rocket's visibility is presentation.
pub fn reload_rocket(w: &mut World, id: ActorId) {
    play(w, id, "LoadRocketMontage");
    let d = pnum(w, id, "DelayBeforeReload").unwrap_or(0.0);
    if let Some(u) = ufo_mut(w, id)
        && u.reload_at == 0.0
    {
        u.reload_at = delay(u, d);
    }
}

/// `onMaxLifetimeExpired` (OnMaxLifetimeExpired @20306): the parent's (Delay(DestroyActor_Delay) ->
/// DestroySpawnedActor), Destroyed = true, ExpiredVisualsMulticast, MaxLifetimeTimer cleared. ExpiredVisualsMulticast
/// (@20362): the rocket and the beam hidden, BPI_SetCollisionEnabled(false), ExitMontage through a proxy, the Collapse
/// timeline (presentation), sfx_cha_C018_ufo_idle_loop_stop on itself, Delay(0.5) -> despawn sound, Delay(0.3) -> portal
/// despawn sound (both at the Capsule, pooled). Called by perks.rs.
pub fn on_max_lifetime_expired(w: &mut World, id: ActorId) {
    let d = pnum(w, id, "DestroyActor_Delay").unwrap_or(0.0);
    let Some(f) = owner_of(w, id) else { return };
    let Some(seq) = w.actors.actors.get(id).map(|a| a.seq) else { return };
    {
        let Some(u) = ufo_mut(w, id) else { return };
        if u.life_end_at == 0.0 {
            u.life_end_at = delay(u, d);
        }
        u.destroyed = true;
        u.rocket_hidden = true;
        u.beam_on = false;
        set_collision(u, false);
    }
    play(w, id, "ExitMontage");
    let fighter = w.fighter(f).id;
    w.events.push(Event::SoundStop {
        fighter,
        key: ak_key(seq),
        fade: None,
        ev: Some(name!("sfx_cha_C018_ufo_idle_loop_stop")),
        curve: None,
        cut: false,
    });
    if let Some(u) = ufo_mut(w, id) {
        if u.despawn_sound_at == 0.0 {
            u.despawn_sound_at = delay(u, 0.5);
        }
        if u.portal_despawn_at == 0.0 {
            u.portal_despawn_at = delay(u, 0.3);
        }
        u.life_at = 0.0;
    }
    parts_on_of(w, id);
}

/// DestroyVisualsMulticast (@19652): the death particle, the rocket and the beam hidden, BPI_SetCollisionEnabled(false),
/// DestroyedMontage through a proxy, OutOfHealthDestroyDelay = its SequenceLength, Delay(0.4) -> destroyed sound.
fn destroy_visuals(w: &mut World, id: ActorId) -> Real {
    if let Some(u) = ufo_mut(w, id) {
        u.rocket_hidden = true;
        u.beam_on = false;
        set_collision(u, false);
    }
    play(w, id, "DestroyedMontage");
    if let Some(u) = ufo_mut(w, id)
        && u.destroyed_sound_at == 0.0
    {
        u.destroyed_sound_at = delay(u, 0.4);
    }
    parts_on_of(w, id);
    let Some(f) = owner_of(w, id) else { return 0.0 };
    let key = text(actor_p(w, ActorRef::Actor(id)), "DestroyedMontage").map(str::to_owned);
    let len = key.and_then(|k| {
        w.data.chars.get(w.names.str(w.fighter(f).ch))?.fighter.actor_montages.as_ref()?.get(&k).map(|m| m.length)
    });
    crate::math::narrow(len.unwrap_or_else(|| f64::from(pnum(w, id, "OutOfHealthDestroyDelay").unwrap_or(0.0))))
}

// ---- the step

/// The intro (@615): ShipMesh shown, the beam hidden, IntroMontage through a proxy, CurrentCannonAmmo =
/// CannonFireCount, (GetInCollision off: the piloting's), the rotation (-90 yaw, as it was), with authority the aim
/// timer K2_SetTimerDelegate(AfterBlastArmed, TimeTillAimReady) @1428; then sfx_cha_C018_ufo_rocket_countdown_alarm on
/// itself, sfx_cha_C018_ufo_spawn on the Instigator, sfx_cha_C018_ufo_idle_loop on itself.
fn intro(w: &mut World, id: ActorId) {
    let ammo = pnum(w, id, "CannonFireCount").unwrap_or(0.0);
    let at = timer_at(w, Some(pnum(w, id, "TimeTillAimReady").unwrap_or(0.0)));
    if let Some(u) = ufo_mut(w, id) {
        u.ship_hidden = false;
        u.beam_on = false;
    }
    play(w, id, "IntroMontage");
    if let Some(u) = ufo_mut(w, id) {
        u.ammo = ammo;
        u.aim_ready_at = at;
    }
    sound(w, id, name!("sfx_cha_C018_ufo_rocket_countdown_alarm"));
    let (Some(f), Some(seq)) = (owner_of(w, id), w.actors.actors.get(id).map(|a| a.seq)) else { return };
    let fighter = w.fighter(f).id;
    w.events.push(Event::Sound {
        fighter,
        ev: name!("sfx_cha_C018_ufo_spawn"),
        x: None,
        y: None,
        hit: None,
        pooled: false,
        key: None,
    });
    w.events.push(Event::SoundStart { fighter, ev: IDLE_LOOP, key: ak_key(seq) });
}

/// ReceiveTick (@12791, not piloted): AimerLocation = MakeVector(0, (AimTarget's location + AimTowardInstigatorOffset).YZ)
/// (@12853-13046: a None AimTarget reads as the origin); with AimOn, Set Ship Aim Direction (@16431): the aimer's side
/// of the actor (Y > 0: sim x less) XOR 'Facing Left' -> 'Facing Left' = that side, TurnMontage through a proxy,
/// RotationDeltaTime = 1 - it, the cha_C018_ufo_rotating RTPC (not posted); else RotationDeltaTime += 2 x delta,
/// clamped to [0, 1]. The beam's source points are presentation.
fn aim_tick(w: &mut World, id: ActorId) {
    let Some(u) = ufo(w, id) else { return };
    let t = u.aim_target.map(|t| (w.fighter(t).body.x, w.fighter(t).body.y));
    let o = obj(actor_p(w, ActorRef::Actor(id)), "AimTowardInstigatorOffset");
    let (oy, oz) = (o.and_then(|o| num(o, "Y")).unwrap_or(0.0), o.and_then(|o| num(o, "Z")).unwrap_or(0.0));
    let Some(u) = ufo_mut(w, id) else { return };
    (u.aim_target_x, u.aim_target_y) = t.unwrap_or((0.0, 0.0));
    u.aimer_x = u.aim_target_x - oy;
    u.aimer_y = u.aim_target_y + oz;
    if !u.aim_on {
        return;
    }
    let left = u.aimer_x < u.x;
    if left != u.facing_left {
        u.facing_left = left;
        u.rdt = 1.0 - u.rdt;
        play(w, id, "TurnMontage");
    } else {
        u.rdt = min(max(u.rdt + (C::DELTA_SECONDS * 2.0), 0.0), 1.0);
    }
}

/// The sensors (DisableCollisionCheckMid / MidLeft / MidRight: boxes 144 x 280 at local (0 / -145 / +145, 60), a Custom
/// profile that overlaps WorldStatic alone). A begin overlap of another actor: Increment Collision(the count, the box),
/// an end overlap: Decrement Collision; MidRight's end overlap has no node (a build bug, kept: its box stays off once a
/// solid has met the sensor). Increment: count + 1, at 1 the box's collision off; Decrement: count - 1, at 0 on (3).
// Deviation: the overlap is the box against the stage's WorldStatic solids in the play plane (`static_shapes`: a
// polygon solid by the shape traces' overlap), tested
// once a step behind the latent pass, while the actor's collision is on (SetActorEnableCollision(false) ends the
// overlaps, true begins them again).
fn sensor_tick(w: &mut World, id: ActorId) {
    let Some(u) = ufo(w, id) else { return };
    let (on, ux, uy) = (u.collision_on, u.x, u.y);
    let Some(f) = owner_of(w, id) else { return };
    let Some(cls) = w.actors.actors.get(id).map(|a| a.cls) else { return };
    let k = size_k(w, id);
    let boxes: Vec<PlaneBox> =
        ["DisableCollisionCheckMidLeft", "DisableCollisionCheckMid", "DisableCollisionCheckMidRight"]
            .iter()
            .map(|c| comp_box(w, f, cls, c, k))
            .collect();
    let gs = if on {
        // (`u.cls + ' DisableCollisionCheck'`: the perk platform names its own class)
        let why = if is_perk(w, id) {
            name!("C018_PerkPlatform_C DisableCollisionCheck")
        } else {
            name!("C018_Platform_C DisableCollisionCheck")
        };
        Some(crate::world::stage_query::static_shapes(w, QUERY_WORLDSTATIC, why))
    } else {
        None
    };
    let over = |b: &PlaneBox| -> bool {
        let Some(gs) = &gs else { return false };
        let (l, r, lo, hi) = (ux + b.ox - b.hx, ux + b.ox + b.hx, uy + b.oy - b.hy, uy + b.oy + b.hy);
        gs.boxes.iter().any(|g| {
            l < crate::math::narrow(g.right)
                && r > crate::math::narrow(g.left)
                && lo < crate::math::narrow(g.top)
                && hi > crate::math::narrow(g.bottom)
        }) || gs.polys.iter().any(|q| q.box_overlaps(ux + b.ox, uy + b.oy, b.hx, b.hy))
    };
    let (in_l, in_m, in_r) = (over(&boxes[0]), over(&boxes[1]), over(&boxes[2]));
    let Some(u) = ufo_mut(w, id) else { return };
    if in_l != u.in_l {
        u.in_l = in_l;
        if in_l {
            u.count_l += 1;
            if u.count_l == 1 {
                u.mid_l_on = false;
            }
        } else {
            u.count_l -= 1;
            if u.count_l == 0 {
                u.mid_l_on = true;
            }
        }
    }
    if in_m != u.in_mid {
        u.in_mid = in_m;
        if in_m {
            u.count_mid += 1;
            if u.count_mid == 1 {
                u.col_on = false;
            }
        } else {
            u.count_mid -= 1;
            if u.count_mid == 0 {
                u.col_on = true;
            }
        }
    }
    if in_r != u.in_r {
        u.in_r = in_r;
        if in_r {
            u.count_r += 1;
            if u.count_r == 1 {
                u.mid_r_on = false;
            }
        }
    }
}

/// The class's ComponentHit on Collision / CollisionMidLeft / CollisionMidRight (@19504): BounceMontage (Montage_Play,
/// no proxy, bStopAllMontages false: beside the playing instance, `montage_play_side`) and
/// sfx_shared_foley_misc_ufo_land on itself.
// Deviation: the blocking hits are taken as a fighter's landing on one of the three (a base that was none of them at the
// last step) and a capsule's bounce off one (`wall_bounce_multiplier`), once a step; a walk into a box's side and the
// CMC's other blocking sweeps are not counted.
fn bounce_tick(w: &mut World, id: ActorId) {
    let Some(u) = ufo(w, id) else { return };
    let mut hit = u.bounced;
    let mut standing = u.standing.clone();
    for f in all_fighters(w) {
        let fi = w.fighter(f);
        let on_it = fi.life.dead == 0.0
            && grounded(w, f)
            && matches!(fi.platforms.base, Some(crate::fighter::body::PlatformRef::Spawned(ActorRef::Actor(a), i)) if a == id && i < 3);
        let tid = fi.id;
        let pos = standing.iter().position(|&s| s == tid);
        match pos {
            None if on_it => {
                standing.push(tid);
                hit = true;
            }
            Some(i) if !on_it => {
                standing.remove(i);
            }
            _ => {}
        }
    }
    if let Some(u) = ufo_mut(w, id) {
        u.bounced = false;
        u.standing = standing;
    }
    if !hit {
        return;
    }
    play_side(w, id, "BounceMontage");
    sound(w, id, name!("sfx_shared_foley_misc_ufo_land"));
}

/// `ufoStep`.
fn step(w: &mut World, id: ActorId) {
    let Some(f) = owner_of(w, id) else { return };
    // The Instigator's OnBFDefeated (@21130): not Destroyed -> OnMaxLifetimeExpired.
    // Deviation: read at the actor's step from the owner's defeat (`life.dead`), not at the broadcast.
    if w.fighter(f).life.dead > 0.0 && ufo(w, id).is_some_and(|u| !u.destroyed) {
        on_max_lifetime_expired(w, id);
    }
    let Some(u) = ufo_mut(w, id) else { return };
    u.age += 1;
    platform_step(w, id);
    aim_tick(w, id);
    // The latent pass.
    let Some(u) = ufo(w, id) else { return };
    let a = crate::real(u.age);
    if a == u.intro_at {
        if let Some(u) = ufo_mut(w, id) {
            u.intro_at = 0.0;
        }
        intro(w, id);
    }
    let Some(u) = ufo(w, id) else { return };
    if a == u.collision_at
        && let Some(u) = ufo_mut(w, id)
    {
        u.collision_at = 0.0;
        set_collision(u, true);
    }
    let Some(u) = ufo(w, id) else { return };
    if a == u.reload_at {
        let target = u.aim_target;
        if let Some(u) = ufo_mut(w, id) {
            u.reload_at = 0.0;
            u.ready_to_fire = true;
        }
        aim_at_target_visuals(w, id, target);
    }
    let Some(u) = ufo(w, id) else { return };
    if a == u.ui_reload_at {
        let (value, count) = (u.ui_value, pnum(w, id, "CannonFireCount").unwrap_or(0.0));
        if let Some(u) = ufo_mut(w, id) {
            u.ui_reload_at = 0.0;
        }
        add_ui_state(w, id, value, count, -1.0, true);
    }
    let Some(u) = ufo(w, id) else { return };
    if a == u.destroyed_sound_at {
        if let Some(u) = ufo_mut(w, id) {
            u.destroyed_sound_at = 0.0;
        }
        sound_at(w, id, name!("sfx_cha_C018_ufo_destroyed"), 0.0);
    }
    let Some(u) = ufo(w, id) else { return };
    if a == u.portal_despawn_at {
        if let Some(u) = ufo_mut(w, id) {
            u.portal_despawn_at = 0.0;
        }
        sound_at(w, id, name!("sfx_cha_C018_ufo_portal_despawn"), 0.0);
    }
    let Some(u) = ufo(w, id) else { return };
    if a == u.despawn_sound_at {
        if let Some(u) = ufo_mut(w, id) {
            u.despawn_sound_at = 0.0;
        }
        sound_at(w, id, name!("sfx_cha_C018_ufo_despawn"), 0.0);
    }
    let Some(u) = ufo(w, id) else { return };
    if a == u.destroy_at || a == u.life_end_at {
        if let Some(x) = w.actors.actors.get_mut(id) {
            x.dead = true; // DestroySpawnedActor(false)
        }
        return;
    }
    // CheckBlastBoxTimer (0.1 s, looping, DestroyOnExitedArena?): DestroySpawnedActor(true).
    let doe = flag(actor_p(w, ActorRef::Actor(id)), "DestroyOnExitedArena?");
    if let Some(u) = ufo_mut(w, id) {
        let (x, y) = (u.x, u.y);
        if blast_timer_exit_rounded(doe, &mut u.blast_at, &u.blast_bounds, a - 1.0, x, y) {
            if let Some(x) = w.actors.actors.get_mut(id) {
                x.dead = true;
            }
            return;
        }
    }
    sensor_tick(w, id);
    parts_on_of(w, id);
    bounce_tick(w, id);
}

/// `ufoLate`: the timer pass: MaxLifetimeTimer, the aim timer, AfterAimDelay (@21884: not Destroyed -> FireBlastServer),
/// then the child's own.
fn late(w: &mut World, id: ActorId) {
    let perk = is_perk(w, id);
    let tick = crate::real(w.flow.tick);
    let Some(u) = ufo(w, id) else { return };
    if u.life_at > 0.0 && tick >= u.life_at {
        if let Some(u) = ufo_mut(w, id) {
            u.life_at = 0.0;
        }
        on_max_lifetime_expired(w, id);
    }
    let Some(u) = ufo(w, id) else { return };
    if u.aim_ready_at > 0.0 && tick >= u.aim_ready_at {
        if let Some(u) = ufo_mut(w, id) {
            u.aim_ready_at = 0.0;
        }
        if perk {
            perks::perk_after_blast_armed(w, id);
        } else {
            after_blast_armed(w, id);
        }
    }
    let Some(u) = ufo(w, id) else { return };
    if u.aim_fire_at > 0.0 && tick >= u.aim_fire_at {
        if let Some(u) = ufo_mut(w, id) {
            u.aim_fire_at = 0.0;
        }
        if ufo(w, id).is_some_and(|u| !u.destroyed) {
            if perk {
                perks::perk_fire_blast(w, id);
            } else {
                fire_blast(w, id);
            }
        }
    }
    if perk {
        perks::perk_timers(w, id);
    }
}

/// The proxies' events (@787-1066 and the others): OnCompleted of FireRocketMontage (with ammo left:
/// ReloadRocketMulticast), of TurnMontage (AimAtTargetVisualsMulticast(AimTarget); the rotating RTPC to 0, not posted),
/// of ExitMontage and DestroyedMontage (ShipMesh hidden @2504 / 2678); every other event of every proxy runs nothing.
fn montage_event(w: &mut World, a: ActorRef, ev: MontageEvent, montage: Name) {
    let ActorRef::Actor(id) = a else { return };
    if ev != MontageEvent::Completed {
        return;
    }
    let p = actor_p(w, a);
    let is = |k: &str| text(p, k).is_some_and(|t| t == w.names.str(montage));
    let (fire, turn, exit) = (is("FireRocketMontage"), is("TurnMontage"), is("ExitMontage") || is("DestroyedMontage"));
    if fire {
        if ufo(w, id).is_some_and(|u| u.ammo > 0.0) {
            reload_rocket(w, id);
        }
    } else if turn {
        let t = ufo(w, id).and_then(|u| u.aim_target);
        aim_at_target_visuals(w, id, t);
    } else if exit && let Some(u) = ufo_mut(w, id) {
        u.ship_hidden = true;
    }
}

/// `ufoDamage` (ReceiveAnyDamage @18668): not Destroyed and Damage > 0: CurrentHealth -= Damage; at <= 0 (@2153)
/// Destroyed, DestroyVisualsMulticast and Delay(OutOfHealthDestroyDelay) -> DestroySpawnedActor(false); else
/// MulticastDamageEffects (@18829: HitReactFront / Back by Round(RandomFloat()), Montage_Play with no proxy and
/// bStopAllMontages false @18981: `montage_play_side`; the hit particle) and sfx_cha_C018_ufo_take_hit on itself.
// Deviation: RandomFloat (142f7fcb0) is the CRT rand() x 1 / 32767; the port draws the world's stream
// (`random_float`), as its other RandomFloat reads.
fn any_damage(w: &mut World, id: ActorId, dmg: Real, causer: ActorRef) {
    let _ = causer;
    let Some(u) = ufo(w, id) else { return };
    if u.destroyed || !(dmg > 0.0) {
        return;
    }
    let health = u.health - dmg;
    if let Some(u) = ufo_mut(w, id) {
        u.health = health;
    }
    if health <= 0.0 {
        if let Some(u) = ufo_mut(w, id) {
            u.destroyed = true;
        }
        let t = destroy_visuals(w, id);
        if let Some(u) = ufo_mut(w, id)
            && u.destroy_at == 0.0
        {
            u.destroy_at = delay(u, t);
        }
        return;
    }
    let back = crate::math::ue::round_to_int(w.random_float()) == 1;
    play_side(w, id, if back { "HitReactBackMontage" } else { "HitReactFrontMontage" });
    sound(w, id, name!("sfx_cha_C018_ufo_take_hit"));
}

/// `ufoEnd` (ReceiveEndPlay @11896), however the actor ended: the parent's (its buff children, the death effects of the
/// class data: none here, platformEnd), BPI_SetCannon(null) on the Instigator @12014 (whatever it holds), the focus actor
/// destroyed, BPI_SetCollisionEnabled(true) @12088, with authority the OnBFDefeated unbind and
/// BeginAlternateCooldownOnAuthority(TriggerAbilityIndex, 0, true) @12485, then RemoveStateForPlayer('UFO') @12736. The
/// loop on its own AkComponent stops with the actor (the UI stops the key).
fn end(w: &mut World, id: ActorId) {
    let Some(f) = owner_of(w, id) else { return };
    let seq = w.actors.actors.get(id).map_or(0, |a| a.seq);
    let ev = name_of(w, actor_p(w, ActorRef::Actor(id)), "Ak Death Location");
    if let Some(ev) = ev {
        sound_at(w, id, ev, 0.0);
    }
    if marvin_of(w, f).is_some() {
        set_cannon(w, f, None);
    }
    if let Some(u) = ufo_mut(w, id) {
        set_collision(u, true);
    }
    if !crate::world::flow::reset_pass(w) {
        let slot = pnum(w, id, "TriggerAbilityIndex").unwrap_or(2.0);
        begin_alternate_cooldown(w, f, crate::math::to_int32(slot), 0, true);
    }
    w.fighter_mut(f).controls.ui_state_ops.push(UiStateOp::Remove(UI_ID));
    with(w, id, |_, pl, _| {
        for p in &mut pl.parts {
            p.dead = true;
        }
    });
    let fighter = w.fighter(f).id;
    w.events.push(Event::SoundStop { fighter, key: ak_key(seq), fade: None, ev: None, curve: None, cut: false });
}

/// `size.scale`: the platform boxes and WallbounceCollision grow with the root (`place_parts`'s places).
fn scale(w: &mut World, id: ActorId, k: Real, old: Real) {
    with(w, id, |_, pl, _| {
        for s in &mut pl.parts {
            s.hx *= k / old;
            s.hy *= k / old;
        }
    });
    place_parts(w, id);
}

/// `ufoIgnorePlatform` (BPI_IgnorePlatformCollision, C018_Platform's own): as the parent's, on Collision,
/// CollisionMidLeft and CollisionMidRight at once, Collision's top for the height test.
fn ignore_platform_collision(w: &mut World, id: ActorId, f: FighterId, ignore: bool, vy: Real) {
    let p = actor_p(w, ActorRef::Actor(id));
    let (allies, enemies) = (flag(p, "IgnoreAllies"), flag(p, "IgnoreEnemies"));
    let Some(pl) = w.actors.actors.get(id).and_then(|a| a.plat.as_ref()) else { return };
    let Some(top) = pl.parts.first().map(|c| c.top) else { return };
    let team = pl.team;
    let fm = w.fighter(f);
    let v = ignore
        || (if fm.team.team == team { allies } else { enemies })
        || fm.body.y - crate::fighter::body::cap_half(w, f) + 10.0 <= top
        || vy > 0.0;
    let fid = fm.id;
    with(w, id, |_, pl, _| {
        for p in pl.parts.iter_mut().take(3) {
            p.set_ignored(fid, v);
        }
    });
}

/// `ignoreWallbounceCollision` (BPI_IgnoreWallbounceCollision): the parent's.
fn ignore_wallbounce_collision(w: &mut World, id: ActorId, f: FighterId, ignore: bool) {
    platform_ignore_wallbounce(w, id, f, ignore);
}

/// `wallBounceMultiplier` (BPI_GetWallBounceMultiplier, C018_Platform's): WallBounceMultiplier. The capsule's bounce off
/// one of the three boxes (`part` below 3) marks `bounced` (it is marked in `UfoPart` alone, not in the parent's
/// WallbounceCollision).
fn wall_bounce_multiplier(w: &mut World, id: ActorId, part: u32, f: FighterId) -> Real {
    let _ = f;
    if part < 3
        && let Some(u) = ufo_mut(w, id)
    {
        u.bounced = true;
    }
    pnum(w, id, "WallBounceMultiplier").unwrap_or(1.0)
}

/// `capsulePiece(u, comp, slot, piece)`: one of the hurt capsules' pieces (`DAMAGE_BOXES`): the hurt shapes the
/// fighters' melee hit boxes meet: Hurtbox1 (capsule 260 / 130, upright at the actor) and Hurtbox2 (capsule 280 / 90,
/// pitched 90: along x, at local Z -150), ProjectileDestructible, each as a box over its core and its two end spheres,
/// while the actor's collision is on. The hit boxes of the World.actors dealers that test the actors and the
/// projectiles' and thrown items' hit bodies meet them through the trace gather (`bodies`).
fn capsule_piece<const SLOT: usize, const PIECE: usize>(w: &mut World, id: ActorId) -> Option<WorldSphere> {
    let a = w.actors.actors.get(id)?;
    let u = ufo(w, id)?;
    if a.dead || !u.collision_on {
        return None;
    }
    let comp = ["Hurtbox1", "Hurtbox2"][SLOT];
    let k = plane_capsule(
        actor_def(w, ActorRef::Fighter(a.owner), a.cls).and_then(|d| obj(&d.components, comp)),
        size_k(w, id),
    )?;
    let (cx, cy) = (u.x + k.ox, u.y + k.oy);
    if PIECE == 0 {
        let mut b = [0.0; 9];
        b[0] = if k.flat { k.half } else { k.r };
        b[4] = if k.flat { k.r } else { k.half };
        b[8] = k.r;
        return Some(WorldSphere { x: cx, y: cy, z: Some(0.0), r: 0.0, bx: Some(b), ..WorldSphere::default() });
    }
    let d = if PIECE == 1 { -k.half } else { k.half };
    let (x, y) = if k.flat { (cx + d, cy) } else { (cx, cy + d) };
    Some(WorldSphere { x, y, z: Some(0.0), r: k.r, ..WorldSphere::default() })
}

/// `DAMAGE_BOXES`: Hurtbox1's and Hurtbox2's capsules, three pieces each.
static DAMAGE_BOXES: [DamageBox; 6] = [
    DamageBox { comp: "Hurtbox1", at: capsule_piece::<0, 0> },
    DamageBox { comp: "Hurtbox1", at: capsule_piece::<0, 1> },
    DamageBox { comp: "Hurtbox1", at: capsule_piece::<0, 2> },
    DamageBox { comp: "Hurtbox2", at: capsule_piece::<1, 0> },
    DamageBox { comp: "Hurtbox2", at: capsule_piece::<1, 1> },
    DamageBox { comp: "Hurtbox2", at: capsule_piece::<1, 2> },
];

/// `ufoBodies`: the bodies the traces meet besides the platform boxes: Capsule (ProjectileNavNoPlatform), Hurtbox1 and
/// Hurtbox2 (ProjectileDestructible), with the actor's collision on. GetInCollision (the piloting's, off from the
/// intro) and the sensors (overlap only) are left out.
fn bodies(w: &World, id: ActorId, out: &mut Vec<TraceBody>) {
    let (Some(a), Some(u)) = (w.actors.actors.get(id), ufo(w, id)) else { return };
    if a.dead || !u.collision_on {
        return;
    }
    let def = actor_def(w, ActorRef::Fighter(a.owner), a.cls);
    for name in ["Capsule", "Hurtbox1", "Hurtbox2"] {
        let comp = def.and_then(|d| obj(&d.components, name));
        let Some(k) = plane_capsule(comp, size_k(w, id)) else { continue };
        let Some(info) = body_info(comp, None).filter(|i| i.on) else { continue };
        let (cx, cy) = (u.x + k.ox, u.y + k.oy);
        out.push(TraceBody {
            ax: if k.flat { cx - k.half } else { cx },
            ay: if k.flat { cy } else { cy - k.half },
            bx: if k.flat { cx + k.half } else { cx },
            by: if k.flat { cy } else { cy + k.half },
            hx: 0.0,
            hy: 0.0,
            r: k.r,
            z: Real::NAN,
            object_type: info.kind,
            profile: info.profile,
            component: w.names.get(name).unwrap_or(Name::NONE),
            actor: Some(ActorRef::Actor(id)),
            platform: -1,
            blocks: info.blocks,
            poly: None,
            floor: None,
        });
    }
}

/// `activate` (ActivateObject @15164): the parent's (nothing for this class), then FireBlastServer (the class's own).
fn activate(w: &mut World, id: ActorId) {
    if is_perk(w, id) {
        perks::perk_fire_blast(w, id);
    } else {
        fire_blast(w, id);
    }
}

/// `platformKind(cls, child)`: the entry of C018_Platform_C (`class` its class, no parents) and of a child class
/// (perks.rs: C018_PerkPlatform_C, parents [C018_Platform_C]). The child's functions are found by the actor's class.
pub fn platform_entry(class: WorldActorClass, parents: &'static [&'static str]) -> ActorEntry {
    let hooks = WorldActorHooks {
        size: Some(SizeHooks { scale: Some(scale), ..SizeHooks::default() }),
        spawn: Some(spawn),
        spawned: Some(spawned),
        activate: Some(activate),
        step: Some(step),
        late: Some(late),
        end: Some(end),
        any_damage: Some(any_damage),
        damage_box: &DAMAGE_BOXES,
        bodies: Some(bodies),
        wall_bounce_multiplier: Some(wall_bounce_multiplier),
        ignore_platform_collision: Some(ignore_platform_collision),
        ignore_wallbounce_collision: Some(ignore_wallbounce_collision),
        ..WorldActorHooks::default()
    };
    ActorEntry {
        parents,
        foreign_owner: true,
        spawned_meet_bodies: true,
        mesh: Some(MeshHooks { montage_event: Some(montage_event), ..MeshHooks::default() }),
        ..ActorEntry::new(ActorClass::World(class), ActorHooks::World(hooks))
    }
}

// ---- C018CannonBlast (`cannonBlastKind`)
//
// A ParentProjectilePhysics child: the world's Projectile is the parent (movement, hit, DestroyOnBlocked /
// DestroyOnHitFighter / DestroyOnCollideWithEnvironment, MaxLifetime 10, 'Ak Death Location'
// sfx_cha_C018_ufo_rocket_impact). Its own graph: BeginPlay (the game state's ignorable platforms: its Platform box is
// NoCollision; sfx_cha_C018_ufo_rocket_loop_alarm on itself @410; the grow timeline: presentation),
// BPI_ReverseMoveDirection (the parent's flip @2068, then @1378-1299 BPI_SetFacingLogic(BPI_IsFacingLeft) on each
// BP_FighterCharacter in StandingFightersOverlap: `standing_facing`), HitAlly (the parent's @1349 alone), SetTeamColor
// (presentation).

fn pr_seq_owner(w: &World, pr: ProjectileId) -> Option<(FighterId, i32)> {
    let p = w.actors.projectiles.get(pr)?;
    Some((p.owner?, p.seq))
}

/// `cannonBlastKind.begin`.
fn cannon_blast_begin(w: &mut World, pr: ProjectileId) {
    let Some((f, seq)) = pr_seq_owner(w, pr) else { return };
    let fighter = w.fighter(f).id;
    w.events.push(Event::SoundStart { fighter, ev: name!("sfx_cha_C018_ufo_rocket_loop_alarm"), key: ak_key(seq) });
}

/// `cannonBlastKind.spawnsAtDeath`: false (the kind's destroyed makes the death spawn).
fn cannon_blast_spawns_at_death(_w: &World, _pr: ProjectileId) -> bool {
    false
}

/// `cannonBlastKind.destroyed`: the death spawn's roll (SpawnActorAtDeath, C018RocketExplosion_C, chance 1, the shot's
/// transform): the parent's DestroySpawnedActor roll, made by the kind (`spawn_at_death` handles none of the World.actors
/// classes but C017's).
// Deviation: the roll and the spawn run in the kind's destroyed (ahead of the death sound and VFX the world posts after
// it; the build rolls after them, in the same call).
fn cannon_blast_destroyed(w: &mut World, pr: ProjectileId) {
    let Some(p) = w.actors.projectiles.get(pr) else { return };
    let (x, y, face, blocked) = (p.x, p.y, p.facing, p.destroy_spawn_blocked.unwrap_or(false));
    let Some(f) = p.owner else { return };
    if crate::world::flow::reset_pass(w) || blocked {
        return;
    }
    let pp = actor_p(w, ActorRef::Projectile(pr));
    if !flag(pp, "SpawnActorAtDeath") {
        return;
    }
    let cls = name_of(w, pp, "ActorClassSpawnedAtDeath");
    let chance = num(pp, "SpawnAtDeathChance").unwrap_or(0.0);
    let Some(cls) = cls else { return };
    if !(chance > 0.0) || !(w.random_float() <= chance) {
        return;
    }
    spawn_rocket_explosion(w, f, cls, x, y, face);
}

/// `cannonBlastKind.end`.
fn cannon_blast_end(w: &mut World, pr: ProjectileId) {
    let Some((f, seq)) = pr_seq_owner(w, pr) else { return };
    let fighter = w.fighter(f).id;
    w.events.push(Event::SoundStop { fighter, key: ak_key(seq), fade: None, ev: None, curve: None, cut: false });
}

/// StandingFightersOverlap: BoxExtent (275, 50, 10) at (-11, 0, 80) under the Capsule (the root), no turn of its own: in
/// the play plane a box of half sizes 275 along the rocket's X and 10 along its Z, turned with the root (yaw to the side
/// of the flight, pitch bRotationFollowsVelocity's: Projectile.pitch), all times the root's scale (Projectile.scale).
const SFO_HX: Real = 275.0;
const SFO_HZ: Real = 10.0;
const SFO_X: Real = -11.0;
const SFO_Z: Real = 80.0;

/// The loop @1378-1299: GetOverlappingActors(BP_FighterCharacter) of StandingFightersOverlap, each fighter's
/// BPI_SetFacingLogic(its own BPI_IsFacingLeft). SetRotationLogic (BaseFighter @138216) sets the same rotation and
/// broadcasts FacingChanged whatever the change (as AnimNotify_C023B_FaceTarget's): the broadcast alone. `side`: the
/// root's facing (+1 right) the overlaps were last updated at (the move ahead of the flip).
// Unverified: the box overlaps a fighter's root capsule (the Custom profile's Pawn response); its other bodies are not
// tested, and the fighters are taken in the world's order.
fn standing_facing(w: &mut World, pr: ProjectileId, side: Real) {
    let Some(p) = w.actors.projectiles.get(pr) else { return };
    let turn = |v| crate::math::ue::turn_by_pitch(p.pitch, v);
    let ([ux, uy], [zx, zy], [ox, oz]) = (turn([1.0, 0.0]), turn([0.0, 1.0]), turn([SFO_X, SFO_Z]));
    let (ux, zx) = (side * ux, side * zx); // the rocket's X and Z in the plane
    let k = p.scale;
    let (hx, hz) = (SFO_HX * k, SFO_HZ * k);
    let (cx, cy) = (p.x + side * ox * k, p.y + oz * k);
    for f in all_fighters(w) {
        if w.fighter(f).life.dead != 0.0 {
            continue;
        }
        let (px, py) = (w.fighter(f).body.x, actor_y(w, f));
        let (r, h) = (cap_radius(w, f), max(capsule_half(w, f) - cap_radius(w, f), 0.0));
        if !capsule_over_box(px, py, r, h, cx, cy, (ux, uy), (zx, zy), hx, hz) {
            continue;
        }
        let left = facing(w, f) < 0.0;
        signal_actors(w, f, OwnerSignal::Facing, left);
        signal_bound(w, f, FighterSignal::Facing, left, 0.0, Name::NONE, None);
    }
}

/// An upright capsule (centre, radius, half segment) over a box in the plane (centre, its axes u / z with half sizes
/// hx / hz): the segment's distance to the box below the radius.
#[allow(clippy::too_many_arguments, reason = "the function's parameters")]
fn capsule_over_box(
    px: Real,
    py: Real,
    r: Real,
    h: Real,
    cx: Real,
    cy: Real,
    u: (Real, Real),
    z: (Real, Real),
    hx: Real,
    hz: Real,
) -> bool {
    // the segment in the box's frame
    let lx = |x: Real, y: Real| (x - cx) * u.0 + (y - cy) * u.1;
    let lz = |x: Real, y: Real| (x - cx) * z.0 + (y - cy) * z.1;
    let (ax, az, bx, bz) = (lx(px, py - h), lz(px, py - h), lx(px, py + h), lz(px, py + h));
    let inside = |x: Real, z: Real| x.abs() <= hx && z.abs() <= hz;
    let clamp_d2 = |x: Real, z: Real| {
        let dx = max(x.abs() - hx, 0.0);
        let dz = max(z.abs() - hz, 0.0);
        dx * dx + dz * dz
    };
    let seg_d2 = |qx: Real, qz: Real| {
        let (ex, ez) = (bx - ax, bz - az);
        let l2 = ex * ex + ez * ez;
        let t = if l2 > 0.0 { min(max(((qx - ax) * ex + (qz - az) * ez) / l2, 0.0), 1.0) } else { 0.0 };
        let (dx, dz) = (ax + ex * t - qx, az + ez * t - qz);
        dx * dx + dz * dz
    };
    if inside(ax, az) || inside(bx, bz) {
        return true;
    }
    let mut d2 = min(clamp_d2(ax, az), clamp_d2(bx, bz));
    for (qx, qz) in [(hx, hz), (-hx, hz), (hx, -hz), (-hx, -hz)] {
        d2 = min(d2, seg_d2(qx, qz));
    }
    // a segment that crosses the box (both ends out) meets it at distance 0
    if seg_crosses_box(ax, az, bx, bz, hx, hz) {
        return true;
    }
    d2 < r * r
}

fn seg_crosses_box(ax: Real, az: Real, bx: Real, bz: Real, hx: Real, hz: Real) -> bool {
    let (mut t0, mut t1) = (0.0, 1.0);
    let (ex, ez) = (bx - ax, bz - az);
    for (p, q) in [(-ex, ax + hx), (ex, hx - ax), (-ez, az + hz), (ez, hz - az)] {
        if p == 0.0 {
            if q < 0.0 {
                return false;
            }
            continue;
        }
        let t = q / p;
        if p < 0.0 {
            t0 = max(t0, t);
        } else {
            t1 = min(t1, t);
        }
        if t0 > t1 {
            return false;
        }
    }
    true
}

/// `cannonBlastReverse` (C018CannonBlast's BPI_ReverseMoveDirection @2068: the parent's flip of the velocity, then the
/// loop).
fn cannon_blast_reverse(w: &mut World, pr: ProjectileId) {
    let Some(p) = w.actors.projectiles.get(pr) else { return };
    let side = if p.vx > 0.0 {
        1.0
    } else if p.vx < 0.0 {
        -1.0
    } else {
        p.facing
    };
    crate::combat::reverse::flip_active_velocity(w, ActorRef::Projectile(pr));
    standing_facing(w, pr, side);
}

/// `cannonBlastKind`: C018CannonBlast_C's (with its `reverse_move`) and C018PerkCannonBlast_C's (perks.rs, without).
pub fn cannon_blast_hooks() -> ProjectileHooks {
    ProjectileHooks {
        begin: Some(cannon_blast_begin),
        spawns_at_death: Some(cannon_blast_spawns_at_death),
        destroyed: Some(cannon_blast_destroyed),
        end: Some(cannon_blast_end),
        ..ProjectileHooks::default()
    }
}

// ---- C018RocketExplosion (`rocketExplosionKind`)
//
// A SpawnedActorParent child: HitBoxOn? true (the HitBox capsule 400 / 400 on Scene: a sphere of 400 x HitBoxScale.X, on
// from the spawn), BeginPlay with authority Delay(0.2) -> HitBoxOn? false, UpdateHitbox (@73-139); MaxLifetime 0.5 (->
// Delay(DestroyActor_Delay, 0) -> DestroySpawnedActor); Die With Instigator; DestroyOnExitedArena?. The
// CollisionScaleUpTimeline (MaxVerticalScale 3) is never played: Scene keeps its scale. HitBoxDamage 12 / 2500 / 14,
// HitEachActorOnlyOnce, DestroyProjectiles. The HitBox on the actors that are no fighter (its template's types: a
// projectile's HurtBox, an item, a destructible actor's body) is `hit_box_actor_hits`, after the fighters.

fn rocket_of(w: &World, id: ActorId) -> Option<&RocketExplosion> {
    match &w.actors.actors.get(id)?.state {
        WorldActorState::RocketExplosion(s) => Some(s),
        _ => None,
    }
}

fn rocket_mut(w: &mut World, id: ActorId) -> Option<&mut RocketExplosion> {
    match &mut w.actors.actors.get_mut(id)?.state {
        WorldActorState::RocketExplosion(s) => Some(s),
        _ => None,
    }
}

/// `rocketExplosionShapes`: the HitBox sphere where it is now: the hit tests and the drawing.
fn rocket_shape(w: &World, id: ActorId) -> Option<WorldSphere> {
    let a = w.actors.actors.get(id)?;
    let s = rocket_of(w, id)?;
    Some(WorldSphere { x: s.x, y: s.y, r: s.r * a.size_scale.unwrap_or(1.0), ..WorldSphere::default() })
}

/// `spawnRocketExplosion`: the owner's Comp_Spawner CompSpawnActor(the class) with the shot's transform (attack id: the
/// owner's count), its BeginPlay (the blast box), the AfterSpawnedActor bindings; then its hit box at the spawn place in
/// its spawn tick, as `spawn_at_death` does for C017_CannonExplotion: the hits pass of the step skips whom it hit.
pub fn spawn_rocket_explosion(
    w: &mut World,
    f: FighterId,
    cls: Name,
    x: Real,
    y: Real,
    facing: Real,
) -> Option<ActorId> {
    let data = w.data.clone();
    let Some(d) = actor_data(w, &data, ActorRef::Fighter(f), cls) else {
        warn_text(w, &format!("spawned actor not exported ({})", w.names.str(cls)));
        return None;
    };
    let none = Props::new();
    let hb = obj(&d.components, "HitBox");
    let class = if cls == name!("C018PerkRocketExplosion_C") {
        WorldActorClass::C018PerkRocketExplosion
    } else {
        WorldActorClass::C018RocketExplosion
    };
    let mut s = RocketExplosion {
        hit: HitPacket::from_props(&w.names, obj(&d.p, "HitBoxDamage").unwrap_or(&none), None),
        attack_id: w.fighter(f).decay.attack_count,
        facing,
        hit_on: flag(&d.p, "HitBoxOn?"),
        types: hit_box_types(hb),
        r: (hb.and_then(|c| num(c, "SphereRadius").or_else(|| num(c, "CapsuleRadius"))).unwrap_or(0.0)
            * crate::registry::props::num_in(&d.p, "HitBoxScale", "X").unwrap_or(1.0)),
        off_at: latent_frames(0.2),
        x,
        y,
        ..RocketExplosion::default()
    };
    s.end_age =
        timer_ticks(num(&d.p, "MaxLifetime")) + 1.0 + latent_frames(num(&d.p, "DestroyActor_Delay").unwrap_or(0.0));
    let sb = spawn_box(w);
    let dead = blast_box_loop(
        &sb,
        &mut s.x,
        &mut s.y,
        None,
        flag(&d.p, "ForceSpawnPositionInsideBlastBox"),
        &mut s.blast_bounds,
        flag(&d.p, "DestroyOnExitedArena?"),
    );
    let actor = new_world_actor(w, cls, class, f, dead, WorldActorState::RocketExplosion(s));
    let id = w.actors.actors.insert(actor);
    let charge = instigator_charge(w, f);
    if dead {
        add_actor(w, id, charge, true);
        return Some(id);
    }
    comp_spawn_actor(w, id, charge, f);
    if w.actors.actors.get(id).is_some_and(|a| !a.dead) {
        rocket_hits(w, id);
    }
    Some(id)
}

/// `rocketExplosionKind.step`.
fn rocket_step(w: &mut World, id: ActorId) {
    let doe = flag(actor_p(w, ActorRef::Actor(id)), "DestroyOnExitedArena?");
    let Some(s) = rocket_mut(w, id) else { return };
    s.age += 1;
    let age = crate::real(s.age);
    if age == s.off_at {
        // Delay(0.2) -> HitBoxOn? false, UpdateHitbox (HitBoxesOff: the collision off, the actors hit forgotten)
        s.hit_on = false;
        s.once.clear();
        s.over.clear();
    }
    let mut dead = age >= s.end_age; // MaxLifetime -> DestroySpawnedActor
    if !dead {
        let (x, y) = (s.x, s.y);
        dead = blast_timer_exit_rounded(doe, &mut s.blast_at, &s.blast_bounds, age - 1.0, x, y);
    }
    if dead && let Some(a) = w.actors.actors.get_mut(id) {
        a.dead = true;
    }
}

/// `explosionHits`: the HitBox sphere against the enemies' hurt shapes (FinnGemBoostBlast's loop), then against the
/// actors.
fn rocket_hits(w: &mut World, id: ActorId) {
    let me = ActorRef::Actor(id);
    let Some(s) = rocket_mut(w, id) else { return };
    if !s.hit_on {
        s.over.clear();
        return;
    }
    let (attack_id, hit, sx, sy) = (s.attack_id, s.hit.clone(), s.x, s.y);
    let Some(shape) = rocket_shape(w, id) else { return };
    let Some(f) = owner_of(w, id) else { return };
    let team = w.fighter(f).team.team;
    for vic in match_fighters(w) {
        let vid = w.fighter(vic).id;
        let Some(s) = rocket_of(w, id) else { return };
        if w.fighter(vic).team.team == team || !alive(w, vic) || no_hit(w, vic) || s.hit_set.contains(&vid) {
            continue;
        }
        let Some(vp) = victim_pose(w, vic) else { continue };
        let met = crate::combat::hit::hurt_hit(w, vic, std::slice::from_ref(&shape), &vp).is_some();
        give_vbuf(w, vp);
        if !met {
            continue;
        }
        let (vx, vy) = (w.fighter(vic).body.x, w.fighter(vic).body.y);
        if hit.do_los_check.unwrap_or(false) && los_blocked(w, sx, sy, vx, vy) {
            continue;
        }
        if projectile_blocks(w, &dealer(w, me), vic) {
            continue;
        }
        if invulnerable(w, vic) {
            let tick = w.flow.tick;
            attack_dodged(w, me, vic, attack_id, hit.base_knock_back.unwrap_or(0.0), tick, None, false);
            continue;
        }
        dodge_over(w, me, vic);
        if hit.hit_each_actor_only_once.unwrap_or(false)
            && let Some(s) = rocket_mut(w, id)
        {
            s.hit_set.insert(vid);
        }
        let rows = class_rows(w, me);
        let d = dealer(w, me);
        if apply_projectile_hit(w, &d, vic, &hit, &rows, V2 { x: sx, y: sy }, None, &[], charge_pct(w, me)) {
            actor_hit_enemy(w, &d, vic);
        }
    }
    if w.actors.actors.get(id).is_none_or(|a| a.dead) {
        return;
    }
    let Some(s) = rocket_mut(w, id) else { return };
    let (mut over, mut once, types) = (std::mem::take(&mut s.over), std::mem::take(&mut s.once), s.types.clone());
    hit_box_actor_hits(
        w,
        me,
        &hit,
        &types,
        Some(HitShape::Sphere { x: shape.x, y: shape.y, r: shape.r }),
        &mut over,
        &mut once,
    );
    if let Some(s) = rocket_mut(w, id) {
        s.over = over;
        s.once = once;
    }
}

/// `rocketExplosionKind`: the entry of C018RocketExplosion_C and of C018PerkRocketExplosion_C (perks.rs, parents
/// [C018RocketExplosion_C]). No spawn hook: [`spawn_rocket_explosion`] makes it.
pub fn rocket_explosion_entry(class: WorldActorClass, parents: &'static [&'static str]) -> ActorEntry {
    let hooks = WorldActorHooks {
        // rocketExplosionShapes reads the scale; the view too
        size: Some(SizeHooks::default()),
        step: Some(rocket_step),
        hits: Some(rocket_hits),
        ..WorldActorHooks::default()
    };
    ActorEntry { parents, ..ActorEntry::new(ActorClass::World(class), ActorHooks::World(hooks)) }
}

/// This file's registry entries: C018_Platform_C, C018CannonBlast_C (foreignOwner, its `reverse_move`),
/// C018RocketExplosion_C.
pub fn register(r: &mut Registry) -> Result<(), RegistryError> {
    r.actors.enter(PLATFORM, platform_entry(WorldActorClass::C018Platform, &[]))?;
    let blast = ProjectileHooks { reverse_move: Some(cannon_blast_reverse), ..cannon_blast_hooks() };
    r.actors.enter(
        CANNON_BLAST,
        ActorEntry {
            foreign_owner: true,
            ..ActorEntry::new(ActorClass::Projectile(ProjectileClass::C018CannonBlast), ActorHooks::Projectile(blast))
        },
    )?;
    r.actors.enter(ROCKET_EXPLOSION, rocket_explosion_entry(WorldActorClass::C018RocketExplosion, &[]))
}
