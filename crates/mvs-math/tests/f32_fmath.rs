//! The FMath ports against the beta exe's own results on a few inputs per routine (bit patterns written by
//! `mvs-math-check`'s `exe-fmath golden`, which checks every routine on every float of one argument and on
//! random tuples).

use mvs_math::f32::estimate::inv_sqrt;
use mvs_math::f32::fmath;

fn f(b: &[u32]) -> Vec<f32> {
    b.iter().map(|&x| f32::from_bits(x)).collect()
}

fn bits(v: &[f32]) -> Vec<u32> {
    v.iter().map(|x| x.to_bits()).collect()
}

/// The routine at 142b1bbe0 with p = (1, 0, 0) and o = 0: n (n.x), n the safe normal of v by InvSqrt.
fn plane_port(v: [f32; 3]) -> [f32; 3] {
    let sq = (v[0] * v[0] + v[1] * v[1]) + v[2] * v[2];
    let s = inv_sqrt(sq);
    let n = [v[0] * s, v[1] * s, v[2] * s];
    let d = (0.0 * n[1] + 1.0 * n[0]) + 0.0 * n[2];
    [n[0] * d + 0.0, n[1] * d + 0.0, n[2] * d + 0.0]
}

#[test]
fn atan2() {
    let cases: &[(&[u32], &[u32])] = &[
        (&[0xc3b3f333, 0x3e99999a], &[0xbfc8f48b]),
        (&[0x44341000, 0xbfd9999a], &[0x3fc95d33]),
        (&[0x3a83126f, 0x42f6e979], &[0x3707e576]),
        (&[0xc2340000, 0xc3b3f333], &[0xc04119dc]),
        (&[0x4a371b00, 0x44341000], &[0x3fc907fd]),
        (&[0x3e99999a, 0x3a83126f], &[0x3fc8a2a1]),
        (&[0xbfd9999a, 0xc2340000], &[0xc046a533]),
        (&[0x42f6e979, 0x4a371b00], &[0x382c9aa0]),
    ];
    for (input, want) in cases {
        let i = f(input);
        let got = bits(&[fmath::atan2(i[0], i[1])]);
        assert_eq!(&got[..], *want, "{input:x?}");
    }
}

#[test]
fn fmod() {
    let cases: &[(&[u32], &[u32])] = &[
        (&[0x3e99999a, 0x43b40000], &[0x3e99999a]),
        (&[0xbfd9999a, 0x43b40000], &[0xbfd9999a]),
        (&[0x42f6e979, 0x43b40000], &[0x42f6e979]),
        (&[0xc3b3f333, 0x43b40000], &[0xc3b3f333]),
        (&[0x44341000, 0x43b40000], &[0x3e800000]),
        (&[0x3a83126f, 0x43b40000], &[0x3a83126f]),
        (&[0xc2340000, 0x43b40000], &[0xc2340000]),
        (&[0x4a371b00, 0x43b40000], &[0x42f00000]),
    ];
    for (input, want) in cases {
        let i = f(input);
        let got = bits(&[fmath::fmod(i[0], i[1])]);
        assert_eq!(&got[..], *want, "{input:x?}");
    }
}

#[test]
fn normalize_axis() {
    let cases: &[(&[u32], &[u32])] = &[
        (&[0x3e99999a], &[0x3e99999a]),
        (&[0xbfd9999a], &[0xbfd99a00]),
        (&[0x42f6e979], &[0x42f6e979]),
        (&[0xc3b3f333], &[0x3dccd000]),
        (&[0x44341000], &[0x3e800000]),
        (&[0x3a83126f], &[0x3a83126f]),
        (&[0xc2340000], &[0xc2340000]),
        (&[0x4a371b00], &[0x42f00000]),
    ];
    for (input, want) in cases {
        let i = f(input);
        let got = bits(&[fmath::normalize_axis(i[0])]);
        assert_eq!(&got[..], *want, "{input:x?}");
    }
}

