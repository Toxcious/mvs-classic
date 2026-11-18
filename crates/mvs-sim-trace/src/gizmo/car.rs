//! Gizmo's car and its buff as the record writes them. The car shell's keys
//! (`CarActor`'s fields, shared with the item cars: `crate::items::car`) are written by [`car_shell_put`].
//!
//! `buff` is the whole buff entry the oracle keeps (`CarBuff`): written from its fighter's list through the reference,
//! read back as a reference to a marker serial that [`resolve_car_buffs`] (after the World has its fighters' buffs)
//! turns into the entry whose extension names the car.

use mvs_sim::chars::gizmo::car::{CarActor, CarBuffExt, GizmoCar, car_of};
use mvs_sim::combat::{BuffRef, buffs::BuffExt};
use mvs_sim::fighter::FighterId;
use mvs_sim::world::World;
use mvs_trace::{Obj, Value};

use crate::conv::{Plain, plain_keys, plain_struct};
use crate::cx::{Lx, Wx};

/// The serial a loaded `buff` waits with.
const PENDING_CAR_BUFF: u32 = u32::MAX - 7;
/// The serial of a held entry loaded as gone (`swept`).
const GONE_CAR_BUFF: u32 = u32::MAX - 8;

macro_rules! shell_head {
    ($mac:ident $($pre:tt)*) => {
        $mac!($($pre)* "driver" = driver, "driven" = driven)
    };
}

macro_rules! shell_tail {
    ($mac:ident $($pre:tt)*) => {
        $mac!($($pre)*
            "driverLeft" = driver_left, "lvx" = lvx, "lvy" = lvy, "wasGround" = was_ground,
            "knockedOffGate" = knocked_off_gate, "jumpOutGate" = jump_out_gate, "poppin" = poppin,
            "wheelie" = wheelie, "wheelieGate" = wheelie_gate, "honkAmmo" = honk_ammo, "hasOverheat" = has_overheat,
            "hurtOn" = hurt_on, "hitTimes" = hit_times, "inside" = inside, "drvGround" = drv_ground,
            "safetyAt" = safety_at, "jumpOutAt" = jump_out_at, "frictionAt" = friction_at,
            "overheatAt" = overheat_at, "hitTimerAt" = hit_timer_at, "blinkAt" = blink_at, "gravAt" = grav_at,
            "knockAt" = knock_at, "dropBuffAt" = drop_buff_at, "honkNext" = honk_next,
            "knockPending" = knock_pending, "driverOut" = driver_out, "buffGone" = buff_gone,
            "dropping" = dropping, "overheatVfx" = overheat_vfx, "pickupGate" = pickup_gate,
            "checkOn" = check_on, "newDriverAt" = new_driver_at, "checkIn" = check_in, "inputUi" = input_ui,
            "speed" = speed, "damage" = damage, "shakeAge" = shake_age, "blink" = blink, "turns" = turns,
        )
    };
}

/// The keys `CarActor` adds to its projectile's `rest` (a car class's put calls it, then writes its own).
pub fn car_shell_put(c: &CarActor, x: &Wx, o: &mut Vec<(String, Value)>) {
    shell_head!(plain_keys put c, x, o;);
    o.push(("buff".to_owned(), crate::black_adam::chain::buff_entry_put(c.buff, x)));
    shell_tail!(plain_keys put c, x, o;);
    o.push(("anim".to_owned(), crate::parts::anim_put(&c.anim, x)));
}

/// [`car_shell_put`]'s inverse; the keys it knows go into `known`.
pub fn car_shell_load(c: &mut CarActor, ob: &Obj, l: &mut Lx, known: &mut Vec<&'static str>) {
    shell_head!(plain_keys load c, ob, l;);
    c.buff = buff_load(c.driver, ob.get("buff"), l);
    shell_tail!(plain_keys load c, ob, l;);
    match ob.get("anim") {
        Some(v) => l.at("anim", |l| crate::parts::anim_load(&mut c.anim, v, l)),
        None => l.miss("key anim absent"),
    }
    known.extend(shell_head!(plain_keys names));
    known.push("buff");
    known.extend(shell_tail!(plain_keys names));
    known.push("anim");
}

