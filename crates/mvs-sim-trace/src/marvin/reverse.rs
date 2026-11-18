//! Marvin's reversal buff as the record writes it: `{ t }`, the pause's ticks left.

use mvs_sim::chars::marvin::reverse::ReverseExt;
use mvs_trace::{Obj, Value};

use crate::conv::{as_obj, check_keys, plain_keys};
use crate::cx::{Lx, Wx};

/// Buff_C018ReverseProjectile_C's extension as the oracle writes it (`ElemBuff.ext`, `actors::elem_ext_put`).
pub fn reverse_put(e: &ReverseExt, x: &Wx) -> Value {
    let mut o: Vec<(String, Value)> = Vec::new();
    plain_keys!(put e, x, o; "t" = t);
    Value::Obj(Obj::new(o))
}

/// [`reverse_put`]'s inverse (`actors::elem_ext_load`, by the buff's class).
pub fn reverse_load(v: &Value, l: &mut Lx) -> ReverseExt {
    let mut e = ReverseExt::default();
    let Some(ob) = as_obj(v, l) else { return e };
    plain_keys!(load e, ob, l; "t" = t);
    check_keys(ob, plain_keys!(names "t" = t), l);
    e
}
