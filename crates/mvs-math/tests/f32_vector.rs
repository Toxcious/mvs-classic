//! The vector, quaternion and rotator ports against the beta exe's own results on a few inputs per routine (bit
//! patterns written by `mvs-math-check`'s `exe-fmath golden`, which checks every routine on every float of one
//! argument and on random tuples), and the inline shapes on cases worked by hand from their disassembly.

use mvs_math::f32::vector::{self, AdvanceResult};

fn f(b: &[u32]) -> Vec<f32> {
    b.iter().map(|&x| f32::from_bits(x)).collect()
}

fn bits(v: &[f32]) -> Vec<u32> {
    v.iter().map(|x| x.to_bits()).collect()
}

fn v3(b: &[u32]) -> [f32; 3] {
    let v = f(b);
    [v[0], v[1], v[2]]
}

fn rows(b: &[u32]) -> [[f32; 3]; 3] {
    [v3(&b[0..3]), v3(&b[3..6]), v3(&b[6..9])]
}

fn flat(m: [[f32; 3]; 3]) -> Vec<u32> {
    bits(&m.concat())
}

#[test]
fn make_from_x() {
    let cases: &[(&[u32], &[u32])] = &[
        (
            &[0x3e99999a, 0xc3b3f333, 0x3f000000],
            &[
                0x3a5a8385, 0xbf7fffea, 0x3ab61844, 0x3f7ffffa, 0x3a5a8393, 0x80000000, 0xb59b6e41, 0x3ab61840,
                0x3f7ffff0,
            ],
        ),
        (
            &[0xbfd9999a, 0x44341000, 0x3f000000],
            &[
                0xbb1aaf01, 0x3f7fffce, 0x3a35fb10, 0xbf7fffd2, 0xbb1aaf03, 0x00000000, 0x35dbeae1, 0xba35faef,
                0x3f7ffffd,
            ],
        ),
        (
            &[0x42f6e979, 0x3a83126f, 0x3f000000],
            &[
                0x3f7fff76, 0x3707e52c, 0x3b84b5cd, 0xb707e575, 0x3f800000, 0x00000000, 0xbb84b5cd, 0xb30ce5a5,
                0x3f7fff76,
            ],
        ),
        (
            &[0xc3b3f333, 0xc2340000, 0x3f000000],
            &[
                0xbf7e0595, 0xbdfe17a7, 0x3ab4b01c, 0x3dfe17b7, 0xbf7e05a5, 0x00000000, 0x3ab34ab8, 0x39335779,
                0x3f7ffff0,
            ],
        ),
        (
            &[0x44341000, 0x4a371b00, 0x3f000000],
            &[
                0x397bbee1, 0x3f800000, 0x3432f4fc, 0xbf800000, 0x397bbee1, 0x00000000, 0xae2ffba2, 0xb432f4fc,
                0x3f800000,
            ],
        ),
        (
            &[0x3a83126f, 0x3e99999a, 0x3f000000],
            &[
                0x3ae0c94b, 0x3f03b5f2, 0x3f5b8493, 0xbf7fffa3, 0x3b5a73be, 0x00000000, 0xbb3b5225, 0xbf5b8443,
                0x3f03b622,
            ],
        ),
        (
            &[0xc2340000, 0xbfd9999a, 0x3f000000],
            &[
                0xbf7fcd3f, 0xbd1a9e32, 0x3c35e749, 0x3d1aa0a3, 0xbf7fd148, 0x00000000, 0x3c35c617, 0x39dbbe8d,
                0x3f7ffbf4,
            ],
        ),
        (
            &[0x4a371b00, 0x42f6e979, 0x3f000000],
            &[
                0x3f800000, 0x382c9aa0, 0x3432f4fc, 0xb82c9aa0, 0x3f800000, 0x00000000, 0xb432f4fc, 0xacf15161,
                0x3f800000,
            ],
        ),
    ];
    for (input, want) in cases {
        assert_eq!(&flat(vector::make_from_x(v3(input)))[..], *want, "{input:x?}");
    }
}

