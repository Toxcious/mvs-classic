//! Arya (content arya, Pawn_Arya, internal C006): her state, her pawn's hooks (the
//! stolen-face override pack and the disguise's branch windows, the evade buff, the knock-out handler), her three
//! notifies and her backstab buff, the thrown dagger and its conditions, the disguise's unmasking shock, the kill pie
//! and her perks. Her attacks themselves are data played by the shared code; the Disguise is shared
//! (`combat::disguise`) and calls the pawn through her hooks.
//!
//! Her hooks are the pawn's own (pawn.rs); the other files' joined hooks are
//! empty. Each file enters its own classes. The bot injections come with the bots' crate.

pub mod actors;
pub mod knife;
pub mod pawn;
pub mod perks;
pub mod state;

pub use state::AryaState;

use crate::chars::CharState;
use crate::fighter::FighterId;
use crate::registry::{CharHooks, Registry, RegistryError};
use crate::world::World;

/// The roster key.
pub const KEY: &str = "arya";

/// `state: () => new AryaState()`.
fn new_state(_w: &World, _f: FighterId) -> CharState {
    CharState::Arya(AryaState::default())
}

/// Arya's entries: her hooks, then each file's classes (`mod.rs` imports `perks.rs` ahead of its own entries).
pub fn register(r: &mut Registry) -> Result<(), RegistryError> {
    let mut h = CharHooks { state: Some(new_state), ..CharHooks::default() };
    pawn::hooks(&mut h);
    r.chars.enter(KEY, h)?;
    perks::register(r)?;
    pawn::register(r)?;
    knife::register(r)?;
    actors::register(r)?;
    Ok(())
}
