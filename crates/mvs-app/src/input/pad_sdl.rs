//! The Linux pads, beside `pad.rs` (selected as `input::pad` on Linux): SDL3's gamepads read into the values
//! `pad.rs` gives (the XInput slots, FXInputInterface's poll 14162ddf0) and `sony.rs` gives (WinDualShock), each
//! pad's buttons and thumbs by its controller id, and the rumble's motors.

use std::time::{Duration, Instant};

use sdl3_sys::gamepad::*;
use sdl3_sys::hints::{SDL_HINT_JOYSTICK_ALLOW_BACKGROUND_EVENTS, SDL_SetHint};
use sdl3_sys::init::{SDL_INIT_GAMEPAD, SDL_InitSubSystem};
use sdl3_sys::joystick::SDL_JoystickID;
use sdl3_sys::stdinc::SDL_free;

/// XINPUT_GAMEPAD_TRIGGER_THRESHOLD: a trigger is its button past 30 / 255.
const TRIGGER_THRESHOLD: u8 = 30;
/// Gamepad_LeftStick_Up from XInput (14162ddf0's stick-as-button): the raw Y past 7849.
const XINPUT_STICK_UP_RAW: i16 = 7849;
/// The same as an axis value (the input tape's pads, read as XInput's).
pub const XINPUT_STICK_UP: f64 = 7849.0 / 32767.0;
pub const SLOTS: usize = 4;
/// SDL ends a rumble after its duration (SDL_MAX_RUMBLE_DURATION_MS, 65535 ms at most); XInput's motors run until
/// told otherwise, so a level held this long is sent again.
const RUMBLE_MS: u32 = 0xffff;
const RUMBLE_AGAIN: Duration = Duration::from_secs(60);
/// The Sony pads `sony.rs` reads (SONY_VID, Model::of): DualShock 4 (two), its wireless adapter, DualSense.
const SONY_VID: u16 = 0x054c;
const SONY_PIDS: [u16; 4] = [0x05c4, 0x09cc, 0x0ba0, 0x0ce6];

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

/// Which Windows reader's values a pad gets.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Family {
    /// `pad.rs`'s XInput reading.
    // Deviation: every pad SDL makes a gamepad (a Switch pad, a third-party PlayStation pad, a generic one) is read
    // as XInput's; Windows reads only XInput pads and the Sony pads below.
    XInput,
    /// `sony.rs`'s reading of the Sony pads.
    Sony,
}

impl Family {
    fn of(vendor: u16, product: u16) -> Family {
        if vendor == SONY_VID && SONY_PIDS.contains(&product) { Family::Sony } else { Family::XInput }
    }
}

/// A pad as SDL's gamepad API gives it: the buttons as bits by SDL_GamepadButton, the axes by SDL_GamepadAxis (the
/// sticks -32768..32767 with Y down positive, the triggers 0..32767).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
struct Snapshot {
    buttons: u32,
    axes: [i16; 6],
}

impl Snapshot {
    fn held(&self, b: SDL_GamepadButton) -> bool {
        self.buttons & (1 << b.0) != 0
    }

    fn axis(&self, a: SDL_GamepadAxis) -> i16 {
        self.axes[a.0 as usize]
    }
}

/// SDL's buttons in the standard mapping's order (the triggers and Gamepad_Special_Left apart): XInput's A, B, X, Y
/// are SDL's south, east, west, north, and so are Sony's Cross, Circle, Square, Triangle.
const SDL_BUTTONS: [(usize, SDL_GamepadButton); 13] = [
    (0, SDL_GAMEPAD_BUTTON_SOUTH),
    (1, SDL_GAMEPAD_BUTTON_EAST),
    (2, SDL_GAMEPAD_BUTTON_WEST),
    (3, SDL_GAMEPAD_BUTTON_NORTH),
    (4, SDL_GAMEPAD_BUTTON_LEFT_SHOULDER),
    (5, SDL_GAMEPAD_BUTTON_RIGHT_SHOULDER),
    (9, SDL_GAMEPAD_BUTTON_START),
    (10, SDL_GAMEPAD_BUTTON_LEFT_STICK),
    (11, SDL_GAMEPAD_BUTTON_RIGHT_STICK),
    (12, SDL_GAMEPAD_BUTTON_DPAD_UP),
    (13, SDL_GAMEPAD_BUTTON_DPAD_DOWN),
    (14, SDL_GAMEPAD_BUTTON_DPAD_LEFT),
    (15, SDL_GAMEPAD_BUTTON_DPAD_RIGHT),
];

