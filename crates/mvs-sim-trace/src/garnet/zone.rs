//! The up special's classes as the record writes them: GarnetLightningBolt_C and
//! GarnetElectricZone_C.

use mvs_sim::chars::garnet::zone::{ElectricZone, GarnetBolt};
use mvs_trace::{ActorRecord, Obj, Value};

use crate::actors::{opt_load, static_object};
use crate::conv::plain_keys;
use crate::cx::{Lx, Wx};

/// a GarnetLightningBolt_C as the record writes it.
pub fn bolt_put(s: &GarnetBolt, x: &Wx, o: &mut Vec<(String, Value)>) {
    plain_keys!(put s, x, o; "boltEnded" = bolt_ended, "projectileBlocked" = projectile_blocked,);
}

/// [`bolt_put`]'s inverse; the keys it knows go into `known`.
pub fn bolt_load(s: &mut GarnetBolt, ob: &Obj, l: &mut Lx, known: &mut Vec<&'static str>) {
    plain_keys!(load s, ob, l; "boltEnded" = bolt_ended, "projectileBlocked" = projectile_blocked,);
    known.extend(plain_keys!(names "boltEnded" = bolt_ended, "projectileBlocked" = projectile_blocked,));
}

macro_rules! zone_keys {
    ($mac:ident $($pre:tt)*) => {
        $mac!($($pre)*
            "atFullPower" = at_full_power, "attackId" = attack_id, "destroyFxBlocked" = destroy_fx_blocked,
            "destroyedEarly" = destroyed_early, "ended" = ended, "facing" = facing, "fullPowerAt" = full_power_at,
            "hit" = hit, "inside" = inside, "radius" = radius,
        )
    };
}

/// a GarnetElectricZone_C as the record writes it: the place (vx and vy readonly 0) and its own keys.
pub fn zone_put(s: &ElectricZone, x: &Wx, r: &mut ActorRecord, o: &mut Vec<(String, Value)>) {
    r.x = Some(f64::from(s.x));
    r.y = Some(f64::from(s.y));
    r.vx = Some(0.0);
    r.vy = Some(0.0);
    zone_keys!(plain_keys put s, x, o;);
    o.push(("p".into(), static_object()));
}

/// [`zone_put`]'s inverse; the keys it knows go into `known`.
pub fn zone_load(s: &mut ElectricZone, r: &ActorRecord, ob: &Obj, l: &mut Lx, known: &mut Vec<&'static str>) {
    opt_load(&mut s.x, r.x, "x", l);
    opt_load(&mut s.y, r.y, "y", l);
    l.at("rest", |l| {
        zone_keys!(plain_keys load s, ob, l;);
    });
    known.extend(zone_keys!(plain_keys names));
}
