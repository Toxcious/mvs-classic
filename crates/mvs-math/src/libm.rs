//! fdlibm / FreeBSD msun ports using only + - * / and sqrt: the port of the oracle's libm, statement for
//! statement, so both give the same bits. Its integer steps are JavaScript's: a word read is `| 0` (signed) or `>>> 0`
//! (unsigned), an arithmetic result fed to a bit operation wraps modulo 2^32, a shift count is taken modulo 32.
//! The constants are fdlibm's literals as written there, not `std::f64::consts`.
#![allow(clippy::approx_constant, clippy::excessive_precision, clippy::unreadable_literal, clippy::needless_late_init)]

#[inline]
fn hi(x: f64) -> i32 {
    (x.to_bits() >> 32) as u32 as i32
}
#[inline]
fn lo(x: f64) -> u32 {
    x.to_bits() as u32
}
#[inline]
fn with_hi(x: f64, h: i32) -> f64 {
    f64::from_bits(((h as u32 as u64) << 32) | (x.to_bits() & 0xffff_ffff))
}
#[inline]
fn lo0(x: f64) -> f64 {
    f64::from_bits(x.to_bits() & 0xffff_ffff_0000_0000)
}
#[inline]
fn words(h: i32, l: u32) -> f64 {
    f64::from_bits(((h as u32 as u64) << 32) | l as u64)
}

const S1: f64 = -1.66666666666666324348e-01;
const S2: f64 = 8.33333333332248946124e-03;
const S3: f64 = -1.98412698298579493134e-04;
const S4: f64 = 2.75573137070700676789e-06;
const S5: f64 = -2.50507602534068634195e-08;
const S6: f64 = 1.58969099521155010221e-10;

pub(crate) fn ksin(x: f64, y: f64, iy: i32) -> f64 {
    if (hi(x) & 0x7fffffff) < 0x3e400000 {
        return x;
    }
    let z = x * x;
    let v = z * x;
    let r = S2 + z * (S3 + z * (S4 + z * (S5 + z * S6)));
    if iy == 0 { x + v * (S1 + z * r) } else { x - ((z * (0.5 * y - v * r) - y) - v * S1) }
}

const C1: f64 = 4.16666666666666019037e-02;
const C2: f64 = -1.38888888888741095749e-03;
const C3: f64 = 2.48015872894767294178e-05;
const C4: f64 = -2.75573143513906633035e-07;
const C5: f64 = 2.08757232129817482790e-09;
const C6: f64 = -1.13596475577881948265e-11;

pub(crate) fn kcos(x: f64, y: f64) -> f64 {
    let ix = hi(x) & 0x7fffffff;
    if ix < 0x3e400000 {
        return 1.0;
    }
    let z = x * x;
    let r = z * (C1 + z * (C2 + z * (C3 + z * (C4 + z * (C5 + z * C6)))));
    if ix < 0x3fd33333 {
        return 1.0 - (0.5 * z - (z * r - x * y));
    }
    let qx = if ix > 0x3fe90000 { 0.28125 } else { words(ix - 0x00200000, 0) };
    let hz = 0.5 * z - qx;
    let a = 1.0 - qx;
    a - (hz - (z * r - x * y))
}

const INVPIO2: f64 = 6.36619772367581382433e-01;
const PIO2_1: f64 = 1.57079632673412561417e+00;
const PIO2_1T: f64 = 6.07710050650619224932e-11;
const PIO2_2: f64 = 6.07710050630396597660e-11;
const PIO2_2T: f64 = 2.02226624879595063154e-21;
const PIO2_3: f64 = 2.02226624871116645580e-21;
const PIO2_3T: f64 = 8.47842766036889956997e-32;

