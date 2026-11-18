//! Tom and Jerry's dynamite as the record writes it: the keys the class adds to a
//! projectile.

use mvs_sim::chars::tom_and_jerry::dynamite::Dynamite;
use mvs_trace::{Obj, Value};

use crate::conv::plain_keys;
use crate::cx::{Lx, Wx};

macro_rules! dynamite_keys {
    ($mac:ident $($pre:tt)*) => {
        $mac!($($pre)*
            "alwaysShow" = always_show, "blasted" = blasted, "blocking" = blocking, "bounceGate" = bounce_gate,
            "bounces" = bounces, "endDone" = end_done, "gateAt" = gate_at, "gravAt" = grav_at,
            "hasSplit" = has_split, "hidden" = hidden, "lifeOnce" = life_once, "reflected" = reflected,
            "showFx" = show_fx, "stopDetonateOnDeath" = stop_detonate_on_death,
        )
    };
}

/// A Dynamite_SpawnedActor_C's own keys.
pub fn dynamite_put(s: &Dynamite, x: &Wx, o: &mut Vec<(String, Value)>) {
    dynamite_keys!(plain_keys put s, x, o;);
}

/// [`dynamite_put`]'s inverse; the keys it knows go into `known`.
pub fn dynamite_load(s: &mut Dynamite, ob: &Obj, l: &mut Lx, known: &mut Vec<&'static str>) {
    dynamite_keys!(plain_keys load s, ob, l;);
    known.extend(dynamite_keys!(plain_keys names));
}
