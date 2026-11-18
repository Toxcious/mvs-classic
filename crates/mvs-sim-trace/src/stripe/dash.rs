//! Stripe's dash listener, boombox and projectile block as the record writes them.

use mvs_sim::chars::stripe::dash::{BlockExt, BoomBox, StripeListener};
use mvs_trace::{ActorRecord, Obj, Value};

use crate::actors::static_object;
use crate::conv::plain_keys;
use crate::cx::{Lx, Wx};

/// A C023B_DashListener_C as the record writes it: the DoOnce and the class's parameters; it stands nowhere.
pub fn listener_put(s: &StripeListener, x: &Wx, r: &mut ActorRecord, o: &mut Vec<(String, Value)>) {
    let _ = r;
    plain_keys!(put s, x, o; "done" = done,);
    o.push(("p".into(), static_object()));
}

/// [`listener_put`]'s inverse; the keys it knows go into `known`.
pub fn listener_load(s: &mut StripeListener, r: &ActorRecord, ob: &Obj, l: &mut Lx, known: &mut Vec<&'static str>) {
    let _ = r;
    l.at("rest", |l| {
        plain_keys!(load s, ob, l; "done" = done,);
    });
    known.extend(plain_keys!(names "done" = done,));
}

macro_rules! box_keys {
    ($mac:ident $($pre:tt)*) => {
        $mac!($($pre)*
            "groundDue" = ground_due, "hasHit" = has_hit, "detonateDue" = detonate_due, "offDue" = off_due,
            "killDue" = kill_due, "rideFrom" = ride_from, "detonations" = detonations,
            "detonateAge" = detonate_age,
        )
    };
}

/// A BoomBox_SpawnedActor_C as the record writes it; its `floor` is null (the ride is not ported).
pub fn boombox_put(s: &BoomBox, x: &Wx, o: &mut Vec<(String, Value)>) {
    box_keys!(plain_keys put s, x, o;);
    o.push(("floor".into(), Value::Null));
}

/// [`boombox_put`]'s inverse; the keys it knows go into `known`.
pub fn boombox_load(s: &mut BoomBox, ob: &Obj, l: &mut Lx, known: &mut Vec<&'static str>) {
    box_keys!(plain_keys load s, ob, l;);
    known.extend(box_keys!(plain_keys names));
    known.push("floor");
}

/// C023B_DashProjectileBlock_C's extension as the oracle writes it: `{ closed }`.
pub fn block_put(e: &BlockExt, x: &Wx) -> Value {
    let mut o: Vec<(String, Value)> = Vec::new();
    plain_keys!(put e, x, o; "closed" = closed,);
    Value::Obj(Obj::new(o))
}

/// [`block_put`]'s inverse.
pub fn block_load(e: &mut BlockExt, v: &Value, l: &mut Lx) {
    let Some(ob) = crate::conv::as_obj(v, l) else { return };
    plain_keys!(load e, ob, l; "closed" = closed,);
}
