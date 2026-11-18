//! Bugs Bunny's ACME box as the record writes it: no place, `p` (the class's parameters:
//! static data) and the gate's `closed` and `reopen`.

use mvs_sim::chars::bugs::dispenser::Dispenser;
use mvs_trace::{ActorRecord, Obj, Value};

use crate::actors::static_object;
use crate::conv::plain_keys;
use crate::cx::{Lx, Wx};

/// An ItemDispenser_Bugs_C as the record writes it.
pub fn dispenser_put(s: &Dispenser, x: &Wx, _r: &mut ActorRecord, o: &mut Vec<(String, Value)>) {
    o.push(("p".into(), static_object()));
    plain_keys!(put s, x, o; "closed" = closed, "reopen" = reopen,);
}

/// [`dispenser_put`]'s inverse; the keys it knows go into `known`.
pub fn dispenser_load(s: &mut Dispenser, _r: &ActorRecord, ob: &Obj, l: &mut Lx, known: &mut Vec<&'static str>) {
    plain_keys!(load s, ob, l; "closed" = closed, "reopen" = reopen,);
    known.extend(plain_keys!(names "closed" = closed, "reopen" = reopen,));
}
