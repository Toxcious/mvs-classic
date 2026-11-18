//! The input tape: per sim tick of an oracle run, the raw input the oracle got since the last tick (the keyboard's key
//! events, the pads as its pad readers saw them) and what each device's read gave the tick (
//! InputDevice.read: the InputState the sim is given). The oracle's recorder writes it, from
//! a Local Play match whose human reads the keyboard and pad 0. The check of
//! `input`: the raw events replayed into the devices, every tick's reads equal to the tape's.
//!
//! Not on the tape: the mouse, the window's blur, the pad claim and the Preferences the
//! reads depend on (the sticks' dead zones, the right stick's style, Up To Jump). The run has the defaults, and a
//! replay takes them.
//!
//! The file is JSON lines: a header line, then one line per tick. Tapes are the oracle's runs: they stay in
//! `.scratch/`, never committed.

use std::path::Path;

use mvs_sim::InputState;
use mvs_sim::input::{AttackBtn, Btn, Dir, Order};
use serde::Deserialize;

use crate::{Error, Result};

/// The tape's first line: `tape` "input", its `version`, the run's `humans` (their devices, 'kbm+pad0') and the tick
/// its script `start`s at.
#[derive(Clone, Debug, Deserialize)]
pub struct TapeHeader {
    pub tape: String,
    pub version: u32,
    #[serde(default)]
    pub run: String,
    #[serde(default)]
    pub humans: Vec<String>,
    #[serde(default)]
    pub start: u32,
}

/// One tick: the World's tick before its step (the tick the reads were given to), the key events since the last
/// tick in the order they came, the devices' reads in call order.
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TapeTick {
    pub tick: u32,
    pub keys: Vec<KeyEvent>,
    pub reads: Vec<TapeRead>,
}

/// A keydown or keyup.
#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct KeyEvent {
    /// KeyboardEvent.code: 'KeyJ', 'Space', 'ArrowLeft'.
    pub code: String,
    pub down: bool,
    pub repeat: bool,
}

/// One InputDevice.read.
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TapeRead {
    /// 'kbm', 'kbm+pad0', 'pad<n>'.
    pub device: String,
    /// The binding set (`<Action>-P<set>`).
    pub set: u8,
    /// The device's pad at the read; None for none.
    pub pad: Option<TapePad>,
    /// What read() returned.
    #[serde(deserialize_with = "input_state")]
    pub out: InputState,
}

/// A pad as the oracle's reader saw it (the browser's Gamepad, standard mapping).
#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct TapePad {
    pub index: u32,
    /// [lx, ly, rx, ry] in the browser's signs: -1..1, down positive.
    pub axes: [f64; 4],
    /// The 17 standard-mapping buttons as (value 0..1, pressed) (0 A, 1 B, 2 X, 3 Y, 4 LB, 5 RB, 6 LT, 7 RT, 8 Back,
    /// 9 Start, 10 LS, 11 RS, 12 up, 13 down, 14 left, 15 right, 16 home). InputDevice reads `pressed`.
    pub buttons: Vec<(f64, bool)>,
}

/// The oracle's InputState as JSON, the fields a device may set.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct RawInput {
    x: f64,
    y: f64,
    attack: bool,
    special: bool,
    jump: bool,
    dodge: bool,
    #[serde(default)]
    jump_local: bool,
    #[serde(default)]
    item: bool,
    order: Option<Vec<String>>,
    #[serde(default)]
    down_btn: bool,
    atk_dir: Option<u8>,
    force_dir: Option<u8>,
    force_press: Option<String>,
    force_release: Option<String>,
    #[serde(default)]
    force: bool,
    #[serde(default)]
    neutral_evade: bool,
    taunt: Option<u8>,
    #[serde(default)]
    taunt_held: bool,
    rsx: Option<f64>,
    #[serde(default)]
    rsy: Option<f64>,
    #[serde(default)]
    axis: bool,
    #[serde(default)]
    call_x: bool,
    #[serde(default)]
    call_y: bool,
}

