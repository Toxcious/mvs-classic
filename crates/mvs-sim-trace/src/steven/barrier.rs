//! The barrier's actors as the record writes them: the Steven_ShieldActor classes (a
//! SpawnedPlatformParent child with a WallBox part: `plat.solids` lists the parent's two boxes, the actor's `solids`
//! and `wallBox` the WallBox too).

use mvs_data::value::Props;
use mvs_sim::actors::WorldActor;
use mvs_sim::chars::steven::barrier::{BoxCache, PlaneBox, StevenShield};
use mvs_sim::world::spawned_platform::{PlatformPart, SpawnedPlatform};
use mvs_trace::{ActorRecord, Obj, Value};

use crate::actors::{opt_load, static_object};
use crate::conv::{Plain, as_obj, check_keys, obj, plain_keys, plain_struct};
use crate::cx::{Lx, Wx};
use crate::shield_actor::{part_load, part_put, parts_put, plat_load, plat_put_without};
use mvs_sim::Real;

plain_struct!(PlaneBox { "hx" = hx, "hy" = hy, "hz" = hz });

macro_rules! shield_keys {
    ($mac:ident $($pre:tt)*) => {
        $mac!($($pre)*
            "age" = age, "blastAt" = blast_at, "blastBounds" = blast_bounds, "block" = block, "bounceCounter" = bounce_counter, "bouncePerk" = bounce_perk,
            "detect" = detect, "endAge" = end_age, "facing" = facing, "grounded" = grounded, "health" = health,
            "hitHeavy" ~ hit_heavy, "hitTick" ~ hit_tick, "hurt" = hurt, "inside" = inside, "near" = near,
            "pendingDeath" = pending_death, "solid" = solid, "tunneling" = tunneling,
        )
    };
}

/// `hurtBox` / `blockBox` as the oracle writes them: a sphere with the box's three half extents in `box` (nine numbers,
/// the diagonal), `r` and `z` 0.
fn cache_put(c: &BoxCache) -> Value {
    let b = [c.hx, 0.0, 0.0, 0.0, c.hy, 0.0, 0.0, 0.0, c.hz];
    obj(vec![
        ("box", Value::List(b.iter().map(|&n| Value::Num(f64::from(n))).collect())),
        ("r", Value::Num(0.0)),
        ("x", Value::Num(f64::from(c.x))),
        ("y", Value::Num(f64::from(c.y))),
        ("z", Value::Num(0.0)),
    ])
}

/// [`cache_put`]'s inverse.
fn cache_load(c: &mut BoxCache, v: &Value, l: &mut Lx) {
    let Some(ob) = as_obj(v, l) else { return };
    let mut b: [Real; 9] = [0.0; 9];
    match ob.get("box") {
        Some(v) => l.at("box", |l| b.load_value(v, l)),
        None => l.miss("key box absent"),
    }
    c.hx = b[0];
    c.hy = b[4];
    c.hz = b[8];
    if [1, 2, 3, 5, 6, 7].iter().any(|&i| b[i] != 0.0) {
        l.at("box", |l| l.miss("a half extent off the diagonal is not 0"));
    }
    match ob.get("x") {
        Some(v) => l.at("x", |l| c.x.load_value(v, l)),
        None => l.miss("key x absent"),
    }
    match ob.get("y") {
        Some(v) => l.at("y", |l| c.y.load_value(v, l)),
        None => l.miss("key y absent"),
    }
    for k in ["r", "z"] {
        let mut n: Real = 0.0;
        match ob.get(k) {
            Some(v) => l.at(k, |l| n.load_value(v, l)),
            None => l.miss(format_args!("key {k} absent")),
        }
        if n != 0.0 {
            l.at(k, |l| l.miss("not 0"));
        }
    }
    check_keys(ob, &["box", "r", "x", "y", "z"], l);
}

/// a Steven_ShieldActor as the record writes it (its platform: `a.plat`, as `shield_actor::shield_put`).
pub fn shield_put(a: &WorldActor, s: &StevenShield, x: &Wx, r: &mut ActorRecord, o: &mut Vec<(String, Value)>) {
    r.x = Some(f64::from(s.x));
    r.y = Some(f64::from(s.y));
    shield_keys!(plain_keys put s, x, o;);
    o.push(("blockBox".into(), cache_put(&s.block_box)));
    o.push(("hurtBox".into(), cache_put(&s.hurt_box)));
    o.push(("p".into(), static_object()));
    match &a.plat {
        Some(pl) => {
            o.push(("plat".into(), plat_put_without(pl, a.seq, x, s.wall_box)));
            o.push(("solids".into(), parts_put(&pl.parts, a.seq, x)));
            let wall = s.wall_box.and_then(|i| pl.parts.get(usize::from(i)));
            o.push(("wallBox".into(), wall.map_or(Value::Null, |p| part_put(p, a.seq, x))));
        }
        None => x.unmapped("a Steven_ShieldActor without its SpawnedPlatform"),
    }
}

/// [`shield_put`]'s inverse; the keys it knows go into `known`.
pub fn shield_load(
    s: &mut StevenShield,
    plat: &mut Option<SpawnedPlatform>,
    r: &ActorRecord,
    ob: &Obj,
    l: &mut Lx,
    known: &mut Vec<&'static str>,
) {
    opt_load(&mut s.x, r.x, "x", l);
    opt_load(&mut s.y, r.y, "y", l);
    l.at("rest", |l| {
        shield_keys!(plain_keys load s, ob, l;);
        match ob.get("blockBox") {
            Some(v) => l.at("blockBox", |l| cache_load(&mut s.block_box, v, l)),
            None => l.miss("key blockBox absent"),
        }
        match ob.get("hurtBox") {
            Some(v) => l.at("hurtBox", |l| cache_load(&mut s.hurt_box, v, l)),
            None => l.miss("key hurtBox absent"),
        }
        // The WallBox is the last part of the actor's list and not of `plat.solids`.
        if s.wall_box.take().is_some()
            && let Some(pl) = plat.as_mut()
        {
            pl.parts.pop();
        }
        match ob.get("plat") {
            Some(v) => l.at("plat", |l| plat_load(plat, v, l)),
            None => l.miss("key plat absent"),
        }
        match (ob.get("wallBox"), plat.as_mut()) {
            (Some(v @ Value::Obj(_)), Some(pl)) => {
                let mut p = PlatformPart::new(l.name("WallBox"), false, 0.0, 0.0, &Props::new());
                p.takes_ignore = false;
                l.at("wallBox", |l| part_load(&mut p, v, l));
                s.wall_box = Some(pl.push(p));
            }
            (Some(Value::Null), _) => {}
            (Some(Value::Obj(_)), None) => l.miss("a wallBox without a plat"),
            (Some(_), _) => l.at("wallBox", |l| l.miss("neither null nor an object")),
            (None, _) => l.miss("key wallBox absent"),
        }
        if let (Some(pl), Some(Value::List(sl))) = (plat.as_ref(), ob.get("solids"))
            && sl.len() != pl.parts.len()
        {
            l.at("solids", |l| l.miss("the actor's solids are not plat.solids and the wallBox"));
        }
    });
    known.extend(shield_keys!(plain_keys names));
    known.extend(["blockBox", "hurtBox", "p", "plat", "solids", "wallBox"]);
}
