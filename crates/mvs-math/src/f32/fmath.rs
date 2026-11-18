//! UE 4.26's FMath routines and the rotator / quaternion conversions built on them, ported from the exe instruction
//! by instruction: the same float operations in the same order, each comparison with the branch the exe takes on a
//! NaN (`!(a >= b)` where the exe jumps on "below"), minss / maxss as `min_ss` / `max_ss`. Rust never fuses or
//! reorders float operations, so the bits are the exe's on every platform. Rotators are [pitch, yaw, roll] in
//! degrees, quaternions [x, y, z, w].

// `!(a >= b)` is the exe's jump on "below or unordered": the NaN branch is the point.
#![allow(clippy::neg_cmp_op_on_partial_ord)]

/// 1e-8 (SMALL_NUMBER)
const SMALL_NUMBER: f32 = f32::from_bits(0x322b_cc77);
/// 1e-4 (KINDA_SMALL_NUMBER)
const KINDA_SMALL_NUMBER: f32 = f32::from_bits(0x38d1_b717);
/// 2^23: floats from here up have no fraction
const NON_FRACTIONAL: f32 = f32::from_bits(0x4b00_0000);
const PI: f32 = f32::from_bits(0x4049_0fdb);
const HALF_PI: f32 = f32::from_bits(0x3fc9_0fdb);
const TWO_PI: f32 = f32::from_bits(0x40c9_0fdb);
const INV_TWO_PI: f32 = f32::from_bits(0x3e22_f983);
/// pi / 180 (DegreesToRadians)
const DEG_TO_RAD: f32 = f32::from_bits(0x3c8e_fa35);
/// pi / 360 (GlobalVectorConstants::DEG_TO_RAD_HALF)
const DEG_TO_RAD_HALF: f32 = f32::from_bits(0x3c0e_fa35);
/// 180 / pi (RAD_TO_DEG)
const RAD_TO_DEG: f32 = f32::from_bits(0x4265_2ee0);

/// minss / minps: the second operand unless the first is smaller (so a NaN in either gives the second).
#[inline]
fn min_ss(a: f32, b: f32) -> f32 {
    if a < b { a } else { b }
}

/// maxss / maxps: the second operand unless the first is larger.
#[inline]
fn max_ss(a: f32, b: f32) -> f32 {
    if a > b { a } else { b }
}

/// cvttss2si / cvttps2dq: toward zero; NaN and out of range give i32::MIN.
#[inline]
fn cvttss2si(v: f32) -> i32 {
    if v.is_nan() || v >= 2_147_483_648.0 || v < -2_147_483_648.0 { i32::MIN } else { v as i32 }
}

/// cvtss2si / cvtps2dq under the default MXCSR: to nearest, ties to even; NaN and out of range give i32::MIN.
#[inline]
fn cvtss2si(v: f32) -> i32 {
    if v.is_nan() || v >= 2_147_483_648.0 || v < -2_147_483_648.0 {
        return i32::MIN;
    }
    let a = v.abs();
    // Below 2^23, adding and taking away 2^23 rounds to an integer, ties to even; above, v is one already.
    let r = if a < NON_FRACTIONAL { (a + NON_FRACTIONAL) - NON_FRACTIONAL } else { a };
    let r = r as i64;
    (if v < 0.0 { -r } else { r }) as i32
}

/// truncf (the CRT's, exact): toward zero, keeping the sign of zero.
#[inline]
fn truncf(v: f32) -> f32 {
    let bits = v.to_bits();
    let e = ((bits >> 23) & 0xff) as i32 - 127;
    if e >= 23 {
        return v;
    }
    if e < 0 {
        return f32::from_bits(bits & 0x8000_0000);
    }
    f32::from_bits(bits & !(0x7f_ffff >> e))
}

