//! The Linux pads, beside `pad.rs` (selected as `input::pad` on Linux): the same public surface, each pad's buttons
//! and thumbs by its controller id and the rumble's motors. For now no pad is ever found.

// Unverified: stub, replaced in Task 9

/// The same as an axis value (the input tape's pads, read as XInput's): Gamepad_LeftStick_Up past 7849 raw.
pub const XINPUT_STICK_UP: f64 = 7849.0 / 32767.0;
pub const SLOTS: usize = 4;

/// One pad's state at a poll: the buttons as bits by the standard mapping's index (mvs_ui::bindings::PAD_BUTTONS), the
/// thumbs as their analog events carry them (up positive), and Gamepad_LeftStick_Up by the reader's own threshold.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct PadState {
    pub buttons: u16,
    pub lx: f64,
    pub ly: f64,
    pub rx: f64,
    pub ry: f64,
    pub stick_up: bool,
}

impl PadState {
    /// A page pad's state (the standard mapping: the axes left X, Y, right X, Y, Y down positive, and the buttons
    /// held as bits by index), as XInput would read it (an input script's pad line, the input tape's pads).
    pub fn of_page(axes: [f64; 4], buttons: u32) -> PadState {
        let ly = -axes[1];
        PadState {
            buttons: (buttons & 0xffff) as u16,
            lx: axes[0],
            ly,
            rx: axes[2],
            ry: -axes[3],
            stick_up: ly > XINPUT_STICK_UP,
        }
    }
}

/// An axis's value as the analog event carries it (14171a330): raw / 32767 positive, raw / 32768 otherwise.
pub fn axis(raw: i16) -> f64 {
    let r = f64::from(raw);
    if raw > 0 { r / 32767.0 } else { r / 32768.0 }
}

/// Which reader's pad holds a controller id.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Owner {
    /// XInput slot n.
    XInput(u32),
    /// The Sony pad of this serial.
    Sony(u32),
}

/// The pads by controller id, polled once a frame.
#[derive(Debug, Default)]
pub struct Pads {
    /// Per controller id: its pad's state.
    pub state: [Option<PadState>; SLOTS],
    /// Per controller id: the pad that holds it.
    pub owner: [Option<Owner>; SLOTS],
}

impl Pads {
    /// Every pad read: none yet.
    pub fn poll(&mut self) {}

    /// A device arrived: nothing to look for yet.
    pub fn device_arrived(&mut self) {}

    /// The lowest controller id with a pad ('kbm+pad0''s pad), if any.
    pub fn first(&self) -> Option<usize> {
        self.state.iter().position(Option::is_some)
    }

    /// The motors of controller id `id` (the low-frequency motor `strong`, the high-frequency one `weak`, 0..1
    /// each): no pad to send them to yet.
    pub fn rumble(&mut self, _id: usize, _strong: f64, _weak: f64) {}
}
