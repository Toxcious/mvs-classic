//! Reindog (content creature, Pawn_Reindog, internal C009, EFighter ReinDog 6): his
//! state, his pawn's hooks, conditions and notifies, the spit and the firebolt, the seismic reactor, the tether and
//! its pull, the Fastball, and his perks (FireStorm, AirSupport, WreckingBall, BigBrother). His attacks themselves are
//! data played by the shared code.
//!
//! His hooks are the pawn's own (`mod.rs`: pawn.rs; `setIsBeingHeld` and `getTetherInfo`, the state's, here). Each
//! file enters its own classes. The bot injections come with the bots' crate.

pub mod fastball;
pub mod fire;
pub mod pawn;
pub mod perks;
pub mod perks_ball;
pub mod seismic;
pub mod state;
pub mod tether;

pub use state::ReindogState;

use crate::chars::CharState;
use crate::fighter::FighterId;
use crate::registry::char_hooks::TetherInfo;
use crate::registry::{CharHooks, Registry, RegistryError};
use crate::world::World;

/// The roster key.
pub const KEY: &str = "creature";

/// `state: () => new ReindogState()`.
fn new_state(_w: &World, _f: FighterId) -> CharState {
    CharState::Creature(ReindogState::default())
}

/// `setIsBeingHeld: (f, held) => { reindog(f).isHeld = held; }` (SetIsBeingHeld @1221).
fn set_is_being_held(w: &mut World, f: FighterId, held: bool) {
    state::reindog_w(w, f).is_held = held;
}

/// `getTetherInfo` (BPI_GetTetherInfo).
fn get_tether_info(w: &World, f: FighterId) -> TetherInfo {
    state::get_tether_info(w, f)
}

/// Reindog's entries: his hooks, then each file's classes (`mod.rs` imports `perks.rs` and `perks_ball.rs` ahead
/// of its own entries).
pub fn register(r: &mut Registry) -> Result<(), RegistryError> {
    let mut h = CharHooks {
        state: Some(new_state),
        set_is_being_held: Some(set_is_being_held),
        get_tether_info: Some(get_tether_info),
        ..CharHooks::default()
    };
    pawn::hooks(&mut h);
    r.chars.enter(KEY, h)?;
    perks::register(r)?;
    perks_ball::register(r)?;
    pawn::register(r)?;
    // (the character registry's order)
    fire::register(r)?;
    tether::register(r)?;
    seismic::register(r)?;
    fastball::register(r)?;
    Ok(())
}
