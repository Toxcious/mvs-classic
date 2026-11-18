//! The menus' input: the raw events a frame hands the
//! view, the pads' polled buttons and sticks turned into key and analog events (`PadPoller`), the window's pointer
//! the screens share (`Pointer`).

use std::collections::{HashMap, HashSet};

use super::manager::Interaction;
use crate::bindings::PAD_BUTTONS;

/// One input event of a frame, in the order it came.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum MenuInput {
    /// A key or a pad button went down or up (its FKey name); a down of a key already down is a repeat. `player`:
    /// the local player whose device it came from.
    Key { key: &'static str, down: bool, repeat: bool, player: usize },
    /// The mouse moved to `x`, `y` in device pixels of the window's client area.
    MouseMove { x: f64, y: f64 },
    /// A mouse button (0 left, 1 middle, 2 right) went down or up.
    MouseButton { button: u8, down: bool },
    /// The wheel turned this many notches (up positive).
    Wheel { notches: f64 },
    /// An analog axis's value (Gamepad_LeftX, Gamepad_RightY...), -1..=1.
    Analog { key: &'static str, value: f64, player: usize },
    /// A character typed (a text box's input).
    Char(char),
    /// A standard-mapping pad's whole state this frame (read once a frame):
    /// `pad` its device index, `player` the local player that holds it. The screen's `PadPoller` turns it into
    /// keys and analog values.
    Pad { pad: usize, player: usize, state: MenuPad },
}

/// A pad's state as the page's Gamepad gives it: the axes (left X, left Y, right X, right Y; Y down positive) and
/// the buttons held, one bit per index of `PAD_BUTTONS`.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct MenuPad {
    pub axes: [f64; 4],
    pub buttons: u32,
}

/// The left stick's key threshold (native exe read: |axis| past 7849 / 32767), also its analog dead zone.
pub const LEFT_STICK_DEAD_ZONE: f64 = 7849.0 / 32767.0;
/// The right stick's analog dead zone.
pub const RIGHT_STICK_DEAD_ZONE: f64 = 8689.0 / 32767.0;
/// A stick key's repeat: 0.2 s after the press, then every 0.1 s.
pub const STICK_KEY_REPEAT_FIRST: f64 = 0.2;
pub const STICK_KEY_REPEAT: f64 = 0.1;
/// The left stick as the keys Gamepad_LeftStick_Left / Right / Up / Down: axis, sign, key.
const STICK_KEYS: [(usize, f64, &str); 4] = [
    (0, -1.0, "Gamepad_LeftStick_Left"),
    (0, 1.0, "Gamepad_LeftStick_Right"),
    (1, -1.0, "Gamepad_LeftStick_Up"),
    (1, 1.0, "Gamepad_LeftStick_Down"),
];
const AXIS_KEYS: [&str; 4] = ["Gamepad_LeftX", "Gamepad_LeftY", "Gamepad_RightX", "Gamepad_RightY"];

