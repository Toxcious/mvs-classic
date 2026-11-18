//! Gizmo's perk buffs' extensions as the record writes them.
//!
//! The Backpack's SpawnedBuffs are whole buff entries of its holder's list in the oracle, written through the
//! references ([`crate::black_adam::chain::buff_entry_put`]); read back by [`resolve_backpack_spawned`] once every
//! fighter has its buffs (the entries are the holder's own ChargeSpeed buffs).

use mvs_sim::chars::gizmo::perks::{BACKPACK, BackpackExt, CHARGE_SPEED, DanceCountExt};
use mvs_sim::combat::{BuffExt, BuffRef};
use mvs_trace::record::WorldRecord;
use mvs_trace::{Obj, Value};

use crate::black_adam::chain::{buff_entry_gone_load, buff_entry_put};
use crate::conv::{Plain, as_obj, check_keys, plain_struct};
use crate::cx::{Lx, Wx};
use mvs_sim::world::World;

plain_struct!(DanceCountExt { "from" = from, "wait" = wait });

/// Buff_C023A_Backpack_C's extension as the oracle writes it.
pub fn backpack_put(e: &BackpackExt, x: &Wx) -> Value {
    let spawned = e.spawned.iter().map(|&b| buff_entry_put(Some(b), x)).collect();
    Value::Obj(Obj::new(vec![("bound".to_owned(), Value::Bool(e.bound)), ("spawned".to_owned(), Value::List(spawned))]))
}

/// [`backpack_put`]'s inverse but the spawned buffs (left empty for [`resolve_backpack_spawned`]).
pub fn backpack_load(e: &mut BackpackExt, v: &Value, l: &mut Lx) {
    let Some(ob) = as_obj(v, l) else { return };
    match ob.get("bound") {
        Some(Value::Bool(b)) => e.bound = *b,
        _ => l.miss("the Backpack's bound is no boolean"),
    }
    check_keys(ob, &["bound", "spawned"], l);
    e.spawned.clear();
}

/// C023A_DanceCount_Debuff_C's extension as the oracle writes it.
pub fn dance_count_put(e: &DanceCountExt, x: &Wx) -> Value {
    e.to_value(x)
}

/// [`dance_count_put`]'s inverse.
pub fn dance_count_load(e: &mut DanceCountExt, v: &Value, l: &mut Lx) {
    e.load_value(v, l);
}

/// Each Backpack's SpawnedBuffs from the record: an entry still in its holder's list is its next unused
/// ChargeSpeed buff, one gone is kept as it was last seen ([`buff_entry_gone_load`]).
pub fn resolve_backpack_spawned(w: &mut World, r: &WorldRecord, l: &mut Lx) {
    let ids: Vec<_> = w.pawns.clone().unwrap_or_else(|| w.players.clone());
    for (&id, fr) in ids.iter().zip(&r.fighters) {
        for (j, br) in fr.buffs.iter().enumerate() {
            if br.class != BACKPACK {
                continue;
            }
            let Value::Obj(eo) = &br.ext else { continue };
            let Some(Value::List(items)) = eo.get("spawned") else { continue };
            let mut used: Vec<u32> = Vec::new();
            let mut out: Vec<BuffRef> = Vec::new();
            for (k, it) in items.iter().enumerate() {
                let live = matches!(it, Value::Obj(o) if !matches!(o.get("swept"), Some(Value::Bool(true))));
                let found = live
                    .then(|| {
                        w.fighter(id)
                            .buffs
                            .buffs
                            .iter()
                            .find(|b| w.names.str(b.cls) == CHARGE_SPEED && !used.contains(&b.serial))
                            .map(|b| b.serial)
                    })
                    .flatten();
                match found {
                    Some(serial) => {
                        used.push(serial);
                        out.push(BuffRef { fighter: id, serial });
                    }
                    None => out.push(buff_entry_gone_load(it, id, u32::MAX - 100 - k as u32, l)),
                }
            }
            if let Some(b) = w.fighter_mut(id).buffs.buffs.get_mut(j)
                && let BuffExt::Backpack(e) = &mut b.ext
            {
                e.spawned = out;
            }
        }
    }
}