#[test]
fn make_from_xz() {
    let cases: &[(&[u32], &[u32])] = &[
        (
            &[0x3e99999a, 0xc3b3f333, 0x3f000000, 0xc3b3f333, 0xbe800000, 0x3e99999a],
            &[
                0x3a5a8385, 0xbf7fffea, 0x3ab61844, 0x3a5a42b9, 0x3ab62f8d, 0x3f7fffe9, 0xbf7ffff3, 0xba5a35d1,
                0x3a5a9067,
            ],
        ),
        (
            &[0xbfd9999a, 0x44341000, 0x3f000000, 0x44341000, 0xbe800000, 0xbfd9999a],
            &[
                0xbb1aaf01, 0x3f7fffce, 0x3a35fb10, 0x3b1aaafe, 0xba34853d, 0x3f7fffcd, 0x3f7fffa3, 0x3b1aca5f,
                0xbb1a8f9b,
            ],
        ),
        (
            &[0x42f6e979, 0x3a83126f, 0x3f000000, 0x3a83126f, 0xbe800000, 0x42f6e979],
            &[
                0x3f7fff76, 0x3707e52c, 0x3b84b5cd, 0xb788beef, 0x3f7fffde, 0x3b04b603, 0xbb84b598, 0xbb04b6d7,
                0x3f7fff54,
            ],
        ),
        (
            &[0xc3b3f333, 0xc2340000, 0x3f000000, 0xc2340000, 0xbe800000, 0xc3b3f333],
            &[
                0xbf7e0595, 0xbdfe17a7, 0x3ab4b01c, 0xbdfe05f2, 0x3f7dfeab, 0x3c72cd50, 0xbb5221f8, 0x3c6e1fd5,
                0xbf7ff8bc,
            ],
        ),
        (
            &[0x44341000, 0x4a371b00, 0x3f000000, 0x4a371b00, 0xbe800000, 0x44341000],
            &[
                0x397bbee1, 0x3f800000, 0x3432f4fc, 0xb97bbee1, 0xb3ea220a, 0x3f800000, 0x3f800000, 0xb97bbee4,
                0x397bbedf,
            ],
        ),
        (
            &[0x3a83126f, 0x3e99999a, 0x3f000000, 0x3e99999a, 0xbe800000, 0x3a83126f],
            &[
                0x3ae0c94b, 0x3f03b5f2, 0x3f5b8493, 0xbf15005b, 0xbf325f54, 0x3ed6a493, 0x3f502b4e, 0xbeffe72b,
                0x3e98b589,
            ],
        ),
        (
            &[0xc2340000, 0xbfd9999a, 0x3f000000, 0xbfd9999a, 0xbe800000, 0xc2340000],
            &[
                0xbf7fcd3f, 0xbd1a9e32, 0x3c35e749, 0xbd1ad047, 0x3f7fd09d, 0xbb871fed, 0xbc3338b7, 0xbb94c547,
                0xbf7ffb66,
            ],
        ),
        (
            &[0x4a371b00, 0x42f6e979, 0x3f000000, 0x42f6e979, 0xbe800000, 0x4a371b00],
            &[
                0x3f800000, 0x382c9aa0, 0x3432f4fc, 0xb82c9aa0, 0x3f800000, 0x33b697fd, 0xb432f406, 0xb3b69bc2,
                0x3f800000,
            ],
        ),
    ];
    for (input, want) in cases {
        assert_eq!(&flat(vector::make_from_xz(v3(&input[0..3]), v3(&input[3..6])))[..], *want, "{input:x?}");
    }
}

