//! Tom and Jerry's tennis ball and his aimer without Jerry as the record writes them.

use mvs_sim::chars::tom_and_jerry::tennis::{NoJerryAim, TennisBall};
use mvs_trace::{ActorRecord, Obj, Value};

use crate::actors::{opt_load, static_object};
use crate::conv::plain_keys;
use crate::cx::{Lx, Wx};

macro_rules! ball_keys {
    ($mac:ident $($pre:tt)*) => {
        $mac!($($pre)*
            "aimZ" = aim_z, "bounceNumber" = bounce_number, "doOnceClosed" = do_once_closed,
            "endBound" = end_bound, "hitRel" = hit_rel, "homeMag" = home_mag, "homeOn" = home_on,
            "homeTarget" = home_target, "instigatingTom" = instigating_tom, "isReflected" = is_reflected,
            "jerry" = jerry, "killOnHit" = kill_on_hit, "maxBounce" = max_bounce, "overJerry" = over_jerry,
            "particleVisible" = particle_visible,
        )
    };
}

macro_rules! aim_keys {
    ($mac:ident $($pre:tt)*) => {
        $mac!($($pre)*
            "aimDirection" = aim_direction, "aimX" = aim_x, "aimY" = aim_y, "jerry" = jerry, "wait" = wait,
        )
    };
}

/// A Tom_and_Jerry_TennisBall_C's own keys.
pub fn ball_put(s: &TennisBall, x: &Wx, o: &mut Vec<(String, Value)>) {
    ball_keys!(plain_keys put s, x, o;);
}

/// [`ball_put`]'s inverse; the keys it knows go into `known`.
pub fn ball_load(s: &mut TennisBall, ob: &Obj, l: &mut Lx, known: &mut Vec<&'static str>) {
    ball_keys!(plain_keys load s, ob, l;);
    known.extend(ball_keys!(plain_keys names));
}

/// A TomAndJerry_NoJerry_SlingShot_Aim_C's own keys and place.
pub fn no_jerry_aim_put(s: &NoJerryAim, x: &Wx, r: &mut ActorRecord, o: &mut Vec<(String, Value)>) {
    r.x = Some(f64::from(s.x));
    r.y = Some(f64::from(s.y));
    aim_keys!(plain_keys put s, x, o;);
    o.push(("p".into(), static_object()));
}

/// [`no_jerry_aim_put`]'s inverse; the keys it knows go into `known`.
pub fn no_jerry_aim_load(s: &mut NoJerryAim, r: &ActorRecord, ob: &Obj, l: &mut Lx, known: &mut Vec<&'static str>) {
    opt_load(&mut s.x, r.x, "x", l);
    opt_load(&mut s.y, r.y, "y", l);
    l.at("rest", |l| {
        aim_keys!(plain_keys load s, ob, l;);
    });
    known.extend(aim_keys!(plain_keys names));
    known.push("p");
}
