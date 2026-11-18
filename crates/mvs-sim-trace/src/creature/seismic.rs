//! `SeismicReactor` (SeismicReactor_C, a projectile's `ext`): the keys a reactor adds to
//! its projectile's `rest`.

use mvs_sim::chars::creature::seismic::SeismicReactor;
use mvs_trace::{Obj, Value};

use crate::conv::plain_keys;
use crate::cx::{Lx, Wx};

/// The keys a reactor adds to its projectile's `rest` (`sway` only while set).
pub fn put(p: &SeismicReactor, x: &Wx, o: &mut Vec<(String, Value)>) {
    plain_keys!(put p, x, o;
        "spin" = spin, "rate" = rate, "hp" = hp, "chargeAt" = charge_at, "blastFrom" = blast_from,
        "blastNext" = blast_next, "endShown" = end_shown, "sway" ~ sway,);
}

/// [`put`]'s inverse.
pub fn load(p: &mut SeismicReactor, ob: &Obj, l: &mut Lx, known: &mut Vec<&'static str>) {
    plain_keys!(load p, ob, l;
        "spin" = spin, "rate" = rate, "hp" = hp, "chargeAt" = charge_at, "blastFrom" = blast_from,
        "blastNext" = blast_next, "endShown" = end_shown, "sway" ~ sway,);
    known.extend(plain_keys!(names
        "spin" = spin, "rate" = rate, "hp" = hp, "chargeAt" = charge_at, "blastFrom" = blast_from,
        "blastNext" = blast_next, "endShown" = end_shown, "sway" ~ sway,));
}
