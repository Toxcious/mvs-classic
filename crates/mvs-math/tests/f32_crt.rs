//! Quick checks of the correctly rounded CRT functions: the special cases, the double-double paths against the
//! fast ones on a sample, and values checked on every input by `mvs-math-check`'s `crt-exhaustive`.

use mvs_math::f32::crt::{self, slow};

fn bits(v: f32) -> u32 {
    v.to_bits()
}

fn xorshift(s: &mut u64) -> u64 {
    *s ^= *s << 13;
    *s ^= *s >> 7;
    *s ^= *s << 17;
    *s
}

#[test]
fn special_cases() {
    assert_eq!(bits(crt::sinf(-0.0)), 0x8000_0000);
    assert!(crt::sinf(f32::INFINITY).is_nan());
    assert_eq!(crt::cosf(0.0), 1.0);
    assert_eq!(bits(crt::tanf(-0.0)), 0x8000_0000);
    assert!(crt::asinf(1.5).is_nan());
    assert_eq!(crt::asinf(1.0), core::f32::consts::FRAC_PI_2);
    assert_eq!(crt::acosf(-1.0), core::f32::consts::PI);
    assert_eq!(crt::acosf(1.0), 0.0);
    assert_eq!(crt::atanf(f32::NEG_INFINITY), -core::f32::consts::FRAC_PI_2);
    assert_eq!(crt::expf(0.0), 1.0);
    assert_eq!(crt::expf(f32::NEG_INFINITY), 0.0);
    assert_eq!(crt::expf(89.0), f32::INFINITY);
    assert_eq!(crt::logf(0.0), f32::NEG_INFINITY);
    assert!(crt::logf(-1.0).is_nan());
    assert_eq!(crt::logf(1.0), 0.0);
    // atan2 of zeros and infinities (C Annex F)
    assert_eq!(bits(crt::atan2f(-0.0, 1.0)), 0x8000_0000);
    assert_eq!(crt::atan2f(0.0, -0.0), core::f32::consts::PI);
    assert_eq!(crt::atan2f(-0.0, -1.0), -core::f32::consts::PI);
    assert_eq!(crt::atan2f(1.0, 0.0), core::f32::consts::FRAC_PI_2);
    assert_eq!(crt::atan2f(f32::INFINITY, f32::INFINITY), core::f32::consts::FRAC_PI_4);
    assert_eq!(crt::atan2f(f32::INFINITY, f32::NEG_INFINITY), 3.0 * core::f32::consts::FRAC_PI_4);
    // pow (C Annex F)
    assert_eq!(crt::powf(f32::NAN, 0.0), 1.0);
    assert_eq!(crt::powf(1.0, f32::NAN), 1.0);
    assert_eq!(crt::powf(-1.0, f32::INFINITY), 1.0);
    assert_eq!(bits(crt::powf(-0.0, 3.0)), 0x8000_0000);
    assert_eq!(crt::powf(-0.0, -3.0), f32::NEG_INFINITY);
    assert_eq!(crt::powf(-0.0, -2.0), f32::INFINITY);
    assert!(crt::powf(-2.0, 0.5).is_nan());
    assert_eq!(crt::powf(-2.0, 3.0), -8.0);
    assert_eq!(crt::powf(0.5, f32::NEG_INFINITY), f32::INFINITY);
    assert_eq!(crt::powf(f32::NEG_INFINITY, -1.0).to_bits(), 0x8000_0000);
    // the exact cases: a midpoint rounds to even
    assert_eq!(crt::powf(1.0 + 1.0 / 4096.0, 2.0), 1.0 + 1.0 / 2048.0);
    assert_eq!(crt::powf(16.0, 0.25), 2.0);
    assert_eq!(crt::powf(2.0, -149.0), f32::from_bits(1));
    // fmod
    assert_eq!(crt::fmodf(7.5, 2.0), 1.5);
    assert_eq!(bits(crt::fmodf(-4.0, 2.0)), 0x8000_0000);
    assert_eq!(crt::fmodf(-7.5, 2.0), -1.5);
    assert!(crt::fmodf(1.0, 0.0).is_nan());
    assert!(crt::fmodf(f32::INFINITY, 1.0).is_nan());
    assert_eq!(crt::fmodf(1.0, f32::INFINITY), 1.0);
    assert_eq!(crt::fmodf(1e10, 3.0), 1.0);
    assert_eq!(crt::fmodf(f32::MAX, 3.0), 0.0);
}

#[test]
fn values_from_the_reference() {
    // results checked on every input against the arbitrary-precision reference
    assert_eq!(bits(crt::sinf(1e22)), 0xbf3b_ecc4);
    assert_eq!(bits(crt::cosf(f32::MAX)), 0x3f5a_5f96);
    assert_eq!(bits(crt::tanf(1.570_796_4)), 0xcbae_8a4a);
    assert_eq!(bits(crt::expf(-103.5)), 0x0000_0001);
    assert_eq!(bits(crt::logf(f32::from_bits(1))), 0xc2ce_8ed0);
}

#[test]
fn double_double_paths_agree_with_the_fast_ones() {
    let mut s = 0x0123_4567_89ab_cdefu64;
    for _ in 0..20_000 {
        let x = f32::from_bits(xorshift(&mut s) as u32);
        let small = (xorshift(&mut s) >> 40) as f32 / 16_777_216.0 * 20.0 - 10.0;
        for v in [x, small] {
            if v.is_finite() {
                assert_eq!(bits(slow::sinf(v)), bits(crt::sinf(v)), "sinf {v:e}");
                assert_eq!(bits(slow::cosf(v)), bits(crt::cosf(v)), "cosf {v:e}");
                assert_eq!(bits(slow::tanf(v)), bits(crt::tanf(v)), "tanf {v:e}");
                assert_eq!(bits(slow::atanf(v)), bits(crt::atanf(v)), "atanf {v:e}");
            }
            if v.abs() < 1.0 {
                assert_eq!(bits(slow::asinf(v)), bits(crt::asinf(v)), "asinf {v:e}");
                assert_eq!(bits(slow::acosf(v)), bits(crt::acosf(v)), "acosf {v:e}");
            }
            if (-110.0..=89.0).contains(&v) {
                assert_eq!(bits(slow::expf(v)), bits(crt::expf(v)), "expf {v:e}");
            }
            if v.is_finite() && v > 0.0 {
                assert_eq!(bits(slow::logf(v)), bits(crt::logf(v)), "logf {v:e}");
            }
            let y = small * 0.3;
            if v.is_finite() && v != 0.0 && y != 0.0 {
                assert_eq!(bits(slow::atan2f(v, y)), bits(crt::atan2f(v, y)), "atan2f {v:e} {y:e}");
            }
            let ax = v.abs();
            if ax.is_finite() && ax > 0.0 && ax != 1.0 && y != 0.0 {
                let z = f64::from(y) * f64::from(ax).ln();
                if z.abs() < 80.0 {
                    assert_eq!(bits(slow::pow_abs(ax, y)), bits(crt::powf(ax, y)), "powf {ax:e} {y:e}");
                }
            }
        }
    }
}
