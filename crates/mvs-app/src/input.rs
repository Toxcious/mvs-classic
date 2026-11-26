//! Input: the keyboard's
//! and the mouse's events and the pads (XInput and the Sony pads over HID), each player's device read once a tick into the sim's canonical
//! InputState through the default bindings of InputDefaults.

pub mod keys;
// (the build's RawInput plugin, 140b14770, is linked but idle: no device configured)
#[cfg(windows)]
pub mod pad;
#[cfg(not(windows))]
#[path = "input/pad_sdl.rs"]
pub mod pad;
#[cfg(windows)]
pub mod sony;
pub mod stick;

use std::sync::Arc;

use mvs_sim::InputState;
use mvs_sim::input::{AttackBtn, Btn, Order, canon_input};

use mvs_ui::bindings::{Bindings, PAD_BUTTONS};
use mvs_ui::menu::input::{LEFT_STICK_DEAD_ZONE, RIGHT_STICK_DEAD_ZONE, STICK_MOUSE_GRACE, Touch};
use mvs_ui::menu::local_play_match::{InputSource, local_player_of};
use pad::{PadState, Pads};
use stick::{DirKey, MoveStick, massage_axis};

/// The actions a device reads (the build's action names of PFGPlayerController's
/// "{Action}-P<n>" events).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    Dir(DirKey),
    Attack,
    Special,
    Dodge,
    Jump,
    Item,
    /// Up / Down / Right / Left / Neutral Attack: InputDirection 0..4.
    Atk(u8),
    Spc(u8),
    NeutralEvade,
    Taunt(u8),
    /// The right stick's tilt in a direction (0 up, 1 down, 2 right, 3 left): HandleRightStickTilt.
    Rs(u8),
}

impl Action {
    /// The action of a build action name; None for an action the match does not read.
    pub fn of(name: &str) -> Option<Action> {
        Some(match name {
            "Left" => Action::Dir(DirKey::Left),
            "Right" => Action::Dir(DirKey::Right),
            "Up" => Action::Dir(DirKey::Up),
            "Down" => Action::Dir(DirKey::Down),
            "Attack" => Action::Attack,
            "Special" => Action::Special,
            "Evade" => Action::Dodge,
            "Jump" => Action::Jump,
            "Pickup Item" => Action::Item,
            "Up Attack" => Action::Atk(0),
            "Down Attack" => Action::Atk(1),
            "Right Attack" => Action::Atk(2),
            "Left Attack" => Action::Atk(3),
            "Neutral Attack" => Action::Atk(4),
            "Up Special" => Action::Spc(0),
            "Down Special" => Action::Spc(1),
            "Right Special" => Action::Spc(2),
            "Left Special" => Action::Spc(3),
            "Neutral Special" => Action::Spc(4),
            "Neutral Evade" => Action::NeutralEvade,
            "Taunt1" => Action::Taunt(0),
            "Taunt2" => Action::Taunt(1),
            "Taunt3" => Action::Taunt(2),
            "Taunt4" => Action::Taunt(3),
            _ => return None,
        })
    }
}

/// What a player's device reads.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DeviceKind {
    /// The keyboard and the mouse alone.
    Kbm,
    /// The build's device 0: the keyboard, the mouse and the first pad (the one human of every match but Local
    /// Play).
    KbmPad0,
    /// Controller id n's pad alone.
    Pad(usize),
}

/// The input preferences a device reads (Struct_Preferences_Input), its binding set's.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct InputPrefs {
    /// Get-Input-Horizontal / VerticalDeadzone (percent / 100).
    pub dead_zone: [f64; 2],
    /// Get-Input-RightStickStyle: 0 attacks, 1 specials.
    pub right_stick_style: u8,
    pub up_to_jump: bool,
    pub vibration: bool,
}

impl Default for InputPrefs {
    fn default() -> InputPrefs {
        InputPrefs { dead_zone: [0.3, 0.3], right_stick_style: 0, up_to_jump: false, vibration: true }
    }
}

impl InputPrefs {
    /// A local player's saved preferences.
    pub fn of(p: &mvs_settings::InputPlayerSettings) -> InputPrefs {
        InputPrefs {
            dead_zone: p.dead_zone(),
            right_stick_style: p.right_stick_style,
            up_to_jump: p.up_to_jump,
            vibration: p.vibration,
        }
    }
}

/// The right stick's dead zone (PFGPlayerController UpdateDeadzone: SetControllerDeadzone(Gamepad_RightX / RightY,
/// 0.1)), and HandleRightStickTilt's RightStickTiltDeadzone and RightStickReleaseDeadzone.
const RS_DEAD: f64 = 0.1;
const RS_TILT: f64 = 0.7;
const RS_RELEASE: f64 = 0.6;
/// Up To Jump's Delay(0.06) in reads: JumpPressed comes 3 reads after the press read.
const UP_JUMP_DELAY_READS: u8 = 3;

fn rs_axis(v: f64) -> f64 {
    if v.abs() <= RS_DEAD { 0.0 } else { v.signum() * (v.abs() - RS_DEAD) / (1.0 - RS_DEAD) }
}

/// Up To Jump: the Delays of the Up-Pn key and of Gamepad_LeftStick_Up, and the jump level
/// they hold.
// Deviation: the fighter makes JumpPressed / JumpReleased from the jump level's edges, so Up To Jump is ORed into
// that level.
#[derive(Clone, Copy, Debug, Default)]
struct UpToJump {
    key: u8,
    stick: u8,
    level: bool,
}