#[test]
fn matrix_rotator() {
    let cases: &[(&[u32], &[u32])] = &[
        (
            &[
                0x3a5a8385, 0xbf7fffea, 0x3ab61844, 0x3f7ffffa, 0x3a5a8393, 0x80000000, 0xb59b6e41, 0x3ab61840,
                0x3f7ffff0,
            ],
            &[0x3da3050f, 0xc2b3e78d, 0xb26bc91a],
        ),
        (
            &[
                0xbb1aaf01, 0x3f7fffce, 0x3a35fb10, 0xbf7fffd2, 0xbb1aaf03, 0x00000000, 0x35dbeae1, 0xba35faef,
                0x3f7ffffd,
            ],
            &[0x3d22eae9, 0x42b4453e, 0xadabe329],
        ),
        (
            &[
                0x3f7fff76, 0x3707e52c, 0x3b84b5cd, 0xb707e575, 0x3f800000, 0x00000000, 0xbb84b5cd, 0xb30ce5a5,
                0x3f7fff76,
            ],
            &[0x3e6d9e05, 0x39f35248, 0x00000000],
        ),
        (
            &[
                0xbf7e0595, 0xbdfe17a7, 0x3ab4b01c, 0x3dfe17b7, 0xbf7e05a5, 0x00000000, 0x3ab34ab8, 0x39335779,
                0x3f7ffff0,
            ],
            &[0x3da1c2a2, 0xc32cdf7e, 0x329666c3],
        ),
        (
            &[
                0x397bbee1, 0x3f800000, 0x3432f4fc, 0xbf800000, 0x397bbee1, 0x00000000, 0xae2ffba2, 0xb432f4fc,
                0x3f800000,
            ],
            &[0x372035ea, 0x42b3f8f5, 0x2b9a990f],
        ),
        (
            &[
                0x3ae0c94b, 0x3f03b5f2, 0x3f5b8493, 0xbf7fffa3, 0x3b5a73be, 0x00000000, 0xbb3b5225, 0xbf5b8443,
                0x3f03b622,
            ],
            &[0x426c24f4, 0x42b39e37, 0xb677168a],
        ),
        (
            &[
                0xbf7fcd3f, 0xbd1a9e32, 0x3c35e749, 0x3d1aa0a3, 0xbf7fd148, 0x00000000, 0x3c35c617, 0x39dbbe8d,
                0x3f7ffbf4,
            ],
            &[0x3f22da0e, 0xc331d626, 0x3402b4bc],
        ),
        (
            &[
                0x3f800000, 0x382c9aa0, 0x3432f4fc, 0xb82c9aa0, 0x3f800000, 0x00000000, 0xb432f4fc, 0xacf15161,
                0x3f800000,
            ],
            &[0x372035ea, 0x3b1a85ec, 0xa3e52ee0],
        ),
    ];
    for (input, want) in cases {
        assert_eq!(&bits(&vector::matrix_rotator(rows(input)))[..], *want, "{input:x?}");
    }
}

#[test]
fn rotator_from_axis_angle() {
    let cases: &[(&[u32], &[u32])] = &[
        (&[0x3e99999a, 0xc3b3f333, 0x3f000000, 0x3e99999a], &[0x3e999904, 0x39da2c28, 0xb9828990]),
        (&[0xbfd9999a, 0x44341000, 0x3f000000, 0xbfd9999a], &[0x3fd9995f, 0xbaa288bb, 0xbb8417f4]),
        (&[0x42f6e979, 0x3a83126f, 0x3f000000, 0x42f6e979], &[0x3eb81c0b, 0x3e46fcbe, 0xc2f6e8f7]),
        (&[0xc3b3f333, 0xc2340000, 0x3f000000, 0xc3b3f333], &[0x3c4b5536, 0x391bd9df, 0x3dcb42c0]),
        (&[0x44341000, 0x4a371b00, 0x3f000000, 0x44341000], &[0xbe7ffc0b, 0x34395280, 0xb87bbb80]),
        (&[0x3a83126f, 0x3e99999a, 0x3f000000, 0x3a83126f], &[0xba0813d5, 0x3a60c94a, 0xb5e6b2aa]),
        (&[0xc2340000, 0xbfd9999a, 0x3f000000, 0xc2340000], &[0xbfdb9e9a, 0x3e3ba8c2, 0xc233e2ad]),
        (&[0x4a371b00, 0x42f6e979, 0x3f000000, 0x4a371b00], &[0xbb04f1b1, 0x3b687fb5, 0xc2f025da]),
    ];
    for (input, want) in cases {
        let got = vector::rotator_from_axis_angle(v3(&input[0..3]), f32::from_bits(input[3]));
        assert_eq!(&bits(&got)[..], *want, "{input:x?}");
    }
}

