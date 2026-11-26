//! The pads as the game reads them, printed: each controller id's reader and each change of its state (a button,
//! or a stick moved past 0.05 since the last line: an idle stick's jitter is not printed).
//!
//! `cargo run --release -p mvs-app --example pad_log -- [seconds] [--rumble]`: polls at 60 Hz for `seconds` (default
//! 10), looks for Sony pads again each second (no window, so no arrival message), and with `--rumble` runs each
//! pad's motors at half for half a second at the start.

use std::time::{Duration, Instant};

use mvs_app::input::pad::{PadState, Pads, SLOTS};
use mvs_ui::bindings::PAD_BUTTONS;

fn line(id: usize, p: &PadState) -> String {
    let keys: Vec<&str> =
        (0..16).filter(|i| p.buttons & (1 << i) != 0).map(|i| PAD_BUTTONS[i].trim_start_matches("Gamepad_")).collect();
    format!(
        "id {id}: L ({:+.4}, {:+.4}) R ({:+.4}, {:+.4}) stick_up {} [{}]",
        p.lx,
        p.ly,
        p.rx,
        p.ry,
        p.stick_up,
        keys.join(" ")
    )
}

/// Whether `b` differs from `a` by more than the idle jitter.
fn moved(a: Option<PadState>, b: Option<PadState>) -> bool {
    match (a, b) {
        (Some(a), Some(b)) => {
            a.buttons != b.buttons
                || a.stick_up != b.stick_up
                || [a.lx - b.lx, a.ly - b.ly, a.rx - b.rx, a.ry - b.ry].iter().any(|d| d.abs() > 0.05)
        }
        (a, b) => a.is_some() != b.is_some(),
    }
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let secs = args.iter().find_map(|a| a.parse::<f64>().ok()).unwrap_or(10.0);
    let rumble = args.iter().any(|a| a == "--rumble");
    let mut pads = Pads::default();
    let start = Instant::now();
    let mut rescan = start;
    let mut last = [None; SLOTS];
    let mut owners = [None; SLOTS];
    #[cfg(windows)]
    let mut listed = 0;
    while start.elapsed().as_secs_f64() < secs {
        if rescan.elapsed() >= Duration::from_secs(1) {
            rescan = Instant::now();
            pads.device_arrived();
        }
        pads.poll();
        #[cfg(windows)]
        let sony = pads.sony.describe();
        #[cfg(windows)]
        if sony.len() != listed {
            listed = sony.len();
            for s in &sony {
                println!(
                    "sony pad {}: {:?} {} reports in {} / out {} bytes, {}",
                    s.serial,
                    s.model,
                    if s.bt { "Bluetooth" } else { "USB" },
                    s.lens.0,
                    s.lens.1,
                    s.path
                );
            }
        }
        for id in 0..SLOTS {
            if pads.owner[id] != owners[id] {
                owners[id] = pads.owner[id];
                println!("id {id}: {:?}", owners[id]);
            }
            if moved(last[id], pads.state[id]) {
                last[id] = pads.state[id];
                match &last[id] {
                    Some(p) => println!("{:7.3}s {}", start.elapsed().as_secs_f64(), line(id, p)),
                    None => println!("{:7.3}s id {id}: none", start.elapsed().as_secs_f64()),
                }
            }
        }
        if rumble {
            let on = start.elapsed() < Duration::from_millis(500);
            for id in 0..SLOTS {
                if pads.owner[id].is_some() {
                    pads.rumble(id, if on { 0.5 } else { 0.0 }, if on { 0.5 } else { 0.0 });
                }
            }
        }
        std::thread::sleep(Duration::from_millis(16));
    }
    #[cfg(windows)]
    for s in pads.sony.describe() {
        println!("sony pad {}: output refused {}", s.serial, s.write_failed);
    }
}
