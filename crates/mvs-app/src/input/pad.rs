//! The pads: the XInput slots (FXInputInterface's poll 14162ddf0) and the Sony pads over HID (WinDualShock,
//! `sony`), each pad's buttons and thumbs by its controller id, and the rumble's motors.
//!
//! Controller ids (1413a1930 / 1413a0e60): one pool of four shared by both readers; a pad found connected takes the
//! lowest free id and gives it back when it goes. FWindowsApplication's PollGameDeviceState (141628950) runs the
//! XInput poll before the external devices' (WinDualShock's), so on the frame both find a pad the XInput one takes
//! the lower id.

use std::time::{Duration, Instant};

use windows::Win32::UI::Input::XboxController::*;

use super::sony::SonyPads;

/// XINPUT_GAMEPAD_TRIGGER_THRESHOLD: a trigger is its button past 30 / 255.
const TRIGGER_THRESHOLD: u8 = 30;
/// Gamepad_LeftStick_Up from XInput (14162ddf0's stick-as-button): the raw Y past 7849.
const XINPUT_STICK_UP_RAW: i16 = 7849;
/// The same as an axis value (the input tape's pads, read as XInput's).
pub const XINPUT_STICK_UP: f64 = 7849.0 / 32767.0;
/// How often a slot found empty is asked again (an empty slot's XInputGetState is slow).
const EMPTY_RETRY: Duration = Duration::from_secs(1);
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

/// The XInput buttons in the standard mapping's order (the triggers apart).
const XBUTTONS: [(usize, XINPUT_GAMEPAD_BUTTON_FLAGS); 14] = [
    (0, XINPUT_GAMEPAD_A),
    (1, XINPUT_GAMEPAD_B),
    (2, XINPUT_GAMEPAD_X),
    (3, XINPUT_GAMEPAD_Y),
    (4, XINPUT_GAMEPAD_LEFT_SHOULDER),
    (5, XINPUT_GAMEPAD_RIGHT_SHOULDER),
    (8, XINPUT_GAMEPAD_BACK),
    (9, XINPUT_GAMEPAD_START),
    (10, XINPUT_GAMEPAD_LEFT_THUMB),
    (11, XINPUT_GAMEPAD_RIGHT_THUMB),
    (12, XINPUT_GAMEPAD_DPAD_UP),
    (13, XINPUT_GAMEPAD_DPAD_DOWN),
    (14, XINPUT_GAMEPAD_DPAD_LEFT),
    (15, XINPUT_GAMEPAD_DPAD_RIGHT),
];

/// XInput slot `k`'s pad, None when the slot is empty.
fn xinput(k: u32) -> Option<PadState> {
    let mut s = XINPUT_STATE::default();
    // SAFETY: a slot index below XUSER_MAX_COUNT and a valid out-pointer.
    if unsafe { XInputGetState(k, &mut s) } != 0 {
        return None;
    }
    let g = s.Gamepad;
    let mut buttons = 0u16;
    for (bit, f) in XBUTTONS {
        if g.wButtons.0 & f.0 != 0 {
            buttons |= 1 << bit;
        }
    }
    if g.bLeftTrigger > TRIGGER_THRESHOLD {
        buttons |= 1 << 6;
    }
    if g.bRightTrigger > TRIGGER_THRESHOLD {
        buttons |= 1 << 7;
    }
    Some(PadState {
        buttons,
        lx: axis(g.sThumbLX),
        ly: axis(g.sThumbLY),
        rx: axis(g.sThumbRX),
        ry: axis(g.sThumbRY),
        stick_up: g.sThumbLY > XINPUT_STICK_UP_RAW,
    })
}

/// Which reader's pad holds a controller id.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Owner {
    /// XInput slot n.
    XInput(u32),
    /// The Sony pad of this serial (`SonyPads`).
    Sony(u32),
}

/// The pads by controller id, polled once a frame.
#[derive(Debug)]
pub struct Pads {
    /// Per controller id: its pad's state.
    pub state: [Option<PadState>; SLOTS],
    /// Per controller id: the pad that holds it.
    pub owner: [Option<Owner>; SLOTS],
    /// When an empty XInput slot is asked next.
    retry: [Option<Instant>; SLOTS],
    /// The motors last sent per controller id (low, high frequency), 0..1.
    sent: [(f64, f64); SLOTS],
    pub sony: SonyPads,
}

