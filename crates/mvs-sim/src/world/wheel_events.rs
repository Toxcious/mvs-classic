//! (the port's own) The Chaos Wheel's events, the `Chaos_*` keys of `mvs_mutators::ALL` that act at a window's
//! start (and some at its end) rather than all through it: Damage Swap, Shuffle, Fresh Start, Underdog, Hot Potato
//! and Body Swap. They are the wheel's alone (the Mutators row does not offer them): `wheel::begin_window` and
//! `end_window` call `begin` and `end`. What an event holds over its window is in the wheel's state
//! (`WheelState::underdog`, `potato`, `swap`).

use crate::Real;
use crate::combat::buffs;
use crate::events::{Event, PopKind};
use crate::fighter::FighterId;
use crate::rng::Rng;
use crate::world::World;

/// One of the wheel's events.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WheelEvent {
    /// The player with the most damage trades it with the one with the least.
    DamageSwap,
    /// Every live player is moved to another's place.
    Shuffle,
    /// Every live player's damage is halved.
    FreshStart,
    /// The player with the fewest ring-outs is Giant with Double Damage for the window.
    Underdog,
    /// One player carries a bomb; damage it deals passes it on; its holder at the window's end is blasted.
    HotPotato,
    /// 2v2: teammates trade fighters for the window (each player's input drives the other's).
    BodySwap,
}

impl WheelEvent {
    pub const ALL: [WheelEvent; 6] = [
        WheelEvent::DamageSwap,
        WheelEvent::Shuffle,
        WheelEvent::FreshStart,
        WheelEvent::Underdog,
        WheelEvent::HotPotato,
        WheelEvent::BodySwap,
    ];

    /// The event's key in `mvs_mutators::ALL`.
    pub fn key(self) -> &'static str {
        match self {
            WheelEvent::DamageSwap => "Chaos_DamageSwap",
            WheelEvent::Shuffle => "Chaos_Shuffle",
            WheelEvent::FreshStart => "Chaos_FreshStart",
            WheelEvent::Underdog => "Chaos_Underdog",
            WheelEvent::HotPotato => "Chaos_HotPotato",
            WheelEvent::BodySwap => "Chaos_BodySwap",
        }
    }

    /// The event of a key, None for any other.
    pub fn of_key(key: &str) -> Option<WheelEvent> {
        WheelEvent::ALL.into_iter().find(|e| e.key() == key)
    }
}

/// Fresh Start's share of the damage kept.
pub const FRESH_START: Real = 0.5;
/// Underdog's mutators (MutatorData_Map keys): Giant and Double Damage (Weaken on Hit's rework).
const UNDERDOG: [&str; 2] = ["Mutator_Giant", "Mutator_Boost_WeakenOnHit"];
/// Hot Potato's blast: damage, launch and the pop text's beat over the holder (ticks).
pub const POTATO_DAMAGE: Real = 15.0;
pub const POTATO_KNOCK_BACK: Real = 2600.0;
pub const POTATO_SCALING: Real = 12.0;
pub const POTATO_POP_EVERY: i32 = 60;

/// Whether the event may be drawn in this match: two live players at least; Body Swap with two players of
/// different teams to pair and no swap on.
pub fn runnable(w: &World, e: WheelEvent) -> bool {
    match e {
        WheelEvent::BodySwap => {
            w.wheel.as_ref().is_some_and(|s| s.swap.is_empty() && !s.swap_kept) && swap_map(w).is_some()
        }
        _ => w.players.len() >= 2,
    }
}

fn live_players(w: &World) -> Vec<FighterId> {
    w.players.iter().copied().filter(|&f| w.fighter(f).life.dead <= 0.0).collect()
}

fn slot(w: &World, f: FighterId) -> i32 {
    w.players.iter().position(|&p| p == f).map_or(-1, |s| i32::try_from(s).expect("a slot"))
}

fn player_at(w: &World, s: i32) -> Option<FighterId> {
    usize::try_from(s).ok().and_then(|s| w.players.get(s).copied())
}

/// A draw below `n` from the wheel's own stream.
fn rand(w: &mut World, n: usize) -> usize {
    let st = w.wheel.as_mut().expect("the wheel's state");
    let mut rng = Rng::new(st.rng);
    let k = crate::math::to_int32(rng.int(crate::real(i32::try_from(n).expect("a small count"))));
    st.rng = rng.s;
    usize::try_from(k).expect("a draw below the bound")
}

