//! The port's own Chaos Wheel rule: the match plays one window plain, then at each window's
//! start one mutator of the set-up's pool comes on for every fighter and the one before it ends. Not in the build: no
//! line here ports a Blueprint. Every effect is one of the build's mutators, spawned and ended as a buff by the
//! build's own code (`world::mutators`, `combat::buffs`); only when one starts and ends is the rule's.
//!
//! The rule's state is [`WheelState`] on `World::wheel`, made with the World when the set-up has a wheel
//! (`MatchSetup::wheel`). Its draws come from a stream of its own, made from the match seed, so they move no other
//! draw of the match and every machine rolls the same.
//!
//! The clock: in a match with its flow (`MatchSetup::flow`) it counts from BPI_SetMatchStarted
//! (`flow::start_match`); a match with no flow never starts (the Lab, TrainGameMode, and the trace matches without
//! one), so there it counts from the World's first step. It stops at the match's end (`MatchState::match_over`):
//! the mutator on then stays on.
//!
//! What the build never does to a mutator, and the rule does:
//! - it ends a fighter mutator in the middle of a life (`buffs::remove_buff`, the build's destroy path, whose end
//!   pass puts back what the state manager computes from the live buffs: size, head size, cooldowns, jumps, moves);
//! - it takes back what two of them leave on the pawn past their buff's end: Low Gravity's OverrideGravity and Air
//!   Show's AirSpecialMax;
//! - it runs Random Items' spawner for a window alone, at a quicker rate of its own and with no centre item, then
//!   stops it (the items already out stay); Bomb Rain and Projectile Party (`own_rules`) run it the same way with
//!   a share of its list;
//! - it plays Final Point Shrink's shrink (Shrinking Arena) over half a window whenever it comes, and puts the blast
//!   box back at the window's end;
//! - it takes back Instant Kill's (Sudden Death's) 200 damage at the window's end.
//!
//! The port's own rules (`own_rules`, the `Chaos_*` keys) are no buffs: the sim reads whether one is on where it
//! counts (`window_has`).
//!
//! Left as the build leaves them at a ring-out: Shields Up's pack (its grey health stays until it is hit off), an
//! Ice that Ice Charge or Freeze Touch gave (it runs out by itself), and the Giant buff a fighter's spawned actor
//! took (`combat::elem::grant_spawned_buffs`: a projectile thrown as a Giant stays big).

use crate::Real;
use crate::State;
use crate::combat::buffs;
use crate::events::Event;
use crate::fighter::FighterId;
use crate::math;
use crate::names::Name;
use crate::rng::Rng;
use crate::setup::{MatchSetup, Wheel};
use crate::world::own_rules::{self, Rule};
use crate::world::wheel_events::{self, WheelEvent};
use crate::world::{World, blast, flow, items, mutators};

/// A Random Items window's first item, in seconds into the window. The build's spawner gives its first 12.5 s after
/// it starts, one item a 20 s window; the rule's own rate is quicker.
pub const ITEM_FIRST: Real = 2.0;
/// A Random Items window's next item, in seconds after the one before.
pub const ITEM_EVERY: Real = 3.0;

/// The classes whose end needs the rule's own tail (`end_window`), and the one world mutator the rule runs.
const LOW_GRAVITY: &str = "Mutator_LowGravity_C";
const AIR_SPECIAL: &str = "Mutator_Boost_AdditionalAirSpecial_C";
const RANDOM_ITEMS: &str = "Mutator_RandomItemSpawns_C";
const SHRINK: &str = "Mutator_ShrinkBlastBoxLastPoint_C";
const INSTANT_KILL: &str = "Mutator_InstantKill_C";
/// Mutator_InstantKill's StartingDamage (its class default), taken back at the window's end.
const SUDDEN_DEATH_DAMAGE: Real = 200.0;
/// Shrinking Arena's shrink, a share of the window: it closes in over half, then holds.
const SHRINK_SHARE: Real = 0.5;