#[test]
fn clamp_axis() {
    let cases: &[(&[u32], &[u32])] = &[
        (&[0x3e99999a], &[0x3e99999a]),
        (&[0xbfd9999a], &[0x43b32666]),
        (&[0x42f6e979], &[0x42f6e979]),
        (&[0xc3b3f333], &[0x3dccd000]),
        (&[0x44341000], &[0x3e800000]),
        (&[0x3a83126f], &[0x3a83126f]),
        (&[0xc2340000], &[0x439d8000]),
        (&[0x4a371b00], &[0x42f00000]),
    ];
    for (input, want) in cases {
        let i = f(input);
        let got = bits(&[fmath::clamp_axis(i[0])]);
        assert_eq!(&got[..], *want, "{input:x?}");
    }
}

#[test]
fn rotator_vector() {
    let cases: &[(&[u32], &[u32])] = &[
        (&[0x3e99999a, 0xc3b3f333, 0x3a83126f], &[0x3f7fff00, 0x3ae4cf2a, 0x3bab9272]),
        (&[0xbfd9999a, 0x44341000, 0xc2340000], &[0x3f7fe288, 0x3b8ee9fb, 0xbcf3069f]),
        (&[0x42f6e979, 0x3a83126f, 0x4a371b00], &[0xbf0d21c2, 0xb7216de7, 0x3f55954f]),
        (&[0xc3b3f333, 0xc2340000, 0x3e99999a], &[0x3f3504e0, 0xbf3504e1, 0x3ae4cff8]),
        (&[0x44341000, 0x4a371b00, 0xbfd9999a], &[0xbeffff5e, 0x3f5db34d, 0x3b8efa17]),
        (&[0x3a83126f, 0x3e99999a, 0x42f6e979], &[0x3f7fff1a, 0x3bab9272, 0x379268a9]),
        (&[0xc2340000, 0xbfd9999a, 0xc3b3f333], &[0x3f34f08d, 0xbcabd860, 0xbf3504f3]),
        (&[0x4a371b00, 0x42f6e979, 0x44341000], &[0x3e8d21c1, 0xbed5954d, 0x3f5db3d8]),
    ];
    for (input, want) in cases {
        let i = f(input);
        let got = bits(&fmath::rotator_vector([i[0], i[1], i[2]]));
        assert_eq!(&got[..], *want, "{input:x?}");
    }
}

#[test]
fn rotator_quaternion() {
    let cases: &[(&[u32], &[u32])] = &[
        (&[0x3e99999a, 0xc3b3f333, 0x3a83126f], &[0x36d8240f, 0x3b2b92b6, 0xba64ce43, 0xbf7fffc1]),
        (&[0xbfd9999a, 0x44341000, 0xc2340000], &[0x3ec3e587, 0x3c6e3a62, 0xbb6ff800, 0x3f6c7d60]),
        (&[0x42f6e979, 0x3a83126f, 0x4a371b00], &[0xbed20533, 0xbee17699, 0xbf43413d, 0x3e728505]),
        (&[0xc3b3f333, 0xc2340000, 0x3e99999a], &[0x3b34672b, 0xb94cf330, 0x3ec3ef2b, 0xbf6c8314]),
        (&[0x44341000, 0x4a371b00, 0xbfd9999a], &[0x3c187a64, 0x3c409e7b, 0x3f5dae85, 0x3efff4f4]),
        (&[0x3a83126f, 0x3e99999a, 0x42f6e979], &[0xbf6175f1, 0xbb17605f, 0x3aa186ae, 0x3ef28313]),
        (&[0xc2340000, 0xbfd9999a, 0xc3b3f333], &[0xbb9f9a56, 0xbec3eb1c, 0x3c5b1446, 0xbf6c7d02]),
        (&[0x4a371b00, 0x42f6e979, 0x44341000], &[0x3f431f65, 0xbed2837f, 0x3ee100b2, 0x3e743754]),
    ];
    for (input, want) in cases {
        let i = f(input);
        let got = bits(&fmath::rotator_quaternion([i[0], i[1], i[2]]));
        assert_eq!(&got[..], *want, "{input:x?}");
    }
}

