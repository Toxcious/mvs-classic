//! FartBomb_SpawnedActor as the record writes it: its own keys; `offs` is the pair of the
//! HitBox2D's and the Mesh's Z over the root.

use mvs_sim::chars::rick::fart_bomb::FartBomb;
use mvs_trace::{Obj, Value};

use crate::conv::{Plain, as_obj, check_keys, obj, plain_keys};
use crate::cx::{Lx, Wx};

macro_rules! bomb_keys {
    ($mac:ident $($pre:tt)*) => {
        $mac!($($pre)*
            "boxOn" = box_on, "detAt" = det_at, "offAt" = off_at, "meshAt" = mesh_at, "endAt" = end_at,
            "meshGone" = mesh_gone, "snapFast" = snap_fast, "kbAt" = kb_at, "groundAt" = ground_at,
        )
    };
}

/// A FartBomb_SpawnedActor_C's own keys.
pub fn fart_bomb_put(s: &FartBomb, x: &Wx, o: &mut Vec<(String, Value)>) {
    bomb_keys!(plain_keys put s, x, o;);
    o.push((
        "offs".into(),
        obj(vec![("hit", Value::Num(f64::from(s.hit_off))), ("mesh", Value::Num(f64::from(s.mesh_off)))]),
    ));
}

/// [`fart_bomb_put`]'s inverse; the keys it knows go into `known`.
pub fn fart_bomb_load(s: &mut FartBomb, ob: &Obj, l: &mut Lx, known: &mut Vec<&'static str>) {
    bomb_keys!(plain_keys load s, ob, l;);
    match ob.get("offs") {
        Some(v) => l.at("offs", |l| {
            let Some(o) = as_obj(v, l) else { return };
            for (k, to) in [("hit", &mut s.hit_off), ("mesh", &mut s.mesh_off)] {
                match o.get(k) {
                    Some(v) => l.at(k, |l| to.load_value(v, l)),
                    None => l.miss(format_args!("key {k} absent")),
                }
            }
            check_keys(o, &["hit", "mesh"], l);
        }),
        None => l.miss("key offs absent"),
    }
    known.extend(bomb_keys!(plain_keys names));
    known.push("offs");
}