/// What a fighter held before the window's mutator came on, of what the rule puts back at its end.
#[derive(State, Debug, Default)]
pub struct WheelSaved {
    /// Saved this window (at the change, or at the respawn inside it).
    pub held: bool,
    /// `air.air_special_max_set`.
    pub air: Option<Real>,
    /// `moves.gravity_override`.
    pub gravity: bool,
}

/// The rule's state. Its fields are the sim track's.
#[derive(State, Debug, Default)]
pub struct WheelState {
    /// The rule's own random stream (`rng::Rng`'s state).
    pub rng: i32,
    /// The tick the clock counts from; -1 before the match starts.
    pub from: i32,
    /// The tick of the next change; -1 before the match starts.
    pub change_at: i32,
    /// The pool index of the mutator on, -1 for none.
    pub current: i32,
    /// The pool index of the next one once drawn, -1 for none.
    pub next: i32,
    /// Two at once (`Wheel::two`): the pool index of the second mutator on, -1 for none.
    pub also: i32,
    /// The second of the next change, drawn with `next` once the match is late, -1 for none.
    pub also_next: i32,
    /// The draw's bag: the pool indexes not drawn yet this round (`draw`), refilled with the whole pool once none
    /// of it may be drawn.
    pub bag: Vec<i32>,
    /// The last draws, newest last, at most `HELD_BACK`: held back from the next draws across a refill.
    pub recent: Vec<i32>,
    /// The match is over: the rule has stopped.
    pub over: bool,
    /// The rule's Random Items window runs the item area's spawner.
    pub items: bool,
    /// By player slot.
    pub saved: Vec<WheelSaved>,
    /// The events' (`wheel_events`): Underdog's player slot, Hot Potato's holder and the slot that passed it to it,
    /// -1 for none.
    pub underdog: i32,
    pub potato: i32,
    pub potato_by: i32,
    /// Body Swap: by player slot, the slot whose fighter it drives (empty: off), and each slot's own team from before
    /// the swap. `swap_kept`: a player ran out of lives while swapped, so the swap stays to the match's end.
    pub swap: Vec<i32>,
    pub swap_teams: Vec<i32>,
    pub swap_kept: bool,
}

/// What the app and the HUD read of the rule each frame (`status`). The indexes are into the set-up's pool
/// (`MatchSetup::wheel`).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct WheelStatus {
    /// The mutator that is on, or None in the plain first window.
    pub current: Option<usize>,
    /// The next mutator, from the tick it is drawn (the set-up's `warn` ahead of the change) to the change.
    pub next: Option<usize>,
    /// Two at once: the second mutator on, and the second of the next change.
    pub also: Option<usize>,
    pub also_next: Option<usize>,
    /// Ticks to the next change; None before the match starts and once it is over.
    pub ticks_left: Option<u32>,
    /// A window's length in ticks.
    pub window_ticks: u32,
}

/// A time of the set-up in whole game ticks (`Math.round(s * 60)`), at least 1.
fn ticks(s: Real) -> i32 {
    math::to_int32(math::round(s * 60.0)).max(1)
}

fn wheel(w: &World) -> &Wheel {
    w.setup.wheel.as_ref().expect("a match with a wheel")
}

fn window_ticks(w: &World) -> i32 {
    ticks(wheel(w).seconds)
}

fn state(w: &World) -> &WheelState {
    w.wheel.as_ref().expect("the wheel's state")
}

fn state_mut(w: &mut World) -> &mut WheelState {
    w.wheel.as_mut().expect("the wheel's state")
}

fn index(i: i32) -> Option<usize> {
    usize::try_from(i).ok()
}

/// The rule's state for a World of `setup` with `seed`, None without a wheel. The stream's seed is the World's
/// own `seed` through constants no other stream of the match uses (`World::rng`: 31337 and 7; a fighter's: 7919 and
/// 104729), so its draws move no other.
pub fn new_state(setup: &MatchSetup, seed: i32) -> Option<WheelState> {
    setup.wheel.as_ref()?;
    let rng = math::dbl::to_int32(f64::from(seed) * 69069.0 + 1013.0);
    Some(WheelState {
        rng,
        from: -1,
        change_at: -1,
        current: -1,
        next: -1,
        also: -1,
        also_next: -1,
        bag: Vec::new(),
        recent: Vec::new(),
        over: false,
        items: false,
        saved: setup.players.iter().map(|_| WheelSaved::default()).collect(),
        underdog: -1,
        potato: -1,
        potato_by: -1,
        swap: Vec::new(),
        swap_teams: Vec::new(),
        swap_kept: false,
    })
}