#[test]
fn advance_time() {
    let cases: &[(bool, &[u32], u32, u32)] = &[
        (true, &[0x3e99999a, 0xc3b3f333, 0x3fc00000], 2, 0x3ecccc00),
        (false, &[0xbfd9999a, 0x44341000, 0x3fc00000], 1, 0x3fc00000),
        (true, &[0x42f6e979, 0x3a83126f, 0x3fc00000], 2, 0x3ee9fc00),
        (false, &[0xc3b3f333, 0xc2340000, 0x3fc00000], 1, 0x00000000),
        (true, &[0x44341000, 0x4a371b00, 0x3fc00000], 2, 0x3e800000),
        (false, &[0x3a83126f, 0x3e99999a, 0x3fc00000], 0, 0x3e9a1cac),
        (true, &[0xc2340000, 0xbfd9999a, 0x3fc00000], 2, 0x3fa66660),
        (false, &[0x4a371b00, 0x42f6e979, 0x3fc00000], 1, 0x3fc00000),
    ];
    let result = [AdvanceResult::Default, AdvanceResult::Finished, AdvanceResult::Looped];
    for &(looping, input, want_result, want_time) in cases {
        let (mv, mut time, end) = (f32::from_bits(input[0]), f32::from_bits(input[1]), f32::from_bits(input[2]));
        let got = vector::advance_time(looping, mv, &mut time, end);
        assert_eq!(got, result[want_result as usize], "{looping} {input:x?}");
        assert_eq!(time.to_bits(), want_time, "{looping} {input:x?}");
    }
}

#[test]
fn is_exceeding_max_speed() {
    let cases: &[(&[u32], bool)] = &[
        (&[0x3e99999a, 0xc3b3f333, 0x3f000000, 0x42f6e979], true),
        (&[0xbfd9999a, 0x44341000, 0x3f000000, 0xc3b3f333], true),
        (&[0x42f6e979, 0x3a83126f, 0x3f000000, 0x44341000], false),
        (&[0xc3b3f333, 0xc2340000, 0x3f000000, 0x3a83126f], true),
        (&[0x44341000, 0x4a371b00, 0x3f000000, 0xc2340000], true),
        (&[0x3a83126f, 0x3e99999a, 0x3f000000, 0x4a371b00], false),
        (&[0xc2340000, 0xbfd9999a, 0x3f000000, 0x3e99999a], true),
        (&[0x4a371b00, 0x42f6e979, 0x3f000000, 0xbfd9999a], true),
    ];
    for &(input, want) in cases {
        assert_eq!(vector::is_exceeding_max_speed(v3(&input[0..3]), f32::from_bits(input[3])), want, "{input:x?}");
    }
}

#[test]
fn size_and_distance() {
    assert_eq!(vector::size([3.0, 4.0, 12.0]), 13.0);
    assert_eq!(vector::size2d([3.0, 4.0]), 5.0);
    assert_eq!(vector::dist([1.0, 2.0, 3.0], [4.0, 6.0, 15.0]), 13.0);
    assert_eq!(vector::dist2d([1.0, 2.0], [4.0, 6.0]), 5.0);
    assert_eq!(vector::size_squared([3.0, 4.0, 12.0]), 169.0);
}

#[test]
fn safe_normal_branches() {
    let tol = vector::SMALL_NUMBER;
    // a sum of exactly 1 and a NaN sum keep the vector; below the tolerance gives zero
    assert_eq!(vector::safe_normal([0.6, 0.0, 0.8], tol), [0.6, 0.0, 0.8]);
    assert!(vector::safe_normal([f32::NAN, 0.0, 1.0], tol)[0].is_nan());
    assert_eq!(vector::safe_normal([1e-5, 0.0, 0.0], tol), [0.0; 3]);
    assert_eq!(vector::safe_normal([0.0; 3], tol), [0.0; 3]);
    // the tolerance is the parameter: a sum of 4e-8 passes 1e-8 and not 1e-4
    assert_eq!(vector::safe_normal([2e-4, 0.0, 0.0], vector::KINDA_SMALL_NUMBER), [0.0; 3]);
    let n = vector::safe_normal([3.0, 0.0, 4.0], tol);
    assert!((n[0] - 0.6).abs() < 1e-6 && n[1] == 0.0 && (n[2] - 0.8).abs() < 1e-6);
    // Normal2D has no == 1 shortcut: (0.6, 0.8) goes through inv_sqrt
    assert_eq!(vector::safe_normal2d([0.0, 0.0]), [0.0; 2]);
    assert_eq!(vector::safe_normal2d([1e-5, 0.0]), [0.0; 2]);
    // Normalize keeps a short vector and reports it
    assert_eq!(vector::normalize([1e-5, 0.0, 0.0]), ([1e-5, 0.0, 0.0], false));
    assert!(vector::normalize([3.0, 0.0, 4.0]).1);
    assert_eq!(vector::direction_unit_vector([1.0, 1.0, 1.0], [1.0, 1.0, 1.0]), [0.0; 3]);
}

