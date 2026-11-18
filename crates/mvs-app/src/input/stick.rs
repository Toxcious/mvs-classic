//! The left stick and the direction keys of one player, as PFGPlayerController's events leave the
//! fighter's MoveXAxisValue / MoveYAxisValue.

/// UPlayerInput::MassageAxisInput (Exponent 1, Sensitivity 1): sign x max(|v| - d, 0) / (1 - d) per axis, a float32;
/// exactly 0 inside the dead zone.
pub fn massage_axis(v: f64, dead_zone: f64) -> f64 {
    let a = v.abs();
    if a.is_nan() || a <= dead_zone {
        return 0.0;
    }
    let s = if v < 0.0 { -1.0 } else { 1.0 };
    f64::from((s * ((a - dead_zone) / (1.0 - dead_zone)).min(1.0)) as f32)
}

/// A direction action.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DirKey {
    Left,
    Right,
    Up,
    Down,
}

impl DirKey {
    pub const ALL: [DirKey; 4] = [DirKey::Left, DirKey::Right, DirKey::Up, DirKey::Down];
}

/// MoveStick.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct MoveStick {
    /// PreviousAxisMove<Dir>: the direction action is down (left, right, up, down).
    pub held: [bool; 4],
    /// The fighter's MoveXAxisValue / MoveYAxisValue as the controller's events left them.
    pub x: f64,
    pub y: f64,
    /// GamePadInputEnabled: set by a non-zero MoveX or MoveY axis value (@159815 / @160628: the stick past the
    /// engine's dead zone), cleared by every Up / Down / Left / Right action, press and release.
    pub game_pad: bool,
}

impl MoveStick {
    pub fn held(&self, k: DirKey) -> bool {
        self.held[k as usize]
    }

    /// The direction actions (press @13744, @15785 ..; release X @11163-11451, Y @5387-5999): PreviousAxisMove<Dir>
    /// per direction, AxisMoveX2 / AxisMoveY2 (the key's sign on a press: of two opposite keys the last pressed wins;
    /// on a release the other key's sign when it is still flagged, else 0), the pawn's MoveX / MoveY with that int,
    /// GamePadInputEnabled = false.
    pub fn dir_action(&mut self, k: DirKey, down: bool) {
        self.held[k as usize] = down;
        let h = |d: DirKey| self.held[d as usize];
        match k {
            DirKey::Left | DirKey::Right => {
                self.x = if down {
                    if k == DirKey::Right { 1.0 } else { -1.0 }
                } else if k == DirKey::Right {
                    if h(DirKey::Left) { -1.0 } else { 0.0 }
                } else if h(DirKey::Right) {
                    1.0
                } else {
                    0.0
                };
            }
            DirKey::Up | DirKey::Down => {
                self.y = if down {
                    if k == DirKey::Up { 1.0 } else { -1.0 }
                } else if k == DirKey::Up {
                    if h(DirKey::Down) { -1.0 } else { 0.0 }
                } else if h(DirKey::Up) {
                    1.0
                } else {
                    0.0
                };
            }
        }
        self.game_pad = false;
    }

    /// The MoveX / MoveY axis events of a frame (@159148 / @159961), after the frame's key events
    /// (UPlayerInput::ProcessInputStack 142e91450 runs every action delegate before any axis delegate, and the axis
    /// delegates every frame, 0 included): a non-zero value sets GamePadInputEnabled; the pawn's MoveX / MoveY is
    /// called with the value when it is not 0 or the flag is set.
    // Unverified: MoveX is taken to run before MoveY (the binding order in the controller's component is not read).
    pub fn axis_events(&mut self, mx: f64, my: f64) {
        if mx != 0.0 {
            self.game_pad = true;
        }
        if mx != 0.0 || self.game_pad {
            self.x = mx;
        }
        if my != 0.0 {
            self.game_pad = true;
        }
        if my != 0.0 || self.game_pad {
            self.y = my;
        }
    }

    /// No key flagged (a new controller, or a window that lost the keyboard).
    pub fn clear(&mut self) {
        self.held = [false; 4];
        self.x = 0.0;
        self.y = 0.0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn opposite_keys_last_wins_and_release_falls_back() {
        let mut s = MoveStick::default();
        s.dir_action(DirKey::Left, true);
        s.dir_action(DirKey::Right, true);
        assert_eq!(s.x, 1.0);
        s.dir_action(DirKey::Right, false);
        assert_eq!(s.x, -1.0);
        s.dir_action(DirKey::Left, false);
        assert_eq!(s.x, 0.0);
    }

    #[test]
    fn centred_stick_in_pad_mode_writes_zero_over_a_key() {
        let mut s = MoveStick::default();
        s.axis_events(0.5, 0.0);
        assert!(s.game_pad);
        s.held[DirKey::Left as usize] = true;
        s.x = -1.0;
        s.axis_events(0.0, 0.0);
        assert_eq!(s.x, 0.0);
    }

    #[test]
    fn massage_is_zero_inside_and_rescaled_outside() {
        assert_eq!(massage_axis(0.3, 0.3), 0.0);
        assert_eq!(massage_axis(-1.0, 0.3), -1.0);
        assert_eq!(massage_axis(0.65, 0.3), f64::from(0.5f32));
    }
}
