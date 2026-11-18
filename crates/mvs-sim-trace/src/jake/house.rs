//! JakeHouse_Actor as the record writes it. The actor stands at its place (the record's x and
//! y); `comps` and `p` are the class's static data; `dealer` is the fart hit box's own DealerActor, an object whole.

use mvs_sim::chars::jake::house::{JakeHouse, Reflected};
use mvs_trace::{ActorRecord, Obj, Value};

use crate::actors::{opt_load, static_object};
use crate::conv::{Plain, plain_keys, plain_struct};
use crate::cx::{Lx, Wx};

plain_struct!(Reflected { "pr" = pr, "t" = t });

macro_rules! house_keys {
    ($mac:ident $($pre:tt)*) => {
        $mac!($($pre)*
            "anim" = anim, "armor" = armor, "conds" = conds, "facing" = facing, "fart" = fart,
            "fartGameTime" = fart_game_time, "fartOffAt" = fart_off_at, "hidden" = hidden, "hideAt" = hide_at,
            "hitSet" = hit_set, "houseAt" = house_at, "inside" = inside, "insideP" = inside_p,
            "jumpGate" = jump_gate, "jumpStopAt" = jump_stop_at, "meshIndex" = mesh_index, "mirrored" = mirrored,
            "reflected" = reflected, "voAt" = vo_at,
        )
    };
}

/// The dealer's own keys (its `hit` and place are the house's; the owner is the actor's).
const DEALER_KEYS: [&str; 12] = ["attackId", "cls", "dead", "facing", "hit", "id", "owner", "p", "vx", "vy", "x", "y"];

/// A JakeHouse_Actor_C as the record writes it.
pub fn house_put(s: &JakeHouse, x: &Wx, r: &mut ActorRecord, o: &mut Vec<(String, Value)>) {
    r.x = Some(f64::from(s.x));
    r.y = Some(f64::from(s.y));
    house_keys!(plain_keys put s, x, o;);
    o.push(("comps".into(), static_object()));
    o.push(("p".into(), static_object()));
    let owner = Obj::new(vec![("ref".into(), Value::Str("fighter".into())), ("id".into(), Value::Num(r.owner))]);
    let d = Obj::new(vec![
        ("attackId".into(), Value::Num(f64::from(s.attack_id))),
        ("cls".into(), Value::Str(r.class.clone())),
        ("dead".into(), Value::Bool(false)),
        ("facing".into(), Value::Num(1.0)),
        ("hit".into(), s.dealer_hit.to_value(x)),
        ("id".into(), Value::Num(-1.0)),
        ("owner".into(), Value::Obj(owner)),
        ("p".into(), static_object()),
        ("vx".into(), Value::Num(0.0)),
        ("vy".into(), Value::Num(0.0)),
        ("x".into(), Value::Num(f64::from(s.dealer_x))),
        ("y".into(), Value::Num(f64::from(s.dealer_y))),
    ]);
    o.push(("dealer".into(), Value::Obj(d)));
}

/// [`house_put`]'s inverse; the keys it knows go into `known`.
pub fn house_load(s: &mut JakeHouse, r: &ActorRecord, ob: &Obj, l: &mut Lx, known: &mut Vec<&'static str>) {
    opt_load(&mut s.x, r.x, "x", l);
    opt_load(&mut s.y, r.y, "y", l);
    l.at("rest", |l| {
        house_keys!(plain_keys load s, ob, l;);
        l.at("dealer", |l| match ob.get("dealer") {
            Some(Value::Obj(d)) => {
                if let Some(v) = d.get("hit") {
                    l.at("hit", |l| s.dealer_hit.load_value(v, l));
                }
                if let Some(v) = d.get("x") {
                    l.at("x", |l| s.dealer_x.load_value(v, l));
                }
                if let Some(v) = d.get("y") {
                    l.at("y", |l| s.dealer_y.load_value(v, l));
                }
                if let Some(v) = d.get("attackId") {
                    l.at("attackId", |l| s.attack_id.load_value(v, l));
                }
                crate::conv::check_keys(d, &DEALER_KEYS, l);
            }
            _ => l.miss("key dealer absent"),
        });
    });
    known.extend(house_keys!(plain_keys names));
    known.extend(["comps", "dealer"]);
}
