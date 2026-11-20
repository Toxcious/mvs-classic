//! The port's own Waves mode: the player against a stream of weak bots, several at once, for a local record. Not in
//! the build: no line here ports a Blueprint.
//!
//! The run's state is [`WavesRun`] on `World::waves`, set at the World's construction (`start`, set-up mode Waves).
//! Its director spawns the enemies as mode pawns (`world/pawns.rs`) on team 1, bound to [`ENEMY_SLOT`], and counts
//! their ring-outs through the World's `ModeHooks::round_end`. Every draw comes from the World's rng, so a run is the
//! same on a replay of its inputs.
//!
//! The player is a mode pawn of set-up player 0 (its character, skin, taunts and perks), bound to slot 0 on team 0,
//! held by PlayerController 0, or by a match bot at the player's difficulty where set-up player 0 is a bot (the trace
//! matches and the tuning runs). The bots' trees are the app's to set on the World after `World::new` returns, so
//! every bot is possessed from the mode's first latent pass on, never at the construction.
//!
//! The director, in the mode's latent pass of each frame: the enemies rung out in the frame before leave the match
//! (`despawn_pawn`), then, while fewer than the alive cap stand and the spawn gap has passed, one enemy spawns at a
//! stage start away from the player, with the respawn's spawn invulnerability. An enemy is a pawn of a character
//! drawn from the run's cast, default skin, no perks, an Easy match bot ([`ENEMY_LEVEL`]), with starting damage that
//! falls over the run; every tenth is a Giant (the build's Mutator_Giant buff) at the Hard level with none.
//!
//! The player's reward, the port's own: an enemy's ring-out takes [`HEAL`] damage off the player ([`GIANT_HEAL`] for
//! a Giant's). With one life and no heal the damage of the whole run adds up on the player.
//!
//! The numbers are a first set, from headless runs of `waves-arena` (mvs-sim-trace) with a Hard Superman bot in the
//! player's seat on Training Room small, six seeds, 2026-11-20. The play test sets the feel:
//! - the alive cap: [`CAP_START`] (3), one more every [`CAP_STEP`] (15) ring-outs, up to [`CAP_MAX`] (6);
//! - an enemy's starting damage: [`DAMAGE_START`] (250) less [`DAMAGE_FALL`] (1) per ring-out so far, never below
//!   [`DAMAGE_MIN`];
//! - the heal: [`HEAL`] = [`GIANT_HEAL`] = 1000, back to 0 at each ring-out;
//! - the first enemy at [`FIRST_SPAWN`] ticks, then one at most every [`SPAWN_GAP`] (120) ticks.
//!
//! Why these numbers. The bot takes about 150 damage a minute from three Easy enemies and rings out only 5 to 10 a
//! minute:
//! - 80 starting damage, no heal: out after 1 to 5 ring-outs, in under 1:30.
//! - 130, no heal: 2 to 7 ring-outs.
//! - 130 with a heal of 10: 2 to 8.
//! - VeryEasy enemies at 130, no heal: 4 to 9.
//! - 250 with the full heal: 12 to 28 ring-outs, at 1:29 to 4:43.
//!
//! No set brings a Hard bot near 100 in 4 to 8 minutes, so the 100 variant's pace is left to the play test.
//!
//! The arena: a despawned enemy keeps its arena slot (`despawn_pawn`), and an Endless run spawns without end, so a
//! spawn takes the slot of an enemy gone for [`REUSE_AFTER`] ticks where there is one (`pawns::spawn_pawn_into`):
//! the World holds at most the enemies of the last [`REUSE_AFTER`] ticks past the ones alive. An id still held to
//! the old enemy (a projectile's owner, the player's last attacker) then reads the new one.
// Unverified: that after REUSE_AFTER ticks no holder of a fighter id still reads the old enemy in a way a Waves run
// shows (an actor of the pawn's that outlives it longer would take the new enemy as its owner).

use crate::Real;
use crate::State;
use crate::ai::bt::BtBot;
use crate::fighter::FighterId;
use crate::math::to_int32;
use crate::modes::tutorial::pawn::{place_of_loc, starts_with_tag};
use crate::rng::Rng;
use crate::setup::{MatchMode, MatchPlayer, MatchSetup, WavesVariant};
use crate::world::World;
use crate::world::pawns::{self, FirstTicks, ModeHooks, PawnCtl, PawnSpawn};

