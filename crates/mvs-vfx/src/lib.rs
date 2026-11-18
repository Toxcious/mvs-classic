//! The Cascade particle player on the CPU : UParticleSystemComponent's systems spawned, ticked
//! and their emitters' particles handed to the renderer as the build's particle vertex data. GPU-free.

// `!(a < b)` is true for a NaN: partial_cmp would hide that.
#![allow(clippy::neg_cmp_op_on_partial_ord)]
// The per-axis loops go axis for axis, reading several arrays at one index.
#![allow(clippy::needless_range_loop)]
// A clamp is written out as a min / max (f64::clamp is banned with the other std float routines).
#![allow(clippy::manual_clamp)]

/// A module type the port has not taken over: panics naming the module type; its arguments count as used.
// (no module type is left without a port; kept for the next one the store brings)
#[allow(unused_macros)]
macro_rules! unported {
    ($what:literal $(, $arg:expr)* $(,)?) => {{
        $(let _ = &$arg;)*
        panic!(concat!("unported: ", $what))
    }};
}

pub mod data;
pub mod distribution;
pub mod draw;
pub mod emitter;
pub mod fill;
pub mod math;
pub mod player;
pub mod random;
pub mod system;
pub mod template;

pub use draw::{DrawParams, EmitterDraw, EmitterKind, SortMode};
pub use player::{Attachment, Player, SystemId};
pub use random::{RandomSource, RandomStream};
pub use system::{CollisionView, Surface};

#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// A particles entry that does not read as its format says.
    #[error("{what}: {why}")]
    Format { what: String, why: String },
    #[error(transparent)]
    Store(#[from] mvs_store::StoreError),
}

pub type Result<T> = std::result::Result<T, Error>;
