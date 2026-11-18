//! A stage platform's drawn offset between two ticks: its offset from the data place (UE Y, Z) at the tick before
//! (`py`, `pz`) and at the latest (`cy`, `cz`).

/// A pass-through platform's drawn offsets from its data place (UE Y, Z).
// Deviation: (the port's own, the fighters' rule) a moving platform is drawn between its last two ticks at the frame's
// alpha, snapped where the tick's own move did not carry it; the build draws one tick per frame.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct PlatformBlend {
    pub py: f64,
    pub pz: f64,
    pub cy: f64,
    pub cz: f64,
}

/// What a platform's tolerance for "the tick's own move carried it" allows: the float rounding of the move's
/// `mx = dx - x0`.
const CARRIED: f64 = 1e-3;

impl PlatformBlend {
    /// One world tick (platformTick): the platform's offset now (`dy`, `dz` in UE) and its run's move of that tick
    /// (`run`: sim x, y; UE Y = -sim x, so the move's start is `dy + mx`, `dz - my`). Where the tick took it from is
    /// the place last recorded when the move carried it from there, else where it is now: a reset, a rollback that
    /// put it elsewhere or a platform with no run is not drawn on the way (the fighters' rule).
    pub fn tick(&mut self, dy: f64, dz: f64, run: Option<(f64, f64)>) {
        let carried =
            run.is_some_and(|(mx, my)| (dy + mx - self.cy).abs() < CARRIED && (dz - my - self.cz).abs() < CARRIED);
        self.py = if carried { self.cy } else { dy };
        self.pz = if carried { self.cz } else { dz };
        self.cy = dy;
        self.cz = dz;
    }

    /// The drawn offset (UE Y, Z) at the frame's alpha (0 the tick before, 1 the latest) (platformAt).
    pub fn at(&self, a: f64) -> (f64, f64) {
        (self.py + (self.cy - self.py) * a, self.pz + (self.cz - self.pz) * a)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A move that carried the platform from where it was is drawn on the way; one from elsewhere (a reset) or with
    /// no run is drawn where it is now.
    #[test]
    fn blends_only_a_carried_move() {
        let mut p = PlatformBlend::default();
        // (sim x moved by -2: UE Y +2; sim y by +1: UE Z +1)
        p.tick(2.0, 1.0, Some((-2.0, 1.0)));
        assert_eq!(p, PlatformBlend { py: 0.0, pz: 0.0, cy: 2.0, cz: 1.0 });
        assert_eq!(p.at(0.5), (1.0, 0.5));
        p.tick(50.0, 1.0, Some((-2.0, 0.0)));
        assert_eq!(p, PlatformBlend { py: 50.0, pz: 1.0, cy: 50.0, cz: 1.0 });
        p.tick(52.0, 1.0, None);
        assert_eq!(p.at(0.0), (52.0, 1.0));
    }
}