/// FMath::Atan2 (1412c8e50): a 4.26 polynomial in min / max of |x|, |y|, folded into the octant.
pub fn atan2(y: f32, x: f32) -> f32 {
    const C: [f32; 6] = [
        f32::from_bits(0x3bec_5a11),
        f32::from_bits(0x3d0f_9abd),
        f32::from_bits(0x3da7_45af),
        f32::from_bits(0x3e08_f4dd),
        f32::from_bits(0x3e4b_54ca),
        f32::from_bits(0x3eaa_9fbe),
    ];
    let ax = x.abs();
    let ay = y.abs();
    let swapped = ay > ax;
    let (t0, t1) = if swapped { (ay, ax) } else { (ax, ay) };
    // ucomiss / jne: zero and NaN both return 0
    if t0 == 0.0 || t0.is_nan() {
        return 0.0;
    }
    let r = t1 / t0;
    let s = r * r;
    let mut p = s * C[0] - C[1];
    p = p * s + C[2];
    p = p * s - C[3];
    p = p * s + C[4];
    p = p * s - C[5];
    p = p * s + 1.0;
    let mut t = r * p;
    if swapped {
        t = HALF_PI - t;
    }
    if !(x >= 0.0) {
        t = PI - t;
    }
    if !(y >= 0.0) {
        t = -t;
    }
    t
}

/// FMath::Fmod (1412ce930): |y| <= 1e-8 gives 0; x - y trunc(x / y), clamped to [-|y|, |y|].
pub fn fmod(x: f32, y: f32) -> f32 {
    let ay = y.abs();
    if !(ay > SMALL_NUMBER) {
        return 0.0;
    }
    let mut div = x / y;
    if !(div.abs() >= NON_FRACTIONAL) {
        div = truncf(div);
    }
    let mut product = div * y;
    if product.abs() > x.abs() {
        product = x;
    }
    let r = x - product;
    if !(r >= -ay) {
        return -ay;
    }
    min_ss(r, ay)
}

/// FRotator::ClampAxis (142b12900): the angle in [0, 360).
pub fn clamp_axis(angle: f32) -> f32 {
    let v = fmod(angle, 360.0);
    if !(v >= 0.0) { v + 360.0 } else { v }
}

/// FRotator::NormalizeAxis (142b2a8a0): the angle in (-180, 180].
pub fn normalize_axis(angle: f32) -> f32 {
    let v = clamp_axis(angle);
    if v > 180.0 { v - 360.0 } else { v }
}

/// FMath::SinCos, inline in FRotator::Vector (141374280) and FVector::RotateAngleAxis (14136dfb0): the angle less
/// the nearest 2 pi multiple (trunc(x / 2pi +- 0.5)), folded into [-pi/2, pi/2], then an 11th-degree sine and a
/// 10th-degree cosine polynomial. (sin, cos).
pub fn sin_cos(value: f32) -> (f32, f32) {
    let mut q = value * INV_TWO_PI;
    q = if !(value >= 0.0) { q - 0.5 } else { q + 0.5 };
    let q = cvttss2si(q) as f32;
    let mut y = value - q * TWO_PI;
    let sign;
    if y > HALF_PI {
        y = PI - y;
        sign = -1.0;
    } else if !(y >= -HALF_PI) {
        y = -PI - y;
        sign = -1.0;
    } else {
        sign = 1.0;
    }
    let y2 = y * y;
    let mut s = f32::from_bits(0x3638_b88e) - y2 * f32::from_bits(0x32cd_365b);
    s = s * y2 - f32::from_bits(0x3950_0bf1);
    s = s * y2 + f32::from_bits(0x3c08_8886);
    s = s * y2 - f32::from_bits(0x3e2a_aaab);
    s = s * y2 + 1.0;
    let mut c = f32::from_bits(0x37cf_b4c2) - y2 * f32::from_bits(0x348b_dd11);
    c = c * y2 - f32::from_bits(0x3ab6_09aa);
    c = c * y2 + f32::from_bits(0x3d2a_aaa3);
    c = c * y2 - 0.5;
    c = c * y2 + 1.0;
    (s * y, c * sign)
}

/// FRotator::Vector (141374280): the unit direction of a rotator, pitch and yaw through Fmod(.., 360) first.
pub fn rotator_vector(r: [f32; 3]) -> [f32; 3] {
    let (sp, cp) = sin_cos(fmod(r[0], 360.0) * DEG_TO_RAD);
    let (sy, cy) = sin_cos(fmod(r[1], 360.0) * DEG_TO_RAD);
    [cp * cy, cp * sy, sp]
}

