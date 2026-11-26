//! A sound event of the store played through the offline mixer and written as a WAV, to hear an event by name (the
//! sounds of the port's own features are picked from the build's events this way).
//!
//! `cargo run --release -p mvs-app --example sound_wav -- <out dir> <event>...`: one `<event>.wav` per event, the
//! mix from the post until a second after the event's length (at most 8 s); `--sure` first posts them past their
//! Play actions' chance (`Call::PostSure`). `--list <prefix>` prints the events whose name starts with the prefix
//! and their lengths instead.

use std::path::PathBuf;
use std::sync::Arc;

use mvs_audio::tape::post::{Call, Line};
use mvs_audio::{Audio, Backend, QUANTUM, RATE};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args: Vec<String> = std::env::args().skip(1).collect();
    let sure = args.first().is_some_and(|a| a == "--sure");
    if sure {
        args.remove(0);
    }
    let store = Arc::new(mvs_store::Store::open(mvs_store::dev_store_dir())?);
    let mut audio = Audio::new(store, Backend::Offline)?;
    if let [flag, prefix] = &args[..]
        && flag == "--list"
    {
        for (name, e) in &audio.init().events {
            if name.starts_with(prefix.as_str()) {
                let len = e.dur[1].map_or("?".to_owned(), |d| format!("{d:.2} s"));
                println!("{name}  {len}");
            }
        }
        return Ok(());
    }
    let Some((dir, events)) = args.split_first() else {
        return Err("usage: sound_wav [--sure] <out dir> <event>... | --list <prefix>".into());
    };
    let dir = PathBuf::from(dir);
    std::fs::create_dir_all(&dir)?;
    audio.call(&Line { tick: 0, call: Call::Resume })?;
    for ev in events {
        let Some(len) = audio.init().events.get(ev.as_str()).map(|e| e.dur[1].unwrap_or(2.0)) else {
            eprintln!("{ev}: no such event");
            continue;
        };
        let keep = vec![ev.clone()];
        audio.call(&Line { tick: 0, call: Call::Scope { name: "wav".into(), keep, lazy: Vec::new() } })?;
        let _ = audio.take_output();
        let post = if sure {
            Call::PostSure { ev: ev.clone(), owner: -1 }
        } else {
            Call::Post { ev: ev.clone(), owner: Some(-1), key: None, stop_key: None, at: None }
        };
        audio.call(&Line { tick: 0, call: post })?;
        let seconds = (len + 1.0).min(8.0);
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss, reason = "a few seconds of quanta")]
        let quanta = (seconds * f64::from(RATE) / QUANTUM as f64).ceil() as usize;
        for _ in 0..quanta {
            audio.pump(0, QUANTUM);
        }
        let out = audio.take_output();
        let peak = out.iter().fold(0f32, |m, s| m.max(s.abs()));
        let path = dir.join(format!("{ev}.wav"));
        mvs_audio::out::write_wav(&path, RATE, 2, &out)?;
        println!("{} ({len:.2} s, peak {peak:.3})", path.display());
        audio.call(&Line { tick: 0, call: Call::Scope { name: "wav".into(), keep: Vec::new(), lazy: Vec::new() } })?;
    }
    Ok(())
}
