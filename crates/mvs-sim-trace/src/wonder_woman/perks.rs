//! Wonder Woman's perks as the record writes them: the FastFallSlam and ProjectileBlock
//! buffs' extensions and the evade shield (a world actor, a ShieldActor of its own).

use mvs_data::value::Value as DataValue;
use mvs_sim::actors::ActorRef;
use mvs_sim::actors::world_actor::WorldActor;
use mvs_sim::chars::wonder_woman::perks::{Block, EvadeShield, Slam, SlamDealer};
use mvs_sim::fighter::common::buff_def;
use mvs_sim::world::spawned_platform::SpawnedPlatform;
use mvs_trace::{ActorRecord, Obj, Value};

use crate::conv::{Plain, as_obj, check_keys, obj};
use crate::cx::{Lx, Wx};
use crate::parts::{data_put, static_put};

const SLAM: &str = "Buff_Perk_WonderWoman_FastFallSlam_C";

/// The slam's dealer (`DealerActor`, made at the first test): its place, facing and owner, the rest the class's data
/// (`hit` its HitBoxInfo, `p` its defaults, `attackId` HitBoxInfo's AttackID).
fn dealer_put(d: &SlamDealer, x: &Wx) -> Value {
    let cls = x.w.names.get(SLAM);
    let def = cls.and_then(|c| buff_def(x.w, ActorRef::Fighter(d.owner), c));
    let hit = def.and_then(|p| p.get("HitBoxInfo"));
    let attack_id =
        hit.and_then(DataValue::as_obj).and_then(|h| h.get("AttackID")).and_then(DataValue::as_f64).unwrap_or(0.0);
    obj(vec![
        ("attackId", Value::Num(attack_id)),
        ("buffActor", Value::Bool(true)),
        ("cls", Value::Str(SLAM.into())),
        ("dead", Value::Bool(false)),
        ("facing", d.facing.to_value(x)),
        ("hit", hit.map_or_else(|| obj(vec![]), data_put)),
        ("id", Value::Num(-1.0)),
        ("owner", ActorRef::Fighter(d.owner).to_value(x)),
        ("p", def.map_or_else(|| obj(vec![]), static_put)),
        ("vx", Value::Num(0.0)),
        ("vy", Value::Num(0.0)),
        ("x", d.x.to_value(x)),
        ("y", d.y.to_value(x)),
    ])
}

fn dealer_load(v: &Value, l: &mut Lx) -> Option<SlamDealer> {
    let o = as_obj(v, l)?;
    let num = |k: &str| match o.get(k) {
        Some(Value::Num(n)) => Some(*n),
        _ => None,
    };
    let owner = match o.get("owner").and_then(|v| l.at("owner", |l| ActorRef::from_value(v, l))) {
        Some(ActorRef::Fighter(f)) => f,
        _ => {
            l.miss("a slam dealer whose owner is no fighter");
            return None;
        }
    };
    let (Some(x), Some(y), Some(facing)) = (num("x"), num("y"), num("facing")) else {
        l.miss("a slam dealer without x, y, facing");
        return None;
    };
    check_keys(
        o,
        &["attackId", "buffActor", "cls", "dead", "facing", "hit", "id", "owner", "p", "vx", "vy", "x", "y"],
        l,
    );
    Some(SlamDealer {
        x: mvs_sim::math::narrow(x),
        y: mvs_sim::math::narrow(y),
        facing: mvs_sim::math::narrow(facing),
        owner,
    })
}

/// Buff_Perk_WonderWoman_FastFallSlam's extension as the oracle writes it (`dodge` left out while unset).
pub fn slam_put(e: &Slam, x: &Wx) -> Value {
    let mut o = vec![
        ("dealer", e.dealer.as_ref().map_or(Value::Null, |d| dealer_put(d, x))),
        ("delay", e.delay.to_value(x)),
        ("hits", e.hits.to_value(x)),
        ("on", Value::Bool(e.on)),
        ("once", e.once.to_value(x)),
        ("over", e.over.to_value(x)),
        ("scale", e.scale.to_value(x)),
    ];
    if let Some(d) = &e.dodge {
        o.push(("dodge", d.to_value(x)));
    }
    obj(o)
}

/// One key of an extension's object into its field.
fn key<T: Plain>(o: &Obj, k: &str, set: &mut T, l: &mut Lx) {
    match o.get(k) {
        Some(v) => l.at(k, |l| set.load_value(v, l)),
        None => l.miss(format_args!("key {k} absent")),
    }
}

/// [`slam_put`]'s inverse.
pub fn slam_load(e: &mut Slam, v: &Value, l: &mut Lx) {
    let Some(o) = as_obj(v, l) else { return };
    key(o, "delay", &mut e.delay, l);
    key(o, "hits", &mut e.hits, l);
    key(o, "on", &mut e.on, l);
    key(o, "once", &mut e.once, l);
    key(o, "over", &mut e.over, l);
    key(o, "scale", &mut e.scale, l);
    e.dealer = match o.get("dealer") {
        Some(Value::Null) | None => None,
        Some(v) => l.at("dealer", |l| dealer_load(v, l)),
    };
    e.dodge = match o.get("dodge") {
        Some(v) => l.at("dodge", |l| Plain::from_value(v, l)),
        None => None,
    };
    check_keys(o, &["dealer", "delay", "dodge", "hits", "on", "once", "over", "scale"], l);
}

/// Buff_Perk_WonderWoman_ProjectileBlock's extension as the oracle writes it.
pub fn block_put(e: &Block, x: &Wx) -> Value {
    obj(vec![("timer", e.timer.to_value(x))])
}

/// [`block_put`]'s inverse.
pub fn block_load(e: &mut Block, v: &Value, l: &mut Lx) {
    let Some(o) = as_obj(v, l) else { return };
    key(o, "timer", &mut e.timer, l);
    check_keys(o, &["timer"], l);
}

/// The evade shield's own keys and place: a ShieldActor's (`a`: its world actor, for the platform).
pub fn evade_put(a: &WorldActor, s: &EvadeShield, x: &Wx, r: &mut ActorRecord, o: &mut Vec<(String, Value)>) {
    crate::shield_actor::shield_put(a, &s.base, x, r, o);
}

/// [`evade_put`]'s inverse.
pub fn evade_load(
    s: &mut EvadeShield,
    plat: &mut Option<SpawnedPlatform>,
    r: &ActorRecord,
    ob: &Obj,
    l: &mut Lx,
    known: &mut Vec<&'static str>,
) {
    crate::shield_actor::shield_load(&mut s.base, plat, r, ob, l, known);
}