#[test]
fn vector_rotation() {
    let cases: &[(&[u32], &[u32])] = &[
        (&[0x3e99999a, 0xc3b3f333, 0x3a83126f], &[0x3926eeb0, 0xc2b3e78c, 0x00000000]),
        (&[0xbfd9999a, 0x44341000, 0xc2340000], &[0xc064ce3b, 0x42b4453e, 0x00000000]),
        (&[0x42f6e979, 0x3a83126f, 0x4a371b00], &[0x42b3fecb, 0x39f35249, 0x00000000]),
        (&[0xc3b3f333, 0xc2340000, 0x3e99999a], &[0x3d421cc9, 0xc32cdf7d, 0x00000000]),
        (&[0x44341000, 0x4a371b00, 0xbfd9999a], &[0xb8082dd4, 0x42b3f8f5, 0x00000000]),
        (&[0x3a83126f, 0x3e99999a, 0x42f6e979], &[0x42b3b8b6, 0x42b39e37, 0x00000000]),
        (&[0xc2340000, 0xbfd9999a, 0xc3b3f333], &[0xc2a5bc68, 0xc331d626, 0x00000000]),
        (&[0x4a371b00, 0x42f6e979, 0x44341000], &[0x3c615fd8, 0x3b1a85ec, 0x00000000]),
    ];
    for (input, want) in cases {
        let i = f(input);
        let got = bits(&fmath::vector_rotation([i[0], i[1], i[2]]));
        assert_eq!(&got[..], *want, "{input:x?}");
    }
}

#[test]
fn quat_rotator() {
    let cases: &[(&[u32], &[u32])] = &[
        (&[0x399d4952, 0xbeb844d0, 0x3f000000, 0xbe800000], &[0xc125970b, 0xc2385371, 0x41cf555f]),
        (&[0xbaded28a, 0x3f38624e, 0x3f000000, 0xbe800000], &[0x41a80727, 0xc31ad7bb, 0xc2b9f53c]),
        (&[0x3dfcd681, 0x358637be, 0x3f000000, 0xbe800000], &[0x40e2ee9e, 0xc1d48524, 0x4069268f]),
        (&[0xbeb844d0, 0xbd3851ec, 0x3f000000, 0xbe800000], &[0xc1b3dc03, 0xc1bd8627, 0xc1260b75]),
        (&[0x3f38624e, 0x453b8001, 0x3f000000, 0xbe800000], &[0x42b40000, 0x4333fc7a, 0xc21930e0]),
        (&[0x358637be, 0x399d4952, 0x3f000000, 0xbe800000], &[0x3c0dc8e0, 0xc1d48535, 0xbc8c9350]),
        (&[0xbd3851ec, 0xbaded28a, 0x3f000000, 0xbe800000], &[0xc0282ffc, 0xc1d46900, 0xbf9924ab]),
        (&[0x453b8001, 0x3dfcd681, 0x3f000000, 0xbe800000], &[0x42b40000, 0x42b3ed66, 0xc2b4177c]),
    ];
    for (input, want) in cases {
        let i = f(input);
        let got = bits(&fmath::quat_rotator([i[0], i[1], i[2], i[3]]));
        assert_eq!(&got[..], *want, "{input:x?}");
    }
}