/// x = n * pi/2 + (r0 + r1), |r0 + r1| <= pi/4: (n mod 4, r0, r1).
fn rem_pio2(x: f64) -> (i32, f64, f64) {
    let hx = hi(x);
    let ix = hx & 0x7fffffff;
    if ix <= 0x3fe921fb {
        return (0, x, 0.0);
    }
    if ix < 0x4002d97c {
        if hx > 0 {
            let mut z = x - PIO2_1;
            if ix != 0x3ff921fb {
                let r0 = z - PIO2_1T;
                return (1, r0, (z - r0) - PIO2_1T);
            }
            z -= PIO2_2;
            let r0 = z - PIO2_2T;
            return (1, r0, (z - r0) - PIO2_2T);
        }
        let mut z = x + PIO2_1;
        if ix != 0x3ff921fb {
            let r0 = z + PIO2_1T;
            return (3, r0, (z - r0) + PIO2_1T);
        }
        z += PIO2_2;
        let r0 = z + PIO2_2T;
        return (3, r0, (z - r0) + PIO2_2T);
    }
    let mut t = x.abs();
    let mut j = ix >> 20;
    // Above 2^20 * pi/2 fdlibm switches to a multi-word reduction; the sim has no such angles, and an exact
    // remainder by the double nearest 2 pi keeps the result defined (and the same everywhere), only less exact.
    if ix > 0x413921fb {
        t %= 2.0 * PI;
        j = (hi(t) >> 20) & 0x7ff;
    }
    let fnv = (t * INVPIO2 + 0.5).floor();
    let mut r = t - fnv * PIO2_1;
    let mut w = fnv * PIO2_1T;
    let mut r0 = r - w;
    if j - ((hi(r0) >> 20) & 0x7ff) > 16 {
        t = r;
        w = fnv * PIO2_2;
        r = t - w;
        w = fnv * PIO2_2T - ((t - r) - w);
        r0 = r - w;
        if j - ((hi(r0) >> 20) & 0x7ff) > 49 {
            t = r;
            w = fnv * PIO2_3;
            r = t - w;
            w = fnv * PIO2_3T - ((t - r) - w);
            r0 = r - w;
        }
    }
    let r1 = (r - r0) - w;
    let n = fnv - 4.0 * (fnv / 4.0).floor();
    if hx < 0 {
        return ((4 - n as i32) & 3, -r0, -r1);
    }
    (n as i32, r0, r1)
}

pub fn sin(x: f64) -> f64 {
    let ix = hi(x) & 0x7fffffff;
    if ix <= 0x3fe921fb {
        return ksin(x, 0.0, 0);
    }
    if ix >= 0x7ff00000 {
        return f64::NAN;
    }
    match rem_pio2(x) {
        (0, r0, r1) => ksin(r0, r1, 1),
        (1, r0, r1) => kcos(r0, r1),
        (2, r0, r1) => -ksin(r0, r1, 1),
        (_, r0, r1) => -kcos(r0, r1),
    }
}

pub fn cos(x: f64) -> f64 {
    let ix = hi(x) & 0x7fffffff;
    if ix <= 0x3fe921fb {
        return kcos(x, 0.0);
    }
    if ix >= 0x7ff00000 {
        return f64::NAN;
    }
    match rem_pio2(x) {
        (0, r0, r1) => kcos(r0, r1),
        (1, r0, r1) => -ksin(r0, r1, 1),
        (2, r0, r1) => -kcos(r0, r1),
        (_, r0, r1) => ksin(r0, r1, 1),
    }
}

const ATANHI: [f64; 4] =
    [4.63647609000806093515e-01, 7.85398163397448278999e-01, 9.82793723247329054082e-01, 1.57079632679489655800e+00];
const ATANLO: [f64; 4] =
    [2.26987774529616870924e-17, 3.06161699786838301793e-17, 1.39033110312309984516e-17, 6.12323399573676603587e-17];
const AT: [f64; 11] = [
    3.33333333333329318027e-01,
    -1.99999999998764832476e-01,
    1.42857142725034663711e-01,
    -1.11111104054623557880e-01,
    9.09088713343650656196e-02,
    -7.69187620504482999495e-02,
    6.66107313738753120669e-02,
    -5.83357013379057348645e-02,
    4.97687799461593236017e-02,
    -3.65315727442169155270e-02,
    1.62858201153657823623e-02,
];

