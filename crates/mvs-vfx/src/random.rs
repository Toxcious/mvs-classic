//! The player's random numbers: one stream every particle draw takes in turn, where a replay hands back recorded draws.

/// Where the player's random fractions come from: one call per draw, in the same
/// order. A replay hands back recorded draws through it.
pub trait RandomSource: Send {
    /// A fraction in [0, 1).
    fn next_f64(&mut self) -> f64;
}

/// FRandomStream (Math/RandomStream.h): Seed = Seed x 196314165 + 907633515, a fraction from the top 23 bits
/// (GetFraction). Unverified: the stock 4.26 source, not read in the exe; the build's particle modules draw from the
/// component's stream or FMath::FRand per module (not read), and a recording from one stream.
#[derive(Clone, Debug)]
pub struct RandomStream {
    seed: u32,
}

impl RandomStream {
    pub fn new(seed: u32) -> RandomStream {
        RandomStream { seed }
    }

    fn mutate(&mut self) {
        self.seed = self.seed.wrapping_mul(196_314_165).wrapping_add(907_633_515);
    }
}

impl RandomSource for RandomStream {
    fn next_f64(&mut self) -> f64 {
        self.mutate();
        let bits = 0x3f80_0000 | (self.seed >> 9);
        f64::from(f32::from_bits(bits) - 1.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fractions_stay_in_range() {
        let mut r = RandomStream::new(7);
        for _ in 0..10_000 {
            let x = r.next_f64();
            assert!((0.0..1.0).contains(&x), "{x}");
        }
    }
}