#[test]
fn finterp_to() {
    let cases: &[(&[u32], &[u32])] = &[
        (&[0x3e99999a, 0xc3b3f333, 0x3c888889, 0x41000000], &[0xc23ee81c]),
        (&[0xbfd9999a, 0x44341000, 0x3c888889, 0x41000000], &[0x42bd1eba]),
        (&[0x42f6e979, 0x3a83126f, 0x3c888889, 0x41000000], &[0x42d5fd9c]),
        (&[0xc3b3f333, 0xc2340000, 0x3c888889, 0x41000000], &[0xc39ef4e8]),
        (&[0x44341000, 0x4a371b00, 0x3c888889, 0x41000000], &[0x48c39e08]),
        (&[0x3a83126f, 0x3e99999a, 0x3c888889, 0x41000000], &[0x3d2763cf]),
        (&[0xc2340000, 0xbfd9999a, 0x3c888889, 0x41000000], &[0xc21ce81b]),
        (&[0x4a371b00, 0x42f6e979, 0x3c888889, 0x41000000], &[0x4a1eb142]),
    ];
    for (input, want) in cases {
        let i = f(input);
        let got = bits(&[fmath::finterp_to(i[0], i[1], i[2], i[3])]);
        assert_eq!(&got[..], *want, "{input:x?}");
    }
}

#[test]
fn finterp_constant_to() {
    let cases: &[(&[u32], &[u32])] = &[
        (&[0x3e99999a, 0xc3b3f333, 0x3c888889, 0x43960000], &[0xc0966667]),
        (&[0xbfd9999a, 0x44341000, 0x3c888889, 0x43960000], &[0x40533335]),
        (&[0x42f6e979, 0x3a83126f, 0x3c888889, 0x43960000], &[0x42ece979]),
        (&[0xc3b3f333, 0xc2340000, 0x3c888889, 0x43960000], &[0xc3b17333]),
        (&[0x44341000, 0x4a371b00, 0x3c888889, 0x43960000], &[0x44355000]),
        (&[0x3a83126f, 0x3e99999a, 0x3c888889, 0x43960000], &[0x3e99999a]),
        (&[0xc2340000, 0xbfd9999a, 0x3c888889, 0x43960000], &[0xc2200000]),
        (&[0x4a371b00, 0x42f6e979, 0x3c888889, 0x43960000], &[0x4a371aec]),
    ];
    for (input, want) in cases {
        let i = f(input);
        let got = bits(&[fmath::finterp_constant_to(i[0], i[1], i[2], i[3])]);
        assert_eq!(&got[..], *want, "{input:x?}");
    }
}

#[test]
fn rinterp_to() {
    let cases: &[(&[u32], &[u32])] = &[
        (
            &[0x3e99999a, 0xc3b3f333, 0x3a83126f, 0xc3b3f333, 0x3e99999a, 0xbfd9999a],
            &[0x3e911145, 0x3deef000, 0xbe102000],
        ),
        (
            &[0xbfd9999a, 0x44341000, 0xc2340000, 0x44341000, 0xbfd9999a, 0x42f6e979],
            &[0xbfc4cd00, 0x3db34000, 0xc1f7b230],
        ),
        (
            &[0x42f6e979, 0x3a83126f, 0x4a371b00, 0x3a83126f, 0x42f6e979, 0xc3b3f333],
            &[0x42e2560f, 0x41249f68, 0x42dc0000],
        ),
        (
            &[0xc3b3f333, 0xc2340000, 0x3e99999a, 0xc2340000, 0xc3b3f333, 0x44341000],
            &[0xc06a2200, 0xc224f778, 0x3e97779a],
        ),
        (
            &[0x44341000, 0x4a371b00, 0xbfd9999a, 0x4a371b00, 0x44341000, 0x3a83126f],
            &[0x4123aac0, 0x42dc0000, 0xbfc77500],
        ),
        (
            &[0x3a83126f, 0x3e99999a, 0x42f6e979, 0x3e99999a, 0x3a83126f, 0xc2340000],
            &[0x3cd44f32, 0x3e8cd79a, 0x42dad604],
        ),
        (
            &[0xc2340000, 0xbfd9999a, 0xc3b3f333, 0xbfd9999a, 0xc2340000, 0x4a371b00],
            &[0xc2259110, 0xc0a9ddc0, 0x412199a0],
        ),
        (
            &[0x4a371b00, 0x42f6e979, 0x44341000, 0x42f6e979, 0x4a371b00, 0x3e99999a],
            &[0x42f08000, 0x42f65424, 0x3e822000],
        ),
    ];
    for (input, want) in cases {
        let i = f(input);
        let got = bits(&fmath::rinterp_to([i[0], i[1], i[2]], [i[3], i[4], i[5]], 1.0 / 60.0, 5.0));
        assert_eq!(&got[..], *want, "{input:x?}");
    }
}

