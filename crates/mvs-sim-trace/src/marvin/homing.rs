//! Marvin's homing buffs as the record writes them: HomingVictimExt on the victim's buff and
//! HomingProjectileExt on the actor's, every key always written.

use mvs_sim::actors::ActorRef;
use mvs_sim::actors::projectile::BuffHome;
use mvs_sim::chars::marvin::homing::{HomingProjExt, HomingVictimExt};
use mvs_trace::{Obj, Value};

use crate::conv::{Plain, as_obj, check_keys, obj, plain_keys};
use crate::cx::{Lx, Wx};

macro_rules! victim_keys {
    (names) => {
        plain_keys!(names "marker" = marker, "team" = team, "over" = over, "queued" = queued, "retries" = retries, "retryAt" = retry_at)
    };
    ($mode:ident $($a:ident),*;) => {
        plain_keys!($mode $($a),*; "marker" = marker, "team" = team, "over" = over, "queued" = queued, "retries" = retries, "retryAt" = retry_at)
    };
}

macro_rules! proj_keys {
    (names) => {
        plain_keys!(names "pm" = pm, "oGrav" = o_grav, "assigned" = assigned, "cancelled" = cancelled, "closed" = closed, "target" = target, "waiting" = waiting, "pending" = pending)
    };
    ($mode:ident $($a:ident),*;) => {
        plain_keys!($mode $($a),*; "pm" = pm, "oGrav" = o_grav, "assigned" = assigned, "cancelled" = cancelled, "closed" = closed, "target" = target, "waiting" = waiting, "pending" = pending)
    };
}

/// Buff_C018HomingVictim_C's extension as the oracle writes it.
pub fn victim_put(e: &HomingVictimExt, x: &Wx) -> Value {
    let mut o: Vec<(String, Value)> = Vec::new();
    victim_keys!(put e, x, o;);
    o.push(("lastCast".to_owned(), last_cast_value(e, x)));
    Value::Obj(Obj::new(o))
}

/// [`victim_put`]'s inverse.
pub fn victim_load(e: &mut HomingVictimExt, v: &Value, l: &mut Lx) {
    let Some(ob) = as_obj(v, l) else { return };
    victim_keys!(load e, ob, l;);
    last_cast_load(e, ob, l);
    let mut known = victim_keys!(names).to_vec();
    known.push("lastCast");
    check_keys(ob, &known, l);
}

/// Buff_C018HomingProjectile_C's extension as the oracle writes it (`ElemBuff.ext`, `actors::elem_ext_put`); `oHome`
/// (the BuffHome the actor had, null for none) between `pm` and `oGrav`.
pub fn homing_projectile_put(e: &HomingProjExt, x: &Wx) -> Value {
    let mut o: Vec<(String, Value)> = Vec::new();
    let home: Option<BuffHome> = e.o_home.map(|(to, mag)| BuffHome { to, mag });
    plain_keys!(put e, x, o; "pm" = pm);
    o.push(("oHome".to_owned(), home.to_value(x)));
    plain_keys!(put e, x, o; "oGrav" = o_grav, "assigned" = assigned, "cancelled" = cancelled, "closed" = closed, "target" = target, "waiting" = waiting, "pending" = pending);
    Value::Obj(Obj::new(o))
}

/// [`homing_projectile_put`]'s inverse (`actors::elem_ext_load`, by the buff's class).
pub fn homing_projectile_load(v: &Value, l: &mut Lx) -> HomingProjExt {
    let mut e = HomingProjExt::default();
    let Some(ob) = as_obj(v, l) else { return e };
    proj_keys!(load e, ob, l;);
    let mut home: Option<BuffHome> = None;
    match ob.get("oHome") {
        Some(h) => l.at("oHome", |l| home.load_value(h, l)),
        None => l.miss(format_args!("key oHome absent")),
    }
    e.o_home = home.map(|h| (h.to, h.mag));
    let mut known: Vec<&'static str> = proj_keys!(names).to_vec();
    known.push("oHome");
    check_keys(ob, &known, l);
    e
}

/// `lastCast`: the actor's reference, or, once it is out of its list and the writer never saw it live, the oracle's
/// `{ ref: 'spawned', id }` from the id the ext kept.
fn last_cast_value(e: &HomingVictimExt, x: &Wx) -> Value {
    let live = match e.last_cast {
        Some(ActorRef::Projectile(id)) => x.w.actors.projectiles.get(id).is_some(),
        Some(ActorRef::Item(id)) => x.w.actors.items.get(id).is_some(),
        Some(_) => true,
        None => false,
    };
    match (e.last_cast, e.last_cast_seq) {
        (Some(a), _) if live => a.to_value(x),
        (_, Some(seq)) => obj(vec![("ref", Value::Str("spawned".to_owned())), ("id", Value::Num(f64::from(seq)))]),
        (Some(a), None) => a.to_value(x),
        (None, None) => Value::Null,
    }
}

/// [`last_cast_value`]'s inverse: a reference of a gone actor loads as none, with its id kept.
fn last_cast_load(e: &mut HomingVictimExt, ob: &Obj, l: &mut Lx) {
    e.last_cast = None;
    e.last_cast_seq = None;
    match ob.get("lastCast") {
        None => l.miss(format_args!("key lastCast absent")),
        Some(Value::Null) => {}
        Some(v) => {
            if let Value::Obj(o) = v
                && let Some(Value::Num(id)) = o.get("id")
            {
                e.last_cast_seq = Some(mvs_sim::math::to_int32(mvs_sim::math::narrow(*id)));
            }
            let mut a: Option<ActorRef> = None;
            l.at("lastCast", |l| a = <ActorRef as Plain>::from_value(v, l));
            e.last_cast = a;
        }
    }
}