#[test]
fn clamped_to_max_size_branches() {
    // below the limit the vector is returned as it is; a limit under 1e-4 or NaN gives zero
    assert_eq!(vector::clamped_to_max_size([3.0, 0.0, 4.0], 10.0), [3.0, 0.0, 4.0]);
    assert_eq!(vector::clamped_to_max_size([3.0, 0.0, 4.0], 0.0), [0.0; 3]);
    assert_eq!(vector::clamped_to_max_size([3.0, 0.0, 4.0], f32::NAN), [0.0; 3]);
    let c = vector::clamped_to_max_size([30.0, 0.0, 40.0], 5.0);
    assert!((vector::size(c) - 5.0).abs() < 1e-5);
}

#[test]
fn clamped_to_size_branches() {
    // V x (1 / 50) x 10: the reciprocal 0.02 rounds below, so not (6, 8)
    assert_eq!(vector::clamped_to_size([30.0, 40.0, 0.0], -100.0, 10.0), [5.999_999_5, 7.999_999_5, 0.0]);
    // inside [min, max] the vector still goes through the reciprocal and back: (1, 1) is not kept
    assert_eq!(vector::clamped_to_size([1.0, 1.0, 0.0], 0.0, 10.0), [0.999_999_94, 0.999_999_94, 0.0]);
    assert_eq!(vector::clamped_to_size([3.0, 0.0, 4.0], -100.0, 10.0), [3.0, 0.0, 4.0]);
    // under min the direction is scaled up to it
    assert_eq!(vector::clamped_to_size([3.0, 0.0, 4.0], 10.0, 20.0), [6.0, 0.0, 8.0]);
    // a size at or under 1e-8 has no direction; a NaN size takes min, the zero direction keeps the result 0
    assert_eq!(vector::clamped_to_size([0.0; 3], 5.0, 10.0), [0.0; 3]);
    assert_eq!(vector::clamped_to_size([f32::NAN, 0.0, 0.0], 5.0, 10.0), [0.0; 3]);
}

#[test]
fn quat_rotate_vector_turns_a_quarter() {
    // 90 degrees about Z takes X to Y
    let q = vector::quat_from_axis_angle([0.0, 0.0, 1.0], 90.0);
    let r = vector::quat_rotate_vector(q, [1.0, 0.0, 0.0]);
    assert!(r[0].abs() < 1e-6 && (r[1] - 1.0).abs() < 1e-6 && r[2].abs() < 1e-6, "{r:?}");
    // the identity leaves the vector alone, bit for bit
    assert_eq!(vector::quat_rotate_vector([0.0, 0.0, 0.0, 1.0], [1.5, -2.5, 3.5]), [1.5, -2.5, 3.5]);
}

#[test]
fn degrees() {
    assert_eq!(vector::deg_acos(2.0).to_bits(), (0.0f32).to_bits());
    // a NaN gives -1, so 180 degrees
    assert_eq!(vector::deg_acos(f32::NAN), mvs_math::f32::crt::acosf(-1.0) * vector::RAD_TO_DEG);
    assert_eq!(vector::deg_asin(-3.0), mvs_math::f32::crt::asinf(-1.0) * vector::RAD_TO_DEG);
    assert_eq!(vector::deg_to_rad(180.0), 180.0 * vector::DEG_TO_RAD);
    assert_eq!(vector::deg_atan2(1.0, 0.0), mvs_math::f32::fmath::atan2(1.0, 0.0) * vector::RAD_TO_DEG);
}

#[test]
fn rot_from_x_along_axes() {
    // X along +X is the zero rotator; along +Z a pitch of 90
    assert_eq!(vector::rot_from_x([1.0, 0.0, 0.0]), [0.0, 0.0, 0.0]);
    let up = vector::rot_from_x([0.0, 0.0, 1.0]);
    assert!((up[0] - 90.0).abs() < 1e-3, "{up:?}");
}
