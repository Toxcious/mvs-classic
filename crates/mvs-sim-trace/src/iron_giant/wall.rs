//! Iron Giant's wall as the record writes it (a PlatformActor: its part the actor's
//! `plat`).

use mvs_sim::actors::world_actor::WorldActor;
use mvs_sim::chars::iron_giant::wall::WallPlatform;
use mvs_sim::world::spawned_platform::SpawnedPlatform;
use mvs_trace::{ActorRecord, Obj, Value};

use crate::actors::{opt_load, static_object};
use crate::conv::plain_keys;
use crate::cx::{Lx, Wx};
use crate::shield_actor::{parts_put, plat_load, plat_put};

macro_rules! wall_keys {
    ($mac:ident $($pre:tt)*) => {
        $mac!($($pre)*
            "age" = age, "blastAt" = blast_at, "blastBounds" = blast_bounds, "recheckAt" = recheck_at, "rx" = rx,
            "rz" = rz,
        )
    };
}

/// the wall's own keys and place (`a`: its world actor, for the platform).
pub fn wall_put(a: &WorldActor, s: &WallPlatform, x: &Wx, r: &mut ActorRecord, o: &mut Vec<(String, Value)>) {
    r.x = Some(f64::from(s.x));
    r.y = Some(f64::from(s.y));
    wall_keys!(plain_keys put s, x, o;);
    o.push(("p".into(), static_object()));
    match &a.plat {
        Some(pl) => {
            o.push(("plat".into(), plat_put(pl, a.seq, x)));
            o.push(("solids".into(), parts_put(&pl.parts, a.seq, x)));
        }
        None => x.unmapped("a wall without its SpawnedPlatform (dead at its spawn)"),
    }
}

/// [`wall_put`]'s inverse.
pub fn wall_load(
    s: &mut WallPlatform,
    plat: &mut Option<SpawnedPlatform>,
    r: &ActorRecord,
    ob: &Obj,
    l: &mut Lx,
    known: &mut Vec<&'static str>,
) {
    opt_load(&mut s.x, r.x, "x", l);
    opt_load(&mut s.y, r.y, "y", l);
    l.at("rest", |l| {
        wall_keys!(plain_keys load s, ob, l;);
        match ob.get("plat") {
            Some(v) => l.at("plat", |l| plat_load(plat, v, l)),
            None => l.miss("key plat absent"),
        }
    });
    known.extend(wall_keys!(plain_keys names));
    known.extend(["plat", "solids"]);
}
