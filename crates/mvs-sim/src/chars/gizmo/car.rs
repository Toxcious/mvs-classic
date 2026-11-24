//! Gizmo's car: the car shell [`CarActor`] (C023A_Car_SpawnedActor's graph, which
//! Parent_Car_SpawnedActor shares: `crate::items::car` holds the item cars), [`GizmoCar`] (C023A_Car_SpawnedActor_C)
//! and the driver's C023A_Car_Buff ([`CarBuffExt`]). No pawn hook; the pawn's `driving` / `car` / `drives` are
//! `GizmoState`'s.
//!
//! The car kind (`carKind`: step, overlap, nonItemPickup, pickupLabel, damageBox, anyDamage, destroyed) is shared with
//! the item cars: its functions are `pub` here and `crate::items::car` enters them with its own make, begin, step and
//! hitBy. `instanceof CarActor` is [`car_of`] / [`car_of_mut`] over the three ProjectileExt variants that hold one.
//!
//! Model. The car is a ParentProjectilePhysics child (SpawnedActorParent), so a Projectile of the world's list (as
//! `chars::bugs::safe`): the parent's movement (ProjectileMovement with bounce 0.1, bRotationFollowsVelocity),
//! blast-box check, spawner list and Die With Instigator. `GizmoState::car` holds it; the view draws it as a
//! projectile kind, its view state in the shell's fields. C023A_Car and C023A_Car_Air spawn it attached
//! (AnimNotify_Spawner AttachToActor: socket carAtt, or fml_un_COM in the air) on Gizmo, who drives it from BeginPlay
//! on (UpdateDriverInfo(true, GetOwner()) @6632): the attached Projectile rides the socket and its movement is off
//! until the detach. HitBoxOn? is false: the class deals its own hits in `overlap` (HitCollision's begin overlap
//! @8859) and in its overheat explosion (@11819, SphereOverlapActors).
//!
//! The graph, by entry:
//! - ReceiveBeginPlay @6101: UpdateDriverInfo(true, owner) (function: attach, C023A_Car_Buff on the driver through
//!   his Comp_State_Manager SpawnBuffActor(stacks 0, 1.0 s, @1322), IsDriven, PickupGate false, HurtCollision overlap
//!   events off, DriverCheck off, the BuffSafty timer 0.2 s -> CarBuff.SetLifeTimer(0), the driver's binds,
//!   UpdateMovementInfo(true, false)), Delay(0.4) @6843 -> SpawnJumpOutGate @465, the honk refill timer
//!   (HonkCooldownTime, looping, authority @3510), sfx car_ignition and car_engine_loop @6898-7028.
//! - ReceiveTick @8474: the engine RTPC, driven -> Wheelie(PoppinWheelie, dt) (function), else the visual gate's close
//!   with WheelieSounds(false, true) -> sfx car_drop.
//! - InputTriggered @7094 (the CarBuff's DriverInput, from the driver's InputPreTriggered): jump (2) or evade (3) with
//!   SpawnJumpOutGate -> DetachCarServer -> @7483 and sfx car_jump_out @7300; special (1) -> SetWheelieServer(true);
//!   attack (0) -> HonkServer @13986. InputReleased @8417: special -> SetWheelieServer(false).
//! - Detach @7483 ([`detach`]), the HitCollision's begin overlap @8859 ([`car_hit_overlap`]), the explosion @11819
//!   ([`explode`]), ReceiveAnyDamage @12333, DriverKnockedBack @12856, OnLand @12309, OnDriverDeath @17779,
//!   BuffDestroyed @17554, DriverTurnEvent -> TurnVisuals @14828, OnHitByAlly @16513, ReceiveEndPlay @7740.
//! - SetOverheatState (function) and NewDriver @8250: Comp_C023A_AllyAttach (`attach.rs`) calls them.
//! - The DriverCheck pickup: its begin / end overlaps @4543 / @5274 -> TrackReachableItems @18189,
//!   BPI_NonItemPickup @6096 -> @2621 -> JumpIn @12117 ([`driver_check`], [`car_non_item_pickup`]).
//!
//! A driver that is no BPI_C023A pawn (a team mate of another class, through the DriverCheck pickup): UpdateDriverInfo
//! attaches the car at his fml_un_COM on the carried pose ([`carry_attach`]) and his CarBuff's
//! BPI_SetBeingCarried(true, true) gives that pose.
//! Deviation: the carried pose leaves out DriverOffset's COM_Override_Location (the fighter's anim,
//! `set_being_carried`).
//! Not ported: the car's own pickup label (SetPickupIcon @3604: `world::items` set_item_ui); ClearOverridePacks
//! (OverridePackPickup has no writer), ClearOverlapsServer (its loop @2218 over OverlappingActors does nothing),
//! UpdateLocality, SetTeamColor, the RTPC and the materials (view state alone).
//! Sim space: x = -UE Y, y = UE Z.

use indexmap::{IndexMap, IndexSet};
use mvs_data::value::Props;

use crate::Real;
use crate::State;
use crate::actors::actor_montage::{ActorAnim, MontageEvent, montage_play};
use crate::actors::projectile::{
    ProjectileExt, activate_movement, attach_parent, attach_to_socket, class_def, class_p, set_friction,
    set_gravity_scale, yaw_x,
};
use crate::actors::{ActorClass, ActorRef, Projectile, ProjectileClass, ProjectileId};
use crate::combat::boxes::WorldSphere;
use crate::combat::buffs::{BuffExt, buff, buff_mut, end_buff, inst_def_of, spawn_buff};
use crate::combat::hits::{
    actor_hit_enemy, apply_projectile_hit, dealer, handler_hit, projectile_blocks, refused_ally_hit,
};
use crate::combat::packet::DataEnum;
use crate::combat::spawned_hits::damage_spawned_actor;
use crate::combat::{BuffClass, BuffRef, DataVec, HitPacket};
use crate::consts::DELTA_SECONDS;
use crate::events::Event;
use crate::fighter::anim::box_pose::scaled_boxes;
use crate::fighter::anim::montages::{start_montage, stop_montage};
use crate::fighter::anim::visibility::{add_invisibility, remove_invisibility};
use crate::fighter::common::{latent_frames, lit, owner_team, warn_text};
use crate::fighter::control::{attack_dodged, char_data, dodge_over};
use crate::fighter::match_side::{SpawnRequest, post_vo};
use crate::fighter::moves::bpi_disable_dash;
use crate::fighter::moves::{Impulse, V2};
use crate::fighter::moves::{
    disable_evade, disable_jump, enable_evade, enable_jump, is_falling, max_accel, set_max_accel,
};
use crate::fighter::traces::{
    OBJ_DAMAGE_DESTRUCTIBLE, OBJ_ITEM_DESTRUCTIBLE, OBJ_PAWN, TraceBody, body_info, sphere_overlap_actors,
};
use crate::fighter::{
    FighterId, actor_y, cap_radius, capsule_half, facing, grounded, in_hitstun, invulnerable, mesh_wx, mesh_wy,
    no_collision,
};
use crate::math::ue::{KINDA_SMALL_NUMBER, finterp_constant_to, fmod, normal, rotate_angle_axis, size2d};
use crate::math::{max, min};
use crate::name;
use crate::names::Name;
use crate::registry::entries::{ActorEntry, ActorHooks, BuffEntry, BuffHooks, MeshHooks, ProjectileHooks};
use crate::registry::props::{flag, num, obj};
use crate::registry::signals::{BuffSignal, BuffSignalArg, FighterSignal};
use crate::registry::{Registry, RegistryError};
use crate::world::World;
use crate::world::flow::reset_pass;
use crate::world::items::track_reachable_actor;
use crate::world::pawns::match_fighters;

/// `CAR`.
pub const CAR: &str = "C023A_Car_SpawnedActor_C";
/// `CAR_BUFF`.
pub const CAR_BUFF: &str = "C023A_Car_Buff_C";
/// `BOUNCE_BUFF`.
const BOUNCE_BUFF: Name = name!("Buff_C023A_Bounce_C");

// ---------------------------------------------------------------- the car's variables

/// `abstract class CarActor extends Projectile`: C023A_Car_SpawnedActor's and Parent_Car_SpawnedActor's variables
/// alike; a child's own variables are its own subclass's. The timers are the car's age they fire at (-1 none; a Real,
/// as `timerTicks` of no time is infinity). `anim`: its SkeletalMesh's montages (`actors::actor_montage`);
/// `input_ui`: IsValid(InputUI), the pickup label's latch (`world::items` set_item_ui).
#[derive(State, Debug)]
pub struct CarActor {
    /// Driver.
    pub driver: Option<FighterId>,
    /// IsDriven.
    pub driven: bool,
    /// CarBuff.
    pub buff: Option<BuffRef>,
    /// DriverFacing (BPI_IsFacingLeft of the driver at the detach).
    pub driver_left: bool,
    /// LaunchVelocity.
    pub lvx: Real,
    pub lvy: Real,
    /// WasMovingOnGround.
    pub was_ground: bool,
    /// KnockedOffGate.
    pub knocked_off_gate: bool,
    /// SpawnJumpOutGate.
    pub jump_out_gate: bool,
    /// PoppinWheelie.
    pub poppin: bool,
    /// RotationAngle.
    pub wheelie: Real,
    /// WeelieVisualGate.
    pub wheelie_gate: bool,
    /// HonkAmmo.
    pub honk_ammo: Real,
    /// HasOverheat.
    pub has_overheat: bool,
    /// HurtCollision's GenerateOverlapEvents.
    pub hurt_on: bool,
    /// HitActors: fighter id -> GetGameTimeInSeconds of the hit.
    pub hit_times: IndexMap<i32, Real>,
    /// The fighters HitCollision overlaps now (its begin events).
    pub inside: IndexSet<i32>,
    /// The driver's grounded state at the car's last tick (OnBFLanded, polled).
    pub drv_ground: bool,
    /// BuffSafty.
    pub safety_at: Real,
    /// Delay(0.4) @6843.
    pub jump_out_at: Real,
    /// FrictionTimer.
    pub friction_at: Real,
    /// OverheatTimer.
    pub overheat_at: Real,
    /// HitActorTimer.
    pub hit_timer_at: Real,
    /// OverheatVisuals' Delay(1.0).
    pub blink_at: Real,
    /// The detach's Delay(0.5) @3425 -> @309: gravity 3, PickupGate, DriverCheck.
    pub grav_at: Real,
    /// The knock's Delay(0.5) -> @404: the same and KnockedOffGate false (@15).
    pub knock_at: Real,
    /// The detach's Delay(0.033) @3370 -> @65: the CarBuff (a new driver's, if one came since) destroyed.
    pub drop_buff_at: Real,
    /// The looping honk timer's next expiry (game seconds of counted deltas).
    pub honk_next: Real,
    /// KnockedBack (the CarBuff's 'knockedBack'), marked by the buff and acted on at the car's tick.
    pub knock_pending: bool,
    /// OnExitedArena (fighterSignal 'exitedArena').
    pub driver_out: bool,
    /// The CarBuff ended by another hand (BuffDestroyed @17554).
    pub buff_gone: bool,
    /// The car's own CarBuff destroy is under way.
    pub dropping: bool,
    /// OverHeatExplosionVisuals: DeathVFX = PS_C023A_CarOverheat @13465.
    pub overheat_vfx: bool,
    /// PickupGate.
    pub pickup_gate: bool,
    /// DriverCheck's collision (QueryOnly or NoCollision).
    pub check_on: bool,
    /// NewDriverTimer (NewDriverCooldownTime): the age it ends at, -1 not active.
    pub new_driver_at: Real,
    /// The fighters whose root capsule DriverCheck overlaps now.
    pub check_in: IndexSet<i32>,
    /// IsValid(InputUI).
    pub input_ui: bool,
    /// CarFields: the engine RTPC's speed.
    pub speed: Real,
    /// HitShake's HitInt.
    pub damage: Real,
    /// The car's age at the last HitShake, -1 none.
    pub shake_age: i32,
    /// Car_Material BlinkAmount.
    pub blink: Real,
    /// DriverTurnEvent count.
    pub turns: i32,
    /// C023A_Car_Damage_Montage (CarMontageMulticast).
    pub anim: ActorAnim,
}

