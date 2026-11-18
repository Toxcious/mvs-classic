//! Wonder Woman's actors as the record writes them: the bracer clash and the lasso (a
//! projectile's `ext`), the bracer shield (a world actor) and Buff_NullifyNextHit's extension.

use mvs_sim::chars::wonder_woman::actors::{BracerClash, BracerShield, Lasso, NullifyNextHit};
use mvs_sim::chars::wonder_woman::perks_lasso::LassoPerkState;
use mvs_trace::{ActorRecord, Obj, Value};

use crate::actors::{opt_load, static_object};
use crate::conv::{Plain, as_obj, check_keys, obj, plain_keys, plain_struct};
use crate::cx::{Lx, Wx};

plain_struct!(LassoPerkState {
    "bounceAt" = bounce_at, "buffer" = buffer, "bufferEnd" = buffer_end, "grapple" = grapple,
    "grappled" = grappled, "tipped" = tipped, "tipper" = tipper,
});

macro_rules! clash_keys {
    ($mac:ident $($pre:tt)*) => {
        $mac!($($pre)* "growth" = growth, "growthValue" = growth_value, "offAge" = off_age, "onAge" = on_age, "unit" = unit,)
    };
}

macro_rules! lasso_keys {
    ($mac:ident $($pre:tt)*) => {
        $mac!($($pre)*
            "allyLassoed" = ally_lassoed, "enemyLassoed" = enemy_lassoed, "inside" = inside, "perk" ~ perk,
            "radius" = radius, "sphereOn" = sphere_on, "voGate" = vo_gate, "wristX" = wrist_x, "wristY" = wrist_y,
        )
    };
}

/// The keys the bracer clash adds to its projectile's `rest`.
pub fn clash_put(p: &BracerClash, x: &Wx, o: &mut Vec<(String, Value)>) {
    clash_keys!(plain_keys put p, x, o;);
}

/// [`clash_put`]'s inverse.
pub fn clash_load(p: &mut BracerClash, ob: &Obj, l: &mut Lx, known: &mut Vec<&'static str>) {
    clash_keys!(plain_keys load p, ob, l;);
    known.extend(clash_keys!(plain_keys names));
}

/// The keys the lasso adds to its projectile's `rest`.
pub fn lasso_put(p: &Lasso, x: &Wx, o: &mut Vec<(String, Value)>) {
    lasso_keys!(plain_keys put p, x, o;);
}

/// [`lasso_put`]'s inverse.
pub fn lasso_load(p: &mut Lasso, ob: &Obj, l: &mut Lx, known: &mut Vec<&'static str>) {
    lasso_keys!(plain_keys load p, ob, l;);
    known.extend(lasso_keys!(plain_keys names));
}

macro_rules! bracer_keys {
    ($mac:ident $($pre:tt)*) => {
        $mac!($($pre)* "age" = age, "blastBounds" = blast_bounds, "radius" = radius, "up" = up,)
    };
}

/// The bracer shield's own keys and place.
pub fn bracer_put(s: &BracerShield, x: &Wx, r: &mut ActorRecord, o: &mut Vec<(String, Value)>) {
    r.x = Some(f64::from(s.x));
    r.y = Some(f64::from(s.y));
    bracer_keys!(plain_keys put s, x, o;);
    o.push(("p".into(), static_object()));
}

/// [`bracer_put`]'s inverse.
pub fn bracer_load(s: &mut BracerShield, r: &ActorRecord, ob: &Obj, l: &mut Lx, known: &mut Vec<&'static str>) {
    opt_load(&mut s.x, r.x, "x", l);
    opt_load(&mut s.y, r.y, "y", l);
    l.at("rest", |l| {
        bracer_keys!(plain_keys load s, ob, l;);
    });
    known.extend(bracer_keys!(plain_keys names));
}

/// Buff_NullifyNextHit's extension as the oracle writes it (`null` before it has one; `broke` only once set).
pub fn nullify_put(e: &NullifyNextHit, x: &Wx) -> Value {
    let Some(by) = e.by else { return Value::Null };
    let mut o = vec![("by", by.to_value(x)), ("proc", e.proc.to_value(x))];
    if e.broke {
        o.push(("broke", Value::Bool(true)));
    }
    obj(o)
}

/// [`nullify_put`]'s inverse.
pub fn nullify_load(e: &mut NullifyNextHit, v: &Value, l: &mut Lx) {
    if matches!(v, Value::Null) {
        *e = NullifyNextHit::default();
        return;
    }
    let Some(ob) = as_obj(v, l) else { return };
    match ob.get("by") {
        Some(b) => e.by = l.at("by", |l| Plain::from_value(b, l)),
        None => l.miss(format_args!("key by absent")),
    }
    match ob.get("proc") {
        Some(p) => l.at("proc", |l| e.proc.load_value(p, l)),
        None => l.miss(format_args!("key proc absent")),
    }
    e.broke = matches!(ob.get("broke"), Some(Value::Bool(true)));
    check_keys(ob, &["by", "proc", "broke"], l);
}
