//! Buff_GarnetStopProjectile's extension on a spawned actor as the record writes it
//!: `instigator` always; `adjusted`, `x`, `y`, `r`, `findScale` together once the
//! stop branch ran; `done`, `delayAt`, `CanOverride`, the carry's `hurt` / `life` once set; and a `true` key per
//! overlap the AdditionalHurtBox holds (`o<fighter>.h<hit>` / `o<fighter>.p<component>`).

use mvs_sim::chars::garnet::stop_projectile::StopExt;
use mvs_sim::combat::phases::SphereKey;
use mvs_trace::{Obj, Value};

use crate::conv::as_obj;
use crate::cx::{Lx, Wx};

fn key_name(k: SphereKey) -> String {
    match k {
        SphereKey::Hit(f, h) => format!("o{f}.h{h}"),
        SphereKey::Pawn(f, i) => format!("o{f}.p{i}"),
    }
}

fn parse_key(k: &str) -> Option<SphereKey> {
    let (f, rest) = k.strip_prefix('o')?.split_once('.')?;
    let f: i32 = f.parse().ok()?;
    if let Some(h) = rest.strip_prefix('h') {
        return Some(SphereKey::Hit(f, h.parse().ok()?));
    }
    Some(SphereKey::Pawn(f, rest.strip_prefix('p')?.parse().ok()?))
}

/// Buff_GarnetStopProjectile_C's extension as the oracle writes it (`ElemBuff.ext`, `actors::elem_ext_put`).
pub fn stop_put(e: &StopExt, _x: &Wx) -> Value {
    let n = Value::Num;
    let b = Value::Bool;
    let mut o: Vec<(String, Value)> = vec![("instigator".to_owned(), n(f64::from(e.instigator)))];
    let mut put = |k: &str, v: Value| o.push((k.to_owned(), v));
    if e.adjusted {
        put("adjusted", b(true));
        put("x", n(f64::from(e.x)));
        put("y", n(f64::from(e.y)));
        put("r", n(f64::from(e.r)));
        put("findScale", n(f64::from(e.find_scale)));
    }
    if e.done {
        put("done", b(true));
    }
    if let Some(d) = e.delay_at {
        put("delayAt", n(f64::from(d)));
    }
    if let Some(c) = e.can_override {
        put("CanOverride", b(c));
    }
    if let Some(h) = e.carry.hurt {
        put("hurt", n(f64::from(h)));
    }
    if let Some(l) = e.carry.life {
        put("life", n(f64::from(l)));
    }
    for k in e.keys.iter() {
        put(&key_name(k), b(true));
    }
    Value::Obj(Obj::new(o))
}

/// [`stop_put`]'s inverse (`actors::elem_ext_load`, by the buff's class).
pub fn stop_load(v: &Value, l: &mut Lx) -> StopExt {
    let mut e = StopExt::default();
    let Some(o) = as_obj(v, l) else { return e };
    for (k, v) in o.entries() {
        let (num, flag) = match v {
            Value::Num(x) => (Some(*x), None),
            Value::Bool(x) => (None, Some(*x)),
            _ => (None, None),
        };
        let ok = match (k.as_str(), num, flag) {
            ("instigator", Some(x), _) => {
                e.instigator = mvs_sim::math::to_int32(mvs_sim::math::narrow(x));
                true
            }
            ("adjusted", _, Some(x)) => {
                e.adjusted = x;
                true
            }
            ("x", Some(x), _) => {
                e.x = mvs_sim::math::narrow(x);
                true
            }
            ("y", Some(x), _) => {
                e.y = mvs_sim::math::narrow(x);
                true
            }
            ("r", Some(x), _) => {
                e.r = mvs_sim::math::narrow(x);
                true
            }
            ("findScale", Some(x), _) => {
                e.find_scale = mvs_sim::math::narrow(x);
                true
            }
            ("done", _, Some(x)) => {
                e.done = x;
                true
            }
            ("delayAt", Some(x), _) => {
                e.delay_at = Some(mvs_sim::math::narrow(x));
                true
            }
            ("CanOverride", _, Some(x)) => {
                e.can_override = Some(x);
                true
            }
            ("hurt", Some(x), _) => {
                e.carry.hurt = Some(mvs_sim::math::narrow(x));
                true
            }
            ("life", Some(x), _) => {
                e.carry.life = Some(mvs_sim::math::narrow(x));
                true
            }
            (k, _, Some(true)) => parse_key(k).map(|s| e.keys.insert(s)).is_some(),
            _ => false,
        };
        if !ok {
            l.miss(format_args!("key {k} = {v:?} has no Rust field"));
        }
    }
    e
}