impl Default for CarActor {
    /// The field initial values; `honkAmmo` and `honkNext` are the constructor's ([`car_construct`]).
    fn default() -> Self {
        CarActor {
            driver: None,
            driven: false,
            buff: None,
            driver_left: false,
            lvx: 0.0,
            lvy: 0.0,
            was_ground: false,
            knocked_off_gate: false,
            jump_out_gate: false,
            poppin: false,
            wheelie: 0.0,
            wheelie_gate: false,
            honk_ammo: 0.0,
            has_overheat: false,
            hurt_on: false,
            hit_times: IndexMap::new(),
            inside: IndexSet::new(),
            drv_ground: false,
            safety_at: -1.0,
            jump_out_at: -1.0,
            friction_at: -1.0,
            overheat_at: -1.0,
            hit_timer_at: -1.0,
            blink_at: -1.0,
            grav_at: -1.0,
            knock_at: -1.0,
            drop_buff_at: -1.0,
            honk_next: 0.0,
            knock_pending: false,
            driver_out: false,
            buff_gone: false,
            dropping: false,
            overheat_vfx: false,
            pickup_gate: false,
            check_on: false,
            new_driver_at: -1.0,
            check_in: IndexSet::new(),
            input_ui: false,
            speed: 0.0,
            damage: 0.0,
            shake_age: -1,
            blink: 100.0,
            turns: 0,
            anim: ActorAnim::new(name!("SkeletalMesh")),
        }
    }
}

/// `GizmoCar extends CarActor` (C023A_Car_SpawnedActor_C).
#[derive(State, Debug, Default)]
pub struct GizmoCar {
    pub car: CarActor,
    /// The car's age at the attach to a driver that is no BPI_C023A pawn: the carried pose's loop starts there.
    pub carried_at: i32,
}

/// C023A_Car_Buff's extension (`CarBuffExt`): the driver's MaxAcceleration at its BeginPlay; the car whose CarBuff it
/// is (none until the car's spawnCarBuff writes it).
#[derive(State, Debug, Default)]
pub struct CarBuffExt {
    pub base: Real,
    pub car: Option<ProjectileId>,
}

/// `instanceof CarActor`: the car shell of a Gizmo car or an item car.
pub fn car_of(e: &ProjectileExt) -> Option<&CarActor> {
    match e {
        ProjectileExt::GizmoCar(c) => Some(&c.car),
        ProjectileExt::ItemCar(c) => Some(&c.car),
        ProjectileExt::BatMobileCar(c) => Some(&c.item.car),
        _ => None,
    }
}

/// [`car_of`], to write.
pub fn car_of_mut(e: &mut ProjectileExt) -> Option<&mut CarActor> {
    match e {
        ProjectileExt::GizmoCar(c) => Some(&mut c.car),
        ProjectileExt::ItemCar(c) => Some(&mut c.car),
        ProjectileExt::BatMobileCar(c) => Some(&mut c.item.car),
        _ => None,
    }
}

/// `isCar`: a live entry that is C023A_Car_SpawnedActor (`instanceof GizmoCar`).
pub fn is_car(w: &World, a: ActorRef) -> bool {
    match a {
        ActorRef::Projectile(id) => {
            w.actors.projectiles.get(id).is_some_and(|p| matches!(p.ext, ProjectileExt::GizmoCar(_)))
        }
        _ => false,
    }
}

/// The car's age at the attach, to write (`carriedAt`, each class declares its own).
fn carried_at_mut(e: &mut ProjectileExt) -> Option<&mut i32> {
    match e {
        ProjectileExt::GizmoCar(c) => Some(&mut c.carried_at),
        ProjectileExt::ItemCar(c) => Some(&mut c.carried_at),
        ProjectileExt::BatMobileCar(c) => Some(&mut c.item.carried_at),
        _ => None,
    }
}

fn carried_at(e: &ProjectileExt) -> i32 {
    match e {
        ProjectileExt::GizmoCar(c) => c.carried_at,
        ProjectileExt::ItemCar(c) => c.carried_at,
        ProjectileExt::BatMobileCar(c) => c.item.carried_at,
        _ => 0,
    }
}

// ---------------------------------------------------------------- reads and writes

fn pr(w: &World, id: ProjectileId) -> &Projectile {
    w.actors.projectiles.get(id).expect("a live projectile")
}

fn pm(w: &mut World, id: ProjectileId) -> &mut Projectile {
    w.actors.projectiles.get_mut(id).expect("a live projectile")
}

/// The car shell of the live projectile `id`.
pub(crate) fn car(w: &World, id: ProjectileId) -> &CarActor {
    car_of(&pr(w, id).ext).expect("a car without its state")
}

/// [`car`], to write.
pub(crate) fn car_w(w: &mut World, id: ProjectileId) -> &mut CarActor {
    car_of_mut(&mut pm(w, id).ext).expect("a car without its state")
}

/// The car's owner (a car's Instigator: always a fighter).
fn owner(w: &World, id: ProjectileId) -> FighterId {
    pr(w, id).owner.expect("a car has an owner")
}

/// The car's age as a number (the timers' clock).
fn age(w: &World, id: ProjectileId) -> Real {
    crate::real(pr(w, id).age)
}

/// A number of the class data the graph reads (no default: a missing one warns and reads 0).
fn num_of(w: &World, cls: Name, p: Option<&Props>, key: &str) -> Real {
    p.and_then(|p| num(p, key)).unwrap_or_else(|| {
        warn_text(w, &format!("{} data missing: {key}", w.names.str(cls)));
        0.0
    })
}

/// A number of the car's class data.
pub(crate) fn cfg(w: &World, id: ProjectileId, key: &str) -> Real {
    let p = pr(w, id);
    num_of(w, p.cls, Some(class_p(w, p)), key)
}

/// A game time: `GetGameTimeInSeconds` (the owner's clock).
fn game_time(w: &World, id: ProjectileId) -> Real {
    w.fighter(owner(w, id)).clock.time
}

/// `sound`: an event of the car at its place.
fn sound(w: &mut World, id: ProjectileId, ev: Name) {
    let p = pr(w, id);
    let e = Event::Sound { fighter: p.owner_id, ev, x: Some(p.x), y: Some(p.y), hit: None, pooled: false, key: None };
    w.events.push(e);
}

/// A sound the car pushes on a fighter's own event list (`sound(f.events, car, ev)`): it reaches the world's after the
/// events the world pushed straight.
fn sound_f(w: &mut World, id: ProjectileId, f: FighterId, ev: Name) {
    let p = pr(w, id);
    let e = Event::Sound { fighter: p.owner_id, ev, x: Some(p.x), y: Some(p.y), hit: None, pooled: false, key: None };
    w.fighter_mut(f).events.push(e);
}

/// A sound the driver's fighter raises at his own place (`f.events.push({ type: 'sound', ... x: f.x, y: f.actorY })`).
fn sound_at(w: &mut World, f: FighterId, ev: Name) {
    let e = Event::Sound {
        fighter: w.fighter(f).id,
        ev,
        x: Some(w.fighter(f).body.x),
        y: Some(actor_y(w, f)),
        hit: None,
        pooled: false,
        key: None,
    };
    w.fighter_mut(f).events.push(e);
}

fn is_gizmo(w: &World, f: FighterId) -> bool {
    crate::chars::gizmo::gizmo_of(w, f).is_some()
}

/// The car's alive buff test: `b.frames > 0` of the entry (a buff out of the list has none).
fn buff_live(w: &World, b: BuffRef) -> bool {
    buff(w, b).is_some_and(|x| x.frames > 0.0)
}

// ---------------------------------------------------------------- the boxes

/// A box of the class's construction tree in the actor's frame (X forward, Z up; the actor's yaw is +-90, so X is the
/// play axis): its centre along X and Z, its half extents along X, UE Y and Z.
#[derive(Copy, Clone, Default)]
struct CarBox {
    fwd: Real,
    up: Real,
    hx: Real,
    hy: Real,
    hz: Real,
}

/// `boxIn`.
fn box_in(w: &World, cs: &Props, name: &str, cls: Name) -> CarBox {
    let Some(c) = obj(cs, name) else {
        warn_text(w, &format!("{}: component missing: {name}", w.names.str(cls)));
        return CarBox::default();
    };
    let t = attach_parent(cs, name);
    let (l, s, e) = (obj(c, "RelativeLocation"), obj(c, "RelativeScale3D"), obj(c, "BoxExtent"));
    let n = |p: Option<&Props>, k: &str, d: Real| p.and_then(|p| num(p, k)).unwrap_or(d);
    let loc = [n(l, "X", 0.0) * t.scale[0], n(l, "Y", 0.0) * t.scale[1], n(l, "Z", 0.0) * t.scale[2]];
    let ext = [
        n(e, "X", 0.0) * n(s, "X", 1.0) * t.scale[0],
        n(e, "Y", 0.0) * n(s, "Y", 1.0) * t.scale[1],
        n(e, "Z", 0.0) * n(s, "Z", 1.0) * t.scale[2],
    ];
    let r = &t.rot;
    let at = |k: usize| t.loc[k] + loc[0] * r[k] + loc[1] * r[3 + k] + loc[2] * r[6 + k];
    // the box's extent along the root's axis k (an axis-aligned bound of the turned box: the HitCollision's roll of
    // -0.198 degrees is in it)
    let along = |k: usize| r[k].abs() * ext[0] + r[3 + k].abs() * ext[1] + r[6 + k].abs() * ext[2];
    CarBox { fwd: at(0), up: at(2), hx: along(0), hy: along(1), hz: along(2) }
}

