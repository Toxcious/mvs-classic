//! (the port's own) The Chaos Wheel's own rules, the `Chaos_*` keys of `mvs_mutators::ALL`: no mutator of the build
//! does what they do, so each is a check the sim makes where it counts (a jump, a hit's knockback, a damage record,
//! a dodge, a tick on the ground) and the view makes for Ghosts. A rule is on all match when the set-up's mutators
//! list its key (the Mutators row), or for a window while the wheel has it on (`wheel::window_has`). The wheel runs
//! Bomb Rain and Projectile Party itself, on the item area's spawner (`wheel::start_items`).
//!
//! None of them keeps state of its own: what is on is read off the set-up and the wheel's state, and Lava Floor's
//! beat off the match's tick.

use crate::Real;
use crate::combat::buffs;
use crate::combat::hit::apply_direct_damage;
use crate::fighter::FighterId;
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
}

impl Rule {
    pub const ALL: [Rule; 9] = [
        Rule::MoonJump,
        Rule::Featherweight,
        Rule::Heavyweight,
        Rule::Vampire,
        Rule::DodgeFrenzy,
        Rule::BombRain,
        Rule::ProjectileParty,
        Rule::LavaFloor,
        Rule::Ghosts,
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

/// Vampire, behind a damage record: `dealer` heals its share of `damage` dealt to another fighter.
pub fn damage_dealt(w: &mut World, f: FighterId, damage: Real, dealer: Option<FighterId>) {
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

/// Ghosts on `f` this tick: hidden but while it attacks, is in hitstun, or flashes.
pub fn ghost_hidden(w: &World, f: FighterId) -> bool {
    on(w, Rule::Ghosts)
        && !crate::fighter::is_attacking(w, f)
        && !crate::fighter::in_hitstun(w, f)
        && w.flow.tick.rem_euclid(GHOST_BEAT) >= GHOST_FLASH
}

/// `ownRulesLatent`: the rules' tick, behind the wheel's in the world's latent pass: Lava Floor's damage on every
/// live player standing on the ground, once each `LAVA_TICKS`.
pub fn own_rules_latent(w: &mut World) {
    if w.flow.tick.rem_euclid(LAVA_TICKS) != 0 || w.flow.match_over || (w.setup.flow && !w.flow.match_started) {
        return;
    }
    if !on(w, Rule::LavaFloor) {
        return;
    }
    for f in w.players.clone() {
        let fr = w.fighter(f);
        if fr.life.dead <= 0.0 && fr.body.on_floor {
            apply_direct_damage(w, f, LAVA_DAMAGE, None);
        }
    }
}
