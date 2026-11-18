//! A tutorial object's own state as the record writes it: a demo
//! controller's or a minigame actor's class fields, an arm per [`DemoState`] / [`MinigameState`] variant. The oracle
//! writes each class's own keys (its functions, the host and the World left out); an arm writes its variant's
//! fields under the oracle names, filled by the track that ports the class. An arm with no fields written yet writes
//! the empty object: the frame differs from the oracle one at `state` until it is filled.

use mvs_sim::modes::tutorial::demo::{DemoState, Place};
use mvs_sim::modes::tutorial::minigame::MinigameState;
use mvs_sim::world::World;
use mvs_trace::Value;

use crate::conv::{Plain, as_obj, check_keys, obj};
use crate::cx::{Lx, Wx};

use super::geometry as geo;
use super::meeseeks as mee;

/// A `Place` (`{x, y}`), null when unset.
pub(super) fn place_put(p: Option<Place>, x: &Wx) -> Value {
    p.map_or(Value::Null, |p| obj(vec![("x", p.x.to_value(x)), ("y", p.y.to_value(x))]))
}

pub(super) fn place_load(p: &mut Option<Place>, v: &Value, l: &mut Lx) {
    if *v == Value::Null {
        *p = None;
        return;
    }
    let Some(ob) = as_obj(v, l) else { return };
    check_keys(ob, &["x", "y"], l);
    let at = p.get_or_insert(Place { x: 0.0, y: 0.0 });
    if let Some(v) = ob.get("x") {
        l.at("x", |l| at.x.load_value(v, l));
    }
    if let Some(v) = ob.get("y") {
        l.at("y", |l| at.y.load_value(v, l));
    }
}

/// A demo controller class's fields.
pub fn demo_put(s: &DemoState, w: &World, x: &Wx) -> Value {
    match s {
        DemoState::Aerial(d) => {
            obj(vec![("at", place_put(d.at, x)), ("cast", d.cast.to_value(x)), ("evade", d.evade.to_value(x))])
        }
        DemoState::Heavy(d) => obj(vec![("at", place_put(d.at, x))]),
        DemoState::ShaggyKick(d) => {
            obj(vec![("at", place_put(d.at, x)), ("fighter", d.fighter.to_value(x)), ("isLeft", d.is_left.to_value(x))])
        }
        DemoState::Special(d) => obj(vec![("at", place_put(d.at, x)), ("dir", d.dir.to_value(x))]),
        DemoState::Evade(d) => obj(vec![("at", place_put(d.at, x)), ("fighter", d.fighter.to_value(x))]),
        DemoState::ChargeUp(d) => obj(vec![("at", place_put(d.at, x)), ("cast", d.cast.to_value(x))]),
        DemoState::Combat(d) => obj(vec![("bag", d.bag.to_value(x)), ("damage", d.damage.to_value(x))]),
        DemoState::AttackDecay(d) => obj(vec![
            ("at", place_put(d.at, x)),
            ("bag", d.bag.to_value(x)),
            ("bagAt", place_put(d.bag_at, x)),
            ("complete", d.complete.to_value(x)),
            ("fighter", d.fighter.to_value(x)),
        ]),
        DemoState::DodgeBasics(d) => {
            obj(vec![("at", place_put(d.at, x)), ("bag", d.bag.to_value(x)), ("fighter", d.fighter.to_value(x))])
        }
        DemoState::DodgeMeterDeplete(d) => obj(vec![("fighter", d.fighter.to_value(x))]),
        DemoState::DodgeMeterReplenish(_) => obj(vec![]),
        DemoState::CooldownsBasics(d) => obj(vec![("fighter", d.fighter.to_value(x))]),
        DemoState::ArmorBasics(d) => {
            let mut e = vec![("bag", d.bag.to_value(x)), ("fighter", d.fighter.to_value(x))];
            if !d.w_set {
                e.push(("w", Value::Null));
            }
            obj(e)
        }
        DemoState::ArmorBreak(d) => obj(vec![
            ("at", place_put(d.at, x)),
            ("bag", d.bag.to_value(x)),
            ("bagAt", place_put(d.bag_at, x)),
            ("complete", d.complete.to_value(x)),
            ("fighter", d.fighter.to_value(x)),
        ]),
        DemoState::DodgeJump(d) => {
            obj(vec![("at", place_put(d.at, x)), ("cast", d.cast.to_value(x)), ("firstLoop", d.first_loop.to_value(x))])
        }
        DemoState::AttackDecayVariety(d) => obj(vec![
            ("at", place_put(d.at, x)),
            ("bag", d.bag.to_value(x)),
            ("bagAt", place_put(d.bag_at, x)),
            ("complete", d.complete.to_value(x)),
            ("fighter", d.fighter.to_value(x)),
        ]),
        DemoState::WallSlide(d) => geo::wall_slide_put(d, w, x),
        DemoState::FastFall(d) => geo::fast_fall_put(d, x),
        DemoState::FastFallPlatform(d) => geo::fast_fall_platform_put(d, w, x),
        DemoState::KbiSide(d) => geo::kbi_side_demo_put(d, x),
        DemoState::KbiUp(d) => geo::kbi_up_demo_put(d, w, x),
        DemoState::AerialCombatBasics(d) => mee::basics_put(d, x),
        DemoState::AerialCombatHighUp(d) => mee::high_up_put(d, x),
        DemoState::EvadeToAttack(d) => mee::evade_put(d, x),
        DemoState::KillBotOffPlatform(d) => {
            let mut e = vec![
                ("at", place_put(d.at, x)),
                ("bot", d.bot.to_value(x)),
                ("cast", d.cast.to_value(x)),
                ("complete", d.complete.to_value(x)),
                ("destroyed", d.destroyed.to_value(x)),
                ("loopCast", d.loop_cast.to_value(x)),
            ];
            if !d.world_set {
                e.push(("world", Value::Null));
            }
            obj(e)
        }
        DemoState::ExtinguishFire(d) => obj(vec![("bag", d.bag.to_value(x)), ("fighter", d.fighter.to_value(x))]),
        DemoState::Ignite(d) => obj(vec![("bag", d.bag.to_value(x)), ("fighter", d.fighter.to_value(x))]),
        // (the oracle has no Batman class: no record holds their fields)
        DemoState::BatmanBatarang(_) | DemoState::BatmanRetrieve(_) | DemoState::BatmanDestroy(_) => obj(vec![]),
    }
}