/// The HUD slot every enemy is bound to (`pawns::bind_slot`): the HUD gives a slot's pawns their damage readout,
/// and the app gives this slot a bot's broker. The player is slot 0.
pub const ENEMY_SLOT: i32 = 1;

/// How many characters a run's cast holds (`draw_cast`).
pub const CAST_SIZE: usize = 5;

/// The ring-outs that clear a 100 Ring-Outs run.
pub const TARGET: u32 = 100;
/// The enemies alive at once at the run's start.
pub const CAP_START: i32 = 3;
/// The most enemies alive at once.
pub const CAP_MAX: i32 = 6;
/// The ring-outs between two steps of the alive cap.
pub const CAP_STEP: i32 = 15;
/// An enemy's starting damage at the run's start.
pub const DAMAGE_START: Real = 250.0;
/// What an enemy's starting damage loses per ring-out so far.
pub const DAMAGE_FALL: Real = 1.0;
/// The least starting damage an enemy has.
pub const DAMAGE_MIN: Real = 0.0;
/// Every this many enemies, one is a Giant.
pub const GIANT_EVERY: i32 = 10;
/// The damage the player loses at an enemy's ring-out (the port's own).
pub const HEAL: Real = 1000.0;
/// The damage the player loses at a Giant's ring-out.
pub const GIANT_HEAL: Real = 1000.0;
/// The ticks before the first enemy.
pub const FIRST_SPAWN: i32 = 60;
/// The least ticks between two spawns.
pub const SPAWN_GAP: i32 = 120;
/// The ticks a despawned enemy's arena slot waits before a spawn takes it.
pub const REUSE_AFTER: i32 = 600;
/// An enemy's bot level (`BtBot::match_bot`): Easy.
pub const ENEMY_LEVEL: i32 = 1;
/// A Giant's bot level: Hard.
pub const GIANT_LEVEL: i32 = 3;
/// The Actor Tag of the stage starts the enemies arrive at (every map's free-for-all starts and respawns).
const START_TAG: &str = "FreeForAll";
/// Mutator_Giant's key in the mutators' map.
const GIANT_MUTATOR: &str = "Mutator_Giant";

/// The run: the director's state and the counts.
#[derive(State, Debug, Default)]
pub struct WavesRun {
    /// The player's pawn.
    pub player: Option<FighterId>,
    /// The first latent pass has run: the bot controllers can be made (the World holds the trees by then).
    pub begun: bool,
    /// Enemies spawned so far.
    pub spawned: i32,
    /// Enemies rung out so far (before the run's end).
    pub ring_outs: i32,
    /// The player has been rung out (counted once).
    pub player_out: bool,
    /// The enemies in the match and not rung out, in spawn order.
    pub enemies: Vec<FighterId>,
    /// The enemies rung out since the last latent pass, which despawns them.
    pub out: Vec<FighterId>,
    /// The despawned enemies whose arena slots a spawn may take, with the tick each left at, oldest first.
    pub gone: Vec<(FighterId, i32)>,
    /// The first tick the next spawn may come at.
    pub next_spawn: i32,
    /// The places the enemies arrive at (the stage's starts of [`START_TAG`], sim plane).
    pub points: Vec<(Real, Real)>,
    pub end: Option<WavesEnd>,
    /// The World's tick at the end.
    pub end_tick: i32,
}

/// What the app and the HUD read of a run each frame (`status`).
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct WavesStatus {
    pub variant: WavesVariant,
    /// Enemies rung out so far.
    pub ring_outs: u32,
    /// The count that clears the run (100 Ring-Outs), or None (Endless).
    pub target: Option<u32>,
    /// Ticks since the run's clock started (the match start).
    pub ticks: u32,
    /// Enemies alive now.
    pub alive: u32,
    /// Set once the run is over.
    pub end: Option<WavesEnd>,
}

/// How a run ended.
#[derive(State, Copy, PartialEq, Eq, Debug)]
pub enum WavesEnd {
    /// 100 Ring-Outs reached its target.
    Cleared,
    /// The player was rung out.
    PlayerOut,
}

fn variant(w: &World) -> WavesVariant {
    match &w.setup.mode {
        Some(MatchMode::Waves { variant, .. }) => *variant,
        _ => panic!("waves: not a Waves set-up"),
    }
}

fn run(w: &World) -> &WavesRun {
    w.waves.as_deref().expect("the Waves run")
}

fn run_mut(w: &mut World) -> &mut WavesRun {
    w.waves.as_deref_mut().expect("the Waves run")
}

