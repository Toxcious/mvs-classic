//! The FFA cars: Item_CarWheel_C (an ItemActorParent child the stage's
//! MapRandomActorSpawnArea drops), the car it spawns for its picker (Parent_Car_SpawnedActor_C, [`ItemCar`]; its
//! children BatMobile_SpawnedActor_Child_C, [`BatMobileCar`], and MysteryMachine_SpawnedCar_C, a data child that takes
//! the parent's entry) and the driver's Car_Buff_C ([`ItemCarBuffExt`]).
//!
//! Parent_Car_SpawnedActor is C023A_Car_SpawnedActor's graph with the class's variables: the shell and the shared kind
//! are `chars::gizmo::car`'s (`CarActor`, `car_kind`, `car_begin`, `car_bodies`, `car_signal`, `car_honk`,
//! `car_jump_out`); what the item cars do apart is here:
//! - UpdateDriverInfo(true) @19-1697: K2_AttachToComponent(the driver's main mesh, fml_un_COM, SnapToTarget) @281 and
//!   the relative location (0, 0, 60 - AttachOffsetCharList[the driver's class], 75 for a class not in it) @750, over
//!   DriverOffset (0, 0, that offset) @1060 laid on the carried pose's fml_un_COM (BPI_COM_Carried_Override(0,
//!   DriverOffset) @4293: an additive translation while Car_Buff's BPI_SetBeingCarried has the pose on): the two
//!   cancel, and the car sits 60 above the carried pose's fml_un_COM for every class ([`item_attach_to`]);
//!   SpawnBuffActor(Car_Buff_C, Driver, Driver, 0, 0.0) @1138, then StartBuff(Max Acceleration, Speed Flat Override,
//!   Can Jump) @1241; DriverHidden -> the driver's AddInvisibility @4545. No KnockedBack bind (the DamageManager's is
//!   only removed @1861).
//! - The HitCollision overlap @8859: HitBoxDamage as the class has it, KnockBackDirectionVector (velocity's UE Y > 0 ?
//!   1 : -1, 1) @9176, AttackDecayType (driver ? 6 : 5) @9212; no CarMontageMulticast and no car_hit after it.
//! - InputTriggered @926 (the CarBuff's DriverInput): jump && Can Jump -> the driver's BPI_StartJump and JumpClient;
//!   ((Can Jump && jump) || evade) && SpawnJumpOutGate && CanLeave -> DetachCarServer and sfx car_jump_out @5837;
//!   special && HasSpecialInput -> SpecialInputServer(true); attack -> HonkServer. InputReleased @7067: special ->
//!   SpecialInputServer(false). Wheelie(false, DeltaSeconds) @7423: PoppinWheelie is never set.
//! - Item_CarWheel's ActivateOnPickup @174: the Instigator's Comp_Spawner.CompSpawnActor(Array_Random(CarList)), then
//!   ReleaseToPool.
//!
//! Not ported: UpdateLocality (view); Wheelie's BPI_COM_Carried_Override(RotationAngle, DriverOffset) @1357, the
//! carried COM's pitch (0, or 35 falling), not laid on the car's place.

use mvs_data::value::{Props, Value};

