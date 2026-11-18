//! `DisguiseShock` (AryaDisguiseShock_C) and `KillPie` (AryaKillPie_C), world actors: their
//! own keys and places. The shock stands at its owner's actor location (the record's x and y, vx and vy readonly 0);
//! the pie falls (x, y, vy; no vx).

use mvs_sim::chars::arya::actors::{DisguiseShock, KillPie};
use mvs_trace::{ActorRecord, Obj, Value};

use crate::actors::{opt_load, static_object};
use crate::conv::plain_keys;
use crate::cx::{Lx, Wx};

macro_rules! shock_keys {
    ($mac:ident $($pre:tt)*) => {
        $mac!($($pre)*
            "activated" = activated, "age" = age, "attackId" = attack_id, "blastAt" = blast_at,
            "blastBounds" = blast_bounds, "facing" = facing, "hit" = hit, "hitOn" = hit_on, "hitSet" = hit_set,
            "radius" = radius, "up" = up, "wait" = wait,
        )
    };
}

macro_rules! pie_keys {
    ($mac:ident $($pre:tt)*) => {
        $mac!($($pre)*
            "age" = age, "blastAt" = blast_at, "blastBounds" = blast_bounds, "eatRadius" = eat_radius,
            "eatUp" = eat_up, "life" = life, "over" = over, "radius" = radius, "stopped" = stopped,
        )
    };
}

/// The shock's own keys.
pub fn shock_put(s: &DisguiseShock, x: &Wx, r: &mut ActorRecord, o: &mut Vec<(String, Value)>) {
    r.x = Some(f64::from(s.x));
    r.y = Some(f64::from(s.y));
    r.vx = Some(0.0);
    r.vy = Some(0.0);
    shock_keys!(plain_keys put s, x, o;);
    o.push(("p".into(), static_object()));
}

/// [`shock_put`]'s inverse.
pub fn shock_load(s: &mut DisguiseShock, r: &ActorRecord, ob: &Obj, l: &mut Lx, known: &mut Vec<&'static str>) {
    opt_load(&mut s.x, r.x, "x", l);
    opt_load(&mut s.y, r.y, "y", l);
    l.at("rest", |l| {
        shock_keys!(plain_keys load s, ob, l;);
    });
    known.extend(shock_keys!(plain_keys names));
}

/// The pie's own keys.
pub fn pie_put(s: &KillPie, x: &Wx, r: &mut ActorRecord, o: &mut Vec<(String, Value)>) {
    r.x = Some(f64::from(s.x));
    r.y = Some(f64::from(s.y));
    r.vy = Some(f64::from(s.vy));
    pie_keys!(plain_keys put s, x, o;);
    o.push(("p".into(), static_object()));
}

/// [`pie_put`]'s inverse.
pub fn pie_load(s: &mut KillPie, r: &ActorRecord, ob: &Obj, l: &mut Lx, known: &mut Vec<&'static str>) {
    opt_load(&mut s.x, r.x, "x", l);
    opt_load(&mut s.y, r.y, "y", l);
    opt_load(&mut s.vy, r.vy, "vy", l);
    l.at("rest", |l| {
        pie_keys!(plain_keys load s, ob, l;);
    });
    known.extend(pie_keys!(plain_keys names));
}