/// XInput's raw value of an SDL stick axis: SDL's XInput reader gives Y as the raw value's bitwise not
/// (SDL_xinputjoystick.c), undone here; X is XInput's own.
fn xinput_y(v: i16) -> i16 {
    !v
}

/// XInput's trigger byte of an SDL trigger: SDL carries a byte b as (int)(b * 257 / 65535 * 32767).
fn trigger_byte(v: i16) -> u8 {
    ((i32::from(v.max(0)) * 255 + 16383) / 32767) as u8
}

/// The report's byte of an SDL stick axis: SDL's PlayStation readers carry a byte b as b * 257 - 32768.
// Unverified: exact for SDL's HIDAPI PS4 / PS5 readers; through the kernel's evdev device SDL scales the kernel's
// range instead, which for hid-playstation's 0..255 gives the same values.
fn sony_byte(v: i16) -> u8 {
    ((i32::from(v) + 32768) / 257) as u8
}

/// A stick byte as WinDualShock's analog event carries it (1409c5150): (b - 128) / 128, up positive for Y (the
/// caller passes 255 - b).
fn stick(b: u8) -> f64 {
    (f64::from(b) - 128.0) * 0.0078125
}

/// The pad's state as the Windows reader of its family gives it.
fn state_of(f: Family, s: &Snapshot) -> PadState {
    let mut buttons = 0u16;
    for (bit, b) in SDL_BUTTONS {
        if s.held(b) {
            buttons |= 1 << bit;
        }
    }
    // (Gamepad_Special_Left: XInput's Back; on a Sony pad the touch pad's click, Share / Create and PS map to no key)
    let special_left = match f {
        Family::XInput => SDL_GAMEPAD_BUTTON_BACK,
        Family::Sony => SDL_GAMEPAD_BUTTON_TOUCHPAD,
    };
    if s.held(special_left) {
        buttons |= 1 << 8;
    }
    // Deviation: a Sony pad's L2 / R2 are its report's own button bits on Windows; SDL gives only the analog
    // triggers, so they press past XInput's threshold as an XInput pad's do.
    if trigger_byte(s.axis(SDL_GAMEPAD_AXIS_LEFT_TRIGGER)) > TRIGGER_THRESHOLD {
        buttons |= 1 << 6;
    }
    if trigger_byte(s.axis(SDL_GAMEPAD_AXIS_RIGHT_TRIGGER)) > TRIGGER_THRESHOLD {
        buttons |= 1 << 7;
    }
    let (lx, ly) = (s.axis(SDL_GAMEPAD_AXIS_LEFTX), s.axis(SDL_GAMEPAD_AXIS_LEFTY));
    let (rx, ry) = (s.axis(SDL_GAMEPAD_AXIS_RIGHTX), s.axis(SDL_GAMEPAD_AXIS_RIGHTY));
    match f {
        Family::XInput => PadState {
            buttons,
            lx: axis(lx),
            ly: axis(xinput_y(ly)),
            rx: axis(rx),
            ry: axis(xinput_y(ry)),
            stick_up: xinput_y(ly) > XINPUT_STICK_UP_RAW,
        },
        Family::Sony => PadState {
            buttons,
            lx: stick(sony_byte(lx)),
            ly: stick(255 - sony_byte(ly)),
            rx: stick(sony_byte(rx)),
            ry: stick(255 - sony_byte(ry)),
            stick_up: sony_byte(ly) < 0x40,
        },
    }
}

