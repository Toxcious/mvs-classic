//! The heal and the Watermelon's classes as the record writes them: HealGrenade_C,
//! HealOverTimeZoneBP_C and Buff_Steven_GreenThumbDamageBoost's extension.

use mvs_sim::chars::steven::watermelon::{GreenThumbBoost, HealGrenade, HealZone};
use mvs_trace::{ActorRecord, Obj, Value};

use crate::actors::opt_load;
use crate::actors::static_object;
use crate::conv::{Plain, plain_keys, plain_struct};
use crate::cx::{Lx, Wx};

plain_struct!(GreenThumbBoost { "orig" = orig, "last" = last, "pos" = pos, "rate" = rate });

/// a HealGrenade_C as the record writes it: its own fields (the velocity is the base's).
pub fn grenade_put(s: &HealGrenade, x: &Wx, o: &mut Vec<(String, Value)>) {
    plain_keys!(put s, x, o; "radius" = radius, "pvx" = pvx, "pvy" = pvy, "spin" = spin, "wa" = wa, "wb" = wb, "wc" = wc, "angle" = angle, "armAt" = arm_at, "detonateAt" = detonate_at, "stevenSpawned" = steven_spawned,);
}

/// [`grenade_put`]'s inverse; the keys it knows go into `known`.
pub fn grenade_load(s: &mut HealGrenade, ob: &Obj, l: &mut Lx, known: &mut Vec<&'static str>) {
    l.at("rest", |l| {
        plain_keys!(load s, ob, l; "radius" = radius, "pvx" = pvx, "pvy" = pvy, "spin" = spin, "wa" = wa, "wb" = wb, "wc" = wc, "angle" = angle, "armAt" = arm_at, "detonateAt" = detonate_at, "stevenSpawned" = steven_spawned,);
    });
    known.extend(plain_keys!(names "radius" = radius, "pvx" = pvx, "pvy" = pvy, "spin" = spin, "wa" = wa, "wb" = wb, "wc" = wc, "angle" = angle, "armAt" = arm_at, "detonateAt" = detonate_at, "stevenSpawned" = steven_spawned,));
}

/// a HealOverTimeZoneBP_C as the record writes it: its place (the base's `x`, `y`) and the rest.
pub fn heal_zone_put(s: &HealZone, x: &Wx, r: &mut ActorRecord, o: &mut Vec<(String, Value)>) {
    r.x = Some(f64::from(s.x));
    r.y = Some(f64::from(s.y));
    plain_keys!(put s, x, o; "attackId" = attack_id, "fogActivated" = fog_activated, "fogEndAt" = fog_end_at, "inside" = inside, "radius" = radius,);
    o.push(("p".into(), static_object()));
}

/// [`heal_zone_put`]'s inverse; the keys it knows go into `known`.
pub fn heal_zone_load(s: &mut HealZone, r: &ActorRecord, ob: &Obj, l: &mut Lx, known: &mut Vec<&'static str>) {
    opt_load(&mut s.x, r.x, "x", l);
    opt_load(&mut s.y, r.y, "y", l);
    l.at("rest", |l| {
        plain_keys!(load s, ob, l; "attackId" = attack_id, "fogActivated" = fog_activated, "fogEndAt" = fog_end_at, "inside" = inside, "radius" = radius,);
    });
    known.extend(plain_keys!(names "attackId" = attack_id, "fogActivated" = fog_activated, "fogEndAt" = fog_end_at, "inside" = inside, "radius" = radius,));
}

/// Buff_Steven_GreenThumbDamageBoost_C's extension as the oracle writes it.
pub fn green_thumb_put(e: &GreenThumbBoost, x: &Wx) -> Value {
    e.to_value(x)
}

/// [`green_thumb_put`]'s inverse.
pub fn green_thumb_load(e: &mut GreenThumbBoost, v: &Value, l: &mut Lx) {
    e.load_value(v, l);
}