/// `CarStatic`: the three boxes (HitCollision under CarMesh: yaw -90, scale 2; HurtCollision under the root Capsule;
/// DriverCheck under the root Capsule), from the construction tree, read per call.
#[derive(Copy, Clone)]
struct CarStatic {
    hit: CarBox,
    hurt: CarBox,
    chk: CarBox,
}

/// `staticOf`; `None` for a class with no data.
fn static_of(w: &World, p: &Projectile) -> Option<CarStatic> {
    let a = class_def(w, p)?;
    Some(CarStatic {
        hit: box_in(w, &a.components, "HitCollision", p.cls),
        hurt: box_in(w, &a.components, "HurtCollision", p.cls),
        chk: box_in(w, &a.components, "DriverCheck", p.cls),
    })
}

/// A box in the world: its centre and half extents in the play plane.
#[derive(Copy, Clone)]
struct WBox {
    x: Real,
    y: Real,
    hx: Real,
    hy: Real,
}

/// `hitBox`: HitCollision now, in the world.
fn hit_box(w: &World, p: &Projectile) -> Option<WBox> {
    let st = static_of(w, p)?;
    let (s, fw) = (p.scale, yaw_x(p));
    Some(WBox { x: p.x + st.hit.fwd * s * fw, y: p.y + st.hit.up * s, hx: st.hit.hx * s, hy: st.hit.hz * s })
}

/// `checkBox`: DriverCheck's box now, in the world.
fn check_box(w: &World, p: &Projectile) -> Option<WBox> {
    let st = static_of(w, p)?;
    Some(WBox {
        x: p.x + st.chk.fwd * p.scale * yaw_x(p),
        y: p.y + st.chk.up * p.scale,
        hx: st.chk.hx * p.scale,
        hy: st.chk.hz * p.scale,
    })
}

/// A fighter's root capsule against an upright box of the play plane, centre (x, y), half extents (hx, hy); a touch is
/// no overlap (as `chars::bugs::safe`).
// Deviation: the box against the root capsule (Pawn) alone, in the play plane.
fn capsule_in_box(w: &World, v: FighterId, b: WBox) -> bool {
    let r = cap_radius(w, v);
    let half = capsule_half(w, v) - r;
    let (vx, ay) = (w.fighter(v).body.x, actor_y(w, v));
    let dx = max(max(b.x - b.hx - vx, 0.0), vx - (b.x + b.hx));
    let dy = max(max(b.y - b.hy - (ay + half), 0.0), ay - half - (b.y + b.hy));
    size2d([dx, dy]) < r
}

// ---------------------------------------------------------------- the class's own pieces

/// `attachTo`: UpdateDriverInfo(true)'s K2_AttachToComponent and relative location, with no attach parent.
fn attach_to(w: &mut World, id: ProjectileId, d: FighterId) {
    match &pr(w, id).ext {
        // @327-888: K2_AttachToComponent(the driver's main mesh, carAtt for a BPI_C023A driver, SnapToTarget),
        // relative location (0, 0, 0); any other driver: fml_un_COM, on his carried pose (carry_attach).
        ProjectileExt::GizmoCar(_) => {
            if pr(w, id).attached {
                return;
            }
            if is_gizmo(w, d) {
                attach_to_socket(w, id, d, "carAtt", 0.0, 0.0);
            } else {
                carry_attach(w, id, d);
            }
        }
        _ => crate::items::car::item_attach_to(w, id, d),
    }
}

/// `spawnCarBuff`: UpdateDriverInfo(true)'s SpawnBuffActor on the driver (and what the class does with the new buff).
fn spawn_car_buff(w: &mut World, id: ProjectileId, d: FighterId) -> Option<BuffRef> {
    match &pr(w, id).ext {
        // C023A_Car_Buff on the driver (stacks 0, 1.0 s, @1322); CarBuff = it @1406 (the buff's hooks reach the car
        // by it).
        ProjectileExt::GizmoCar(_) => {
            let b = spawn_buff(w, d, name!("C023A_Car_Buff_C"), 1.0, Some(0), Some(d), false, Some(d));
            if let Some(BuffExt::C023ACarBuff(e)) = b.and_then(|b| buff_mut(w, b)).map(|x| &mut x.ext) {
                e.car = Some(id);
            }
            b
        }
        _ => crate::items::car::item_spawn_car_buff(w, id, d),
    }
}

/// `ramPacket`: the HitCollision overlap's packet, written on HitBoxDamage; `sp` the speed the gate read, `vx` the
/// velocity's sim x it read (the driver's, else the car's), `d` the driver.
fn ram_packet(w: &World, id: ProjectileId, sp: Real, vx: Real, d: Option<FighterId>) -> HitPacket {
    match &pr(w, id).ext {
        // alpha = clamp((speed - 800) / 1500); Damage lerp(2, 10), BaseKnockBack lerp(1000, 1250), KnockBackScaling
        // lerp(5, 10), KnockBackDirectionVector (velocity's UE Y > 0 ? 1 : -1, 1) (IgnoreFacing: UE Y, sim -x),
        // AttackDecayType 6 with a driver, else 5.
        ProjectileExt::GizmoCar(_) => {
            let alpha = min(max((sp - 800.0) / 1500.0, 0.0), 1.0);
            let lerp = |a: Real, z: Real| a + alpha * (z - a);
            let mut p = pr(w, id).hit.clone();
            p.damage = Some(lerp(2.0, 10.0));
            p.base_knock_back = Some(lerp(1000.0, 1250.0));
            p.knock_back_scaling = Some(lerp(5.0, 10.0));
            p.knock_back_direction_vector =
                Some(DataVec { x: Some(if vx < 0.0 { 1.0 } else { -1.0 }), y: Some(1.0), z: None });
            p.attack_decay_type = Some(DataEnum::Value(if d.is_some() { 6 } else { 5 }));
            p
        }
        _ => crate::items::car::item_ram_packet(w, id, vx, d),
    }
}

/// `rammed`: after the overlap's successful pfgDoDamage, ahead of HitActors. CarMontageMulticast(
/// C023A_Car_Damage_Montage) (a PlayMontage proxy; its events do nothing the sim reads) and sfx car_hit; the item
/// cars have neither.
fn rammed(w: &mut World, id: ProjectileId) {
    if matches!(pr(w, id).ext, ProjectileExt::GizmoCar(_)) {
        montage_play(w, ActorRef::Projectile(id), name!("C023A_Car_Damage_Montage"), 1.0, 0.0, true);
        sound(w, id, name!("sfx_cha_C023a_car_hit"));
    }
}

/// UpdateDriverInfo(true)'s attach for a driver that is no BPI_C023A pawn (both graphs: C023A_Car @327-888, Parent_Car
/// @281 / @750): K2_AttachToComponent(his main mesh, fml_un_COM, SnapToTarget) and the relative location (0, 0, 60 -
/// AttachOffsetCharList[his class], 75 for a class not in it), over DriverOffset (0, 0, that offset) @1244 laid on the
/// carried pose's fml_un_COM (BPI_COM_Carried_Override(0, DriverOffset) @4453: an additive translation while the
/// CarBuff's BPI_SetBeingCarried has the pose on): the two cancel, and the car sits 60 above the carried pose's
/// fml_un_COM for every class ([`ride`]).
pub(crate) fn carry_attach(w: &mut World, id: ProjectileId, d: FighterId) {
    attach_to_socket(w, id, d, "fml_un_COM", 0.0, 60.0);
    let a = pr(w, id).age;
    if let Some(c) = carried_at_mut(&mut pm(w, id).ext) {
        *c = a;
    }
    ride(w, id);
}

/// The carried pose's fml_un_COM (Base_AnimBP layer root BlendListByBool_16 on IsBeingCarried: the CarBuff's
/// BPI_SetBeingCarried(true, true) gives CarryInt 1, the anim set's CarriedAbove, looped) + 60 x the mesh scale, at
/// the attach and behind each move of the car's tick (kind.moved): Projectile's attached place reads the shown pose.
/// A BPI_C023A driver gets BPI_DrivingCar instead (no carried pose): his own pose's bone, nothing here.
// Unverified: the clip's time counted from the attach (the blend list's 0.05 s blend in and the clip's own start not
// read); the box pose's fml_un_COM in every CarriedAbove clip is the same in each frame, so the time changes nothing.
pub(crate) fn ride(w: &mut World, id: ProjectileId) {
    let p = pr(w, id);
    let Some(d) = car(w, id).driver else { return };
    if !p.attached || is_gizmo(w, d) {
        return;
    }
    let cd = char_data(w, d);
    let clip = cd.fighter.anim_set.get("CarriedAbove").and_then(|v| v.as_str()).map(str::to_owned);
    let anim = clip.as_deref().and_then(|c| cd.boxes.anims.get(c)).map(|a| a.length);
    let com = w.fixed.samplers.get(&w.fighter(d).ch).and_then(|s| s.com);
    let (Some(clip), Some(length), Some(o)) = (clip, anim, com) else {
        let msg = format!(
            "{}: the driver has no CarriedAbove box clip or no fml_un_COM track (the car stays at the bone of his own pose)",
            w.names.str(p.cls)
        );
        return warn_text(w, &msg);
    };
    let o = o as usize;
    // the looping sequence player's time wrap: AdvanceTime's FMath::Fmod
    let t = if length > 0.0 {
        fmod(crate::real(p.age - carried_at(&p.ext)) / 60.0, crate::math::narrow(length))
    } else {
        0.0
    };
    let mut sm = Vec::new();
    if !scaled_boxes(w, d, lit(w, &clip), t, &mut sm) {
        return;
    }
    let mesh_scale = w.fighter(d).scaled.mesh_scale; // `d.ch.meshScale`: at his size scale
    let x = mesh_wx(w, d) + crate::real(sm[o]) * facing(w, d);
    let y = mesh_wy(w, d) + crate::real(sm[o + 1]) + 60.0 * mesh_scale;
    let p = pm(w, id);
    p.x = x;
    p.y = y;
}

// ---------------------------------------------------------------- the call surface

/// `driverInput(w, f)`: the driver's pending input as the car reads it at its tick (GetPendingInputVector @144: the
/// car ticks behind its driver, whose AddMovementInput of the frame the CharacterMovementComponent consumes in the
/// next frame), UE Y in x and UE Z in y (the pawn adds no Z input).
pub fn driver_input(w: &World, f: FighterId) -> (Real, Real) {
    (w.fighter(f).air.move_in, 0.0)
}

/// The hook `carBounce.fn` (`perks.rs`): HandleBounceOnAuthority(Attacker, HitBoxDamage) of the attacker's Bounce
/// buff, its BounceSuccessful.
fn car_bounce(w: &mut World, id: ProjectileId, by: FighterId) -> bool {
    crate::chars::gizmo::perks::car_bounce(w, id, by)
}

