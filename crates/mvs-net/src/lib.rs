//! Online play: one peer's side of a match as rollback over the deterministic sim, inputs only on the
//! wire. A referee (`mvs_protocol::referee`) owns the match's clock and rules every human slot's input stream; each
//! peer sends its own inputs to the referee alone and steps the World on the ruled streams, predicting beyond them.
//!
//! Deviation: the port's own netcode; the build ran dedicated servers with client prediction.

pub mod backend;
pub mod codec;
pub mod codes;
pub mod events;
pub mod fail;
pub mod lan;
pub mod link;
pub mod pending;
pub mod service;
pub mod session;
