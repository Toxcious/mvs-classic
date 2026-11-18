//! Stripe's state, actors and buff extensions as the record writes them (`chars/stripe/`): `state.rs` his fighter
//! state (`StripeRecord`), a file per port file with its classes' `put` and `load`. `fighter.rs`, `actors.rs` and
//! `parts.rs` call them.

pub mod bomb;
pub mod dash;
pub mod gun;
pub mod saw;
pub mod state;

pub use state::{stripe_load, stripe_put};