impl UpToJump {
    fn tick(&mut self) {
        for d in [&mut self.key, &mut self.stick] {
            if *d > 0 {
                *d -= 1;
                if *d == 0 {
                    self.level = true;
                }
            }
        }
    }
    fn press(&mut self, stick: bool, on: bool) {
        let d = if stick { &mut self.stick } else { &mut self.key };
        if on && *d == 0 {
            *d = UP_JUMP_DELAY_READS;
        }
    }
    fn release(&mut self, on: bool) {
        if on {
            self.level = false;
        }
    }
}

/// An ordered set of actions (kept in insertion order).
#[derive(Clone, Debug, Default, PartialEq)]
struct Actions(Vec<Action>);

impl Actions {
    fn has(&self, a: Action) -> bool {
        self.0.contains(&a)
    }
    fn add(&mut self, a: Action) {
        if !self.has(a) {
            self.0.push(a);
        }
    }
    fn delete(&mut self, a: Action) -> bool {
        let n = self.0.len();
        self.0.retain(|x| *x != a);
        self.0.len() != n
    }
    fn clear(&mut self) {
        self.0.clear();
    }
}

/// A player's input device: its held / tapped / untapped actions, its left stick and
/// direction keys, its right stick's tilt, isGamePadKey and LastInteractionType. The keyboard's and the mouse's
/// events reach it through `Devices`; its pad's state is set once a tick before `read`.
#[derive(Debug)]
pub struct InputDevice {
    pub kind: DeviceKind,
    /// The binding set (the human's local player index).
    pub set: usize,
    pub prefs: InputPrefs,
    bindings: Arc<Bindings>,
    held: Actions,
    /// Pressed since the last read, so sub-tick taps still register; released since the last read.
    tapped: Actions,
    untapped: Actions,
    pub stick: MoveStick,
    /// PFGPlayerController isGamePadKey (written by the Attack action's press and release and the Special
    /// action's release; read by the rumble's gates).
    pub is_game_pad_key: bool,
    /// UI_Manager's LastInteractionType (0 gamepad, 1 keyboard, 2 mouse).
    pub last_interaction: u8,
    /// The pad's state at this tick (`Devices::poll` sets it), None with no pad.
    pub pad: Option<PadState>,
    rs_tilted: bool,
    rsx: f64,
    rsy: f64,
    lsx: f64,
    lsy: f64,
    stick_up: bool,
    stick_up_prev: bool,
    up_jump: UpToJump,
    pad_down: u32,
    pad_buttons: Actions,
    pad_prev: Actions,
}

impl InputDevice {
    pub fn new(kind: DeviceKind, set: usize, bindings: Arc<Bindings>) -> InputDevice {
        InputDevice {
            kind,
            set,
            prefs: InputPrefs::default(),
            bindings,
            held: Actions::default(),
            tapped: Actions::default(),
            untapped: Actions::default(),
            stick: MoveStick::default(),
            is_game_pad_key: false,
            last_interaction: if kind == DeviceKind::Kbm { 1 } else { 0 },
            pad: None,
            rs_tilted: false,
            rsx: 0.0,
            rsy: 0.0,
            lsx: 0.0,
            lsy: 0.0,
            stick_up: false,
            stick_up_prev: false,
            up_jump: UpToJump::default(),
            pad_down: 0,
            pad_buttons: Actions::default(),
            pad_prev: Actions::default(),
        }
    }

    /// The device has the keyboard and the mouse.
    pub fn keyboard(&self) -> bool {
        matches!(self.kind, DeviceKind::Kbm | DeviceKind::KbmPad0)
    }

    /// The actions of a key of binding set `set`.
    fn actions(&self, key: &str, set: usize) -> Vec<Action> {
        self.bindings.actions_of(key, set).iter().filter_map(|a| Action::of(a)).collect()
    }

    /// A keyboard key's or mouse button's actions (set P0: sets 1..3 hold no keyboard key).
    pub fn key_actions(&self, key: &str) -> Vec<Action> {
        self.actions(key, 0)
    }

    pub fn key_down(&mut self, ks: &[Action], repeat: bool) {
        if !repeat && ks.contains(&Action::Attack) {
            self.is_game_pad_key = false;
        }
        for &k in ks {
            if !repeat {
                self.tapped.add(k);
            }
            self.held.add(k);
            // (a repeat of a key dropped at a blur or a match's start holds it again, as `held` does)
            if let Action::Dir(d) = k
                && (!repeat || !self.stick.held(d))
            {
                self.stick.dir_action(d, true);
            }
        }
    }

    pub fn key_up(&mut self, ks: &[Action]) {
        if ks.contains(&Action::Attack) || ks.contains(&Action::Special) {
            self.is_game_pad_key = false;
        }
        for &k in ks {
            self.held.delete(k);
            self.untapped.add(k);
            if let Action::Dir(d) = k {
                self.stick.dir_action(d, false);
            }
        }
    }

    pub fn mouse_down(&mut self, ks: &[Action]) {
        if ks.contains(&Action::Attack) {
            self.is_game_pad_key = false;
        }
        for &k in ks {
            self.tapped.add(k);
            self.held.add(k);
            if let Action::Dir(d) = k {
                self.stick.dir_action(d, true);
            }
        }
    }

    /// A mouse press taken back before any read saw it (a click the Lab Tools' panel took:
    /// the panel's capture listener stops it before the game's): its actions neither held nor tapped.
    pub fn take_back(&mut self, ks: &[Action]) {
        for &k in ks {
            self.tapped.delete(k);
            if self.held.delete(k)
                && let Action::Dir(d) = k
            {
                self.stick.dir_action(d, false);
            }
        }
    }

