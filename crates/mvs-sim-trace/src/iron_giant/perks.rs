//! Iron Giant's perks as the record writes them (the
//! DamageReflect, FastWeaponMode, ScorchedEarth and C017_TempHealth_DamageReflect_Passive extensions.
//!
//! DamageReflect's `thorns` is the whole buff entry the oracle keeps: written from the owner's list through the
//! reference, read back as a reference to a marker serial that [`resolve_thorns`] (after the World has its fighters'
//! buffs) turns into the first Buff_Thorns_C of the same list.

use mvs_sim::chars::iron_giant::perks::{DamageReflect, FastWeapon, Scorch};
use mvs_sim::combat::BuffRef;
use mvs_sim::combat::buffs::BuffExt;
use mvs_sim::world::World;
use mvs_trace::Value;

use crate::conv::{Plain, as_obj, check_keys, obj, plain_struct};
use crate::cx::{Lx, Wx};

/// The serial a loaded `thorns` waits with.
const PENDING_THORNS: u32 = u32::MAX - 5;
const THORNS: &str = "Buff_Thorns_C";

plain_struct!(FastWeapon { "wait" = wait });
plain_struct!(Scorch {
    "thruster" = thruster, "bound" = bound, "slots" = slots, "usedL" = used_l, "usedR" = used_r, "source" = source,
    "lx" = lx, "ly" = ly, "rx" = rx, "ry" = ry, "left" = left, "right" = right,
});

/// Buff_C017_FastWeaponMode's extension as the oracle writes it.
pub fn fast_put(e: &FastWeapon, x: &Wx) -> Value {
    e.to_value(x)
}

/// [`fast_put`]'s inverse.
pub fn fast_load(e: &mut FastWeapon, v: &Value, l: &mut Lx) {
    e.load_value(v, l);
}

/// Buff_C017_ScorchedEarth's extension as the oracle writes it.
pub fn scorch_put(e: &Scorch, x: &Wx) -> Value {
    e.to_value(x)
}

/// [`scorch_put`]'s inverse.
pub fn scorch_load(e: &mut Scorch, v: &Value, l: &mut Lx) {
    e.load_value(v, l);
}

/// C017_TempHealth_DamageReflect_Passive's extension as the oracle writes it: `thorns` the whole entry or null, `types`
/// left out until the marker's first add.
pub fn reflect_put(e: &DamageReflect, x: &Wx) -> Value {
    let mut o: Vec<(&str, Value)> = vec![("thorns", crate::black_adam::chain::buff_entry_put(e.thorns, x))];
    if let Some(t) = &e.types {
        o.push(("types", Value::List(t.iter().map(|n| Value::Str(x.str(*n).to_owned())).collect())));
    }
    obj(o)
}

/// [`reflect_put`]'s inverse (`thorns` a marker for [`resolve_thorns`]).
pub fn reflect_load(e: &mut DamageReflect, v: &Value, l: &mut Lx) {
    let Some(o) = as_obj(v, l) else { return };
    e.thorns = match o.get("thorns") {
        None | Some(Value::Null) => None,
        Some(Value::Obj(_)) => match l.fighters.first() {
            // (any fighter: the pass puts the owner's)
            Some(&(_, fighter)) => Some(BuffRef { fighter, serial: PENDING_THORNS }),
            None => {
                l.at("thorns", |l| l.miss("a held buff with no fighter in the World"));
                None
            }
        },
        Some(_) => {
            l.at("thorns", |l| l.miss("thorns is no object"));
            None
        }
    };
    e.types = match o.get("types") {
        None => None,
        Some(Value::List(a)) => {
            let mut t = Vec::with_capacity(a.len());
            for s in a {
                match s {
                    Value::Str(s) => t.push(l.name(s)),
                    _ => l.at("types", |l| l.miss("a TempHealthTypes entry that is no string")),
                }
            }
            Some(t)
        }
        Some(_) => {
            l.at("types", |l| l.miss("types is no list"));
            None
        }
    };
    check_keys(o, &["thorns", "types"], l);
}

/// Turns the markers [`reflect_load`] left into the Buff_Thorns_C of the reflect buff's own fighter (the first).
pub fn resolve_thorns(w: &mut World, l: &mut Lx) {
    let mut fix: Vec<(mvs_sim::fighter::FighterId, u32, u32)> = Vec::new();
    for (id, f) in w.fighters.iter() {
        for b in &f.buffs.buffs {
            let BuffExt::DamageReflect(e) = &b.ext else { continue };
            if e.thorns.is_none_or(|t| t.serial != PENDING_THORNS) {
                continue;
            }
            match f.buffs.buffs.iter().find(|t| w.names.str(t.cls) == THORNS) {
                Some(t) => fix.push((id, b.serial, t.serial)),
                None => l.miss(format_args!("a DamageReflect's thorns buff is in no list of its fighter")),
            }
        }
    }
    for (id, serial, thorns) in fix {
        if let Some(b) = w.fighter_mut(id).buffs.buffs.iter_mut().find(|b| b.serial == serial)
            && let BuffExt::DamageReflect(e) = &mut b.ext
        {
            e.thorns = Some(BuffRef { fighter: id, serial: thorns });
        }
    }
}