/// The rule as the app and the HUD read it, or None in a World with no wheel.
pub fn status(w: &World) -> Option<WheelStatus> {
    let st = w.wheel.as_ref()?;
    let window = window_ticks(w);
    let ticks_left = if st.from < 0 || st.over { None } else { u32::try_from(st.change_at - w.flow.tick).ok() };
    Some(WheelStatus {
        current: index(st.current),
        next: index(st.next),
        also: index(st.also),
        also_next: index(st.also_next),
        ticks_left,
        window_ticks: u32::try_from(window).expect("a positive window"),
    })
}

/// The class of a pool entry (MutatorData_Map's MutatorMap), None where the data has none.
fn class_of(w: &World, i: usize) -> Option<&str> {
    let key = wheel(w).pool.get(i)?;
    flow::mutator_data(w)?.map.get(key).map(String::as_str).filter(|c| !c.is_empty())
}

/// Whether the draw may give pool entry `i`: a class the port runs, not given by the match's own mutators (the
/// set-up's list, the Silly selector's pick, a world mutator the handler holds), and for Random Items a stage with
/// an item area and no spawner of the mode's own (FreeForAll with rules: FFAGameMode's runs all match; the Lab's
/// FreeForAll has none, so it may roll there).
fn runnable(w: &World, i: usize) -> bool {
    if w.setup.mutators.iter().any(|m| m == &wheel(w).pool[i]) {
        return false;
    }
    if let Some(r) = Rule::of_key(&wheel(w).pool[i]) {
        return r.items().is_none() || items_runnable(w);
    }
    if let Some(e) = WheelEvent::of_key(&wheel(w).pool[i]) {
        return wheel_events::runnable(w, e);
    }
    let Some(cls) = class_of(w, i) else { return false };
    if mutators::match_fighter_classes(w).iter().any(|c| c == cls) {
        return false;
    }
    if w.world_mutators.as_ref().is_some_and(|h| h.buffs.iter().any(|b| w.names.str(b.cls) == cls)) {
        return false;
    }
    let world = flow::mutator_data(w).is_some_and(|m| m.world.iter().any(|c| c == cls));
    if !world {
        return true;
    }
    match cls {
        RANDOM_ITEMS => items_runnable(w),
        // (Free For All shrinks the box itself; a stage with no shrunk box has nothing to shrink to)
        SHRINK => w.setup.team_style != crate::setup::TeamStyle::FreeForAll && w.stage.shrunk.is_some(),
        _ => false,
    }
}

/// Whether a window may run the item area's spawner: a stage with an item area and no spawner of the mode's own.
fn items_runnable(w: &World) -> bool {
    !flow::ffa_items_on(&w.setup) && !w.data.spawn_areas.is_empty() && w.item_area.is_some()
}

/// Whether pool entry `i` runs the item spawner (Random Items, Bomb Rain, Projectile Party): two of them are never
/// on at once.
fn runs_items(w: &World, i: usize) -> bool {
    class_of(w, i) == Some(RANDOM_ITEMS) || Rule::of_key(&wheel(w).pool[i]).is_some_and(|r| r.items().is_some())
}

/// Whether the window on has `key` (the mutator on or, two at once, the second): the port's own rules' test
/// (`own_rules::on`).
pub fn window_has(w: &World, key: &str) -> bool {
    let Some(st) = w.wheel.as_ref() else { return false };
    let Some(pool) = w.setup.wheel.as_ref().map(|x| &x.pool) else { return false };
    [st.current, st.also].into_iter().filter_map(index).any(|i| pool.get(i).is_some_and(|k| k == key))
}

/// The window `key` is on in: its start tick and its length in ticks, None while the wheel does not have it on.
pub fn window_of(w: &World, key: &str) -> Option<(i32, i32)> {
    if !window_has(w, key) || state(w).change_at < 0 {
        return None;
    }
    let len = window_ticks(w);
    Some((state(w).change_at - len, len))
}

