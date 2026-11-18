//! `WreckingBall` (Creature_WreckingBall_C, a projectile's `ext`): the keys a
//! wrecking ball adds to its projectile's `rest`.

use mvs_sim::chars::creature::perks_ball::WreckingBall;
use mvs_trace::{Obj, Value};

use crate::conv::plain_keys;
use crate::cx::{Lx, Wx};

/// The keys a wrecking ball adds to its projectile's `rest`.
pub fn put(p: &WreckingBall, x: &Wx, o: &mut Vec<(String, Value)>) {
    plain_keys!(put p, x, o; "ally" = ally,);
}

/// [`put`]'s inverse.
pub fn load(p: &mut WreckingBall, ob: &Obj, l: &mut Lx, known: &mut Vec<&'static str>) {
    plain_keys!(load p, ob, l; "ally" = ally,);
    known.extend(plain_keys!(names "ally" = ally,));
}
