//! (the port's own) The Chaos Wheel's own rules, the `Chaos_*` keys of `mvs_mutators::ALL`: no mutator of the build
//! does what they do, so each is a check the sim makes where it counts (a jump, a hit's knockback, a damage record,
//! a dodge, a tick on the ground) and the view makes for Ghosts. A rule is on all match when the set-up's mutators
//! list its key (the Mutators row), or for a window while the wheel has it on (`wheel::window_has`). The wheel runs
//! Bomb Rain and Projectile Party itself, on the item area's spawner (`wheel::start_items`).
//!
//! None of them keeps state of its own: what is on is read off the set-up and the wheel's state, Wind's beat and
//! Rising Lava's height off the match's tick.

use crate::Real;
use crate::combat::buffs;
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
    /// Rising Lava: lava rises from the blast box's bottom over the main floor and sinks again (`lava_level`);
    /// a fighter that touches it takes `LAVA_DAMAGE` and is bounced up.
    LavaFloor,
    /// Fighters are hidden but while they attack or are hit, and at a short flash each second (the view's).
    Ghosts,
    /// The ground keeps `ICE` of its friction and braking.
    IceRink,
    /// Every live player drifts `WIND_SPEED` sideways, the way turning each `WIND_TICKS`.
    Wind,
    /// Every live player drifts `MAGNET_SPEED` toward the blast box's middle.
    Magnet,
}