pub fn atan(x: f64) -> f64 {
    if x.is_nan() {
        return f64::NAN;
    }
    let mut x = x;
    let hx = hi(x);
    let ix = hx & 0x7fffffff;
    let id: i32;
    if ix >= 0x44100000 {
        return if hx > 0 { ATANHI[3] + ATANLO[3] } else { -ATANHI[3] - ATANLO[3] };
    }
    if ix < 0x3fdc0000 {
        if ix < 0x3e200000 {
            return x;
        }
        id = -1;
    } else {
        x = x.abs();
        if ix < 0x3ff30000 {
            if ix < 0x3fe60000 {
                id = 0;
                x = (2.0 * x - 1.0) / (2.0 + x);
            } else {
                id = 1;
                x = (x - 1.0) / (x + 1.0);
            }
        } else if ix < 0x40038000 {
            id = 2;
            x = (x - 1.5) / (1.0 + 1.5 * x);
        } else {
            id = 3;
            x = -1.0 / x;
        }
    }
    let z = x * x;
    let w = z * z;
    let s1 = z * (AT[0] + w * (AT[2] + w * (AT[4] + w * (AT[6] + w * (AT[8] + w * AT[10])))));
    let s2 = w * (AT[1] + w * (AT[3] + w * (AT[5] + w * (AT[7] + w * AT[9]))));
    if id < 0 {
        return x - x * (s1 + s2);
    }
    let id = id as usize;
    let r = ATANHI[id] - ((x * (s1 + s2) - ATANLO[id]) - x);
    if hx < 0 { -r } else { r }
}

const PI: f64 = 3.1415926535897931160e+00;
const PI_LO: f64 = 1.2246467991473531772e-16;
const PI_2: f64 = 1.5707963267948965580e+00;
const PI_4: f64 = 7.8539816339744827900e-01;

pub fn atan2(y: f64, x: f64) -> f64 {
    if x.is_nan() || y.is_nan() {
        return f64::NAN;
    }
    let (hx, lx, hy, ly) = (hi(x), lo(x), hi(y), lo(y));
    let ix = hx & 0x7fffffff;
    let iy = hy & 0x7fffffff;
    if (hx.wrapping_sub(0x3ff00000) | lx as i32) == 0 {
        return atan(y);
    }
    let m = ((hy >> 31) & 1) | ((hx >> 30) & 2);
    if (iy | ly as i32) == 0 {
        return match m {
            0 | 1 => y,
            2 => PI,
            _ => -PI,
        };
    }
    if (ix | lx as i32) == 0 {
        return if hy < 0 { -PI_2 } else { PI_2 };
    }
    if ix == 0x7ff00000 {
        if iy == 0x7ff00000 {
            return match m {
                0 => PI_4,
                1 => -PI_4,
                2 => 3.0 * PI_4,
                _ => -3.0 * PI_4,
            };
        }
        return match m {
            0 => 0.0,
            1 => -0.0,
            2 => PI,
            _ => -PI,
        };
    }
    if iy == 0x7ff00000 {
        return if hy < 0 { -PI_2 } else { PI_2 };
    }
    let k = (iy - ix) >> 20;
    let z = if k > 60 {
        PI_2 + 0.5 * PI_LO
    } else if hx < 0 && k < -60 {
        0.0
    } else {
        atan((y / x).abs())
    };
    match if k > 60 { m & 1 } else { m } {
        0 => z,
        1 => -z,
        2 => PI - (z - PI_LO),
        _ => (z - PI_LO) - PI,
    }
}

// k_tan.c and s_tan.c.
const T: [f64; 13] = [
    3.33333333333334091986e-01,
    1.33333333333201242699e-01,
    5.39682539762260521377e-02,
    2.18694882948595424599e-02,
    8.86323982359930005737e-03,
    3.59207910759131235356e-03,
    1.45620945432529025516e-03,
    5.88041240820264096874e-04,
    2.46463134818469906812e-04,
    7.81794442939557092300e-05,
    7.14072491382608190305e-05,
    -1.85586374855275456654e-05,
    2.59073051863633712884e-05,
];
const PIO4: f64 = 7.85398163397448278999e-01;
const PIO4LO: f64 = 3.06161699786838301793e-17;

