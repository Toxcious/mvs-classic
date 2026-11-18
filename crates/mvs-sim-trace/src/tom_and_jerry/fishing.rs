//! Tom and Jerry's fishing lure as the record writes it: the keys the class adds to a
//! projectile (`returning` is the base's).

use mvs_sim::chars::tom_and_jerry::fishing::Lure;
use mvs_trace::{Obj, Value};

use crate::conv::plain_keys;
use crate::cx::{Lx, Wx};

macro_rules! lure_keys {
    ($mac:ident $($pre:tt)*) => {
        $mac!($($pre)*
            "cableX" = cable_x, "cableY" = cable_y, "jerryMesh" = jerry_mesh, "lureMesh" = lure_mesh,
            "noRope" = no_rope, "ribbon" = ribbon, "jerryLeft" = jerry_left, "conds" = conds, "bait" = bait,
            "hasBounced" = has_bounced, "refreshJerry" = refresh_jerry, "jerryAttached" = jerry_attached,
            "showImpact" = show_impact, "lureHp" = lure_hp, "activateClosed" = activate_closed,
            "reelClosed" = reel_closed, "grabBound" = grab_bound, "pullingJerry" = pulling_jerry,
            "gravAt" = grav_at, "arenaAt" = arena_at, "returnAt" = return_at, "destroyAt" = destroy_at,
            "move" = mv, "kbAt" = kb_at, "stunWas" = stun_was, "stunAge" = stun_age, "montageAt" = montage_at,
        )
    };
}

/// A FishingLure_C's own keys.
pub fn lure_put(s: &Lure, x: &Wx, o: &mut Vec<(String, Value)>) {
    lure_keys!(plain_keys put s, x, o;);
}

/// [`lure_put`]'s inverse; the keys it knows go into `known`.
pub fn lure_load(s: &mut Lure, ob: &Obj, l: &mut Lx, known: &mut Vec<&'static str>) {
    lure_keys!(plain_keys load s, ob, l;);
    known.extend(lure_keys!(plain_keys names));
}
