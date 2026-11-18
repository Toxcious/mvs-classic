//! `TetherProjectile` (TetherProjectile_C, a projectile's `ext`): the keys a tether adds
//! to its projectile's `rest`. `bigBrother` is declared without a value in the oracle: absent until the pull sets it.

use mvs_sim::chars::creature::tether::TetherProjectile;
use mvs_trace::{Obj, Value};

use crate::conv::plain_keys;
use crate::cx::{Lx, Wx};

/// The keys a tether adds to its projectile's `rest`.
pub fn put(p: &TetherProjectile, x: &Wx, o: &mut Vec<(String, Value)>) {
    plain_keys!(put p, x, o;
        "homeTo" = home_to, "homeMag" = home_mag, "pinned" = pinned, "ally" = ally, "pulled" = pulled,
        "breakWarning" = break_warning, "stopOnce" = stop_once, "at0225" = at0225, "at3" = at3, "at5" = at5,
        "at1" = at1, "retryAt" = retry_at, "retryWho" = retry_who, "breakAt" = break_at, "endAt" = end_at,
        "endDue" = end_due, "ribbonAt" = ribbon_at, "over" = over, "vfxOn" = vfx_on, "vfxAt" = vfx_at,
        "bigBrother" ~ big_brother,);
}

/// [`put`]'s inverse.
pub fn load(p: &mut TetherProjectile, ob: &Obj, l: &mut Lx, known: &mut Vec<&'static str>) {
    plain_keys!(load p, ob, l;
        "homeTo" = home_to, "homeMag" = home_mag, "pinned" = pinned, "ally" = ally, "pulled" = pulled,
        "breakWarning" = break_warning, "stopOnce" = stop_once, "at0225" = at0225, "at3" = at3, "at5" = at5,
        "at1" = at1, "retryAt" = retry_at, "retryWho" = retry_who, "breakAt" = break_at, "endAt" = end_at,
        "endDue" = end_due, "ribbonAt" = ribbon_at, "over" = over, "vfxOn" = vfx_on, "vfxAt" = vfx_at,
        "bigBrother" ~ big_brother,);
    known.extend(plain_keys!(names
        "homeTo" = home_to, "homeMag" = home_mag, "pinned" = pinned, "ally" = ally, "pulled" = pulled,
        "breakWarning" = break_warning, "stopOnce" = stop_once, "at0225" = at0225, "at3" = at3, "at5" = at5,
        "at1" = at1, "retryAt" = retry_at, "retryWho" = retry_who, "breakAt" = break_at, "endAt" = end_at,
        "endDue" = end_due, "ribbonAt" = ribbon_at, "over" = over, "vfxOn" = vfx_on, "vfxAt" = vfx_at,
        "bigBrother" ~ big_brother,));
}