/// The class of a MutatorData_Map key.
fn class_of(w: &World, key: &str) -> Option<crate::names::Name> {
    let c = crate::world::flow::mutator_data(w)?.map.get(key)?;
    (!c.is_empty()).then(|| w.lit(c))
}

/// An event's start.
pub fn begin(w: &mut World, e: WheelEvent) {
    match e {
        WheelEvent::DamageSwap => damage_swap(w),
        WheelEvent::Shuffle => shuffle(w),
        WheelEvent::FreshStart => {
            for f in live_players(w) {
                let fr = w.fighter_mut(f);
                fr.stats.damage *= FRESH_START;
            }
        }
        WheelEvent::Underdog => underdog(w),
        WheelEvent::HotPotato => {
            let live = live_players(w);
            if live.len() < 2 {
                return;
            }
            let k = rand(w, live.len());
            let s = slot(w, live[k]);
            let st = w.wheel.as_mut().expect("the wheel's state");
            st.potato = s;
            st.potato_by = -1;
            pop_potato(w);
        }
        WheelEvent::BodySwap => body_swap(w),
    }
}

/// An event's end: Underdog's mutators off, Hot Potato's blast, Body Swap's control back.
pub fn end(w: &mut World) {
    let (underdog, potato, by) = {
        let st = w.wheel.as_mut().expect("the wheel's state");
        let out = (st.underdog, st.potato, st.potato_by);
        st.underdog = -1;
        st.potato = -1;
        st.potato_by = -1;
        out
    };
    if let Some(f) = player_at(w, underdog) {
        for key in UNDERDOG {
            if let Some(cls) = class_of(w, key) {
                buffs::remove_buff(w, f, Some(cls), None);
            }
        }
    }
    if let Some(f) = player_at(w, potato).filter(|&f| w.fighter(f).life.dead <= 0.0) {
        blast(w, f, player_at(w, by));
    }
    end_swap(w);
}

/// Damage Swap: the live player with the most damage and the one with the least (the first in slot order on a tie)
/// trade it.
fn damage_swap(w: &mut World) {
    let live = live_players(w);
    let dmg = |f: FighterId| w.fighter(f).stats.damage;
    let (Some(&hi), Some(&lo)) = (
        live.iter().reduce(|a, b| if dmg(*b) > dmg(*a) { b } else { a }),
        live.iter().reduce(|a, b| if dmg(*b) < dmg(*a) { b } else { a }),
    ) else {
        return;
    };
    if hi == lo {
        return;
    }
    let (a, b) = (dmg(hi), dmg(lo));
    w.fighter_mut(hi).stats.damage = b;
    w.fighter_mut(lo).stats.damage = a;
}

/// Shuffle: each live player moved to the place of the one a random step on in slot order (never its own), its
/// feet where the other's were, its motion stopped.
fn shuffle(w: &mut World) {
    use crate::fighter::body::{actor_y, cap_half};
    let live = live_players(w);
    let n = live.len();
    if n < 2 {
        return;
    }
    let step = 1 + rand(w, n - 1);
    let feet: Vec<(Real, Real)> = live.iter().map(|&f| (w.fighter(f).body.x, actor_y(w, f) - cap_half(w, f))).collect();
    for (i, &f) in live.iter().enumerate() {
        let (x, y) = feet[(i + step) % n];
        let ay = y + cap_half(w, f);
        crate::world::spawn_spot::fighter_teleport_to(w, f, x, ay);
        let b = &mut w.fighter_mut(f).body;
        b.vx = 0.0;
        b.vy = 0.0;
    }
}

/// Underdog: the live player with the fewest ring-outs scored (then the most damage, then the first) gets Giant and
/// Double Damage for the window.
fn underdog(w: &mut World) {
    let live = live_players(w);
    let key = |f: FighterId| {
        let s = &w.fighter(f).stats;
        (s.ring_outs, -s.damage)
    };
    let Some(&f) = live.iter().reduce(|a, b| if key(*b) < key(*a) { b } else { a }) else { return };
    let s = slot(w, f);
    w.wheel.as_mut().expect("the wheel's state").underdog = s;
    for k in UNDERDOG {
        if let Some(cls) = class_of(w, k) {
            buffs::spawn_buff(w, f, cls, 0.0, Some(0), Some(f), true, None);
        }
    }
}

