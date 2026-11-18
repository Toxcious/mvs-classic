//! The marker as the record writes it: GarnetMarker2_C.

use mvs_sim::chars::garnet::marker::GarnetMarker2;
use mvs_trace::{Obj, Value};

use crate::conv::plain_keys;
use crate::cx::{Lx, Wx};

macro_rules! marker_keys {
    ($mac:ident $($pre:tt)*) => {
        $mac!($($pre)*
            "activatedX" = activated_x, "activatedY" = activated_y, "attachParent" = attach_parent,
            "attachedFighter" = attached_fighter, "beamX" = beam_x, "beamY" = beam_y, "collide" = collide,
            "conds" = conds, "detachOnce" = detach_once, "instigating" = instigating, "isActivated" = is_activated,
            "lifeAt" = life_at, "over" = over, "queryAt" = query_at, "runOutOn" = run_out_on,
            "starHidden" = star_hidden, "uiRetryAt" = ui_retry_at,
        )
    };
}

/// a GarnetMarker2_C as the record writes it.
pub fn marker_put(s: &GarnetMarker2, x: &Wx, o: &mut Vec<(String, Value)>) {
    marker_keys!(plain_keys put s, x, o;);
}

/// [`marker_put`]'s inverse; the keys it knows go into `known`.
pub fn marker_load(s: &mut GarnetMarker2, ob: &Obj, l: &mut Lx, known: &mut Vec<&'static str>) {
    marker_keys!(plain_keys load s, ob, l;);
    known.extend(marker_keys!(plain_keys names));
}