/// The car's held `buff`: a reference to the marker [`resolve_car_buffs`] resolves, or (an entry its list has
/// swept) the value kept for the writer.
// Unverified: the gone entries of two cars at once share one serial: the writer then finds the later one's value.
fn buff_load(driver: Option<FighterId>, v: Option<&Value>, l: &mut Lx) -> Option<BuffRef> {
    let v = match v {
        None | Some(Value::Null) => return None,
        Some(v) => v,
    };
    let Value::Obj(o) = v else {
        l.at("buff", |l| l.miss("a car's buff that is no object"));
        return None;
    };
    let Some(fighter) = driver.or_else(|| l.fighter(0.0)) else {
        l.at("buff", |l| l.miss("a car's buff with no fighter to hold it"));
        return None;
    };
    if matches!(o.get("swept"), Some(Value::Bool(true))) {
        return Some(crate::black_adam::chain::buff_entry_gone_load(v, fighter, GONE_CAR_BUFF, l));
    }
    Some(BuffRef { fighter, serial: PENDING_CAR_BUFF })
}

/// The car a buff's extension names.
fn buff_car(e: &BuffExt) -> Option<mvs_sim::actors::ProjectileId> {
    match e {
        BuffExt::C023ACarBuff(e) => e.car,
        BuffExt::ItemCarBuff(e) => e.car,
        _ => None,
    }
}

/// Turns the markers [`buff_load`] left into the entries of the fighters' lists: the one whose extension names the
/// car, the driver's first.
pub fn resolve_car_buffs(w: &mut World, l: &mut Lx) {
    let mut fix = Vec::new();
    for (id, p) in w.actors.projectiles.iter() {
        let Some(c) = car_of(&p.ext) else { continue };
        if c.buff.is_none_or(|r| r.serial != PENDING_CAR_BUFF) {
            continue;
        }
        let ours = |f: FighterId| {
            w.fighter(f)
                .buffs
                .buffs
                .iter()
                .find(|b| buff_car(&b.ext) == Some(id))
                .map(|b| BuffRef { fighter: f, serial: b.serial })
        };
        let found = c.driver.and_then(ours).or_else(|| w.fighters.iter().find_map(|(f, _)| ours(f)));
        match found {
            Some(b) => fix.push((id, b)),
            None => l.miss(format_args!("a car's buff is in no list of a fighter")),
        }
    }
    for (id, b) in fix {
        if let Some(p) = w.actors.projectiles.get_mut(id)
            && let Some(c) = mvs_sim::chars::gizmo::car::car_of_mut(&mut p.ext)
        {
            c.buff = Some(b);
        }
    }
}

/// A C023A_Car_SpawnedActor_C's own keys: the shell's, then `carriedAt`.
pub fn gizmo_car_put(s: &GizmoCar, x: &Wx, o: &mut Vec<(String, Value)>) {
    car_shell_put(&s.car, x, o);
    plain_keys!(put s, x, o; "carriedAt" = carried_at);
}

/// [`gizmo_car_put`]'s inverse; the keys it knows go into `known`.
pub fn gizmo_car_load(s: &mut GizmoCar, ob: &Obj, l: &mut Lx, known: &mut Vec<&'static str>) {
    car_shell_load(&mut s.car, ob, l, known);
    plain_keys!(load s, ob, l; "carriedAt" = carried_at);
    known.extend(plain_keys!(names "carriedAt" = carried_at));
}

plain_struct!(CarBuffExt { "base" = base, "car" = car });

/// C023A_Car_Buff_C's extension as the oracle writes it.
pub fn car_buff_put(e: &CarBuffExt, x: &Wx) -> Value {
    e.to_value(x)
}

/// [`car_buff_put`]'s inverse.
pub fn car_buff_load(e: &mut CarBuffExt, v: &Value, l: &mut Lx) {
    e.load_value(v, l);
}
