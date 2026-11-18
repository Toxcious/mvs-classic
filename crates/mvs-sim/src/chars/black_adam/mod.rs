//! Black Adam (content blackadam, Pawn_C021, internal C021, EFighter C021 25): his
//! state, his pawn's hooks (the hover direction), his flight notifies and perk conditions, C021_AnimBP's hover lists
//! and AimGrab blends, the electric chains and their victim buff, the force field (and the Bubble perk's), the bolt
//! and the Earth Smash pillars, the homing ball, the double strike, the grab hit and his perks. His attacks
//! themselves are data played by the shared code. `projectiles.rs` is split in three files: projectiles.rs (the
//! homing ball), double_strike.rs, grab_hit.rs.
//!
//! The files' pawn hooks are joined onto the main table's (chain.rs reset / matchReset, projectiles.rs hitEnemy /
//! knockedBack / matchReset, anim.rs animUpdate / poseUpdated / death); pawn.rs composes them in that
//! order from each file's functions. Each file enters its own classes. The bot injections come with the bots' crate.

pub mod anim;
pub mod bolt;
pub mod chain;
pub mod double_strike;
pub mod field;
pub mod grab_hit;
pub mod pawn;
pub mod perks;
pub mod projectiles;
pub mod state;

pub use state::BlackAdamState;

use crate::chars::CharState;
use crate::fighter::FighterId;
use crate::registry::{CharHooks, Registry, RegistryError};
use crate::world::World;

/// The roster key.
pub const KEY: &str = "blackadam";

/// `state: () => new BlackAdamState()`.
fn new_state(_w: &World, _f: FighterId) -> CharState {
    CharState::BlackAdam(BlackAdamState::default())
}

/// Black Adam's entries: his hooks, then each file's classes (`mod.rs` imports `perks.rs`, which imports
/// `field.rs`, ahead of its own entries; then the character registry's order).
pub fn register(r: &mut Registry) -> Result<(), RegistryError> {
    let mut h = CharHooks { state: Some(new_state), ..CharHooks::default() };
    pawn::hooks(&mut h);
    r.chars.enter(KEY, h)?;
    field::register(r)?;
    perks::register(r)?;
    pawn::register(r)?;
    chain::register(r)?;
    projectiles::register(r)?;
    double_strike::register(r)?;
    grab_hit::register(r)?;
    anim::register(r)?;
    bolt::register(r)?;
    Ok(())
}