// ---------------------------------------------------------------- the movement

/// BPI_SetInitialMovement(Velocity, MaxSpeed, InitialSpeed) and ProjectileMovement.Activate(true): the attached
/// movement becomes the car's own (InitialSpeed is read at InitializeComponent alone: nothing here). A car spawned
/// unattached (Item_CarWheel's) had its movement deactivated by SpawnedActorParent's BeginPlay (no
/// EnableProjectileMovement): the Activate turns it on (C023A_Car's, spawned attached, was never off in the port).
fn set_initial_movement(p: &mut Projectile, vx: Real, vy: Real, max_speed: Real) {
    p.attached = false;
    activate_movement(p);
    p.vx = vx;
    p.vy = vy;
    p.max_speed = max_speed;
}

/// UpdateMovementInfo(IsDriven, KnockedOff) (function).
fn update_movement_info(w: &mut World, id: ProjectileId, driven: bool, knocked_off: bool) {
    if driven {
        // Deactivate (the attach holds it), gravity 1, the friction timer cleared, friction 0.
        set_gravity_scale(pm(w, id), 1.0);
        car_w(w, id).friction_at = -1.0;
        set_friction(pm(w, id), 0.0);
        return;
    }
    let c = car(w, id);
    // SelectVector((0, 1, 0), (0, -1, 0), DriverFacing): UE +Y is sim -x
    let fx = if c.driver_left { -1.0 } else { 1.0 };
    let (lvx, lvy, wheelie, was_ground) = (c.lvx, c.lvy, c.wheelie, c.was_ground);
    if knocked_off {
        // Normal(LaunchVelocity, 1e-4) when VSize(LaunchVelocity) > 50, else the facing; x 1100, MaxSpeed 3000.
        let (mut dx, mut dy) = (fx, 0.0);
        if size2d([lvx, lvy]) > 50.0 {
            let n = normal([0.0, lvx, lvy], KINDA_SMALL_NUMBER);
            dx = n[1];
            dy = n[2];
        }
        set_initial_movement(pm(w, id), dx * 1100.0, dy * 1100.0, 3000.0);
    } else {
        // The facing turned about X by (left ? RotationAngle : -RotationAngle) when WasMovingOnGround (both give an
        // upward Z of sin(RotationAngle)); x FMax(|LaunchVelocity|, MinLaunchSpeed), MaxSpeed 3000.
        let (mut dx, mut dy) = (fx, 0.0);
        if was_ground {
            // RotateAngleAxis(the facing as UE (0, +-1, 0), left ? RotationAngle : -RotationAngle, X) @1041
            let uy = -fx;
            let r = rotate_angle_axis([0.0, uy, 0.0], if fx < 0.0 { wheelie } else { -wheelie }, [1.0, 0.0, 0.0]);
            dx = -r[1];
            dy = r[2];
        }
        let speed = max(size2d([lvx, lvy]), cfg(w, id, "MinLaunchSpeed")); // VSize @1155
        set_initial_movement(pm(w, id), dx * speed, dy * speed, 3000.0);
    }
    // Activate, then the FrictionTimer (FrictionDelayTime) -> UpdateFriction.
    let t = crate::combat::hit::knockback::timer_ticks(Some(cfg(w, id, "FrictionDelayTime")));
    let a = age(w, id);
    car_w(w, id).friction_at = a + t;
}

// ---------------------------------------------------------------- the DriverCheck pickup

/// DriverCheck (Pawn: Overlap) against a fighter's root capsule, with its begin / end overlap handlers.
/// Begin @4543 (authority): NewDriverTimer active -> nothing; a BP_FighterCharacter whose BPI_GetRootCapsule is the
/// overlapping component, with a Comp_Actor_Destructible: same TeamIndex, PickupGate and not IsDriven ->
/// TrackReachableItems(fighter, true) @5208, else (false) @5233. End @5274: the root capsule -> TrackReachableItems(false)
/// @5477. TrackReachableItems @18189: the fighter's Comp_ItemManager.TrackReachableItems(self, add) @18315.
// Deviation: tested where HitCollision is (the overlap hook: a fighter's move, the car's move), unturned by the car's
// pitch.
fn driver_check(w: &mut World, id: ProjectileId, v: FighterId) {
    if w.fighter(v).life.is_minion {
        return; // a minion is no player of the match: no Comp_ItemManager list here (World.reach); not the owner
    }
    let vid = w.fighter(v).id;
    let p = pr(w, id);
    let c = car(w, id);
    let on = c.check_on && !p.dead && !no_collision(w, v) && check_box(w, p).is_some_and(|b| capsule_in_box(w, v, b));
    if on == c.check_in.contains(&vid) {
        return;
    }
    if !on {
        car_w(w, id).check_in.shift_remove(&vid);
        track_reachable_actor(w, v, id, false); // @5477
        return;
    }
    let c = car_w(w, id);
    c.check_in.insert(vid);
    if c.new_driver_at >= 0.0 {
        return; // @4603
    }
    let add =
        w.fighter(v).team.team == w.fighter(owner(w, id)).team.team && car(w, id).pickup_gate && !car(w, id).driven;
    track_reachable_actor(w, v, id, add);
}

/// DriverCheck.SetCollisionEnabled: the overlaps are updated at once: on, a begin for every fighter inside; off, an
/// end for every one it overlapped.
// Unverified: SetCollisionEnabled(NoCollision) taken to end the overlaps at once with their end events, as the
// query-enabled change updates them (the native read covers the query-enabled types).
fn set_driver_check(w: &mut World, id: ProjectileId, on: bool) {
    car_w(w, id).check_on = on;
    if on {
        for v in match_fighters(w) {
            driver_check(w, id, v);
        }
        return;
    }
    let ids: Vec<i32> = car(w, id).check_in.iter().copied().collect();
    for fid in ids {
        car_w(w, id).check_in.shift_remove(&fid);
        if let Some(v) = match_fighters(w).into_iter().find(|&g| w.fighter(g).id == fid) {
            track_reachable_actor(w, v, id, false);
        }
    }
}

/// UpdateDriverInfo(false, ...) (function, the undriven branch @1698): the driver's binds off, HurtCollision's overlap
/// events on (@2031), his CarBuff destroyed (the delegates cleared first: no BuffDestroyed; @2538), the NewDriverTimer
/// (NewDriverCooldownTime, @2598; its end NewDriverCooldown @14256 clears it), ResetWheelieValue (view: RotationAngle
/// stays), Driver null, IsDriven / PoppinWheelie false, UpdateMovementInfo(false, HitOff). Parent_Car_SpawnedActor's
/// has DriverHidden -> the driver's RemoveInvisibility @2601 behind the timer. `in_buff`: the call is inside the
/// CarBuff's own handler, which ends it on its return.
// Deviation: the IgnorePortal tag the branch takes off is not ported: Rick's portal reads the class default's Tags
// (`chars::rick::portal`), so a car nobody drives still never enters a portal.
fn driver_off(w: &mut World, id: ProjectileId, hit_off: bool, in_buff: Option<BuffRef>) {
    car_w(w, id).hurt_on = true;
    let (d, b) = (car(w, id).driver, car(w, id).buff);
    if let (Some(d), Some(b)) = (d, b)
        && Some(b) != in_buff
        && buff_live(w, b)
    {
        car_w(w, id).dropping = true;
        end_buff(w, d, b);
        car_w(w, id).dropping = false;
    }
    if let Some(g) = d.and_then(|d| crate::chars::gizmo::gizmo_mut(w, d))
        && g.drives == Some(id)
    {
        g.drives = None;
    }
    let t = crate::combat::hit::knockback::timer_ticks(Some(cfg(w, id, "NewDriverCooldownTime")));
    let a = age(w, id);
    car_w(w, id).new_driver_at = a + t;
    if let Some(d) = d
        && driver_hidden(w, id)
    {
        remove_invisibility(w, d);
    }
    let c = car_w(w, id);
    c.buff = None;
    c.driver = None;
    c.driven = false;
    c.poppin = false;
    update_movement_info(w, id, false, hit_off);
}

/// The class data's DriverHidden (absent from C023A_Car's data).
fn driver_hidden(w: &World, id: ProjectileId) -> bool {
    let p = pr(w, id);
    flag(class_p(w, p), "DriverHidden")
}

/// UpdateDriverInfo(true, Driver) (function, the driven branch @19-1697): with no attach parent, K2_AttachToComponent
/// and the relative location (the class's attachTo: C023A_Car's carAtt @327-888 for a BPI_C023A driver, the car rides
/// his pose; any other driver `carry_attach`); DriverOffset; the CarBuff on the driver (spawnCarBuff); IsDriven;
/// PickupGate false @1436; HurtCollision overlap events off; DriverCheck off @1480 (its end overlaps: the reach
/// entries go) and ClearOverlapsServer (nothing: the header); the BuffSafty timer 0.2 s; the IgnorePortal tag
/// (Deviation at `driver_off`); the driver's binds; UpdateMovementInfo(true, false); UpdateLocality and
/// BPI_COM_Carried_Override (view); Parent_Car_SpawnedActor's DriverHidden -> the driver's AddInvisibility @4545 last.
/// GizmoState.drives is C023A_Car's alone.
fn driver_on(w: &mut World, id: ProjectileId, d: FighterId) {
    car_w(w, id).driver = Some(d); // (ahead of the attach: Parent_Car's place reads the driver's pose)
    attach_to(w, id, d);
    if is_car(w, ActorRef::Projectile(id))
        && let Some(g) = crate::chars::gizmo::gizmo_mut(w, d)
    {
        g.drives = Some(id);
    }
    let gr = grounded(w, d);
    {
        let c = car_w(w, id);
        c.drv_ground = gr;
        c.driven = true;
        c.hurt_on = false;
    }
    let b = spawn_car_buff(w, id, d);
    car_w(w, id).buff = b;
    car_w(w, id).pickup_gate = false;
    set_driver_check(w, id, false);
    let t = crate::combat::hit::knockback::timer_ticks(Some(0.2));
    let a = age(w, id);
    car_w(w, id).safety_at = a + t + 1.0; // BuffSafty
    update_movement_info(w, id, true, false);
    if driver_hidden(w, id) {
        add_invisibility(w, d);
    }
}

/// NewDriver(Driver) @8250 (Comp_C023A_AllyAttach ActorReachedLogic @1702: a Gizmo reached an undriven car of his
/// team; JumpIn @12192): a BPI_BaseFighter and not IsDriven -> NewDriverMulticast @14277 -> UpdateDriverInfo(true,
/// Driver, false). The owner, a team mate Gizmo (BPI_C023A: the carAtt branch) or a team mate of another class
/// (fml_un_COM); the CarBuff's hooks find the car through the buff (CarBuffExt.car). Parent_Car_SpawnedActor's
/// NewDriver is the same (its attachTo).
pub fn new_driver(w: &mut World, car_id: ProjectileId, d: FighterId) {
    if car(w, car_id).driven || pr(w, car_id).dead {
        return;
    }
    driver_on(w, car_id, d);
}