/// The motor levels SDL_RumbleGamepad takes for strong / weak (0..1): XInput's XINPUT_VIBRATION speeds, or
/// WinDualShock's (int)(v * 255) per motor (1409c5c20) as the 16-bit level whose high byte SDL's Sony readers send.
fn motor_levels(f: Family, strong: f64, weak: f64) -> (u16, u16) {
    match f {
        Family::XInput => ((strong.clamp(0.0, 1.0) * 65535.0) as u16, (weak.clamp(0.0, 1.0) * 65535.0) as u16),
        Family::Sony => {
            // (the float's int, a negative one 0)
            let byte = |v: f64| u16::from(((v as f32 * 255.0) as i32).clamp(0, 255) as u8);
            (byte(strong) * 257, byte(weak) * 257)
        }
    }
}

/// Which pad holds a controller id.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Owner {
    /// The SDL gamepad of this joystick instance id.
    Gamepad(u32),
}

/// A pad SDL lists at a poll.
#[derive(Clone, Copy, Debug)]
struct Found {
    owner: Owner,
    /// SDL's gamepad type is an Xbox 360 or Xbox One pad's (an XInput pad on Windows).
    xbox: bool,
    state: PadState,
}

/// The pads by controller id, polled once a frame.
#[derive(Debug, Default)]
pub struct Pads {
    /// Per controller id: its pad's state.
    pub state: [Option<PadState>; SLOTS],
    /// Per controller id: the pad that holds it.
    pub owner: [Option<Owner>; SLOTS],
    /// The motors last sent per controller id (low, high frequency), 0..1, and when.
    sent: [(f64, f64); SLOTS],
    sent_at: [Option<Instant>; SLOTS],
    /// SDL's gamepad subsystem: None until the first poll starts it, then whether it started.
    started: Option<bool>,
    /// The gamepads this reader opened, by instance id, with their family.
    open: Vec<(u32, Family)>,
}