use crate::Real;
use crate::State;
use crate::actors::projectile::{ProjectileExt, class_p, max_lifetime_at};
use crate::actors::{ActorClass, ActorRef, ItemClass, ItemId, Projectile, ProjectileClass, ProjectileId};
use crate::chars::gizmo::car::{
    CarActor, car, car_begin, car_bodies, car_construct, car_honk, car_jump_out, car_kind, car_of, car_signal,
    carry_attach, cfg, ride,
};
use crate::combat::DataVec;
use crate::combat::buffs::{BuffExt, buff, buff_mut, inst_def_of, set_buff_values, spawn_buff, update_states};
use crate::combat::packet::DataEnum;
use crate::combat::{BuffClass, BuffRef, HitPacket};
use crate::events::Event;
use crate::fighter::common::warn_text;
use crate::fighter::control::data_name;
use crate::fighter::match_side::SpawnRequest;
use crate::fighter::moves::Impulse;
use crate::fighter::moves::bpi_disable_dash;
use crate::fighter::moves::max_jumps;
use crate::fighter::moves::try_jump;
use crate::fighter::moves::{disable_evade, disable_jump, enable_evade, enable_jump, max_accel, set_max_accel};
use crate::fighter::{FighterId, actor_y, facing};
use crate::math::to_int32;
use crate::name;
use crate::registry::entries::{ActorEntry, ActorHooks, BuffEntry, BuffHooks, ItemHooks, ProjectileHooks};
use crate::registry::props::{arr, flag, num};
use crate::registry::signals::{BuffSignal, BuffSignalArg};
use crate::registry::{Registry, RegistryError};
use crate::world::World;

/// `PARENT_CAR`.
pub const PARENT_CAR: &str = "Parent_Car_SpawnedActor_C";
/// `BATMOBILE`.
pub const BATMOBILE: &str = "BatMobile_SpawnedActor_Child_C";
/// MysteryMachine_SpawnedCar_C (Parent_Car_SpawnedActor_C's entry).
pub const MYSTERY_MACHINE: &str = "MysteryMachine_SpawnedCar_C";
/// `CAR_BUFF`.
pub const CAR_BUFF: &str = "Car_Buff_C";
/// `WHEEL`.
pub const WHEEL: &str = "Item_CarWheel_C";
/// `BOUNCE_BUFF`.
const BOUNCE_BUFF: crate::names::Name = name!("Buff_C023A_Bounce_C");

/// `ItemCar extends CarActor` (Parent_Car_SpawnedActor_C and its data child MysteryMachine_SpawnedCar_C).
#[derive(State, Debug, Default)]
pub struct ItemCar {
    pub car: CarActor,
    /// The car's age at the attach to a driver that is no BPI_C023A pawn: the carried pose's loop starts there.
    pub carried_at: i32,
}

/// `BatMobileCar extends ItemCar` (BatMobile_SpawnedActor_Child_C).
#[derive(State, Debug, Default)]
pub struct BatMobileCar {
    pub item: ItemCar,
    /// IsBoosting.
    pub boosting: bool,
    /// BoostTimer: the car's age it ends at, -1 not active.
    pub boost_at: Real,
    /// FireDamageGate and ThrusterEffect's visibility.
    pub thruster: bool,
    /// StartingDamage.
    pub start_damage: Real,
    /// StartingBaseKnockBack.
    pub start_knock_back: Real,
}

/// Car_Buff's extension (`CarBuffExt`): BaseMaxAceleration, the car that spawned it (its DriverInput /
/// DriverInputRelease binds), CanJump.
#[derive(State, Debug, Default)]
pub struct ItemCarBuffExt {
    pub base: Real,
    pub car: Option<ProjectileId>,
    pub can_jump: bool,
}

// ---------------------------------------------------------------- the class's own pieces

/// `ItemCar.attachTo`: fml_un_COM with (0, 0, 60 - offset) over DriverOffset (0, 0, offset): 60 above the bone (the
/// header; `carry_attach`). A BPI_C023A driver gets BPI_DrivingCar instead of the carried pose: his own pose's bone.
pub(crate) fn item_attach_to(w: &mut World, id: ProjectileId, d: FighterId) {
    if !w.actors.projectiles.get(id).expect("a live projectile").attached {
        carry_attach(w, id, d);
    }
}

/// `ItemCar.spawnCarBuff`: SpawnBuffActor(Car_Buff_C, Driver, Driver, 0, 0.0, false, false) @1138 (no life timer:
/// BuffSafty's SetLifeTimer(0) later changes nothing), then StartBuff @1241.
pub(crate) fn item_spawn_car_buff(w: &mut World, id: ProjectileId, d: FighterId) -> Option<BuffRef> {
    let b = spawn_buff(w, d, name!("Car_Buff_C"), 0.0, Some(0), Some(d), false, Some(d));
    if let Some(b) = b {
        start_car_buff(w, d, b, id);
    }
    b
}

