//! Harley Quinn (content harley, Pawn_Harley): her state, her pawn's hooks (the idle layer
//! by the satchel in her hand) and conditions, the bomb (HarleyBomb_C, a rigid sphere that sticks, detonates and is
//! carried through tunnels), the jack-in-the-box, the gloves, the confetti buffs and her perks (Bullseye,
//! ConfettiExplosion, SmoothMoves). Her attacks themselves are data played by the shared code.
//!
//! The bomb is bomb.rs, her other actors are actors.rs, her pawn's own logic is pawn.rs. Each file enters its
//! own classes. The bot injections come with the bots' crate.

pub mod actors;
pub mod bomb;
pub mod pawn;
pub mod perks;
pub mod state;

pub use state::{HarleyState, harley_mut, harley_of};

use crate::chars::CharState;
use crate::fighter::FighterId;
use crate::registry::{CharHooks, Registry, RegistryError};
use crate::world::World;

/// The roster key.
pub const KEY: &str = "harley";

/// `state: () => new HarleyState()`.
fn new_state(_w: &World, _f: FighterId) -> CharState {
    CharState::Harley(HarleyState::default())
}

/// Harley Quinn's entries: her hooks, then each file's classes (`mod.rs` imports `actors.rs`, which imports
/// `bomb.rs`, and `perks.rs` ahead of its own entries).
pub fn register(r: &mut Registry) -> Result<(), RegistryError> {
    let mut h = CharHooks { state: Some(new_state), ..CharHooks::default() };
    pawn::hooks(&mut h);
    r.chars.enter(KEY, h)?;
    bomb::register(r)?;
    actors::register(r)?;
    perks::register(r)?;
    pawn::register(r)?;
    Ok(())
}
