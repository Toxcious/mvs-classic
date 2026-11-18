//! The sound engine's warnings over the oracle's post tape of a whole match (`hud.tape`, recorded by
//! the oracle's recorder), played offline on the developer's store with the port's own base scope
//! (`base_events`) in place of the oracle's menu, base and preload scopes: no lazy event's medium may be missing when
//! it
//! plays; the posts dropped for an event no scope holds are counted and printed.
//!
//! `MVS_AUDIO_TAPES` names the tapes' directory (default the main tree's `.scratch/audio-tapes`); a run without
//! the tape or the store says so and passes.

use std::path::PathBuf;
use std::sync::Arc;

use indexmap::IndexMap;
use mvs_app::audio::{Index, base_events, hud_named};
use mvs_audio::tape::post::{Call, Line};
use mvs_audio::{Audio, Backend, Note, QUANTUM, RATE};

fn tapes() -> PathBuf {
    std::env::var_os("MVS_AUDIO_TAPES")
        .map_or_else(|| mvs_sim_trace::run::default_traces().join("../audio-tapes"), PathBuf::from)
}

/// The oracle's own scopes outside a match, which the port makes elsewhere.
fn page_only(l: &Line) -> bool {
    matches!(&l.call, Call::Scope { name, .. } if ["menu", "base", "preload"].contains(&name.as_str()))
}

#[test]
fn hud_tape_warnings() {
    let path = tapes().join("hud.tape");
    let Ok(tape) = mvs_audio::tape::read::<Line>(&path) else {
        eprintln!("note: no tape {}; nothing checked", path.display());
        return;
    };
    let Ok(store) = mvs_store::Store::open(mvs_store::dev_store_dir()) else {
        eprintln!("note: no store; nothing checked");
        return;
    };
    let mut a = Audio::new(Arc::new(store), Backend::Offline).expect("the offline audio");
    let known = Index::new(a.init().events.keys().cloned());
    let hud = hud_named(a.store(), &known);
    let base = Call::Scope { name: "base".into(), keep: base_events(&known, &hud), lazy: Vec::new() };
    a.call(&Line { tick: 0, call: base }).expect("the base scope");
    let mut warns: IndexMap<&'static str, u32> = IndexMap::new();
    let mut failed = 0;
    let per_tick = RATE as usize / 60;
    let mut owed = 0;
    let mut at = tape.first().map_or(0, |l| l.tick);
    // (one tick of sound mixed, its notes taken: the lazy posts whose media are in go out with them)
    let mut step = |a: &mut Audio, tick: u32, warns: &mut IndexMap<&'static str, u32>| {
        owed += per_tick;
        let frames = owed / QUANTUM * QUANTUM;
        owed -= frames;
        a.pump(tick, frames);
        drop(a.take_output());
        for n in a.take_notes() {
            if let Note::Warn { what, .. } = n {
                *warns.entry(what).or_insert(0) += 1;
            }
        }
    };
    for l in tape.iter().filter(|l| !page_only(l)) {
        while at < l.tick {
            step(&mut a, at, &mut warns);
            at += 1;
        }
        if let Err(e) = a.call(l) {
            failed += 1;
            eprintln!("tick {}: {e}", l.tick);
        }
    }
    for _ in 0..120 {
        step(&mut a, at, &mut warns);
        at += 1;
    }
    eprintln!("hud.tape: {} lines, {failed} calls failed; warnings: {warns:?}", tape.len());
    assert_eq!(warns.get("medium not loaded"), None, "a lazy event played without its medium");
}
