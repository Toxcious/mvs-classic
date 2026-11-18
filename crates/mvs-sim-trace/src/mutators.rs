//! The fighter mutators' extensions as the oracle writes them (the fighter mutator's `{ left }`,
//! the grow-with-damage mutator's `{ dealt }` and its projectile buff's `{ size, damage }`).

use mvs_sim::combat::elem::ElemExt;
use mvs_sim::combat::grow_with_damage::GrowExt;
use mvs_sim::combat::mutator_buffs::InstantKillExt;
use mvs_trace::Value;

use crate::conv::{Plain, plain_struct};
use crate::cx::{Lx, Wx};

plain_struct!(InstantKillExt { "left" = left });
plain_struct!(GrowExt { "dealt" = dealt });

/// Mutator_InstantKill_C's extension.
pub fn instant_kill_put(e: &InstantKillExt, x: &Wx) -> Value {
    e.to_value(x)
}

/// [`instant_kill_put`]'s inverse.
pub fn instant_kill_load(e: &mut InstantKillExt, v: &Value, l: &mut Lx) {
    e.load_value(v, l);
}

/// Mutator_GrowWithDamage_C's extension.
pub fn grow_put(e: &GrowExt, x: &Wx) -> Value {
    e.to_value(x)
}

/// [`grow_put`]'s inverse.
pub fn grow_load(e: &mut GrowExt, v: &Value, l: &mut Lx) {
    e.load_value(v, l);
}

/// Buff_GrowWithDamageProjectile_C's extension on a spawned actor: `{ size, damage }`.
pub fn grow_projectile_put(size: f64, damage: f64, x: &Wx) -> Value {
    Value::Obj(mvs_trace::Obj::new(vec![
        ("damage".to_owned(), damage.to_value(x)),
        ("size".to_owned(), size.to_value(x)),
    ]))
}

/// [`grow_projectile_put`]'s inverse.
pub fn grow_projectile_load(v: &Value, l: &mut Lx) -> ElemExt {
    let (mut size, mut damage) = (0.0, 0.0);
    if let Value::Obj(o) = v {
        for (k, set) in [("size", &mut size), ("damage", &mut damage)] {
            match o.get(k) {
                Some(v) => l.at(k, |l| set.load_value(v, l)),
                None => l.miss(format_args!("key {k} absent")),
            }
        }
    } else {
        l.miss(format_args!("{v:?} for a grow projectile extension"));
    }
    ElemExt::GrowProjectile { size, damage }
}
