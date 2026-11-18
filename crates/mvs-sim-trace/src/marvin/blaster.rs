//! Marvin's blasters as the record writes them: the class's fields beside the projectile's
//! own, the private `ext` ({ X, Y, Z }) and `relX` included (the oracle object's own keys).

use mvs_sim::chars::marvin::blaster::Blaster;
use mvs_trace::{Obj, Value};

use crate::conv::{Plain, as_obj, check_keys, obj, plain_keys};
use crate::cx::{Lx, Wx};

macro_rules! blaster_keys {
    (names) => {
        plain_keys!(names
            "muzzleOn" = muzzle_on, "muzzleOffAge" = muzzle_off_age, "muzzleFrozen" = muzzle_frozen, "mx" = mx,
            "my" = my, "mz" = mz, "ux" = ux, "uy" = uy, "uz" = uz, "mScale" = m_scale, "relX" = rel_x)
    };
    ($mode:ident $($a:ident),*;) => {
        plain_keys!($mode $($a),*;
            "muzzleOn" = muzzle_on, "muzzleOffAge" = muzzle_off_age, "muzzleFrozen" = muzzle_frozen, "mx" = mx,
            "my" = my, "mz" = mz, "ux" = ux, "uy" = uy, "uz" = uz, "mScale" = m_scale, "relX" = rel_x)
    };
}

/// a blaster of Marvin as the record writes it.
pub fn blaster_put(s: &Blaster, x: &Wx, o: &mut Vec<(String, Value)>) {
    blaster_keys!(put s, x, o;);
    o.push((
        "ext".to_owned(),
        obj(vec![("X", s.ext_x.to_value(x)), ("Y", s.ext_y.to_value(x)), ("Z", s.ext_z.to_value(x))]),
    ));
}

/// [`blaster_put`]'s inverse; the keys it knows go into `known`.
pub fn blaster_load(s: &mut Blaster, ob: &Obj, l: &mut Lx, known: &mut Vec<&'static str>) {
    blaster_keys!(load s, ob, l;);
    known.extend_from_slice(blaster_keys!(names));
    known.push("ext");
    match ob.get("ext") {
        Some(v) => l.at("ext", |l| {
            let Some(e) = as_obj(v, l) else { return };
            for (k, slot) in [("X", &mut s.ext_x), ("Y", &mut s.ext_y), ("Z", &mut s.ext_z)] {
                match e.get(k) {
                    Some(v) => l.at(k, |l| slot.load_value(v, l)),
                    None => l.miss(format_args!("key {k} absent")),
                }
            }
            check_keys(e, &["X", "Y", "Z"], l);
        }),
        None => l.miss(format_args!("key ext absent")),
    }
}
