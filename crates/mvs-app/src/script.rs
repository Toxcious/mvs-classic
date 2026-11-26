//! The input scripts (`--input-script <jsonl>`): a frame-numbered list of mouse, key and pad events fed to the window
//! as the player's, for the menu tapes and pictures. The oracle plays the same file in its menu runner
//! on its stepped clock, so a script is one list of events at 60 Hz in a 1920x1080 window.
//!
//! The file is JSON lines. An optional line `{"start": "title" | "menu" | "launcher"}` names where the run begins: the
//! title screen, the main menu once it has settled (360 frames after it opens, not on the script's clock), or the
//! launcher's home (its own 1200x720 window); the default is "menu". Then one event per line, `f` the frame it is fed
//! before (frames count from 0, the first the script's):
//!
//! ```text
//! {"f":0,"ev":"move","x":960,"y":540}            the mouse to x, y (device pixels)
//! {"f":3,"ev":"down","button":0}                 a mouse button down (0 left, 1 middle, 2 right)
//! {"f":5,"ev":"up","button":0}
//! {"f":9,"ev":"wheel","dy":-100}                 a wheel event's deltaY (one notch is 100, up is negative)
//! {"f":12,"ev":"key","code":"Enter","down":true} a key by its KeyboardEvent.code; down false: up
//! {"f":20,"ev":"pad","axes":[0,0,0,0],"buttons":[0]}  pad 0's whole state from here on: the four axes (left X, Y,
//!                                                right X, Y), the standard mapping's buttons held (by index)
//! {"f":40,"ev":"gate","for":"lab"}          wait for the Lab's match (see below)
//! {"f":600,"ev":"end"}                           the run's last frame (else the last event's)
//! {"f":30,"ev":"do","what":"showModal","args":[..]}  a named action of the oracle's runner (see below)
//! {"f":50,"ev":"text","text":"gg"}               typed characters (the port's own: no oracle script has it)
//! ```
//!
//! The port's own `do` lines for the two-exe online runs (no oracle script has them), run by the menus: `click`
//! (args [name, n]) presses the n-th shown widget whose path ends in that name (a `#k` suffix included) once it is
//! shown, its move, press and release one a frame; `widgets` (args [text]) logs every shown widget whose path holds
//! the text, with its centre and a TextBlock's text; `joinCode` (args [code]) is the join box's GO with that code;
//! `lobby` logs the custom game's lobby as its Start button sees it (leader, teams, ready list, blockers). Online, each
//! exe's `--data` needs a player name (settings.json profile.name: without one the session sends no hello and the host
//! core takes no lobby request), and the lobby opens as Versus, which starts two humans as a 1v1. A dev config's
//! run logs the net service's own lines (`mvs: net:`). A scripted run with
//! `--online-config` runs in real time, at most 60 frames a second, on the wall clock (app.rs).
//!
//! In a match the frames' keys and typed characters go to the match chat's box (`ScriptRun::match_input`, run.rs),
//! and its keys, mouse buttons and pad to the match's devices (`ScriptRun::device_input`) as the window's raw input
//! goes there: a scripted match moves, attacks and throws. A scripted run's pads are the script's alone (the pad
//! line's state is controller id 0; `Devices::script_pads`): a real pad drives nothing.
//!
//! A `do` line is one of the oracle runner's actions, kept by name in the oracle's resolved script: the menus run
//! those they have (menus.rs `run_do`) and log the rest. `win` is a gate of its own (the oracle waits on the match
//! and its podium): the match is won for team 0 120 ticks after its first frame and the script goes on once it has
//! ended (`Gate::Win`, run.rs), the frames between not on the tape. A line `{"query": "?test&stay&queue=fake&.."}` is the
//! oracle's page query: the exe reads the fake queue's options from it (`Script::query`).
//!
//! A gate waits on what loads in real time (a level, a match): from its frame the script's clock stops and the
//! program runs its own frames until the thing is up (`lab`: the Lab's match is loaded and in play; `menu`: the main
//! menu is up again after a level or a match), then `GATE_SETTLE` more, and the script goes on with the same frame
//! counter: the frames of the wait are not on the tape. Both sides stop at the same frame, so the tapes line up.
//! A gate still waiting after `GATE_TIMEOUT` program frames ends the run with an error naming it.
//!
//! A line `{"shots": [f, ..]}` lists the frames a picture is taken of (and `--shot-frames`'s). The frames count on
//! through the screens the script does not drive (the loading screen, a match, the results: `ScriptRun::passive`),
//! whose pictures are taken and whose `end` ends the run as the menus' do; a match under a script steps one tick a
//! frame.
//!
//! The events of a frame are fed in file order before that frame's tick. The tape records every frame from 0 to the
//! last, after its paint (`--menu-tape <out>`, the oracle's menu tape format, `mvs_ui::menu::tape`); `--shot-frames
//! f,..` writes a PNG of those frames to `--out`.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use mvs_ui::menu::MenuInput;
use mvs_ui::menu::input::MenuPad;
use mvs_ui::menu::tape::{MenuFrame, MenuHeader, MenuTape};
use serde::Deserialize;