pub(crate) fn ktan(x: f64, y: f64, iy: i32) -> f64 {
    let (mut x, mut y) = (x, y);
    let hx = hi(x);
    let ix = hx & 0x7fffffff;
    if ix < 0x3e300000 && x.trunc() == 0.0 {
        if ((ix | lo(x) as i32) | (iy + 1)) == 0 {
            return 1.0 / x.abs();
        }
        if iy == 1 {
            return x;
        }
        let w = x + y;
        let z = lo0(w);
        let v = y - (z - x);
        let a = -1.0 / w;
        let t = lo0(a);
        let s = 1.0 + t * z;
        return t + a * (s + t * v);
    }
    let big = ix >= 0x3fe59428;
    if big {
        if hx < 0 {
            x = -x;
            y = -y;
        }
        x = (PIO4 - x) + (PIO4LO - y);
        y = 0.0;
    }
    let z = x * x;
    let w = z * z;
    let mut r = T[1] + w * (T[3] + w * (T[5] + w * (T[7] + w * (T[9] + w * T[11]))));
    let mut v = z * (T[2] + w * (T[4] + w * (T[6] + w * (T[8] + w * (T[10] + w * T[12])))));
    let s = z * x;
    r = y + z * (s * (r + v) + y);
    r += T[0] * s;
    let ww = x + r;
    if big {
        v = iy as f64;
        return (1 - ((hx >> 30) & 2)) as f64 * (v - 2.0 * (x - ((ww * ww) / (ww + v) - r)));
    }
    if iy == 1 {
        return ww;
    }
    let zz = lo0(ww);
    v = r - (zz - x);
    let a = -1.0 / ww;
    let t = lo0(a);
    let ss = 1.0 + t * zz;
    t + a * (ss + t * v)
}

pub fn tan(x: f64) -> f64 {
    let ix = hi(x) & 0x7fffffff;
    if ix <= 0x3fe921fb {
        return ktan(x, 0.0, 1);
    }
    if ix >= 0x7ff00000 {
        return f64::NAN;
    }
    let (n, r0, r1) = rem_pio2(x);
    ktan(r0, r1, 1 - ((n & 1) << 1))
}

// e_asin.c and e_acos.c: their shared rational approximation of (asin(x) - x) / x^3.
const PIO2_HI: f64 = 1.57079632679489655800e+00;
const PIO2_LO: f64 = 6.12323399573676603587e-17;
const PIO4_HI: f64 = 7.85398163397448278999e-01;
const PS0: f64 = 1.66666666666666657415e-01;
const PS1: f64 = -3.25565818622400915405e-01;
const PS2: f64 = 2.01212532134862925881e-01;
const PS3: f64 = -4.00555345006794114027e-02;
const PS4: f64 = 7.91534994289814532176e-04;
const PS5: f64 = 3.47933107596021167570e-05;
const QS1: f64 = -2.40339491173441421878e+00;
const QS2: f64 = 2.02094576023350569471e+00;
const QS3: f64 = -6.88283971605453293030e-01;
const QS4: f64 = 7.70381505559019352791e-02;
#[inline]
fn asin_p(t: f64) -> f64 {
    t * (PS0 + t * (PS1 + t * (PS2 + t * (PS3 + t * (PS4 + t * PS5)))))
}
#[inline]
fn asin_q(t: f64) -> f64 {
    1.0 + t * (QS1 + t * (QS2 + t * (QS3 + t * QS4)))
}

pub fn asin(x: f64) -> f64 {
    let hx = hi(x);
    let ix = hx & 0x7fffffff;
    if ix >= 0x3ff00000 {
        if ((ix - 0x3ff00000) | lo(x) as i32) == 0 {
            return x * PIO2_HI + x * PIO2_LO;
        }
        return f64::NAN;
    }
    if ix < 0x3fe00000 {
        if ix < 0x3e400000 {
            return x;
        }
        let t = x * x;
        return x + x * (asin_p(t) / asin_q(t));
    }
    let mut t = (1.0 - x.abs()) * 0.5;
    let p = asin_p(t);
    let q = asin_q(t);
    let s = t.sqrt();
    if ix >= 0x3fef3333 {
        t = PIO2_HI - (2.0 * (s + s * (p / q)) - PIO2_LO);
    } else {
        let w = lo0(s);
        let c = (t - w * w) / (s + w);
        let r = p / q;
        t = PIO4_HI - ((2.0 * s * r - (PIO2_LO - 2.0 * c)) - (PIO4_HI - 2.0 * w));
    }
    if hx > 0 { t } else { -t }
}