/// `ItemCar.ramPacket`: HitBoxDamage with KnockBackDirectionVector and AttackDecayType.
pub(crate) fn item_ram_packet(w: &World, id: ProjectileId, vx: Real, d: Option<FighterId>) -> HitPacket {
    let mut p = w.actors.projectiles.get(id).expect("a live projectile").hit.clone();
    p.knock_back_direction_vector = Some(DataVec { x: Some(if vx < 0.0 { 1.0 } else { -1.0 }), y: Some(1.0), z: None });
    p.attack_decay_type = Some(DataEnum::Value(if d.is_some() { 6 } else { 5 }));
    p
}

fn pr(w: &World, id: ProjectileId) -> &Projectile {
    w.actors.projectiles.get(id).expect("a live projectile")
}

fn pm(w: &mut World, id: ProjectileId) -> &mut Projectile {
    w.actors.projectiles.get_mut(id).expect("a live projectile")
}

fn bat(p: &Projectile) -> Option<&BatMobileCar> {
    match &p.ext {
        ProjectileExt::BatMobileCar(b) => Some(b),
        _ => None,
    }
}

fn bat_mut(p: &mut Projectile) -> Option<&mut BatMobileCar> {
    match &mut p.ext {
        ProjectileExt::BatMobileCar(b) => Some(b),
        _ => None,
    }
}

/// `specialInput(on)`: SpecialInputServer(on). Parent_Car's: nothing of its own (HasSpecialInput gates the press);
/// the BatMobile's boost.
///
/// SpecialInputMulticast @1489: true and not IsBoosting -> @870 IsBoosting, BoostTimer (BoostTime ->
/// CheckToStopBoost), FireDamageGate, the Thruster visible, then @189 (the CarBuff a Car_Buff): UpdateBuffInfo(20000,
/// 8000), GetOwner's LaunchCharacter((0, facing left ? 5000 : -5000, 0), true, false), Damage = StartingDamage x 1.25,
/// BaseKnockBack = StartingBaseKnockBack x 1.2. True while boosting: nothing. False: IsBoosting false; the BoostTimer
/// active -> nothing, else @1057.
fn special_input(w: &mut World, id: ProjectileId, on: bool) {
    if bat(pr(w, id)).is_none() {
        return;
    }
    if on {
        if bat(pr(w, id)).is_some_and(|b| b.boosting) {
            return;
        }
        // Deviation: the BoostTimer is taken at the car's tick (as the parent's timers: `car_step`).
        let t = crate::combat::hit::knockback::timer_ticks(Some(cfg(w, id, "BoostTime")));
        let a = crate::real(pr(w, id).age);
        {
            let b = bat_mut(pm(w, id)).expect("a BatMobile");
            b.boosting = true;
            b.boost_at = a + t;
            b.thruster = true;
        }
        let (buf, d) = (car(w, id).buff, car(w, id).driver);
        let (Some(buf), Some(d)) = (buf, d) else { return };
        if !buff(w, buf).is_some_and(|x| x.frames > 0.0) {
            return;
        }
        update_buff_info(w, d, buf, 20000.0, 8000.0);
        let o = pr(w, id).owner.expect("a car has an owner");
        // UE +Y is sim -x: facing left gives UE Y +5000, sim vx -5000. XYOverride true, ZOverride false.
        let vx = facing(w, o) * 5000.0;
        w.fighter_mut(o).moves.impulse = Some(Impulse { vx, vy: 0.0, xy: true, kind: 0, z: false, d: None });
        let p = pm(w, id);
        let b = bat(p).expect("a BatMobile");
        let (dmg, kb) = ((b.start_damage * 1.25), (b.start_knock_back * 1.2));
        p.hit.damage = Some(dmg);
        p.hit.base_knock_back = Some(kb);
        return;
    }
    bat_mut(pm(w, id)).expect("a BatMobile").boosting = false;
    if bat(pr(w, id)).is_some_and(|b| b.boost_at >= 0.0) {
        return;
    }
    stop_boost(w, id);
}

