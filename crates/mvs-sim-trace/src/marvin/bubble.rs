//! Marvin's bubble and its bubbled-projectile buff as the record writes them: the
//! BubbleProj's `captured` and `hasProjectile`, and the buff's ext (the bubble, the velocity before the stop, the
//! flags, and what the carry keeps: `hurt`, `life`; keys the oracle omits while unset).

use mvs_sim::chars::marvin::bubble::{BubbleProj, BubbledExt};
use mvs_trace::{Obj, Value};

use crate::conv::{as_obj, check_keys, plain_keys};
use crate::cx::{Lx, Wx};

macro_rules! bubbled_keys {
    (names) => {
        plain_keys!(names "bubble" ~ bubble, "vx" ~ vx, "vy" ~ vy, "activated" ~ activated, "wait" ~ wait,
            "hurt" ~ carry.hurt, "life" ~ carry.life)
    };
    ($mode:ident $($a:ident),*;) => {
        plain_keys!($mode $($a),*; "bubble" ~ bubble, "vx" ~ vx, "vy" ~ vy, "activated" ~ activated, "wait" ~ wait,
            "hurt" ~ carry.hurt, "life" ~ carry.life)
    };
}

/// a C018BubbleProj_C as the record writes it.
pub fn bubble_put(s: &BubbleProj, x: &Wx, o: &mut Vec<(String, Value)>) {
    plain_keys!(put s, x, o; "captured" = captured, "hasProjectile" = has_projectile);
}

/// [`bubble_put`]'s inverse; the keys it knows go into `known`.
pub fn bubble_load(s: &mut BubbleProj, ob: &Obj, l: &mut Lx, known: &mut Vec<&'static str>) {
    plain_keys!(load s, ob, l; "captured" = captured, "hasProjectile" = has_projectile);
    known.extend_from_slice(plain_keys!(names "captured" = captured, "hasProjectile" = has_projectile));
}

/// Buff_C018BubbledProjectile_C's extension as the oracle writes it (`ElemBuff.ext`, `actors::elem_ext_put`).
pub fn bubbled_put(e: &BubbledExt, x: &Wx) -> Value {
    let mut o: Vec<(String, Value)> = Vec::new();
    bubbled_keys!(put e, x, o;);
    Value::Obj(Obj::new(o))
}

/// [`bubbled_put`]'s inverse (`actors::elem_ext_load`, by the buff's class).
pub fn bubbled_load(v: &Value, l: &mut Lx) -> BubbledExt {
    let mut e = BubbledExt::default();
    let Some(ob) = as_obj(v, l) else { return e };
    bubbled_keys!(load e, ob, l;);
    check_keys(ob, bubbled_keys!(names), l);
    e
}