/// Hot Potato's pass, behind a damage record: damage its holder deals to another live player hands it the bomb.
pub fn potato_dealt(w: &mut World, f: FighterId, damage: Real, dealer: Option<FighterId>) {
    let Some(st) = w.wheel.as_ref() else { return };
    if st.potato < 0 || damage <= 0.0 {
        return;
    }
    let Some(d) = dealer.filter(|&d| d != f && slot(w, d) == st.potato) else { return };
    let to = slot(w, f);
    if to < 0 {
        return;
    }
    let by = slot(w, d);
    let st = w.wheel.as_mut().expect("the wheel's state");
    st.potato = to;
    st.potato_by = by;
    pop_potato(w);
}

/// The wheel's tick for the events: the holder's pop text once each `POTATO_POP_EVERY`.
pub fn latent(w: &mut World) {
    if w.wheel.as_ref().is_some_and(|s| s.potato >= 0) && w.flow.tick.rem_euclid(POTATO_POP_EVERY) == 0 {
        pop_potato(w);
    }
}

fn pop_potato(w: &mut World) {
    let s = w.wheel.as_ref().map_or(-1, |s| s.potato);
    let Some(f) = player_at(w, s) else { return };
    let fighter = w.fighter(f).id;
    w.fighter_mut(f).events.push(Event::PopText {
        fighter,
        kind: PopKind::HotPotato,
        stacks: 0.0,
        gain: None,
        params: None,
    });
}

/// The blast: a level hit of the rule's own from above the holder, straight up, credited to the one who passed it.
fn blast(w: &mut World, f: FighterId, by: Option<FighterId>) {
    use crate::combat::HitPacket;
    use crate::combat::packet::DataEnum;
    use crate::fighter::moves::V2;
    let p = HitPacket {
        damage: Some(POTATO_DAMAGE),
        base_knock_back: Some(POTATO_KNOCK_BACK),
        knock_back_scaling: Some(POTATO_SCALING),
        hit_reaction: Some(DataEnum::Value(2)),
        hit_pause_time: Some(0.2),
        hit_stun_mult: Some(1.0),
        ignore_facing: Some(true),
        attack_id: Some(0),
        play_on_hit_effects: Some(true),
        additive_kb_mode: Some(DataEnum::Value(0)),
        kb_scaling_starting_dmg: Some(0.0),
        armor_pierce: Some(true),
        kbi_multiplier: Some(1.0),
        ..HitPacket::default()
    };
    let b = &w.fighter(f).body;
    let at = V2 { x: b.x, y: b.y };
    let dealer = crate::actors::ActorRef::Fighter(by.unwrap_or(f));
    crate::combat::hits::apply_level_hit(
        w,
        dealer,
        crate::name!("Chaos_HotPotato"),
        f,
        &p,
        at,
        V2 { x: 0.0, y: 1.0 },
        &[],
    );
}

/// Body Swap's pairs, by player slot the slot whose fighter it would drive, or None with no pair to make. The
/// players with lives left are taken a team at a time in turn (teams in slot order: a 2v2 gives A, B, A, B) and
/// paired off two by two, a pair of teammates left as it is; one left over joins the last pair in a ring of three
/// when it is on neither's team (Free For All of three).
fn swap_map(w: &World) -> Option<Vec<i32>> {
    let n = w.players.len();
    let team = |s: usize| w.fighter(w.players[s]).team.team;
    let out = |s: usize| w.flow.stocks.as_ref().is_some_and(|l| l.get(life(w, w.players[s])).is_none_or(|&x| x <= 0));
    let mut groups: Vec<(i32, Vec<usize>)> = Vec::new();
    for s in (0..n).filter(|&s| !out(s)) {
        match groups.iter_mut().find(|g| g.0 == team(s)) {
            Some(g) => g.1.push(s),
            None => groups.push((team(s), vec![s])),
        }
    }
    let most = groups.iter().map(|g| g.1.len()).max().unwrap_or(0);
    let order: Vec<usize> = (0..most).flat_map(|k| groups.iter().filter_map(move |g| g.1.get(k).copied())).collect();
    let mut map: Vec<i32> = (0..n).map(|s| i32::try_from(s).expect("a slot")).collect();
    let at = |s: usize| i32::try_from(s).expect("a slot");
    for &[a, b] in order.as_chunks::<2>().0 {
        if team(a) != team(b) {
            map[a] = at(b);
            map[b] = at(a);
        }
    }
    if let [.., a, b, c] = order[..]
        && order.len() % 2 == 1
        && map[a] == at(b)
        && team(c) != team(a)
        && team(c) != team(b)
    {
        map[a] = at(b);
        map[b] = at(c);
        map[c] = at(a);
    }
    map.iter().enumerate().any(|(s, &t)| t != at(s)).then_some(map)
}

