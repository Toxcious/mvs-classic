//! Morty (C019; content morty, Pawn_C019): his state and C019_AnimBP's slot graph, his
//! pawn's conditions and notifies, the cook grenades, the blaster's charge shot, the splitters, the whip, the scrub
//! shield, the marker and its recall, the mortar aim and its seismic boom, the ship, the aim punch, the portals
//! (accelerators) and his perks (Cleaner, Cook, Hammer). His attacks themselves are data played by the shared code.
//!
//! The marker, the ship, the mortar aim, the aim punch and the portal are the World's typed lists
//! (`actors::lists`), stepped by `world::spawning`, which calls this module's functions (re-exported here). Each file
//! enters its own classes. The bot injections come with the bots' crate.

pub mod accelerator;
pub mod actors;
pub mod aim_punch;
pub mod marker;
pub mod mortar;
pub mod pawn;
pub mod perks;
pub mod projectiles;
pub mod scrub;
pub mod ship;
pub mod state;

pub use accelerator::new_accelerator;
pub use actors::{
    accelerator_tick, activate_marker, destroy_accelerator, destroy_marker, marker_tick, mortar_hurt, seismic_boom,
    ship_tick,
};
pub use aim_punch::{aim_punch_activate, aim_punch_step, new_aim_punch};
pub use marker::new_marker;
pub use mortar::{mortar_activate, mortar_step, new_mortar};
pub use ship::{new_ship, ship_boxes, ship_statics};
pub use state::{MortyState, morty_mut, morty_of, morty_slot_mix, morty_slot_weights, set_lists};

use crate::chars::CharState;
use crate::fighter::FighterId;
use crate::registry::{CharHooks, Registry, RegistryError};
use crate::world::World;

/// The roster key.
pub const KEY: &str = "morty";

/// `state: () => new MortyState()`.
fn new_state(_w: &World, _f: FighterId) -> CharState {
    CharState::Morty(MortyState::default())
}

/// Morty's entries: his hooks, then each file's classes (`mod.rs` imports `perks.rs` and `scrub.rs` ahead of its
/// own entries; then the registry imports `projectiles.rs`; `actors.rs` marks a buff graph ported).
pub fn register(r: &mut Registry) -> Result<(), RegistryError> {
    let mut h = CharHooks {
        state: Some(new_state),
        slot_mix: Some(state::slot_mix),
        slot_mix_mut: Some(state::slot_mix_mut),
        ..CharHooks::default()
    };
    pawn::hooks(&mut h);
    r.chars.enter(KEY, h)?;
    perks::register(r)?;
    scrub::register(r)?;
    pawn::register(r)?;
    projectiles::register(r)?;
    actors::register(r)?;
    Ok(())
}