/// FVector::RotateAngleAxis (14136dfb0): v turned by `angle_deg` about the unit `axis` (the angle is not wrapped).
pub fn rotate_angle_axis(v: [f32; 3], angle_deg: f32, axis: [f32; 3]) -> [f32; 3] {
    let (s, c) = sin_cos(angle_deg * DEG_TO_RAD);
    let [ax, ay, az] = axis;
    let (xx, yy, zz) = (ax * ax, ay * ay, az * az);
    let (xy, yz, zx) = (ax * ay, ay * az, az * ax);
    let (xs, ys, zs) = (ax * s, ay * s, az * s);
    let omc = 1.0 - c;
    [
        ((omc * xx + c) * v[0] + (omc * xy - zs) * v[1]) + (omc * zx + ys) * v[2],
        ((omc * xy + zs) * v[0] + (omc * yy + c) * v[1]) + (omc * yz - xs) * v[2],
        ((omc * zx - ys) * v[0] + (omc * yz + xs) * v[1]) + (omc * zz + c) * v[2],
    ]
}

/// FVector::Rotation (14136e340): yaw and pitch of a direction by the Atan2 polynomial, roll 0.
pub fn vector_rotation(v: [f32; 3]) -> [f32; 3] {
    let yaw = atan2(v[1], v[0]) * RAD_TO_DEG;
    let pitch = atan2(v[2], (v[0] * v[0] + v[1] * v[1]).sqrt()) * RAD_TO_DEG;
    [pitch, yaw, 0.0]
}

/// FMath::FastAsin, inline in FQuat::Rotator (14136e550): a 7th-degree polynomial in |x| times sqrt(1 - |x|).
pub fn fast_asin(value: f32) -> f32 {
    let a = value.abs();
    let root = max_ss(1.0 - a, 0.0).sqrt();
    let mut p = f32::from_bits(0x3bda_90c5) - a * f32::from_bits(0x3aa5_7a2c);
    p = p * a - f32::from_bits(0x3c8b_fc66);
    p = p * a + f32::from_bits(0x3cfd_10f8);
    p = p * a - f32::from_bits(0x3d4d_8392);
    p = p * a + f32::from_bits(0x3db6_3a9e);
    p = p * a - f32::from_bits(0x3e5b_bfca);
    // HALF_PI here is 0x3fc90fda, one below the float nearest pi / 2
    let half_pi = f32::from_bits(0x3fc9_0fda);
    p = p * a + half_pi;
    let t = root * p;
    if !(value >= 0.0) { t - half_pi } else { half_pi - t }
}

/// FQuat::Rotator (14136e550): pitch by FastAsin away from the poles, +-90 within 0.4999995 of them (roll then
/// from the yaw and Atan2(x, w), normalised).
pub fn quat_rotator(q: [f32; 4]) -> [f32; 3] {
    const POLE: f32 = f32::from_bits(0x3eff_ffef);
    let [x, y, z, w] = q;
    let singularity = x * z - y * w;
    let yaw_y = (y * x + y * x) + (w * z + w * z);
    let yaw_x = 1.0 - ((y * y + y * y) + (z * z + z * z));
    if !(singularity >= -POLE) {
        let yaw = atan2(yaw_y, yaw_x) * RAD_TO_DEG;
        let roll = normalize_axis(-yaw - atan2(x, w) * 2.0 * RAD_TO_DEG);
        [-90.0, yaw, roll]
    } else if singularity > POLE {
        let yaw = atan2(yaw_y, yaw_x) * RAD_TO_DEG;
        let roll = normalize_axis(yaw - atan2(x, w) * 2.0 * RAD_TO_DEG);
        [90.0, yaw, roll]
    } else {
        let pitch = fast_asin(singularity + singularity) * RAD_TO_DEG;
        let yaw = atan2(yaw_y, yaw_x) * RAD_TO_DEG;
        let roll = atan2(y * z * -2.0 - (x * w + x * w), 1.0 - ((y * y + y * y) + (x * x + x * x))) * RAD_TO_DEG;
        [pitch, yaw, roll]
    }
}

/// VectorMod(x, 360), one lane: x - 360 trunc(x / 360) (x / 360 itself from 2^23 up), clamped to [-360, 360].
fn mod_360(x: f32) -> f32 {
    let div = x / 360.0;
    let truncated = cvttss2si(div) as f32;
    let whole = if NON_FRACTIONAL <= div.abs() { div } else { truncated };
    max_ss(-360.0, min_ss(x - whole * 360.0, 360.0))
}

