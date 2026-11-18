//! Marvin's UFO and its rocket explosions as the record writes them.
//!
//! The UFO's `solids` are its three `UfoPart`s (`col`, `midL`, `midR`, each with its `comp` and `main` besides a plain
//! part's keys) and the parent's WallbounceCollision; its `plat` lists that one alone and has no `collision`. The Rust
//! actor's platform holds all four parts, the three first.

use mvs_sim::actors::world_actor::WorldActor;
use mvs_sim::chars::marvin::ufo::{RocketExplosion, Ufo};
use mvs_sim::world::spawned_platform::SpawnedPlatform;
use mvs_trace::{ActorRecord, Obj, Value};

use crate::actors::{opt_load, static_object};
use crate::conv::{Plain, as_obj, plain_keys};
use crate::cx::{Lx, Wx};
use crate::parts::{anim_load, anim_put};
use crate::shield_actor::{part_load, part_make, part_put, plat_load, plat_put};

macro_rules! ufo_keys {
    ($mac:ident $($pre:tt)*) => {
        $mac!($($pre)*
            "age" = age, "aimFireAt" = aim_fire_at, "aimOn" = aim_on, "aimReadyAt" = aim_ready_at,
            "aimTarget" = aim_target, "aimTargetX" = aim_target_x, "aimTargetY" = aim_target_y, "aimerX" = aimer_x,
            "aimerY" = aimer_y, "ammo" = ammo, "beamOn" = beam_on, "blastAt" = blast_at,
            "blastBounds" = blast_bounds, "bounced" = bounced, "child" = child, "colOn" = col_on,
            "collisionAt" = collision_at, "collisionOn" = collision_on, "countL" = count_l, "countMid" = count_mid,
            "countR" = count_r, "despawnSoundAt" = despawn_sound_at, "destroyAt" = destroy_at,
            "destroyed" = destroyed, "destroyedSoundAt" = destroyed_sound_at, "disableCount" = disable_count,
            "facingLeft" = facing_left, "health" = health, "inL" = in_l, "inMid" = in_mid, "inR" = in_r,
            "introAt" = intro_at, "lifeAt" = life_at, "lifeEndAt" = life_end_at, "midLOn" = mid_l_on,
            "midROn" = mid_r_on, "portalDespawnAt" = portal_despawn_at, "rdt" = rdt, "readyToFire" = ready_to_fire,
            "reloadAt" = reload_at, "rocketHidden" = rocket_hidden, "shipHidden" = ship_hidden,
            "standing" = standing, "uiReloadAt" = ui_reload_at, "uiState" = ui_state, "uiValue" = ui_value,
        )
    };
}

/// The three UfoParts' keys.
const PARTS: [(&str, &str); 3] = [("col", "Collision"), ("midL", "CollisionMidLeft"), ("midR", "CollisionMidRight")];

/// A part with `comp` and `main` (a `UfoPart`'s own keys).
fn ufo_part_put(p: &mvs_sim::world::spawned_platform::PlatformPart, seq: i32, x: &Wx, comp: &str, main: bool) -> Value {
    let Value::Obj(mut o) = part_put(p, seq, x) else { return Value::Null };
    o.insert("comp".into(), Value::Str(comp.into()));
    o.insert("main".into(), Value::Bool(main));
    Value::Obj(o)
}

/// A UFO's box `i` (0..3) as the oracle writes it, wherever it is written whole (a fighter's `base`).
pub(crate) fn ufo_part_value(p: &mvs_sim::world::spawned_platform::PlatformPart, seq: i32, x: &Wx, i: usize) -> Value {
    ufo_part_put(p, seq, x, PARTS[i].1, i == 0)
}

/// `v` without `comp` and `main`.
fn plain_part(v: &Value, l: &mut Lx) -> Option<Value> {
    let ob = as_obj(v, l)?;
    let rest: Vec<(String, Value)> = ob.entries().iter().filter(|(k, _)| k != "comp" && k != "main").cloned().collect();
    Some(Value::Obj(Obj::new(rest)))
}

/// a UFO of Marvin as the record writes it (`a`: its world actor, for the platform and its parts).
pub fn ufo_put(a: &WorldActor, s: &Ufo, x: &Wx, r: &mut ActorRecord, o: &mut Vec<(String, Value)>) {
    r.x = Some(f64::from(s.x));
    r.y = Some(f64::from(s.y));
    ufo_keys!(plain_keys put s, x, o;);
    o.push(("anim".into(), anim_put(&s.anim, x)));
    o.push(("onCamera".into(), Value::Bool(true)));
    o.push(("p".into(), static_object()));
    let Some(pl) = &a.plat else {
        x.unmapped("a UFO without its SpawnedPlatform (dead at its spawn)");
        return;
    };
    for (i, (key, comp)) in PARTS.into_iter().enumerate() {
        if let Some(p) = pl.parts.get(i) {
            o.push((key.into(), ufo_part_put(p, a.seq, x, comp, i == 0)));
        }
    }
    let mut v = plat_put(pl, a.seq, x);
    if let Value::Obj(po) = &mut v {
        let wb = pl.wallbounce.and_then(|i| pl.parts.get(usize::from(i)));
        po.insert("solids".into(), Value::List(wb.map(|p| part_put(p, a.seq, x)).into_iter().collect()));
        po.insert("collision".into(), Value::Null);
    }
    o.push(("plat".into(), v));
    let solids = pl
        .parts
        .iter()
        .enumerate()
        .map(|(i, p)| if i < 3 { ufo_part_value(p, a.seq, x, i) } else { part_put(p, a.seq, x) });
    o.push(("solids".into(), Value::List(solids.collect())));
}

