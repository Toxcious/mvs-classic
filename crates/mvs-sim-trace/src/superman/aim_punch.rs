//! `AimMarker` (SupermanAimPunch_C).

use mvs_sim::chars::superman::aim_punch::AimMarker;
use mvs_trace::{ActorRecord, Obj, Value};

use crate::conv::plain_keys;
use crate::cx::{Lx, Wx};

/// `{static: "object"}`: the class's parameters (`p`).
fn static_object() -> Value {
    Value::Obj(Obj::new(vec![("static".to_owned(), Value::Str("object".to_owned()))]))
}

/// The class's own keys: the place into the record, the rest into `o`. `sniper` is written only when true (the oracle
/// leaves the key out otherwise).
pub fn put(s: &AimMarker, x: &Wx, r: &mut ActorRecord, o: &mut Vec<(String, Value)>) {
    r.x = Some(f64::from(s.x));
    r.y = Some(f64::from(s.y));
    plain_keys!(put s, x, o;
        "age" = age, "blastAt" = blast_at, "blastBounds" = blast_bounds, "bounds" = bounds, "checkIn" = check_in, "hidden" = hidden, "loop" = looping, "pivotX" = pivot_x,
        "pivotY" = pivot_y, "prevX" = prev_x, "prevY" = prev_y, "serverX" = server_x, "serverY" = server_y,
    );
    if s.sniper {
        o.push(("sniper".into(), Value::Bool(true)));
    }
    o.push(("p".into(), static_object()));
}

/// [`put`]'s inverse.
pub fn load(s: &mut AimMarker, r: &ActorRecord, ob: &Obj, l: &mut Lx, known: &mut Vec<&'static str>) {
    for (v, got, k) in [(&mut s.x, r.x, "x"), (&mut s.y, r.y, "y")] {
        match got {
            Some(g) => *v = mvs_sim::math::narrow(g),
            None => l.at(k, |l| l.miss("undefined (the Rust actor has it)")),
        }
    }
    l.at("rest", |l| {
        plain_keys!(load s, ob, l;
            "age" = age, "blastAt" = blast_at, "blastBounds" = blast_bounds, "bounds" = bounds, "checkIn" = check_in, "hidden" = hidden, "loop" = looping,
            "pivotX" = pivot_x, "pivotY" = pivot_y, "prevX" = prev_x, "prevY" = prev_y, "serverX" = server_x,
            "serverY" = server_y,
        );
    });
    s.sniper = matches!(ob.get("sniper"), Some(Value::Bool(true)));
    known.extend(plain_keys!(names
        "age" = age, "blastAt" = blast_at, "blastBounds" = blast_bounds, "bounds" = bounds, "checkIn" = check_in, "hidden" = hidden, "loop" = looping, "pivotX" = pivot_x,
        "pivotY" = pivot_y, "prevX" = prev_x, "prevY" = prev_y, "serverX" = server_x, "serverY" = server_y,
    ));
    known.push("sniper");
}
