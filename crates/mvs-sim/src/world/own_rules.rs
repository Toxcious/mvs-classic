//! (the port's own) The Chaos Wheel's own rules, the `Chaos_*` keys of `mvs_mutators::ALL`: no mutator of the build
//! does what they do, so each is a check the sim makes where it counts (a jump, a hit's knockback, a damage record,
//! a dodge, a tick on the ground) and the view makes for Ghosts. A rule is on all match when the set-up's mutators
//! list its key (the Mutators row), or for a window while the wheel has it on (`wheel::window_has`). The wheel runs
//! Bomb Rain and Projectile Party itself, on the item area's spawner (`wheel::start_items`).
//!
//! None of them keeps state of its own but Turbo's and Slow Motion's (`Pace`, the world's): what is on is read off
//! the set-up and the wheel's state, and Lava Floor's and Wind's beats off the match's tick.

use crate::Real;
use crate::State;
use crate::combat::buffs;
use crate::combat::hit::apply_direct_damage;
use crate::fighter::FighterId;
use crate::input::{InputState, Order};
use crate::world::World;

/// One of the port's own rules.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Rule {
    /// Jumps twice as high.
    MoonJump,
    /// Every fighter takes `FEATHERWEIGHT` times the knockback.
    Featherweight,
    /// Every fighter takes `HEAVYWEIGHT` times the knockback.
    Heavyweight,
    /// Damage a fighter deals heals `VAMPIRE` of it off its own.
    Vampire,
    /// A dodge spends no dodge meter, and air dodges never run out.
    DodgeFrenzy,
    /// The item spawner drops bombs only (the wheel's).
    BombRain,
    /// The item spawner drops thrown items only (the wheel's).
    ProjectileParty,
    /// Standing on the ground adds `LAVA_DAMAGE` every `LAVA_TICKS`.
    LavaFloor,
    /// Fighters are hidden but while they attack or are hit, and at a short flash each second (the view's).
    Ghosts,
    /// The whole match runs at `TURBO` times its speed.
    Turbo,
    /// The whole match runs at `SLOW_MOTION` times its speed.
    SlowMotion,
    /// The ground keeps `ICE` of its friction and braking.
    IceRink,
    /// Every live player drifts `WIND_SPEED` sideways, the way turning each `WIND_TICKS`.
    Wind,
    /// Every live player drifts `MAGNET_SPEED` toward the blast box's middle.
    Magnet,
}

impl Rule {
    pub const ALL: [Rule; 14] = [
        Rule::MoonJump,
        Rule::Featherweight,
        Rule::Heavyweight,
        Rule::Vampire,
        Rule::DodgeFrenzy,
        Rule::BombRain,
        Rule::ProjectileParty,
        Rule::LavaFloor,
        Rule::Ghosts,
        Rule::Turbo,
        Rule::SlowMotion,
        Rule::IceRink,
        Rule::Wind,
        Rule::Magnet,
    ];

    /// The rule's key in `mvs_mutators::ALL` and a set-up's mutators.
    pub fn key(self) -> &'static str {
        match self {
            Rule::MoonJump => "Chaos_MoonJump",
            Rule::Featherweight => "Chaos_Featherweight",
            Rule::Heavyweight => "Chaos_Heavyweight",
            Rule::Vampire => "Chaos_Vampire",
            Rule::DodgeFrenzy => "Chaos_DodgeFrenzy",
            Rule::BombRain => "Chaos_BombRain",
            Rule::ProjectileParty => "Chaos_ProjectileParty",
            Rule::LavaFloor => "Chaos_LavaFloor",
            Rule::Ghosts => "Chaos_Ghosts",
            Rule::Turbo => "Chaos_Turbo",
            Rule::SlowMotion => "Chaos_SlowMotion",
            Rule::IceRink => "Chaos_IceRink",
            Rule::Wind => "Chaos_Wind",
            Rule::Magnet => "Chaos_Magnet",
        }
    }

    /// The rule of a key, None for a key of the build's.
    pub fn of_key(key: &str) -> Option<Rule> {
        Rule::ALL.into_iter().find(|r| r.key() == key)
    }

    /// The item spawner's list the rule keeps of the stage's (Random Items' own list), None for a rule that is no
    /// spawner's.
    pub fn items(self) -> Option<&'static [&'static str]> {
        match self {
            Rule::BombRain => Some(&BOMBS),
            Rule::ProjectileParty => Some(&THROWN),
            _ => None,
        }
    }
}

