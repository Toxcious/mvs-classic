//! Finn's down attack's shockwave as the record writes it: the FinnShockwaveActor_C. The
//! actor's root never moves (the record's x and y, vx and vy readonly 0).

use mvs_sim::chars::finn::shockwave::{FinnShockwave, Sphere, Then};
use mvs_trace::{ActorRecord, Obj, Value};

use crate::actors::{opt_load, static_object};
use crate::conv::{named_enum, plain_keys, plain_struct};
use crate::cx::{Lx, Wx};

named_enum!(Then, "Finn shockwave step", [Then::Shock => "shock", Then::Stop => "stop", Then::Destroy => "destroy"]);

plain_struct!(Sphere { "fwd" = fwd, "r" = r, "ps" = ps, "psScale" = ps_scale });

macro_rules! wave_keys {
    ($mac:ident $($pre:tt)*) => {
        $mac!($($pre)*
            "age" = age, "attackId" = attack_id, "blastAt" = blast_at, "blastBounds" = blast_bounds,
            "decalAlpha" = decal_alpha, "decalT" = decal_t, "facing" = facing, "height" = height, "hit" = hit,
            "rot" = rot, "shocks" = shocks, "spheres" = spheres, "stage" = stage, "then" = then, "wait" = wait,
        )
    };
}

/// A FinnShockwaveActor_C as the record writes it.
pub fn shockwave_put(s: &FinnShockwave, x: &Wx, r: &mut ActorRecord, o: &mut Vec<(String, Value)>) {
    r.x = Some(f64::from(s.x));
    r.y = Some(f64::from(s.y));
    r.vx = Some(0.0);
    r.vy = Some(0.0);
    wave_keys!(plain_keys put s, x, o;);
    o.push(("p".into(), static_object()));
}

/// [`shockwave_put`]'s inverse; the keys it knows go into `known`.
pub fn shockwave_load(s: &mut FinnShockwave, r: &ActorRecord, ob: &Obj, l: &mut Lx, known: &mut Vec<&'static str>) {
    opt_load(&mut s.x, r.x, "x", l);
    opt_load(&mut s.y, r.y, "y", l);
    l.at("rest", |l| {
        wave_keys!(plain_keys load s, ob, l;);
    });
    known.extend(wave_keys!(plain_keys names));
}