/// BPI_NonItemPickup(PickUpActor) @6096 -> @2621 (authority) -> JumpIn @12117: a BP_FighterCharacter -> NewDriver
/// @12192, JumpInMontageMulticast @12215 -> @17165: a BPI_C023A pawn plays C023A_Car_JumpIn_Montage (Montage_Play(m,
/// 1.0, 0, 0.0, true) @17384), any other fighter Montage_Stop(0.0) @17499; then sfx car_engine_loop on the car
/// @12238. Parent_Car_SpawnedActor's (JumpIn @11773, JumpInMontageMulticast @16772): Montage_Stop(0.0) on any fighter
/// @16939. Montage_Play on the pawn's mesh is past the branch manager (as `chars::creature::pawn` wall_bounce_effects):
/// the montage replaces the attack slot's and BeginAttack is not set.
// Deviation: what else the port's startMontage does for an attack start rides along (the attack count, the charge
// values, the hit flag reset, the ground / air section choice: the montage has the one section Default).
fn jump_in(w: &mut World, id: ProjectileId, f: FighterId) {
    new_driver(w, id, f);
    if is_car(w, ActorRef::Projectile(id)) && is_gizmo(w, f) {
        start_montage(w, f, name!("C023A_Car_JumpIn_Montage"), &mvs_anim::montage::PROXY_NONE, None);
        w.fighter_mut(f).charge.begin_attack = false;
    } else {
        stop_montage(w, f, Some(0.0), None);
    }
    sound(w, id, name!("sfx_cha_C023a_car_engine_loop"));
}

/// @7483 (DetachCarMulticast; the Server event's argument, Evade, is read nowhere).
fn detach(w: &mut World, id: ProjectileId, in_buff: Option<BuffRef>) {
    let Some(d) = car(w, id).driver else { return };
    pm(w, id).attached = false; // K2_DetachFromActor(KeepWorld)
    sound_f(w, id, d, name!("sfx_cha_C023a_car_engine_loop_stop"));
    let left = facing(w, d) < 0.0;
    car_w(w, id).driver_left = left;
    car_w(w, id).driven = false;
    update_movement_info(w, id, false, false); // @2675, with the LaunchVelocity / WasMovingOnGround of before
    let (dvx, dvy, gr) = (w.fighter(d).body.vx, w.fighter(d).body.vy, grounded(w, d));
    let c = car_w(w, id);
    c.lvx = dvx;
    c.lvy = dvy;
    c.was_ground = gr;
    // NetImpulse((0, Velocity.Y / 3, JumpOffLaunch.Z), XYOverride, ZOverride).
    let jo = obj(class_p(w, pr(w, id)), "JumpOffLaunch").and_then(|j| num(j, "Z")).unwrap_or(0.0);
    w.fighter_mut(d).moves.impulse = Some(Impulse { vx: (dvx / 3.0), vy: jo, xy: true, kind: 0, z: true, d: None });
    driver_off(w, id, false, in_buff);
    // The Sequence @3365-3425 starts both Delays in this frame: Delay(0.033) @3370 -> @65 (the CarBuff destroyed: gone
    // already unless NewDriver came since) and Delay(0.5) @3425 -> @309.
    let a = age(w, id);
    let c = car_w(w, id);
    c.drop_buff_at = a + latent_frames(0.033);
    // A Delay node already pending ignores the call (execDelay): a second detach inside 0.5 s (a NewDriver came
    // since) keeps the first's resume.
    if c.grav_at < a {
        c.grav_at = a + latent_frames(0.5);
    }
    // NewDriverTimer started in the driver's tick, before the car's own update of the frame: the Delay fires in frame
    // F + 29 at the car's tick, the timer in F + 30, one tick behind the count driver_off takes (made inside the
    // car's step), as safety_at.
    c.new_driver_at += 1.0;
}

/// DriverKnockedBack @12856 -> @12871 (DriverKnockedBackMulticast), at the car's tick.
// Deviation: the driver's KnockedBack is carried by his CarBuff ('knockedBack') and acted on at the car's tick, so the
// car rides the socket for the rest of the frame and LaunchVelocity is his velocity once the hit stored its launch.
fn knocked_off(w: &mut World, id: ProjectileId) {
    let Some(d) = car(w, id).driver else { return };
    pm(w, id).attached = false;
    let left = facing(w, d) < 0.0;
    let (dvx, dvy, gr) = (w.fighter(d).body.vx, w.fighter(d).body.vy, grounded(w, d));
    let c = car_w(w, id);
    c.driver_left = left;
    c.knocked_off_gate = true;
    c.lvx = dvx;
    c.lvy = dvy;
    c.driven = false;
    c.was_ground = gr;
    driver_off(w, id, true, None); // UpdateDriverInfo(false, null, true)
    // The CarBuff is gone (its IsValid false: the Delay(0.2) -> @528 finds none), then Delay(0.5) -> @404: gravity 3,
    // PickupGate, @15 KnockedOffGate false.
    let a = age(w, id);
    let c = car_w(w, id);
    if c.knock_at < a {
        c.knock_at = a + latent_frames(0.5); // a pending Delay ignores the call
    }
}

// ---------------------------------------------------------------- the hits

/// The HitCollision's begin overlap @8859 with a fighter.
// Deviation: HitCollision is tested against the fighters' root capsules alone, unturned by the car's pitch
// (bRotationFollowsVelocity), at the car's place of the test; its other actors (a spawned actor's or an item's body:
// the BPI_SpawnedActor branch; a non-fighter's sfx car_physics) are not met.
fn car_hit_overlap(w: &mut World, id: ProjectileId, vic: FighterId) {
    let vid = w.fighter(vic).id;
    let p = pr(w, id);
    let b = if p.dead || w.fighter(vic).life.dead > 0.0 || no_collision(w, vic) { None } else { hit_box(w, p) };
    let Some(b) = b.filter(|&b| capsule_in_box(w, vic, b)) else {
        car_w(w, id).inside.shift_remove(&vid);
        return;
    };
    if !car_w(w, id).inside.insert(vid) {
        return;
    }
    if car(w, id).hit_times.contains_key(&vid) {
        return;
    }
    // A driver's |velocity| > 1400 or his pending input; no driver: the movement's |Velocity| > 1400.
    // Unverified: the pending input is the driver's AddMovementInput of the frame on a fighter's move into the box
    // (overlapMidFrame) and 0 on the test behind the car's move (the build's attached box moves inside the driver's
    // movement tick, after ConsumeInputVector).
    let d = car(w, id).driver;
    let p = pr(w, id);
    let (vx, vy) = match d {
        Some(d) => (w.fighter(d).body.vx, w.fighter(d).body.vy),
        None => (p.vx, p.vy),
    };
    let sp = size2d([vx, vy]);
    let go = match d {
        Some(d) => sp > 1400.0 || (w.overlap_mid_frame && w.fighter(d).air.move_in != 0.0),
        None => sp > 1400.0,
    };
    if !go || car(w, id).knocked_off_gate {
        return;
    }
    let team = w.fighter(owner(w, id)).team.team;
    if w.fighter(vic).team.team == team {
        return; // Comp_Actor_Destructible.TeamIndex
    }
    if car(w, id).has_overheat {
        explode(w, id); // -> @11819
        return;
    }
    // The packet, written on HitBoxDamage (the class's ram_packet).
    let packet = ram_packet(w, id, sp, vx, d);
    pm(w, id).hit = packet.clone();
    let at = V2 { x: b.x, y: b.y };
    let mut ok = false;
    let bkb = packet.base_knock_back.unwrap_or(0.0);
    let kx = packet.knock_back_direction_vector.and_then(|v| v.x).unwrap_or(0.0);
    let tick = w.flow.tick;
    if let Some(d) = d {
        // The driver's Comp_Pawn_HitBoxHandler.pfgDoDamage(HitCollision, ...): his hit (his dealt mods, his hit pause,
        // his attack decay), with its gates ahead of DamageAndLaunch (World.handlerHit).
        if in_hitstun(w, d) || crate::combat::phases::no_hit_by_team(w, team, vic) {
            return;
        }
        if invulnerable(w, vic) {
            attack_dodged(w, ActorRef::Fighter(d), vic, packet.attack_id.unwrap_or(0), bkb, tick, Some(d), false);
            return;
        }
        dodge_over(w, ActorRef::Fighter(d), vic);
        // The combo-resistance (and decay) name: GetComboResAttackNameFromHitbox with the driver as DamageDealer (no
        // SpawnedActorParent) -> Conv_ObjectToString(Hitbox.AttackAnimation), null in the car's packet: "None".
        handler_hit(w, d, vic, &packet, name!("None"), at);
        ok = true; // Unverified: DamageAndLaunch's Hit taken as true (as the melee path)
    } else {
        // the car's own handler (a spawned actor's hit)
        let dl = dealer(w, ActorRef::Projectile(id));
        if crate::combat::phases::no_hit_by_team(w, team, vic) || projectile_blocks(w, &dl, vic) {
            return;
        }
        if invulnerable(w, vic) {
            let aid = pr(w, id).attack_id;
            attack_dodged(w, ActorRef::Projectile(id), vic, aid, bkb, tick, None, false);
            return;
        }
        dodge_over(w, ActorRef::Projectile(id), vic);
        let pct = crate::combat::elem::charge_pct(w, ActorRef::Projectile(id));
        if apply_projectile_hit(w, &dl, vic, &packet, &[], at, Some(V2 { x: -kx, y: 1.0 }), &[], pct) {
            actor_hit_enemy(w, &dl, vic);
            ok = true;
        }
    }
    if !ok {
        return;
    }
    // The class's own (rammed), then HitActors[victim] = now; the HitActorTimer when none runs.
    rammed(w, id);
    let now = game_time(w, id);
    car_w(w, id).hit_times.insert(vid, now);
    if car(w, id).hit_timer_at < 0.0 {
        let t = crate::combat::hit::knockback::timer_ticks(Some(cfg(w, id, "CanHitAgainTime")));
        let a = age(w, id);
        car_w(w, id).hit_timer_at = a + t;
    }
}

