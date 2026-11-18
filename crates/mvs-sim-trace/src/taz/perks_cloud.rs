//! The perk TeamFightCloud's buffs as the record writes them.
//!
//! The cloud's `dogpile` is a reference while the cloud is in the actors' list and the object whole once it is gone
//! (the oracle keeps it). `attach` is the whole buff entry of Buff_C015AttachToDogpile_C, whose `cloud` names the perk
//! buff's entry back: a cycle no rule writes (no match has one).

use mvs_sim::actors::world_actor::WorldActor;
use mvs_sim::actors::{ActorId, ActorRef};
use mvs_sim::chars::taz::perks_cloud::{AttachExt, CloudState};
use mvs_sim::combat::attach_ally::AttachRel;
use mvs_sim::fighter::FighterId;
use mvs_trace::{ActorRecord, Obj, Value};

use crate::conv::{Plain, as_obj, check_keys, obj, plain_keys};
use crate::cx::{Lx, Wx};

const CLOUD_KEYS: [&str; 14] = [
    "ally",
    "attach",
    "attachOn",
    "bound",
    "connected",
    "dogpile",
    "inputs",
    "inside",
    "labels",
    "origDur",
    "origHits",
    "origMax",
    "origMin",
    "overlap",
];

fn aref(id: Option<ActorId>, x: &Wx) -> Value {
    id.map(ActorRef::Actor).to_value(x)
}

fn aref_load(v: &Value, l: &mut Lx) -> Option<ActorId> {
    match Option::<ActorRef>::from_value(v, l).flatten() {
        Some(ActorRef::Actor(id)) => Some(id),
        None => None,
        Some(_) => {
            l.miss("a reference to no world actor");
            None
        }
    }
}

/// The gone cloud whole, as the oracle writes the object it keeps.
fn gone_put(a: &WorldActor, x: &Wx) -> Value {
    let r = crate::actors::actor_put(a, x);
    let mut o: Vec<(String, Value)> = vec![
        ("cls".into(), Value::Str(r.class.clone())),
        ("dead".into(), Value::Bool(r.dead)),
        ("id".into(), Value::Num(r.id)),
        ("owner".into(), a.owner.to_value(x)),
        ("vx".into(), Value::Num(r.vx.unwrap_or(0.0))),
        ("vy".into(), Value::Num(r.vy.unwrap_or(0.0))),
        ("x".into(), Value::Num(r.x.unwrap_or(0.0))),
        ("y".into(), Value::Num(r.y.unwrap_or(0.0))),
    ];
    if let Value::Obj(rest) = &r.rest {
        o.extend(rest.entries().iter().cloned());
    }
    Value::Obj(Obj::new(o))
}

/// [`gone_put`]'s inverse.
fn gone_load(o: &Obj, l: &mut Lx) -> Option<Box<WorldActor>> {
    let num = |k: &str| match o.get(k) {
        Some(Value::Num(n)) => Some(*n),
        _ => None,
    };
    let (Some(Value::Str(class)), Some(id)) = (o.get("cls"), num("id")) else {
        l.miss("a gone cloud with no class or id");
        return None;
    };
    let owner = match o.get("owner") {
        Some(Value::Obj(r)) => match r.get("id") {
            Some(Value::Num(n)) => *n,
            _ => -1.0,
        },
        _ => -1.0,
    };
    const OWN: [&str; 8] = ["cls", "dead", "id", "owner", "vx", "vy", "x", "y"];
    let rest = o.entries().iter().filter(|(k, _)| !OWN.contains(&k.as_str())).cloned().collect();
    let r = ActorRecord {
        family: "actors".to_owned(),
        id,
        class: class.clone(),
        owner,
        x: num("x"),
        y: num("y"),
        vx: num("vx"),
        vy: num("vy"),
        dead: matches!(o.get("dead"), Some(Value::Bool(true))),
        rest: Value::Obj(Obj::new(rest)),
        ..ActorRecord::default()
    };
    crate::actors::actor_make(&r, l).map(Box::new)
}

