//! `Firebolt` (Firebolt_C and Firebolt_Child_C, a projectile's `ext`): the keys a firebolt
//! adds to its projectile's `rest`.

use mvs_sim::chars::creature::fire::Firebolt;
use mvs_trace::{Obj, Value};

use crate::conv::plain_keys;
use crate::cx::{Lx, Wx};

/// The keys a firebolt adds to its projectile's `rest`.
pub fn put(p: &Firebolt, x: &Wx, o: &mut Vec<(String, Value)>) {
    plain_keys!(put p, x, o;
        "left" = left, "gateClosed" = gate_closed, "gateWall" = gate_wall, "noDeathSpawn" = no_death_spawn,);
}

/// [`put`]'s inverse.
pub fn load(p: &mut Firebolt, ob: &Obj, l: &mut Lx, known: &mut Vec<&'static str>) {
    plain_keys!(load p, ob, l;
        "left" = left, "gateClosed" = gate_closed, "gateWall" = gate_wall, "noDeathSpawn" = no_death_spawn,);
    known.extend(plain_keys!(names
        "left" = left, "gateClosed" = gate_closed, "gateWall" = gate_wall, "noDeathSpawn" = no_death_spawn,));
}
