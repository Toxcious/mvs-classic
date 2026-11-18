//! LeBron (content lebron, Pawn_C016, fighter byte 21): his state, his pawn's hooks
//! (the ball as a buff of the pawn and the HasBall copy behind it), his basketball, the sass bubble and the fence,
//! the dunk's dash, the dunk shockwave and his perks. His attacks themselves are data played by the shared code.
//!
//! His hooks are the pawn's own (pawn.rs); the other files' joined hooks are all empty. Each file enters
//! its own classes. The bot injections come with the bots' crate.

pub mod ball;
pub mod bubble;
pub mod dunk_wave;
pub mod later;
pub mod pawn;
pub mod perks;
pub mod state;

pub use state::LebronState;

use crate::chars::CharState;
use crate::fighter::FighterId;
use crate::registry::{CharHooks, Registry, RegistryError};
use crate::world::World;

/// The roster key.
pub const KEY: &str = "lebron";

/// `state: () => new LebronState()`.
fn new_state(_w: &World, _f: FighterId) -> CharState {
    CharState::Lebron(LebronState::default())
}

/// LeBron's entries: his hooks, then each file's classes.
pub fn register(r: &mut Registry) -> Result<(), RegistryError> {
    let mut h = CharHooks { state: Some(new_state), ..CharHooks::default() };
    pawn::hooks(&mut h);
    r.chars.enter(KEY, h)?;
    pawn::register(r)?;
    ball::register(r)?;
    bubble::register(r)?;
    later::register(r)?;
    dunk_wave::register(r)?;
    perks::register(r)?;
    Ok(())
}
