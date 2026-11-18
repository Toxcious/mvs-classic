//! Finn's state, actors and buff extensions as the record writes them (`chars/finn/`): `state.rs` his fighter state
//! (`FinnRecord`), a file per port file with its classes' `put` and `load`. `fighter.rs`, `actors.rs` and
//! `parts.rs` call them.

pub mod aimer;
pub mod bmo;
pub mod coins;
pub mod gem;
pub mod high_five;
pub mod perks;
pub mod shockwave;
pub mod shop;
pub mod state;

pub use state::{finn_load, finn_put};
