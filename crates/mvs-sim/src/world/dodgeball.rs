//! The port's own Dodgeball rule: a barrier down the middle of the stage keeps each team on its half, a fighter's
//! own attacks spare an opponent, and the items the rule drops on both halves hit when thrown across. Not in the
//! build: no line here ports a Blueprint. The items, their throw and their hits are the build's own.
//!
//! The rule's state is [`DodgeballState`] on `World::dodgeball`, made with the World when the set-up has the rule
//! (`MatchSetup::dodgeball`). Its draws come from a stream of its own, made from the match seed, so they move no
//! other draw of the match and every machine draws the same.
//!
//! What it does, and where:
//! - the barrier (`dodgeball_reset`, at the World's reset after the stage is built): one run-time Floor solid at
//!   x = 0, [`HALF_WIDTH`] each way, from under the lowest blast line to over the highest, with the floor's bits less
//!   the item bit, so an item lying, rolling or in flight passes it and a fighter or its projectile does not; and
//!   each player's respawn spot moved out from the middle to clear it by the fighter's capsule;
//! - the put-back (`put_back`, each tick behind every move of the step): a live fighter whose capsule is past its
//!   half's face of the barrier is placed at that face. The wall alone does not hold: a fighter's move against a
//!   box's side is tested where the half-step ends, not swept, and the fastest knockback steps past it; portals,
//!   recall moves, teleports, pulls and holds place a fighter with no side test at all;
//! - the drops (`dodgeball_latent`, in the world's leftover latent pass): an item of the rule's pool on each half
//!   [`DROP_FIRST`] in, then one a half every [`DROP_EVERY`], none on a half with [`DROP_CAP`] items lying on it.
//!
//! The barrier, the put-back and the drops do nothing in Free For All (no halves) nor in a mode with pawns of its
//! own (`MatchSetup::mode`).

use crate::Real;
use crate::State;
use crate::events::Event;
use crate::fighter::FighterId;
use crate::math;
use crate::names::Name;
use crate::rng::Rng;
use crate::setup::{MatchSetup, TeamStyle};
use crate::world::World;
use crate::world::items;
use crate::world::stage::{self, FLOOR_FLAGS, RuntimeSolidSpec, SOLID_ITEM};

/// The barrier's half width in the play plane (UE cm): 150 thick, more than the fastest knockback's half-step
/// (about 125), and the put-back, not a step past the box's middle, decides which side a fighter ends on.
pub const HALF_WIDTH: Real = 75.0;
/// How far the barrier reaches past the stage's lowest and highest blast lines.
const BARRIER_REACH: Real = 1000.0;
/// The barrier's half depth (UE X), across the play plane.
const BARRIER_DEPTH: Real = 500.0;
/// The barrier's run-time solid number (`RuntimeSolid::spec`): no tutorial's numbering reaches it, and the rule
/// never runs in a tutorial.
pub const BARRIER_SPEC: i32 = 1_000_000;
/// The class name of the barrier's spec, the port's own (the logs and the drawing side).
pub const BARRIER_CLASS: &str = "DodgeballBarrier";
/// The gap a respawn spot leaves between the fighter's capsule and the barrier.
const RESPAWN_GAP: Real = 25.0;

/// The first drop on each half, in seconds from the clock's start.
pub const DROP_FIRST: Real = 3.0;
/// The next drop on each half, in seconds after the one before.
pub const DROP_EVERY: Real = 4.0;
/// No drop on a half with this many items lying on it (every live item not held, whoever made it).
pub const DROP_CAP: usize = 3;
/// How far a drop's point keeps from the barrier's middle: its half width and an item's reach past it.
pub const DROP_CLEAR: Real = 200.0;
/// Item_Dynamite_C joins the pool: its blast reaches through the barrier (to be tried at the play test).
const DYNAMITE: bool = false;

/// The rule's state.
#[derive(State, Debug, Default)]
pub struct DodgeballState {
    /// The rule's own random stream (`rng::Rng`'s state).
    pub rng: i32,
    /// The tick the drops' clock counts from; -1 before the match starts.
    pub from: i32,
    /// The tick of the next drop on both halves; -1 before the match starts.
    pub drop_at: i32,
    /// The match is over: the drops have stopped.
    pub over: bool,
}

/// The rule's state for a World of `setup` with `seed`, None without the rule. The stream's seed is the World's own
/// `seed` through constants no other stream of the match uses (`World::rng`: 31337 and 7; a fighter's: 7919 and
/// 104729; the Chaos Wheel's: 69069 and 1013), so its draws move no other.
pub fn new_state(setup: &MatchSetup, seed: i32) -> Option<DodgeballState> {
    setup.dodgeball?;
    let rng = math::dbl::to_int32(f64::from(seed) * 48271.0 + 2053.0);
    Some(DodgeballState { rng, from: -1, drop_at: -1, over: false })
}