pub fn acos(x: f64) -> f64 {
    let hx = hi(x);
    let ix = hx & 0x7fffffff;
    if ix >= 0x3ff00000 {
        if ((ix - 0x3ff00000) | lo(x) as i32) == 0 {
            return if hx > 0 { 0.0 } else { PI + 2.0 * PIO2_LO };
        }
        return f64::NAN;
    }
    if ix < 0x3fe00000 {
        if ix <= 0x3c600000 {
            return PIO2_HI + PIO2_LO;
        }
        let z = x * x;
        return PIO2_HI - (x - (PIO2_LO - x * (asin_p(z) / asin_q(z))));
    }
    if hx < 0 {
        let z = (1.0 + x) * 0.5;
        let s = z.sqrt();
        return PI - 2.0 * (s + ((asin_p(z) / asin_q(z)) * s - PIO2_LO));
    }
    let z = (1.0 - x) * 0.5;
    let s = z.sqrt();
    let df = lo0(s);
    let c = (z - df * df) / (s + df);
    2.0 * (df + ((asin_p(z) / asin_q(z)) * s + c))
}

// The logarithms: V8 takes log2 from FreeBSD (e_log2.c with k_log.h's k_log1p), log and log10 from fdlibm.
const LG1: f64 = 6.666666666666735130e-01;
const LG2_: f64 = 3.999999999940941908e-01;
const LG3: f64 = 2.857142874366239149e-01;
const LG4: f64 = 2.222219843214978396e-01;
const LG5: f64 = 1.818357216161805012e-01;
const LG6: f64 = 1.531383769920937332e-01;
const LG7: f64 = 1.479819860511658591e-01;
const TWO54: f64 = 1.80143985094819840000e+16;
const IVLN2HI: f64 = 1.44269504072144627571e+00;
const IVLN2LO: f64 = 1.67517131648865118353e-10;
const LN2_HI: f64 = 6.93147180369123816490e-01;
const LN2_LO: f64 = 1.90821492927058770002e-10;
const IVLN10: f64 = 4.34294481903251816668e-01;
const LOG10_2HI: f64 = 3.01029995663611771306e-01;
const LOG10_2LO: f64 = 3.69423907715893078616e-13;

fn klog1p(f: f64) -> f64 {
    let s = f / (2.0 + f);
    let z = s * s;
    let w = z * z;
    let t1 = w * (LG2_ + w * (LG4 + w * LG6));
    let t2 = z * (LG1 + w * (LG3 + w * (LG5 + w * LG7)));
    s * (0.5 * f * f + (t2 + t1))
}

/// x split as 2^k * (1 + f), sqrt(2)/2 < 1 + f < sqrt(2): Ok((f, k, the high word's fraction bits)), or Err(the
/// result) for the special inputs (zero, negative, NaN or infinite, 1).
fn log_split(x: f64) -> Result<(f64, i32, i32), f64> {
    let mut x = x;
    let mut hx = hi(x);
    let lx = lo(x);
    let mut k = 0;
    if hx < 0x00100000 {
        if ((hx & 0x7fffffff) | lx as i32) == 0 {
            return Err(f64::NEG_INFINITY);
        }
        if hx < 0 {
            return Err(f64::NAN);
        }
        k -= 54;
        x *= TWO54;
        hx = hi(x);
    }
    if hx >= 0x7ff00000 {
        return Err(x + x);
    }
    if hx == 0x3ff00000 && lx == 0 {
        return Err(0.0);
    }
    k += (hx >> 20) - 1023;
    hx &= 0x000fffff;
    let i = (hx + 0x95f64) & 0x100000;
    x = with_hi(x, hx | (i ^ 0x3ff00000));
    Ok((x - 1.0, k + (i >> 20), hx))
}

