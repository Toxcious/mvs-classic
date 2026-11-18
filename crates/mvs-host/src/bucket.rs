//! A token bucket: `per_sec` a second and `burst` at once, by the caller's clock in ms.

/// A link's budget of one kind of message.
#[derive(Clone, Debug, PartialEq)]
pub struct Bucket {
    per_sec: f64,
    burst: f64,
    have: f64,
    at: f64,
}

impl Bucket {
    /// A full bucket at `now`.
    pub fn new(per_sec: f64, burst: f64, now: f64) -> Bucket {
        Bucket { per_sec, burst, have: burst, at: now }
    }

    /// Takes `cost` at `now`; false when it is over what is left (nothing taken).
    pub fn take(&mut self, cost: f64, now: f64) -> bool {
        let v = self.have + ((now - self.at) / 1000.0) * self.per_sec;
        // (Math.min: a NaN stays NaN, and a NaN's comparison is false: the bucket then takes every cost)
        self.have = if v.is_nan() { f64::NAN } else { v.min(self.burst) };
        self.at = now;
        if self.have < cost {
            return false;
        }
        self.have -= cost;
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn burst_then_rate() {
        let mut b = Bucket::new(2.0, 3.0, 0.0);
        assert!(b.take(1.0, 0.0) && b.take(1.0, 0.0) && b.take(1.0, 0.0));
        assert!(!b.take(1.0, 0.0));
        assert!(!b.take(1.0, 400.0));
        assert!(b.take(1.0, 500.0));
        // (the backlog of a long wait is capped at the burst)
        assert!(b.take(3.0, 100_000.0));
        assert!(!b.take(1.0, 100_000.0));
    }

    #[test]
    fn nan_clock_takes_every_cost() {
        let mut b = Bucket::new(2.0, 1.0, 0.0);
        assert!(b.take(1.0, f64::NAN));
        assert!(b.take(100.0, f64::NAN));
        assert!(b.take(1.0, 5.0), "a NaN backlog stays NaN");
    }
}