/// The most draws held back from the next ones: a third of the pool, at most 3.
const HELD_BACK: usize = 3;

/// Whether pool entry `i` is a Chaos mutator (`mvs_mutators::Tier::Chaos`).
fn is_chaos(w: &World, i: usize) -> bool {
    mvs_mutators::tier_of(&wheel(w).pool[i]) == mvs_mutators::Tier::Chaos
}

fn as_index(i: usize) -> i32 {
    i32::try_from(i).expect("a short pool")
}

/// A new round of the bag: every entry of the pool but the Chaos ones, and of those as many as the set-up's rate
/// keeps (`mvs_mutators::ChaosRate::of`), picked from the rule's stream (no draw when it keeps all or none).
fn new_round(w: &mut World) -> Vec<i32> {
    let n = wheel(w).pool.len();
    let mut chaos: Vec<i32> = (0..n).filter(|&i| is_chaos(w, i)).map(as_index).collect();
    let keep = wheel(w).chaos.of(chaos.len());
    let mut bag: Vec<i32> = (0..n).filter(|&i| !is_chaos(w, i)).map(as_index).collect();
    if keep > 0 && keep < chaos.len() {
        let st = state_mut(w);
        let mut rng = Rng::new(st.rng);
        for _ in 0..keep {
            let count = crate::real(u32::try_from(chaos.len()).expect("a short pool"));
            let k = usize::try_from(math::to_int32(rng.int(count))).expect("a draw below the bound");
            bag.push(chaos.remove(k));
        }
        st.rng = rng.s;
    } else if keep > 0 {
        bag.append(&mut chaos);
    }
    bag.sort_unstable();
    bag
}

/// The draw, a shuffle bag rather than a free pick, so a match goes through the whole pool before a mutator comes
/// back and none comes back soon after it was on: one of the bag's runnable entries, but the one on and the last
/// few draws (`HELD_BACK`, a third of the pool at most), from the rule's stream. A bag with none to give is refilled
/// with a new round (`new_round`: the pool, Chaos mutators at the set-up's rate); the last draws are held back across
/// the refill as long as some other entry may be drawn, then only the one on. -1 when no entry but the one on may be
/// drawn. A Chaos mutator is never drawn at the rate Off. `other`, the change's first draw when this is its second,
/// is never given either.
fn draw(w: &mut World, other: i32) -> i32 {
    let n = wheel(w).pool.len();
    let (current, also) = (state(w).current, state(w).also);
    let held = HELD_BACK.min(n / 3);
    let off = wheel(w).chaos == mvs_mutators::ChaosRate::Off;
    let runnable: Vec<i32> = (0..n)
        .filter(|&i| runnable(w, i) && !(off && is_chaos(w, i)))
        .map(as_index)
        .filter(|&i| i != current && i != also && i != other)
        .filter(|&i| {
            other < 0
                || !(runs_items(w, usize::try_from(i).expect("an index"))
                    && runs_items(w, usize::try_from(other).expect("an index")))
        })
        .collect();
    let fresh = |st: &WheelState, from: &[i32]| -> Vec<i32> {
        let recent = &st.recent[st.recent.len().saturating_sub(held)..];
        runnable.iter().copied().filter(|i| from.contains(i) && !recent.contains(i)).collect()
    };
    let mut picks = fresh(state(w), &state(w).bag);
    if picks.is_empty() {
        let bag = new_round(w);
        picks = fresh(state(w), &bag);
        if picks.is_empty() {
            picks.clone_from(&runnable);
        }
        state_mut(w).bag = bag;
    }
    if picks.is_empty() {
        return -1;
    }
    let st = state_mut(w);
    let mut rng = Rng::new(st.rng);
    let count = crate::real(u32::try_from(picks.len()).expect("a short pool"));
    let k = usize::try_from(math::to_int32(rng.int(count))).expect("a draw below the bound");
    st.rng = rng.s;
    let pick = picks[k];
    st.bag.retain(|&i| i != pick);
    st.recent.push(pick);
    if st.recent.len() > HELD_BACK {
        st.recent.remove(0);
    }
    pick
}