    pub fn mouse_up(&mut self, ks: &[Action]) {
        for &k in ks {
            if !self.held.delete(k) {
                continue;
            }
            if k == Action::Attack || k == Action::Special {
                self.is_game_pad_key = false;
            }
            self.untapped.add(k);
            if let Action::Dir(d) = k {
                self.stick.dir_action(d, false);
            }
        }
    }

    /// The window lost the keyboard, so no key up will come: every held key is let go.
    pub fn blur(&mut self) {
        self.held.clear();
        self.stick.clear();
        self.up_jump.key = 0;
        self.up_jump.level = false;
    }

    /// ResetInputs (PC @0-1796): the presses and releases queued since the last read dropped
    /// (a paused controller fires none of them); the held keys stay.
    pub fn drop_queued(&mut self) {
        self.tapped.clear();
        self.untapped.clear();
    }

    /// (the port's own; at an unpause and the Lab's pause and unpause) what was pressed on the
    /// menu is not a game press: the keys let go as at a blur, the queued presses dropped, and the pad's buttons held
    /// now taken as held at the last read, so a button pressed on the menu is no tap at the next read.
    pub fn resume(&mut self) {
        self.blur();
        self.drop_queued();
        self.pad_buttons.clear();
        self.pad_prev = self.pad_state();
    }

    /// At a match's start: a new controller has no key flagged and isGamePadKey false.
    pub fn reset(&mut self) {
        self.held.clear();
        self.stick.clear();
        self.is_game_pad_key = false;
        self.tapped.clear();
        self.untapped.clear();
        self.up_jump = UpToJump::default();
        self.stick_up = false;
        self.stick_up_prev = false;
    }

    /// The pad's actions of this read: its buttons through the bindings, the direction
    /// buttons as direction actions, the massaged left stick, Gamepad_LeftStick_Up, the right stick's tilt.
    fn pad_state(&mut self) -> Actions {
        let mut s = Actions::default();
        let Some(p) = self.pad else {
            self.pad_down = 0;
            self.lsx = 0.0;
            self.lsy = 0.0;
            self.stick_up = false;
            for d in DirKey::ALL {
                if self.pad_buttons.has(Action::Dir(d)) {
                    self.stick.dir_action(d, false);
                }
            }
            self.pad_buttons.clear();
            return s;
        };
        let mut buttons = Actions::default();
        let mut down = 0u32;
        for (i, key) in PAD_BUTTONS.iter().enumerate() {
            if p.buttons & (1 << i) == 0 {
                continue;
            }
            down |= 1 << i;
            for a in self.actions(key, self.set) {
                buttons.add(a);
            }
        }
        let (ax, ay) = (p.lx, p.ly);
        if ax.abs() > 0.5 || ay.abs() > 0.5 {
            down |= 1 << 30;
        }
        if down & !self.pad_down != 0 {
            self.last_interaction = 0;
        }
        self.pad_down = down;
        for d in DirKey::ALL {
            let on = buttons.has(Action::Dir(d));
            if on != self.pad_buttons.has(Action::Dir(d)) {
                self.stick.dir_action(d, on);
            }
        }
        s.0.clone_from(&buttons.0);
        self.pad_buttons = buttons;
        // MoveX = Gamepad_LeftX, MoveY = Gamepad_LeftY at scale 1 (DefaultInput.ini 280-283), up positive; each axis
        // its own dead zone.
        let [dx, dy] = self.prefs.dead_zone;
        self.lsx = massage_axis(ax, dx);
        self.lsy = massage_axis(ay, dy);
        // Gamepad_LeftStick_Up as a key: the pad reader's stick-as-button, before any dead zone.
        self.stick_up = p.stick_up;
        // NormalAttackX = Gamepad_RightX, NormalAttackY = Gamepad_RightY (up positive), each past its 0.1 dead zone.
        self.rsx = rs_axis(p.rx);
        self.rsy = rs_axis(p.ry);
        let rsy = self.rsy;
        let m = self.rsx.abs().max(rsy.abs());
        if !self.rs_tilted && m >= RS_TILT {
            self.rs_tilted = true;
            // DetermineRightStickDir: |X| > |Y| -> X < 0 ? left 3 : right 2, else Y < 0 ? down 1 : up 0.
            let dir = if self.rsx.abs() > rsy.abs() {
                if self.rsx < 0.0 { 3 } else { 2 }
            } else if rsy < 0.0 {
                1
            } else {
                0
            };
            s.add(Action::Rs(dir));
        } else if self.rs_tilted && m <= RS_RELEASE {
            self.rs_tilted = false;
        } else if self.rs_tilted {
            for &k in &self.pad_prev.0 {
                if matches!(k, Action::Rs(_)) {
                    s.add(k);
                }
            }
        }
        s
    }