/// [`demo_put`]'s inverse: the record's fields into the variant the World holds.
pub fn demo_load(s: &mut DemoState, v: &Value, l: &mut Lx) {
    match s {
        DemoState::Aerial(d) => {
            let Some(ob) = as_obj(v, l) else { return };
            check_keys(ob, &["at", "cast", "evade"], l);
            if let Some(v) = ob.get("at") {
                l.at("at", |l| place_load(&mut d.at, v, l));
            }
            if let Some(v) = ob.get("cast") {
                l.at("cast", |l| d.cast.load_value(v, l));
            }
            if let Some(v) = ob.get("evade") {
                l.at("evade", |l| d.evade.load_value(v, l));
            }
        }
        DemoState::Heavy(d) => {
            let Some(ob) = as_obj(v, l) else { return };
            check_keys(ob, &["at"], l);
            if let Some(v) = ob.get("at") {
                l.at("at", |l| place_load(&mut d.at, v, l));
            }
        }
        DemoState::ShaggyKick(d) => {
            let Some(ob) = as_obj(v, l) else { return };
            check_keys(ob, &["at", "fighter", "isLeft"], l);
            if let Some(v) = ob.get("at") {
                l.at("at", |l| place_load(&mut d.at, v, l));
            }
            if let Some(v) = ob.get("fighter") {
                l.at("fighter", |l| d.fighter.load_value(v, l));
            }
            if let Some(v) = ob.get("isLeft") {
                l.at("isLeft", |l| d.is_left.load_value(v, l));
            }
        }
        DemoState::Special(d) => {
            let Some(ob) = as_obj(v, l) else { return };
            check_keys(ob, &["at", "dir"], l);
            if let Some(v) = ob.get("at") {
                l.at("at", |l| place_load(&mut d.at, v, l));
            }
            if let Some(v) = ob.get("dir") {
                l.at("dir", |l| d.dir.load_value(v, l));
            }
        }
        DemoState::Evade(d) => {
            let Some(ob) = as_obj(v, l) else { return };
            check_keys(ob, &["at", "fighter"], l);
            if let Some(v) = ob.get("at") {
                l.at("at", |l| place_load(&mut d.at, v, l));
            }
            if let Some(v) = ob.get("fighter") {
                l.at("fighter", |l| d.fighter.load_value(v, l));
            }
        }
        DemoState::ChargeUp(d) => {
            let Some(ob) = as_obj(v, l) else { return };
            check_keys(ob, &["at", "cast"], l);
            if let Some(v) = ob.get("at") {
                l.at("at", |l| place_load(&mut d.at, v, l));
            }
            if let Some(v) = ob.get("cast") {
                l.at("cast", |l| d.cast.load_value(v, l));
            }
        }
        DemoState::Combat(d) => {
            let Some(ob) = as_obj(v, l) else { return };
            check_keys(ob, &["bag", "damage"], l);
            if let Some(v) = ob.get("bag") {
                l.at("bag", |l| d.bag.load_value(v, l));
            }
            if let Some(v) = ob.get("damage") {
                l.at("damage", |l| d.damage.load_value(v, l));
            }
        }
        DemoState::AttackDecay(d) => {
            let Some(ob) = as_obj(v, l) else { return };
            check_keys(ob, &["at", "bag", "bagAt", "complete", "fighter"], l);
            if let Some(v) = ob.get("at") {
                l.at("at", |l| place_load(&mut d.at, v, l));
            }
            if let Some(v) = ob.get("bag") {
                l.at("bag", |l| d.bag.load_value(v, l));
            }
            if let Some(v) = ob.get("bagAt") {
                l.at("bagAt", |l| place_load(&mut d.bag_at, v, l));
            }
            if let Some(v) = ob.get("complete") {
                l.at("complete", |l| d.complete.load_value(v, l));
            }
            if let Some(v) = ob.get("fighter") {
                l.at("fighter", |l| d.fighter.load_value(v, l));
            }
        }
        DemoState::DodgeBasics(d) => {
            let Some(ob) = as_obj(v, l) else { return };
            check_keys(ob, &["at", "bag", "fighter"], l);
            if let Some(v) = ob.get("at") {
                l.at("at", |l| place_load(&mut d.at, v, l));
            }
            if let Some(v) = ob.get("bag") {
                l.at("bag", |l| d.bag.load_value(v, l));
            }
            if let Some(v) = ob.get("fighter") {
                l.at("fighter", |l| d.fighter.load_value(v, l));
            }
        }
        DemoState::DodgeMeterDeplete(d) => {
            let Some(ob) = as_obj(v, l) else { return };
            check_keys(ob, &["fighter"], l);
            if let Some(v) = ob.get("fighter") {
                l.at("fighter", |l| d.fighter.load_value(v, l));
            }
        }
        DemoState::DodgeMeterReplenish(_) => {}
        DemoState::CooldownsBasics(d) => {
            let Some(ob) = as_obj(v, l) else { return };
            check_keys(ob, &["fighter"], l);
            if let Some(v) = ob.get("fighter") {
                l.at("fighter", |l| d.fighter.load_value(v, l));
            }
        }
        DemoState::ArmorBasics(d) => {
            let Some(ob) = as_obj(v, l) else { return };
            check_keys(ob, &["bag", "fighter", "w"], l);
            if let Some(v) = ob.get("bag") {
                l.at("bag", |l| d.bag.load_value(v, l));
            }
            if let Some(v) = ob.get("fighter") {
                l.at("fighter", |l| d.fighter.load_value(v, l));
            }
            d.w_set = ob.get("w") != Some(&Value::Null);
        }
        DemoState::ArmorBreak(d) => {
            let Some(ob) = as_obj(v, l) else { return };
            check_keys(ob, &["at", "bag", "bagAt", "complete", "fighter"], l);
            if let Some(v) = ob.get("at") {
                l.at("at", |l| place_load(&mut d.at, v, l));
            }
            if let Some(v) = ob.get("bag") {
                l.at("bag", |l| d.bag.load_value(v, l));
            }
            if let Some(v) = ob.get("bagAt") {
                l.at("bagAt", |l| place_load(&mut d.bag_at, v, l));
            }
            if let Some(v) = ob.get("complete") {
                l.at("complete", |l| d.complete.load_value(v, l));
            }
            if let Some(v) = ob.get("fighter") {
                l.at("fighter", |l| d.fighter.load_value(v, l));
            }
        }
        DemoState::DodgeJump(d) => {
            let Some(ob) = as_obj(v, l) else { return };
            check_keys(ob, &["at", "cast", "firstLoop"], l);
            if let Some(v) = ob.get("at") {
                l.at("at", |l| place_load(&mut d.at, v, l));
            }
            if let Some(v) = ob.get("cast") {
                l.at("cast", |l| d.cast.load_value(v, l));
            }
            if let Some(v) = ob.get("firstLoop") {
                l.at("firstLoop", |l| d.first_loop.load_value(v, l));
            }
        }
        DemoState::AttackDecayVariety(d) => {
            let Some(ob) = as_obj(v, l) else { return };
            check_keys(ob, &["at", "bag", "bagAt", "complete", "fighter"], l);
            if let Some(v) = ob.get("at") {
                l.at("at", |l| place_load(&mut d.at, v, l));
            }
            if let Some(v) = ob.get("bag") {
                l.at("bag", |l| d.bag.load_value(v, l));
            }
            if let Some(v) = ob.get("bagAt") {
                l.at("bagAt", |l| place_load(&mut d.bag_at, v, l));
            }
            if let Some(v) = ob.get("complete") {
                l.at("complete", |l| d.complete.load_value(v, l));
            }
            if let Some(v) = ob.get("fighter") {
                l.at("fighter", |l| d.fighter.load_value(v, l));
            }
        }
        DemoState::WallSlide(d) => geo::wall_slide_load(d, v, l),
        DemoState::FastFall(d) => geo::fast_fall_load(d, v, l),
        DemoState::FastFallPlatform(d) => geo::fast_fall_platform_load(d, v, l),
        DemoState::KbiSide(d) => geo::kbi_side_demo_load(d, v, l),
        DemoState::KbiUp(d) => geo::kbi_up_demo_load(d, v, l),
        DemoState::AerialCombatBasics(d) => mee::basics_load(d, v, l),
        DemoState::AerialCombatHighUp(d) => mee::high_up_load(d, v, l),
        DemoState::EvadeToAttack(d) => mee::evade_load(d, v, l),
        DemoState::KillBotOffPlatform(d) => {
            let Some(ob) = as_obj(v, l) else { return };
            check_keys(ob, &["at", "bot", "cast", "complete", "destroyed", "loopCast", "world"], l);
            if let Some(v) = ob.get("at") {
                l.at("at", |l| place_load(&mut d.at, v, l));
            }
            if let Some(v) = ob.get("bot") {
                l.at("bot", |l| d.bot.load_value(v, l));
            }
            if let Some(v) = ob.get("cast") {
                l.at("cast", |l| d.cast.load_value(v, l));
            }
            if let Some(v) = ob.get("complete") {
                l.at("complete", |l| d.complete.load_value(v, l));
            }
            if let Some(v) = ob.get("destroyed") {
                l.at("destroyed", |l| d.destroyed.load_value(v, l));
            }
            if let Some(v) = ob.get("loopCast") {
                l.at("loopCast", |l| d.loop_cast.load_value(v, l));
            }
            d.world_set = ob.get("world") != Some(&Value::Null);
        }
        DemoState::ExtinguishFire(d) => {
            let Some(ob) = as_obj(v, l) else { return };
            check_keys(ob, &["bag", "fighter"], l);
            if let Some(v) = ob.get("bag") {
                l.at("bag", |l| d.bag.load_value(v, l));
            }
            if let Some(v) = ob.get("fighter") {
                l.at("fighter", |l| d.fighter.load_value(v, l));
            }
        }
        DemoState::Ignite(d) => {
            let Some(ob) = as_obj(v, l) else { return };
            check_keys(ob, &["bag", "fighter"], l);
            if let Some(v) = ob.get("bag") {
                l.at("bag", |l| d.bag.load_value(v, l));
            }
            if let Some(v) = ob.get("fighter") {
                l.at("fighter", |l| d.fighter.load_value(v, l));
            }
        }
        DemoState::BatmanBatarang(_) | DemoState::BatmanRetrieve(_) | DemoState::BatmanDestroy(_) => {}
    }
}

