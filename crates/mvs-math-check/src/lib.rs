//! Long checks of `mvs_math::f32`, run on request (each takes seconds to minutes on 16 threads):
//! - `crt-exhaustive`: the correctly rounded CRT functions on every float input (one-argument ones) or on large
//!   samples (atan2f, powf, fmodf), against `reference`;
//! - `zen4-tables`: writes the rsqrtss / rcpss tables from this CPU, or checks them against it on every input;
//! - `exe-fmath`: the FMath ports against the beta exe's own routines, called in a mapped copy of the exe (`exe`).
//!
//! The quick tests (`cargo test -p mvs-math-check`) check the reference itself and a sample of each function.

pub mod hardware;
pub mod par;
pub mod reference;

#[cfg(windows)]
pub mod exe;

/// Equal bits, or both NaN (the ports do not promise the exe's NaN payloads).
pub fn same(a: f32, b: f32) -> bool {
    a.to_bits() == b.to_bits() || (a.is_nan() && b.is_nan())
}

/// xorshift64: the samples' generator.
pub fn next(state: &mut u64) -> u64 {
    *state ^= *state << 13;
    *state ^= *state >> 7;
    *state ^= *state << 17;
    *state
}

/// A float of random bits.
pub fn any_float(state: &mut u64) -> f32 {
    f32::from_bits(next(state) as u32)
}

/// A float uniform in [lo, hi).
pub fn float_in(state: &mut u64, lo: f32, hi: f32) -> f32 {
    let u = (next(state) >> 40) as f32 * (1.0 / 16_777_216.0);
    lo + (hi - lo) * u
}
