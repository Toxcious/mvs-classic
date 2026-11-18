//! The Court's state as the record writes it (`stages/the-court/`): the entry's own state (`StageRun.s`) and its
//! placed actors' own keys.

use mvs_sim::stages::the_court::pickups::{CourtPickup, CourtPowerup, CourtTaken};
use mvs_sim::stages::the_court::score::CourtScore;
use mvs_sim::stages::the_court::{CourtOwn, M010Hoop, M010Wall};
use mvs_trace::{Obj, Value};

use crate::conv::{Plain, as_obj, check_keys, obj, plain_keys, plain_struct, text};
use crate::cx::{Lx, Wx};

plain_struct!(CourtPickup {
    "cls" = cls, "id" = id, "x" = x, "y" = y, "born" = born, "armed" = armed, "dead" = dead,
});

plain_struct!(CourtTaken {
    "cls" = cls, "id" = id, "fighter" = fighter, "tick" = tick, "x" = x, "y" = y,
});

plain_struct!(CourtScore {
    "byHoop" = by_hoop, "scoredAt" = scored_at, "scoredHoop" = scored_hoop, "tickAt" = tick_at,
    "finishedAt" = finished_at, "shownAt" = shown_at,
});

/// The powerup chain: `pickup`, `seq` and `taken` exist once PowerupSpawner began (AllowHazards true).
fn powerup_put(p: &CourtPowerup, x: &Wx) -> Value {
    let mut o: Vec<(String, Value)> = vec![("actor".into(), p.actor.to_value(x))];
    if p.begun {
        o.push(("pickup".into(), p.pickup.to_value(x)));
        o.push(("seq".into(), p.seq.to_value(x)));
        o.push(("taken".into(), p.taken.to_value(x)));
    }
    Value::Obj(Obj::new(o))
}

fn powerup_load(p: &mut CourtPowerup, v: &Value, l: &mut Lx) {
    let Some(ob) = as_obj(v, l) else { return };
    check_keys(ob, &["actor", "pickup", "seq", "taken"], l);
    if let Some(v) = ob.get("actor") {
        l.at("actor", |l| p.actor.load_value(v, l));
    }
    p.begun = ob.get("seq").is_some();
    if p.begun {
        if let Some(v) = ob.get("pickup") {
            l.at("pickup", |l| p.pickup.load_value(v, l));
        }
        if let Some(v) = ob.get("seq") {
            l.at("seq", |l| p.seq.load_value(v, l));
        }
        if let Some(v) = ob.get("taken") {
            l.at("taken", |l| p.taken.load_value(v, l));
        }
    }
}

/// `StageRun.s` of The Court.
pub fn own_put(s: &CourtOwn, x: &Wx) -> Value {
    let ball = obj(vec![("item", s.script.ball.item.to_value(x))]);
    obj(vec![
        ("hoops", s.hoops.to_value(x)),
        ("score", s.score.to_value(x)),
        ("script", obj(vec![("ball", ball), ("powerup", powerup_put(&s.script.powerup, x))])),
        ("walls", s.walls.to_value(x)),
    ])
}

pub fn own_load(s: &mut CourtOwn, v: &Value, l: &mut Lx) {
    let Some(ob) = as_obj(v, l) else { return };
    check_keys(ob, &["hoops", "score", "script", "walls"], l);
    if let Some(v) = ob.get("hoops") {
        l.at("hoops", |l| s.hoops.load_value(v, l));
    }
    if let Some(v) = ob.get("walls") {
        l.at("walls", |l| s.walls.load_value(v, l));
    }
    if let Some(v) = ob.get("score") {
        l.at("score", |l| s.score.load_value(v, l));
    }
    if let Some(v) = ob.get("script") {
        l.at("script", |l| {
            let Some(so) = as_obj(v, l) else { return };
            check_keys(so, &["ball", "powerup"], l);
            if let Some(b) = so.get("ball") {
                l.at("ball", |l| {
                    let Some(bo) = as_obj(b, l) else { return };
                    check_keys(bo, &["item"], l);
                    if let Some(i) = bo.get("item") {
                        l.at("item", |l| s.script.ball.item.load_value(i, l));
                    }
                });
            }
            if let Some(p) = so.get("powerup") {
                l.at("powerup", |l| powerup_load(&mut s.script.powerup, p, l));
            }
        });
    }
}