/// Buff_C015_TeamFightCloud_C's extension as the oracle writes it.
pub fn cloud_put(e: &CloudState, x: &Wx) -> Value {
    let dogpile = match (&e.dogpile, &e.gone) {
        (Some(id), _) => aref(Some(*id), x),
        (None, Some(a)) => gone_put(a, x),
        (None, None) => Value::Null,
    };
    let (attach, attach_on) = match e.attach {
        Some(b) => {
            x.unmapped("Buff_C015_TeamFightCloud_C's attach buff (its entry names the perk buff back: no rule)");
            (Value::Null, b.fighter.to_value(x))
        }
        None => (Value::Null, Value::Null),
    };
    let bound: Vec<ActorRef> = e.bound.iter().map(|&id| ActorRef::Actor(id)).collect();
    let mut o: Vec<(String, Value)> = vec![
        ("ally".into(), e.ally.to_value(x)),
        ("attach".into(), attach),
        ("attachOn".into(), attach_on),
        ("bound".into(), bound.to_value(x)),
        ("connected".into(), e.connected.to_value(x)),
        ("dogpile".into(), dogpile),
    ];
    plain_keys!(put e, x, o; "inputs" = inputs, "inside" = inside, "labels" = labels, "origDur" = orig_dur,
        "origHits" = orig_hits, "origMax" = orig_max, "origMin" = orig_min,);
    o.push(("overlap".into(), aref(e.overlap, x)));
    Value::Obj(Obj::new(o))
}

/// [`cloud_put`]'s inverse.
pub fn cloud_load(e: &mut CloudState, v: &Value, l: &mut Lx) {
    let Some(o) = as_obj(v, l) else { return };
    plain_keys!(load e, o, l; "ally" = ally, "connected" = connected, "inputs" = inputs, "inside" = inside,
        "labels" = labels, "origDur" = orig_dur, "origHits" = orig_hits, "origMax" = orig_max, "origMin" = orig_min,);
    e.attach = None;
    if !matches!(o.get("attach"), None | Some(Value::Null)) {
        l.at("attach", |l| l.miss("an attach buff (no rule to load it)"));
    }
    e.bound = match o.get("bound") {
        Some(Value::List(xs)) => l.at("bound", |l| xs.iter().filter_map(|v| aref_load(v, l)).collect()),
        _ => {
            l.miss("key bound absent");
            Vec::new()
        }
    };
    e.overlap = match o.get("overlap") {
        Some(v) => l.at("overlap", |l| aref_load(v, l)),
        None => None,
    };
    (e.dogpile, e.gone) = match o.get("dogpile") {
        None | Some(Value::Null) => (None, None),
        Some(Value::Obj(d)) if d.get("ref").is_none() => (None, l.at("dogpile", |l| gone_load(d, l))),
        Some(v) => (l.at("dogpile", |l| aref_load(v, l)), None),
    };
    e.ending = None;
    check_keys(o, &CLOUD_KEYS, l);
}

fn rel_put(r: Option<AttachRel>, x: &Wx) -> Option<Value> {
    r.map(|r| obj(vec![("dx", r.dx.to_value(x)), ("dy", r.dy.to_value(x))]))
}

/// Buff_C015AttachToDogpile_C's extension as the oracle writes it (`AttachState`, then `cloud`).
pub fn attach_put(e: &AttachExt, x: &Wx) -> Value {
    let mut o: Vec<(String, Value)> =
        vec![("by".into(), e.attach.by.to_value(x)), ("bound".into(), e.attach.bound.to_value(x))];
    if let Some(r) = rel_put(e.attach.rel, x) {
        o.push(("rel".into(), r));
    }
    if e.cloud.is_some() {
        x.unmapped("Buff_C015AttachToDogpile_C's cloud (the perk buff's entry names this one back: no rule)");
    }
    Value::Obj(Obj::new(o))
}

/// [`attach_put`]'s inverse.
pub fn attach_load(e: &mut AttachExt, v: &Value, l: &mut Lx) {
    let Some(o) = as_obj(v, l) else { return };
    e.attach.by = match o.get("by") {
        Some(v) => l.at("by", |l| Option::<FighterId>::from_value(v, l).flatten()),
        None => None,
    };
    plain_keys!(load e, o, l; "bound" = attach.bound,);
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
    e.cloud = None;
    if o.get("cloud").is_some() {
        l.at("cloud", |l| l.miss("the perk buff's entry (no rule to load it)"));
    }
    check_keys(o, &["bound", "by", "cloud", "rel"], l);
}
