//! (the port's own) The Chaos Wheel's own rules, the `Chaos_*` keys of `mvs_mutators::ALL`: no mutator of the build
//! does what they do, so each is a check the sim makes where it counts (a jump, a hit's knockback, a damage record,
//! a dodge, a tick on the ground) and the view makes for Ghosts. A rule is on all match when the set-up's mutators
//! list its key (the Mutators row), or for a window while the wheel has it on (`wheel::window_has`). The wheel runs
//! Bomb Rain and Projectile Party itself, on the item area's spawner (`wheel::start_items`).
//!
//! What is on is read off the set-up and the wheel's state, Wind's gusts, Black Hole's opening, Rising Lava's and
//! Flood's height off the match's tick. The one state of their own is a fighter's `body.wet` (Flood's splash).

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
    /// Wind: gusts of `WIND_GUST` ticks, each after a warning of `WIND_WARN`, blow every live player one way
    /// (`gust_at`), harder in the air, so a launch carries further downwind.
    Wind,
    /// Black Hole (key `Chaos_Magnet`): a hole over the stage's middle (`hole_at`) pulls every live player in,
    /// harder the nearer; its core deals `HOLE_DAMAGE` and throws the fighter out.
    BlackHole,
    /// Flood: clear tropical water rises from the blast box's bottom over the main floor and drains again
    /// (`water_at`); fighters in it float, swim and move against its drag.
    Flood,
    /// Storm Flood: Flood's water as a storm sea (rain, lightning, swells): the same rules, its own look and sound.
    StormFlood,
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
        Rule::IceRink,
        Rule::Wind,
        Rule::BlackHole,
        Rule::Flood,
        Rule::StormFlood,
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
            Rule::BlackHole => "Chaos_Magnet",
            Rule::Flood => "Chaos_Flood",
            Rule::StormFlood => "Chaos_StormFlood",
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
/// Rising Lava's touch: the sound posted on the fighter it bounces (the `name!` in `bounce`), behind its hit.
/// The game's side plays the fighter's pain line with it each time, not by the line's chance as for another hit.
// Unverified: the beta has no such rule and so no sound for it; the user picked the shared firewall's ignite by
// ear.
pub const LAVA_HIT_SOUND: &str = "sfx_shared_combat_firewall_ignite";
/// Bomb Rain's and Projectile Party's spawner: the next item, seconds after the one before.
pub const ITEMS_EVERY: Real = 2.0;
/// Ghosts: a hidden fighter shows for `GHOST_FLASH` ticks of every `GHOST_BEAT`.
pub const GHOST_BEAT: i32 = 60;
pub const GHOST_FLASH: i32 = 8;
/// Ice Rink's share of the ground's friction, braking friction and braking deceleration.
pub const ICE: Real = 0.1;
/// Wind: the first warning's start after the rule comes on, the warning's and the gust's length and the cycle of one
/// warning and gust (ticks). A gust that would outlast the wheel's window does not come.
pub const WIND_FIRST: i32 = 60;
pub const WIND_WARN: i32 = 90;
pub const WIND_GUST: i32 = 120;
pub const WIND_EVERY: i32 = 330;
/// Wind: the gust's push (units a second) on a fighter on the ground and off it, and its ramp in and out (ticks).
pub const WIND_GROUND: Real = 900.0;
pub const WIND_AIR: Real = 1400.0;
pub const WIND_RAMP_IN: Real = 5.0;
/// Wind: the gust's lift (units a second) on a fighter off the ground, so it swoops them up as it carries them.
pub const WIND_AIR_LIFT: Real = 300.0;
pub const WIND_RAMP_OUT: Real = 24.0;
/// Black Hole: its middle's height over the main floor's top (units), the ticks it takes to open and to close (the
/// window's last), its core's radius and the reach of its pull (units).
pub const HOLE_HEIGHT: Real = 520.0;
pub const HOLE_OPEN: i32 = 60;
pub const HOLE_CLOSE: i32 = 30;
pub const HOLE_CORE: Real = 150.0;
pub const HOLE_REACH: Real = 3200.0;
/// Black Hole: its pull (units a second) at the reach's edge and at the middle (it grows by the square of the
/// nearness), and beyond the reach falling with the distance.
pub const HOLE_PULL_FAR: Real = 300.0;
pub const HOLE_PULL_NEAR: Real = 1600.0;
/// Black Hole: the core's hit, and the share of its throw's way that is up.
pub const HOLE_DAMAGE: Real = 12.0;
pub const HOLE_KNOCK_BACK: Real = 2200.0;
pub const HOLE_SCALING: Real = 4.0;
pub const HOLE_LIFT: Real = 0.35;
/// Black Hole listed in the set-up's mutators (on all match): open `HOLE_ROW_WINDOW` ticks, then closed
/// `HOLE_ROW_REST`, from the match's start.
pub const HOLE_ROW_WINDOW: i32 = 1200;
pub const HOLE_ROW_REST: i32 = 600;
/// Black Hole's core: the sound posted on the fighter it throws, behind its hit (the `name!` in `throw_out`); the
/// game's side plays the fighter's pain line with it.
// Unverified: the beta has no such rule and so no sound for it; the shared large charge hit is picked by its name.
pub const HOLE_HIT_SOUND: &str = "sfx_shared_combat_charge_hit_large";

