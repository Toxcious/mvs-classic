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
//! - it takes back what three of them leave on the pawn past their buff's end: Low Gravity's OverrideGravity, Extra
//!   Air Special's AirSpecialMax + 1 and the Ice Gauntlets mutator's gauntlet buff;
//! - it runs Random Items' spawner for a window alone, at a quicker rate of its own and with no centre item, then
//!   stops it (the items already out stay).
//!
//! Left as the build leaves them at a ring-out: Grey Health's pack (its 25 grey health stays until it is hit off),
//! a Weakened or an Ice that Weaken on Hit or Charge Freeze gave (each runs out by itself), and the Giant buff a
//! fighter's spawned actor took (`combat::elem::grant_spawned_buffs`: a projectile thrown as a Giant stays big).

use crate::Real;
use crate::State;
use crate::combat::buffs::{self, BuffRef};
use crate::events::Event;
use crate::fighter::FighterId;
use crate::math;
use crate::names::Name;
use crate::rng::Rng;
use crate::setup::{MatchSetup, Wheel};
use crate::world::{World, flow, items, mutators};

/// A Random Items window's first item, in seconds into the window. The build's spawner gives its first 12.5 s after
/// it starts, one item a 20 s window; the rule's own rate is quicker.
pub const ITEM_FIRST: Real = 2.0;
/// A Random Items window's next item, in seconds after the one before.
pub const ITEM_EVERY: Real = 3.0;

/// The classes whose end needs the rule's own tail (`end_window`), and the one world mutator the rule runs.
const LOW_GRAVITY: &str = "Mutator_LowGravity_C";
const AIR_SPECIAL: &str = "Mutator_Boost_AdditionalAirSpecial_C";
const ICE_GAUNTLETS: &str = "Mutator_Boost_IceGauntlets_C";
const RANDOM_ITEMS: &str = "Mutator_RandomItemSpawns_C";

/// What a fighter held before the window's mutator came on, of what the rule puts back at its end.
#[derive(State, Debug, Default)]
pub struct WheelSaved {
    /// Saved this window (at the change, or at the respawn inside it).
    pub held: bool,
    /// `air.air_special_max_set`.
    pub air: Option<Real>,
    /// `moves.gravity_override`.
    pub gravity: bool,
    /// A live Buff_SupermanAllyIceGauntlet_C (a Superman team mate's pellet gave it).
    pub gauntlet: bool,
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
    /// The match is over: the rule has stopped.
    pub over: bool,
    /// The rule's Random Items window runs the item area's spawner.
    pub items: bool,
    /// By player slot.
    pub saved: Vec<WheelSaved>,
}

/// What the app and the HUD read of the rule each frame (`status`). The indexes are into the set-up's pool
/// (`MatchSetup::wheel`).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct WheelStatus {
    /// The mutator that is on, or None in the plain first window.
    pub current: Option<usize>,
    /// The next mutator, from the tick it is drawn (the set-up's `warn` ahead of the change) to the change.
    pub next: Option<usize>,
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
        over: false,
        items: false,
        saved: setup.players.iter().map(|_| WheelSaved::default()).collect(),
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
    let Some(cls) = class_of(w, i) else { return false };
    if w.setup.mutators.iter().any(|m| m == &wheel(w).pool[i]) {
        return false;
    }
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
    cls == RANDOM_ITEMS && !flow::ffa_items_on(&w.setup) && !w.data.spawn_areas.is_empty() && w.item_area.is_some()
}

/// The draw: one of the pool's runnable entries but the one on, from the rule's stream; -1 when there is none.
fn draw(w: &mut World) -> i32 {
    let current = index(state(w).current);
    let picks: Vec<usize> = (0..wheel(w).pool.len()).filter(|&i| Some(i) != current && runnable(w, i)).collect();
    if picks.is_empty() {
        return -1;
    }
    let st = state_mut(w);
    let mut rng = Rng::new(st.rng);
    let n = crate::real(u32::try_from(picks.len()).expect("a short pool"));
    let k = usize::try_from(math::to_int32(rng.int(n))).expect("a draw below the bound");
    st.rng = rng.s;
    i32::try_from(picks[k]).expect("a short pool")
}

