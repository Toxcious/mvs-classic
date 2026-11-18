//! Tom and Jerry's state, actors and buff extensions as the record writes them (`chars/tom-and-jerry/`): `state.rs` the fighter
//! state (`TomAndJerryRecord`), a file per port file with its classes' `put` and `load`. `fighter.rs`, `actors.rs` and
//! `parts.rs` call them.

pub mod champagne;
pub mod cheese;
pub mod dynamite;
pub mod fishing;
pub mod jerry;
pub mod kicked;
pub mod rocket;
pub mod slingshot;
pub mod state;
pub mod tennis;
pub mod trap;

pub use state::{tj_load, tj_put};