fn len_real(n: usize) -> Real {
    crate::real(u32::try_from(n).expect("a short list"))
}

/// A draw of `0..n` from the World's rng.
fn pick(w: &mut World, n: usize) -> usize {
    usize::try_from(to_int32(w.random_int(len_real(n)))).expect("an index")
}

/// The mode's begin play at the World's construction (`reset.rs`): the run on `World::waves`, the World's
/// `ModeHooks`, the player's pawn (set-up player 0, its perks); the first spawns come from the latent pass.
pub fn start(w: &mut World) {
    let st = &w.data.stage;
    let mut points: Vec<(Real, Real)> = starts_with_tag(st, START_TAG).iter().map(|s| place_of_loc(&s.loc)).collect();
    if points.is_empty() {
        points = st.spawns.iter().chain(&st.respawns).map(|s| place_of_loc(&s.loc)).collect();
    }
    if points.is_empty() {
        panic!("waves: the stage has no PlayerStart");
    }
    // The player's start: Player1 of the 1v1 starts, as a match's first player.
    let at = st
        .spawns
        .iter()
        .find(|s| s.tag.as_deref() == Some("Player1") && s.tags.iter().any(|t| t == "1v1"))
        .or_else(|| st.spawns.first())
        .map_or(points[0], |s| place_of_loc(&s.loc));
    w.waves = Some(Box::new(WavesRun { points, next_spawn: FIRST_SPAWN, ..WavesRun::default() }));
    w.mode = Some(ModeHooks { round_end, latent: Some(latent) });
    let p = w.setup.players.first().cloned().unwrap_or_default();
    let key = p.character.clone().unwrap_or_else(|| crate::world::DEFAULT_CHARACTER.to_owned());
    let character = w.lit(&key);
    let o = PawnSpawn { character, x: at.0, y: at.1, facing_left: at.0 > 0.0, at: FirstTicks::Leftover };
    let f = pawns::spawn_pawn(w, o);
    pawns::bind_slot(w, f, 0);
    {
        let fr = w.fighter_mut(f);
        fr.team.team = 0;
        fr.team.bot_controlled = p.bot;
        fr.controls.bot_controlled = p.bot;
    }
    let taunts: Vec<crate::names::Name> = p.taunts.iter().map(|k| w.lit(k)).collect();
    crate::fighter::anim::set_taunt_slots(w, f, Some(&taunts));
    run_mut(w).player = Some(f);
    if !p.bot {
        pawns::possess(w, f, PawnCtl::Player);
    }
    crate::combat::perks::add_player_perks(w, f, 0);
}

/// The World's `ModeHooks::round_end`, for every loser that leaves the blast box: a counted enemy goes from the match
/// in the next latent pass and never comes back; the player's ring-out ends the run. The loser is hidden (as the
/// tutorial's game mode hides a loser) behind the blast's death effects.
/// A fighter rung out twice (its inner and outer blast box both left) is counted once.
pub fn round_end(w: &mut World, f: FighterId) {
    let r = run(w);
    let counted = if r.player == Some(f) { r.player_out } else { !r.enemies.contains(&f) };
    if counted {
        return;
    }
    crate::world::blast::blast_box_death_fx(w, f);
    // (as a match's OnPlayerRingoutImpl: the characters' and the perks' handlers, with the loser still in)
    crate::world::blast::player_knocked_out(w, f);
    w.fighter_mut(f).vis.invisible = true;
    let tick = w.flow.tick;
    let target = (variant(w) == WavesVariant::RingOuts100).then_some(TARGET);
    let giant = carries_giant(w, f);
    let r = run_mut(w);
    if r.player == Some(f) {
        r.player_out = true;
        if r.end.is_none() {
            r.end = Some(WavesEnd::PlayerOut);
            r.end_tick = tick;
        }
        return;
    }
    let Some(i) = r.enemies.iter().position(|&e| e == f) else { return };
    r.enemies.remove(i);
    r.out.push(f);
    if r.end.is_some() {
        return;
    }
    r.ring_outs += 1;
    if target.is_some_and(|t| u32::try_from(r.ring_outs).is_ok_and(|n| n >= t)) {
        r.end = Some(WavesEnd::Cleared);
        r.end_tick = tick;
    }
    let player = r.player;
    if let Some(p) = player {
        heal(w, p, if giant { GIANT_HEAL } else { HEAL });
    }
}