/// The clock has started: BPI_SetMatchStarted in a match with its flow, the first step in one without.
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
        return;
    }
    let window = window_ticks(w);
    let warn = ticks(wheel(w).warn).min(window);
    if state(w).next < 0 && tick >= state(w).change_at - warn {
        let next = draw(w);
        state_mut(w).next = next;
        if next >= 0 {
            w.events.push(Event::WheelNext { index: next });
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

/// The current window's class, if a mutator is on.
fn current_class(w: &World) -> Option<Name> {
    let i = index(w.wheel.as_ref()?.current)?;
    class_of(w, i).map(|c| w.lit(c))
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
        let saved = WheelSaved {
            held: true,
            air: fr.air.air_special_max_set,
            gravity: fr.moves.gravity_override,
            gauntlet: live_gauntlet(w, f).is_some(),
        };
        state_mut(w).saved[s] = saved;
    }
    buffs::spawn_buff(w, f, cls, 0.0, Some(0), Some(f), true, None);
}

/// A window's start: its mutator on every live fighter, or Random Items' spawner started.
fn begin_window(w: &mut World) {
    let Some(cls) = current_class(w) else { return };
    if is_world_class(w, cls) {
        start_items(w);
        return;
    }
    for f in live_players(w) {
        give(w, f, cls);
    }
}

/// A window's end: its mutator off every fighter (`buffs::remove_buff`: K2_DestroyActor, its EndPlay and the state
/// manager's end pass), then the rule's own tail; or Random Items' spawner stopped.
fn end_window(w: &mut World) {
    let Some(cls) = current_class(w) else { return };
    if is_world_class(w, cls) {
        stop_items(w);
        return;
    }
    let name = w.names.str(cls).to_owned();
    for (s, f) in w.players.clone().into_iter().enumerate() {
        buffs::remove_buff(w, f, Some(cls), None);
        let saved = std::mem::take(&mut state_mut(w).saved[s]);
        match name.as_str() {
            // OverrideGravity at its BeginPlay has no clear of its own: a ring-out's reset clears it
            // (`match_side::reset_for_respawn`). The rule clears it where the fighter had none before.
            LOW_GRAVITY if saved.held && !saved.gravity => crate::fighter::moves::clear_gravity_override(w, f),
            // AirSpecialMax + 1 at its BeginPlay is never taken back in the build (each life's mutator adds one
            // more); the rule puts the value back.
            AIR_SPECIAL if saved.held => w.fighter_mut(f).air.air_special_max_set = saved.air,
            // The gauntlet the mutator gave (its own SpawnBuffActor with the pawn as the Instigator, no life timer)
            // stays for the life in the build; the rule ends it with the mutator, unless the fighter held one before
            // the window (a team mate Superman's: the mutator's spawn refreshed that one).
            ICE_GAUNTLETS if saved.held && !saved.gauntlet => end_own_gauntlet(w, f),
            _ => {}
        }
    }
}

/// The live Buff_SupermanAllyIceGauntlet_C on `f`. The port keeps no Instigator on it (`FighterBuff::src` is
/// set for a few classes only), so the mutator's and a team mate's are told apart by what the fighter held before.
fn live_gauntlet(w: &World, f: FighterId) -> Option<BuffRef> {
    let cls = crate::name!("Buff_SupermanAllyIceGauntlet_C");
    let b = w.fighter(f).buffs.buffs.iter().find(|b| b.cls == cls && b.frames > 0.0)?;
    Some(BuffRef { fighter: f, serial: b.serial })
}

/// The Ice Gauntlets mutator's gauntlet on `f`, ended.
fn end_own_gauntlet(w: &mut World, f: FighterId) {
    if let Some(b) = live_gauntlet(w, f) {
        buffs::end_buff(w, f, b);
    }
}

/// Random Items' window: the item area's StartTimeSpawnerServer with the mutator's own list
/// (`mutators::items_begin`), its first item `ITEM_FIRST` in; no centre item.
fn start_items(w: &mut World) {
    if w.item_area.is_none() {
        return;
    }
    let list = mutators::random_items_list(w);
    let tick = w.flow.tick;
    items::start_time_spawner_server(w, &list, crate::real(tick));
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
    w.wheel.as_ref().filter(|s| s.items).map(|_| ITEM_EVERY)
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
    begin_window(w);
}

/// `wheelFighterSpawned`: a pawn's spawn (`reset::reset`) or respawn (`flow::respawn`), behind Apply Mutators To
/// Fighter: the window's mutator on it, as the build gives a match's own mutators at each respawn.
pub fn wheel_fighter_spawned(w: &mut World, f: FighterId) {
    let Some(cls) = current_class(w) else { return };
    if !is_world_class(w, cls) {
        give(w, f, cls);
    }
}

/// `wheelReset`: the World's reset made a new item area (a Lab reset: a new level); a Random Items window on gets
/// the rule's spawner again from there.
pub fn wheel_reset(w: &mut World) {
    if w.wheel.as_ref().is_some_and(|s| s.items) {
        start_items(w);
    }
}