/// Flood: how far over the main floor's top the water settles (units), the ticks it takes to rise from the blast
/// box's bottom and to drain at its window's end.
pub const WATER_OVER: Real = 230.0;
pub const WATER_RISE_TICKS: i32 = 210;
pub const WATER_DRAIN_TICKS: i32 = 180;
/// Flood listed in the set-up's mutators (on all match): a rise and drain over `WATER_ROW_WINDOW` ticks, then
/// `WATER_ROW_REST` ticks dry, from the match's start.
pub const WATER_ROW_WINDOW: i32 = 1200;
pub const WATER_ROW_REST: i32 = 600;
/// Flood: a fighter's top speed in the water (its walk, run, dash and air drift) as a share of its own, at full depth
/// (`submerged` 1; a shallower fighter keeps more). It still reaches it at once: the water lowers the top speed, not
/// the acceleration.
pub const WATER_SPEED: Real = 0.65;
/// Flood: the fastest a fighter sinks at full depth (units a second), against its own terminal speed out of it.
pub const WATER_FALL: Real = 700.0;
/// Flood: the water's lift on a fighter off the ground, a share of its gravity at full depth. Over 1, a fighter whose
/// middle is under the surface floats up and rests where the lift meets the gravity (`1 / WATER_LIFT` deep: head
/// and shoulders out), bobbing there.
pub const WATER_LIFT: Real = 1.25;
/// Flood: the fastest the lift raises a fighter (units a second); a jump or a stroke rises faster on its own, the lift
/// then only bearing its gravity.
pub const WATER_RISE: Real = 400.0;
/// Flood: the water's drag on a fighter's rise and fall, the share of the speed lost a second at full depth.
pub const WATER_DRAG: Real = 3.0;
/// Flood: the drag on a launch in the water (`launch::phys_knockback`), the share of its speed lost a second at full
/// depth on top of the knockback's own friction: a hit sends a fighter as far, then the water stops it sooner.
pub const WATER_KNOCK_DRAG: Real = 2.5;
/// Flood: the stick held down past this (below 0) dives: the lift lets go and the fighter sinks at `WATER_FALL`.
pub const WATER_DIVE: Real = -0.5;
/// Flood: the depth from which a jump off the ground is a swim stroke, and the stroke's speed as a share of the air
/// jump's (its height that share squared). A jump nearer the surface leaps out as high as one out of the water.
pub const WATER_DEEP: Real = 0.95;
pub const WATER_STROKE: Real = 0.6;
/// Flood: the fall speed (units a second) from which a fighter's entry is the big splash.
pub const WATER_SPLASH_FAST: Real = 1200.0;
/// Flood: a projectile's speed lost a second under the surface, and the ticks it lasts there at most before its own
/// lifetime's end comes (it fizzles; one without a lifetime of its own is only slowed).
pub const WATER_PROJECTILE_DRAG: Real = 2.0;
pub const WATER_FIZZLE_TICKS: i32 = 30;
/// Flood: the sounds posted on a fighter (the `name!`s in `water_sound`'s callers): its entry at `WATER_SPLASH_FAST`
/// or faster, a slower entry or its way out, and a swim stroke.
// Unverified: the beta has no such rule and so no sound for it; the splashes and the stroke are picked by name.
pub const WATER_SPLASH_BIG_SOUND: &str = "sfx_map_batcave_batmobile_splash";
pub const WATER_SPLASH_SOUND: &str = "sfx_map_batcave_batmobile_splash_start";
pub const WATER_STROKE_SOUND: &str = "sfx_cha_c018_bubblegun_bubble_small_death";

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