/// The player's reward for a ring-out: `by` damage off, none below 0 (the port's own: with one life and no heal, the
/// damage of a whole run would add up on the player, the module's doc).
fn heal(w: &mut World, p: FighterId, by: Real) {
    // (a heal's path: the buffs and signals bound to the damage see it)
    crate::combat::buffs::apply_healing(w, p, by);
}

/// Whether the World runs a Waves run (the shared code's gate on the mode's own rules).
pub fn on(w: &World) -> bool {
    w.waves.is_some()
}

/// Whether fighter `f` carries the Giant mutator's buff (a Giant enemy).
fn carries_giant(w: &World, f: FighterId) -> bool {
    let Some(cls) = w.data.mutators.map.get(GIANT_MUTATOR) else { return false };
    let cls = w.lit(cls);
    w.fighter(f).buffs.buffs.iter().any(|b| b.cls == cls)
}

/// The World's `ModeHooks::latent`: the director (the module's doc).
pub fn latent(w: &mut World) {
    if !run(w).begun {
        run_mut(w).begun = true;
        possess_player_bot(w);
    }
    let tick = w.flow.tick;
    for f in std::mem::take(&mut run_mut(w).out) {
        // The holds the enemy is the instigator of end with it (a match ends them at the respawn, which never comes).
        let victims = w.fighter(f).holds.hold_victims.clone();
        for v in victims {
            if w.fighter(v).holds.hold.as_ref().is_some_and(|h| h.bound) {
                crate::combat::holds::end_hold_buff(w, v);
            }
        }
        pawns::despawn_pawn(w, f, false);
        run_mut(w).gone.push((f, tick));
    }
    let r = run(w);
    if r.end.is_some() || tick < r.next_spawn || len_i32(r.enemies.len()) >= alive_cap(r.ring_outs) {
        return;
    }
    spawn_enemy(w);
    run_mut(w).next_spawn = tick + SPAWN_GAP;
}

fn len_i32(n: usize) -> i32 {
    i32::try_from(n).expect("a short list")
}

/// The enemies that may stand at once after `ring_outs` ring-outs.
pub fn alive_cap(ring_outs: i32) -> i32 {
    (CAP_START + ring_outs / CAP_STEP).min(CAP_MAX)
}

/// The starting damage of an enemy spawned after `ring_outs` ring-outs (a Giant's is 0).
pub fn starting_damage(ring_outs: i32) -> Real {
    let d = DAMAGE_START - DAMAGE_FALL * crate::real(ring_outs);
    if d < DAMAGE_MIN { DAMAGE_MIN } else { d }
}

/// Whether the `n`th enemy of the run (from 1) is a Giant.
pub fn is_giant(n: i32) -> bool {
    n % GIANT_EVERY == 0
}

/// A match bot of `level` for the pawn `f`, its first think read as a tutorial's bag's (`possess_bot`).
fn bot_ctl(w: &World, f: FighterId, seed: i32, level: i32) -> PawnCtl {
    let Some(trees) = w.bot_trees.clone() else { panic!("waves: a bot needs World::bot_trees") };
    let mut bot = BtBot::match_bot(seed, &trees, level);
    let cl = &w.fighter(f).clock;
    bot.ticks = 1;
    bot.now = if w.scratch.step_at != 0 { cl.prev_time } else { cl.time };
    bot.fighter_born = cl.creation_time;
    PawnCtl::Bot(Box::new(pawns::bot_ctl(bot, None, None)))
}

/// A bot in the player's seat (set-up player 0 a bot): a match bot at its difficulty, seeded as the app seeds a
/// player's bot (12345 + the slot).
fn possess_player_bot(w: &mut World) {
    let Some(p) = w.setup.players.first().filter(|p| p.bot) else { return };
    let level = to_int32(p.difficulty.unwrap_or(0.0));
    let Some(f) = run(w).player else { return };
    let ctl = bot_ctl(w, f, 12345, level);
    pawns::possess(w, f, ctl);
}

/// The place of the next enemy: a draw among the stage's points, the half furthest from the player along x.
fn spawn_point(w: &mut World) -> (Real, Real) {
    let px = run(w).player.map_or(0.0, |p| w.fighter(p).body.x);
    let mut order: Vec<(Real, Real)> = run(w).points.clone();
    // (a stable sort: the points' order breaks ties)
    order.sort_by(|a, b| (b.0 - px).abs().total_cmp(&(a.0 - px).abs()));
    let n = order.len().div_ceil(2);
    let i = pick(w, n);
    order[i]
}