use crate::{Error, Result};

/// Where a run begins.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum Start {
    /// The title screen.
    Title,
    /// The launcher's home (a 1200x720 window; the oracle's launcher is Electron: no tape).
    Launcher,
    /// The main menu, settled.
    #[default]
    Menu,
}

impl Start {
    pub fn name(self) -> &'static str {
        match self {
            Start::Title => "title",
            Start::Launcher => "launcher",
            Start::Menu => "menu",
        }
    }
}

/// Frames the main menu is left to settle before a `menu` start's frame 0.
pub const SETTLE_FRAMES: u32 = 360;

/// Frames run after a gate's condition holds, before the script goes on (animations, fades and the loading screen's
/// timers are over by then, whatever frame of the wait it came true at).
pub const GATE_SETTLE: u32 = 300;

/// Program frames a gate may wait (a level's or a match's load included) before the run ends with an error.
pub const GATE_TIMEOUT: u32 = 3600;

/// What a gate waits for.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Gate {
    /// The Lab's match is loaded and in play.
    Lab,
    /// The main menu is up again.
    Menu,
    /// A `do win` line: the match is won for team 0 and its end is over (released by the match's end, run.rs).
    Win,
}

impl Gate {
    pub fn name(self) -> &'static str {
        match self {
            Gate::Lab => "lab",
            Gate::Menu => "menu",
            Gate::Win => "win",
        }
    }
}

/// A pad's state (the standard mapping): the axes (left X, left Y, right X, right Y) and the buttons held, one bit
/// per index (`PAD_BUTTONS`).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct PadState {
    pub axes: [f64; 4],
    pub buttons: u32,
}

/// The FKey names of the standard mapping's buttons, by index.
pub const PAD_BUTTONS: [&str; 16] = [
    "Gamepad_FaceButton_Bottom",
    "Gamepad_FaceButton_Right",
    "Gamepad_FaceButton_Left",
    "Gamepad_FaceButton_Top",
    "Gamepad_LeftShoulder",
    "Gamepad_RightShoulder",
    "Gamepad_LeftTrigger",
    "Gamepad_RightTrigger",
    "Gamepad_Special_Left",
    "Gamepad_Special_Right",
    "Gamepad_LeftThumbstick",
    "Gamepad_RightThumbstick",
    "Gamepad_DPad_Up",
    "Gamepad_DPad_Down",
    "Gamepad_DPad_Left",
    "Gamepad_DPad_Right",
];