/// The seconds left of a timed match from which two at once comes on.
pub const LATE_SECONDS: Real = 60.0;

/// Whether the match is late for two at once: its last `LATE_SECONDS` (a timed match), or one ring-out from its end
/// (under stocks two teams left and one on its last life, `flow::stock_match_point`; on points a team one short
/// of the win).
fn late(w: &World) -> bool {
    let Some(r) = w.setup.rules else { return false };
    if r.duration > 0.0 && w.flow.match_started && w.round_time_remaining() <= LATE_SECONDS {
        return true;
    }
    if r.stocks {
        return flow::stock_match_point(w);
    }
    r.knockouts_to_win > 0.0 && w.flow.team_knockouts.contains(&(r.knockouts_to_win - 1.0))
}

// The clock has started: BPI_SetMatchStarted in a match with its flow, the first step in one without.
fn started(w: &World) -> bool {
    !w.setup.flow || w.flow.match_started
}

/// `wheelLatent`: the rule's tick, in the world's leftover latent pass behind the world mutators'
/// (`flow::latent`): the clock's start, the draw `warn` ahead of each change, the change, and the stop at the match's
/// end.
pub fn wheel_latent(w: &mut World) {
    if w.wheel.is_none() || state(w).over {
        return;
    }
    wheel_events::latent(w);
    let tick = w.flow.tick;
    if state(w).from < 0 {
        if !started(w) {
            return;
        }
        let from = if w.setup.flow { w.flow.match_start_tick } else { tick };
        let window = window_ticks(w);
        let st = state_mut(w);
        st.from = from;
        st.change_at = from + window;
    }
    if w.flow.match_over {
        state_mut(w).over = true;
        // (the results read each player's own fighter: its team back)
        wheel_events::swap_back(w);
        return;
    }
    let window = window_ticks(w);
    let warn = ticks(wheel(w).warn).min(window);
    if state(w).next < 0 && tick >= state(w).change_at - warn {
        let next = draw(w, -1);
        state_mut(w).next = next;
        if next >= 0 {
            w.events.push(Event::WheelNext { index: next });
            if wheel(w).two && late(w) {
                let also = draw(w, next);
                state_mut(w).also_next = also;
            }
        }
    }
    if tick < state(w).change_at {
        return;
    }
    let next = state(w).next;
    state_mut(w).change_at += window;
    if next < 0 {
        return; // nothing to roll: what is on stays on
    }
    end_window(w);
    let st = state_mut(w);
    st.current = next;
    st.next = -1;
    st.also = st.also_next;
    st.also_next = -1;
    begin_window(w);
    w.events.push(Event::WheelChange { index: next });
}

/// The fighters the change reaches: the players alive (one out of play gets the window's mutator at its respawn).
fn live_players(w: &World) -> Vec<FighterId> {
    w.players.iter().copied().filter(|&f| w.fighter(f).life.dead <= 0.0).collect()
}

fn slot(w: &World, f: FighterId) -> Option<usize> {
    w.players.iter().position(|&p| p == f)
}

/// The current window's classes: the mutator on and, two at once, the second.
fn on_classes(w: &World) -> Vec<Name> {
    let Some(st) = w.wheel.as_ref() else { return Vec::new() };
    [st.current, st.also].into_iter().filter_map(index).filter_map(|i| class_of(w, i)).map(|c| w.lit(c)).collect()
}

fn is_world_class(w: &World, cls: Name) -> bool {
    let s = w.names.str(cls);
    flow::mutator_data(w).is_some_and(|m| m.world.iter().any(|c| c == s))
}

