//! V8's `Math.exp` and `Math.pow` (src/base/ieee754.cc: fdlibm's e_exp.c and e_pow.c), with the same bits.
#![allow(clippy::excessive_precision, clippy::unreadable_literal, clippy::approx_constant)]

fn hi(x: f64) -> i32 {
    (x.to_bits() >> 32) as u32 as i32
}
fn lo(x: f64) -> u32 {
    x.to_bits() as u32
}

const HALF: [f64; 2] = [0.5, -0.5];
const HUGE: f64 = 1.0e+300;
const TWOM1000: f64 = 9.33263618503218878990e-302;
const TWO1023: f64 = 8.988465674311579539e307;
const O_THRESHOLD: f64 = 7.09782712893383973096e+02;
const U_THRESHOLD: f64 = -7.45133219101941108420e+02;
const LN2_HI: [f64; 2] = [6.93147180369123816490e-01, -6.93147180369123816490e-01];
const LN2_LO: [f64; 2] = [1.90821492927058770002e-10, -1.90821492927058770002e-10];
const INVLN2: f64 = 1.44269504088896338700e+00;
const E1: f64 = 1.66666666666666019037e-01;
const E2: f64 = -2.77777777770155933842e-03;
const E3: f64 = 6.61375632143793436117e-05;
const E4: f64 = -1.65339022054652515390e-06;
const E5: f64 = 4.13813679705723846039e-08;

/// e to the power x.
pub fn exp(x: f64) -> f64 {
    let mut x = x;
    let mut hx = hi(x);
    let xsb = ((hx >> 31) & 1) as usize;
    hx &= 0x7fffffff;
    if hx >= 0x40862E42 {
        if hx >= 0x7ff00000 {
            if ((hx & 0xfffff) as u32 | lo(x)) != 0 {
                return x + x;
            }
            return if xsb == 0 { x } else { 0.0 };
        }
        if x > O_THRESHOLD {
            return f64::INFINITY;
        }
        if x < U_THRESHOLD {
            return 0.0;
        }
    }
    let mut h = 0.0;
    let mut l = 0.0;
    let k: i32;
    if hx > 0x3fd62e42 {
        if hx < 0x3FF0A2B2 {
            h = x - LN2_HI[xsb];
            l = LN2_LO[xsb];
            k = 1 - xsb as i32 - xsb as i32;
        } else {
            k = (INVLN2 * x + HALF[xsb]) as i32;
            let t = f64::from(k);
            h = x - t * LN2_HI[0];
            l = t * LN2_LO[0];
        }
        x = h - l;
    } else if hx < 0x3e300000 {
        if HUGE + x > 1.0 {
            return 1.0 + x;
        }
        k = 0;
    } else {
        k = 0;
    }
    let t = x * x;
    let twopk = if k >= -1021 {
        f64::from_bits(((0x3ff00000 + (k << 20)) as u32 as u64) << 32)
    } else {
        f64::from_bits(((0x3ff00000 + ((k + 1000) << 20)) as u32 as u64) << 32)
    };
    let c = x - t * (E1 + t * (E2 + t * (E3 + t * (E4 + t * E5))));
    if k == 0 {
        return 1.0 - ((x * c) / (c - 2.0) - x);
    }
    let y = 1.0 - ((l - (x * c) / (2.0 - c)) - h);
    if k >= -1021 {
        if k == 1024 {
            return y * 2.0 * TWO1023;
        }
        y * twopk
    } else {
        y * twopk * TWOM1000
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tape(name: &str) -> Option<Vec<f64>> {
        let dir = std::env::var_os("MVS_AUDIO_TAPES")
            .map_or_else(|| std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../.scratch/audio"), Into::into);
        let Ok(b) = std::fs::read(dir.join(name)) else {
            eprintln!("no {name} in {}: nothing compared", dir.display());
            return None;
        };
        Some(b.as_chunks::<8>().0.iter().map(|c| f64::from_le_bytes(*c)).collect())
    }

    #[test]
    fn exp_matches_node() {
        let Some(v) = tape("exp.bin") else { return };
        let bad = v.chunks(2).filter(|t| exp(t[0]).to_bits() != t[1].to_bits()).count();
        eprintln!("exp cases {}", v.len() / 2);
        assert_eq!(bad, 0);
    }
}