    /// The device's InputState for a sim tick (InputDevice.read): its pad's actions, its keys' and buttons' taps
    /// since the last read, as `canonInput` gives it.
    pub fn read(&mut self) -> InputState {
        let pad = self.pad_state();
        for &k in &pad.0 {
            if !self.pad_prev.has(k) {
                self.tapped.add(k);
            }
        }
        for &k in &self.pad_prev.0 {
            if !pad.has(k) {
                self.untapped.add(k);
            }
        }
        // (isGamePadKey: the pad's Attack pressed or released, its Special released; the port's own pad binding of a
        // directional attack or special counts as its Attack or Special)
        let attack = |s: &Actions| s.0.iter().any(|a| matches!(a, Action::Attack | Action::Atk(_)));
        let special = |s: &Actions| s.0.iter().any(|a| matches!(a, Action::Special | Action::Spc(_)));
        if attack(&pad) != attack(&self.pad_prev) || (special(&self.pad_prev) && !special(&pad)) {
            self.is_game_pad_key = true;
        }
        self.stick.axis_events(self.lsx, self.lsy);
        let on = |k: Action| self.held.has(k) || self.tapped.has(k) || pad.has(k);
        // HandleRightStickTilt's button by Get-Input-RightStickStyle (0 NormalAttack, 1 SpecialAttack).
        let rs_button = if self.prefs.right_stick_style == 1 { AttackBtn::Special } else { AttackBtn::Attack };
        let mut force_dir = None;
        let mut force_press = None;
        let (mut atk_held, mut spc_held) = (false, false);
        // A directional action pressed this tick passes its direction; holding it holds the button.
        for (b, d, k) in (0..5u8)
            .map(|d| (AttackBtn::Attack, d, Action::Atk(d)))
            .chain((0..5u8).map(|d| (AttackBtn::Special, d, Action::Spc(d))))
        {
            if !on(k) {
                continue;
            }
            match b {
                AttackBtn::Attack => atk_held = true,
                AttackBtn::Special => spc_held = true,
            }
            if self.tapped.has(k) {
                force_dir = Some(d);
                force_press = Some(b);
            }
        }
        // Each attack action (and the right stick's release) fires its own Released on key up.
        let mut force_release = None;
        for &k in &self.untapped.0 {
            let b = match k {
                Action::Rs(_) => Some(rs_button),
                Action::Attack | Action::Atk(_) => Some(AttackBtn::Attack),
                Action::Special | Action::Spc(_) => Some(AttackBtn::Special),
                _ => None,
            };
            match b {
                Some(AttackBtn::Attack) => force_release = Some(AttackBtn::Attack),
                Some(AttackBtn::Special) => {
                    force_release.get_or_insert(AttackBtn::Special);
                }
                None => {}
            }
        }
        for &k in &pad.0 {
            let Action::Rs(d) = k else { continue };
            match rs_button {
                AttackBtn::Attack => atk_held = true,
                AttackBtn::Special => spc_held = true,
            }
            if self.tapped.has(k) {
                force_dir = Some(d);
                force_press = Some(rs_button);
            }
        }
        // The presses of this tick in key event order.
        let mut order = Order::default();
        for &k in &self.tapped.0 {
            let b = match k {
                Action::Jump => Some(Btn::Jump),
                Action::Item => Some(Btn::Item),
                Action::Dodge | Action::NeutralEvade => Some(Btn::Dodge),
                Action::Rs(_) => Some(match rs_button {
                    AttackBtn::Attack => Btn::Attack,
                    AttackBtn::Special => Btn::Special,
                }),
                Action::Attack | Action::Atk(_) => Some(Btn::Attack),
                Action::Special | Action::Spc(_) => Some(Btn::Special),
                _ => None,
            };
            if let Some(b) = b
                && !order.iter().any(|x| x == b)
                && (order.len as usize) < order.btns.len()
            {
                order.btns[order.len as usize] = Some(b);
                order.len += 1;
            }
        }
        // Up To Jump: the Delays ending this read, then this read's presses and releases.
        let utj = self.prefs.up_to_jump;
        self.up_jump.tick();
        if self.tapped.has(Action::Dir(DirKey::Up)) {
            self.up_jump.press(false, utj);
        }
        if self.untapped.has(Action::Dir(DirKey::Up)) {
            self.up_jump.release(utj);
        }
        if self.stick_up && !self.stick_up_prev {
            self.up_jump.press(true, utj);
        } else if !self.stick_up && self.stick_up_prev {
            self.up_jump.release(utj);
        }
        self.stick_up_prev = self.stick_up;
        if self.untapped.has(Action::Jump) {
            self.up_jump.level = false;
        }
        let taunt = (0..4u8).find(|&n| self.tapped.has(Action::Taunt(n)));
        let taunt_held = (0..4u8).any(|n| on(Action::Taunt(n)));
        let inp = InputState {
            x: mvs_sim::math::narrow(self.stick.x),
            y: mvs_sim::math::narrow(self.stick.y),
            attack: on(Action::Attack) || atk_held,
            special: on(Action::Special) || spc_held,
            force_dir,
            force_press,
            force_release,
            force: force_dir.is_some_and(|d| d != 4),
            rsx: Some(mvs_sim::math::narrow(self.rsx)),
            rsy: Some(mvs_sim::math::narrow(self.rsy)),
            jump: on(Action::Jump) || self.up_jump.level,
            dodge: on(Action::Dodge) || on(Action::NeutralEvade),
            neutral_evade: self.tapped.has(Action::NeutralEvade),
            taunt,
            taunt_held,
            item: on(Action::Item),
            order: Some(order),
            axis: self.stick.game_pad,
            ..InputState::default()
        };
        self.pad_prev = pad;
        self.tapped.clear();
        self.untapped.clear();
        canon_input(&inp)
    }
}

/// The match's humans' devices and the raw events the window hands them.
#[derive(Debug, Default)]
pub struct Devices {
    /// Per player slot: its device (None: no human there).
    slots: Vec<Option<InputDevice>>,
    pads: Pads,
    /// The keyboard's keys down (a repeat is a key already down).
    keys_down: Vec<&'static str>,
    /// The mouse pointer in client pixels of the window (WM_MOUSEMOVE); None while it is outside the window.
    pub pointer: Option<(f32, f32)>,
    /// The wheel's notches since the last `take_wheel` (up positive).
    pub wheel: f32,
    /// The characters typed since the last `take_chars` (WM_CHAR), for a text box.
    pub chars: Vec<char>,
    /// When a pad last had a stick past its dead zone.
    stick_at: Option<std::time::Instant>,
    /// What the player touched since the last `take_touches`, in order: the window's one record of the device last
    /// used, which the menus' pointer takes in a frame no screen took input in (app.rs).
    touches: Vec<Touch>,
    /// Each pad's buttons and left stick keys (Gamepad_LeftStick_Left, Right, Up, Down as bits 0..3) at the last
    /// poll: a touch is one going down.
    pads_down: [(u16, u8); pad::SLOTS],
    /// A scripted run's pads (`script_pads`): Some, the poll gives controller id 0 this state (None: no pad) and
    /// reads no hardware.
    scripted: Option<Option<PadState>>,
}