/// [`ufo_put`]'s inverse; the keys it knows go into `known`.
pub fn ufo_load(
    s: &mut Ufo,
    plat: &mut Option<SpawnedPlatform>,
    r: &ActorRecord,
    ob: &Obj,
    l: &mut Lx,
    known: &mut Vec<&'static str>,
) {
    opt_load(&mut s.x, r.x, "x", l);
    opt_load(&mut s.y, r.y, "y", l);
    l.at("rest", |l| {
        ufo_keys!(plain_keys load s, ob, l;);
        match ob.get("anim") {
            Some(v) => l.at("anim", |l| anim_load(&mut s.anim, v, l)),
            None => l.miss("key anim absent"),
        }
        // The platform: the oracle `plat` holds the WallbounceCollision alone; the three UfoParts come first here.
        let Some(pv) = ob.get("plat") else {
            l.miss("key plat absent");
            return;
        };
        let mut front: Vec<_> = Vec::new();
        if let Some(pl) = plat.as_mut()
            && pl.parts.len() == 4
        {
            front = pl.parts.drain(..3).collect();
            pl.wallbounce = Some(0);
        }
        l.at("plat", |l| plat_load(plat, pv, l));
        let Some(pl) = plat.as_mut() else { return };
        let fresh = front.is_empty();
        if fresh {
            for (i, (key, _)) in PARTS.into_iter().enumerate() {
                let made = ob.get(key).and_then(|v| plain_part(v, l)).map(|v| l.at(key, |l| part_make(&v, l)));
                match made {
                    Some(mut p) => {
                        p.takes_ignore = i == 0;
                        if let Some(Value::Obj(po)) = ob.get(key)
                            && let Some(Value::Str(c)) = po.get("comp")
                        {
                            p.comp = l.name(c);
                        }
                        front.push(p);
                    }
                    None => l.miss(format_args!("key {key} absent")),
                }
            }
        } else {
            for (i, (key, _)) in PARTS.into_iter().enumerate() {
                match ob.get(key).and_then(|v| plain_part(v, l)) {
                    Some(v) => l.at(key, |l| part_load(&mut front[i], &v, l)),
                    None => l.miss(format_args!("key {key} absent")),
                }
            }
        }
        if front.len() == 3 {
            pl.parts.splice(0..0, front);
            pl.wallbounce = pl.wallbounce.map(|i| i + 3);
        }
    });
    known.extend(ufo_keys!(plain_keys names));
    known.extend(["anim", "onCamera", "p", "plat", "solids", "col", "midL", "midR"]);
}

macro_rules! rocket_keys {
    ($mac:ident $($pre:tt)*) => {
        $mac!($($pre)*
            "age" = age, "attackId" = attack_id, "blastAt" = blast_at, "blastBounds" = blast_bounds,
            "endAge" = end_age, "facing" = facing, "hit" = hit, "hitOn" = hit_on, "hitSet" = hit_set,
            "offAt" = off_at, "once" = once, "over" = over, "r" = r, "types" = types,
        )
    };
}

/// a rocket explosion of Marvin as the record writes it (vx and vy readonly 0).
pub fn rocket_explosion_put(s: &RocketExplosion, x: &Wx, r: &mut ActorRecord, o: &mut Vec<(String, Value)>) {
    r.x = Some(f64::from(s.x));
    r.y = Some(f64::from(s.y));
    r.vx = Some(0.0);
    r.vy = Some(0.0);
    rocket_keys!(plain_keys put s, x, o;);
    o.push(("p".into(), static_object()));
}

/// [`rocket_explosion_put`]'s inverse; the keys it knows go into `known`.
pub fn rocket_explosion_load(
    s: &mut RocketExplosion,
    r: &ActorRecord,
    ob: &Obj,
    l: &mut Lx,
    known: &mut Vec<&'static str>,
) {
    opt_load(&mut s.x, r.x, "x", l);
    opt_load(&mut s.y, r.y, "y", l);
    l.at("rest", |l| {
        rocket_keys!(plain_keys load s, ob, l;);
    });
    known.extend(rocket_keys!(plain_keys names));
}

// `Plain` is named for the macros' expansions.
const _: fn() = || {
    fn plain<T: Plain>() {}
    plain::<mvs_sim::Real>();
};