/// UpdateHitActorList (function): the entries hit CanHitAgainTime ago or more (or nearly 0 s ago) leave; with some
/// left, the timer again with LocalShortestTime (the shortest time since a hit of those left, as the graph has it).
fn update_hit_actor_list(w: &mut World, id: ProjectileId) {
    let now = game_time(w, id);
    let again = cfg(w, id, "CanHitAgainTime");
    let mut shortest = Real::INFINITY;
    let c = car_w(w, id);
    c.hit_times.retain(|_, t| {
        let dt = now - *t;
        if dt >= again || dt.abs() <= 0.001 {
            false
        } else {
            if dt < shortest {
                shortest = dt;
            }
            true
        }
    });
    let some = !c.hit_times.is_empty();
    let a = age(w, id);
    car_w(w, id).hit_timer_at =
        if some { a + crate::combat::hit::knockback::timer_ticks(Some(shortest)) } else { -1.0 };
}

/// @11819: OverHeatExplosionVisuals (DeathVFX = PS_C023A_CarOverheat), SphereOverlapActors(the actor location,
/// OverHeatExplosionRadius, [Pawn, DamageDestructible, ItemDestructible], self ignored), and for each actor of another
/// team or the driver: Overheat_Damage with KnockBackScaling 1 for the driver, 15 for the rest,
/// KnockBackDirectionVector (car's UE Y >= its UE Y ? -1 : 1, 1), the car's Comp_Pawn_HitBoxHandler.pfgDoDamage(no hit
/// box); then K2_DestroyActor and sfx car_explode at the car.
fn explode(w: &mut World, id: ProjectileId) {
    if pr(w, id).dead {
        return;
    }
    car_w(w, id).overheat_vfx = true;
    let radius = cfg(w, id, "OverHeatExplosionRadius");
    let (cx, cy) = (pr(w, id).x, pr(w, id).y);
    let types = [crate::real(OBJ_PAWN), crate::real(OBJ_DAMAGE_DESTRUCTIBLE), crate::real(OBJ_ITEM_DESTRUCTIBLE)];
    let found = sphere_overlap_actors(w, cx, cy, radius, &types, &[Some(ActorRef::Projectile(id))], -1.0);
    let base = {
        let p = pr(w, id);
        obj(class_p(w, p), "Overheat_Damage").map(|b| HitPacket::from_props(&w.names, b, None))
    }
    .unwrap_or_default();
    let owner_f = owner(w, id);
    let team = w.fighter(owner_f).team.team;
    let me = ActorRef::Projectile(id);
    let tick = w.flow.tick;
    for o in found {
        if pr(w, id).dead {
            break;
        }
        let is_driver = matches!(o, ActorRef::Fighter(f) if Some(f) == car(w, id).driver);
        match o {
            ActorRef::Fighter(vic) => {
                let (ox, vteam) = (w.fighter(vic).body.x, w.fighter(vic).team.team);
                if vteam == team && !is_driver {
                    continue;
                }
                let mut p = base.clone();
                p.knock_back_scaling = Some(if is_driver { 1.0 } else { 15.0 });
                let kx = if cx <= ox { -1.0 } else { 1.0 };
                p.knock_back_direction_vector = Some(DataVec { x: Some(kx), y: Some(1.0), z: None });
                if vteam == team {
                    // the driver, of the car's team: DamageAndLaunch's ForceKnockback branch (World.refusedAllyHit).
                    // Deviation: the dealer is the car's owner with his dealt mods (the car's handler has its own
                    // State Manager).
                    refused_ally_hit(w, owner_f, vic, &p, false, me); // the car deals: the taken pass runs on its own driver too
                    continue;
                }
                let dl = dealer(w, me);
                if w.fighter(vic).life.dead > 0.0
                    || crate::combat::phases::no_hit_by_team(w, team, vic)
                    || projectile_blocks(w, &dl, vic)
                {
                    continue;
                }
                if invulnerable(w, vic) {
                    let aid = pr(w, id).attack_id;
                    attack_dodged(w, me, vic, aid, p.base_knock_back.unwrap_or(0.0), tick, None, false);
                    continue;
                }
                dodge_over(w, me, vic);
                let pct = crate::combat::elem::charge_pct(w, me);
                let (px, py) = (pr(w, id).x, pr(w, id).y);
                if apply_projectile_hit(
                    w,
                    &dl,
                    vic,
                    &p,
                    &[],
                    V2 { x: px, y: py },
                    Some(V2 { x: -kx, y: 1.0 }),
                    &[],
                    pct,
                ) {
                    actor_hit_enemy(w, &dl, vic);
                }
            }
            ActorRef::Projectile(op) => {
                if owner_team(w, o) == team || pr(w, op).dead {
                    continue;
                }
                let mut p = base.clone();
                p.knock_back_scaling = Some(15.0);
                let ox = pr(w, op).x;
                let kx = if cx <= ox { -1.0 } else { 1.0 };
                p.knock_back_direction_vector = Some(DataVec { x: Some(kx), y: Some(1.0), z: None });
                damage_spawned_actor(w, me, op, &p);
            }
            ActorRef::Item(_) => {
                let msg = format!("{}: the overheat explosion met an item (not hit)", w.names.str(pr(w, id).cls));
                warn_text(w, &msg);
            }
            _ => {}
        }
    }
    if !pr(w, id).dead {
        crate::world::spawning::destroy_projectile(w, id, false);
    }
    sound(w, id, name!("sfx_cha_C023a_car_explode"));
}

/// SetOverheatState (function; Comp_C023A_AllyAttach's call): once, HasOverheat, the OverheatTimer (OverheatLifetime)
/// -> OverheatTimeEnd -> @11819, OverheatVisuals, sfx car_overheat at the Capsule, the owner's VO
/// ability_joyride_launch.
pub fn set_overheat_state(w: &mut World, id: ProjectileId) {
    if car(w, id).has_overheat || pr(w, id).dead {
        return;
    }
    let t = crate::combat::hit::knockback::timer_ticks(Some(cfg(w, id, "OverheatLifetime")));
    let a = age(w, id);
    let c = car_w(w, id);
    c.has_overheat = true;
    c.overheat_at = a + t;
    c.blink = 0.25; // OverheatVisuals @13485 (the particle system shown), Delay(1.0) -> @477
    c.blink_at = a + latent_frames(1.0);
    sound(w, id, name!("sfx_cha_C023a_car_overheat"));
    let o = owner(w, id);
    post_vo(w, o, name!("ability_joyride_launch"), None, 1.0);
}

// ---------------------------------------------------------------- the kind

/// Wheelie(PoppinIt?, DeltaSeconds) (function).
fn wheelie(w: &mut World, id: ProjectileId, d: FighterId) {
    let ws = cfg(w, id, "WheeleiSpeed");
    let falling = is_falling(w, d); // DriverMovementComp.IsFalling() @358
    let poppin = car(w, id).poppin;
    let rate = if falling { if poppin { ws * 1.75 } else { ws * 0.5 } } else { ws };
    let (mv, vx, vy) = (w.fighter(d).air.move_in, w.fighter(d).body.vx, w.fighter(d).body.vy);
    let moving = mv != 0.0 || size2d([vx, vy]) > 25.0; // GetPendingInputVector or |Velocity| > 25
    let target = if poppin {
        if falling {
            600.0
        } else if moving {
            50.0
        } else {
            0.0
        }
    } else if falling {
        35.0
    } else {
        0.0
    };
    let r = finterp_constant_to(car(w, id).wheelie, target, DELTA_SECONDS, rate); // FInterpTo_Constant
    let c = car_w(w, id);
    c.wheelie = if r >= 360.0 { 360.0 - r } else { r };
    if c.wheelie_gate && c.wheelie == 0.0 {
        c.wheelie_gate = false; // WheelieSounds(true, false): nothing
    } else if !c.wheelie_gate && c.wheelie != 0.0 {
        c.wheelie_gate = true; // WheelieSounds(false, false)
        sound_f(w, id, d, name!("sfx_cha_C023a_car_boost"));
        let o = owner(w, id);
        post_vo(w, o, name!("emote_laugh"), None, 0.1);
    }
}

/// `carBegin(w, car)`: ReceiveBeginPlay's own part, behind the class's lead: UpdateDriverInfo(true, GetOwner())
/// (@6632; Parent_Car's @5094), Delay(0.4) -> SpawnJumpOutGate (@6843; @494), sfx car_ignition and car_engine_loop
/// (@6898-7028; @5120-5250). (SpawnedActorParent ReceiveBeginPlay's NumberOfActorsAllowed came ahead: the world's
/// parent begin play.)
pub fn car_begin(w: &mut World, id: ProjectileId) {
    let o = owner(w, id);
    driver_on(w, id, o);
    car_w(w, id).jump_out_at = latent_frames(0.4);
    sound(w, id, name!("sfx_cha_C023a_car_ignition"));
    sound(w, id, name!("sfx_cha_C023a_car_engine_loop"));
}

/// `carHonk(car, f)`: the input the driver's CarBuff hands the car, for what both graphs do with it: HonkServer
/// (C023A_Car @13986, Parent_Car @6043 -> @13717): HonkAmmo > 0 -> HonkAmmo - 1, HonkMulticast: sfx car_honk_single
/// at the car.
pub fn car_honk(w: &mut World, id: ProjectileId, f: FighterId) {
    let c = car_w(w, id);
    if c.honk_ammo <= 0.0 {
        return;
    }
    c.honk_ammo -= 1.0;
    sound_f(w, id, f, name!("sfx_cha_C023a_car_honk_single"));
}

/// `carJumpOut(car, b, f)`: DetachCarServer (C023A_Car @7483, Parent_Car @13299 -> @6058) and sfx car_jump_out (@7300;
/// @5837), from inside the CarBuff's own handler `b`, which ends it on its return (UpdateDriverInfo(false)'s CarBuff
/// K2_DestroyActor).
pub fn car_jump_out(w: &mut World, id: ProjectileId, b: BuffRef, f: FighterId) {
    detach(w, id, Some(b));
    sound_f(w, id, f, name!("sfx_cha_C023a_car_jump_out"));
}