/// VectorNormalizeRotator, one lane: the angle in (-180, 180].
fn normalize_rotator_lane(x: f32) -> f32 {
    let v0 = mod_360(x);
    let v2 = if 0.0 <= v0 { v0 } else { v0 + 360.0 };
    if 180.0 < v2 { v2 - 360.0 } else { v2 }
}

/// VectorSinCos, one lane (packed, inline in FRotator::Quaternion 14136a6d0): the angle less 2 pi round(x / 2pi)
/// (cvtps2dq), folded by |x| > pi/2, the same polynomials as `sin_cos`. (sin, cos).
pub fn vector_sin_cos(angle: f32) -> (f32, f32) {
    let quotient = cvtss2si(INV_TWO_PI * angle) as f32;
    let x = angle - quotient * TWO_PI;
    let c = f32::from_bits(PI.to_bits() | (x.to_bits() & 0x8000_0000));
    let reflected = c - x;
    let fold = HALF_PI < x.abs();
    let x = if fold { reflected } else { x };
    let sign = if fold { -1.0 } else { 1.0 };
    let x2 = x * x;
    let mut s = f32::from_bits(0xb2cd_365b) * x2 + f32::from_bits(0x3638_b88e);
    s = s * x2 + f32::from_bits(0xb950_0bf1);
    s = s * x2 + f32::from_bits(0x3c08_8886);
    s = s * x2 + f32::from_bits(0xbe2a_aaab);
    s = s * x2 + 1.0;
    let mut co = f32::from_bits(0xb48b_dd11) * x2 + f32::from_bits(0x37cf_b4c2);
    co = co * x2 + f32::from_bits(0xbab6_09aa);
    co = co * x2 + f32::from_bits(0x3d2a_aaa3);
    co = co * x2 - 0.5;
    co = co * x2 + 1.0;
    (s * x, co * sign)
}

/// FRotator::Quaternion (14136a6d0): half angles from VectorMod360 x pi/360, VectorSinCos, then the packed products.
pub fn rotator_quaternion(r: [f32; 3]) -> [f32; 4] {
    let (sp, cp) = vector_sin_cos(mod_360(r[0]) * DEG_TO_RAD_HALF);
    let (sy, cy) = vector_sin_cos(mod_360(r[1]) * DEG_TO_RAD_HALF);
    let (sr, cr) = vector_sin_cos(mod_360(r[2]) * DEG_TO_RAD_HALF);
    [
        (sy * sp) * cr + -((cy * cp) * sr),
        -((cy * sp) * cr) + -((sy * cp) * sr),
        (sy * cp) * cr + -((cy * sp) * sr),
        (cy * cp) * cr + (sy * sp) * sr,
    ]
}

/// FMath::FInterpTo (14135cc10).
pub fn finterp_to(current: f32, target: f32, delta_time: f32, interp_speed: f32) -> f32 {
    if !(interp_speed > 0.0) {
        return target;
    }
    let dist = target - current;
    if !(dist * dist >= SMALL_NUMBER) {
        return target;
    }
    let alpha = delta_time * interp_speed;
    let alpha = if !(alpha >= 0.0) { 0.0 } else { min_ss(alpha, 1.0) };
    alpha * dist + current
}

/// FMath::FInterpConstantTo (14135cbb0): no test of the speed.
pub fn finterp_constant_to(current: f32, target: f32, delta_time: f32, interp_speed: f32) -> f32 {
    let dist = target - current;
    if !(dist * dist >= SMALL_NUMBER) {
        return target;
    }
    let step = delta_time * interp_speed;
    let delta = if !(dist >= -step) { -step } else { min_ss(step, dist) };
    delta + current
}

/// FMath::VInterpTo (141373640).
pub fn vinterp_to(current: [f32; 3], target: [f32; 3], delta_time: f32, interp_speed: f32) -> [f32; 3] {
    if !(interp_speed > 0.0) {
        return target;
    }
    let d = [target[0] - current[0], target[1] - current[1], target[2] - current[2]];
    if !((d[0] * d[0] + d[1] * d[1]) + d[2] * d[2] >= KINDA_SMALL_NUMBER) {
        return target;
    }
    let alpha = delta_time * interp_speed;
    let alpha = if !(alpha >= 0.0) { 0.0 } else { min_ss(alpha, 1.0) };
    [d[0] * alpha + current[0], d[1] * alpha + current[1], d[2] * alpha + current[2]]
}