/// What a fighter of `team` deals does not reach `vic`: the rule has halves and `vic` is on another team. The one
/// dealer that reaches an opponent under the rule is an item the stage dropped, thrown by a fighter, and the test
/// that picks its victim does not ask.
pub(crate) fn spares_team(w: &World, team: i32, vic: FighterId) -> bool {
    halves(w) && w.fighter(vic).team.team != team
}

/// [`spares_team`] by the dealing fighter.
pub(crate) fn spares(w: &World, owner: FighterId, vic: FighterId) -> bool {
    halves(w) && w.fighter(vic).team.team != w.fighter(owner).team.team
}

/// The rule has halves: it is on, the match has two teams, and no mode brings pawns of its own.
fn halves(w: &World) -> bool {
    w.setup.dodgeball.is_some() && w.setup.team_style != TeamStyle::FreeForAll && w.setup.mode.is_none()
}

/// The barrier stands: the rule has halves and its set-up asks for the barrier.
fn barrier_on(w: &World) -> bool {
    halves(w) && w.setup.dodgeball.is_some_and(|d| d.barrier)
}

/// The barrier's box in the play plane as left, right, bottom and top, for the drawing side. None without the rule
/// or its barrier.
pub fn barrier(w: &World) -> Option<[Real; 4]> {
    let s = w.runtime_solids.as_ref()?.iter().find(|s| s.spec == BARRIER_SPEC && !s.destroyed)?;
    let g = s.ground();
    Some([math::narrow(g.left), math::narrow(g.right), math::narrow(g.bottom), math::narrow(g.top)])
}

/// The team that plays the left half (negative x): the team of the player whose spawn spot stands furthest left.
/// Every other team plays the right half. The spots go by player index in 1v1 and by team in 2v2, so the halves
/// follow where the fighters start, whatever team number a menu gave them.
fn left_team(w: &World) -> i32 {
    let mut best: Option<(Real, i32)> = None;
    for (i, &f) in w.players.iter().enumerate() {
        let Some(s) = w.stage.spawns.get(i) else { continue };
        if best.is_none_or(|(x, _)| s.x < x) {
            best = Some((s.x, w.fighter(f).team.team));
        }
    }
    best.map_or(0, |(_, t)| t)
}

/// The World's reset, after the stage is built and the fighters spawned: the barrier (once: a
/// later reset keeps it) and each player's respawn spot moved out from the middle until the fighter's capsule clears
/// the barrier by [`RESPAWN_GAP`] (the stages' respawn spots stand 100 to 220 from the middle).
pub fn dodgeball_reset(w: &mut World) {
    if !barrier_on(w) {
        return;
    }
    if barrier(w).is_none() {
        let (blast, outer) = (w.stage.blast, w.stage.blast_outer);
        let bottom = math::min(blast.bottom, outer.bottom) - BARRIER_REACH;
        let top = math::max(blast.top, outer.top) + BARRIER_REACH;
        // (UE axes: the play plane's x is -Y, its y is Z; the box is centred on Y = 0)
        let d = RuntimeSolidSpec {
            cls: BARRIER_CLASS.to_owned(),
            floor: true,
            center: [0.0, 0.0, (bottom + top) / 2.0],
            half_extent: [BARRIER_DEPTH, HALF_WIDTH, (top - bottom) / 2.0],
            rot: None,
            component: Name::NONE,
            flags: Some(FLOOR_FLAGS & !SOLID_ITEM),
        };
        let made = stage::add_runtime_solid(w, BARRIER_SPEC, &d);
        debug_assert!(made, "the barrier's box is a rectangle on the axes");
    }
    let left = left_team(w);
    for i in 0..w.players.len() {
        let f = w.players[i];
        let Some(s) = w.stage.respawns.get(i).copied() else { continue };
        let clear = HALF_WIDTH + crate::fighter::cap_radius(w, f) + RESPAWN_GAP;
        let x = if w.fighter(f).team.team == left { math::min(s.x, -clear) } else { math::max(s.x, clear) };
        w.stage.respawns[i].x = x;
    }
}

/// Each live player whose capsule is past its half's face of the barrier (`left_team` the left half, negative x;
/// every other team the right) is placed with its capsule at that face, its speed across the barrier dropped as a
/// wall stops it (the speed along it stays: a fighter knocked into it slides on). One dead, rung out or waiting for
/// its respawn (`Life::dead`) is left alone: its respawn spot is clear of the barrier.
///
/// Called in `World::step` behind every move of the tick (the CMC moves, the holds' and the buffs' moves, the
/// characters' world ticks, the timers' teleports) and the ring-out test, ahead of the frame's tail, which reads the
/// places (the item reach, the record): the place a frame ends with is on the fighter's half.
pub fn put_back(w: &mut World) {
    if !barrier_on(w) {
        return;
    }
    let left = left_team(w);
    for i in 0..w.players.len() {
        let f = w.players[i];
        if w.fighter(f).life.dead != 0.0 {
            continue;
        }
        let face = HALF_WIDTH + crate::fighter::cap_radius(w, f);
        let fr = w.fighter_mut(f);
        if fr.team.team == left {
            if fr.body.x > -face {
                fr.body.x = -face;
                fr.body.vx = math::min(fr.body.vx, 0.0);
            }
        } else if fr.body.x < face {
            fr.body.x = face;
            fr.body.vx = math::max(fr.body.vx, 0.0);
        }
    }
}

