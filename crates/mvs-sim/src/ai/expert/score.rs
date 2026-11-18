//! The Expert's score of a rollout, from the bot's side, at the horizon: what changed since the plan's first tick
//! (damage, knockouts) and where the fighters stand (hitstun, the blast zones, off stage). One point is one percent
//! of damage; the weights are hand-written.

use crate::Real;
use crate::fighter::FighterId;
use crate::math;
use crate::world::World;

/// A knockout, given or taken.
const KO: Real = 300.0;
/// A percent of damage.
const DAMAGE: Real = 1.0;
/// A frame of hitstun left at the horizon.
const STUN: Real = 0.4;
/// A fighter near a blast zone: at the edge, this many points per 100 percent of its damage, falling to none
/// `DANGER_RANGE` away.
const DANGER: Real = 40.0;
const DANGER_RANGE: Real = 1200.0;
/// The bot near a blast zone whatever its damage (riding a rising platform out, drifting off the side): points at
/// the edge, falling to none `EDGE_RANGE` away. A ring-out past the horizon is not seen any other way.
const EDGE: Real = 60.0;
const EDGE_RANGE: Real = 500.0;
/// Standing on a spawned platform (a ship, a rocket), the bot's place is also taken this many horizons further along
/// the way the platform carried it over the rollout: a ride out of the blast zone shows before the horizon reaches it.
const RIDE_AHEAD: Real = 3.0;
/// The bot off stage: points per unit outside the floor's span, per unit below its top, and back per air jump and
/// air dodge it still has.
const OFF_X: Real = 0.03;
const OFF_BELOW: Real = 0.06;
const AIR_JUMP: Real = 8.0;
const AIR_DODGE: Real = 5.0;
/// Per unit of horizontal distance to the nearest other fighter (close in, where the attacks reach).
const APART: Real = 0.004;

/// The fighters' damage and knockouts at a plan's first tick, by slot.
#[derive(Clone, Debug, Default)]
pub struct Before {
    damage: Vec<Real>,
    kos: Vec<i32>,
    at: Vec<(Real, Real)>,
}

impl Before {
    pub fn take(&mut self, w: &World) {
        self.damage.clear();
        self.kos.clear();
        self.at.clear();
        for &f in &w.players {
            let x = w.fighter(f);
            self.damage.push(x.stats.damage);
            self.kos.push(x.stats.kos);
            self.at.push((x.body.x, x.body.y));
        }
    }
}

/// How far (`x`, `y`) is from the nearest blast zone (below 0 past it).
fn to_blast_at(w: &World, x: Real, y: Real) -> Real {
    let b = &w.stage.blast;
    let (lo, hi) = (math::min(b.left, b.right), math::max(b.left, b.right));
    math::min(math::min(x - lo, hi - x), math::min(y - b.bottom, b.top - y))
}

/// How far `f` is from the nearest blast zone.
fn to_blast(w: &World, f: FighterId) -> Real {
    let b = &w.fighter(f).body;
    to_blast_at(w, b.x, b.y)
}

/// How close `f` is to a blast zone, scaled by its damage.
fn danger(w: &World, f: FighterId) -> Real {
    let near = math::max(0.0, 1.0 - to_blast(w, f) / DANGER_RANGE);
    DANGER * near * w.fighter(f).stats.damage / 100.0
}

/// One fighter's standing: damage and knockouts taken since `before`, its hitstun and danger.
fn standing(w: &World, before: &Before, i: usize, f: FighterId) -> Real {
    let x = w.fighter(f);
    let ko = before.kos.get(i).is_some_and(|&k| x.stats.kos > k) || x.life.dead != 0.0;
    let taken = if ko { KO } else { DAMAGE * (x.stats.damage - before.damage.get(i).copied().unwrap_or(0.0)) };
    let stun = if crate::fighter::in_hitstun(w, f) { STUN * math::max(0.0, x.hit.hitstun) } else { 0.0 };
    taken + stun + if ko { 0.0 } else { danger(w, f) }
}

/// The bot's own place: how near a blast zone it is (or a platform it rides takes it), off stage and the recovery
/// it has left, and how far it is from the nearest other fighter.
fn place(w: &World, before: &Before, slot: usize, me: FighterId) -> Real {
    let x = w.fighter(me);
    let mut edge = to_blast(w, me);
    if let (Some(&(x0, y0)), Some(crate::fighter::body::PlatformRef::Spawned(..))) =
        (before.at.get(slot), crate::fighter::body::floor_actor(w, me))
    {
        let (ax, ay) = (x.body.x + (x.body.x - x0) * RIDE_AHEAD, x.body.y + (x.body.y - y0) * RIDE_AHEAD);
        edge = math::min(edge, to_blast_at(w, ax, ay));
    }
    let mut s = -EDGE * math::max(0.0, 1.0 - edge / EDGE_RANGE);
    if super::plan::off_stage(w, me) {
        let g = &w.stage.ground;
        let out = math::max(0.0, math::max(math::narrow(g.left) - x.body.x, x.body.x - math::narrow(g.right)));
        let below = math::max(0.0, math::narrow(g.top) - x.body.y);
        s -= OFF_X * out + OFF_BELOW * below;
        if x.jump.jumps < 2 {
            s += AIR_JUMP;
        }
        if x.air.air_evades == 0 {
            s += AIR_DODGE;
        }
    }
    if let Some(e) = super::plan::target(w, me) {
        s -= APART * (w.fighter(e).body.x - x.body.x).abs();
    }
    s
}

/// The rollout's score for `me` (in slot `slot`): the others' standing (another team's counts for the bot, a
/// teammate's not at all) less its own, and its place.
pub fn score(w: &World, before: &Before, slot: usize) -> Real {
    let me = w.players[slot];
    let team = w.fighter(me).team.team;
    let mut s = 0.0;
    for (i, &f) in w.players.iter().enumerate() {
        if i == slot {
            s -= standing(w, before, i, f);
        } else if w.fighter(f).team.team != team {
            s += standing(w, before, i, f);
        }
    }
    s + place(w, before, slot, me)
}