/// The clock of the rule `key` while it is on: the ticks since it came on, the ticks it is on for (None: for good)
/// and the tick it came on. In the set-up's mutators it is on from the match's start, `row` (on, off) ticks over and
/// over when given, else for good; in the wheel, for its window.
fn clock(w: &World, key: &str, row: Option<(i32, i32)>) -> Option<(i32, Option<i32>, i32)> {
    if w.setup.mutators.iter().any(|m| m == key) {
        if w.setup.flow && !w.flow.match_started {
            return None;
        }
        let start = if w.setup.flow { w.flow.match_start_tick } else { 0 };
        let t = w.flow.tick - start;
        return Some(match row {
            Some((on, off)) => {
                let k = t.div_euclid(on + off);
                (t - k * (on + off), Some(on), start + k * (on + off))
            }
            None => (t, None, start),
        });
    }
    let (start, len) = crate::world::wheel::window_of(w, key)?;
    Some((w.flow.tick - start, Some(len), start))
}

/// Wind's gust: its way, its age and its strength.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Gust {
    /// The way it blows (sim x): -1 or 1.
    pub dir: Real,
    /// Ticks since its warning started; the gust itself starts at `WIND_WARN`.
    pub age: Real,
    /// Its push now: 0 through the warning, ramping to 1 and back to 0 at its end.
    pub blow: Real,
}

/// Wind's gust `part` of a tick after this one (the view's, between two ticks: `alpha - 1`), None while Wind is off
/// or between a gust's end and the next warning.
#[allow(clippy::cast_precision_loss)]
pub fn gust_at(w: &World, part: Real) -> Option<Gust> {
    use crate::math::{max, min};
    let (t, len, start) = clock(w, Rule::Wind.key(), None)?;
    let into = t - WIND_FIRST;
    if into < 0 {
        return None;
    }
    let k = into.div_euclid(WIND_EVERY);
    let c = into.rem_euclid(WIND_EVERY);
    if c >= WIND_WARN + WIND_GUST || len.is_some_and(|len| WIND_FIRST + k * WIND_EVERY + WIND_WARN + WIND_GUST > len) {
        return None;
    }
    let age = max(c as Real + part, 0.0);
    let g = age - WIND_WARN as Real;
    let blow =
        if g <= 0.0 { 0.0 } else { max(min(min(g / WIND_RAMP_IN, (WIND_GUST as Real - g) / WIND_RAMP_OUT), 1.0), 0.0) };
    // Each gust's way is drawn off the tick its rule came on and its count, the same on every machine.
    let mut h = start.cast_unsigned().wrapping_mul(2_654_435_761) ^ k.cast_unsigned().wrapping_mul(2_246_822_519);
    h ^= h >> 15;
    h = h.wrapping_mul(2_654_435_761);
    Some(Gust { dir: if (h >> 16) & 1 == 1 { 1.0 } else { -1.0 }, age, blow })
}

/// Wind: each live player not in a hit's pause swept (`moves::sweep_move`, the stage's walls stop it) by the gust's
/// push, `WIND_AIR` and a lift of `WIND_AIR_LIFT` off the ground. A fighter on the ground walks off a ledge it is blown over.
fn gusts(w: &mut World) {
    let Some(g) = gust_at(w, 0.0).filter(|g| g.blow > 0.0) else { return };
    let dt = crate::consts::DT;
    for f in w.players.clone() {
        let fr = w.fighter(f);
        if fr.life.dead > 0.0 || fr.hit.hitpause > 0.0 {
            continue;
        }
        let x = fr.body.x;
        let (speed, lift) = if crate::fighter::grounded(w, f) { (WIND_GROUND, 0.0) } else { (WIND_AIR, WIND_AIR_LIFT) };
        let ay = crate::fighter::body::actor_y(w, f);
        crate::fighter::moves::sweep_move(w, f, x + g.dir * speed * g.blow * dt, ay + lift * g.blow * dt);
    }
}