/// A fighter's place in the stock rule's lives (`flow.stocks`).
fn life(w: &World, f: FighterId) -> usize {
    usize::try_from(w.fighter(f).id).expect("a player id")
}

/// Body Swap's start: each player drives the fighter `swap_map` pairs it with, and the fighter takes its driver's
/// team and lives (the score and the lives stay with the player; the damage, the place and the character with the
/// fighter).
fn body_swap(w: &mut World) {
    let Some(map) = swap_map(w) else { return };
    let home: Vec<i32> = w.players.iter().map(|&f| w.fighter(f).team.team).collect();
    let lives = w.flow.stocks.clone();
    for (s, &t) in map.iter().enumerate() {
        let (own, body) = (w.players[s], w.players[usize::try_from(t).expect("a slot")]);
        w.fighter_mut(body).team.team = home[s];
        if let Some(l) = &lives {
            let x = l[life(w, own)];
            let at = life(w, body);
            w.flow.stocks.as_mut().expect("the stock rule")[at] = x;
        }
    }
    let st = w.wheel.as_mut().expect("the wheel's state");
    st.swap = map;
    st.swap_teams = home;
}

/// Body Swap's end at its window's: control, teams and lives back, unless a player ran out of lives while swapped,
/// when the swap stays to the match's end (its fighter is out, and the one it left goes on with its driver).
fn end_swap(w: &mut World) {
    let Some(st) = w.wheel.as_ref().filter(|s| !s.swap.is_empty() && !s.swap_kept) else { return };
    let out = w.flow.stocks.as_ref().is_some_and(|l| {
        st.swap
            .iter()
            .enumerate()
            .any(|(s, &t)| usize::try_from(t).is_ok_and(|t| t != s && l[life(w, w.players[t])] <= 0))
    });
    if out {
        w.wheel.as_mut().expect("the wheel's state").swap_kept = true;
    } else {
        swap_back(w);
    }
}

/// Body Swap undone: each player's own fighter, team and lives back (at the window's end, and at the match's for
/// the results).
pub fn swap_back(w: &mut World) {
    let Some(st) = w.wheel.as_mut() else { return };
    if st.swap.is_empty() {
        return;
    }
    let map = std::mem::take(&mut st.swap);
    let home = std::mem::take(&mut st.swap_teams);
    st.swap_kept = false;
    let lives = w.flow.stocks.clone();
    for (s, &t) in map.iter().enumerate() {
        let (own, body) = (w.players[s], w.players[usize::try_from(t).expect("a slot")]);
        w.fighter_mut(own).team.team = home[s];
        if let Some(l) = &lives {
            let x = l[life(w, body)];
            let at = life(w, own);
            w.flow.stocks.as_mut().expect("the stock rule")[at] = x;
        }
    }
}

/// The fighter player slot `slot`'s input drives: its own, or under Body Swap the one it is paired with. The app's
/// bots think for this fighter.
pub fn controlled(w: &World, slot: usize) -> FighterId {
    let to = w.wheel.as_ref().and_then(|s| s.swap.get(slot)).and_then(|&t| usize::try_from(t).ok());
    w.players[to.unwrap_or(slot)]
}

/// The player slot whose input drives the fighter of slot `slot` (Body Swap's inverse of `controlled`).
pub fn input_slot(w: &World, slot: usize) -> usize {
    let Some(st) = w.wheel.as_ref().filter(|s| !s.swap.is_empty()) else { return slot };
    st.swap.iter().position(|&t| usize::try_from(t) == Ok(slot)).unwrap_or(slot)
}