/// `carKind.step`: the shell's tick (the driver's broadcasts, the timers, the wheelie); true: it is destroyed.
pub fn car_step(w: &mut World, id: ProjectileId) -> bool {
    // The driver's broadcasts marked since the last tick.
    if car(w, id).driver_out {
        return true; // OnDriverDeath @17779: K2_DestroyActor
    }
    if car(w, id).knock_pending {
        car_w(w, id).knock_pending = false;
        if car(w, id).driven {
            knocked_off(w, id);
        }
    }
    if car(w, id).buff_gone {
        // BuffDestroyed @17554: UpdateDriverInfo(false, null, false) with no detach.
        // Deviation: the build leaves the car attached with its movement active; the port detaches it.
        let c = car_w(w, id);
        c.buff_gone = false;
        c.buff = None;
        if c.driven {
            driver_off(w, id, false, None);
        }
    }
    let d = if car(w, id).driven { car(w, id).driver } else { None };
    // OnLand @12309 (the driver's OnBFLanded, polled at the car's tick): RotationAngle 0.
    if let Some(d) = d {
        let gr = grounded(w, d);
        let c = car_w(w, id);
        if gr && !c.drv_ground {
            c.wheelie = 0.0;
        }
        c.drv_ground = gr;
    }
    // ReceiveTick @8474: the engine RTPC (|the driver's Velocity.Y|), then the wheelie.
    car_w(w, id).speed = d.map_or(0.0, |d| w.fighter(d).body.vx.abs());
    if let Some(d) = d {
        wheelie(w, id, d);
    } else if car(w, id).wheelie_gate {
        car_w(w, id).wheelie_gate = false; // WheelieSounds(false, true) -> sfx car_drop
        sound(w, id, name!("sfx_cha_C023a_car_drop"));
    }
    // Its latent actions and timers.
    // Deviation: the car's timers are taken at its tick, not in the frame's timer pass.
    let a = age(w, id);
    let c = car(w, id);
    if a == c.safety_at
        && let Some(b) = c.buff
        && let Some(x) = buff_mut(w, b)
    {
        x.frames = Real::INFINITY; // CarBuff.SetLifeTimer(0)
    }
    let c = car_w(w, id);
    if a == c.jump_out_at {
        c.jump_out_gate = true; // @465
    }
    if a == c.blink_at {
        c.blink = 0.05; // @477
    }
    if a == car(w, id).drop_buff_at {
        // @65-272: a valid CarBuff loses its DriverInput binds and is destroyed (its BuffDestroyed bind gone first @88).
        let (b, d) = (car(w, id).buff, car(w, id).driver);
        if let (Some(b), Some(d)) = (b, d)
            && buff_live(w, b)
        {
            car_w(w, id).dropping = true;
            end_buff(w, d, b);
            car_w(w, id).dropping = false;
        }
    }
    for knock in [false, true] {
        let c = car(w, id);
        if a != if knock { c.knock_at } else { c.grav_at } {
            continue;
        }
        // @309 / @404: gravity 3, PickupGate @354 / @449, (after a knock @15 KnockedOffGate false) DriverCheck
        // QueryOnly @365 / @26, whether a driver came since or not.
        set_gravity_scale(pm(w, id), 3.0);
        car_w(w, id).pickup_gate = true;
        if knock {
            car_w(w, id).knocked_off_gate = false; // @15
        }
        set_driver_check(w, id, true);
    }
    // NewDriverTimer's end (NewDriverCooldown @14256). A Delay(0.5) and a 0.5 s timer started in frame F fire in F +
    // 29 (the Delay, at the owner's tick) and F + 30 (the timer): the timer is still active when the Delay turns
    // DriverCheck on, so a fighter inside it then is refused (@4603) and has to leave it and come back.
    // Deviation: the tick of the end, as the car's other timers (taken at its tick).
    let c = car_w(w, id);
    if c.new_driver_at >= 0.0 && a >= c.new_driver_at {
        c.new_driver_at = -1.0;
    }
    if a == c.friction_at {
        c.friction_at = -1.0; // UpdateFriction (function)
        if !c.driven {
            set_friction(pm(w, id), 0.2);
        }
    }
    // The honk refill (UpdateHonkCount, looping HonkCooldownTime): the looping timer adds its rate to its expiry.
    if crate::consts::clock(a - 1.0) > f64::from(car(w, id).honk_next) {
        let rate = cfg(w, id, "HonkCooldownTime");
        let cap = cfg(w, id, "HonkAmmoMax");
        let c = car_w(w, id);
        c.honk_next += rate;
        if c.honk_ammo < cap {
            c.honk_ammo += 1.0;
        }
    }
    if a == car(w, id).hit_timer_at {
        update_hit_actor_list(w, id);
    }
    if a == car(w, id).overheat_at {
        explode(w, id); // OverheatTimeEnd -> @11819 (destroys the car itself)
    }
    false
}

/// `carKind.overlap`: DriverCheck and the HitCollision overlap.
pub fn car_overlap(w: &mut World, id: ProjectileId, vic: FighterId) {
    driver_check(w, id, vic);
    car_hit_overlap(w, id, vic);
}

/// `carKind.nonItemPickup`: jumpIn.
pub fn car_non_item_pickup(w: &mut World, id: ProjectileId, f: FighterId) {
    jump_in(w, id, f);
}

/// `carKind.damageBox`: the HurtCollision box (ItemDestructible, HitBox Overlap) while its overlap events are on
/// (undriven).
pub fn car_damage_box(w: &World, id: ProjectileId) -> Option<WorldSphere> {
    let p = pr(w, id);
    let st = static_of(w, p)?;
    if !car(w, id).hurt_on || p.no_collision {
        return None;
    }
    Some(WorldSphere {
        x: p.x + st.hurt.fwd * p.scale * yaw_x(p),
        y: p.y + st.hurt.up * p.scale,
        z: Some(0.0),
        r: 0.0,
        bx: Some([0.0, 0.0, st.hurt.hy * p.scale, st.hurt.hx * p.scale, 0.0, 0.0, 0.0, st.hurt.hz * p.scale, 0.0]),
        ..WorldSphere::default()
    })
}

/// `carKind.anyDamage`: ReceiveAnyDamage @12333: sfx car_damage, HitShake(RandomIntegerInRange(0, 2)); driven: nothing
/// more; overheated: OverheatTimeEnd (the explosion); else Health -= Damage, at 0 or below K2_DestroyActor and sfx
/// car_explode. True: it is destroyed.
pub fn car_any_damage(w: &mut World, id: ProjectileId, dmg: Real, _by: ActorRef) -> bool {
    sound(w, id, name!("sfx_cha_C023a_car_damage"));
    // Deviation: FMath::Rand taken from the world stream (as `chars::bugs::safe`).
    let dmg_roll = w.random_int(3.0);
    let a = pr(w, id).age;
    let c = car_w(w, id);
    c.damage = dmg_roll;
    c.shake_age = a;
    if c.driven {
        return false;
    }
    if c.has_overheat {
        explode(w, id);
        return false;
    }
    let p = pm(w, id);
    p.health -= dmg;
    if p.health > 0.0 {
        return false;
    }
    sound(w, id, name!("sfx_cha_C023a_car_explode"));
    true
}

/// `carKind.destroyed`: ReceiveEndPlay @7740: a CarBuff still valid and not being destroyed loses its delegates and is
/// destroyed, with sfx car_explode; ClearOverridePacks, ClearOverlapsServer; @2472 the owner's
/// BeginAlternateCooldownOnAuthority(1, 0, true); Parent_Car's (@6315) then DriverHidden -> the driver's
/// RemoveInvisibility @6718. DeathVFX as OverHeatExplosionVisuals left it.
pub fn car_destroyed(w: &mut World, id: ProjectileId) {
    let (d, b) = (car(w, id).driver, car(w, id).buff);
    if let (Some(d), Some(b)) = (d, b)
        && buff_live(w, b)
        && !car(w, id).dropping
    {
        car_w(w, id).dropping = true;
        end_buff(w, d, b);
        car_w(w, id).dropping = false;
        sound(w, id, name!("sfx_cha_C023a_car_explode"));
    }
    car_w(w, id).buff = None;
    if let Some(g) = d.and_then(|d| crate::chars::gizmo::gizmo_mut(w, d))
        && g.drives == Some(id)
    {
        g.drives = None;
    }
    // The DriverCheck's overlaps end with the actor (their end events: the reach entries go). Not in the reset's pass
    // (the reset clears the reach lists), as `world::items` destroy_item.
    // Unverified: the end events at the destroy taken as fired (UWorld::DestroyActor's overlap clear, not read).
    let reset = reset_pass(w);
    if !reset {
        set_driver_check(w, id, false);
    }
    // Inside the Lab reset's pass the call into the pawn is left out (the build's reaches the old pawn), as
    // `chars::marvin::bubble` and `chars::garnet::marker`.
    let o = owner(w, id);
    if !reset {
        crate::fighter::control::begin_alternate_cooldown(w, o, 1, 0, true);
    }
    if let Some(d) = d
        && driver_hidden(w, id)
        && !reset
    {
        remove_invisibility(w, d);
    }
    if let Some(g) = crate::chars::gizmo::gizmo_mut(w, o)
        && g.car == Some(id)
    {
        g.car = None;
    }
    if car(w, id).overheat_vfx {
        // the parent's death effect with this DeathVFX (the world's destroy_projectile, at the actor, no offset)
        let p = pm(w, id);
        p.destroy_fx_blocked = Some(true);
        let e = Event::ProjectileDeath {
            cls: p.cls,
            fighter: p.owner_id,
            vfx: Some(name!("PS_C023A_CarOverheat")),
            x: p.x,
            y: p.y,
            ox: 0.0,
            rot: [0.0; 3],
            scale: 1.0,
            tint: None,
            to: None,
        };
        w.events.push(e);
    }
}

/// `carBodies`: the HurtCollision as a body of the traces (ItemDestructible; a query meets it whatever its overlap
/// events); the item cars' too.
pub fn car_bodies(w: &World, a: ActorRef, out: &mut Vec<TraceBody>) {
    let ActorRef::Projectile(id) = a else { return };
    let p = pr(w, id);
    let comp = class_def(w, p).and_then(|d| obj(&d.components, "HurtCollision"));
    let info = body_info(comp, None).filter(|i| i.on);
    let (Some(info), Some(st)) = (info, static_of(w, p)) else { return };
    if p.dead || p.no_collision {
        return;
    }
    let x = p.x + st.hurt.fwd * p.scale * yaw_x(p);
    let y = p.y + st.hurt.up * p.scale;
    out.push(TraceBody {
        ax: x,
        ay: y,
        bx: x,
        by: y,
        hx: st.hurt.hx * p.scale,
        hy: st.hurt.hz * p.scale,
        r: 0.0,
        z: Real::NAN,
        object_type: info.kind,
        profile: info.profile,
        component: name!("HurtCollision"),
        actor: Some(a),
        platform: -1,
        blocks: info.blocks,
        poly: None,
        floor: None,
    });
}

/// `carSignal`: the driver's FacingChanged (DriverTurnEvent -> TurnVisuals @14828: driven -> the VFX and sfx
/// car_turn) and OnExitedArena (OnDriverDeath @17779: K2_DestroyActor, at the car's tick), bound while he drives; the
/// item cars' too.
#[allow(clippy::too_many_arguments, reason = "the hook's parameters")]
pub fn car_signal(
    w: &mut World,
    id: ProjectileId,
    f: FighterId,
    sig: FighterSignal,
    _flag: bool,
    _num: Real,
    _tag: Name,
    _by: Option<FighterId>,
) {
    let c = car(w, id);
    if !c.driven || c.driver != Some(f) {
        return;
    }
    match sig {
        FighterSignal::Facing => {
            car_w(w, id).turns += 1;
            sound_f(w, id, f, name!("sfx_cha_C023a_car_turn"));
        }
        FighterSignal::ExitedArena => car_w(w, id).driver_out = true,
        _ => {}
    }
}