/// One event of a script.
#[derive(Clone, Debug, PartialEq)]
pub enum Event {
    Move {
        x: f64,
        y: f64,
    },
    Button {
        button: u8,
        down: bool,
    },
    Wheel {
        dy: f64,
    },
    /// Typed characters (the port's own).
    Text(String),
    /// A key by its FKey name.
    Key {
        key: &'static str,
        down: bool,
    },
    Pad(PadState),
    /// Wait for what loads in real time (`Gate`).
    Gate(Gate),
    /// A named action of the oracle's runner (`what`, its `args`, null when none).
    Do {
        what: String,
        args: serde_json::Value,
    },
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Line {
    f: Option<u32>,
    ev: Option<String>,
    start: Option<String>,
    shots: Option<Vec<u32>>,
    #[serde(rename = "for")]
    wait: Option<String>,
    x: Option<f64>,
    y: Option<f64>,
    button: Option<u8>,
    dy: Option<f64>,
    code: Option<String>,
    down: Option<bool>,
    axes: Option<[f64; 4]>,
    buttons: Option<Vec<u32>>,
    query: Option<String>,
    what: Option<String>,
    args: Option<serde_json::Value>,
    text: Option<String>,
}

const NAMED: &[(&str, &str)] = &[
    ("Space", "SpaceBar"),
    ("Enter", "Enter"),
    ("NumpadEnter", "Enter"),
    ("Tab", "Tab"),
    ("Escape", "Escape"),
    ("Backquote", "Tilde"),
    ("ArrowUp", "Up"),
    ("ArrowDown", "Down"),
    ("ArrowLeft", "Left"),
    ("ArrowRight", "Right"),
    ("Home", "Home"),
    ("End", "End"),
    ("PageUp", "PageUp"),
    ("PageDown", "PageDown"),
    ("Backspace", "BackSpace"),
    ("Delete", "Delete"),
    ("ShiftLeft", "LeftShift"),
    ("ShiftRight", "RightShift"),
    ("ControlLeft", "LeftControl"),
    ("ControlRight", "RightControl"),
    ("AltLeft", "LeftAlt"),
    ("AltRight", "RightAlt"),
    ("CapsLock", "CapsLock"),
    ("Insert", "Insert"),
    ("Comma", "Comma"),
    ("Period", "Period"),
    ("Slash", "Slash"),
    ("Semicolon", "Semicolon"),
    ("Quote", "Apostrophe"),
    ("BracketLeft", "LeftBracket"),
    ("BracketRight", "RightBracket"),
    ("Backslash", "Backslash"),
    ("Minus", "Hyphen"),
    ("Equal", "Equals"),
    ("NumpadMultiply", "Multiply"),
    ("NumpadAdd", "Add"),
    ("NumpadSubtract", "Subtract"),
    ("NumpadDecimal", "Decimal"),
    ("NumpadDivide", "Divide"),
];
const LETTERS: [&str; 26] = [
    "A", "B", "C", "D", "E", "F", "G", "H", "I", "J", "K", "L", "M", "N", "O", "P", "Q", "R", "S", "T", "U", "V", "W",
    "X", "Y", "Z",
];
const DIGITS: [&str; 10] = ["Zero", "One", "Two", "Three", "Four", "Five", "Six", "Seven", "Eight", "Nine"];
const NUMPAD: [&str; 10] = [
    "NumPadZero",
    "NumPadOne",
    "NumPadTwo",
    "NumPadThree",
    "NumPadFour",
    "NumPadFive",
    "NumPadSix",
    "NumPadSeven",
    "NumPadEight",
    "NumPadNine",
];
const FUNCTION: [&str; 12] = ["F1", "F2", "F3", "F4", "F5", "F6", "F7", "F8", "F9", "F10", "F11", "F12"];

/// The FKey name of a KeyboardEvent.code.
pub fn key_name(code: &str) -> Option<&'static str> {
    if let Some((_, k)) = NAMED.iter().find(|(c, _)| *c == code) {
        return Some(k);
    }
    let digit = |s: &str| s.parse::<usize>().ok().filter(|d| *d < 10);
    if let Some(l) = code.strip_prefix("Key") {
        return LETTERS.iter().find(|x| **x == l).copied();
    }
    if let Some(d) = code.strip_prefix("Digit").and_then(digit) {
        return Some(DIGITS[d]);
    }
    if let Some(d) = code.strip_prefix("Numpad").and_then(digit) {
        return Some(NUMPAD[d]);
    }
    code.strip_prefix('F')
        .and_then(|n| n.parse::<usize>().ok())
        .filter(|n| (1..=12).contains(n))
        .map(|n| FUNCTION[n - 1])
}

/// A script: where it starts, its events by frame, its length in frames.
#[derive(Clone, Debug, Default)]
pub struct Script {
    pub name: String,
    pub start: Start,
    /// (frame, event), frames rising, file order within a frame.
    pub events: Vec<(u32, Event)>,
    /// The frames the script asks a picture of (`{"shots": [..]}` lines), rising.
    pub shots: Vec<u32>,
    /// The run's frames: the last frame + 1.
    pub frames: u32,
    /// The oracle's page query ('' none: the default `?test&stay`).
    pub query: String,
}

fn bad(what: impl Into<String>, why: impl ToString) -> Error {
    Error::Match(format!("input script {}: {}", what.into(), why.to_string()))
}

impl Script {
    pub fn read(path: &Path) -> Result<Script> {
        let text = std::fs::read_to_string(path).map_err(|e| bad(path.display().to_string(), e))?;
        let name = path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
        Script::parse(&name, &text)
    }