pub fn log2(x: f64) -> f64 {
    let (f, k, _) = match log_split(x) {
        Ok(v) => v,
        Err(r) => return r,
    };
    let y = k as f64;
    let hfsq = 0.5 * f * f;
    let r = klog1p(f);
    let h = lo0(f - hfsq);
    let l = (f - h) - hfsq + r;
    let mut val_hi = h * IVLN2HI;
    let mut val_lo = (l + h) * IVLN2LO + l * IVLN2HI;
    let w = y + val_hi;
    val_lo += (y - w) + val_hi;
    val_hi = w;
    val_lo + val_hi
}

/// fdlibm's e_log.c (V8's log, not FreeBSD's k_log.h form).
pub fn log(x: f64) -> f64 {
    let (f, k, hx) = match log_split(x) {
        Ok(v) => v,
        Err(r) => return r,
    };
    let kf = k as f64;
    if (0x000fffff & (2 + hx)) < 3 {
        if f == 0.0 {
            return kf * LN2_HI + kf * LN2_LO;
        }
        let r = f * f * (0.5 - 0.33333333333333333 * f);
        return if k == 0 { f - r } else { kf * LN2_HI - ((r - kf * LN2_LO) - f) };
    }
    let s = f / (2.0 + f);
    let z = s * s;
    let w = z * z;
    let t1 = w * (LG2_ + w * (LG4 + w * LG6));
    let t2 = z * (LG1 + w * (LG3 + w * (LG5 + w * LG7)));
    let r = t2 + t1;
    if ((hx - 0x6147a) | (0x6b851 - hx)) > 0 {
        let hfsq = 0.5 * f * f;
        return if k == 0 {
            f - (hfsq - s * (hfsq + r))
        } else {
            kf * LN2_HI - ((hfsq - (s * (hfsq + r) + kf * LN2_LO)) - f)
        };
    }
    if k == 0 { f - s * (f - r) } else { kf * LN2_HI - ((s * (f - r) - kf * LN2_LO) - f) }
}

/// fdlibm's e_log10.c (V8's log10, not FreeBSD's): log10(x) = n * log10(2) + log(x') / ln(10), x = 2^n * x'.
pub fn log10(x: f64) -> f64 {
    let mut x = x;
    let mut hx = hi(x);
    let mut k: i32 = 0;
    if hx < 0x00100000 {
        if ((hx & 0x7fffffff) | lo(x) as i32) == 0 {
            return f64::NEG_INFINITY;
        }
        if hx < 0 {
            return f64::NAN;
        }
        k -= 54;
        x *= TWO54;
        hx = hi(x);
    }
    if hx >= 0x7ff00000 {
        return x + x;
    }
    if hx == 0x3ff00000 && lo(x) == 0 {
        return 0.0;
    }
    k += (hx >> 20) - 1023;
    let i = ((k as u32) >> 31) as i32;
    let y = (k + i) as f64;
    x = with_hi(x, (hx & 0x000fffff) | ((0x3ff - i) << 20));
    (y * LOG10_2LO + IVLN10 * log(x)) + y * LOG10_2HI
}