fn btn(s: &str) -> std::result::Result<Btn, String> {
    Ok(match s {
        "attack" => Btn::Attack,
        "special" => Btn::Special,
        "jump" => Btn::Jump,
        "dodge" => Btn::Dodge,
        "item" => Btn::Item,
        _ => return Err(format!("order: no button {s}")),
    })
}

fn attack_btn(s: &str) -> std::result::Result<AttackBtn, String> {
    Ok(match s {
        "attack" => AttackBtn::Attack,
        "special" => AttackBtn::Special,
        _ => return Err(format!("no attack button {s}")),
    })
}

fn dir(d: u8) -> std::result::Result<Dir, String> {
    const ALL: [Dir; 10] = [
        Dir::Up,
        Dir::Down,
        Dir::Fwd,
        Dir::Back,
        Dir::Neutral,
        Dir::Default,
        Dir::FwdUp,
        Dir::FwdDown,
        Dir::BackUp,
        Dir::BackDown,
    ];
    ALL.get(d as usize).copied().ok_or_else(|| format!("atkDir: no Dir {d}"))
}

impl TryFrom<RawInput> for InputState {
    type Error = String;

    fn try_from(r: RawInput) -> std::result::Result<InputState, String> {
        let order = match r.order {
            None => None,
            Some(list) => {
                // (canonInput keeps the first five)
                let mut o = Order::default();
                for (i, b) in list.iter().take(5).enumerate() {
                    o.btns[i] = Some(btn(b)?);
                    o.len = i as u8 + 1;
                }
                Some(o)
            }
        };
        Ok(InputState {
            x: mvs_sim::math::narrow(r.x),
            y: mvs_sim::math::narrow(r.y),
            attack: r.attack,
            special: r.special,
            jump: r.jump,
            dodge: r.dodge,
            jump_local: r.jump_local,
            item: r.item,
            order,
            down_btn: r.down_btn,
            atk_dir: r.atk_dir.map(dir).transpose()?,
            force_dir: r.force_dir,
            force_press: r.force_press.as_deref().map(attack_btn).transpose()?,
            force_release: r.force_release.as_deref().map(attack_btn).transpose()?,
            force: r.force,
            neutral_evade: r.neutral_evade,
            taunt: r.taunt,
            taunt_held: r.taunt_held,
            rsx: r.rsx.map(mvs_sim::math::narrow),
            rsy: r.rsy.map(mvs_sim::math::narrow),
            axis: r.axis,
            call_x: r.call_x,
            call_y: r.call_y,
        })
    }
}

fn input_state<'de, D: serde::Deserializer<'de>>(d: D) -> std::result::Result<InputState, D::Error> {
    InputState::try_from(RawInput::deserialize(d)?).map_err(serde::de::Error::custom)
}

/// A whole tape.
#[derive(Clone, Debug)]
pub struct InputTape {
    pub header: TapeHeader,
    pub ticks: Vec<TapeTick>,
}

fn bad(what: impl std::fmt::Display, why: impl std::fmt::Display) -> Error {
    Error::Match(format!("input tape {what}: {why}"))
}

impl InputTape {
    /// The tape at `path`.
    pub fn read(path: &Path) -> Result<InputTape> {
        let text = std::fs::read_to_string(path).map_err(|e| bad(path.display(), e))?;
        InputTape::parse(&text).map_err(|e| match e {
            Error::Match(why) => Error::Match(format!("{}: {why}", path.display())),
            e => e,
        })
    }

