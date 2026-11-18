//! Batcave's state as the record writes it (`stages/batcave/`): the entry's own state (`StageRun.s`) and its
//! placed actors' own keys.

use mvs_sim::stages::batcave::{BatcaveBats, BatcaveCar, BatcaveFog, BatcaveOwn, VentsState};
use mvs_trace::{Obj, Value};

use super::wall::{wall_load, wall_put};
use crate::conv::{as_obj, check_keys, plain_keys, plain_struct};
use crate::cx::{Lx, Wx};

plain_struct!(VentsState {
    "warning" = warning,
    "hidden" = hidden,
    "fogs" = fogs,
    "fans" = fans,
    "leftVent" = left_vent,
    "rightVent" = right_vent,
});

/// `StageRun.s` of Batcave.
pub fn own_put(s: &BatcaveOwn, x: &Wx) -> Value {
    let mut o: Vec<(String, Value)> = Vec::new();
    plain_keys!(put s, x, o; "vents" = vents, "cars" = cars, "bats" = bats, "tick" = tick, "pf" = pf);
    Value::Obj(Obj::new(o))
}

pub fn own_load(s: &mut BatcaveOwn, v: &Value, l: &mut Lx) {
    let Some(ob) = as_obj(v, l) else { return };
    plain_keys!(load s, ob, l; "vents" = vents, "cars" = cars, "bats" = bats, "tick" = tick, "pf" = pf);
    check_keys(ob, plain_keys!(names "vents" = vents, "cars" = cars, "bats" = bats, "tick" = tick, "pf" = pf), l);
}

/// A `BatcaveFog`'s own keys (beside the `StageActor` ones), pushed on `o`.
pub fn batcave_fog_put(a: &BatcaveFog, x: &Wx, o: &mut Vec<(String, Value)>) {
    plain_keys!(put a, x, o;
        "z" = z, "side" = side, "delayOn" = delay_on, "fogActivated" = fog_activated, "buffed" = buffed,
        "inside" = inside, "particle" = particle, "dissolve" = dissolve);
}

/// Loads them from `ob`, each key read pushed on `known`.
pub fn batcave_fog_load(a: &mut BatcaveFog, ob: &Obj, l: &mut Lx, known: &mut Vec<&'static str>) {
    plain_keys!(load a, ob, l;
        "z" = z, "side" = side, "delayOn" = delay_on, "fogActivated" = fog_activated, "buffed" = buffed,
        "inside" = inside, "particle" = particle, "dissolve" = dissolve);
    known.extend(plain_keys!(names
        "z" = z, "side" = side, "delayOn" = delay_on, "fogActivated" = fog_activated, "buffed" = buffed,
        "inside" = inside, "particle" = particle, "dissolve" = dissolve));
}

/// A `BatcaveCar`'s own keys (beside the `StageActor` ones), pushed on `o`.
pub fn batcave_car_put(a: &BatcaveCar, x: &Wx, o: &mut Vec<(String, Value)>) {
    wall_put(&a.wall, x, o);
    plain_keys!(put a, x, o;
        "isDead" = is_dead, "almostDead" = almost_dead, "alarmOnce" = alarm_once, "lightsAt" = lights_at,
        "topAt" = top_at, "frontAt" = front_at, "frontOff" = front_off, "splashOffAt" = splash_off_at,
        "color" = color, "montage" = montage, "montageAt" = montage_at, "montageDone" = montage_done,
        "riders" = riders, "weight" = weight, "inBox1" = in_box1, "respawnMontage" = respawn_montage,
        "edges" = edges);
}

/// Loads them from `ob`, each key read pushed on `known`.
pub fn batcave_car_load(a: &mut BatcaveCar, ob: &Obj, l: &mut Lx, known: &mut Vec<&'static str>) {
    wall_load(&mut a.wall, ob, l, known);
    plain_keys!(load a, ob, l;
        "isDead" = is_dead, "almostDead" = almost_dead, "alarmOnce" = alarm_once, "lightsAt" = lights_at,
        "topAt" = top_at, "frontAt" = front_at, "frontOff" = front_off, "splashOffAt" = splash_off_at,
        "color" = color, "montage" = montage, "montageAt" = montage_at, "montageDone" = montage_done,
        "riders" = riders, "weight" = weight, "inBox1" = in_box1, "respawnMontage" = respawn_montage,
        "edges" = edges);
    known.extend(plain_keys!(names
        "isDead" = is_dead, "almostDead" = almost_dead, "alarmOnce" = alarm_once, "lightsAt" = lights_at,
        "topAt" = top_at, "frontAt" = front_at, "frontOff" = front_off, "splashOffAt" = splash_off_at,
        "color" = color, "montage" = montage, "montageAt" = montage_at, "montageDone" = montage_done,
        "riders" = riders, "weight" = weight, "inBox1" = in_box1, "respawnMontage" = respawn_montage,
        "edges" = edges));
}

/// A `BatcaveBats`'s own keys (beside the `StageActor` ones), pushed on `o`.
pub fn batcave_bats_put(a: &BatcaveBats, x: &Wx, o: &mut Vec<(String, Value)>) {
    plain_keys!(put a, x, o;
        "on" = on, "warnAt" = warn_at, "mainAt" = main_at, "warnOffAt" = warn_off_at, "gateClosed" = gate_closed,
        "gateInit" = gate_init, "inside" = inside);
}

/// Loads them from `ob`, each key read pushed on `known`.
pub fn batcave_bats_load(a: &mut BatcaveBats, ob: &Obj, l: &mut Lx, known: &mut Vec<&'static str>) {
    plain_keys!(load a, ob, l;
        "on" = on, "warnAt" = warn_at, "mainAt" = main_at, "warnOffAt" = warn_off_at, "gateClosed" = gate_closed,
        "gateInit" = gate_init, "inside" = inside);
    known.extend(plain_keys!(names
        "on" = on, "warnAt" = warn_at, "mainAt" = main_at, "warnOffAt" = warn_off_at, "gateClosed" = gate_closed,
        "gateInit" = gate_init, "inside" = inside));
}