    pub fn parse(name: &str, text: &str) -> Result<Script> {
        let mut s = Script { name: name.to_string(), ..Script::default() };
        let mut end = None;
        let mut last = 0;
        for (i, l) in text.lines().enumerate().filter(|(_, l)| !l.trim().is_empty()) {
            let at = format!("{name} line {}", i + 1);
            let line: Line = serde_json::from_str(l).map_err(|e| bad(&at, e))?;
            if let Some(start) = &line.start {
                s.start = match start.as_str() {
                    "title" => Start::Title,
                    "launcher" => Start::Launcher,
                    "menu" => Start::Menu,
                    o => return Err(bad(&at, format!("start {o}: title, menu or launcher"))),
                };
                continue;
            }
            if let Some(q) = line.query {
                s.query = q;
                continue;
            }
            if let Some(list) = line.shots {
                s.shots.extend(list);
                continue;
            }
            let f = line.f.ok_or_else(|| bad(&at, "no f"))?;
            if f < last {
                return Err(bad(&at, format!("frame {f} after {last}: frames rise")));
            }
            last = f;
            let ev = line.ev.as_deref().ok_or_else(|| bad(&at, "no ev"))?;
            let need = |v: Option<f64>, what: &str| v.ok_or_else(|| bad(&at, format!("{ev}: no {what}")));
            let event = match ev {
                "move" => Event::Move { x: need(line.x, "x")?, y: need(line.y, "y")? },
                "down" | "up" => Event::Button { button: line.button.unwrap_or(0), down: ev == "down" },
                "wheel" => Event::Wheel { dy: need(line.dy, "dy")? },
                "text" => Event::Text(line.text.clone().ok_or_else(|| bad(&at, "text: no text"))?),
                "key" => {
                    let code = line.code.as_deref().ok_or_else(|| bad(&at, "key: no code"))?;
                    let key = key_name(code).ok_or_else(|| bad(&at, format!("key: no FKey for {code}")))?;
                    Event::Key { key, down: line.down.unwrap_or(true) }
                }
                "pad" => {
                    let mut buttons = 0;
                    for b in line.buttons.unwrap_or_default() {
                        if b >= 16 {
                            return Err(bad(&at, format!("pad: button {b} past 15")));
                        }
                        buttons |= 1 << b;
                    }
                    Event::Pad(PadState { axes: line.axes.unwrap_or_default(), buttons })
                }
                "gate" => Event::Gate(match line.wait.as_deref() {
                    Some("lab") => Gate::Lab,
                    Some("menu") => Gate::Menu,
                    o => return Err(bad(&at, format!("gate: for {o:?}: lab or menu"))),
                }),
                "do" => Event::Do {
                    what: line.what.ok_or_else(|| bad(&at, "do: no what"))?,
                    args: line.args.unwrap_or(serde_json::Value::Null),
                },
                "end" => {
                    end = Some(f);
                    continue;
                }
                o => return Err(bad(&at, format!("ev {o}: move, down, up, wheel, key, pad, gate, do or end"))),
            };
            s.events.push((f, event));
        }
        s.shots.sort_unstable();
        s.shots.dedup();
        s.frames = end.unwrap_or(last).max(s.shots.last().copied().unwrap_or(0)) + 1;
        Ok(s)
    }
}

/// A script's event for the match's devices, as the window hands its raw input to them (`platform::Window::pump`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum DeviceInput {
    /// A key by its FKey name, down or up.
    Key(&'static str, bool),
    /// A mouse button by its FKey name, down or up.
    Mouse(&'static str, bool),
    /// Pad 0's whole state from here on.
    Pad(PadState),
}

impl DeviceInput {
    /// The frame's events the devices take: keys, mouse buttons and the pad.
    fn of(fed: &[Fed]) -> impl Iterator<Item = DeviceInput> + '_ {
        fed.iter().filter_map(|f| match *f {
            Fed::Input(MenuInput::Key { key, down, .. }) => Some(DeviceInput::Key(key, down)),
            Fed::Input(MenuInput::MouseButton { button, down }) => {
                let name = mvs_ui::menu::input::mouse_button_key(button);
                (name != "None").then_some(DeviceInput::Mouse(name, down))
            }
            Fed::Pad(p) => Some(DeviceInput::Pad(p)),
            Fed::Input(_) => None,
        })
    }
}

/// What a frame's events turned into, for the menus' input.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Fed {
    Input(MenuInput),
    /// Pad 0's whole state (the pad poller reads it as a browser's gamepad API gives it).
    Pad(PadState),
}

/// A script being played: the next frame's events, the keys held (a key down again is a repeat).
#[derive(Debug)]
pub struct Player {
    script: Script,
    next: usize,
    frame: u32,
    held: BTreeSet<&'static str>,
    waiting: Option<Gate>,
    /// Program frames spent at the gate.
    waited: u32,
    /// The `do` actions fed and not taken yet (`take_dos`).
    dos: Vec<(String, serde_json::Value)>,
}

impl Player {
    pub fn new(script: Script) -> Player {
        Player { script, next: 0, frame: 0, held: BTreeSet::new(), waiting: None, waited: 0, dos: Vec::new() }
    }

    pub fn script(&self) -> &Script {
        &self.script
    }

    /// The frame the next `feed` is for.
    pub fn frame(&self) -> u32 {
        self.frame
    }

    pub fn done(&self) -> bool {
        self.frame >= self.script.frames
    }

    /// The gate the script is stopped at: the program runs its frames (no `feed`, no tape line) until it holds,
    /// then `GATE_SETTLE` more, then `release`.
    pub fn waiting(&self) -> Option<Gate> {
        self.waiting
    }

    pub fn release(&mut self) {
        self.waiting = None;
        self.waited = 0;
    }

    /// One program frame passed (call every frame, whatever screen runs): an error once a gate has waited
    /// `GATE_TIMEOUT` frames, so a run whose gate never holds or is never released ends instead of hanging.
    pub fn frame_passed(&mut self) -> std::result::Result<(), String> {
        let Some(g) = self.waiting else { return Ok(()) };
        self.waited += 1;
        if self.waited < GATE_TIMEOUT {
            return Ok(());
        }
        Err(format!(
            "{}: the {} gate at frame {} did not release in {GATE_TIMEOUT} frames",
            self.script.name,
            g.name(),
            self.frame
        ))
    }

    /// The `do` actions fed since the last call, in order.
    pub fn take_dos(&mut self) -> Vec<(String, serde_json::Value)> {
        std::mem::take(&mut self.dos)
    }

