//! Iron Giant's state, actors and buff extensions as the record writes them (`chars/iron-giant/`): `state.rs` his
//! fighter state (`GiantRecord`), a file per port file with its classes' `put` and `load`. `fighter.rs`, `actors.rs`,
//! `parts.rs` and `world.rs` call them.

pub mod actor_state;
pub mod actors;
pub mod cannon_aim;
pub mod perks;
pub mod plasma;
pub mod state;
pub mod turret;
pub mod turret_actors;
pub mod wall;

pub use state::{giant_load, giant_put, resolve_pilot_buffs};