/// `checkToStopBoost` (CheckToStopBoost @1038): the BoostTimer's end: IsBoosting -> nothing; else @1057.
fn check_to_stop_boost(w: &mut World, id: ProjectileId) {
    let b = bat_mut(pm(w, id)).expect("a BatMobile");
    b.boost_at = -1.0;
    if !b.boosting {
        stop_boost(w, id);
    }
}

/// `stopBoost` @1057: the BoostTimer cleared, and with a Car_Buff UpdateBuffInfo(Max Acceleration, Speed Flat
/// Override), the Thruster hidden, FireDamageGate false, Damage and BaseKnockBack back.
fn stop_boost(w: &mut World, id: ProjectileId) {
    bat_mut(pm(w, id)).expect("a BatMobile").boost_at = -1.0;
    let (buf, d) = (car(w, id).buff, car(w, id).driver);
    let (Some(buf), Some(d)) = (buf, d) else { return };
    if !buff(w, buf).is_some_and(|x| x.frames > 0.0) {
        return;
    }
    let (accel, speed) = {
        let p = pr(w, id);
        let c = class_p(w, p);
        (num(c, "Max Acceleration").unwrap_or(0.0), num(c, "Speed Flat Override").unwrap_or(0.0))
    };
    update_buff_info(w, d, buf, accel, speed);
    let p = pm(w, id);
    let b = bat_mut(p).expect("a BatMobile");
    b.thruster = false;
    let (dmg, kb) = (b.start_damage, b.start_knock_back);
    p.hit.damage = Some(dmg);
    p.hit.base_knock_back = Some(kb);
}

// ---------------------------------------------------------------- the item cars

/// The item car's `make` (a BatMobileCar for BatMobile_SpawnedActor_Child_C, else an ItemCar).
fn item_car_make(_w: &World, _f: FighterId, r: &SpawnRequest) -> ProjectileExt {
    if r.cls == name!("BatMobile_SpawnedActor_Child_C") {
        ProjectileExt::BatMobileCar(BatMobileCar { boost_at: -1.0, ..BatMobileCar::default() })
    } else {
        ProjectileExt::ItemCar(ItemCar::default())
    }
}

/// The item car's constructor tail: the shell's, and the BatMobile's StartingDamage / StartingBaseKnockBack (BeginPlay:
/// from HitBoxDamage, then the parent's).
fn item_car_construct(w: &World, p: &mut Projectile) {
    car_construct(w, p);
    let (dmg, kb) = (p.hit.damage.unwrap_or(0.0), p.hit.base_knock_back.unwrap_or(0.0));
    if let ProjectileExt::BatMobileCar(b) = &mut p.ext {
        b.start_damage = dmg;
        b.start_knock_back = kb;
    }
}

/// The item car's `begin` (`carBegin`, ReceiveBeginPlay @5094-5250).
fn item_car_begin(w: &mut World, id: ProjectileId) {
    car_begin(w, id);
}

/// The item car's `moved` (`car.ride()`).
fn item_car_moved(w: &mut World, id: ProjectileId) {
    ride(w, id);
}

/// The item car's `step`: `carKind.step`, then the BatMobile's boost timer, then the attached MaxLifetime.
fn item_car_step(w: &mut World, id: ProjectileId) -> bool {
    if crate::chars::gizmo::car::car_step(w, id) {
        return true;
    }
    if pr(w, id).dead {
        return false;
    }
    if bat(pr(w, id)).is_some_and(|b| crate::real(pr(w, id).age) == b.boost_at) {
        check_to_stop_boost(w, id);
    }
    // Deviation: the attached car's MaxLifetimeTimer (the port's attached move has no expiry).
    let p = pr(w, id);
    p.attached && max_lifetime_at(p) > 0 && p.age >= max_lifetime_at(p)
}