/// The constructor's tail (`CarActor`'s constructor: HonkAmmo and the honk timer's first expiry from the class data).
pub(crate) fn car_construct(w: &World, p: &mut Projectile) {
    let cls = p.cls;
    let props = Some(class_p(w, p));
    let ammo = num_of(w, cls, props, "HonkAmmo");
    let next = num_of(w, cls, props, "HonkCooldownTime");
    if let Some(c) = car_of_mut(&mut p.ext) {
        c.honk_ammo = ammo;
        c.honk_next = next;
    }
}

// ---------------------------------------------------------------- C023A_Car_SpawnedActor_C

/// The car's `make`.
fn gizmo_car_make(_w: &World, _f: FighterId, _r: &SpawnRequest) -> ProjectileExt {
    ProjectileExt::GizmoCar(GizmoCar::default())
}

/// The car's `moved` (`car.ride()`).
fn gizmo_car_moved(w: &mut World, id: ProjectileId) {
    ride(w, id);
}

/// The car's `begin`.
fn gizmo_car_begin(w: &mut World, id: ProjectileId) {
    let o = owner(w, id);
    if let Some(g) = crate::chars::gizmo::gizmo_mut(w, o) {
        g.car = Some(id);
    } else {
        let msg = format!("{CAR} spawned by a pawn other than Gizmo: GizmoState.car not written");
        warn_text(w, &msg);
    }
    // UpdateDriverInfo(true, GetOwner()) @6632 (the spawner attached it: no attach of its own).
    if !pr(w, id).attached {
        let msg = format!("{CAR} spawned unattached: the attach of UpdateDriverInfo is not ported");
        warn_text(w, &msg);
    }
    car_begin(w, id);
}

/// The car's `hitBy` (OnHitByAlly @16513): an attacker that is the car's Instigator and has Buff_C023A_Bounce -> its
/// HandleBounceOnAuthority(Attacker, HitBoxDamage); BounceSuccessful -> OverheatTimeEnd.
#[allow(clippy::too_many_arguments, reason = "the hook's parameters")]
fn gizmo_car_hit_by(
    w: &mut World,
    id: ProjectileId,
    ally: bool,
    _dx: Real,
    _dy: Real,
    _bkb: Real,
    _attacker_type: i32,
    by: Option<ActorRef>,
) {
    if !ally {
        return;
    }
    let Some(by) = by else {
        let msg = format!(
            "{CAR} OnHitByAlly: the attacker of an ally hit is not given (melee): the Bounce check is not made"
        );
        return warn_text(w, &msg);
    };
    let ActorRef::Fighter(f) = by else { return };
    if f != owner(w, id) || !w.fighter(f).buffs.buffs.iter().any(|b| b.cls == BOUNCE_BUFF && b.frames > 0.0) {
        return;
    }
    if car_bounce(w, id, f) {
        explode(w, id);
    }
}

/// CarMontageMulticast's PlayMontage proxy: its events lead nowhere in the graph (`mesh: { montageEvent: () => {} }`).
fn car_montage_event(_w: &mut World, _a: ActorRef, _e: MontageEvent, _montage: Name) {}

// ---------------------------------------------------------------- C023A_Car_Buff_C

/// The car the driver `f` drives now with the CarBuff `b`.
fn driven_with(w: &World, f: FighterId, b: BuffRef) -> Option<ProjectileId> {
    let Some(BuffExt::C023ACarBuff(e)) = buff(w, b).map(|x| &x.ext) else { return None };
    let id = e.car?;
    let p = w.actors.projectiles.get(id)?;
    let c = car_of(&p.ext)?;
    (!p.dead && c.driven && c.driver == Some(f) && c.buff == Some(b)).then_some(id)
}

/// C023A_Car_Buff_C ReceiveBeginPlay: the owner's InputPreTriggered / InputReleased binds (DriverInputs /
/// DriverInputsRelease), DisableEvade @353, DisableJump @489, MaxAcceleration (BaseMaxAceleration kept) =
/// ModifiedAcelerationSpeed, DisableDash @865, BlockFastFall, sfx car_jump_in and car_honk on the owner,
/// AddActiveAttack @1411, the three ThruATK flags, then BPI_DrivingCar(true) on a BPI_C023A owner (@2192), else
/// BPI_SetBeingCarried(true, true) @2326 (the carried pose: the car's place reads it, [`ride`]).
fn car_buff_begin(w: &mut World, f: FighterId, b: BuffRef, _from: Option<FighterId>, _dur: Option<Real>) {
    let base = max_accel(w, f);
    if let Some(x) = buff_mut(w, b) {
        x.ext = BuffExt::C023ACarBuff(CarBuffExt { base, car: None });
    }
    disable_evade(w, f);
    disable_jump(w, f);
    let cls = w.names.str(name!("C023A_Car_Buff_C")).to_owned();
    let accel = {
        let d = inst_def_of(w, b);
        d.as_deref().and_then(|d| num(d, "ModifiedAcelerationSpeed"))
    }
    .unwrap_or_else(|| {
        warn_text(w, &format!("{cls} data missing: ModifiedAcelerationSpeed"));
        0.0
    });
    set_max_accel(w, f, accel);
    bpi_disable_dash(w, f, true);
    sound_at(w, f, name!("sfx_cha_C023a_car_jump_in"));
    sound_at(w, f, name!("sfx_cha_C023a_car_honk"));
    crate::fighter::add_active_attack(w, f);
    if let Some(g) = crate::chars::gizmo::gizmo_mut(w, f) {
        g.driving = true;
    } else {
        crate::fighter::anim::set_being_carried(w, f, true, true); // @2326
    }
}

/// C023A_Car_Buff_C tick: InputReleased (DriverInputsRelease -> the car's @8417: special -> SetWheelieServer(false)).
// Deviation: the owner's InputReleased is polled at the buff's tick (the special button's falling edge).
fn car_buff_tick(w: &mut World, f: FighterId, b: BuffRef) -> bool {
    if let Some(id) = driven_with(w, f, b) {
        let c = &w.fighter(f).controls;
        if c.prev.special && !c.inp.special {
            car_w(w, id).poppin = false;
        }
    }
    false
}

/// C023A_Car_Buff_C on (knockedBack, inputPreTriggered): true: the buff handled the input.
fn car_buff_on(w: &mut World, f: FighterId, b: BuffRef, s: BuffSignal, a: &BuffSignalArg) -> bool {
    let Some(id) = driven_with(w, f, b) else { return false };
    if s == BuffSignal::KnockedBack {
        car_w(w, id).knock_pending = true; // DriverKnockedBack @12856 (the car's own bind of the driver's KnockedBack)
        return false;
    }
    if s != BuffSignal::InputPreTriggered {
        return false;
    }
    // DriverInputs -> DriverInputServer -> broadcast DriverInput(InputType, 0) -> the car's InputTriggered @7094.
    let t = a.input;
    if (t == Some(2) || t == Some(3)) && car(w, id).jump_out_gate {
        car_jump_out(w, id, b, f);
        return true;
    }
    if t == Some(1) {
        car_w(w, id).poppin = true; // SetWheelieServer(true)
    }
    if t == Some(0) {
        car_honk(w, id, f);
    }
    false
}

/// C023A_Car_Buff_C ReceiveEndPlay: the reverse (MaxAcceleration = BaseMaxAceleration @2589, DisableDash(false),
/// BlockFastFall(false), RemoveActiveAttack, the ThruATK flags, BPI_DrivingCar(false) @3878 (any other owner
/// BPI_SetBeingCarried(false, false) @4028), EnableEvade @4167 / EnableJump), the binds off. A CarBuff the car did not
/// destroy itself: its BuffDestroyed (@17554) at the car's tick.
fn car_buff_end(w: &mut World, f: FighterId, b: BuffRef) {
    enable_evade(w, f);
    enable_jump(w, f);
    let base = match buff(w, b).map(|x| &x.ext) {
        Some(BuffExt::C023ACarBuff(e)) => e.base,
        _ => max_accel(w, f),
    };
    set_max_accel(w, f, base);
    bpi_disable_dash(w, f, false);
    crate::fighter::remove_active_attack(w, f);
    if let Some(g) = crate::chars::gizmo::gizmo_mut(w, f) {
        g.driving = false;
    } else {
        crate::fighter::anim::set_being_carried(w, f, false, false); // @4028
    }
    let car_id = match buff(w, b).map(|x| &x.ext) {
        Some(BuffExt::C023ACarBuff(e)) => e.car,
        _ => None,
    };
    if let Some(id) = car_id
        && let Some(p) = w.actors.projectiles.get_mut(id)
        && !p.dead
        && let Some(c) = car_of_mut(&mut p.ext)
        && c.buff == Some(b)
        && !c.dropping
    {
        c.buff_gone = true;
    }
}

/// `carKind`'s hooks with `make`, `moved`, `begin`, `step` and `hitBy` left to the class (the item cars spread it).
pub fn car_kind() -> ProjectileHooks {
    ProjectileHooks {
        construct: Some(car_construct),
        step: Some(car_step),
        overlap: Some(car_overlap),
        non_item_pickup: Some(car_non_item_pickup),
        // SetPickupIcon @3604-4472, BPI_NonItemHidePickupUI @5937 -> @5525-6091
        pickup_label: true,
        damage_box: Some(car_damage_box),
        any_damage: Some(car_any_damage),
        destroyed: Some(car_destroyed),
        ..ProjectileHooks::default()
    }
}

/// This file's registry entries: the car, its buff.
pub fn register(r: &mut Registry) -> Result<(), RegistryError> {
    let car = ProjectileHooks {
        make: Some(gizmo_car_make),
        moved: Some(gizmo_car_moved),
        begin: Some(gizmo_car_begin),
        hit_by: Some(gizmo_car_hit_by),
        bodies: Some(car_bodies),
        fighter_signal: Some(car_signal),
        ..car_kind()
    };
    r.actors.enter(
        CAR,
        ActorEntry {
            mesh: Some(MeshHooks { montage_event: Some(car_montage_event), ..MeshHooks::default() }),
            ..ActorEntry::new(
                ActorClass::Projectile(ProjectileClass::C023ACarSpawnedActor),
                ActorHooks::Projectile(car),
            )
        },
    )?;
    r.buffs.enter(
        CAR_BUFF,
        BuffEntry {
            thru_atk: Some([true; 3]),
            active_attack: true,
            block_fast_fall: true,
            ..BuffEntry::new(
                BuffClass::C023ACarBuff,
                BuffHooks {
                    begin: Some(car_buff_begin),
                    tick: Some(car_buff_tick),
                    on: Some(car_buff_on),
                    end: Some(car_buff_end),
                    ..BuffHooks::default()
                },
            )
        },
    )
}
