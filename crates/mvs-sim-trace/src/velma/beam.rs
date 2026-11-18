//! Velma's beam as the record writes it: the VelmaHealBeam_C / VelmaChildBeam_C actor and
//! Buff_VelmaHealOverTimeBlast_C's extension. The actor's own keys beyond the record's rows go into `rest`; `holder`
//! (the Fighter the oracle keeps) is no number, so the record's row leaves it undefined and `rest` skips it.

use mvs_sim::chars::velma::beam::{Beam, BlastExt};
use mvs_trace::{ActorRecord, Obj, Value};

use crate::actors::{opt_load, static_object};
use crate::conv::{as_obj, check_keys, obj, plain_keys};
use crate::cx::{Lx, Wx};

macro_rules! beam_keys {
    ($mac:ident $($pre:tt)*) => {
        $mac!($($pre)*
            "attackType" = attack_type, "begun" = begun, "beamSet" = beam_set, "dieSet" = die_set,
            "dirY" = dir_y, "dirZ" = dir_z, "duration" = duration, "endX" = end_x, "endY" = end_y,
            "final" = final_hit, "hit" = hit, "lastHit" = last_hit, "length" = length, "on" = on,
            "pitch" = pitch, "pivot" = pivot, "projN" = proj_n, "projSet" = proj_set, "reachX" = reach_x,
            "reachY" = reach_y, "startTick" = start_tick, "storedLeft" = stored_left, "vfxN" = vfx_n,
            "vfxSet" = vfx_set,
        )
    };
}

/// A VelmaHealBeam_C / VelmaChildBeam_C as the record writes it (`attackId` is the class's readonly 0).
pub fn beam_put(s: &Beam, x: &Wx, r: &mut ActorRecord, o: &mut Vec<(String, Value)>) {
    r.x = Some(f64::from(s.x));
    r.y = Some(f64::from(s.y));
    r.vx = Some(0.0);
    r.vy = Some(0.0);
    o.push(("attackId".into(), Value::Num(0.0)));
    beam_keys!(plain_keys put s, x, o;);
    o.push(("p".into(), static_object()));
}

/// [`beam_put`]'s inverse; the keys it knows go into `known`.
pub fn beam_load(s: &mut Beam, r: &ActorRecord, ob: &Obj, l: &mut Lx, known: &mut Vec<&'static str>) {
    opt_load(&mut s.x, r.x, "x", l);
    opt_load(&mut s.y, r.y, "y", l);
    l.at("rest", |l| {
        beam_keys!(plain_keys load s, ob, l;);
    });
    known.push("attackId");
    known.extend(beam_keys!(plain_keys names));
}

const BLAST_KEYS: [&str; 4] = ["from", "life", "n", "set"];

/// Buff_VelmaHealOverTimeBlast_C's extension as the oracle writes it: `{ from, set, n, life }`.
pub fn blast_put(e: &BlastExt, x: &Wx) -> Value {
    let mut o: Vec<(String, Value)> = Vec::new();
    plain_keys!(put e, x, o; "from" = from, "life" = life, "n" = n, "set" = set,);
    obj(o.iter().map(|(k, v)| (k.as_str(), v.clone())).collect())
}

/// [`blast_put`]'s inverse.
pub fn blast_load(e: &mut BlastExt, v: &Value, l: &mut Lx) {
    let Some(o) = as_obj(v, l) else { return };
    plain_keys!(load e, o, l; "from" = from, "life" = life, "n" = n, "set" = set,);
    check_keys(o, &BLAST_KEYS, l);
}
