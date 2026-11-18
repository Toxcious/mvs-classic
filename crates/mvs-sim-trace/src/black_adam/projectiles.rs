//! `HomingBall` (C021_Homing_Projectile_C, a projectile's `ext`): the keys a
//! homing ball adds to its projectile's `rest`.

use mvs_sim::chars::black_adam::projectiles::HomingBall;
use mvs_trace::{Obj, Value};

use crate::conv::plain_keys;
use crate::cx::{Lx, Wx};

macro_rules! ball_keys {
    ($mac:ident $($pre:tt)*) => {
        $mac!($($pre)*
            "enableAt" = enable_at, "homeAt" = home_at, "homingMag" = homing_mag, "homingOn" = homing_on,
            "pickupFrom" = pickup_from, "pickupOn" = pickup_on,
        )
    };
}

/// The keys a homing ball adds to its projectile's `rest`.
pub fn put(p: &HomingBall, x: &Wx, o: &mut Vec<(String, Value)>) {
    ball_keys!(plain_keys put p, x, o;);
}

/// [`put`]'s inverse.
pub fn load(p: &mut HomingBall, ob: &Obj, l: &mut Lx, known: &mut Vec<&'static str>) {
    ball_keys!(plain_keys load p, ob, l;);
    known.extend(ball_keys!(plain_keys names));
}
