//! Buff_C017_Ally_Pilot's extension as the record writes it: `null` before
//! its begin, else `{stage, wait, tries, snap, lim, ended}` with `lim` `{left, right, bottom, top}` or `null`.

use mvs_sim::chars::iron_giant::turret::PilotHold;
use mvs_sim::world::blast::BlastBounds;
use mvs_trace::{Obj, Value};

use crate::conv::{Plain, as_obj, check_keys, plain_keys};
use crate::cx::{Lx, Wx};

macro_rules! hold_keys {
    ($mac:ident $($pre:tt)*) => {
        $mac!($($pre)*
            "ended" = ended, "snap" = snap, "stage" = stage, "tries" = tries, "wait" = wait,
        )
    };
}

fn lim_put(l: Option<&BlastBounds>) -> Value {
    let Some(l) = l else { return Value::Null };
    Value::Obj(Obj::new(vec![
        ("bottom".to_owned(), Value::Num(f64::from(l.bottom))),
        ("left".to_owned(), Value::Num(f64::from(l.left))),
        ("right".to_owned(), Value::Num(f64::from(l.right))),
        ("top".to_owned(), Value::Num(f64::from(l.top))),
    ]))
}

fn lim_load(v: &Value, l: &mut Lx) -> Option<BlastBounds> {
    if matches!(v, Value::Null) {
        return None;
    }
    let o = as_obj(v, l)?;
    let mut b = BlastBounds::default();
    for (k, f) in [("bottom", &mut b.bottom), ("left", &mut b.left), ("right", &mut b.right), ("top", &mut b.top)] {
        match o.get(k) {
            Some(v) => l.at(k, |l| f.load_value(v, l)),
            None => l.miss(format_args!("key {k} absent")),
        }
    }
    check_keys(o, &["bottom", "left", "right", "top"], l);
    Some(b)
}

/// Buff_C017_Ally_Pilot's extension as the oracle writes it (`null` before it has one).
pub fn pilot_put(e: &PilotHold, x: &Wx) -> Value {
    if !e.begun {
        return Value::Null;
    }
    let mut o: Vec<(String, Value)> = Vec::new();
    hold_keys!(plain_keys put e, x, o;);
    o.push(("lim".into(), lim_put(e.lim.as_ref())));
    Value::Obj(Obj::new(o))
}

/// [`pilot_put`]'s inverse.
pub fn pilot_load(e: &mut PilotHold, v: &Value, l: &mut Lx) {
    match v {
        Value::Null => *e = PilotHold::default(),
        Value::Obj(ob) => {
            e.begun = true;
            hold_keys!(plain_keys load e, ob, l;);
            e.lim = match ob.get("lim") {
                Some(v) => l.at("lim", |l| lim_load(v, l)),
                None => {
                    l.miss(format_args!("key lim absent"));
                    None
                }
            };
            let mut known: Vec<&str> = hold_keys!(plain_keys names).to_vec();
            known.push("lim");
            check_keys(ob, &known, l);
        }
        _ => l.miss(format_args!("Buff_C017_Ally_Pilot's extension is neither null nor an object")),
    }
}
