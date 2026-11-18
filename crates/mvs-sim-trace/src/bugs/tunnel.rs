//! Bugs Bunny's tunnels as the record writes them, and Buff_ProjectileThroughTunnel's
//! extension (his port's).

use mvs_sim::actors::tunnel_carry::ThroughTunnelExt;
use mvs_sim::chars::bugs::tunnel::{ActorStamp, FighterStamp, TeleportExt, Tunnel, TunnelingExt};
use mvs_sim::fighter::FighterId;
use mvs_trace::{ActorRecord, Obj, Value};

use crate::actors::{opt_load, static_object};
use crate::conv::{Plain, as_obj, plain_keys, plain_struct};
use crate::cx::{Lx, Wx};

// A Stamp (`{ a, t }`): the actor and the time it was added.
plain_struct!(FighterStamp { "a" = a, "t" = t } make |v, l| {
    let ob = as_obj(v, l)?;
    let a = FighterId::from_value(ob.get("a")?, l)?;
    let mut s = FighterStamp { a, t: 0.0 };
    s.load_value(v, l);
    Some(s)
});
plain_struct!(ActorStamp { "a" = a, "t" = t } make |v, l| {
    let ob = as_obj(v, l)?;
    let a = mvs_sim::actors::ActorRef::from_value(ob.get("a")?, l)?;
    let mut s = ActorStamp { a, t: 0.0 };
    s.load_value(v, l);
    Some(s)
});

macro_rules! tunnel_keys {
    ($mode:ident $($args:tt)*) => {
        plain_keys!($mode $($args)*
            "age" = age, "allies" = allies, "blastAt" = blast_at, "blastBounds" = blast_bounds,
            "duration" = duration, "durLeft" = dur_left, "facingLeft" = facing_left, "groundLeft" = ground_left,
            "inside" = inside, "projInside" = proj_inside, "start" = start, "stopLeft" = stop_left, "team" = team,
            "teleported" = teleported, "transitioningOut" = transitioning_out, "tunneled" = tunneled,
            "visualsOpen" = visuals_open,)
    };
}

/// A BugsBunnyTunnelParent_C as the record writes it.
pub fn tunnel_put(s: &Tunnel, x: &Wx, r: &mut ActorRecord, o: &mut Vec<(String, Value)>) {
    r.x = Some(f64::from(s.x));
    r.y = Some(f64::from(s.y));
    o.push(("p".into(), static_object()));
    tunnel_keys!(put s, x, o;);
}

/// [`tunnel_put`]'s inverse; the keys it knows go into `known`.
pub fn tunnel_load(s: &mut Tunnel, r: &ActorRecord, ob: &Obj, l: &mut Lx, known: &mut Vec<&'static str>) {
    opt_load(&mut s.x, r.x, "x", l);
    opt_load(&mut s.y, r.y, "y", l);
    tunnel_keys!(load s, ob, l;);
    known.extend(tunnel_keys!(names));
}

/// Buff_Tunneling_C's extension as the oracle writes it; `listener` is the bare OnJumpPressed actor (Deviation (n): the
/// flag), or null.
pub fn tunneling_put(e: &TunnelingExt, x: &Wx) -> Value {
    let n = Value::Num;
    let b = Value::Bool;
    let listener = match e.listener {
        Some(f) => Value::Obj(Obj::new(vec![
            ("cls".to_owned(), Value::Str("Buff_Tunneling_C.OnJumpPressed".to_owned())),
            ("id".to_owned(), n(-1.0)),
            ("dead".to_owned(), b(false)),
            ("owner".to_owned(), f.to_value(x)),
            ("p".to_owned(), Value::Obj(Obj::new(Vec::new()))),
        ])),
        None => Value::Null,
    };
    Value::Obj(Obj::new(vec![
        ("start".to_owned(), n(f64::from(e.start))),
        ("left".to_owned(), n(f64::from(e.left))),
        ("expired".to_owned(), b(e.expired)),
        ("early".to_owned(), b(e.early)),
        ("pending".to_owned(), n(f64::from(e.pending))),
        ("facingLeft".to_owned(), b(e.facing_left)),
        ("x".to_owned(), n(f64::from(e.x))),
        ("y".to_owned(), n(f64::from(e.y))),
        ("listener".to_owned(), listener),
    ]))
}

