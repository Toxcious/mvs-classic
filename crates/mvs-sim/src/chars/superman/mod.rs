//! Superman (content superman, Pawn_Superman, fighter byte 10): his state, his pawn's
//! hooks and the ice meter, the flights, the heat vision, the ice breath, the aim punch and his perks. His attacks
//! themselves are data played by the shared code.
//!
//! Each file fills its own hooks into his [`CharHooks`] (its own table and the hook joiner) and enters
//! its own classes; a hook two files have (matchReset) is joined here, the pawn's first. `bots.rs` (his bot tasks
//! and the bot injections) comes with the bots' crate.

pub mod aim_punch;
pub mod bots;
#[cfg(test)]
mod bots_tests;
pub mod flight;
pub mod ice;
pub mod laser;
pub mod pawn;
pub mod perks;
pub mod state;

pub use state::SupermanState;

use crate::chars::CharState;
use crate::fighter::FighterId;
use crate::registry::{CharHooks, Registry, RegistryError};
use crate::world::World;

/// The roster key.
pub const KEY: &str = "superman";

/// `state: () => new SupermanState()`.
fn new_state(_w: &World, _f: FighterId) -> CharState {
    CharState::Superman(SupermanState::default())
}

/// `matchReset`: the pawn's own, then flight.rs's (the hook joiner's order).
fn match_reset(w: &mut World, f: FighterId) {
    pawn::match_reset(w, f);
    flight::match_reset(w, f);
}

/// Superman's entries: his hooks, then each file's classes.
pub fn register(r: &mut Registry) -> Result<(), RegistryError> {
    let mut h = CharHooks { state: Some(new_state), match_reset: Some(match_reset), ..CharHooks::default() };
    pawn::hooks(&mut h);
    flight::hooks(&mut h);
    ice::hooks(&mut h);
    r.chars.enter(KEY, h)?;
    pawn::register(r)?;
    flight::register(r)?;
    laser::register(r)?;
    ice::register(r)?;
    aim_punch::register(r)?;
    perks::register(r)?;
    Ok(())
}
