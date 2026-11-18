//! Velma's state, actors and buff extensions as the record writes them (`chars/velma/`): `state.rs` the fighter
//! states (`VelmaRecord`, `VelmaPoliceRecord`), a file per port file with its classes' `put` and `load` (the pawn has
//! none). `fighter.rs`, `actors.rs` and `parts.rs` call them.

pub mod beam;
pub mod capture;
pub mod carry;
pub mod evidence;
pub mod gadgets;
pub mod mark;
pub mod perks;
pub mod police;
pub mod state;

pub use state::{police_load, police_put, velma_load, velma_put};
