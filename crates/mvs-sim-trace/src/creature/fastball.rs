//! `Fastball` (Fastball_C, an item's `ext`): the keys a fastball adds to its item's
//! `rest`.

use mvs_sim::chars::creature::fastball::{Fastball, Hold};
use mvs_trace::{Obj, Value};

use crate::conv::{plain_keys, plain_struct};
use crate::cx::{Lx, Wx};

plain_struct!(Hold { "id" = id, "had" = had });

/// The keys a fastball adds to its item's `rest`.
pub fn put(s: &Fastball, x: &Wx, o: &mut Vec<(String, Value)>) {
    plain_keys!(put s, x, o;
        "picked" = picked, "everHeld" = ever_held, "pickDue" = pick_due, "dropDue" = drop_due,
        "triggered" = triggered, "endTail" = end_tail, "kbSeq" = kb_seq, "stunWas" = stun_was,
        "stunAge" = stun_age, "holds" = holds, "holdLast" = hold_last,);
}

/// [`put`]'s inverse.
pub fn load(s: &mut Fastball, ob: &Obj, l: &mut Lx, known: &mut Vec<&'static str>) {
    plain_keys!(load s, ob, l;
        "picked" = picked, "everHeld" = ever_held, "pickDue" = pick_due, "dropDue" = drop_due,
        "triggered" = triggered, "endTail" = end_tail, "kbSeq" = kb_seq, "stunWas" = stun_was,
        "stunAge" = stun_age, "holds" = holds, "holdLast" = hold_last,);
    known.extend(plain_keys!(names
        "picked" = picked, "everHeld" = ever_held, "pickDue" = pick_due, "dropDue" = drop_due,
        "triggered" = triggered, "endTail" = end_tail, "kbSeq" = kb_seq, "stunWas" = stun_was,
        "stunAge" = stun_age, "holds" = holds, "holdLast" = hold_last,));
}