/// A minigame actor class's fields.
pub fn minigame_put(s: &MinigameState, w: &World, x: &Wx) -> Value {
    match s {
        MinigameState::DodgeBasics(m) => {
            obj(vec![("enemies", m.enemies.to_value(x)), ("gameOver", m.game_over.to_value(x))])
        }
        MinigameState::DodgeRefill(_) => obj(vec![]),
        MinigameState::ArmorBreak(m) => {
            obj(vec![("enemies", m.enemies.to_value(x)), ("gameOver", m.game_over.to_value(x))])
        }
        MinigameState::SpawnWall(d) => geo::spawn_wall_put(d, w, x),
        MinigameState::SpawnPlatform(d) => geo::spawn_platform_put(d, w, x),
        MinigameState::KbiSide(d) => geo::kbi_side_put(d, w, x),
        MinigameState::KbiUp(d) => geo::kbi_up_put(d, w, x),
        MinigameState::AerialCombat(d) => mee::aerial_combat_put(d, x),
        MinigameState::PositioningTools(d) => mee::positioning_put(d, x),
        MinigameState::DownSpike(m) => obj(vec![
            ("bot", m.bot.to_value(x)),
            ("startTick", m.start_tick.to_value(x)),
            ("timeLimitSet", m.time_limit_set.to_value(x)),
        ]),
        MinigameState::ExtinguishFire(_) => obj(vec![]),
        MinigameState::Ignite(_) => obj(vec![]),
        // (the oracle has no Batman class: no record holds their fields)
        MinigameState::BatmanBatarang(_) | MinigameState::BatmanRetrieve(_) | MinigameState::BatmanDestroy(_) => {
            obj(vec![])
        }
    }
}