/// Moon Jump: the jump's speed and its cut height (twice the height is the speed times the root of two).
pub const MOON_JUMP_SPEED: Real = std::f32::consts::SQRT_2;
pub const MOON_JUMP_HEIGHT: Real = 2.0;
/// Featherweight's and Heavyweight's share of the knockback a fighter takes.
pub const FEATHERWEIGHT: Real = 1.5;
pub const HEAVYWEIGHT: Real = 0.6;
/// Vampire's share of the damage dealt healed.
pub const VAMPIRE: Real = 0.5;
/// Lava Floor: the damage and the ticks between two (2 a second).
pub const LAVA_DAMAGE: Real = 1.0;
pub const LAVA_TICKS: i32 = 30;
/// Bomb Rain's and Projectile Party's spawner: the next item, seconds after the one before.
pub const ITEMS_EVERY: Real = 2.0;
/// Ghosts: a hidden fighter shows for `GHOST_FLASH` ticks of every `GHOST_BEAT`.
pub const GHOST_BEAT: i32 = 60;
pub const GHOST_FLASH: i32 = 8;
/// Turbo's and Slow Motion's ticks run per frame.
pub const TURBO: Real = 1.3;
pub const SLOW_MOTION: Real = 0.7;
/// Ice Rink's share of the ground's friction, braking friction and braking deceleration.
pub const ICE: Real = 0.1;
/// Wind's drift (units a second) and the ticks it blows one way (left first).
pub const WIND_SPEED: Real = 300.0;
pub const WIND_TICKS: i32 = 600;
/// Magnet's drift (units a second); a fighter nearer the middle than a tick's drift is put on it.
pub const MAGNET_SPEED: Real = 200.0;

/// The items each spawner rule keeps of Random Items' list (a class the list lacks is never spawned: its effects
/// and sounds are the list's).
const BOMBS: [&str; 2] = ["Item_Dynamite_C", "Item_ProximityMine_C"];
const THROWN: [&str; 8] = [
    "Item_Snowball_C",
    "Item_SuperSnowball_C",
    "Item_CarWheel_C",
    "Item_Rock_C",
    "Item_Pie_C",
    "Item_DrumStick_C",
    "Item_Basketball_C",
    "Item_BatarangV2_C",
];

/// Whether `r` is on now: listed in the set-up's mutators, or on in the wheel's window.
pub fn on(w: &World, r: Rule) -> bool {
    let key = r.key();
    w.setup.mutators.iter().any(|m| m == key) || crate::world::wheel::window_has(w, key)
}

/// The share of the knockback every fighter takes (Featherweight, Heavyweight; both on: both).
pub fn taken_knockback(w: &World) -> Real {
    let mut k = 1.0;
    if on(w, Rule::Featherweight) {
        k *= FEATHERWEIGHT;
    }
    if on(w, Rule::Heavyweight) {
        k *= HEAVYWEIGHT;
    }
    k
}

/// Behind a damage record: Hot Potato's pass (`wheel_events::potato_dealt`), then Vampire: `dealer` heals its
/// share of `damage` dealt to another fighter.
pub fn damage_dealt(w: &mut World, f: FighterId, damage: Real, dealer: Option<FighterId>) {
    crate::world::wheel_events::potato_dealt(w, f, damage, dealer);
    let Some(d) = dealer.filter(|&d| d != f && damage > 0.0) else { return };
    if w.fighter(d).life.is_minion || !on(w, Rule::Vampire) {
        return;
    }
    buffs::apply_healing(w, d, damage * VAMPIRE);
}

/// The air dodges a fighter may make in one airtime: the build's, or no limit under Dodge Frenzy.
pub fn air_evade_max(w: &World) -> Real {
    if on(w, Rule::DodgeFrenzy) { Real::MAX } else { crate::consts::AIR_EVADE_MAX }
}

/// Whether a dodge spends the dodge meter (not under Dodge Frenzy).
pub fn evade_spends(w: &World) -> bool {
    !on(w, Rule::DodgeFrenzy)
}

/// Ice Rink's share of the ground's friction and braking (`moves::modes`), 1 when off.
pub fn ice(w: &World) -> Real {
    if on(w, Rule::IceRink) { ICE } else { 1.0 }
}

/// Wind and Magnet: each live player swept (`moves::sweep_move`, the stage's walls stop it) by the tick's drift.
/// A fighter on the ground walks off a ledge it is blown over.
fn drift(w: &mut World) {
    let wind = on(w, Rule::Wind);
    let magnet = on(w, Rule::Magnet);
    if !wind && !magnet {
        return;
    }
    let dt = crate::consts::DT;
    let left = (w.flow.tick / WIND_TICKS).rem_euclid(2) == 0;
    let mid = (w.stage.blast.left + w.stage.blast.right) / 2.0;
    for f in w.players.clone() {
        if w.fighter(f).life.dead > 0.0 {
            continue;
        }
        let x = w.fighter(f).body.x;
        let mut dx = 0.0;
        if wind {
            dx += if left { -WIND_SPEED * dt } else { WIND_SPEED * dt };
        }
        if magnet {
            let to = mid - x;
            let step = MAGNET_SPEED * dt;
            dx += if to.abs() <= step {
                to
            } else if to > 0.0 {
                step
            } else {
                -step
            };
        }
        if dx != 0.0 {
            let ay = crate::fighter::body::actor_y(w, f);
            crate::fighter::moves::sweep_move(w, f, x + dx, ay);
        }
    }
}

