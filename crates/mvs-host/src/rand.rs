//! The host core's randomness and wall clock (a random int, random bytes and the wall time),
//! injected so a test can replay a recorded stream.

use std::time::{SystemTime, UNIX_EPOCH};

/// The core's draws and its wall clock.
pub trait HostRand: Send {
    /// A whole number in [lo, hi) (randomInt(lo, hi); `hi > lo`, `hi - lo` at most 2^48 as node's).
    fn int(&mut self, lo: u64, hi: u64) -> u64;
    /// Random bytes (randomBytes).
    fn bytes(&mut self, out: &mut [u8]);
    /// Milliseconds since 1970 (Date.now): a match record's end.
    fn wall_ms(&mut self) -> u64 {
        SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| u64::try_from(d.as_millis()).unwrap_or(u64::MAX))
    }
}

/// The operating system's random numbers.
#[derive(Clone, Copy, Debug, Default)]
pub struct SystemRand;

impl HostRand for SystemRand {
    fn int(&mut self, lo: u64, hi: u64) -> u64 {
        assert!(hi > lo, "SystemRand::int: an empty range {lo}..{hi}");
        let n = hi - lo;
        assert!(n <= 1 << 48, "SystemRand::int: a range past 2^48 ({lo}..{hi})");
        // (48 random bits; the values past the last whole multiple of n are drawn again, so that each of the n is as
        // likely)
        let limit = (1u64 << 48) - ((1u64 << 48) % n);
        loop {
            let mut b = [0u8; 8];
            ::rand::fill(&mut b[2..]);
            let x = u64::from_be_bytes(b);
            if x < limit {
                return lo + x % n;
            }
        }
    }

    fn bytes(&mut self, out: &mut [u8]) {
        ::rand::fill(out);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn int_in_range() {
        let mut r = SystemRand;
        for _ in 0..1000 {
            let x = r.int(5, 8);
            assert!((5..8).contains(&x));
            assert!(r.int(0, 1 << 32) < 1 << 32);
        }
        assert_eq!(r.int(7, 8), 7);
        let mut b = [0u8; 12];
        r.bytes(&mut b);
        assert!(r.wall_ms() > 1_600_000_000_000);
    }
}
