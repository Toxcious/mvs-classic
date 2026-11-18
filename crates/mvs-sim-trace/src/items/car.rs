//! The FFA cars and their buff as the record writes them: the shell's keys are
//! `crate::gizmo::car::car_shell_put`'s, then `carriedAt`, and the BatMobile's own after them.

use mvs_sim::items::car::{BatMobileCar, ItemCar, ItemCarBuffExt};
use mvs_trace::{Obj, Value};

use crate::conv::{Plain, plain_keys, plain_struct};
use crate::cx::{Lx, Wx};
use crate::gizmo::car::{car_shell_load, car_shell_put};

/// A Parent_Car_SpawnedActor_C / MysteryMachine_SpawnedCar_C as the record writes it.
pub fn item_car_put(s: &ItemCar, x: &Wx, o: &mut Vec<(String, Value)>) {
    car_shell_put(&s.car, x, o);
    plain_keys!(put s, x, o; "carriedAt" = carried_at);
}

/// [`item_car_put`]'s inverse; the keys it knows go into `known`.
pub fn item_car_load(s: &mut ItemCar, ob: &Obj, l: &mut Lx, known: &mut Vec<&'static str>) {
    car_shell_load(&mut s.car, ob, l, known);
    plain_keys!(load s, ob, l; "carriedAt" = carried_at);
    known.extend(plain_keys!(names "carriedAt" = carried_at));
}

macro_rules! bat_keys {
    ($mac:ident $($pre:tt)*) => {
        $mac!($($pre)*
            "boosting" = boosting, "boostAt" = boost_at, "thruster" = thruster,
            "startDamage" = start_damage, "startKnockBack" = start_knock_back,
        )
    };
}

/// A BatMobile_SpawnedActor_Child_C as the record writes it.
pub fn batmobile_put(s: &BatMobileCar, x: &Wx, o: &mut Vec<(String, Value)>) {
    item_car_put(&s.item, x, o);
    bat_keys!(plain_keys put s, x, o;);
}

/// [`batmobile_put`]'s inverse; the keys it knows go into `known`.
pub fn batmobile_load(s: &mut BatMobileCar, ob: &Obj, l: &mut Lx, known: &mut Vec<&'static str>) {
    item_car_load(&mut s.item, ob, l, known);
    bat_keys!(plain_keys load s, ob, l;);
    known.extend(bat_keys!(plain_keys names));
}

plain_struct!(ItemCarBuffExt { "base" = base, "car" = car, "canJump" = can_jump });

/// Car_Buff_C's extension as the oracle writes it.
pub fn car_buff_put(e: &ItemCarBuffExt, x: &Wx) -> Value {
    e.to_value(x)
}

/// [`car_buff_put`]'s inverse.
pub fn car_buff_load(e: &mut ItemCarBuffExt, v: &Value, l: &mut Lx) {
    e.load_value(v, l);
}
