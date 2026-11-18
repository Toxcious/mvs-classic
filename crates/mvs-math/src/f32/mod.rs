//! The sim's float32 math as the beta computes it: the C runtime functions the exe imports (`crt`, correctly
//! rounded), UE's FMath routines ported from the exe instruction by instruction (`fmath`), and the SSE reciprocal
//! estimates with the exe's Newton steps (`estimate`). Plain f32 / f64 operations only, which Rust never fuses or
//! reorders: every platform gives the same bits.

pub mod crt;
mod double_double;
pub mod estimate;
mod estimate_tables;
pub mod fmath;
#[doc(hidden)]
pub mod reduce;
pub mod vector;
