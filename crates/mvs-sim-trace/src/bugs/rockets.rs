//! Bugs Bunny's rockets as the record writes them: the keys the class adds to a projectile.

use mvs_sim::chars::bugs::rockets::Rocket;
use mvs_trace::{Obj, Value};

use crate::conv::plain_keys;
use crate::cx::{Lx, Wx};

macro_rules! rocket_keys {
    ($mac:ident $($pre:tt)*) => {
        $mac!($($pre)*
            "speed" = speed, "exploded" = exploded, "blastFrom" = blast_from, "blastNext" = blast_next,
        )
    };
}

/// A Bugs Bunny rocket's own keys; `ignored` is a list of the fighters' oracle ids.
pub fn rocket_put(s: &Rocket, x: &Wx, o: &mut Vec<(String, Value)>) {
    rocket_keys!(plain_keys put s, x, o;);
    o.push(("ignored".to_owned(), Value::List(s.ignored.iter().map(|&f| Value::Num(f64::from(x.fid(f)))).collect())));
}

/// [`rocket_put`]'s inverse; the keys it knows go into `known`.
pub fn rocket_load(s: &mut Rocket, ob: &Obj, l: &mut Lx, known: &mut Vec<&'static str>) {
    rocket_keys!(plain_keys load s, ob, l;);
    s.ignored.clear();
    match ob.get("ignored") {
        Some(Value::List(a)) => {
            for v in a {
                match v {
                    Value::Num(n) => {
                        if let Some(f) = l.fighter(*n) {
                            s.ignored.push(f);
                        }
                    }
                    _ => l.miss("an ignored fighter that is not a number"),
                }
            }
        }
        _ => l.miss("key ignored absent"),
    }
    known.extend(rocket_keys!(plain_keys names));
    known.push("ignored");
}
