//! (the port's own) A round trip's colour, the same wherever one is shown: the ping line top right and the custom
//! lobby's connection line. The build shows no round trip.

/// Green, yellow and red: the frame rate line's colours, a round trip's by these bounds.
pub const GREEN: [f64; 4] = [0.2, 1.0, 0.2, 1.0];
pub const YELLOW: [f64; 4] = [1.0, 1.0, 0.0, 1.0];
pub const RED: [f64; 4] = [1.0, 0.25, 0.2, 1.0];

/// The round trips at or below which a round trip is green and yellow, ms; above the second it is red (the port's own
/// numbers).
pub const GOOD_MS: f64 = 80.0;
pub const FAIR_MS: f64 = 150.0;

/// The colour of a round trip of `ms`.
pub fn color(ms: f64) -> [f64; 4] {
    if ms <= GOOD_MS {
        GREEN
    } else if ms <= FAIR_MS {
        YELLOW
    } else {
        RED
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_round_trip_colours_by_its_bounds() {
        assert_eq!(color(0.0), GREEN);
        assert_eq!(color(80.0), GREEN);
        assert_eq!(color(80.5), YELLOW);
        assert_eq!(color(150.0), YELLOW);
        assert_eq!(color(150.5), RED);
        assert_eq!(color(400.0), RED);
    }
}
