//! Finn's BMO as the record writes it.

use mvs_sim::chars::finn::bmo::BmoActor;
use mvs_trace::{ActorRecord, Obj, Value};

use crate::actors::{opt_load, static_object};
use crate::conv::plain_keys;
use crate::cx::{Lx, Wx};

macro_rules! bmo_keys {
    ($mac:ident $($pre:tt)*) => {
        $mac!($($pre)*
            "age" = age, "blastAt" = blast_at, "blastBounds" = blast_bounds, "boundTo" = bound_to,
            "checkLeft" = check_left, "crouchHeight" = crouch_height, "facingLeft" = facing_left,
            "followX" = follow_x, "followZ" = follow_z, "isCrouching" = is_crouching, "meshVisible" = mesh_visible,
            "onCamera" = on_camera, "relX" = rel_x, "relZ" = rel_z, "visible" = visible, "voLeft" = vo_left,
            "wallSliding" = wall_sliding,
        )
    };
}

/// A BMO_FallowActor_C as the record writes it (its place is the record's x and y).
pub fn bmo_put(s: &BmoActor, x: &Wx, r: &mut ActorRecord, o: &mut Vec<(String, Value)>) {
    r.x = Some(f64::from(s.x));
    r.y = Some(f64::from(s.y));
    bmo_keys!(plain_keys put s, x, o;);
    o.push(("p".into(), static_object()));
}

/// [`bmo_put`]'s inverse; the keys it knows go into `known`.
pub fn bmo_load(s: &mut BmoActor, r: &ActorRecord, ob: &Obj, l: &mut Lx, known: &mut Vec<&'static str>) {
    opt_load(&mut s.x, r.x, "x", l);
    opt_load(&mut s.y, r.y, "y", l);
    l.at("rest", |l| {
        bmo_keys!(plain_keys load s, ob, l;);
    });
    known.extend(bmo_keys!(plain_keys names));
}