#[test]
fn vinterp_to() {
    let cases: &[(&[u32], &[u32])] = &[
        (
            &[0x3e99999a, 0xc3b3f333, 0x3a83126f, 0xc3b3f333, 0x3e99999a, 0xbfd9999a],
            &[0xc1edbbbc, 0xc3a4f111, 0xbe1020c5],
        ),
        (
            &[0xbfd9999a, 0x44341000, 0xc2340000, 0x44341000, 0xbfd9999a, 0x42f6e979],
            &[0x4269d99a, 0x4425059a, 0xc1f7b22d],
        ),
        (
            &[0x42f6e979, 0x3a83126f, 0x4a371b00, 0x3a83126f, 0x42f6e979, 0xc3b3f333],
            &[0x42e2560f, 0x41249f68, 0x4a27d848],
        ),
        (
            &[0xc3b3f333, 0xc2340000, 0x3e99999a, 0xc2340000, 0xc3b3f333, 0x44341000],
            &[0xc3a6d444, 0xc28e7bbc, 0x42712eef],
        ),
        (
            &[0x44341000, 0x4a371b00, 0xbfd9999a, 0x4a371b00, 0x44341000, 0x3a83126f],
            &[0x4874c90f, 0x4a27d9b0, 0xbfc774bd],
        ),
        (
            &[0x3a83126f, 0x3e99999a, 0x42f6e979, 0x3e99999a, 0x3a83126f, 0xc2340000],
            &[0x3cd44f32, 0x3e8cd7b9, 0x42dad604],
        ),
        (
            &[0xc2340000, 0xbfd9999a, 0xc3b3f333, 0xbfd9999a, 0xc2340000, 0x4a371b00],
            &[0xc2259111, 0xc0a9ddde, 0x4873d186],
        ),
        (
            &[0x4a371b00, 0x42f6e979, 0x44341000, 0x42f6e979, 0x4a371b00, 0x3e99999a],
            &[0x4a27d8e9, 0x4874404b, 0x44251044],
        ),
    ];
    for (input, want) in cases {
        let i = f(input);
        let got = bits(&fmath::vinterp_to([i[0], i[1], i[2]], [i[3], i[4], i[5]], 1.0 / 60.0, 5.0));
        assert_eq!(&got[..], *want, "{input:x?}");
    }
}

#[test]
fn map_range_clamped() {
    let cases: &[(&[u32], &[u32])] = &[
        (&[0x3e99999a, 0xc0400000, 0x437a0000, 0x3f800000, 0x3e800000], &[0x3f7d7ee3]),
        (&[0xbfd9999a, 0xc0400000, 0x437a0000, 0x3f800000, 0x3e800000], &[0x3f7f0371]),
        (&[0x42f6e979, 0xc0400000, 0x437a0000, 0x3f800000, 0x3e800000], &[0x3f20088c]),
        (&[0xc3b3f333, 0xc0400000, 0x437a0000, 0x3f800000, 0x3e800000], &[0x3f800000]),
        (&[0x44341000, 0xc0400000, 0x437a0000, 0x3f800000, 0x3e800000], &[0x3e800000]),
        (&[0x3a83126f, 0xc0400000, 0x437a0000, 0x3f800000, 0x3e800000], &[0x3f7db8fa]),
        (&[0xc2340000, 0xc0400000, 0x437a0000, 0x3f800000, 0x3e800000], &[0x3f800000]),
        (&[0x4a371b00, 0xc0400000, 0x437a0000, 0x3f800000, 0x3e800000], &[0x3e800000]),
    ];
    for (input, want) in cases {
        let i = f(input);
        let got = bits(&[fmath::map_range_clamped(i[0], i[1], i[2], i[3], i[4])]);
        assert_eq!(&got[..], *want, "{input:x?}");
    }
}