/// The window's mutator on one fighter, as Apply Mutators To Fighter gives a match's own
/// (`mutators::apply_mutators_to_fighter`: SpawnBuffActor(class, pawn, pawn, 0 stacks, 0.0, true, false)), what the
/// rule puts back at the end saved first. A respawn inside the window keeps what was saved at the change, and Extra
/// Air Special's max goes back to it before the new life's mutator adds its one (the build's own respawn would add a
/// second: 4 in the second life).
fn give(w: &mut World, f: FighterId, cls: Name) {
    let Some(s) = slot(w, f) else { return };
    if state(w).saved[s].held {
        if w.names.str(cls) == AIR_SPECIAL {
            let air = state(w).saved[s].air;
            w.fighter_mut(f).air.air_special_max_set = air;
        }
    } else {
        let fr = w.fighter(f);
        let saved = WheelSaved { held: true, air: fr.air.air_special_max_set, gravity: fr.moves.gravity_override };
        state_mut(w).saved[s] = saved;
    }
    buffs::spawn_buff(w, f, cls, 0.0, Some(0), Some(f), true, None);
}

/// The window's pool entries on: the mutator and, two at once, the second.
fn on_indexes(w: &World) -> Vec<usize> {
    let Some(st) = w.wheel.as_ref() else { return Vec::new() };
    [st.current, st.also].into_iter().filter_map(index).collect()
}

/// The spawner's list of a pool entry that runs it: Random Items' own, or the share of it an own rule keeps.
fn items_list(w: &World, i: usize) -> Option<Vec<Name>> {
    let all = mutators::random_items_list(w);
    if class_of(w, i) == Some(RANDOM_ITEMS) {
        return Some(all);
    }
    let keep = Rule::of_key(&wheel(w).pool[i])?.items()?;
    Some(all.into_iter().filter(|&n| keep.contains(&w.names.str(n))).collect())
}

/// A window's start: each of its fighter mutators on every live fighter, the item spawner started for one that runs
/// it, Shrinking Arena's shrink started. An own rule with no spawner needs nothing: the sim reads it.
fn begin_window(w: &mut World) {
    for i in on_indexes(w) {
        if let Some(e) = WheelEvent::of_key(&wheel(w).pool[i]) {
            wheel_events::begin(w, e);
            continue;
        }
        if let Some(list) = items_list(w, i) {
            start_items(w, &list);
            continue;
        }
        if class_of(w, i) == Some(SHRINK) {
            start_shrink(w);
            continue;
        }
        let Some(cls) = class_of(w, i).map(|c| w.lit(c)) else { continue };
        if is_world_class(w, cls) {
            continue;
        }
        for f in live_players(w) {
            give(w, f, cls);
        }
    }
}

/// A window's end: each of its mutators off every fighter (`buffs::remove_buff`: K2_DestroyActor, its EndPlay and
/// the state manager's end pass), then the rule's own tail; or Random Items' spawner stopped.
fn end_window(w: &mut World) {
    wheel_events::end(w);
    for i in on_indexes(w) {
        if runs_items(w, i) {
            stop_items(w);
        } else if class_of(w, i) == Some(SHRINK) {
            stop_shrink(w);
        }
    }
    let fighter: Vec<Name> = on_classes(w).into_iter().filter(|&c| !is_world_class(w, c)).collect();
    if fighter.is_empty() {
        return;
    }
    for (s, f) in w.players.clone().into_iter().enumerate() {
        for &cls in &fighter {
            buffs::remove_buff(w, f, Some(cls), None);
        }
        let saved = std::mem::take(&mut state_mut(w).saved[s]);
        for &cls in &fighter {
            tail(w, f, cls, &saved);
        }
    }
}

/// The rule's own tail at a window's end for one fighter mutator `cls`, from what the fighter held before.
fn tail(w: &mut World, f: FighterId, cls: Name, saved: &WheelSaved) {
    match w.names.str(cls) {
        // OverrideGravity at its BeginPlay has no clear of its own: a ring-out's reset clears it
        // (`match_side::reset_for_respawn`). The rule clears it where the fighter had none before.
        LOW_GRAVITY if saved.held && !saved.gravity => crate::fighter::moves::clear_gravity_override(w, f),
        // Air Show's AirSpecialMax at its BeginPlay is never taken back in the build (the build's mutator adds one
        // more each life); the rule puts the value back.
        AIR_SPECIAL if saved.held => w.fighter_mut(f).air.air_special_max_set = saved.air,
        // Sudden Death's 200 is the window's alone: a fighter alive at its end gets it back off (a hit taken in the
        // window stays)
        INSTANT_KILL if w.fighter(f).life.dead <= 0.0 => buffs::apply_healing(w, f, SUDDEN_DEATH_DAMAGE),
        _ => {}
    }
}