    /// A tape from its lines.
    pub fn parse(text: &str) -> Result<InputTape> {
        let mut lines = text.lines().enumerate().filter(|(_, l)| !l.is_empty());
        let (_, first) = lines.next().ok_or_else(|| bad("line 1", "no header"))?;
        let header: TapeHeader = serde_json::from_str(first).map_err(|e| bad("line 1", e))?;
        if header.tape != "input" || header.version != 1 {
            return Err(bad(
                "line 1",
                format!("a {} tape of version {}, not an input tape of version 1", header.tape, header.version),
            ));
        }
        let mut ticks = Vec::new();
        for (i, l) in lines {
            ticks.push(serde_json::from_str::<TapeTick>(l).map_err(|e| bad(format!("line {}", i + 1), e))?);
        }
        Ok(InputTape { header, ticks })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"{"tape":"input","version":1,"run":"input-tape","humans":["kbm+pad0"],"start":240}
{"tick":0,"keys":[],"reads":[{"device":"kbm+pad0","set":0,"pad":null,"out":{"x":0,"y":0,"attack":false,"special":false,"force":false,"rsx":0,"jump":false,"dodge":false,"neutralEvade":false,"item":false,"order":[],"axis":false}}]}
{"tick":1,"keys":[{"code":"KeyJ","down":true,"repeat":false}],"reads":[{"device":"kbm+pad0","set":0,"pad":{"index":0,"axes":[0.5,0,0,0],"buttons":[[0,false],[0,false],[0,false],[0,false],[0,false],[0,false],[0,false],[0.3,false],[0,false],[0,false],[0,false],[0,false],[0,false],[0,false],[0,false],[1,true],[0,false]]},"out":{"x":0.5,"y":0,"attack":true,"special":false,"force":true,"forceDir":2,"forcePress":"attack","rsx":0,"jump":false,"dodge":false,"neutralEvade":false,"item":false,"order":["attack","jump"],"axis":true,"taunt":1,"tauntHeld":true}}]}
"#;

    #[test]
    fn a_tape_reads_back() {
        let t = InputTape::parse(SAMPLE).unwrap();
        assert_eq!(t.header.humans, ["kbm+pad0"]);
        assert_eq!(t.header.start, 240);
        assert_eq!(t.ticks.len(), 2);
        let r0 = &t.ticks[0].reads[0];
        assert!(r0.pad.is_none());
        assert_eq!(r0.out.order.unwrap().len, 0);
        assert_eq!(r0.out.rsx, Some(0.0));
        let k = &t.ticks[1];
        assert_eq!(k.keys[0], KeyEvent { code: "KeyJ".into(), down: true, repeat: false });
        let o = k.reads[0].out;
        assert_eq!(o.x, 0.5);
        assert_eq!(o.force_dir, Some(2));
        assert_eq!(o.force_press, Some(AttackBtn::Attack));
        assert_eq!(o.order.unwrap().iter().collect::<Vec<_>>(), [Btn::Attack, Btn::Jump]);
        assert_eq!(o.taunt, Some(1));
        assert!(o.taunt_held && o.axis);
        let pad = k.reads[0].pad.as_ref().unwrap();
        assert_eq!(pad.axes[0], 0.5);
        assert_eq!((pad.buttons[7], pad.buttons[15]), ((0.3, false), (1.0, true)));
    }

    #[test]
    fn a_field_the_reader_does_not_know_is_refused() {
        let s = SAMPLE.replace(r#""axis":true"#, r#""axis":true,"extra":1"#);
        assert!(InputTape::parse(&s).is_err());
    }

    /// The tape `INPUT_TAPE` names (an oracle run's), when it is set: every tick reads, in
    /// order from 0, each with a read per human.
    #[test]
    fn the_runs_tape_reads() {
        let Some(p) = std::env::var_os("INPUT_TAPE") else {
            eprintln!("note: INPUT_TAPE names no tape: not checked");
            return;
        };
        let t = InputTape::read(Path::new(&p)).unwrap();
        assert!(!t.ticks.is_empty());
        for (i, k) in t.ticks.iter().enumerate() {
            assert_eq!(k.tick as usize, i);
            assert_eq!(k.reads.len(), t.header.humans.len(), "tick {i}");
        }
        let keys: usize = t.ticks.iter().map(|k| k.keys.len()).sum();
        let pads = t.ticks.iter().filter(|k| k.reads.iter().any(|r| r.pad.is_some())).count();
        eprintln!("{}: {} ticks, {keys} key events, {pads} ticks with a pad", Path::new(&p).display(), t.ticks.len());
    }
}
