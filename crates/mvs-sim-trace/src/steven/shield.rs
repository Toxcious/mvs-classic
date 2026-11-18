//! Comp_StevenShield's classes as the record writes them: StevenCollisionBlock_C and
//! Buff_StevenSharedShield's extension.

use mvs_sim::actors::WorldActor;
use mvs_sim::chars::steven::shield::{SharedShield, StevenBlock};
use mvs_sim::world::spawned_platform::{PlatformPart, SpawnedPlatform};
use mvs_trace::{ActorRecord, Obj, Value};

use crate::actors::static_object;
use crate::conv::{Plain, plain_struct};
use crate::cx::{Lx, Wx};
use crate::shield_actor::{part_load, parts_put};

plain_struct!(SharedShield { "steven" = steven, "charges" = charges });

/// a StevenCollisionBlock_C as the record writes it: `p` (the class's defaults) and `solids` (its Box part, the
/// actor's `plat`; no place of its own).
pub fn block_put(a: &WorldActor, s: &StevenBlock, x: &Wx, r: &mut ActorRecord, o: &mut Vec<(String, Value)>) {
    let _ = (s, r);
    o.push(("p".into(), static_object()));
    match &a.plat {
        Some(pl) => o.push(("solids".into(), parts_put(&pl.parts, a.seq, x))),
        None => x.unmapped("a StevenCollisionBlock_C without its platform (dead at its spawn)"),
    }
}

/// [`block_put`]'s inverse; the keys it knows go into `known`. The platform is made where the actor has none: the Box
/// the one part (the parent's `collision` slot), its team the owner's (not written: 0 here, read only by the drop
/// through test, whose class data has no flag).
pub fn block_load(
    s: &mut StevenBlock,
    plat: &mut Option<SpawnedPlatform>,
    r: &ActorRecord,
    ob: &Obj,
    l: &mut Lx,
    known: &mut Vec<&'static str>,
) {
    let _ = (s, r);
    l.at("rest", |l| {
        let Some(Value::List(sl)) = ob.get("solids") else {
            l.miss("key solids absent");
            return;
        };
        let pl = plat.get_or_insert_with(|| SpawnedPlatform {
            age: 0,
            team: 0,
            collision: None,
            wallbounce: None,
            parts: Vec::new(),
            listed: true,
            collision_at: -1.0,
            edge_at: None,
        });
        if pl.parts.len() != sl.len() {
            let comp = l.name("Box");
            pl.parts =
                sl.iter().map(|_| PlatformPart::new(comp, false, 0.0, 0.0, &mvs_data::value::Props::new())).collect();
            pl.collision = (!sl.is_empty()).then_some(0);
        }
        for (i, (q, v)) in pl.parts.iter_mut().zip(sl).enumerate() {
            l.at(&format!("solids[{i}]"), |l| part_load(q, v, l));
        }
    });
    known.extend(["p", "solids"]);
}

/// Buff_StevenSharedShield_C's extension as the oracle writes it.
pub fn shared_shield_put(e: &SharedShield, x: &Wx) -> Value {
    e.to_value(x)
}

/// [`shared_shield_put`]'s inverse.
pub fn shared_shield_load(e: &mut SharedShield, v: &Value, l: &mut Lx) {
    e.load_value(v, l);
}
