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
use crate::setup::TeamStyle;
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

/// Whether the event may be drawn in this match: two live players at least; Body Swap only in a 2v2.
pub fn runnable(w: &World, e: WheelEvent) -> bool {
    match e {
        WheelEvent::BodySwap => w.setup.team_style == TeamStyle::TwoVsTwo && w.players.len() == 4,
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
        WheelEvent::BodySwap => w.wheel.as_mut().expect("the wheel's state").swap = true,
    }
}

/// An event's end: Underdog's mutators off, Hot Potato's blast, Body Swap's control back.
pub fn end(w: &mut World) {
    let (underdog, potato, by) = {
        let st = w.wheel.as_mut().expect("the wheel's state");
        st.swap = false;
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

/// The fighter player slot `slot`'s input drives: its own, or under Body Swap its teammate's. The app's bots think
/// for this fighter.
pub fn controlled(w: &World, slot: usize) -> FighterId {
    let own = w.players[slot];
    if !w.wheel.as_ref().is_some_and(|s| s.swap) {
        return own;
    }
    let team = w.fighter(own).team.team;
    w.players.iter().copied().find(|&p| p != own && w.fighter(p).team.team == team).unwrap_or(own)
}

/// The player slot whose input drives the player in slot `slot` (Body Swap's inverse of `controlled`; the swap is
/// its own inverse).
pub fn input_slot(w: &World, slot: usize) -> usize {
    let f = controlled(w, slot);
    w.players.iter().position(|&p| p == f).unwrap_or(slot)
}