/// [`minigame_put`]'s inverse.
pub fn minigame_load(s: &mut MinigameState, v: &Value, l: &mut Lx) {
    match s {
        MinigameState::DodgeBasics(m) => {
            let Some(ob) = as_obj(v, l) else { return };
            check_keys(ob, &["enemies", "gameOver"], l);
            if let Some(v) = ob.get("enemies") {
                l.at("enemies", |l| m.enemies.load_value(v, l));
            }
            if let Some(v) = ob.get("gameOver") {
                l.at("gameOver", |l| m.game_over.load_value(v, l));
            }
        }
        MinigameState::DodgeRefill(_) => {}
        MinigameState::ArmorBreak(m) => {
            let Some(ob) = as_obj(v, l) else { return };
            check_keys(ob, &["enemies", "gameOver"], l);
            if let Some(v) = ob.get("enemies") {
                l.at("enemies", |l| m.enemies.load_value(v, l));
            }
            if let Some(v) = ob.get("gameOver") {
                l.at("gameOver", |l| m.game_over.load_value(v, l));
            }
        }
        MinigameState::SpawnWall(d) => geo::spawn_wall_load(d, v, l),
        MinigameState::SpawnPlatform(d) => geo::spawn_platform_load(d, v, l),
        MinigameState::KbiSide(d) => geo::kbi_side_load(d, v, l),
        MinigameState::KbiUp(d) => geo::kbi_up_load(d, v, l),
        MinigameState::AerialCombat(d) => mee::aerial_combat_load(d, v, l),
        MinigameState::PositioningTools(d) => mee::positioning_load(d, v, l),
        MinigameState::DownSpike(m) => {
            let Some(ob) = as_obj(v, l) else { return };
            check_keys(ob, &["bot", "startTick", "timeLimitSet"], l);
            if let Some(v) = ob.get("bot") {
                l.at("bot", |l| m.bot.load_value(v, l));
            }
            if let Some(v) = ob.get("startTick") {
                l.at("startTick", |l| m.start_tick.load_value(v, l));
            }
            if let Some(v) = ob.get("timeLimitSet") {
                l.at("timeLimitSet", |l| m.time_limit_set.load_value(v, l));
            }
        }
        MinigameState::ExtinguishFire(_) => {}
        MinigameState::Ignite(_) => {}
        MinigameState::BatmanBatarang(_) | MinigameState::BatmanRetrieve(_) | MinigameState::BatmanDestroy(_) => {}
    }
}