/// Turbo's and Slow Motion's state, the world's (`World::pace`): the share of a tick owed, and the input of a tick
/// not run, held for the next.
#[derive(State, PartialEq, Debug, Default)]
pub struct Pace {
    pub owed: Real,
    pub held: Option<Vec<InputState>>,
}

/// The ticks the match runs per frame: Turbo's and Slow Motion's (both on: both), 1 when off.
pub fn pace(w: &World) -> Real {
    let mut p = 1.0;
    if on(w, Rule::Turbo) {
        p *= TURBO;
    }
    if on(w, Rule::SlowMotion) {
        p *= SLOW_MOTION;
    }
    p
}

/// `World::step` under Turbo or Slow Motion: a frame runs as many whole ticks as it owes (the share left over is
/// owed to the next). A frame that runs none holds its input for the next, merged so its presses are not lost; a
/// second tick of one frame runs its input held, with no press of its own.
pub fn paced_step(w: &mut World, inputs: &[InputState], pace: Real) {
    let mut now: Vec<InputState> = inputs.to_vec();
    if let Some(held) = w.pace.held.take() {
        for (i, h) in now.iter_mut().zip(held.iter()) {
            *i = merged(h, i);
        }
    }
    if pace == 1.0 {
        w.pace.owed = 0.0;
        w.step_once(&now);
        return;
    }
    w.pace.owed += pace;
    if w.pace.owed < 1.0 {
        w.pace.held = Some(now);
        return;
    }
    w.pace.owed -= 1.0;
    w.step_once(&now);
    let again: Vec<InputState> = now.iter().map(held_only).collect();
    while w.pace.owed >= 1.0 {
        w.pace.owed -= 1.0;
        w.step_once(&again);
    }
}

/// A frame's input after one not run: the sticks of the later, every flag of either, a press of the later or else
/// of the earlier.
fn merged(a: &InputState, b: &InputState) -> InputState {
    InputState {
        attack: a.attack || b.attack,
        special: a.special || b.special,
        jump: a.jump || b.jump,
        dodge: a.dodge || b.dodge,
        jump_local: a.jump_local || b.jump_local,
        item: a.item || b.item,
        order: if b.order.is_some_and(|o| o.len > 0) { b.order } else { a.order.or(b.order) },
        down_btn: a.down_btn || b.down_btn,
        atk_dir: b.atk_dir.or(a.atk_dir),
        force_dir: b.force_dir.or(a.force_dir),
        force_press: b.force_press.or(a.force_press),
        force_release: b.force_release.or(a.force_release),
        force: a.force || b.force,
        neutral_evade: a.neutral_evade || b.neutral_evade,
        taunt: b.taunt.or(a.taunt),
        taunt_held: a.taunt_held || b.taunt_held,
        ..*b
    }
}

/// A frame's input for its second tick: what is held, no press.
fn held_only(i: &InputState) -> InputState {
    InputState {
        jump_local: false,
        order: i.order.map(|_| Order::default()),
        atk_dir: None,
        force_dir: None,
        force_press: None,
        force_release: None,
        force: false,
        neutral_evade: false,
        taunt: None,
        ..*i
    }
}

/// Ghosts on `f` this tick: hidden but while it attacks, is in hitstun, or flashes.
pub fn ghost_hidden(w: &World, f: FighterId) -> bool {
    on(w, Rule::Ghosts)
        && !crate::fighter::is_attacking(w, f)
        && !crate::fighter::in_hitstun(w, f)
        && w.flow.tick.rem_euclid(GHOST_BEAT) >= GHOST_FLASH
}

/// `ownRulesLatent`: the rules' tick, behind the wheel's in the world's latent pass: Wind's and Magnet's drift,
/// then Lava Floor's damage on every live player standing on the ground, once each `LAVA_TICKS`.
pub fn own_rules_latent(w: &mut World) {
    if w.flow.match_over || (w.setup.flow && !w.flow.match_started) {
        return;
    }
    drift(w);
    if w.flow.tick.rem_euclid(LAVA_TICKS) != 0 || !on(w, Rule::LavaFloor) {
        return;
    }
    for f in w.players.clone() {
        let fr = w.fighter(f);
        if fr.life.dead <= 0.0 && fr.body.on_floor {
            apply_direct_damage(w, f, LAVA_DAMAGE, None);
        }
    }
}