impl Pads {
    /// The pad `o`'s state this poll: a newly found pad takes the lowest free id (1413a1930), a gone one frees its
    /// own (1413a0e60).
    // (a copy of `pad.rs`'s Pads::set, `pad.rs:143`, which keys the rule by its own readers; the copy stays until
    // the maintainer decides whether one rule shared by both platforms is a change he wants)
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
                    self.sent_at[id] = None;
                }
            }
            (None, None) => {}
        }
    }

    /// One poll's pads into the ids: a pad no longer listed frees its id, then the listed ones are set, the Xbox-style
    /// ones first (FWindowsApplication's PollGameDeviceState 141628950 runs the XInput poll before WinDualShock's, so
    /// on the frame both find a pad the XInput one takes the lower id), each kind in SDL's order.
    fn take(&mut self, found: &[Found]) {
        for o in self.owner {
            if let Some(o) = o
                && !found.iter().any(|f| f.owner == o)
            {
                self.set(o, None);
            }
        }
        let mut order: Vec<&Found> = found.iter().collect();
        order.sort_by_key(|f| !f.xbox);
        for f in order {
            self.set(f.owner, Some(f.state));
        }
    }

    /// Every pad read: the gamepads SDL lists (SDL finds pads coming and going itself, as the window's event pump
    /// runs), each opened when first seen and closed when gone.
    pub fn poll(&mut self) {
        if *self.started.get_or_insert_with(start) {
            let found = self.read();
            self.take(&found);
            self.rumble_again();
        }
    }

    /// The gamepads SDL lists now, opened, with their states.
    fn read(&mut self) -> Vec<Found> {
        let ids = gamepads();
        self.open.retain(|&(j, _)| {
            let here = ids.contains(&j);
            if !here {
                // SAFETY: the gamepad subsystem is up; a gamepad this reader opened is closed once.
                unsafe {
                    let g = SDL_GetGamepadFromID(SDL_JoystickID(j));
                    if !g.is_null() {
                        SDL_CloseGamepad(g);
                    }
                }
            }
            here
        });
        let mut found = Vec::new();
        for j in ids {
            let id = SDL_JoystickID(j);
            let family = match self.open.iter().find(|o| o.0 == j) {
                Some(&(_, f)) => f,
                None => {
                    // SAFETY: an instance id SDL listed this poll.
                    let (g, vendor, product) =
                        unsafe { (SDL_OpenGamepad(id), SDL_GetGamepadVendorForID(id), SDL_GetGamepadProductForID(id)) };
                    if g.is_null() {
                        continue;
                    }
                    let f = Family::of(vendor, product);
                    self.open.push((j, f));
                    f
                }
            };
            // SAFETY: an instance id this reader opened; SDL_GetGamepadFromID gives its gamepad or null.
            let (g, kind) = unsafe { (SDL_GetGamepadFromID(id), SDL_GetGamepadTypeForID(id)) };
            if g.is_null() {
                continue;
            }
            let mut s = Snapshot::default();
            // SAFETY: an open gamepad; the buttons and axes are SDL's own enumerations.
            unsafe {
                for b in 0..SDL_GAMEPAD_BUTTON_COUNT.0 {
                    if SDL_GetGamepadButton(g, SDL_GamepadButton(b)) {
                        s.buttons |= 1 << b;
                    }
                }
                for (a, v) in s.axes.iter_mut().enumerate() {
                    *v = SDL_GetGamepadAxis(g, SDL_GamepadAxis(a as i32));
                }
            }
            found.push(Found {
                owner: Owner::Gamepad(j),
                xbox: kind == SDL_GAMEPAD_TYPE_XBOX360 || kind == SDL_GAMEPAD_TYPE_XBOXONE,
                state: state_of(family, &s),
            });
        }
        found
    }

    /// A device arrived: SDL finds new pads itself.
    pub fn device_arrived(&mut self) {}

    /// The lowest controller id with a pad ('kbm+pad0''s pad), if any.
    pub fn first(&self) -> Option<usize> {
        self.state.iter().position(Option::is_some)
    }

    /// The motors of controller id `id` (the low-frequency motor `strong`, the high-frequency one `weak`, 0..1
    /// each), sent on a change over 1e-8: XInput's XINPUT_VIBRATION (14162ddf0), WinDualShock's
    /// ScePadVibrationParam of (int)(v * 255) per motor (1409c5c20).
    // Unverified: SDL_RumbleGamepad on a pad is not tried on hardware (the pad it was tested with has no motors).
    pub fn rumble(&mut self, id: usize, strong: f64, weak: f64) {
        let (s0, w0) = self.sent[id];
        if (strong - s0).abs() <= 1e-8 && (weak - w0).abs() <= 1e-8 {
            return;
        }
        self.sent[id] = (strong, weak);
        self.send(id);
    }

    /// Controller id `id`'s motors as last asked, to its gamepad.
    fn send(&mut self, id: usize) {
        let Some(Owner::Gamepad(j)) = self.owner[id] else { return };
        let Some(&(_, f)) = self.open.iter().find(|o| o.0 == j) else { return };
        let (strong, weak) = self.sent[id];
        let (low, high) = motor_levels(f, strong, weak);
        self.sent_at[id] = Some(Instant::now());
        // SAFETY: an instance id this reader opened; SDL_GetGamepadFromID gives its gamepad or null.
        unsafe {
            let g = SDL_GetGamepadFromID(SDL_JoystickID(j));
            if !g.is_null() {
                SDL_RumbleGamepad(g, low, high, RUMBLE_MS);
            }
        }
    }

    /// The motors running since RUMBLE_AGAIN sent again, before SDL's duration ends them.
    fn rumble_again(&mut self) {
        for id in 0..SLOTS {
            if self.sent[id] != (0.0, 0.0) && self.sent_at[id].is_some_and(|t| t.elapsed() >= RUMBLE_AGAIN) {
                self.send(id);
            }
        }
    }
}