const BP: [f64; 2] = [1.0, 1.5];
const DP_H: [f64; 2] = [0.0, 5.84962487220764160156e-01];
const DP_L: [f64; 2] = [0.0, 1.35003920212974897128e-08];
const TWO53: f64 = 9007199254740992.0;
const HUGE: f64 = 1e300;
const TINY: f64 = 1e-300;
const L1: f64 = 5.99999999999994648725e-01;
const L2: f64 = 4.28571428578550184252e-01;
const L3: f64 = 3.33333329818377432918e-01;
const L4: f64 = 2.72728123808534006489e-01;
const L5: f64 = 2.30660745775561754067e-01;
const L6: f64 = 2.06975017800338417784e-01;
const P1: f64 = 1.66666666666666019037e-01;
const P2: f64 = -2.77777777770155933842e-03;
const P3: f64 = 6.61375632143793436117e-05;
const P4: f64 = -1.65339022054652515390e-06;
const P5: f64 = 4.13813679705723846039e-08;
const LG2: f64 = 6.93147180559945286227e-01;
const LG2_H: f64 = 6.93147182464599609375e-01;
const LG2_L: f64 = -1.90465429995776804525e-09;
const OVT: f64 = 8.0085662595372944372e-17;
const CP: f64 = 9.61796693925975554329e-01;
const CP_H: f64 = 9.61796700954437255859e-01;
const CP_L: f64 = -7.02846165095275826516e-09;
const IVLN2: f64 = 1.44269504088896338700e+00;
const IVLN2_H: f64 = 1.44269502162933349609e+00;
const IVLN2_L: f64 = 1.92596299112661746302e-08;

