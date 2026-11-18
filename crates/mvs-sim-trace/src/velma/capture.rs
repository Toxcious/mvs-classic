//! Velma's capture as the record writes it: Buff_VelmaAttachToAlly_C's extension, the
//! parent's `AttachState` (`by`, `bound`, `rel`) then its own keys.

use mvs_sim::chars::velma::capture::CaptureExt;
use mvs_sim::combat::attach_ally::AttachRel;
use mvs_sim::fighter::FighterId;
use mvs_trace::{Obj, Value};

use crate::conv::{Plain, as_obj, check_keys, obj, plain_keys};
use crate::cx::{Lx, Wx};

const KEYS: [&str; 11] =
    ["bound", "by", "checkIn", "faceIn", "faceLeft", "held", "ins", "insFacing", "launchBound", "launchIn", "rel"];

fn rel_put(r: Option<AttachRel>, x: &Wx) -> Option<Value> {
    r.map(|r| obj(vec![("dx", r.dx.to_value(x)), ("dy", r.dy.to_value(x))]))
}

/// Buff_VelmaAttachToAlly_C's extension as the oracle writes it.
pub fn capture_put(e: &CaptureExt, x: &Wx) -> Value {
    let mut o: Vec<(String, Value)> =
        vec![("by".into(), e.attach.by.to_value(x)), ("bound".into(), e.attach.bound.to_value(x))];
    if let Some(r) = rel_put(e.attach.rel, x) {
        o.push(("rel".into(), r));
    }
    plain_keys!(put e, x, o; "ins" = ins, "held" = held, "launchIn" = launch_in, "checkIn" = check_in,
        "faceIn" = face_in, "launchBound" = launch_bound, "faceLeft" = face_left, "insFacing" = ins_facing,);
    Value::Obj(Obj::new(o))
}

/// [`capture_put`]'s inverse.
pub fn capture_load(e: &mut CaptureExt, v: &Value, l: &mut Lx) {
    let Some(o) = as_obj(v, l) else { return };
    e.attach.by = match o.get("by") {
        Some(v) => l.at("by", |l| Option::<FighterId>::from_value(v, l).flatten()),
        None => None,
    };
    plain_keys!(load e, o, l; "bound" = attach.bound, "ins" = ins, "held" = held, "launchIn" = launch_in,
        "checkIn" = check_in, "faceIn" = face_in, "launchBound" = launch_bound, "faceLeft" = face_left,
        "insFacing" = ins_facing,);
    e.attach.rel = match o.get("rel") {
        Some(Value::Obj(r)) => {
            let n = |k: &str| match r.get(k) {
                Some(Value::Num(n)) => *n,
                _ => 0.0,
            };
            Some(AttachRel { dx: mvs_sim::math::narrow(n("dx")), dy: mvs_sim::math::narrow(n("dy")) })
        }
        _ => None,
    };
    check_keys(o, &KEYS, l);
}