    /// The events of the frame, in file order; the frame counter goes on.
    pub fn feed(&mut self) -> Vec<Fed> {
        let mut out = Vec::new();
        if self.waiting.is_some() {
            return out;
        }
        while let Some((f, e)) = self.script.events.get(self.next).cloned() {
            if f > self.frame {
                break;
            }
            self.next += 1;
            if let Event::Do { what, args } = e {
                if what == "win" {
                    self.waiting = Some(Gate::Win);
                    return out;
                }
                self.dos.push((what, args));
                continue;
            }
            if let Event::Gate(g) = e {
                // (the frame goes on after the gate: the rest of its events are fed by the next call, the frame
                // counter not moved)
                self.waiting = Some(g);
                return out;
            }
            if let Event::Text(t) = e {
                out.extend(t.chars().map(|c| Fed::Input(MenuInput::Char(c))));
                continue;
            }
            out.push(match e {
                Event::Move { x, y } => Fed::Input(MenuInput::MouseMove { x, y }),
                Event::Button { button, down } => Fed::Input(MenuInput::MouseButton { button, down }),
                // (a wheel's notches are -deltaY / 100)
                Event::Wheel { dy } => Fed::Input(MenuInput::Wheel { notches: -dy / 100.0 }),
                Event::Key { key, down } => {
                    let repeat = down && !self.held.insert(key);
                    if !down {
                        self.held.remove(key);
                    }
                    Fed::Input(MenuInput::Key { key, down, repeat, player: 0 })
                }
                Event::Pad(p) => Fed::Pad(p),
                Event::Gate(_) | Event::Do { .. } | Event::Text(_) => {
                    unreachable!("a gate, a do and a text are taken above")
                }
            });
        }
        self.frame += 1;
        out
    }
}

/// The flags of a scripted run: `--input-script`, `--menu-tape`, `--shot-frames`.
#[derive(Clone, Debug, Default)]
pub struct ScriptOptions {
    pub input_script: Option<PathBuf>,
    pub menu_tape: Option<PathBuf>,
    pub shot_frames: Vec<u32>,
    /// `--live-loads`: the loads go on as a player's, the frames running on while they load (a run that times the
    /// frames or pictures the loading screen; its tape is not the oracle's frame for frame).
    pub live_loads: bool,
}

impl ScriptOptions {
    /// Takes `arg` and its value from `it` when it is one of the script flags. Ok(false): not one of them.
    pub fn flag(&mut self, arg: &str, it: &mut impl Iterator<Item = String>) -> std::result::Result<bool, String> {
        match arg {
            "--input-script" => self.input_script = Some(PathBuf::from(it.next().ok_or("--input-script: no value")?)),
            "--menu-tape" => self.menu_tape = Some(PathBuf::from(it.next().ok_or("--menu-tape: no value")?)),
            "--live-loads" => self.live_loads = true,
            "--shot-frames" => {
                for f in it.next().ok_or("--shot-frames: no value")?.split(',') {
                    self.shot_frames.push(f.parse().map_err(|_| format!("--shot-frames: {f} is not a frame"))?);
                }
            }
            _ => return Ok(false),
        }
        Ok(true)
    }

    /// The arguments are consistent: a tape or shots need a script.
    pub fn check(&self) -> std::result::Result<(), String> {
        if self.input_script.is_none() && (self.menu_tape.is_some() || !self.shot_frames.is_empty() || self.live_loads)
        {
            return Err("--menu-tape / --shot-frames / --live-loads: no --input-script".to_owned());
        }
        Ok(())
    }

    pub fn active(&self) -> bool {
        self.input_script.is_some()
    }
}

/// A scripted run: the script playing and the tape it writes.
#[derive(Debug)]
pub struct ScriptRun {
    pub player: Player,
    tape: Option<(PathBuf, MenuTape)>,
    shots: BTreeSet<u32>,
    /// Frames the Lab's match has been in play while the script waits at a `lab` gate.
    lab_up: u32,
    /// The keys and typed characters of a match's frames, for its chat box (taken by run.rs).
    pub match_input: Vec<MenuInput>,
    /// The keys, mouse buttons and pad of a match's frames, for the match's devices (taken by run.rs before its
    /// ticks: the Lab's frame's before the frame's tick, another match's at the next frame's).
    pub device_input: Vec<DeviceInput>,
    /// The `do click` lines (a widget's name, its n-th) waiting for their widget, the oldest first (menus.rs).
    pub clicks: Vec<(String, Option<u64>)>,
    /// The rest of a click's input, one a frame.
    pub pressing: Vec<MenuInput>,
    /// The frame `drive` fed whose tape line is not in yet (`passive` puts it in after the frame's paint).
    driven: Option<u32>,
    /// `--live-loads` (`ScriptOptions`).
    pub live_loads: bool,
    /// The Lab's match takes the script's events (`drive`): a released `lab` gate's frame is fed by the next
    /// frame's `drive`, before that frame's tick.
    driving: bool,
    /// Pad 0's state as the last `pad` line set it (the oracle runner's gamepad stub): the menus poll it
    /// each frame from that line on.
    pub pad: Option<MenuPad>,
}