impl Rule {
    pub const ALL: [Rule; 12] = [
        Rule::MoonJump,
        Rule::Featherweight,
        Rule::Heavyweight,
        Rule::Vampire,
        Rule::DodgeFrenzy,
        Rule::BombRain,
        Rule::ProjectileParty,
        Rule::LavaFloor,
        Rule::Ghosts,
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
/// Rising Lava: the damage a touch deals and its bounce (a launch straight up).
pub const LAVA_DAMAGE: Real = 10.0;
pub const LAVA_KNOCK_BACK: Real = 2000.0;
pub const LAVA_SCALING: Real = 4.0;
/// Rising Lava: how far over the main floor's top it rises (units), the ticks it takes to rise from the blast box's
/// bottom (at most half a window) and to sink back (at most a quarter of one, the window's last).
pub const LAVA_OVER: Real = 60.0;
pub const LAVA_RISE_TICKS: i32 = 600;
pub const LAVA_SINK_TICKS: i32 = 180;
/// Rising Lava listed in the set-up's mutators (on all match): a rise and sink over `LAVA_ROW_WINDOW` ticks, then
/// `LAVA_ROW_REST` ticks down, from the match's start.
pub const LAVA_ROW_WINDOW: i32 = 1200;
pub const LAVA_ROW_REST: i32 = 600;
/// Rising Lava's touch: the sound posted on the fighter it bounces (the `name!` in `bounce`).
// Unverified: the beta has no such rule and so no sound for it; the shared firewall's burn hit is picked by its name.
pub const LAVA_HIT_SOUND: &str = "sfx_shared_combat_firewall_burn_hit";
/// Bomb Rain's and Projectile Party's spawner: the next item, seconds after the one before.
pub const ITEMS_EVERY: Real = 2.0;
/// Ghosts: a hidden fighter shows for `GHOST_FLASH` ticks of every `GHOST_BEAT`.
pub const GHOST_BEAT: i32 = 60;
pub const GHOST_FLASH: i32 = 8;
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

/// Rising Lava's surface (sim y) this tick, None while it is off. It rises from the blast box's bottom to
/// `LAVA_OVER` over the main floor's top, stays there, and sinks back by the end of its window.
pub fn lava_level(w: &World) -> Option<Real> {
    lava_level_at(w, 0.0)
}

/// Rising Lava's surface `part` of a tick after this one (the view's, between two ticks: `alpha - 1`).
pub fn lava_level_at(w: &World, part: Real) -> Option<Real> {
    let key = Rule::LavaFloor.key();
    let (t, len) = if w.setup.mutators.iter().any(|m| m == key) {
        if w.setup.flow && !w.flow.match_started {
            return None;
        }
        let start = if w.setup.flow { w.flow.match_start_tick } else { 0 };
        ((w.flow.tick - start).rem_euclid(LAVA_ROW_WINDOW + LAVA_ROW_REST), LAVA_ROW_WINDOW)
    } else {
        let (start, len) = crate::world::wheel::window_of(w, key)?;
        (w.flow.tick - start, len)
    };
    let low = w.stage.blast.bottom;
    let high = crate::math::narrow(w.stage.ground.top) + LAVA_OVER;
    let rise = LAVA_RISE_TICKS.min(len / 2).max(1);
    let sink = LAVA_SINK_TICKS.min(len / 4).max(1);
    let (t, len, rise, sink) = (t as Real + part, len as Real, rise as Real, sink as Real);
    Some(if t < 0.0 || t >= len {
        low
    } else if t < rise {
        low + (high - low) * (t / rise)
    } else if t < len - sink {
        high
    } else {
        high - (high - low) * ((t - (len - sink)) / sink)
    })
}

/// Rising Lava's touch: each live player whose feet are under the surface, not in a hit's pause or launch and not
/// already rising out of it, takes
/// `LAVA_DAMAGE` and a launch straight up. The lava at rest on the blast box's bottom touches no one.
fn lava(w: &mut World) {
    let Some(level) = lava_level(w).filter(|&l| l > w.stage.blast.bottom) else { return };
    for f in w.players.clone() {
        let fr = w.fighter(f);
        let held = fr.hit.hitpause > 0.0 || fr.hit.launch.is_some();
        if fr.life.dead > 0.0 || held || fr.body.vy > 0.0 || fr.body.y - crate::fighter::body::cap_half(w, f) >= level {
            continue;
        }
        bounce(w, f);
    }
}

/// The lava's bounce: a level hit of `LAVA_DAMAGE` launching `f` straight up, dealt by itself (no one's knockout).
fn bounce(w: &mut World, f: FighterId) {
    use crate::combat::HitPacket;
    use crate::combat::packet::DataEnum;
    use crate::fighter::moves::V2;
    let p = HitPacket {
        damage: Some(LAVA_DAMAGE),
        base_knock_back: Some(LAVA_KNOCK_BACK),
        knock_back_scaling: Some(LAVA_SCALING),
        hit_reaction: Some(DataEnum::Value(2)),
        hit_pause_time: Some(0.1),
        hit_stun_mult: Some(0.5),
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
    let ev = crate::name!("sfx_shared_combat_firewall_burn_hit");
    let fighter = w.fighter(f).id;
    w.events.push(crate::Event::Sound {
        fighter,
        ev,
        x: Some(at.x),
        y: Some(at.y),
        hit: None,
        pooled: false,
        key: None,
    });
    let dealer = crate::actors::ActorRef::Fighter(f);
    crate::combat::hits::apply_level_hit(
        w,
        dealer,
        crate::name!("Chaos_LavaFloor"),
        f,
        &p,
        at,
        V2 { x: 0.0, y: 1.0 },
        &[],
    );
}

/// Ghosts on `f` this tick: hidden but while it attacks, is in hitstun, or flashes.
pub fn ghost_hidden(w: &World, f: FighterId) -> bool {
    on(w, Rule::Ghosts)
        && !crate::fighter::is_attacking(w, f)
        && !crate::fighter::in_hitstun(w, f)
        && w.flow.tick.rem_euclid(GHOST_BEAT) >= GHOST_FLASH
}

/// `ownRulesLatent`: the rules' tick, behind the wheel's in the world's latent pass: Wind's and Magnet's drift,
/// then Rising Lava's touch.
pub fn own_rules_latent(w: &mut World) {
    if w.flow.match_over || (w.setup.flow && !w.flow.match_started) {
        return;
    }
    drift(w);
    lava(w);
}