/// One enemy of the run (the module's doc).
fn spawn_enemy(w: &mut World) {
    let n = run(w).spawned + 1;
    run_mut(w).spawned = n;
    let giant = is_giant(n);
    let cast = w.setup.mode.as_ref().map_or(&[][..], MatchMode::cast).to_vec();
    let key = if cast.is_empty() {
        crate::world::DEFAULT_CHARACTER.to_owned()
    } else {
        let i = pick(w, cast.len());
        cast[i].clone()
    };
    let character = w.lit(&key);
    let (x, y) = spawn_point(w);
    let px = run(w).player.map_or(0.0, |p| w.fighter(p).body.x);
    let tick = w.flow.tick;
    let reuse = match run(w).gone.first() {
        Some(&(g, at)) if at + REUSE_AFTER <= tick => {
            run_mut(w).gone.remove(0);
            Some(g)
        }
        _ => None,
    };
    let o = PawnSpawn { character, x, y, facing_left: x > px, at: FirstTicks::Leftover };
    let f = pawns::spawn_pawn_into(w, o, reuse);
    pawns::bind_slot(w, f, ENEMY_SLOT);
    let damage = if giant { 0.0 } else { starting_damage(run(w).ring_outs) };
    {
        let fr = w.fighter_mut(f);
        fr.team.team = 1;
        fr.team.bot_controlled = true;
        fr.controls.bot_controlled = true;
        fr.stats.starting_damage = damage;
        fr.stats.damage = damage;
    }
    crate::fighter::match_side::spawn_invulnerability(w, f);
    let seed = to_int32(w.random_int(2_147_483_648.0));
    let ctl = bot_ctl(w, f, seed, if giant { GIANT_LEVEL } else { ENEMY_LEVEL });
    pawns::possess(w, f, ctl);
    if giant {
        match w.data.mutators.map.get(GIANT_MUTATOR).filter(|c| !c.is_empty()) {
            Some(cls) => {
                let cls = w.lit(cls);
                crate::combat::buffs::spawn_buff(w, f, cls, 0.0, Some(0), Some(f), true, None);
            }
            None => crate::fighter::common::warn_text(w, "waves: the data has no Mutator_Giant"),
        }
    }
    run_mut(w).enemies.push(f);
}

/// The run as the app and the HUD read it, or None in a World with no Waves run.
pub fn status(w: &World) -> Option<WavesStatus> {
    let r = w.waves.as_deref()?;
    let variant = variant(w);
    let now = if r.end.is_some() { r.end_tick } else { w.flow.tick };
    Some(WavesStatus {
        variant,
        ring_outs: u32::try_from(r.ring_outs).unwrap_or(0),
        target: (variant == WavesVariant::RingOuts100).then_some(TARGET),
        ticks: u32::try_from(now).unwrap_or(0),
        alive: u32::try_from(r.enemies.len()).unwrap_or(u32::MAX),
        end: r.end,
    })
}

/// A Waves run's set-up: `player` (the local player, its character, perks, skin and input preferences) alone on
/// team 0, hazards on, no rules (the mode ends the run), and the enemies' `cast` (`draw_cast`). The menus' start and
/// the app's `--waves` switch both build it here.
pub fn setup(variant: WavesVariant, player: MatchPlayer, cast: Vec<String>) -> MatchSetup {
    MatchSetup {
        players: vec![MatchPlayer { bot: false, team: Some(0), ..player }],
        allow_hazards: true,
        mode: Some(MatchMode::Waves { variant, cast }),
        ..MatchSetup::default()
    }
}

/// The run's cast: `CAST_SIZE` keys of `roster` drawn from `seed` (fewer when the roster is shorter), in draw order.
/// The menus call it with the match's seed before the World exists, so the cast can be loaded up front.
pub fn draw_cast(seed: i32, roster: &[String]) -> Vec<String> {
    let mut left: Vec<&String> = roster.iter().collect();
    left.dedup();
    let mut rng = Rng::new(seed);
    let mut out = Vec::new();
    while out.len() < CAST_SIZE && !left.is_empty() {
        let i = usize::try_from(to_int32(rng.int(len_real(left.len())))).expect("an index");
        let k = left.remove(i);
        if !out.contains(k) {
            out.push(k.clone());
        }
    }
    out
}

#[cfg(test)]
mod tests;