/// Black Hole: where it is and how far open.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Hole {
    /// Its middle (sim x, y).
    pub x: Real,
    pub y: Real,
    /// 0 to 1 as it opens, 1 while open, back to 0 as it closes.
    pub open: Real,
    /// Ticks since it came on.
    pub age: Real,
}

/// Black Hole `part` of a tick after this one (the view's), None while it is off.
#[allow(clippy::cast_precision_loss)]
pub fn hole_at(w: &World, part: Real) -> Option<Hole> {
    use crate::math::{max, min, narrow};
    let (t, len, _) = clock(w, Rule::BlackHole.key(), Some((HOLE_ROW_WINDOW, HOLE_ROW_REST)))?;
    let len = len? as Real;
    let t = t as Real + part;
    if t < 0.0 || t >= len {
        return None;
    }
    let open = max(min(min(t / HOLE_OPEN as Real, (len - t) / HOLE_CLOSE as Real), 1.0), 0.0);
    let g = &w.stage.ground;
    Some(Hole { x: narrow((g.left + g.right) / 2.0), y: narrow(g.top) + HOLE_HEIGHT, open, age: t })
}

/// Black Hole: each live player not in a hit's pause or launch pulled toward it (swept, as Wind's push; along the
/// ground only while on it), harder the nearer; one whose middle reaches its core, while it is open, is thrown out.
fn hole(w: &mut World) {
    use crate::math::{max, min};
    let Some(h) = hole_at(w, 0.0).filter(|h| h.open > 0.0) else { return };
    let dt = crate::consts::DT;
    for f in w.players.clone() {
        let fr = w.fighter(f);
        if fr.life.dead > 0.0 || fr.hit.hitpause > 0.0 || fr.hit.launch.is_some() {
            continue;
        }
        let (x, y) = (fr.body.x, fr.body.y);
        let (dx, dy) = (h.x - x, h.y - y);
        let d = (dx * dx + dy * dy).sqrt();
        if d < HOLE_CORE && h.open >= 1.0 {
            throw_out(w, f, -dx, -dy, d);
            continue;
        }
        if d <= 0.0 {
            continue;
        }
        let pull = if d < HOLE_REACH {
            let near = 1.0 - d / HOLE_REACH;
            HOLE_PULL_FAR + (HOLE_PULL_NEAR - HOLE_PULL_FAR) * near * near
        } else {
            HOLE_PULL_FAR * HOLE_REACH / d
        };
        let step = min(pull * h.open * dt, max(d - HOLE_CORE * 0.5, 0.0));
        let up = if crate::fighter::grounded(w, f) { 0.0 } else { dy / d * step };
        let ay = crate::fighter::body::actor_y(w, f);
        crate::fighter::moves::sweep_move(w, f, x + dx / d * step, ay + up);
    }
}

/// Black Hole's core: a level hit of `HOLE_DAMAGE` throwing `f` out the way (`ox`, `oy`) it is from the middle
/// (`d` away; straight up at the middle), lifted by `HOLE_LIFT`.
fn throw_out(w: &mut World, f: FighterId, ox: Real, oy: Real, d: Real) {
    let (ox, oy) = if d > 1.0 { (ox / d, oy / d + HOLE_LIFT) } else { (0.0, 1.0) };
    let n = (ox * ox + oy * oy).sqrt();
    let dir = crate::fighter::moves::V2 { x: ox / n, y: oy / n };
    rule_hit(
        w,
        f,
        crate::name!("Chaos_Magnet"),
        [HOLE_DAMAGE, HOLE_KNOCK_BACK, HOLE_SCALING],
        dir,
        crate::name!("sfx_shared_combat_charge_hit_large"),
    );
}

/// Rising Lava's surface (sim y) this tick, None while it is off. It rises from the blast box's bottom to
/// `LAVA_OVER` over the main floor's top, stays there, and sinks back by the end of its window.
pub fn lava_level(w: &World) -> Option<Real> {
    lava_level_at(w, 0.0)
}

