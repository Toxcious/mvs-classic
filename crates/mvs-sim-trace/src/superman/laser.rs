//! `LaserSweep` (SupermanGroundLazer_C, SupermanGroundLazerAir_C) and `LaserDamage` (SupermanGroundLazerDamage_C).

use mvs_sim::chars::superman::laser::{LaserDamage, LaserSweep};
use mvs_trace::{ActorRecord, Obj, Value};

use crate::actors::{opt_load, static_object};
use crate::conv::plain_keys;
use crate::cx::{Lx, Wx};

macro_rules! sweep_keys {
    ($mac:ident $($pre:tt)*) => {
        $mac!($($pre)*
            "age" = age, "alpha" = alpha, "attackId" = attack_id, "blastAt" = blast_at, "blastBounds" = blast_bounds, "bound" = bound,
            "damage" = damage, "duration" = duration, "endY" = end_y, "endZ" = end_z, "ended" = ended, "ex" = ex, "ey" = ey,
            "hit" = hit, "ignited" = ignited, "length" = length, "nx" = nx, "ny" = ny, "passThrough" = pass_through,
            "startY" = start_y, "startZ" = start_z, "sx" = sx, "sy" = sy, "track" = track,
        )
    };
}

macro_rules! damage_keys {
    ($mac:ident $($pre:tt)*) => {
        $mac!($($pre)*
            "activated" = activated, "animTime" = anim_time, "attackId" = attack_id, "exploding" = exploding, "facing" = facing,
            "hd" = hd, "hit" = hit, "hitOn" = hit_on, "hitSet" = hit_set, "hx" = hx, "hy" = hy, "lastIndex" = last_index,
            "once" = once, "over" = over, "tail" = tail, "time" = time, "track" = track, "types" = types,
        )
    };
}

/// The sweep's own keys (it has no position of its own).
pub fn sweep_put(s: &LaserSweep, x: &Wx, _r: &mut ActorRecord, o: &mut Vec<(String, Value)>) {
    sweep_keys!(plain_keys put s, x, o;);
    o.push(("p".into(), static_object()));
}

/// [`sweep_put`]'s inverse.
pub fn sweep_load(s: &mut LaserSweep, _r: &ActorRecord, ob: &Obj, l: &mut Lx, known: &mut Vec<&'static str>) {
    l.at("rest", |l| {
        sweep_keys!(plain_keys load s, ob, l;);
    });
    known.extend(sweep_keys!(plain_keys names));
}

/// The damage actor's own keys.
pub fn damage_put(s: &LaserDamage, x: &Wx, r: &mut ActorRecord, o: &mut Vec<(String, Value)>) {
    r.x = Some(f64::from(s.x));
    r.y = Some(f64::from(s.y));
    r.vx = Some(0.0);
    r.vy = Some(0.0);
    damage_keys!(plain_keys put s, x, o;);
    o.push(("p".into(), static_object()));
}

/// [`damage_put`]'s inverse.
pub fn damage_load(s: &mut LaserDamage, r: &ActorRecord, ob: &Obj, l: &mut Lx, known: &mut Vec<&'static str>) {
    opt_load(&mut s.x, r.x, "x", l);
    opt_load(&mut s.y, r.y, "y", l);
    l.at("rest", |l| {
        damage_keys!(plain_keys load s, ob, l;);
    });
    known.extend(damage_keys!(plain_keys names));
}
