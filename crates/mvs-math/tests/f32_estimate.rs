//! Quick checks of the Zen 4 estimates; `mvs-math-check`'s `zen4-tables` compares them with the CPU on every input.

use mvs_math::f32::estimate::{inv_sqrt, rcp_estimate, reciprocal_accurate, rsqrt_estimate};

#[test]
fn estimates_at_known_points() {
    assert_eq!(rsqrt_estimate(1.0).to_bits(), 0x3f7f_f800);
    assert_eq!(rsqrt_estimate(4.0).to_bits(), 0x3eff_f800);
    assert_eq!(rcp_estimate(1.0).to_bits(), 0x3f7f_f000);
    assert_eq!(rcp_estimate(-0.5).to_bits(), 0xbfff_f000);
}

#[test]
fn special_cases() {
    assert_eq!(rsqrt_estimate(0.0), f32::INFINITY);
    assert_eq!(rsqrt_estimate(-0.0), f32::NEG_INFINITY);
    assert_eq!(rsqrt_estimate(f32::from_bits(1)), f32::INFINITY);
    assert_eq!(rsqrt_estimate(-1.0).to_bits(), 0xffc0_0000);
    assert_eq!(rsqrt_estimate(f32::INFINITY).to_bits(), 0);
    assert_eq!(rsqrt_estimate(f32::from_bits(0x7f80_0001)).to_bits(), 0x7fc0_0001);
    assert_eq!(rcp_estimate(-0.0), f32::NEG_INFINITY);
    assert_eq!(rcp_estimate(f32::NEG_INFINITY).to_bits(), 0x8000_0000);
    // the estimate of 1 / 2^126 and 1 / 2^127 is below the smallest normal float: flushed; of 1 / 2^125 it is not
    assert_eq!(rcp_estimate(f32::from_bits(0x7f00_0000)).to_bits(), 0);
    assert_eq!(rcp_estimate(f32::from_bits(0x7e80_0000)).to_bits(), 0);
    assert_eq!(rcp_estimate(f32::from_bits(0x7e00_0000)).to_bits(), 0x00ff_f000);
}

#[test]
fn newton_steps_converge() {
    // Below about 1e-19 the reciprocal steps square an estimate past the float range (as the exe's do).
    let mut x = 1e-18f32;
    while x < 1e18 {
        let r = inv_sqrt(x);
        let e = (f64::from(r) * f64::from(x).sqrt() - 1.0).abs();
        assert!(e < 4e-7, "inv_sqrt({x:e}) = {r:e}");
        let q = reciprocal_accurate(x);
        let e = (f64::from(q) * f64::from(x) - 1.0).abs();
        assert!(e < 4e-7, "reciprocal_accurate({x:e}) = {q:e}");
        x *= 1.37;
    }
}