/// Rising Lava's surface `part` of a tick after this one (the view's, between two ticks: `alpha - 1`).
pub fn lava_level_at(w: &World, part: Real) -> Option<Real> {
    let (t, len, _) = clock(w, Rule::LavaFloor.key(), Some((LAVA_ROW_WINDOW, LAVA_ROW_REST)))?;
    let len = len?;
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

/// Which water a flood is: the same rules, its own look and sound.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WaterStyle {
    /// Flood: clear tropical water.
    Tropical,
    /// Storm Flood: a dark storm sea.
    Storm,
}

/// A flood's water: its surface, its kind and how far through its window it is.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Water {
    /// The surface (sim y).
    pub level: Real,
    pub style: WaterStyle,
    /// Ticks since the water started rising.
    pub age: Real,
    /// How far risen: 0 at the blast box's bottom, 1 settled over the main floor.
    pub risen: Real,
}

/// A flood's water `part` of a tick after this one (the view's, between two ticks: `alpha - 1`), None while no
/// flood is on. It rises from the blast box's bottom to `WATER_OVER` over the main floor's top, eased in and out,
/// stays there, and drains by the end of its window.
pub fn water_at(w: &World, part: Real) -> Option<Water> {
    let (style, (t, len, _)) = [(Rule::Flood, WaterStyle::Tropical), (Rule::StormFlood, WaterStyle::Storm)]
        .into_iter()
        .find_map(|(r, style)| Some((style, clock(w, r.key(), Some((WATER_ROW_WINDOW, WATER_ROW_REST)))?)))?;
    let len = len?;
    let low = w.stage.blast.bottom;
    let high = crate::math::narrow(w.stage.ground.top) + WATER_OVER;
    let rise = WATER_RISE_TICKS.min(len / 2).max(1);
    let drain = WATER_DRAIN_TICKS.min(len / 4).max(1);
    let (t, len, rise, drain) = (t as Real + part, len as Real, rise as Real, drain as Real);
    let ease = |x: Real| x * x * (3.0 - 2.0 * x);
    let risen = if t < 0.0 || t >= len {
        0.0
    } else if t < rise {
        ease(t / rise)
    } else if t < len - drain {
        1.0
    } else {
        1.0 - ease((t - (len - drain)) / drain)
    };
    Some(Water { level: low + (high - low) * risen, style, age: crate::math::max(t, 0.0), risen })
}

/// A flood's water this tick, None while no flood is on or it is down at the blast box's bottom.
pub fn water(w: &World) -> Option<Water> {
    water_at(w, 0.0).filter(|wa| wa.risen > 0.0)
}

/// How deep `f` is in a flood's water `wa`: 0 with its feet at or over the surface, 1 with its middle under it
/// (clamped), its feet `body.y - cap_half`, its middle `body.y`.
pub fn submerged(w: &World, f: FighterId, wa: &Water) -> Real {
    let half = crate::fighter::body::cap_half(w, f);
    let feet = w.fighter(f).body.y - half;
    if half <= 0.0 { 0.0 } else { crate::math::max(0.0, crate::math::min((wa.level - feet) / half, 1.0)) }
}

/// How deep `f` is in a flood's water now (`submerged`), 0 with no flood on and for a dead fighter.
pub fn water_depth(w: &World, f: FighterId) -> Real {
    match water(w) {
        Some(wa) if w.fighter(f).life.dead <= 0.0 => submerged(w, f, &wa),
        _ => 0.0,
    }
}

/// Flood's share of `f`'s top speed (`fighter::max_walk`, the dash's push): 1 out of the water, down to
/// `WATER_SPEED` at full depth.
pub fn water_speed(w: &World, f: FighterId) -> Real {
    let d = water_depth(w, f);
    if d <= 0.0 { 1.0 } else { 1.0 - (1.0 - WATER_SPEED) * d }
}

/// Flood's cap on `f`'s fall (`moves::cap_fall`): its terminal speed `t`, down to `WATER_FALL` at full depth.
pub fn water_terminal(w: &World, f: FighterId, t: Real) -> Real {
    let d = water_depth(w, f);
    if d <= 0.0 { t } else { t + (crate::math::min(t, WATER_FALL) - t) * d }
}