impl Drop for Pads {
    fn drop(&mut self) {
        for &(j, _) in &self.open {
            // SAFETY: a gamepad this reader opened, closed once.
            unsafe {
                let g = SDL_GetGamepadFromID(SDL_JoystickID(j));
                if !g.is_null() {
                    SDL_CloseGamepad(g);
                }
            }
        }
    }
}

/// SDL's gamepad subsystem started (on the main thread, from the first poll); false when it would not start.
fn start() -> bool {
    // (SDL drops a pad's input while no window of the program has the keyboard, SDL_joystick.c's
    // SDL_PrivateJoystickShouldIgnoreEvent; XInput, which the Windows file reads, does not, so the pads are read in the
    // background here too)
    // SAFETY: a hint set and SDL_InitSubSystem, from the thread that runs the window's event pump; the strings are
    // static and nul-terminated.
    unsafe {
        SDL_SetHint(SDL_HINT_JOYSTICK_ALLOW_BACKGROUND_EVENTS, c"1".as_ptr());
        SDL_InitSubSystem(SDL_INIT_GAMEPAD)
    }
}

/// The instance ids of the gamepads SDL lists, in its order.
fn gamepads() -> Vec<u32> {
    let mut n = 0;
    // SAFETY: SDL_GetGamepads writes the count and returns an array SDL allocated (null on failure), freed here.
    unsafe {
        let p = SDL_GetGamepads(&mut n);
        if p.is_null() {
            return Vec::new();
        }
        let ids = std::slice::from_raw_parts(p, n.max(0) as usize).iter().map(|j| j.0).collect();
        SDL_free(p.cast());
        ids
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pad(n: u32, xbox: bool) -> Found {
        Found { owner: Owner::Gamepad(n), xbox, state: PadState::default() }
    }

    #[test]
    fn a_pad_takes_the_lowest_free_id() {
        let mut p = Pads::default();
        p.take(&[pad(7, false)]);
        p.take(&[pad(7, false), pad(3, false)]);
        assert_eq!(p.owner, [Some(Owner::Gamepad(7)), Some(Owner::Gamepad(3)), None, None]);
        p.take(&[pad(3, false)]);
        assert_eq!(p.first(), Some(1));
        p.take(&[pad(3, false), pad(9, false)]);
        assert_eq!(p.owner[..2], [Some(Owner::Gamepad(9)), Some(Owner::Gamepad(3))]);
    }

    #[test]
    fn an_id_comes_back_when_its_pad_goes() {
        let mut p = Pads::default();
        p.take(&[pad(1, false), pad(2, false)]);
        p.take(&[pad(2, false)]);
        assert_eq!(p.owner, [None, Some(Owner::Gamepad(2)), None, None]);
        assert_eq!(p.state[0], None);
        // (the same pad back, under SDL's new instance id)
        p.take(&[pad(2, false), pad(5, false)]);
        assert_eq!(p.owner[..2], [Some(Owner::Gamepad(5)), Some(Owner::Gamepad(2))]);
        assert!(p.state[0].is_some());
    }

    #[test]
    fn two_arrivals_in_one_poll_xbox_style_first() {
        let mut p = Pads::default();
        p.take(&[pad(1, false), pad(2, true)]);
        assert_eq!(p.owner[..2], [Some(Owner::Gamepad(2)), Some(Owner::Gamepad(1))]);
        // (a pad already holding its id keeps it whatever arrives)
        p.take(&[pad(1, false), pad(2, true), pad(3, true)]);
        assert_eq!(p.owner[..3], [Some(Owner::Gamepad(2)), Some(Owner::Gamepad(1)), Some(Owner::Gamepad(3))]);
    }

    #[test]
    fn a_fifth_pad_gets_no_id() {
        let mut p = Pads::default();
        p.take(&[pad(1, true), pad(2, true), pad(3, true), pad(4, true), pad(5, true)]);
        assert!(!p.owner.contains(&Some(Owner::Gamepad(5))));
        assert_eq!(p.state.iter().flatten().count(), SLOTS);
        p.take(&[pad(2, true), pad(3, true), pad(4, true), pad(5, true)]);
        assert_eq!(p.owner[0], Some(Owner::Gamepad(5)));
    }

    /// SDL's trigger value of an XInput trigger byte (SDL_xinputjoystick.c's b * 257 - 32768, mapped by
    /// SDL_GetGamepadAxis to 0..32767).
    fn sdl_trigger(b: u8) -> i16 {
        ((f32::from(b) * 257.0 / 65535.0) * 32767.0) as i16
    }

    fn snapshot(buttons: &[SDL_GamepadButton], axes: [i16; 6]) -> Snapshot {
        Snapshot { buttons: buttons.iter().fold(0, |m, b| m | 1 << b.0), axes }
    }

    #[test]
    fn an_sdl_pad_reads_as_xinput_does() {
        // XInput: A, LB, Back, d-pad left; thumbs L (-32768, 7850), R (32767, -100); triggers 31 and 30. As SDL's
        // XInput reader gives it: Y as its bitwise not.
        let s = snapshot(
            &[
                SDL_GAMEPAD_BUTTON_SOUTH,
                SDL_GAMEPAD_BUTTON_LEFT_SHOULDER,
                SDL_GAMEPAD_BUTTON_BACK,
                SDL_GAMEPAD_BUTTON_DPAD_LEFT,
                SDL_GAMEPAD_BUTTON_GUIDE,
            ],
            [-32768, !7850, 32767, !-100, sdl_trigger(31), sdl_trigger(30)],
        );
        let want = PadState {
            buttons: 1 | 1 << 4 | 1 << 6 | 1 << 8 | 1 << 14,
            lx: -1.0,
            ly: 7850.0 / 32767.0,
            rx: 1.0,
            ry: -100.0 / 32768.0,
            stick_up: true,
        };
        assert_eq!(state_of(Family::XInput, &s), want);
        // (7849 is not past XInput's threshold; a full trigger is pressed)
        let s = snapshot(&[], [0, !7849, 0, 0, sdl_trigger(255), 0]);
        assert!(!state_of(Family::XInput, &s).stick_up);
        assert_eq!(state_of(Family::XInput, &s).buttons, 1 << 6);
    }

    #[test]
    fn a_sony_pad_reads_as_windualshock_does() {
        // (bytes: L (0, 63), R (255, 128); Cross, Options, the touch pad's click; Share is no key)
        let b = |b: i32| (b * 257 - 32768) as i16;
        let s = snapshot(
            &[SDL_GAMEPAD_BUTTON_SOUTH, SDL_GAMEPAD_BUTTON_START, SDL_GAMEPAD_BUTTON_TOUCHPAD, SDL_GAMEPAD_BUTTON_BACK],
            [b(0), b(63), b(255), b(128), 0, 0],
        );
        let want =
            PadState { buttons: 1 | 1 << 8 | 1 << 9, lx: -1.0, ly: 0.5, rx: 0.9921875, ry: -0.0078125, stick_up: true };
        assert_eq!(state_of(Family::Sony, &s), want);
        assert!(!state_of(Family::Sony, &snapshot(&[], [0, b(64), 0, 0, 0, 0])).stick_up);
    }

    #[test]
    fn the_motors_scale_as_each_reader_sends_them() {
        assert_eq!(motor_levels(Family::XInput, 0.5, 2.0), (32767, 65535));
        assert_eq!(motor_levels(Family::Sony, 0.5, -1.0), (127 * 257, 0));
        assert_eq!(Family::of(0x054c, 0x0ce6), Family::Sony);
        assert_eq!(Family::of(0x0e6f, 0x0259), Family::XInput);
    }
}
