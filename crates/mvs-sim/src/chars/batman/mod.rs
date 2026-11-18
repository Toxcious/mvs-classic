//! Batman (content batman, Pawn_BatmanV2, internal C004, EFighter 2): his state, his
//! pawn's hooks, the glide and aim slots of his AnimBP, the batarang, the bomb, the grapple and his perks. His attacks
//! themselves are data played by the shared code. The smoke family is shared (`actors::smoke_bomb`) and calls the
//! pawn through his hooks.
//!
//! Each file fills its own hooks into his [`CharHooks`] (its own table and the hook joiner) and enters
//! its own classes. `bots.rs` (his bot tasks and the bot injections) comes with the bots' crate.

pub mod anim;
pub mod batarang;
pub mod bomb;
pub mod bots;
pub mod glide;
pub mod grapple;
pub mod pawn;
pub mod perks;
pub mod perks_grapple;
pub mod state;

pub use state::BatmanState;

use crate::chars::CharState;
use crate::fighter::FighterId;
use crate::registry::{CharHooks, Registry, RegistryError};
use crate::world::World;

/// The roster key.
pub const KEY: &str = "batman";

/// `state: () => new BatmanState()`.
fn new_state(_w: &World, _f: FighterId) -> CharState {
    CharState::Batman(BatmanState::default())
}

/// Batman's entries: his hooks, then each file's classes.
pub fn register(r: &mut Registry) -> Result<(), RegistryError> {
    let mut h = CharHooks { state: Some(new_state), ..CharHooks::default() };
    pawn::hooks(&mut h);
    anim::hooks(&mut h);
    glide::hooks(&mut h);
    grapple::hooks(&mut h);
    r.chars.enter(KEY, h)?;
    pawn::register(r)?;
    batarang::register(r)?;
    bomb::register(r)?;
    grapple::register(r)?;
    perks::register(r)?;
    perks_grapple::register(r)?;
    Ok(())
}