/// Flood's drag on a launch (`launch::phys_knockback`): the share of the velocity kept over a substep of `h` seconds,
/// 1 out of the water.
pub fn water_knock_keep(w: &World, f: FighterId, h: Real) -> Real {
    let d = water_depth(w, f);
    if d <= 0.0 { 1.0 } else { 1.0 - crate::math::min(WATER_KNOCK_DRAG * d * h, 1.0) }
}

/// Whether `f` swims: off the ground in a flood's water, where a jump is a stroke and any number of them may be made
/// (`moves::try_jump`).
pub fn swimming(w: &World, f: FighterId) -> bool {
    !crate::fighter::grounded(w, f) && water_depth(w, f) > 0.0
}

/// Flood's share of an air jump's speed (`moves::do_jump`): `WATER_STROKE` for a stroke, `WATER_DEEP` or deeper,
/// else 1.
pub fn stroke_share(w: &World, f: FighterId, air: bool) -> Real {
    if air && !crate::fighter::grounded(w, f) && water_depth(w, f) >= WATER_DEEP { WATER_STROKE } else { 1.0 }
}

/// A swim stroke's sound on `f`, behind `moves::try_jump`'s jump in the water (a leap out of it has the splash).
pub fn stroke(w: &mut World, f: FighterId) {
    if stroke_share(w, f, true) < 1.0 {
        let b = &w.fighter(f).body;
        let (x, y) = (b.x, b.y);
        water_sound(w, f, crate::name!("sfx_cha_c018_bubblegun_bubble_small_death"), x, y);
    }
}

/// `ev` posted on `f` at (`x`, `y`).
fn water_sound(w: &mut World, f: FighterId, ev: crate::names::Name, x: Real, y: Real) {
    let fighter = w.fighter(f).id;
    w.events.push(crate::Event::Sound { fighter, ev, x: Some(x), y: Some(y), hit: None, pooled: false, key: None });
}

/// Flood's water on each live player and projectile, behind the tick's moves: a fighter crossing the surface splashes
/// (its feet in or out; out, its spent air jumps come back); off the ground and not in a hit's pause, flying or held,
/// the water drags its rise and fall (a launch's is the knockback's, `water_knock_keep`) and lifts it toward
/// `WATER_RISE` unless it dives. Its top speed and fall are the movement's (`water_speed`, `water_terminal`). A
/// projectile under the surface slows and fizzles.
fn flood(w: &mut World) {
    use crate::math::min;
    let Some(wa) = water(w) else {
        for i in 0..w.players.len() {
            let f = w.players[i];
            w.fighter_mut(f).body.wet = false;
        }
        return;
    };
    let dt = crate::consts::DT;
    for i in 0..w.players.len() {
        let f = w.players[i];
        if w.fighter(f).life.dead > 0.0 {
            w.fighter_mut(f).body.wet = false;
            continue;
        }
        let d = submerged(w, f, &wa);
        let fr = w.fighter(f);
        let (x, vy, wet) = (fr.body.x, fr.body.vy, fr.body.wet);
        if (d > 0.0) != wet {
            let ev = if wet {
                let j = &mut w.fighter_mut(f).jump;
                j.jumps = j.jumps.min(1); // the air jumps back
                crate::name!("sfx_map_batcave_batmobile_splash_start")
            } else if vy <= -WATER_SPLASH_FAST {
                crate::name!("sfx_map_batcave_batmobile_splash")
            } else {
                crate::name!("sfx_map_batcave_batmobile_splash_start")
            };
            w.fighter_mut(f).body.wet = d > 0.0;
            water_sound(w, f, ev, x, wa.level);
        }
        let fr = w.fighter(f);
        let held = fr.hit.hitpause > 0.0 || fr.moves.move_none || crate::fighter::moves::is_flying(w, f);
        if d <= 0.0 || held || crate::fighter::grounded(w, f) || crate::fighter::body::no_collision(w, f) {
            continue;
        }
        let kb = fr.hit.kb_mode;
        let dive = fr.controls.inp.y < WATER_DIVE;
        let g = crate::consts::GRAVITY_Z * fr.moves.gravity_scale;
        let mut vy = if kb { vy } else { vy * (1.0 - min(WATER_DRAG * d * dt, 1.0)) };
        if !dive && g > 0.0 {
            let up = g * WATER_LIFT * d * dt;
            vy = if vy >= WATER_RISE { vy + min(up, g * dt) } else { min(vy + up, WATER_RISE) };
        }
        w.fighter_mut(f).body.vy = vy;
    }
    projectiles_in_water(w, wa.level);
}