#[test]
fn rotate_angle_axis() {
    let cases: &[(&[u32], &[u32])] = &[
        (&[0x3e99999a, 0xc3b3f333, 0x3a83126f, 0x3e99999a], &[0x3fe75d58, 0xc3b3f268, 0xbf90986f]),
        (&[0xbfd9999a, 0x44341000, 0xc2340000, 0xbfd9999a], &[0x41762795, 0x4433cb08, 0xc2674106]),
        (&[0x42f6e979, 0x3a83126f, 0x4a371b00, 0x42f6e979], &[0x4a08582f, 0xc9b74f57, 0x49a1b4a0]),
        (&[0xc3b3f333, 0xc2340000, 0x3e99999a, 0xc3b3f333], &[0xc3b3eb1c, 0xc23602ef, 0x3e81548f]),
        (&[0x44341000, 0x4a371b00, 0xbfd9999a, 0x44341000], &[0xc6185c42, 0x4a371a98, 0x45f55e4a]),
        (&[0x3a83126f, 0x3e99999a, 0x42f6e979, 0x3a83126f], &[0x3a8285e2, 0x3e98f026, 0x42f6e979]),
        (&[0xc2340000, 0xbfd9999a, 0xc3b3f333, 0xc2340000], &[0xc2b03f9d, 0xc300705a, 0xc3a3c746]),
        (&[0x4a371b00, 0x42f6e979, 0x44341000, 0x4a371b00], &[0x47ed50fa, 0x49fdc275, 0x4a03cf9d]),
    ];
    for (input, want) in cases {
        let i = f(input);
        let got = bits(&fmath::rotate_angle_axis([i[0], i[1], i[2]], i[3], [0.6, 0.0, 0.8]));
        assert_eq!(&got[..], *want, "{input:x?}");
    }
}

#[test]
fn plane() {
    let cases: &[(&[u32], &[u32])] = &[
        (&[0x3e99999a, 0xc3b3f333, 0x3f000000], &[0x353a8442, 0xba5a8372, 0x359b6e37]),
        (&[0xbfd9999a, 0x44341000, 0x3f000000], &[0x36baee0a, 0xbb1aaee3, 0xb5dbeade]),
        (&[0x42f6e979, 0x3a83126f, 0x3f000000], &[0x3f7ffeec, 0x3707e4e3, 0x3b84b585]),
        (&[0xc3b3f333, 0xc2340000, 0x3f000000], &[0x3f7c0f14, 0x3dfc2102, 0xbab34aac]),
        (&[0x44341000, 0x4a371b00, 0x3f000000], &[0x33778fdc, 0x397bbee1, 0x2e2ffba2]),
        (&[0x3a83126f, 0x3e99999a, 0x3f000000], &[0x364560e2, 0x3a674d88, 0x3ac0c09c]),
        (&[0xc2340000, 0xbfd9999a, 0x3f000000], &[0x3f7f9a88, 0x3d1a7f8b, 0xbc35c339]),
        (&[0x4a371b00, 0x42f6e979, 0x3f000000], &[0x3f800000, 0x382c9aa0, 0x3432f4fc]),
    ];
    for (input, want) in cases {
        let i = f(input);
        let got = bits(&plane_port([i[0], i[1], i[2]]));
        assert_eq!(&got[..], *want, "{input:x?}");
    }
}