impl Default for Pads {
    fn default() -> Pads {
        Pads {
            state: [None; SLOTS],
            owner: [None; SLOTS],
            retry: [None; SLOTS],
            sent: [(0.0, 0.0); SLOTS],
            sony: SonyPads::default(),
        }
    }
}

impl Pads {
    /// The pad `o`'s state this poll: a newly found pad takes the lowest free id (1413a1930), a gone one frees its
    /// own (1413a0e60).
    fn set(&mut self, o: Owner, s: Option<PadState>) {
        let id = self.owner.iter().position(|x| *x == Some(o));
        match (id, s) {
            (Some(id), Some(s)) => self.state[id] = Some(s),
            (Some(id), None) => {
                self.owner[id] = None;
                self.state[id] = None;
            }
            (None, Some(s)) => {
                // (a fifth pad gets no id and is not read)
                if let Some(id) = self.owner.iter().position(Option::is_none) {
                    self.owner[id] = Some(o);
                    self.state[id] = Some(s);
                    self.sent[id] = (0.0, 0.0);
                }
            }
            (None, None) => {}
        }
    }

    /// Every pad read: the XInput slots (an empty one at most once a second), then the Sony pads.
    pub fn poll(&mut self) {
        let now = Instant::now();
        for k in 0..SLOTS {
            if self.retry[k].is_some_and(|t| now < t) {
                continue;
            }
            let s = xinput(k as u32);
            self.retry[k] = if s.is_none() { Some(now + EMPTY_RETRY) } else { None };
            self.set(Owner::XInput(k as u32), s);
        }
        self.sony.poll();
        for id in 0..SLOTS {
            if let Some(Owner::Sony(n)) = self.owner[id]
                && !self.sony.open(n)
            {
                self.owner[id] = None;
                self.state[id] = None;
            }
        }
        // Unverified: of two Sony pads found on one poll, the one opened first takes the lower id (libScePad's
        // assignment of pads to WinDualShock's four scePadOpen users 1..4 is not read).
        let sony: Vec<_> = self.sony.states().collect();
        for (n, s) in sony {
            self.set(Owner::Sony(n), s);
        }
        self.sony.flush();
    }

    /// A HID device arrived (WM_DEVICECHANGE): the Sony pads are looked for again at the next poll.
    pub fn device_arrived(&mut self) {
        self.sony.rescan();
    }

    /// The lowest controller id with a pad ('kbm+pad0''s pad), if any.
    pub fn first(&self) -> Option<usize> {
        self.state.iter().position(Option::is_some)
    }

    /// The motors of controller id `id` (the low-frequency motor `strong`, the high-frequency one `weak`, 0..1
    /// each), sent on a change over 1e-8: XInput's XINPUT_VIBRATION (14162ddf0), WinDualShock's
    /// ScePadVibrationParam of (int)(v * 255) per motor (1409c5c20).
    pub fn rumble(&mut self, id: usize, strong: f64, weak: f64) {
        let (s0, w0) = self.sent[id];
        if (strong - s0).abs() <= 1e-8 && (weak - w0).abs() <= 1e-8 {
            return;
        }
        self.sent[id] = (strong, weak);
        match self.owner[id] {
            Some(Owner::XInput(k)) => {
                let v = XINPUT_VIBRATION {
                    wLeftMotorSpeed: (strong.clamp(0.0, 1.0) * 65535.0) as u16,
                    wRightMotorSpeed: (weak.clamp(0.0, 1.0) * 65535.0) as u16,
                };
                // SAFETY: a slot index below XUSER_MAX_COUNT and a valid vibration struct.
                unsafe {
                    XInputSetState(k, &v);
                }
            }
            Some(Owner::Sony(n)) => {
                // (the float's int, a negative one 0)
                let byte = |v: f64| ((v as f32 * 255.0) as i32).max(0) as u8;
                self.sony.rumble(n, byte(strong), byte(weak));
            }
            None => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_go_to_the_lowest_free() {
        let mut p = Pads::default();
        let s = Some(PadState::default());
        p.set(Owner::XInput(2), s);
        p.set(Owner::Sony(0), s);
        assert_eq!(p.owner[..2], [Some(Owner::XInput(2)), Some(Owner::Sony(0))]);
        p.set(Owner::XInput(2), None);
        assert_eq!(p.first(), Some(1));
        p.set(Owner::XInput(0), s);
        assert_eq!(p.owner[0], Some(Owner::XInput(0)));
    }
}
