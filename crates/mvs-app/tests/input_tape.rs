//! The input tape `INPUT_TAPE` names (an oracle run's) replayed into the devices: each
//! tick's key events and pads given to them, every read equal to the oracle's as canonInput gives it to the sim.

use std::path::Path;

use mvs_app::input::pad::{PadState, SLOTS, XINPUT_STICK_UP};
use mvs_app::input::{DeviceKind, Devices};
use mvs_app::tape::{InputTape, TapePad};
use mvs_ui::bindings::Bindings;

const LETTERS: [&str; 26] = [
    "A", "B", "C", "D", "E", "F", "G", "H", "I", "J", "K", "L", "M", "N", "O", "P", "Q", "R", "S", "T", "U", "V", "W",
    "X", "Y", "Z",
];

/// KeyboardEvent.code as the oracle names its FKey, the codes a tape's script presses.
fn key_of(code: &str) -> &'static str {
    match code {
        "Space" => "SpaceBar",
        "ArrowLeft" => "Left",
        "ArrowRight" => "Right",
        "ArrowUp" => "Up",
        "ArrowDown" => "Down",
        "ShiftLeft" => "LeftShift",
        _ => {
            let c = code.strip_prefix("Key").filter(|c| c.len() == 1).and_then(|c| c.bytes().next());
            match c {
                Some(b @ b'A'..=b'Z') => LETTERS[usize::from(b - b'A')],
                _ => panic!("the replay names no key for code {code}"),
            }
        }
    }
}

/// A browser pad (standard mapping, down positive) as the analog events and the buttons' bits give it.
fn pad_of(p: &TapePad) -> PadState {
    let mut buttons = 0u16;
    for (i, &(_, pressed)) in p.buttons.iter().enumerate().take(16) {
        if pressed {
            buttons |= 1 << i;
        }
    }
    let ly = -p.axes[1];
    PadState { buttons, lx: p.axes[0], ly, rx: p.axes[2], ry: -p.axes[3], stick_up: ly > XINPUT_STICK_UP }
}

#[test]
fn the_tape_replays_into_the_devices() {
    let Some(path) = std::env::var_os("INPUT_TAPE") else {
        eprintln!("note: INPUT_TAPE names no tape: not checked");
        return;
    };
    let tape = InputTape::read(Path::new(&path)).unwrap();
    assert_eq!(tape.header.humans, ["kbm+pad0"], "the replay makes one kbm+pad0 human");
    let store = mvs_store::Store::open(mvs_store::dev_store_dir()).unwrap();
    let data = mvs_data::GameData::load(&store, &["shaggy"], "trainingroomsmall").unwrap();
    let mut devices = Devices::new(Bindings::new(data.input_defaults()), &[(0, DeviceKind::KbmPad0, 0)]);
    devices.device(0).unwrap().reset();
    let mut held: Vec<&str> = Vec::new();
    let (mut keys, mut pads) = (0, 0);
    for t in &tape.ticks {
        for k in &t.keys {
            let name = key_of(&k.code);
            // (the devices take a down of a held key as its repeat, as the OS sends one)
            assert_eq!(k.down && held.contains(&name), k.repeat, "tick {}: {}'s repeat", t.tick, k.code);
            if k.down {
                held.push(name);
            } else {
                held.retain(|h| *h != name);
            }
            devices.key(name, k.down);
            keys += 1;
        }
        let read = &t.reads[0];
        assert_eq!((read.device.as_str(), read.set), ("kbm+pad0", 0), "tick {}", t.tick);
        let mut state = [None; SLOTS];
        if let Some(p) = &read.pad {
            state[p.index as usize] = Some(pad_of(p));
            pads += 1;
        }
        devices.set_pads(state);
        let ours = devices.device(0).unwrap().read();
        // (the oracle's read is the state before canonInput; the devices' is the sim's, after it)
        assert_eq!(ours, mvs_sim::input::canon_input(&read.out), "tick {}", t.tick);
    }
    eprintln!("{}: {} ticks equal ({keys} key events, {pads} reads with a pad)", path.display(), tape.ticks.len());
}
