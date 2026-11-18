//! Rick's state, actors and buff extensions as the record writes them (`chars/rick/`): `state.rs` his fighter state
//! (`RickRecord`), a file per port file with its classes' `put` and `load` (`silenced.rs` the silenced buff).
//! `fighter.rs`, `actors.rs` and `parts.rs` call them.

pub mod aimer;
pub mod air_flap;
pub mod fart_bomb;
pub mod grenade;
pub mod meeseeks;
pub mod portal;
pub mod silenced;
pub mod state;
pub mod teleport;

pub use state::{rick_load, rick_put};