pub fn pow(x: f64, y: f64) -> f64 {
    let (hx, lx, hy, ly) = (hi(x), lo(x), hi(y), lo(y));
    let mut ix = hx & 0x7fffffff;
    let iy = hy & 0x7fffffff;
    if (iy | ly as i32) == 0 {
        return 1.0;
    }
    if x.is_nan() || y.is_nan() {
        return f64::NAN;
    }
    let mut yisint = 0;
    if hx < 0 {
        if iy >= 0x43400000 {
            yisint = 2;
        } else if iy >= 0x3ff00000 {
            let k = (iy >> 20) - 0x3ff;
            if k > 20 {
                let j = ly.wrapping_shr((52 - k) as u32);
                if j.wrapping_shl((52 - k) as u32) == ly {
                    yisint = 2 - (j & 1) as i32;
                }
            } else if ly == 0 {
                let j = iy.wrapping_shr((20 - k) as u32);
                if j.wrapping_shl((20 - k) as u32) == iy {
                    yisint = 2 - (j & 1);
                }
            }
        }
    }
    if ly == 0 {
        if iy == 0x7ff00000 {
            if (ix.wrapping_sub(0x3ff00000) | lx as i32) == 0 {
                return f64::NAN;
            }
            if ix >= 0x3ff00000 {
                return if hy >= 0 { y } else { 0.0 };
            }
            return if hy < 0 { -y } else { 0.0 };
        }
        if iy == 0x3ff00000 {
            return if hy < 0 { 1.0 / x } else { x };
        }
        if hy == 0x40000000 {
            return x * x;
        }
        if hy == 0x3fe00000 && hx >= 0 {
            return x.sqrt();
        }
    }
    let mut ax = x.abs();
    if lx == 0 && (ix == 0x7ff00000 || ix == 0 || ix == 0x3ff00000) {
        let mut z = ax;
        if hy < 0 {
            z = 1.0 / z;
        }
        if hx < 0 {
            if ((ix - 0x3ff00000) | yisint) == 0 {
                z = f64::NAN;
            } else if yisint == 1 {
                z = -z;
            }
        }
        return z;
    }
    if hx < 0 && yisint == 0 {
        return f64::NAN;
    }
    let s = if hx < 0 && yisint == 1 { -1.0 } else { 1.0 };
    let t1: f64;
    let t2: f64;
    if iy > 0x41e00000 {
        if iy > 0x43f00000 {
            if ix <= 0x3fefffff {
                return if hy < 0 { HUGE * HUGE } else { TINY * TINY };
            }
            if ix >= 0x3ff00000 {
                return if hy > 0 { HUGE * HUGE } else { TINY * TINY };
            }
        }
        if ix < 0x3fefffff {
            return if hy < 0 { s * HUGE * HUGE } else { s * TINY * TINY };
        }
        if ix > 0x3ff00000 {
            return if hy > 0 { s * HUGE * HUGE } else { s * TINY * TINY };
        }
        let t = ax - 1.0;
        let w = (t * t) * (0.5 - t * (0.3333333333333333333333 - t * 0.25));
        let u = IVLN2_H * t;
        let v = t * IVLN2_L - w * IVLN2;
        t1 = lo0(u + v);
        t2 = v - (t1 - u);
    } else {
        let mut n: i32 = 0;
        if ix < 0x00100000 {
            ax *= TWO53;
            n -= 53;
            ix = hi(ax);
        }
        n += (ix >> 20) - 0x3ff;
        let j = ix & 0x000fffff;
        ix = j | 0x3ff00000;
        let k: usize;
        if j <= 0x3988e {
            k = 0;
        } else if j < 0xbb67a {
            k = 1;
        } else {
            k = 0;
            n += 1;
            ix -= 0x00100000;
        }
        ax = with_hi(ax, ix);
        let mut u = ax - BP[k];
        let mut v = 1.0 / (ax + BP[k]);
        let ss = u * v;
        let sh = lo0(ss);
        let mut th = words(((ix >> 1) | 0x20000000) + 0x00080000 + ((k as i32) << 18), 0);
        let mut tl = ax - (th - BP[k]);
        let sl = v * ((u - sh * th) - sh * tl);
        let mut s2 = ss * ss;
        let mut r = s2 * s2 * (L1 + s2 * (L2 + s2 * (L3 + s2 * (L4 + s2 * (L5 + s2 * L6)))));
        r += sl * (sh + ss);
        s2 = sh * sh;
        th = lo0(3.0 + s2 + r);
        tl = r - ((th - 3.0) - s2);
        u = sh * th;
        v = sl * th + tl * ss;
        let ph = lo0(u + v);
        let pl = v - (ph - u);
        let zh = CP_H * ph;
        let zl = CP_L * ph + pl * CP + DP_L[k];
        let t = n as f64;
        t1 = lo0(((zh + zl) + DP_H[k]) + t);
        t2 = zl - (((t1 - t) - DP_H[k]) - zh);
    }
    let y1 = lo0(y);
    let pl = (y - y1) * t1 + y * t2;
    let mut ph = y1 * t1;
    let mut z = pl + ph;
    let mut j = hi(z);
    let i0 = lo(z);
    if j >= 0x40900000 {
        if (j.wrapping_sub(0x40900000) | i0 as i32) != 0 {
            return s * HUGE * HUGE;
        }
        if pl + OVT > z - ph {
            return s * HUGE * HUGE;
        }
    } else if (j & 0x7fffffff) >= 0x4090cc00 {
        if (j.wrapping_sub(0xc090cc00_u32 as i32) | i0 as i32) != 0 {
            return s * TINY * TINY;
        }
        if pl <= z - ph {
            return s * TINY * TINY;
        }
    }
    let i = j & 0x7fffffff;
    let mut k = (i >> 20) - 0x3ff;
    let mut n: i32 = 0;
    if i > 0x3fe00000 {
        n = j.wrapping_add(0x00100000_i32.wrapping_shr((k + 1) as u32));
        k = ((n & 0x7fffffff) >> 20) - 0x3ff;
        let t = words(n & !(0x000fffff_i32.wrapping_shr(k as u32)), 0);
        n = ((n & 0x000fffff) | 0x00100000).wrapping_shr((20 - k) as u32);
        if j < 0 {
            n = -n;
        }
        ph -= t;
    }
    let mut t = lo0(pl + ph);
    let u = t * LG2_H;
    let v = (pl - (t - ph)) * LG2 + t * LG2_L;
    z = u + v;
    let w = v - (z - u);
    t = z * z;
    let t1 = z - t * (P1 + t * (P2 + t * (P3 + t * (P4 + t * P5))));
    let r = (z * t1) / (t1 - 2.0) - (w + z * w);
    z = 1.0 - (r - z);
    j = hi(z).wrapping_add(n.wrapping_shl(20));
    // a subnormal result: scale in two exact-then-rounded steps (scalbn)
    if j >> 20 <= 0 {
        z = if n < -1000 {
            z * words((n + 54 + 1023) << 20, 0) * words((1023 - 54) << 20, 0)
        } else {
            z * words((n + 1023) << 20, 0)
        };
    } else {
        z = with_hi(z, j);
    }
    s * z
}