/// Shrinking Arena: Final Point Shrink's BPI_ShrinkBlastBox (`blast::start_blast_shrink`, by its class, so the
/// camera follows) over `SHRINK_SHARE` of the window.
fn start_shrink(w: &mut World) {
    let time = wheel(w).seconds * SHRINK_SHARE;
    blast::start_blast_shrink(w, time, crate::name!("Mutator_ShrinkBlastBoxLastPoint_C"));
}

/// Shrinking Arena's end: the blast box whole again at once (the camera's reset path: the shrink's tick back at
/// -1), where the rule's shrink is the one on.
fn stop_shrink(w: &mut World) {
    if w.flow.blast_shrink_by != Some(crate::name!("Mutator_ShrinkBlastBoxLastPoint_C")) {
        return;
    }
    w.flow.blast_shrink_tick = -1;
    w.flow.blast_shrink_time = None;
    w.flow.blast_shrink_by = None;
    blast::set_blast_shrink(w, 0.0);
}

/// A spawner window (Random Items, Bomb Rain, Projectile Party): the item area's StartTimeSpawnerServer with
/// `list` (`mutators::items_begin`), its first item `ITEM_FIRST` in; no centre item.
fn start_items(w: &mut World, list: &[Name]) {
    if w.item_area.is_none() || list.is_empty() {
        return;
    }
    let tick = w.flow.tick;
    items::start_time_spawner_server(w, list, crate::real(tick));
    let first = tick + ticks(ITEM_FIRST);
    if let Some(ar) = w.item_area.as_mut() {
        ar.timer_at = first;
    }
    state_mut(w).items = true;
}

/// The window's end: the spawner's timer and a spawn whose effect is out are cleared; the items already out stay.
fn stop_items(w: &mut World) {
    if let Some(ar) = w.item_area.as_mut() {
        ar.timer_at = -1;
        ar.pending_at = -1;
    }
    state_mut(w).items = false;
}

/// The next item's time while the rule's Random Items window runs the spawner (`items::spawn_pending`'s re-arm),
/// None otherwise (the build's own rate).
pub fn item_rate(w: &World) -> Option<Real> {
    w.wheel.as_ref().filter(|s| s.items)?;
    let own = on_indexes(w).into_iter().any(|i| Rule::of_key(&wheel(w).pool[i]).is_some());
    Some(if own { own_rules::ITEMS_EVERY } else { ITEM_EVERY })
}

/// The change to pool entry `index` now (None: back to plain), outside the clock and with no event: the one on ends
/// and `index` comes on as at a window's change. For a test or a tool; the clock's next change goes on from there.
pub fn change_to(w: &mut World, index: Option<usize>) {
    if w.wheel.is_none() {
        return;
    }
    end_window(w);
    let st = state_mut(w);
    st.current = index.map_or(-1, |i| i32::try_from(i).expect("a short pool"));
    st.next = -1;
    st.also = -1;
    st.also_next = -1;
    begin_window(w);
}

/// `wheelFighterSpawned`: a pawn's spawn (`reset::reset`) or respawn (`flow::respawn`), behind Apply Mutators To
/// Fighter: the window's mutator on it, as the build gives a match's own mutators at each respawn.
pub fn wheel_fighter_spawned(w: &mut World, f: FighterId) {
    for cls in on_classes(w) {
        if !is_world_class(w, cls) {
            give(w, f, cls);
        }
    }
}

/// `wheelReset`: the World's reset made a new item area (a Lab reset: a new level); a Random Items window on gets
/// the rule's spawner again from there.
pub fn wheel_reset(w: &mut World) {
    if !w.wheel.as_ref().is_some_and(|s| s.items) {
        return;
    }
    if let Some(list) = on_indexes(w).into_iter().find_map(|i| items_list(w, i)) {
        start_items(w, &list);
    }
}