/// The left stick's directions as keys (past LEFT_STICK_DEAD_ZONE, the engine's key threshold): axis (0 X, 1 Y up
/// positive), sign, key.
const STICK_KEYS: [(usize, f64, &str); 4] = [
    (0, -1.0, "Gamepad_LeftStick_Left"),
    (0, 1.0, "Gamepad_LeftStick_Right"),
    (1, 1.0, "Gamepad_LeftStick_Up"),
    (1, -1.0, "Gamepad_LeftStick_Down"),
];

impl Devices {
    /// The devices of the match's humans: (player slot, device, binding set) each.
    pub fn new(bindings: Bindings, humans: &[(usize, DeviceKind, usize)]) -> Devices {
        let bindings = Arc::new(bindings);
        let mut d = Devices::default();
        for &(slot, kind, set) in humans {
            if d.slots.len() <= slot {
                d.slots.resize_with(slot + 1, || None);
            }
            d.slots[slot] = Some(InputDevice::new(kind, set, bindings.clone()));
        }
        d
    }

    /// The next match's humans seated (`new`'s set-up) on the pads already open: the slots made anew, the keys down
    /// forgotten.
    pub fn seat(&mut self, bindings: Bindings, humans: &[(usize, DeviceKind, usize)]) {
        let pads = std::mem::take(&mut self.pads);
        let (pointer, stick_at, touches, pads_down, scripted) =
            (self.pointer, self.stick_at, std::mem::take(&mut self.touches), self.pads_down, self.scripted);
        *self = Devices { pads, pointer, stick_at, touches, pads_down, scripted, ..Devices::new(bindings, humans) };
    }

    /// The bindings as they are now, on every seated device (a rebind made while the match runs). A held key may be
    /// another action's from here on: what the devices hold is let go, as at an unpause.
    pub fn rebind(&mut self, bindings: &Bindings) {
        let bindings = Arc::new(bindings.clone());
        for d in self.slots.iter_mut().flatten() {
            d.bindings = bindings.clone();
            d.resume();
        }
    }

    /// What the player touched since the last call, in order.
    pub fn take_touches(&mut self) -> Vec<Touch> {
        std::mem::take(&mut self.touches)
    }

    /// The mouse button `name`'s press taken back from the keyboard's device (`InputDevice::take_back`).
    pub fn take_back_mouse(&mut self, name: &str) {
        if let Some(d) = self.keyboard() {
            let ks = d.key_actions(name);
            d.take_back(&ks);
        }
    }

    /// Every device's `InputDevice::resume`.
    pub fn resume_all(&mut self) {
        for d in self.slots.iter_mut().flatten() {
            d.resume();
        }
    }

    /// Player `slot`'s device's `InputDevice::resume` (the primary device's).
    pub fn resume(&mut self, slot: usize) {
        if let Some(d) = self.device(slot) {
            d.resume();
        }
    }

    /// ResetInputs on every device: the queued presses and releases dropped.
    pub fn drop_queued(&mut self) {
        for d in self.slots.iter_mut().flatten() {
            d.drop_queued();
        }
    }

    /// Whether `s`'s input is menu player 0's: the one whose last device the menus keep.
    fn player_0s(&self, s: InputSource) -> bool {
        let first = self.pads.first().map_or(-1, |k| k as i64);
        local_player_of(s, first) == Some(0)
    }

    /// Each device's preferences, its binding set's (read at each read: a change made during a match
    /// holds from the next tick).
    pub fn read_prefs(&mut self, s: &mvs_settings::InputSettings) {
        for d in self.slots.iter_mut().flatten() {
            d.prefs = s.players.get(d.set).map_or_else(InputPrefs::default, InputPrefs::of);
        }
    }

    /// The wheel's notches since the last call.
    pub fn take_wheel(&mut self) -> f32 {
        std::mem::take(&mut self.wheel)
    }

    /// The characters typed since the last call.
    pub fn take_chars(&mut self) -> Vec<char> {
        std::mem::take(&mut self.chars)
    }

    /// The device of player slot `slot`, if one is bound.
    pub fn device(&mut self, slot: usize) -> Option<&mut InputDevice> {
        self.slots.get_mut(slot).and_then(Option::as_mut)
    }

    /// The device of player slot `slot`, if one is bound, to read.
    pub fn get(&self, slot: usize) -> Option<&InputDevice> {
        self.slots.get(slot).and_then(Option::as_ref)
    }

    /// The device whose input the HUD's labels of player index `player` show: the
    /// player's own; outside Local Play index 0 reads the local human's (`me`) as well, the literal an item's label
    /// passes (ItemActorParent @947).
    pub fn label_device(&self, player: usize, me: usize, local_play: bool) -> Option<&InputDevice> {
        self.get(player).or_else(|| if player == 0 && !local_play { self.get(me) } else { None })
    }

    /// The device with the keyboard and the mouse (the first that reads them).
    fn keyboard(&mut self) -> Option<&mut InputDevice> {
        self.slots.iter_mut().flatten().find(|d| d.keyboard())
    }

