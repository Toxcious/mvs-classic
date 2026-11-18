//! The sim's random stream: mulberry32 over a 32-bit state, with JavaScript's integer rules
//! (`| 0` wraps, `Math.imul` is a wrapping multiply, `>>> 0` reads the bits unsigned).

use crate::Real;
use crate::State;

#[derive(State, Debug, PartialEq, Eq)]
pub struct Rng {
    pub s: i32,
}

impl Rng {
    /// `new Rng(seed)`: the seed, already `| 0`.
    pub const fn new(seed: i32) -> Self {
        Rng { s: seed }
    }

    /// `next()`: a float in [0, 1): the top 24 bits of the draw, so that no draw rounds to 1.0 in float32.
    /// Deviation: the reference draw is the whole 32 bits / 2^32 in double; this one is its float32 form, the same draw
    /// rounded toward zero to 24 bits (an index pick `int(n)` gives the reference index but where a draw lies within
    /// 2^-23 of a multiple of 1 / n).
    #[allow(
        clippy::should_implement_trait,
        reason = "`next` is the stream's draw, not an iterator (it never ends and is read one draw at a time)"
    )]
    pub fn next(&mut self) -> Real {
        self.s = self.s.wrapping_add(0x6d2b_79f5);
        let mut t = self.s;
        // `>>>` is a shift of the bits as unsigned; `>>> 0` reads them as unsigned.
        t = (t ^ (t.cast_unsigned() >> 15).cast_signed()).wrapping_mul(t | 1);
        t ^= t.wrapping_add((t ^ (t.cast_unsigned() >> 7).cast_signed()).wrapping_mul(t | 61));
        let u = (t ^ (t.cast_unsigned() >> 14).cast_signed()).cast_unsigned();
        crate::real(u >> 8) * (1.0 / 16_777_216.0)
    }

    /// `int(n)`: `Math.floor(next() * n)`.
    pub fn int(&mut self, n: Real) -> Real {
        (self.next() * n).floor()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // The values V8 gives for the World's stream of seed 12345: new Rng((12345 * 31337 + 7) | 0); the float32 draw is
    // V8's cut to its top 24 bits.
    #[test]
    fn same_stream_as_v8() {
        let seed = mvs_math::js::to_int32(12345.0 * 31337.0 + 7.0);
        assert_eq!(seed, 386_855_272);
        let mut r = Rng::new(seed);
        let top24 = |v8: f64| (v8 * 16_777_216.0).floor() / 16_777_216.0;
        assert_eq!(f64::from(r.next()), top24(0.4510229795705527));
        assert_eq!(f64::from(r.next()), top24(0.9640769951511174));
        assert_eq!(r.int(10.0), 3.0);
        assert_eq!(r.s, 1_586_585_415);
    }

    /// The reference draw (`next()` in double, all 32 bits) of the same state.
    fn ts_next(s: &mut i32) -> f64 {
        *s = s.wrapping_add(0x6d2b_79f5);
        let mut t = *s;
        t = (t ^ (t.cast_unsigned() >> 15).cast_signed()).wrapping_mul(t | 1);
        t ^= t.wrapping_add((t ^ (t.cast_unsigned() >> 7).cast_signed()).wrapping_mul(t | 61));
        f64::from((t ^ (t.cast_unsigned() >> 14).cast_signed()).cast_unsigned()) / 4_294_967_296.0
    }

    // An index pick gives the double draw's index on every draw but those within 2^-23 of a multiple of 1 / n
    // (where the float32 draw's cut, or its product's rounding, crosses the multiple); never n.
    #[test]
    fn index_picks_as_the_ts() {
        let mut moved = 0;
        for n in [2.0_f32, 3.0, 4.0, 5.0, 6.0, 7.0, 10.0, 12.0, 100.0, 1000.0] {
            let (mut r, mut s) = (Rng::new(386_855_272), 386_855_272);
            for _ in 0..1_000_000 {
                let x = ts_next(&mut s);
                let ts = (x * f64::from(n)).floor();
                let i = r.int(n);
                assert!(i < n);
                if f64::from(i) != ts {
                    // a tolerance on purpose: the float32 draw against the double reference draw
                    // the draw lies at a multiple of 1 / n (the cut takes one just above it below)
                    #[allow(clippy::disallowed_methods)] // the nearest multiple; a half never occurs
                    let k = (x * f64::from(n)).round() / f64::from(n);
                    assert!((x - k).abs() <= 2.0 / 16_777_216.0, "n {n}: draw {x} gives {i}, the reference {ts}");
                    moved += 1;
                }
            }
        }
        // (a boundary draw is rare: about n x 2^-24 of the draws; 25 of these 10 million)
        assert!(moved <= 25, "{moved} index picks moved");
    }
}