impl ScriptRun {
    pub fn new(o: &ScriptOptions, size: [u32; 2]) -> Result<Option<ScriptRun>> {
        let Some(path) = &o.input_script else { return Ok(None) };
        let script = Script::read(path)?;
        let script_shots = script.shots.clone();
        let tape = o.menu_tape.as_ref().map(|p| {
            let header = MenuHeader {
                tape: "menu".into(),
                version: 1,
                script: script.name.clone(),
                start: script.start.name().into(),
                size,
            };
            (p.clone(), MenuTape { header, frames: Vec::new() })
        });
        Ok(Some(ScriptRun {
            player: Player::new(script),
            tape,
            shots: o.shot_frames.iter().chain(&script_shots).copied().collect(),
            lab_up: 0,
            match_input: Vec::new(),
            device_input: Vec::new(),
            clicks: Vec::new(),
            pressing: Vec::new(),
            driven: None,
            driving: false,
            pad: None,
            live_loads: o.live_loads,
        }))
    }

    /// A drawn frame of a screen the script does not drive (the loading screen, a match, the results), before its
    /// present: the script's clock goes on as the oracle runner's does through a match, the frame's events go to no
    /// menu,
    /// and its tape line is the oracle's menu tape record with no view. A `lab` gate is released `GATE_SETTLE` frames
    /// after
    /// `in_play` first holds. The frame, when a picture of it is due.
    pub fn passive(&mut self, in_play: bool, hub: Option<&crate::online::hub::OnlineHub>) -> Option<u32> {
        if let Some(n) = self.driven.take() {
            return self.painted(Self::line(n)).then_some(n);
        }
        if self.player.waiting() == Some(Gate::Lab) && in_play {
            self.lab_up += 1;
            if self.lab_up > GATE_SETTLE {
                self.lab_up = 0;
                self.player.release();
                if self.driving {
                    return None;
                }
            }
        }
        if self.player.waiting().is_some() || self.done() {
            return None;
        }
        let n = self.player.frame();
        let fed = self.player.feed();
        if in_play {
            self.device_input.extend(DeviceInput::of(&fed));
            self.match_input.extend(fed.into_iter().filter_map(|f| match f {
                Fed::Input(i @ (MenuInput::Key { .. } | MenuInput::Char(_))) => Some(i),
                _ => None,
            }));
        }
        // (a screen no script drives runs no action but the state reads: logged, as the oracle runner's `do` lines
        // are)
        for (what, args) in self.player.take_dos() {
            if what == "queueState" {
                crate::menus::queue_state(hub, n);
            } else {
                eprintln!("mvs: script: frame {n}: do {what} {args}: not run on this screen");
            }
        }
        // (a gate's frame is the script's once it is released)
        if self.player.waiting().is_some() {
            return None;
        }
        self.painted(Self::line(n)).then_some(n)
    }

    /// The tape line of a frame with no view.
    fn line(n: u32) -> MenuFrame {
        MenuFrame { frame: n, now: f64::from(n) / 60.0, mouse: None, fader: 0.0, roots: Vec::new() }
    }

    /// A frame `drive` fed that no `passive` followed (its match ended before the paint): its line in, no picture.
    fn flush(&mut self) {
        if let Some(n) = self.driven.take() {
            self.painted(Self::line(n));
        }
    }

    /// The frame's events for the Lab's match (call once per frame, before its tick; `passive` after the frame's
    /// paint puts its tape line in): none while a gate waits. `to_devices`: the match's devices take them as well
    /// (`device_input`).
    pub fn drive(&mut self, to_devices: bool) -> Vec<Fed> {
        self.flush();
        self.driving = true;
        if self.player.waiting().is_some() || self.done() {
            return Vec::new();
        }
        let n = self.player.frame();
        let fed = self.player.feed();
        // (a gate's frame is the script's once it is released)
        if self.player.waiting().is_none() {
            self.driven = Some(n);
        }
        if to_devices {
            self.device_input.extend(DeviceInput::of(&fed));
        }
        fed
    }

    /// The frame's events for a screen with no match under it that takes the script's input (the results screen),
    /// as the menus take them: the pad line's state each frame from its line on. Call once per frame, before the
    /// screen's frame; `passive` after its paint puts the tape line in.
    pub fn screen_input(&mut self) -> Vec<MenuInput> {
        let mut input = Vec::new();
        for f in self.drive(false) {
            match f {
                Fed::Input(i) => input.push(i),
                Fed::Pad(p) => self.pad = Some(MenuPad { axes: p.axes, buttons: p.buttons }),
            }
        }
        if let Some(state) = self.pad {
            input.push(MenuInput::Pad { pad: 0, player: 0, state });
        }
        input
    }

