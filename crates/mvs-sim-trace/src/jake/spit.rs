//! ProjectileAimer_SpitJake and JakeSpitCharacterHitBox as the record writes them. `p` is the
//! class's static data; the hit box's `vx`, `vy` are readonly 0 and its `facing` readonly -1.

use mvs_sim::chars::jake::spit::{SpitAimer, SpitHitBox};
use mvs_trace::{ActorRecord, Obj, Value};

use crate::actors::{opt_load, static_object};
use crate::conv::plain_keys;
use crate::cx::{Lx, Wx};

macro_rules! aimer_keys {
    ($mac:ident $($pre:tt)*) => {
        $mac!($($pre)*
            "age" = age, "arc" = arc, "ay" = ay, "az" = az, "blastAt" = blast_at, "blastBounds" = blast_bounds,
            "count" = count, "fl" = fl, "sy" = sy, "sz" = sz, "up" = up,
        )
    };
}

macro_rules! hit_box_keys {
    ($mac:ident $($pre:tt)*) => {
        $mac!($($pre)*
            "age" = age, "attackId" = attack_id, "blastAt" = blast_at, "blastBounds" = blast_bounds,
            "bound" = bound, "endAt" = end_at, "hit" = hit, "hitOn" = hit_on, "hitSet" = hit_set,
            "initialSpeed" = initial_speed, "kbSeq" = kb_seq, "maxSpeed" = max_speed, "onAt" = on_at,
            "parent" = parent, "radius" = radius, "stunEnd" = stun_end,
        )
    };
}

/// A ProjectileAimer_SpitJake_C as the record writes it.
pub fn aimer_put(s: &SpitAimer, x: &Wx, r: &mut ActorRecord, o: &mut Vec<(String, Value)>) {
    r.x = Some(f64::from(s.x));
    r.y = Some(f64::from(s.y));
    aimer_keys!(plain_keys put s, x, o;);
    o.push(("p".into(), static_object()));
}

/// [`aimer_put`]'s inverse; the keys it knows go into `known`.
pub fn aimer_load(s: &mut SpitAimer, r: &ActorRecord, ob: &Obj, l: &mut Lx, known: &mut Vec<&'static str>) {
    opt_load(&mut s.x, r.x, "x", l);
    opt_load(&mut s.y, r.y, "y", l);
    l.at("rest", |l| {
        aimer_keys!(plain_keys load s, ob, l;);
    });
    known.extend(aimer_keys!(plain_keys names));
}

/// A JakeSpitCharacterHitBox_C as the record writes it.
pub fn hit_box_put(s: &SpitHitBox, x: &Wx, r: &mut ActorRecord, o: &mut Vec<(String, Value)>) {
    r.x = Some(f64::from(s.x));
    r.y = Some(f64::from(s.y));
    r.vx = Some(0.0);
    r.vy = Some(0.0);
    hit_box_keys!(plain_keys put s, x, o;);
    o.push(("facing".into(), Value::Num(-1.0)));
    o.push(("p".into(), static_object()));
}

/// [`hit_box_put`]'s inverse; the keys it knows go into `known`.
pub fn hit_box_load(s: &mut SpitHitBox, r: &ActorRecord, ob: &Obj, l: &mut Lx, known: &mut Vec<&'static str>) {
    opt_load(&mut s.x, r.x, "x", l);
    opt_load(&mut s.y, r.y, "y", l);
    l.at("rest", |l| {
        hit_box_keys!(plain_keys load s, ob, l;);
    });
    known.extend(hit_box_keys!(plain_keys names));
    known.push("facing");
}