    /// A keyboard key went down or up (its FKey name). A down of a key already down is a repeat.
    pub fn key(&mut self, name: &'static str, down: bool) {
        let repeat = down && self.keys_down.contains(&name);
        if down {
            if !repeat {
                self.keys_down.push(name);
            }
        } else {
            self.keys_down.retain(|k| *k != name);
        }
        if down && !repeat && self.player_0s(InputSource::Kbm) {
            self.touches.push(Touch::Key(name));
        }
        let Some(d) = self.keyboard() else { return };
        if down && name != "Escape" && name != "Tab" {
            d.last_interaction = 1;
        }
        let ks = d.key_actions(name);
        if down { d.key_down(&ks, repeat) } else { d.key_up(&ks) }
    }

    /// A mouse button (its FKey name) went down or up over the window.
    pub fn mouse(&mut self, name: &'static str, down: bool) {
        if down && self.player_0s(InputSource::Kbm) {
            self.touches.push(Touch::MouseButton);
        }
        let Some(d) = self.keyboard() else { return };
        d.last_interaction = 2;
        let ks = d.key_actions(name);
        if down { d.mouse_down(&ks) } else { d.mouse_up(&ks) }
    }

    /// (the port's own, dev) A keyboard key (its FKey name) is held.
    pub fn key_held(&self, name: &str) -> bool {
        self.keys_down.contains(&name)
    }

    /// The mouse moved `dist` pixels. False: a pad's stick made it through a pointer
    /// layer of the OS or Steam, no mouse move for the window (it moves no pointer and
    /// reaches no screen). Else the mouse is the keyboard device's last interaction and the window's touch, unless the
    /// move is shorter than 2 pixels.
    pub fn mouse_moved(&mut self, dist: f64) -> bool {
        if self.stick_at.is_some_and(|t| t.elapsed().as_secs_f64() < STICK_MOUSE_GRACE) {
            return false;
        }
        if dist > 1.0 {
            if self.player_0s(InputSource::Kbm) {
                self.touches.push(Touch::MouseMove);
            }
            if let Some(d) = self.keyboard() {
                d.last_interaction = 2;
            }
        }
        true
    }

    /// The window lost the keyboard: every held key let go.
    pub fn blur(&mut self) {
        self.keys_down.clear();
        if let Some(d) = self.keyboard() {
            d.blur();
        }
    }

    /// The pads polled and each device's pad state of this tick set ('kbm+pad0' the first pad, a pad device its
    /// controller id's).
    pub fn poll(&mut self) {
        match self.scripted {
            Some(p) => {
                self.pads.state = [None; pad::SLOTS];
                self.pads.state[0] = p;
            }
            None => self.pads.poll(),
        }
        self.give_pads();
    }

    /// A scripted run's pads from now on: controller id 0 holds `pad` (None: no pad), the hardware is not read.
    pub fn script_pads(&mut self, pad: Option<PadState>) {
        self.scripted = Some(pad);
    }

    /// An input script's events of a frame, as the window's raw input (`platform::Window::pump`): the keys and
    /// buttons at once, the pad at the next poll.
    pub fn script_input(&mut self, input: impl IntoIterator<Item = crate::script::DeviceInput>) {
        use crate::script::DeviceInput;
        for i in input {
            match i {
                DeviceInput::Key(k, down) => self.key(k, down),
                DeviceInput::Mouse(k, down) => self.mouse(k, down),
                DeviceInput::Pad(p) => self.script_pads(Some(PadState::of_page(p.axes, p.buttons))),
            }
        }
    }

    /// The pads' states set from elsewhere than the readers (a replayed input tape), and each device's pad state set as
    /// `poll` does.
    pub fn set_pads(&mut self, state: [Option<PadState>; pad::SLOTS]) {
        self.pads.state = state;
        self.give_pads();
    }

    fn give_pads(&mut self) {
        let first = self.pads.first();
        for d in self.slots.iter_mut().flatten() {
            d.pad = match d.kind {
                DeviceKind::Kbm => None,
                DeviceKind::KbmPad0 => first.and_then(|k| self.pads.state[k]),
                DeviceKind::Pad(k) => self.pads.state.get(k).copied().flatten(),
            };
        }
        for k in 0..pad::SLOTS {
            let p = self.pads.state[k];
            self.pad_touches(k, p);
        }
    }

    /// Pad `k`'s poll as the window's record: a button or a left stick key going down is a touch; a stick past its
    /// dead zone makes the mouse moves of the next STICK_MOUSE_GRACE s the stick's. Only the pad of menu player 0
    /// counts (a pad no local player holds is not read; the cursor and the last device are player 0's).
    // Deviation: the usual stick-moved test checks the left stick at the menus' 0.25 and the right at Detect Scroll's
    // 0.15;
    // here each stick at the engine's own dead zone (7849 / 8689 of 32767, past which its analog event comes every
    // frame), so a right stick that rests a little off centre does not keep the mouse from the menus.
    fn pad_touches(&mut self, k: usize, p: Option<PadState>) {
        let Some(p) = p else {
            self.pads_down[k] = (0, 0);
            return;
        };
        let (was_buttons, was_stick) = self.pads_down[k];
        let mine = self.player_0s(InputSource::Pad(k));
        let mut stick = 0u8;
        for (bit, &(axis, sign, _)) in STICK_KEYS.iter().enumerate() {
            if (if axis == 1 { p.ly } else { p.lx }) * sign > LEFT_STICK_DEAD_ZONE {
                stick |= 1 << bit;
            }
        }
        self.pads_down[k] = (p.buttons, stick);
        if !mine {
            return;
        }
        for (i, key) in PAD_BUTTONS.iter().enumerate() {
            if p.buttons & !was_buttons & (1 << i) != 0 {
                self.touches.push(Touch::Key(key));
            }
        }
        for (bit, &(_, _, key)) in STICK_KEYS.iter().enumerate() {
            if stick & !was_stick & (1 << bit) != 0 {
                self.touches.push(Touch::Key(key));
            }
        }
        if stick != 0 || p.rx.abs() > RIGHT_STICK_DEAD_ZONE || p.ry.abs() > RIGHT_STICK_DEAD_ZONE {
            self.stick_at = Some(std::time::Instant::now());
        }
    }

