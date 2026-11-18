//! JakeStretch as the record writes them. The actor stands at its place (the record's x and
//! y, vx and vy readonly 0); `p` is the class's static data.

use mvs_sim::actors::spline::SplineCurve;
use mvs_sim::chars::jake::stretch::{Stretch, Wiggle};
use mvs_trace::{ActorRecord, Obj, Value};

use crate::actors::{opt_load, static_object};
use crate::conv::{plain_keys, plain_struct};
use crate::cx::{Lx, Wx};

plain_struct!(Wiggle { "pt" = pt, "sx" = sx, "sy" = sy, "dx" = dx, "dy" = dy, "t" = t });
plain_struct!(SplineCurve { "ox" = ox, "oy" = oy, "pts" = pts, "sx" = sx, "table" = table });

macro_rules! stretch_keys {
    ($mac:ident $($pre:tt)*) => {
        $mac!($($pre)*
            "actTick" = act_tick, "armor" = armor, "attackId" = attack_id, "bounceHouse" = bounce_house,
            "bounceMax" = bounce_max, "bounceMin" = bounce_min, "bounceMul" = bounce_mul, "bounced" = bounced,
            "count" = count, "dims" = dims, "ends" = ends, "facing" = facing, "facingLeft" = facing_left, "hit" = hit,
            "hitIds" = hit_ids, "inc" = inc, "jake" = jake, "kbSeq" = kb_seq, "knockIn" = knock_in,
            "knockOn" = knock_on, "lastX" = last_x, "lastY" = last_y, "launchX" = launch_x, "launchY" = launch_y,
            "legPitch" = leg_pitch, "legRelX" = leg_rel_x, "legRelY" = leg_rel_y, "legSaveX" = leg_save_x,
            "legSaveY" = leg_save_y, "legX" = leg_x, "legY" = leg_y, "length" = length, "meshDims" = mesh_dims,
            "meshEnds" = mesh_ends, "meshScale" = mesh_scale, "meshScales" = mesh_scales, "newCount" = new_count,
            "pitchDX" = pitch_dx, "pitchDY" = pitch_dy, "playing" = playing, "points" = points,
            "retracting" = retracting, "scales" = scales, "spikeMin" = spike_min, "spikeMul" = spike_mul,
            "spline" = spline, "sticky" = sticky, "taperDist" = taper_dist, "taperMin" = taper_min,
            "ticking" = ticking, "tl" = tl, "total" = total, "touching" = touching, "velX" = vel_x, "velY" = vel_y,
            "wiggles" = wiggles,
        )
    };
}

/// A JakeStretch_C as the record writes it.
pub fn stretch_put(s: &Stretch, x: &Wx, r: &mut ActorRecord, o: &mut Vec<(String, Value)>) {
    r.x = Some(f64::from(s.x));
    r.y = Some(f64::from(s.y));
    r.vx = Some(0.0);
    r.vy = Some(0.0);
    stretch_keys!(plain_keys put s, x, o;);
    o.push(("p".into(), static_object()));
}

/// [`stretch_put`]'s inverse; the keys it knows go into `known`.
pub fn stretch_load(s: &mut Stretch, r: &ActorRecord, ob: &Obj, l: &mut Lx, known: &mut Vec<&'static str>) {
    opt_load(&mut s.x, r.x, "x", l);
    opt_load(&mut s.y, r.y, "y", l);
    l.at("rest", |l| {
        stretch_keys!(plain_keys load s, ob, l;);
    });
    known.extend(stretch_keys!(plain_keys names));
}
