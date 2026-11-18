//! The match simulation: a [`World`] built once from the match's data and set-up, then stepped once
//! per tick from every player's input. Everything a step reads or writes lives in the `World` as plain data: the
//! fighters in an arena, the actors in typed arenas by family, the match's flow and timers, the random streams. A
//! rollback saves it by `clone_from` into a [`snapshot::SnapshotRing`] slot; the desync hash walks the same fields.
//!
//! The rules of the code (`crates/mvs-sim/README.md` "Rules of the code"): a number is [`Real`]; its math goes through
//! [`math`] (the crate's `clippy.toml` refuses `f64::sin`, `f64::min`, `f64::round` and the rest); a class family is an
//! enum and a `match`, never a trait object; a closure kept in the state is a command enum; maps the step iterates are
//! `IndexMap`s (insertion order, as JavaScript's `Map` has it).
//!
//! `README.md` maps the modules.

#![forbid(unsafe_code)]
// A float to integer `as` cast saturates where JavaScript's `| 0` wraps (`math::to_int32`); a wide integer narrowed
// with `as` wraps silently. Each cast that is exact by construction carries its own `allow` with the reason.
#![deny(clippy::cast_possible_truncation, clippy::cast_sign_loss, clippy::cast_possible_wrap)]

// The derive's generated code names `::mvs_sim::state::StateHash`, in this crate as in any other.
extern crate self as mvs_sim;

pub mod actors;
pub mod ai;
pub mod chars;
pub mod combat;
pub mod consts;
pub mod events;
pub mod fighter;
pub mod ids;
pub mod input;
pub mod items;
pub mod modes;
pub mod names;
pub mod registry;
pub mod rng;
pub mod setup;
pub mod snapshot;
pub mod stages;
pub mod state;
pub mod world;

pub use mvs_sim_derive::State;

/// The body of a call-surface function no track has ported yet: it names the function (`file fn`), takes its
/// arguments (so the signature reads as written) and panics. A port replaces the call; none is left at the end of
/// the rewrite.
#[macro_export]
macro_rules! unported {
    ($what:literal $(, $arg:expr)* $(,)?) => {{
        $(let _ = &$arg;)*
        panic!(concat!("unported: ", $what))
    }};
}

/// A number, as the build's float32. Where the build keeps
/// a double the float32 result moves (the semantic sweep matches each to the build's own routine).
pub type Real = f32;

/// What the build keeps in double: FTimerManager's InternalTime and a timer's ExpireTime (re-armed by
/// `Expire += (double)((float)calls * Rate)`), FApp's DeltaTime.
pub type Clock = f64;

/// The sim's numerics version: two games of another one step apart (their desync hashes differ), so it is a part
/// of the online compatibility id. Raised on every change to the sim's floating-point results.
pub const NUMERICS_VERSION: u32 = 3;

pub mod math;
pub use math::real;

pub use events::Event;
pub use fighter::Fighter;
pub use ids::{Arena, Id};
pub use input::InputState;
pub use registry::{BindError, Registry, RegistryError};
pub use setup::MatchSetup;
pub use world::World;