    /// The frame's events (call once per frame, before the menus' tick).
    pub fn feed(&mut self) -> Vec<Fed> {
        self.flush();
        self.driving = false;
        self.player.feed()
    }

    /// The frame `feed` just fed, painted: its tape line goes in, and whether a picture is due.
    pub fn painted(&mut self, frame: MenuFrame) -> bool {
        let shot = self.shots.contains(&frame.frame);
        if let Some((_, t)) = &mut self.tape {
            t.frames.push(frame);
        }
        shot
    }

    pub fn done(&self) -> bool {
        self.player.done()
    }

    /// The tape written, when `--menu-tape` named one.
    pub fn finish(self) -> Result<()> {
        if let Some((path, t)) = self.tape {
            t.write(&path).map_err(Error::Ui)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const S: &str = r#"{"start":"title"}
{"f":0,"ev":"move","x":960,"y":540}
{"f":2,"ev":"key","code":"Enter","down":true}
{"f":2,"ev":"key","code":"Enter","down":true}
{"f":3,"ev":"key","code":"Enter","down":false}
{"f":3,"ev":"wheel","dy":-200}
{"f":4,"ev":"pad","axes":[0.5,0,0,-1],"buttons":[0,12]}
{"f":6,"ev":"end"}
"#;

    #[test]
    fn a_do_line_is_kept_for_the_menus_and_the_query_read() {
        let text = r#"{"query":"?test&stay&queue=fake&queueBots=3"}
{"f":0,"ev":"do","what":"showModal","args":["WBP_MatchOptions","matchoptions"]}
{"f":0,"ev":"down","x":1,"y":2}
"#;
        let s = Script::parse("t", text).unwrap();
        assert_eq!(s.query, "?test&stay&queue=fake&queueBots=3");
        let mut p = Player::new(s);
        assert_eq!(p.feed().len(), 1);
        assert_eq!(
            p.take_dos(),
            vec![("showModal".to_owned(), serde_json::json!(["WBP_MatchOptions", "matchoptions"]))]
        );
        assert!(p.take_dos().is_empty());
    }

    #[test]
    fn a_script_reads() {
        let s = Script::parse("t", S).unwrap();
        assert_eq!(s.start, Start::Title);
        assert_eq!(s.events.len(), 6);
        assert_eq!(s.frames, 7);
    }

    #[test]
    fn keys_repeat_and_the_wheel_flips() {
        let mut p = Player::new(Script::parse("t", S).unwrap());
        assert_eq!(p.feed(), [Fed::Input(MenuInput::MouseMove { x: 960.0, y: 540.0 })]);
        assert!(p.feed().is_empty());
        let f2 = p.feed();
        assert_eq!(f2[0], Fed::Input(MenuInput::Key { key: "Enter", down: true, repeat: false, player: 0 }));
        assert_eq!(f2[1], Fed::Input(MenuInput::Key { key: "Enter", down: true, repeat: true, player: 0 }));
        let f3 = p.feed();
        assert_eq!(f3[1], Fed::Input(MenuInput::Wheel { notches: 2.0 }));
        let Fed::Pad(pad) = p.feed()[0] else { panic!("pad") };
        assert_eq!(pad.buttons, 1 | (1 << 12));
        assert!(!p.done());
        p.feed();
        p.feed();
        assert!(p.done());
    }

    #[test]
    fn a_gate_stops_the_script_until_it_is_released() {
        let text = r#"{"f":1,"ev":"move","x":1,"y":2}
{"f":2,"ev":"key","code":"Enter"}
{"f":2,"ev":"gate","for":"lab"}
{"f":2,"ev":"key","code":"Escape"}
{"f":3,"ev":"end"}"#;
        let mut p = Player::new(Script::parse("g", text).unwrap());
        p.feed();
        p.feed();
        assert_eq!(p.frame(), 2);
        assert_eq!(p.feed().len(), 1);
        assert_eq!(p.waiting(), Some(Gate::Lab));
        assert_eq!(p.frame(), 2, "the frame of a gate does not pass");
        assert!(p.feed().is_empty());
        assert_eq!(p.frame(), 2);
        p.release();
        let f = p.feed();
        assert_eq!(f, [Fed::Input(MenuInput::Key { key: "Escape", down: true, repeat: false, player: 0 })]);
        assert_eq!(p.frame(), 3);
    }

    #[test]
    fn a_gate_never_released_ends_the_run() {
        let text = "{\"f\":0,\"ev\":\"gate\",\"for\":\"menu\"}\n{\"f\":1,\"ev\":\"end\"}";
        let mut p = Player::new(Script::parse("g", text).unwrap());
        assert!(p.frame_passed().is_ok(), "no gate yet");
        p.feed();
        for _ in 1..GATE_TIMEOUT {
            assert!(p.frame_passed().is_ok());
        }
        let e = p.frame_passed().unwrap_err();
        assert!(e.contains("menu gate"), "{e}");
        p.release();
        assert!(p.frame_passed().is_ok());
    }

    #[test]
    fn frames_count_on_a_screen_the_script_does_not_drive() {
        let text = "{\"shots\":[2]}\n{\"f\":1,\"ev\":\"gate\",\"for\":\"lab\"}\n{\"f\":3,\"ev\":\"end\"}";
        let script = Script::parse("p", text).unwrap();
        let shots = script.shots.iter().copied().collect();
        let mut r = ScriptRun {
            player: Player::new(script),
            tape: None,
            shots,
            lab_up: 0,
            match_input: Vec::new(),
            device_input: Vec::new(),
            clicks: Vec::new(),
            pressing: Vec::new(),
            driven: None,
            driving: false,
            pad: None,
            live_loads: false,
        };
        assert_eq!(r.passive(false, None), None, "frame 0: no picture");
        assert_eq!(r.passive(false, None), None, "frame 1's gate");
        for _ in 0..GATE_SETTLE {
            assert_eq!(r.passive(true, None), None, "the gate settles");
        }
        assert_eq!(r.player.waiting(), Some(Gate::Lab));
        assert_eq!(r.passive(true, None), None, "released: frame 1");
        assert_eq!(r.passive(true, None), Some(2));
        assert!(!r.done());
        assert_eq!(r.passive(true, None), None);
        assert!(r.done(), "the end's frame passed");
        assert_eq!(r.passive(true, None), None);
        assert_eq!(r.player.frame(), 4, "no frame past the end");
    }

    #[test]
    fn the_lab_takes_a_released_gates_frame_before_its_tick() {
        let text = "{\"shots\":[1]}
{\"f\":0,\"ev\":\"gate\",\"for\":\"lab\"}
{\"f\":1,\"ev\":\"key\",\"code\":\"F1\",\"down\":true}
{\"f\":2,\"ev\":\"end\"}";
        let script = Script::parse("d", text).unwrap();
        let shots = script.shots.iter().copied().collect();
        let mut r = ScriptRun {
            player: Player::new(script),
            tape: None,
            shots,
            lab_up: 0,
            match_input: Vec::new(),
            device_input: Vec::new(),
            clicks: Vec::new(),
            pressing: Vec::new(),
            driven: None,
            driving: false,
            pad: None,
            live_loads: false,
        };
        assert!(r.drive(true).is_empty(), "frame 0's gate");
        assert_eq!(r.passive(true, None), None);
        for _ in 1..GATE_SETTLE {
            assert!(r.drive(true).is_empty());
            assert_eq!(r.passive(true, None), None, "the gate settles");
        }
        assert!(r.drive(true).is_empty());
        assert_eq!(r.passive(true, None), None, "released: its frame is the next drive's");
        assert_eq!(r.player.waiting(), None);
        assert!(r.drive(true).is_empty(), "frame 0's rest");
        assert_eq!(r.passive(true, None), None);
        assert_eq!(r.drive(true), [Fed::Input(MenuInput::Key { key: "F1", down: true, repeat: false, player: 0 })]);
        assert_eq!(r.passive(true, None), Some(1), "frame 1's picture after its paint");
    }

    #[test]
    fn a_bad_line_is_named() {
        assert!(Script::parse("t", r#"{"f":1,"ev":"key","code":"Nope"}"#).is_err());
        assert!(Script::parse("t", "{\"f\":2,\"ev\":\"end\"}\n{\"f\":1,\"ev\":\"end\"}").is_err());
        assert!(Script::parse("t", r#"{"f":0,"ev":"jump"}"#).is_err());
    }

    #[test]
    fn dom_codes_map_to_fkeys() {
        assert_eq!(key_name("KeyW"), Some("W"));
        assert_eq!(key_name("Digit3"), Some("Three"));
        assert_eq!(key_name("ArrowLeft"), Some("Left"));
        assert_eq!(key_name("F5"), Some("F5"));
        assert_eq!(key_name("Numpad2"), Some("NumPadTwo"));
        assert_eq!(key_name("Nope"), None);
    }

    #[test]
    fn a_win_line_is_a_gate_the_match_releases() {
        let text = r#"{"start":"menu"}
{"f":2,"ev":"do","what":"win"}
{"f":4,"ev":"key","code":"Enter","down":true}
"#;
        let s = Script::parse("w", text).expect("the script");
        let mut p = Player::new(s);
        for _ in 0..3 {
            p.feed();
        }
        assert_eq!(p.waiting(), Some(Gate::Win));
        assert!(p.take_dos().is_empty());
        // (the frames of the wait are not the script's)
        assert!(p.feed().is_empty());
        p.release();
        let mut fed = Vec::new();
        for _ in 0..3 {
            fed.extend(p.feed());
        }
        assert_eq!(fed.len(), 1);
    }
}
