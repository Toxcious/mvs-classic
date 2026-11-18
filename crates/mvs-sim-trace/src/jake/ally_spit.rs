//! JakeEatAlly_Aim and JakeSpitAllyHitBox as the record writes them. `p` and `comps` are
//! the class's static data. The hit box's `vx`, `vy` are readonly 0, its `facing` readonly 1, and its `sphere` the
//! shared shape list `hitBoxShapes` writes (its values at the last test: nothing before the box is on); the packet's
//! IgnoreActor is the ally's reference in the oracle, a flag in the Rust packet.

use mvs_sim::chars::jake::ally_spit::{JakeEatAllyAim, JakeSpitAllyHitBox};
use mvs_trace::{ActorRecord, Obj, Value};

use crate::actors::{opt_load, static_object};
use crate::conv::{Plain, plain_keys};
use crate::cx::{Lx, Wx};

macro_rules! aim_keys {
    ($mac:ident $($pre:tt)*) => {
        $mac!($($pre)*
            "aimY" = aim_y, "aimZ" = aim_z, "arc" = arc, "ally" = ally, "bindAt" = bind_at, "bound" = bound,
            "buffCheck" = buff_check, "count" = count, "hit" = hit, "jake" = jake, "lastIndex" = last_index,
            "moving" = moving, "prev" = prev, "shown" = shown, "shownAt" = shown_at, "spline" = spline,
        )
    };
}

macro_rules! hit_box_keys {
    ($mac:ident $($pre:tt)*) => {
        $mac!($($pre)*
            "attackId" = attack_id, "bindAt" = bind_at, "bound" = bound, "dieAt" = die_at, "dirY" = dir_y, "dirZ" = dir_z, "hitSet" = hit_set,
            "on" = on, "radius" = radius, "stunSeq" = stun_seq, "stunWas" = stun_was,
        )
    };
}

/// A JakeEatAlly_Aim_C as the record writes it.
pub fn aim_put(s: &JakeEatAllyAim, x: &Wx, r: &mut ActorRecord, o: &mut Vec<(String, Value)>) {
    r.x = Some(f64::from(s.x));
    r.y = Some(f64::from(s.y));
    aim_keys!(plain_keys put s, x, o;);
    o.push(("facingLeft".into(), Value::Bool(false)));
    o.push(("comps".into(), static_object()));
    o.push(("p".into(), static_object()));
}

/// [`aim_put`]'s inverse; the keys it knows go into `known`.
pub fn aim_load(s: &mut JakeEatAllyAim, r: &ActorRecord, ob: &Obj, l: &mut Lx, known: &mut Vec<&'static str>) {
    opt_load(&mut s.x, r.x, "x", l);
    opt_load(&mut s.y, r.y, "y", l);
    l.at("rest", |l| {
        aim_keys!(plain_keys load s, ob, l;);
    });
    known.extend(aim_keys!(plain_keys names));
    known.extend(["facingLeft", "comps"]);
}

/// `hit` with IgnoreActor as the oracle holds it (the owner fighter's reference where the packet's flag is set).
fn hit_with_ref(s: &JakeSpitAllyHitBox, owner: f64, x: &Wx) -> Value {
    let Value::Obj(o) = s.hit.to_value(x) else { unreachable!("a packet is written as an object") };
    let mut out = Obj::new(Vec::new());
    for (k, v) in o.entries() {
        if k == "IgnoreActor" && matches!(v, Value::Bool(true)) {
            let me = Obj::new(vec![("ref".into(), Value::Str("fighter".into())), ("id".into(), Value::Num(owner))]);
            out.insert(k.clone(), Value::Obj(me));
        } else {
            out.insert(k.clone(), v.clone());
        }
    }
    Value::Obj(out)
}

/// A JakeSpitAllyHitBox_C as the record writes it.
pub fn hit_box_put(s: &JakeSpitAllyHitBox, x: &Wx, r: &mut ActorRecord, o: &mut Vec<(String, Value)>) {
    r.x = Some(f64::from(s.x));
    r.y = Some(f64::from(s.y));
    r.vx = Some(0.0);
    r.vy = Some(0.0);
    hit_box_keys!(plain_keys put s, x, o;);
    o.push(("hit".into(), hit_with_ref(s, r.owner, x)));
    o.push(("facing".into(), Value::Num(1.0)));
    // (`sphere`: written by each hit test while the box is on)
    let (sx, sy, sr) = if s.on { (s.x, s.y, s.radius) } else { (0.0, 0.0, 0.0) };
    let sp = Obj::new(vec![
        ("x".into(), Value::Num(f64::from(sx))),
        ("y".into(), Value::Num(f64::from(sy))),
        ("r".into(), Value::Num(f64::from(sr))),
        ("z".into(), Value::Num(0.0)),
    ]);
    o.push(("sphere".into(), Value::List(vec![Value::Obj(sp)])));
    o.push(("p".into(), static_object()));
}

/// [`hit_box_put`]'s inverse; the keys it knows go into `known`.
pub fn hit_box_load(s: &mut JakeSpitAllyHitBox, r: &ActorRecord, ob: &Obj, l: &mut Lx, known: &mut Vec<&'static str>) {
    opt_load(&mut s.x, r.x, "x", l);
    opt_load(&mut s.y, r.y, "y", l);
    l.at("rest", |l| {
        hit_box_keys!(plain_keys load s, ob, l;);
        if let Some(Value::Obj(h)) = ob.get("hit") {
            let mut out = Obj::new(Vec::new());
            for (k, v) in h.entries() {
                if k == "IgnoreActor" && matches!(v, Value::Obj(_)) {
                    out.insert(k.clone(), Value::Bool(true));
                } else {
                    out.insert(k.clone(), v.clone());
                }
            }
            l.at("hit", |l| s.hit.load_value(&Value::Obj(out), l));
        }
    });
    known.extend(hit_box_keys!(plain_keys names));
    known.extend(["hit", "facing", "sphere"]);
}
