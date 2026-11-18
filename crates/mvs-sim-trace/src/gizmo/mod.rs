//! Gizmo's state, actors and buff extensions as the record writes them (`chars/gizmo/`): `state.rs` the fighter
//! state (`GizmoRecord`), a file per port file with its classes' `put` and `load`. `fighter.rs`, `actors.rs` and
//! `parts.rs` call them.

pub mod bow;
pub mod car;
pub mod music;
pub mod perks;
pub mod popcorn;
pub mod state;

pub use state::{gizmo_load, gizmo_put, resolve_gizmo_buffs};