    /// The controller id a player's device rumbles, if it has a pad now.
    pub fn pad_slot(&self, slot: usize) -> Option<usize> {
        let d = self.slots.get(slot)?.as_ref()?;
        match d.kind {
            DeviceKind::Kbm => None,
            DeviceKind::KbmPad0 => self.pads.first(),
            DeviceKind::Pad(k) => self.pads.state.get(k)?.map(|_| k),
        }
    }

    /// The pads' rumble sender.
    pub fn pads_mut(&mut self) -> &mut Pads {
        &mut self.pads
    }

    /// Every player slot with a device.
    pub fn slots(&self) -> impl Iterator<Item = usize> + '_ {
        self.slots.iter().enumerate().filter(|(_, d)| d.is_some()).map(|(i, _)| i)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mvs_data::InputDefaults;

    fn devices() -> Devices {
        let mut d = InputDefaults::default();
        let row: indexmap::IndexMap<String, Vec<String>> = [
            ("Attack", vec!["J", "Gamepad_FaceButton_Left", "LeftMouseButton"]),
            ("Jump", vec!["SpaceBar", "Gamepad_FaceButton_Bottom"]),
            ("Left", vec!["A", "Gamepad_DPad_Left"]),
            ("Up Attack", vec!["I"]),
        ]
        .into_iter()
        .map(|(a, k)| (a.to_owned(), k.into_iter().map(str::to_owned).collect()))
        .collect();
        d.rows.insert("Default-P0".into(), row);
        Devices::new(Bindings::new(&d), &[(0, DeviceKind::KbmPad0, 0)])
    }

    #[test]
    fn a_mouse_move_makes_the_mouse_the_last_interaction() {
        let mut d = devices();
        d.key("J", true);
        assert_eq!(d.get(0).unwrap().last_interaction, 1);
        d.mouse_moved(1.0);
        assert_eq!(d.get(0).unwrap().last_interaction, 1);
        d.mouse_moved(3.0);
        assert_eq!(d.get(0).unwrap().last_interaction, 2);
        // (a move while the pad's stick is out is the stick's)
        d.key("J", true);
        d.set_pads([Some(PadState { lx: 0.8, ..PadState::default() }), None, None, None]);
        d.mouse_moved(30.0);
        assert_eq!(d.get(0).unwrap().last_interaction, 1);
    }

    #[test]
    fn a_rebind_reaches_the_seated_device_and_lets_its_held_keys_go() {
        let mut d = devices();
        let held = |d: &Devices| d.get(0).unwrap().held.has(Action::Attack);
        d.key("J", true);
        assert!(held(&d));
        // Attack from J to K, as the Controls screen edits the bindings
        let mut b = (*d.get(0).unwrap().bindings).clone();
        b.remove_action_mapping("Attack-P0", "J");
        b.add_action_mapping("Attack-P0", "K");
        d.rebind(&b);
        assert!(!held(&d), "the old key's hold is let go");
        d.key("J", false);
        d.key("J", true);
        assert!(!held(&d), "the old key is Attack's no more");
        d.key("K", true);
        assert!(held(&d));
    }

    /// The window's record as the menus' pointer takes it (app.rs, a frame no screen took input in).
    fn record(d: &mut Devices, p: &mut mvs_ui::menu::input::Pointer) {
        p.touched(&d.take_touches());
    }

    fn pad(buttons: u16, lx: f64) -> [Option<PadState>; pad::SLOTS] {
        [Some(PadState { buttons, lx, ..PadState::default() }), None, None, None]
    }

    #[test]
    fn a_mouse_move_after_a_pad_makes_the_mouse_the_last_interaction() {
        let mut d = devices();
        let mut p = mvs_ui::menu::input::Pointer::default();
        d.set_pads(pad(1, 0.0));
        assert_eq!(d.take_touches(), [Touch::Key(PAD_BUTTONS[0])]);
        d.set_pads(pad(1, 0.0));
        assert!(d.take_touches().is_empty(), "a button held is no new touch");
        d.set_pads(pad(0, 0.0));
        d.set_pads(pad(1, 0.0));
        record(&mut d, &mut p);
        assert_eq!(p.interaction.last_type, 0);
        // no stick out: the move is the mouse's
        assert!(d.mouse_moved(30.0));
        record(&mut d, &mut p);
        assert_eq!(p.interaction.last_type, 2);
    }

    #[test]
    fn a_pad_stick_after_the_mouse_makes_the_pad_the_last_interaction() {
        let mut d = devices();
        let mut p = mvs_ui::menu::input::Pointer::default();
        assert!(d.mouse_moved(30.0));
        record(&mut d, &mut p);
        assert_eq!(p.interaction.last_type, 2);
        // a stick inside its dead zone is no touch
        d.set_pads(pad(0, 0.2));
        assert!(d.take_touches().is_empty());
        d.set_pads(pad(0, 0.8));
        assert_eq!(d.take_touches(), [Touch::Key("Gamepad_LeftStick_Right")]);
        d.set_pads(pad(0, 0.0));
        d.set_pads(pad(0, 0.8));
        record(&mut d, &mut p);
        assert_eq!(p.interaction.last_type, 0);
        // a mouse move inside the stick's grace is the stick's own (a pointer layer of the OS or Steam): dropped
        assert!(!d.mouse_moved(30.0));
        assert!(d.take_touches().is_empty());
    }

