//! `IcePellet` (Superman_IceBreath_C, Superman_IceBreathEducated_C: a projectile's `ext`) and `IceStream`
//! (Superman_IceBreath_Stream_C).

use mvs_sim::chars::superman::ice::{IcePellet, IceStream};
use mvs_trace::{ActorRecord, Obj, Value};

use crate::conv::plain_keys;
use crate::cx::{Lx, Wx};

/// The keys a pellet adds to its projectile's `rest` (`unit`, `endAge`, `allies`, `inside`, `touching`).
pub fn pellet_put(p: &IcePellet, x: &Wx, o: &mut Vec<(String, Value)>) {
    plain_keys!(put p, x, o;
        "unit" = unit, "endAge" = end_age, "allies" = allies, "inside" = inside, "touching" = touching,
    );
}

/// [`pellet_put`]'s inverse.
pub fn pellet_load(p: &mut IcePellet, ob: &Obj, l: &mut Lx, known: &mut Vec<&'static str>) {
    plain_keys!(load p, ob, l;
        "unit" = unit, "endAge" = end_age, "allies" = allies, "inside" = inside, "touching" = touching,
    );
    known.extend(plain_keys!(names
        "unit" = unit, "endAge" = end_age, "allies" = allies, "inside" = inside, "touching" = touching,
    ));
}

/// The stream's own keys (`rotation`, `socket`, `p`); it has no place of its own (the record's x and y stay absent).
pub fn stream_put(s: &IceStream, x: &Wx, _r: &mut ActorRecord, o: &mut Vec<(String, Value)>) {
    plain_keys!(put s, x, o; "rotation" = rotation, "socket" = socket,);
    o.push(("p".into(), crate::actors::static_object()));
}

/// [`stream_put`]'s inverse.
pub fn stream_load(s: &mut IceStream, _r: &ActorRecord, ob: &Obj, l: &mut Lx, known: &mut Vec<&'static str>) {
    l.at("rest", |l| {
        plain_keys!(load s, ob, l; "rotation" = rotation, "socket" = socket,);
    });
    known.extend(plain_keys!(names "rotation" = rotation, "socket" = socket,));
}