/// A time of the rule in whole game ticks (`Math.round(s * 60)`), at least 1.
fn ticks(s: Real) -> i32 {
    math::to_int32(math::round(s * 60.0)).max(1)
}

fn state_mut(w: &mut World) -> &mut DodgeballState {
    w.dodgeball.as_mut().expect("the rule's state")
}

/// The drops' clock, in the world's leftover latent pass behind the Chaos Wheel's. As the wheel's,
/// it counts from BPI_SetMatchStarted in a match with its flow and from the World's first step in one without, and
/// stops at the match's end. At each drop tick, half 0 then half 1.
pub fn dodgeball_latent(w: &mut World) {
    if !halves(w) || w.dodgeball.as_ref().is_none_or(|s| s.over) {
        return;
    }
    let tick = w.flow.tick;
    if w.dodgeball.as_ref().is_some_and(|s| s.from < 0) {
        if w.setup.flow && !w.flow.match_started {
            return;
        }
        let from = if w.setup.flow { w.flow.match_start_tick } else { tick };
        let st = state_mut(w);
        st.from = from;
        st.drop_at = from + ticks(DROP_FIRST);
    }
    if w.flow.match_over {
        state_mut(w).over = true;
        return;
    }
    if w.dodgeball.as_ref().is_some_and(|s| tick < s.drop_at) {
        return;
    }
    state_mut(w).drop_at += ticks(DROP_EVERY);
    for half in 0..2 {
        drop_on(w, half);
    }
}

/// The items lying on `half` (0 left of x = 0, 1 the rest): live, not held, not pooled, whoever made them.
pub fn lying_on(w: &World, half: usize) -> usize {
    w.actors.items.iter().filter(|(_, i)| !i.dead && !i.held && !i.pooled && usize::from(i.x >= 0.0) == half).count()
}

/// The rule's pool (classes of `ItemsData.actors`): the throwables that hurt, and the Dynamite behind [`DYNAMITE`].
pub fn pool() -> Vec<Name> {
    let mut out = vec![
        crate::name!("Item_Rock_C"),
        crate::name!("Item_Pie_C"),
        crate::name!("Item_DrumStick_C"),
        crate::name!("Item_Snowball_C"),
        crate::name!("Item_SuperSnowball_C"),
    ];
    if DYNAMITE {
        out.push(crate::name!("Item_Dynamite_C"));
    }
    out
}

/// One drop on `half` unless it holds [`DROP_CAP`] items: a class of the pool and a point of the stage's item area
/// (MapRandomActorSpawnArea's box) cut to the half and kept [`DROP_CLEAR`] from the middle, drawn from the rule's
/// stream (the class, then x, then the height); then the area's own spawn of it (`items::spawn_area_item`: no owner,
/// no spawner, so a stage item), with its PS_ItemSpawn effect.
// Deviation: the area's spawn shows the effect VFXToSpawnDelay (0.4 s) ahead of the item; the rule's shows both in
// the same tick.
fn drop_on(w: &mut World, half: usize) {
    if lying_on(w, half) >= DROP_CAP {
        return;
    }
    // (a stage with no item area has no drops; `actor_of_class` would warn at each one)
    if w.data.spawn_areas.is_empty() {
        return;
    }
    let Some(area) = items::actor_of_class(w) else { return };
    // (the play plane's x is -UE Y)
    let lo = 0.0 - (area.origin[1] + area.extent[1]);
    let hi = 0.0 - (area.origin[1] - area.extent[1]);
    let (lo, hi) = if half == 0 { (lo, math::min(hi, -DROP_CLEAR)) } else { (math::max(lo, DROP_CLEAR), hi) };
    if lo >= hi {
        return;
    }
    let (zlo, zhi) = (area.origin[2] - area.extent[2], area.origin[2] + area.extent[2]);
    let classes = pool();
    let n = crate::real(u32::try_from(classes.len()).expect("a short pool"));
    let st = state_mut(w);
    let mut rng = Rng::new(st.rng);
    let k = usize::try_from(math::to_int32(rng.int(n))).expect("a draw below the bound");
    let x = lo + ((hi - lo) * rng.next());
    let y = zlo + ((zhi - zlo) * rng.next());
    st.rng = rng.s;
    let cls = classes[k];
    let Some(src) = items::area_item_src(w, cls) else {
        let msg =
            format!("Dodgeball: item class {} is not in the items part or not ported (skipped)", w.names.str(cls));
        crate::fighter::common::warn_text(w, &msg);
        return;
    };
    let (fx, vfx) = (crate::name!("MapRandomActorSpawnArea_C"), crate::name!("PS_ItemSpawn"));
    w.events.push(Event::ItemFx { cls: fx, item: -1, fighter: -1, vfx, x, y, scale: 2.0 });
    items::spawn_area_item(w, cls, src, x, y);
}