/// FMath::RInterpTo (14136ab80): the normalised difference times clamp(dt speed, 0, 1), the sum normalised.
pub fn rinterp_to(current: [f32; 3], target: [f32; 3], delta_time: f32, interp_speed: f32) -> [f32; 3] {
    // ucomiss / jne: a NaN on either side counts as equal
    let same = |a: f32, b: f32| a == b || a.is_nan() || b.is_nan();
    if same(delta_time, 0.0) || (0..3).all(|i| same(current[i], target[i])) {
        return current;
    }
    if !(interp_speed > 0.0) {
        return target;
    }
    let alpha = delta_time * interp_speed;
    let delta = [0, 1, 2].map(|i| normalize_rotator_lane(target[i] - current[i]));
    // FRotator::IsNearlyZero normalises again before comparing with 1e-4
    if !delta.iter().any(|&d| KINDA_SMALL_NUMBER < normalize_rotator_lane(d).abs()) {
        return target;
    }
    let alpha = if !(alpha >= 0.0) { 0.0 } else { min_ss(alpha, 1.0) };
    [0, 1, 2].map(|i| normalize_rotator_lane(delta[i] * alpha + current[i]))
}

/// FMath::Lerp as the Lerp node runs it (142f71510): (b - a) alpha + a.
pub fn lerp(a: f32, b: f32, alpha: f32) -> f32 {
    (b - a) * alpha + a
}

/// UKismetMathLibrary::MapRangeClamped (142b29590).
pub fn map_range_clamped(value: f32, in_a: f32, in_b: f32, out_a: f32, out_b: f32) -> f32 {
    let range = in_b - in_a;
    let p = if range.abs() > SMALL_NUMBER {
        let t = (value - in_a) / range;
        if !(t >= 0.0) { 0.0 } else { min_ss(t, 1.0) }
    } else if !(value >= in_b) {
        0.0
    } else {
        1.0
    };
    (out_b - out_a) * p + out_a
}

/// FMath::FloorToInt as the Floor node runs it (142f6bde0): cvtss2si(2x - 0.5) >> 1.
pub fn floor_to_int(x: f32) -> i32 {
    cvtss2si(x * 2.0 - 0.5) >> 1
}

/// FMath::RoundToInt as the Round node runs it (142f817a0): cvtss2si(2x + 0.5) >> 1.
pub fn round_to_int(x: f32) -> i32 {
    cvtss2si(x * 2.0 + 0.5) >> 1
}

/// FMath::TruncToInt as the FTrunc node runs it (142f6c5d0): cvttss2si.
pub fn trunc_to_int(x: f32) -> i32 {
    cvttss2si(x)
}

/// FMath::FRand's float from one rand() (RandomFloat 142f7fcb0): (r & 0x7fff) x f32(1 / 32767), so 32767 gives 1.
pub fn rand_to_float(r: i32) -> f32 {
    (r & 0x7fff) as f32 * f32::from_bits(0x3800_0100)
}

/// FRandomStream: InitialSeed and Seed, the multiplicative congruential step and the float from it
/// (RandomFloatInRangeFromStream 142b2be20).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RandomStream {
    pub initial_seed: i32,
    pub seed: i32,
}

impl RandomStream {
    pub fn new(seed: i32) -> RandomStream {
        RandomStream { initial_seed: seed, seed }
    }

    /// Seed = Seed x 196314165 + 907633515.
    pub fn mutate(&mut self) {
        self.seed = self.seed.wrapping_mul(196_314_165).wrapping_add(907_633_515);
    }

    /// [0, 1): the seed's top 23 bits as a mantissa of [1, 2), less 1.
    pub fn get_fraction(&mut self) -> f32 {
        self.mutate();
        f32::from_bits(0x3f80_0000 | (self.seed.cast_unsigned() >> 9)) - 1.0
    }

    /// FRandomStream::FRandRange: (max - min) x fraction + min.
    pub fn frand_range(&mut self, min: f32, max: f32) -> f32 {
        let span = max - min;
        span * self.get_fraction() + min
    }
}