/// The item car's `hitBy`: OnHitByAlly @16140: an attacker that is the car's Instigator with Car_Buff and
/// Buff_C023A_Bounce -> its HandleBounceOnAuthority; BounceSuccessful -> OverheatTimeEnd (the Bounce of an item car is
/// not ported: warned).
#[allow(clippy::too_many_arguments, reason = "the hook's parameters")]
fn item_car_hit_by(
    w: &mut World,
    id: ProjectileId,
    ally: bool,
    _dx: Real,
    _dy: Real,
    _bkb: Real,
    _attacker_type: i32,
    by: Option<ActorRef>,
) {
    let Some(ActorRef::Fighter(f)) = by.filter(|_| ally) else { return };
    if Some(f) != pr(w, id).owner {
        return;
    }
    if w.fighter(f).buffs.buffs.iter().any(|b| b.cls == BOUNCE_BUFF && b.frames > 0.0) {
        let msg = format!(
            "{} OnHitByAlly: Buff_C023A_Bounce HandleBounceOnAuthority on an item car not ported",
            w.names.str(pr(w, id).cls)
        );
        warn_text(w, &msg);
    }
}

// ---------------------------------------------------------------- Car_Buff_C

/// UpdateBuffInfo(MaxAcceleration, SpeedFlatOverride) @4666: the owner's MaxAcceleration and the buff's
/// SpeedFlatOverride, then UpdatePlayerStatus.
fn update_buff_info(w: &mut World, f: FighterId, b: BuffRef, accel: Real, speed: Real) {
    set_max_accel(w, f, accel);
    set_buff_values(w, b, &Props::from_iter([("SpeedFlatOverride".to_owned(), Value::Num(f64::from(speed)))]));
    update_states(w, f);
}