/// Flood on the projectiles: each live one moving free under the surface `level` slows by `WATER_PROJECTILE_DRAG`,
/// and one with a lifetime of its own running ends `WATER_FIZZLE_TICKS` after it went in at the latest.
fn projectiles_in_water(w: &mut World, level: Real) {
    let keep = 1.0 - crate::math::min(WATER_PROJECTILE_DRAG * crate::consts::DT, 1.0);
    let ids: Vec<_> = w.actors.projectiles.ids().collect();
    for id in ids {
        let Some(p) = w.actors.projectiles.get_mut(id) else { continue };
        if p.dead || p.attached || !p.moving || p.y >= level {
            continue;
        }
        p.vx *= keep;
        p.vy *= keep;
        let paused = p.lifetime_pause.as_ref().is_some_and(|x| x.count > 0);
        if p.life_ticks > 0 && !paused {
            p.life_ticks = p.life_ticks.min(p.age + WATER_FIZZLE_TICKS);
        }
    }
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
    rule_hit(
        w,
        f,
        crate::name!("Chaos_LavaFloor"),
        [LAVA_DAMAGE, LAVA_KNOCK_BACK, LAVA_SCALING],
        crate::fighter::moves::V2 { x: 0.0, y: 1.0 },
        crate::name!("sfx_shared_combat_firewall_ignite"),
    );
}

/// A rule's level hit on `f` named `rule`, dealt by itself (no one's knockout): [damage, base knockback, its
/// scaling] launching it along `dir`, with `sound` posted on it. The hit's pain line is the sound's, which the game's
/// side plays each time (it needs no chance): the hit event carries none.
fn rule_hit(
    w: &mut World,
    f: FighterId,
    rule: crate::names::Name,
    hit: [Real; 3],
    dir: crate::fighter::moves::V2,
    sound: crate::names::Name,
) {
    use crate::combat::HitPacket;
    use crate::combat::packet::DataEnum;
    use crate::fighter::moves::V2;
    let p = HitPacket {
        damage: Some(hit[0]),
        base_knock_back: Some(hit[1]),
        knock_back_scaling: Some(hit[2]),
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
    let dealer = crate::actors::ActorRef::Fighter(f);
    crate::combat::hits::apply_level_hit(w, dealer, rule, f, &p, at, dir, &[]);
    let fighter = w.fighter(f).id;
    if let Some(crate::Event::Hit { pain_damage, .. }) =
        w.events.iter_mut().rev().find(|e| matches!(e, crate::Event::Hit { victim, .. } if *victim == fighter))
    {
        *pain_damage = 0.0;
    }
    w.events.push(crate::Event::Sound {
        fighter,
        ev: sound,
        x: Some(at.x),
        y: Some(at.y),
        hit: None,
        pooled: false,
        key: None,
    });
}

/// Ghosts on `f` this tick: hidden but while it attacks, is in hitstun, or flashes.
pub fn ghost_hidden(w: &World, f: FighterId) -> bool {
    on(w, Rule::Ghosts)
        && !crate::fighter::is_attacking(w, f)
        && !crate::fighter::in_hitstun(w, f)
        && w.flow.tick.rem_euclid(GHOST_BEAT) >= GHOST_FLASH
}

/// `ownRulesLatent`: the rules' tick, behind the wheel's in the world's latent pass: Wind's gusts, Black Hole's pull
/// and core, Rising Lava's touch, then Flood's water.
pub fn own_rules_latent(w: &mut World) {
    if w.flow.match_over || (w.setup.flow && !w.flow.match_started) {
        return;
    }
    gusts(w);
    hole(w);
    lava(w);
    flood(w);
}
