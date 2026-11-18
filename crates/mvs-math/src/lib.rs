//! The sim's math, giving the same bits as the oracle's sim (its libm, hypot and det-math)
//! and the JavaScript number operations ported sim code leans on (`js`). Sim code calls these,
//! never `f64::sin` and the like (the platform's libm differs by machine), never `f64::min` / `max` / `round` (their
//! NaN, signed-zero and tie rules are not JavaScript's), and never `as i32` for a JavaScript `| 0` (it saturates).
//!
//! Checked against V8 by the math tape the oracle's math recorder writes, then
//! `cargo test -p mvs-math` (`tests/tape.rs`).
//!
//! `f32` is the float32 math of the build (numerics stage B), checked on every input by `mvs-math-check`.

pub mod det;
pub mod f32;
pub mod hypot;
pub mod js;
pub mod libm;