const HOOP_KEYS: &[&str] = &[
    "hoopIndex",
    "anim",
    "animAt",
    "scoredAt",
    "points",
    "explodedAt",
    "dodge",
    "inHit",
    "inTop",
    "inBack",
    "inBottom",
    "inFront",
];

/// A `M010Hoop`'s own keys (beside the `StageActor` ones), pushed on `o`.
pub fn m010_hoop_put(a: &M010Hoop, x: &Wx, o: &mut Vec<(String, Value)>) {
    plain_keys!(put a, x, o; "hoopIndex" = hoop_index, "animAt" = anim_at, "scoredAt" = scored_at, "points" = points, "explodedAt" = exploded_at, "dodge" ~ dodge, "inHit" = in_hit, "inTop" = in_top, "inBack" = in_back, "inBottom" = in_bottom, "inFront" = in_front);
    // the SkeletalMesh's last PlayAnimation: "" before any
    let anim = a.anim.map_or_else(String::new, |n| x.str(n).to_owned());
    o.push(("anim".into(), Value::Str(anim)));
}

/// Loads them from `ob`, each key read pushed on `known`.
pub fn m010_hoop_load(a: &mut M010Hoop, ob: &Obj, l: &mut Lx, known: &mut Vec<&'static str>) {
    plain_keys!(load a, ob, l; "hoopIndex" = hoop_index, "animAt" = anim_at, "scoredAt" = scored_at, "points" = points, "explodedAt" = exploded_at, "dodge" ~ dodge, "inHit" = in_hit, "inTop" = in_top, "inBack" = in_back, "inBottom" = in_bottom, "inFront" = in_front);
    if let Some(v) = ob.get("anim") {
        l.at("anim", |l| {
            a.anim = match text(v, l) {
                Some("") | None => None,
                Some(s) => Some(l.name(s)),
            };
        });
    }
    known.extend(HOOP_KEYS);
}

const WALL_KEYS: &[&str] = &[
    "floor",
    "left",
    "hp",
    "maxHp",
    "uy",
    "endY",
    "on",
    "edges",
    "glow",
    "damage",
    "dissolve",
    "hitAt",
    "brokenAt",
    "respawnAt",
    "touchAt",
    "touchX",
    "touchY",
    "movePos",
    "moveOn",
    "moveFrom",
    "dissolvePos",
    "dissolveOn",
    "dissolveFrom",
    "inside",
];

/// A `M010Wall`'s own keys (beside the `StageActor` ones), pushed on `o`.
pub fn m010_wall_put(a: &M010Wall, x: &Wx, o: &mut Vec<(String, Value)>) {
    plain_keys!(put a, x, o; "floor" = floor, "left" = left, "hp" = hp, "maxHp" = max_hp, "uy" = uy, "endY" = end_y, "on" = on, "edges" = edges, "glow" = glow, "damage" = damage, "dissolve" = dissolve, "hitAt" = hit_at, "brokenAt" = broken_at, "respawnAt" = respawn_at, "touchAt" = touch_at, "touchX" = touch_x, "touchY" = touch_y, "movePos" = move_pos, "moveOn" = move_on, "moveFrom" = move_from, "dissolvePos" = dissolve_pos, "dissolveOn" = dissolve_on, "dissolveFrom" = dissolve_from, "inside" = inside);
}

/// Loads them from `ob`, each key read pushed on `known`.
pub fn m010_wall_load(a: &mut M010Wall, ob: &Obj, l: &mut Lx, known: &mut Vec<&'static str>) {
    plain_keys!(load a, ob, l; "floor" = floor, "left" = left, "hp" = hp, "maxHp" = max_hp, "uy" = uy, "endY" = end_y, "on" = on, "edges" = edges, "glow" = glow, "damage" = damage, "dissolve" = dissolve, "hitAt" = hit_at, "brokenAt" = broken_at, "respawnAt" = respawn_at, "touchAt" = touch_at, "touchX" = touch_x, "touchY" = touch_y, "movePos" = move_pos, "moveOn" = move_on, "moveFrom" = move_from, "dissolvePos" = dissolve_pos, "dissolveOn" = dissolve_on, "dissolveFrom" = dissolve_from, "inside" = inside);
    known.extend(WALL_KEYS);
}