/// What a pad's poll gives, in order.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum PadEvent {
    /// A button or a stick direction pressed (a stick key again at its repeat) by `pad`: the owner's `take` first,
    /// else the view's keyDown.
    Press { key: &'static str, player: usize, pad: usize, button: bool },
    /// Let go: the manager's keyUp.
    Release { key: &'static str, player: usize },
    /// The raw left stick axis for a listening binding row (view.analog of lx, then ly).
    Raw { value: f64 },
    /// The manager's Process Analog Value Changed with the engine's keys and signs (Y up positive), `frames` the
    /// frame's length in 60 Hz frames.
    Analog { key: &'static str, value: f64, player: usize, frames: f64 },
}

#[derive(Debug, Default)]
struct PadPoll {
    held: HashSet<&'static str>,
    /// Each held stick key's next repeat (s of the poller's clock).
    repeat: HashMap<&'static str, f64>,
    /// The axes as last sent (lx, ly, rx, ry).
    sent: [f64; 4],
}

/// A view's pads, polled once a frame: the buttons and the left stick's directions as key
/// presses, the sticks as analog values. Used by a menu screen and by the Lab Tools panel over the match.
#[derive(Debug, Default)]
pub struct PadPoller {
    polls: Vec<Option<PadPoll>>,
}

impl PadPoller {
    pub fn new() -> PadPoller {
        PadPoller::default()
    }

    /// One frame's poll of `pads` (device index, player, state; the pads a player holds), `first` the frame after
    /// the view took no input (a button down then is held, not pressed), `dt` the frame's s, `now` the clock of
    /// the stick keys' repeat (s). The events go to `out` in order.
    pub fn poll(
        &mut self,
        pads: &[(usize, usize, MenuPad)],
        first: bool,
        dt: f64,
        now: f64,
        out: &mut dyn FnMut(PadEvent),
    ) {
        for (i, p) in self.polls.iter_mut().enumerate() {
            if let Some(p) = p
                && (first || !pads.iter().any(|(d, _, _)| *d == i))
            {
                p.held.clear();
            }
        }
        for &(index, player, ref pad) in pads {
            if self.polls.len() <= index {
                self.polls.resize_with(index + 1, || None);
            }
            let st = self.polls[index].get_or_insert_with(PadPoll::default);
            poll_pad(pad, index, player, st, first, dt, now, out);
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn poll_pad(
    pad: &MenuPad,
    index: usize,
    p: usize,
    st: &mut PadPoll,
    first: bool,
    dt: f64,
    now: f64,
    out: &mut dyn FnMut(PadEvent),
) {
    let [lx, ly, rx, ry] = pad.axes;
    // (the mouse moves a stick makes through the OS are the window's: Devices::mouse_moved)
    for (axis, sign, key) in STICK_KEYS {
        let down = (if axis == 1 { ly } else { lx }) * sign > LEFT_STICK_DEAD_ZONE;
        if down && first {
            st.held.insert(key);
            st.repeat.insert(key, now + STICK_KEY_REPEAT_FIRST);
        } else if down && !st.held.contains(key) {
            st.held.insert(key);
            st.repeat.insert(key, now + STICK_KEY_REPEAT_FIRST);
            out(PadEvent::Press { key, player: p, pad: index, button: false });
        } else if down && now >= st.repeat.get(key).copied().unwrap_or(f64::INFINITY) {
            st.repeat.insert(key, now + STICK_KEY_REPEAT);
            out(PadEvent::Press { key, player: p, pad: index, button: false });
        } else if !down && st.held.remove(key) {
            out(PadEvent::Release { key, player: p });
        }
    }
    for (i, key) in PAD_BUTTONS.iter().copied().enumerate() {
        let down = pad.buttons & (1 << i) != 0;
        if down && first {
            st.held.insert(key);
        } else if down && !st.held.contains(key) {
            st.held.insert(key);
            out(PadEvent::Press { key, player: p, pad: index, button: true });
        } else if !down && st.held.remove(key) {
            out(PadEvent::Release { key, player: p });
        }
    }
    out(PadEvent::Raw { value: lx });
    out(PadEvent::Raw { value: ly });
    // (the port's own) the right stick's scroll step of a frame is scaled by the frame's length in 60 Hz frames
    let frames = dt * 60.0;
    for (i, key) in AXIS_KEYS.into_iter().enumerate() {
        let v = match i {
            0 => lx,
            1 => -ly,
            2 => rx,
            _ => -ry,
        };
        let dead = if i < 2 { LEFT_STICK_DEAD_ZONE } else { RIGHT_STICK_DEAD_ZONE };
        if v.abs() <= dead && v == st.sent[i] {
            continue;
        }
        st.sent[i] = v;
        out(PadEvent::Analog { key, value: v, player: p, frames });
    }
}

/// How long after a stick was past its dead zone a mouse move is the stick's (the
/// port's own: a pad layer of the OS or of Steam that moves the OS pointer with a stick). The window drops such a
/// move before any screen sees it (mvs-app Devices::mouse_moved).
pub const STICK_MOUSE_GRACE: f64 = 0.25;

/// What the player touched, as the window saw it (the port's own): a keyboard key or a pad button pressed (its FKey
/// name; a stick past its key threshold is its Gamepad_LeftStick_* key), a hand's mouse move of more than a pixel, a
/// mouse button pressed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Touch {
    Key(&'static str),
    MouseMove,
    MouseButton,
}

/// The window's pointer as the port keeps it (the port's own): player 0's point in device pixels of
/// the client area, written by the mouse and by the game's cursor (the left stick), one for the window so a new
/// menu screen starts where the last one left it (UI_Global and its cursor outlive a level in the build); the last
/// interaction every screen's manager starts from; whether a screen owns the pointer this frame (the OS cursor is
/// not shown then).
///
/// The last interaction (`interaction`) is the one model of the device the player last touched: the manager of an
/// active screen that took the frame's input keeps it (screen.rs writes it back at the frame's end), and in a frame
/// no screen took input (the match's play, the title, a closed panel) the window's touches do (`touched`).
#[derive(Debug, Default)]
pub struct Pointer {
    /// None until a mouse move or a cursor's start position set it.
    pub at: Option<[f64; 2]>,
    /// Counts the writes: a view copies the point when it has not seen the last one.
    pub version: u64,
    /// The OS pointer's last point as a screen last saw it, what a mouse move's distance is measured from.
    pub mouse: Option<[f64; 2]>,
    /// The game's cursor wrote the point this frame (UI_Cursor's Set Position): where the window moves the OS pointer.
    warp: Option<[f64; 2]>,
    /// Player 0's LastInteractionType, InputType and cursorMode (the build's one UI_Manager's).
    pub interaction: Interaction,
    /// An active screen with the game's cursor ran this frame (hideOsCursor's owner).
    os_cursor_claimed: bool,
    /// An active screen took input this frame: its manager wrote `interaction`.
    screen_input: bool,
    /// UI_Cursor's MousePosNormalized (player 0's, what BP_Banner_PM's parallax reads): the class default (0, 0)
    /// until the first OffsetView, kept across screens as the build's one cursor keeps it.
    pub mouse_pos_normalized: [f64; 2],
    /// MPC_MousePOS's MousePos vector, which OffsetView writes (R, G eased toward MousePosNormalized, B, A it):
    /// what M_ProgressionParallax reads. The collection's default (0, 0, 0, 0) until the first.
    pub mpc_mouse_pos: [f64; 4],
    /// (tests) MousePosNormalized held at its value over what OffsetView writes.
    pub mouse_pos_pinned: bool,
}

impl Pointer {
    /// setPagePointer.
    pub fn set(&mut self, x: f64, y: f64) {
        if self.at == Some([x, y]) {
            return;
        }
        self.at = Some([x, y]);
        self.version += 1;
    }

    /// UI_Cursor's Set Position: PC.SetMouseLocation (UI_Cursor @281), which moves the OS pointer itself (SetCursorPos
    /// through FSceneViewport::SetMouse). The point is the window's at once; the
    /// program moves the OS pointer there after the frame (`take_warp`), so the mouse goes on from where the stick
    /// left it.
    pub fn set_by_game(&mut self, x: f64, y: f64) {
        self.set(x, y);
        self.warp = Some([x, y]);
    }

    /// Where the game's cursor put the pointer since the last call, for the window to move the OS pointer there.
    pub fn take_warp(&mut self) -> Option<[f64; 2]> {
        self.warp.take()
    }

    /// The window's frame begins: no screen has owned the pointer or taken input in it yet.
    pub fn begin_frame(&mut self) {
        self.os_cursor_claimed = false;
        self.screen_input = false;
    }

    /// The window's mouse moves of the frame, before any screen's: the mouse writes the point whatever shows (a
    /// match, a loading screen), so the next menu screen opens with the pointer where the mouse is.
    /// The moves a stick made through the OS never get here.
    pub fn window_moves(&mut self, input: &[MenuInput]) {
        for ev in input {
            if let MenuInput::MouseMove { x, y } = *ev {
                self.set(x, y);
            }
        }
    }

    /// An active screen with the game's cursor ran this frame: the OS cursor is not shown over the window.
    pub fn claim_os_cursor(&mut self) {
        self.os_cursor_claimed = true;
    }

    /// The OS cursor is not shown over the window this frame (someone owns the pointer); the app asks after the
    /// screens' frames.
    pub fn os_cursor_hidden(&self) -> bool {
        self.os_cursor_claimed
    }

    /// An active screen's manager took the frame's input and wrote `interaction`.
    pub fn screen_took_input(&mut self) {
        self.screen_input = true;
    }

    /// Whether a screen took this frame's input.
    pub fn input_taken(&self) -> bool {
        self.screen_input
    }

    /// The window's touches of a frame no screen took input in, in order: the device type of the last (InputType and
    /// cursorMode stay what the menus last wrote).
    pub fn touched(&mut self, touches: &[Touch]) {
        for &t in touches {
            match t {
                Touch::Key(key) => self.interaction.played_key(key),
                Touch::MouseMove | Touch::MouseButton => self.interaction.played_mouse(),
            }
        }
    }
}

/// A mouse button's FKey name (0 left, 1 middle, 2 right, 3 and 4 the thumb buttons; 'None'
/// for another).
pub fn mouse_button_key(button: u8) -> &'static str {
    match button {
        0 => "LeftMouseButton",
        1 => "MiddleMouseButton",
        2 => "RightMouseButton",
        3 => "ThumbMouseButton",
        4 => "ThumbMouseButton2",
        _ => "None",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(p: &mut PadPoller, pad: MenuPad, first: bool, now: f64) -> Vec<PadEvent> {
        let mut v = Vec::new();
        p.poll(&[(0, 0, pad)], first, 1.0 / 60.0, now, &mut |e| v.push(e));
        v
    }

    fn presses(v: &[PadEvent]) -> Vec<&'static str> {
        v.iter()
            .filter_map(|e| match e {
                PadEvent::Press { key, .. } => Some(*key),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn a_button_held_at_the_first_frame_is_no_press() {
        let mut p = PadPoller::new();
        let a = MenuPad { buttons: 1, ..MenuPad::default() };
        assert!(presses(&run(&mut p, a, true, 0.0)).is_empty());
        assert!(presses(&run(&mut p, a, false, 0.1)).is_empty());
        let up = run(&mut p, MenuPad::default(), false, 0.2);
        assert!(up.contains(&PadEvent::Release { key: "Gamepad_FaceButton_Bottom", player: 0 }));
        assert_eq!(presses(&run(&mut p, a, false, 0.3)), ["Gamepad_FaceButton_Bottom"]);
    }

    #[test]
    fn a_stick_key_repeats_after_a_fifth_then_every_tenth() {
        let mut p = PadPoller::new();
        let right = MenuPad { axes: [0.5, 0.0, 0.0, 0.0], buttons: 0 };
        let mut at = Vec::new();
        for f in 0..64 {
            let t = f as f64 / 128.0;
            if !presses(&run(&mut p, right, false, t)).is_empty() {
                at.push(f);
            }
        }
        // (steps of 1 / 128 s, exact: 0.2 s is past at 26, then every 0.1 s: 39, 52)
        assert_eq!(at, [0, 26, 39, 52]);
    }

    #[test]
    fn axes_send_while_past_the_dead_zone_or_changed() {
        let mut p = PadPoller::new();
        let analog = |v: &[PadEvent]| v.iter().filter(|e| matches!(e, PadEvent::Analog { .. })).count();
        assert_eq!(analog(&run(&mut p, MenuPad::default(), false, 0.0)), 0);
        let small = MenuPad { axes: [0.1, 0.0, 0.0, 0.0], buttons: 0 };
        assert_eq!(analog(&run(&mut p, small, false, 0.1)), 1);
        assert_eq!(analog(&run(&mut p, small, false, 0.2)), 0);
        let big = MenuPad { axes: [0.0, 0.9, 0.0, 0.0], buttons: 0 };
        let v = run(&mut p, big, false, 0.3);
        // (lx back to 0 changed; ly sent with the engine's sign)
        assert!(v.contains(&PadEvent::Analog { key: "Gamepad_LeftY", value: -0.9, player: 0, frames: 1.0 }));
        assert_eq!(analog(&v), 2);
    }
}
