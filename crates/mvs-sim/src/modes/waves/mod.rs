//! The port's own Waves mode (`docs/waves-plan.md`): the player against a stream of weak bots, several at once, for
//! a local record. Not in the build: no line here ports a Blueprint.
//!
//! The run's state is [`WavesRun`] on `World::waves`, set at the World's construction (`start`, set-up mode Waves).
//! Its director spawns the enemies as mode pawns (`world/pawns.rs`) on team 1, bound to [`ENEMY_SLOT`], and counts
//! their ring-outs through the World's `ModeHooks::round_end`. Every draw comes from the World's rng, so a run is the
//! same on a replay of its inputs.

use crate::State;
use crate::setup::{MatchMode, MatchPlayer, MatchSetup, WavesVariant};
use crate::world::World;

/// The HUD slot every enemy is bound to (`pawns::bind_slot`): the HUD gives a slot's pawns their damage readout,
/// and the app gives this slot a bot's broker. The player is slot 0.
pub const ENEMY_SLOT: i32 = 1;

/// How many characters a run's cast holds (`draw_cast`).
pub const CAST_SIZE: usize = 5;

/// The run: the director's state and the counts. Its fields are the sim track's.
#[derive(State, Debug, Default)]
pub struct WavesRun {}

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
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum WavesEnd {
    /// 100 Ring-Outs reached its target.
    Cleared,
    /// The player was rung out.
    PlayerOut,
}

/// The mode's begin play at the World's construction (`reset.rs`): the run on `World::waves`, the World's
/// `ModeHooks`, the player's pawn (set-up player 0, its perks) and the director's first spawns.
pub fn start(w: &mut World) {
    crate::unported!("waves::start", w)
}

/// The run as the app and the HUD read it, or None in a World with no Waves run.
pub fn status(w: &World) -> Option<WavesStatus> {
    w.waves.as_ref()?;
    crate::unported!("waves::status", w)
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
    crate::unported!("waves::draw_cast", seed, roster)
}