/// StartBuff(MaxAcceleration, SpeedFlatOverride, CanJump) @5035 -> StartBuffMulticast @5040: the owner's
/// MaxAcceleration and SpeedFlatOverride, UpdatePlayerStatus; the owner's InputPreTriggered / InputReleased binds;
/// @3766 DisableEvade, no CanJump -> DisableJump @187; @224 BaseMaxAceleration = the owner's MaxAcceleration and
/// MaxAcceleration = ModifiedAcelerationSpeed; DisableDash(true), BlockFastFall, sfx car_jump_in and car_honk on the
/// owner, AddActiveAttack, the three ThruATK flags; a BPI_C023A owner BPI_DrivingCar(true), any other
/// BPI_SetBeingCarried(true, true) @2024.
/// BaseMaxAceleration is read after the multicast wrote the car's Max Acceleration, so the EndPlay leaves the owner at
/// that value, not at his own of before.
fn start_car_buff(w: &mut World, f: FighterId, b: BuffRef, car_id: ProjectileId) {
    let (can_jump, accel, speed) = {
        let p = pr(w, car_id);
        let c = class_p(w, p);
        (flag(c, "Can Jump"), num(c, "Max Acceleration").unwrap_or(0.0), num(c, "Speed Flat Override").unwrap_or(0.0))
    };
    update_buff_info(w, f, b, accel, speed);
    disable_evade(w, f);
    if !can_jump {
        disable_jump(w, f);
    }
    let base = max_accel(w, f);
    if let Some(x) = buff_mut(w, b) {
        x.ext = BuffExt::ItemCarBuff(ItemCarBuffExt { base, car: Some(car_id), can_jump });
    }
    let modified = inst_def_of(w, b).as_deref().and_then(|d| num(d, "ModifiedAcelerationSpeed")).unwrap_or(0.0);
    set_max_accel(w, f, modified);
    bpi_disable_dash(w, f, true);
    for ev in [name!("sfx_cha_C023a_car_jump_in"), name!("sfx_cha_C023a_car_honk")] {
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
    crate::fighter::add_active_attack(w, f);
    if let Some(g) = crate::chars::gizmo::gizmo_mut(w, f) {
        g.driving = true;
    } else {
        crate::fighter::anim::set_being_carried(w, f, true, true); // @2024
    }
}

/// The buff's car, the live car whose CarBuff it is (`car.buff === b`).
fn buff_car(w: &World, b: BuffRef) -> Option<(ProjectileId, bool)> {
    let Some(BuffExt::ItemCarBuff(e)) = buff(w, b).map(|x| &x.ext) else { return None };
    let id = e.car?;
    let p = w.actors.projectiles.get(id)?;
    let c = car_of(&p.ext)?;
    (!p.dead && c.buff == Some(b)).then_some((id, e.can_jump))
}

/// Car_Buff_C begin: nothing (StartBuff comes from the car).
fn car_buff_begin(_w: &mut World, _f: FighterId, _b: BuffRef, _from: Option<FighterId>, _dur: Option<Real>) {}

/// Car_Buff_C tick: InputReleased (DriverInputsRelease -> the car's @7067: special -> SpecialInputServer(false)).
// Deviation: the owner's InputReleased is polled at the buff's tick (the special button's falling edge).
fn car_buff_tick(w: &mut World, f: FighterId, b: BuffRef) -> bool {
    if let Some((id, _)) = buff_car(w, b) {
        let c = &w.fighter(f).controls;
        if c.prev.special && !c.inp.special {
            special_input(w, id, false);
        }
    }
    false
}

/// Car_Buff_C on (inputPreTriggered): true: the buff handled the input.
fn car_buff_on(w: &mut World, f: FighterId, b: BuffRef, s: BuffSignal, a: &BuffSignalArg) -> bool {
    let Some((id, can_jump)) = buff_car(w, b) else { return false };
    if s != BuffSignal::InputPreTriggered {
        return false;
    }
    // DriverInputs -> DriverInputServer -> broadcast DriverInput(InputType, 0) -> the car's InputTriggered.
    let t = a.input;
    if t == Some(2) && can_jump {
        // the driver's BPI_StartJump and JumpClient's (@17909) as one call
        // Unverified: the server's StartJump and the client's JumpClient taken as one jump.
        if crate::real(w.fighter(f).jump.jumps) < max_jumps(w, f) {
            try_jump(w, f);
        }
        return false;
    }
    let (leave, special) = {
        let p = pr(w, id);
        let c = class_p(w, p);
        (flag(c, "CanLeave"), flag(c, "HasSpecialInput"))
    };
    if ((can_jump && t == Some(2)) || t == Some(3)) && car(w, id).jump_out_gate && leave {
        car_jump_out(w, id, b, f);
        return true;
    }
    if t == Some(1) && special {
        special_input(w, id, true);
    }
    if t == Some(0) {
        car_honk(w, id, f);
    }
    false
}

/// Car_Buff_C ReceiveEndPlay @4510: the binds off, EnableEvade, EnableJump (whether StartBuff disabled it or not),
/// MaxAcceleration = BaseMaxAceleration, DisableDash(false), BlockFastFall(false), ClearActiveAttack, the ThruATK
/// flags, BPI_DrivingCar(false) on a BPI_C023A owner, else BPI_SetBeingCarried(false, false) @3726. A CarBuff the car
/// did not destroy itself: its BuffDestroyed at the car's tick.
fn car_buff_end(w: &mut World, f: FighterId, b: BuffRef) {
    let ext = match buff(w, b).map(|x| &x.ext) {
        Some(BuffExt::ItemCarBuff(e)) => Some((e.base, e.car)),
        _ => None,
    };
    enable_evade(w, f);
    enable_jump(w, f);
    let base = ext.map_or_else(|| max_accel(w, f), |e| e.0);
    set_max_accel(w, f, base);
    bpi_disable_dash(w, f, false);
    // ClearActiveAttack (Comp_Pawn_BranchManager): AttackIsActive 0 and the OnIsAttacking broadcast, no AttackEnded.
    let fm = w.fighter_mut(f);
    fm.charge.attack_is_active = 0;
    fm.charge.anim_attack_off = 0;
    if let Some(g) = crate::chars::gizmo::gizmo_mut(w, f) {
        g.driving = false;
    } else {
        crate::fighter::anim::set_being_carried(w, f, false, false); // @3726
    }
    if let Some(id) = ext.and_then(|e| e.1)
        && let Some(p) = w.actors.projectiles.get_mut(id)
        && let Some(c) = crate::chars::gizmo::car::car_of_mut(&mut p.ext)
        && c.buff == Some(b)
        && !c.dropping
    {
        c.buff_gone = true;
    }
}

// ---------------------------------------------------------------- Item_CarWheel_C

/// Item_CarWheel's ActivateOnPickup @174 (authority): Array_Random(CarList), the Instigator's
/// Comp_Spawner.CompSpawnActor(the class, not attached, socket None, no world transforms, the identity transform, no
/// direction, speeds 0): at the Instigator's location - (0, 0, 100); then ReleaseToPool (the item ends).
// Deviation: Array_Random's FMath::RandRange taken from the world stream.
fn wheel_picked_up(w: &mut World, it: ItemId, f: FighterId) -> bool {
    let list: Vec<String> = match crate::world::items::item_def(w, it) {
        Some(p) => arr(p, "CarList").iter().filter_map(|v| v.as_str().map(str::to_owned)).collect(),
        None => Vec::new(),
    };
    if list.is_empty() {
        return true;
    }
    #[allow(clippy::cast_precision_loss, reason = "a short list")]
    let n = list.len() as Real;
    let k = usize::try_from(to_int32(w.random_int(n))).expect("an index");
    let cls = data_name(w, &list[k]);
    let (x, y) = (w.fighter(f).body.x, (actor_y(w, f) - 100.0));
    crate::world::spawning::comp_spawn_projectile(w, f, cls, x, y, [Some(0.0); 3], 0.0, 0.0, None);
    true
}

/// The item cars' hooks: `carKind` spread under their own.
fn item_car_kind() -> ProjectileHooks {
    ProjectileHooks {
        make: Some(item_car_make),
        construct: Some(item_car_construct),
        begin: Some(item_car_begin),
        moved: Some(item_car_moved),
        step: Some(item_car_step),
        hit_by: Some(item_car_hit_by),
        bodies: Some(car_bodies),
        fighter_signal: Some(car_signal),
        ..car_kind()
    }
}

/// This file's registry entries: the two item cars (and the data child), the
/// buff, the wheel.
pub fn register(r: &mut Registry) -> Result<(), RegistryError> {
    for (cls, class) in [
        (PARENT_CAR, ProjectileClass::ParentCarSpawnedActor),
        (BATMOBILE, ProjectileClass::BatMobileSpawnedActorChild),
        (MYSTERY_MACHINE, ProjectileClass::MysteryMachineSpawnedCar),
    ] {
        r.actors.enter(cls, ActorEntry::new(ActorClass::Projectile(class), ActorHooks::Projectile(item_car_kind())))?;
    }
    r.buffs.enter(
        CAR_BUFF,
        BuffEntry {
            thru_atk: Some([true; 3]),
            active_attack: true,
            block_fast_fall: true,
            ..BuffEntry::new(
                BuffClass::CarBuff,
                BuffHooks {
                    begin: Some(car_buff_begin),
                    tick: Some(car_buff_tick),
                    on: Some(car_buff_on),
                    end: Some(car_buff_end),
                    ..BuffHooks::default()
                },
            )
        },
    )?;
    let wheel = ItemHooks { picked_up: Some(wheel_picked_up), ..ItemHooks::default() };
    r.actors.enter(
        WHEEL,
        ActorEntry {
            ownerless: true,
            ..ActorEntry::new(ActorClass::Item(ItemClass::ItemCarWheel), ActorHooks::Item(wheel))
        },
    )
}