/// [`tunneling_put`]'s inverse.
pub fn tunneling_load(e: &mut TunnelingExt, v: &Value, l: &mut Lx) {
    let Some(ob) = as_obj(v, l) else { return };
    l.at("start", |l| e.start.load_value(ob.get("start").unwrap_or(&Value::Null), l));
    l.at("left", |l| e.left.load_value(ob.get("left").unwrap_or(&Value::Null), l));
    l.at("expired", |l| e.expired.load_value(ob.get("expired").unwrap_or(&Value::Null), l));
    l.at("early", |l| e.early.load_value(ob.get("early").unwrap_or(&Value::Null), l));
    l.at("pending", |l| e.pending.load_value(ob.get("pending").unwrap_or(&Value::Null), l));
    l.at("facingLeft", |l| e.facing_left.load_value(ob.get("facingLeft").unwrap_or(&Value::Null), l));
    l.at("x", |l| e.x.load_value(ob.get("x").unwrap_or(&Value::Null), l));
    l.at("y", |l| e.y.load_value(ob.get("y").unwrap_or(&Value::Null), l));
    e.listener = match ob.get("listener") {
        Some(Value::Obj(lo)) => lo.get("owner").and_then(|o| FighterId::from_value(o, l)),
        _ => None,
    };
}

plain_struct!(TeleportExt {
    "sx" = sx, "sy" = sy, "ax" = ax, "ay" = ay, "dx" = dx, "dy" = dy, "alpha" = alpha, "rate" = rate, "left" = left,
    "post" = post, "ex0" = ex0, "ey0" = ey0, "ex1" = ex1, "ey1" = ey1, "by" = by,
});

/// Buff_TunnelTeleport_C's extension as the oracle writes it.
pub fn teleport_put(e: &TeleportExt, x: &Wx) -> Value {
    e.to_value(x)
}

/// [`teleport_put`]'s inverse.
pub fn teleport_load(e: &mut TeleportExt, v: &Value, l: &mut Lx) {
    e.load_value(v, l);
}

/// Buff_ProjectileThroughTunnel_C's extension on a spawned actor as the oracle writes it (`ElemBuff.ext`,
/// `actors::elem_ext_put`): `bx`, `by` from the begin; `edge`, `hurt`, `life` where set; the keys of StartMove once it
/// ran.
pub fn through_tunnel_put(e: &ThroughTunnelExt, x: &Wx) -> Value {
    let n = Value::Num;
    let mut o: Vec<(String, Value)> =
        vec![("bx".to_owned(), n(f64::from(e.bx))), ("by".to_owned(), n(f64::from(e.by)))];
    let mut put = |k: &str, v: Value| o.push((k.to_owned(), v));
    if e.edge {
        put("edge", Value::Bool(true));
    }
    if let Some(h) = e.carry.hurt {
        put("hurt", n(f64::from(h)));
    }
    if let Some(v) = e.carry.life {
        put("life", n(f64::from(v)));
    }
    if e.started {
        put("rate", n(f64::from(e.rate)));
        put("sx", n(f64::from(e.sx)));
        put("sy", n(f64::from(e.sy)));
        put("ex", n(f64::from(e.ex)));
        put("ey", n(f64::from(e.ey)));
        put("trail", Value::Bool(e.trail));
        put("vsx", n(f64::from(e.vsx)));
        put("vsy", n(f64::from(e.vsy)));
        put("vdx", n(f64::from(e.vdx)));
        put("vdy", n(f64::from(e.vdy)));
        put("t", n(f64::from(e.t)));
        put("play", Value::Bool(e.play));
    }
    let _ = x;
    Value::Obj(Obj::new(o))
}

/// [`through_tunnel_put`]'s inverse (`actors::elem_ext_load`, by the buff's class).
pub fn through_tunnel_load(v: &Value, l: &mut Lx) -> ThroughTunnelExt {
    let mut e = ThroughTunnelExt::default();
    let Some(ob) = as_obj(v, l) else { return e };
    let num = |k: &str, to: &mut mvs_sim::Real, l: &mut Lx| {
        if let Some(v) = ob.get(k) {
            l.at(k, |l| to.load_value(v, l));
        }
    };
    num("bx", &mut e.bx, l);
    num("by", &mut e.by, l);
    e.edge = matches!(ob.get("edge"), Some(Value::Bool(true)));
    e.carry.hurt = match ob.get("hurt") {
        Some(Value::Num(h)) => Some(mvs_sim::math::narrow(*h)),
        _ => None,
    };
    e.carry.life = match ob.get("life") {
        Some(Value::Num(h)) => Some(mvs_sim::math::narrow(*h)),
        _ => None,
    };
    e.started = ob.get("sx").is_some();
    if e.started {
        num("rate", &mut e.rate, l);
        num("sx", &mut e.sx, l);
        num("sy", &mut e.sy, l);
        num("ex", &mut e.ex, l);
        num("ey", &mut e.ey, l);
        num("vsx", &mut e.vsx, l);
        num("vsy", &mut e.vsy, l);
        num("vdx", &mut e.vdx, l);
        num("vdy", &mut e.vdy, l);
        num("t", &mut e.t, l);
        e.trail = matches!(ob.get("trail"), Some(Value::Bool(true)));
        e.play = matches!(ob.get("play"), Some(Value::Bool(true)));
    }
    e
}