    #[test]
    fn a_key_after_a_pad_makes_the_keyboard_the_last_interaction() {
        let mut d = devices();
        let mut p = mvs_ui::menu::input::Pointer::default();
        d.set_pads(pad(1, 0.0));
        d.key("F1", true);
        let touches = d.take_touches();
        assert_eq!(touches, [Touch::Key(PAD_BUTTONS[0]), Touch::Key("F1")]);
        // (the menus' cursor mode as a mouse move in a menu left it)
        p.interaction = mvs_ui::menu::manager::Interaction { last_type: 2, input_type: 2, cursor_mode: true };
        p.touched(&touches);
        // play writes the device type alone: the Lab Tools' panel F1 opens still shows the cursor
        assert_eq!(
            p.interaction,
            mvs_ui::menu::manager::Interaction { last_type: 1, input_type: 2, cursor_mode: true }
        );
        // a held key's repeat is no touch
        d.key("F1", true);
        assert!(d.take_touches().is_empty());
    }

    #[test]
    fn a_pad_no_menu_player_holds_touches_nothing() {
        let mut d = devices();
        // a second pad, no menu player's ('kbm+pad0' holds the first): its presses and its stick's drift are not read
        d.set_pads([Some(PadState::default()), Some(PadState::default()), None, None]);
        d.set_pads([
            Some(PadState::default()),
            Some(PadState { buttons: 1, lx: 0.9, ..PadState::default() }),
            None,
            None,
        ]);
        assert!(d.take_touches().is_empty());
        assert!(d.mouse_moved(30.0), "the second pad's stick keeps no mouse move from the menus");
    }

    #[test]
    fn a_press_on_the_pause_menu_is_no_game_press_after_the_resume() {
        let mut d = devices();
        // a click on Resume (the left button is Attack's) and a key, while the match is paused (no read)
        d.mouse("LeftMouseButton", true);
        d.mouse("LeftMouseButton", false);
        d.key("J", true);
        d.key("J", false);
        d.resume_all();
        let i = d.device(0).unwrap().read();
        assert!(!i.attack, "no Attack on the first tick after the resume");
        // (without the resume the click is the game's)
        d.mouse("LeftMouseButton", true);
        d.mouse("LeftMouseButton", false);
        assert!(d.device(0).unwrap().read().attack);
    }

    #[test]
    fn a_click_the_lab_tools_took_is_no_game_press() {
        let mut d = devices();
        d.mouse("LeftMouseButton", true);
        d.take_back_mouse("LeftMouseButton");
        assert!(!d.device(0).unwrap().read().attack);
        d.mouse("LeftMouseButton", false);
        assert!(!d.device(0).unwrap().read().attack);
    }

    #[test]
    fn a_tap_inside_a_tick_registers_once() {
        let mut d = devices();
        d.key("J", true);
        d.key("J", false);
        let i = d.device(0).unwrap().read();
        assert!(i.attack);
        assert_eq!(i.force_release, Some(AttackBtn::Attack));
        assert_eq!(i.order.unwrap().iter().collect::<Vec<_>>(), [Btn::Attack]);
        let i = d.device(0).unwrap().read();
        assert!(!i.attack);
        assert_eq!(i.order.unwrap().len, 0);
    }

    #[test]
    fn a_directional_attack_forces_its_direction() {
        let mut d = devices();
        d.key("I", true);
        let i = d.device(0).unwrap().read();
        assert_eq!((i.force_dir, i.force_press, i.force), (Some(0), Some(AttackBtn::Attack), true));
        assert!(i.attack);
    }

    #[test]
    fn the_pad_moves_the_stick_and_presses() {
        let mut d = devices();
        d.key("A", true);
        let dev = d.device(0).unwrap();
        assert_eq!(dev.read().x, -1.0);
        dev.pad = Some(PadState { buttons: 1, lx: 1.0, ..PadState::default() });
        let i = dev.read();
        assert_eq!(i.x, 1.0);
        assert!(i.axis && i.jump);
        dev.pad = Some(PadState::default());
        let i = dev.read();
        // (pad mode: the centred stick writes 0 over the held key)
        assert_eq!(i.x, 0.0);
        assert!(!i.jump);
    }

    #[test]
    fn a_pad_button_bound_to_a_directional_special_forces_it() {
        let mut d = devices();
        let mut b = (*d.get(0).unwrap().bindings).clone();
        b.add_action_mapping("Up Special-P0", "Gamepad_RightShoulder");
        d.rebind(&b);
        let dev = d.device(0).unwrap();
        let rb = 1 << PAD_BUTTONS.iter().position(|k| *k == "Gamepad_RightShoulder").unwrap();
        dev.pad = Some(PadState { buttons: rb, ..PadState::default() });
        let i = dev.read();
        assert_eq!((i.force_dir, i.force_press, i.force), (Some(0), Some(AttackBtn::Special), true));
        assert!(i.special);
        dev.pad = Some(PadState::default());
        let i = dev.read();
        assert_eq!(i.force_release, Some(AttackBtn::Special));
        assert!(dev.is_game_pad_key, "the release of a pad special counts for the rumble");
    }
}
