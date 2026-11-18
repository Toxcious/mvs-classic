//! A Lab left from its pause menu lets the pause mix go: mix_pause_on mutes hdr_sfx, the SFX bus and one more and
//! pauses all but the ambience's and music's buses, so without its mix_pause_off most effects of every later match
//! (the shared combat and item sounds among them) are silent while the music plays. The session below is the exe's:
//! the base scope, a Lab match on Sky Arena, its pause menu up (mix_pause_on), the Lab left from it
//! (`MatchAudio::left_paused`, then its end), and the next match's shared effects.
//!
//! The dev store and the stage trace's header are the inputs; without them it says so and passes.

use std::sync::Arc;

use mvs_app::audio::{BeginOpts, Index, MatchAudio, base_events, hud_named};
use mvs_audio::tape::post::{Call, Line};
use mvs_audio::{Audio, Backend, QUANTUM, RATE};

const TRACE: &str = "stage-m011";

/// The peak of the output over `ticks` ticks from `tick`.
fn mix(a: &mut Audio, tick: &mut u32, ticks: u32) -> f32 {
    let mut peak = 0.0f32;
    let mut owed = 0;
    for _ in 0..ticks {
        owed += RATE as usize / 60;
        let frames = owed / QUANTUM * QUANTUM;
        owed -= frames;
        a.pump(*tick, frames);
        peak = a.take_output().iter().fold(peak, |p, s| p.max(s.abs()));
        let _ = a.take_notes();
        *tick += 1;
    }
    peak
}

fn call(a: &mut Audio, l: &Line) {
    a.call(l).unwrap_or_else(|e| panic!("{l:?}: {e}"));
}

/// The session up to the next match: the peak of each of its shared effects posted alone. `released`: the
/// Lab's leaving posts the pause mix's end.
fn session(store: &Arc<mvs_store::Store>, known: &Index, w: &mvs_sim::World, released: bool) -> Vec<f32> {
    let mut a = Audio::new(store.clone(), Backend::Offline).expect("the offline audio");
    let hud = hud_named(a.store(), known);
    call(
        &mut a,
        &Line { tick: 0, call: Call::Scope { name: "base".into(), keep: base_events(known, &hud), lazy: Vec::new() } },
    );
    let opts = || BeginOpts { music_skin: Some("mus_base".into()), ..BeginOpts::default() };
    let mut tick = 0;
    // the Lab: its match, its pause menu up, then left from it
    let mut lines = Vec::new();
    let mut lab = MatchAudio::with_opts(w, known.clone(), &mut lines, opts());
    for l in &lines {
        call(&mut a, l);
    }
    mix(&mut a, &mut tick, 30);
    let pause = |ev: &str, tick: u32| Line {
        tick,
        call: Call::Post { ev: ev.into(), owner: Some(-1), key: None, stop_key: None, at: None },
    };
    call(&mut a, &pause("mix_pause_on", tick));
    mix(&mut a, &mut tick, 30);
    let mut end = Vec::new();
    if released {
        MatchAudio::left_paused(i32::try_from(tick).unwrap(), &mut end);
    }
    lab.end(i32::try_from(tick).unwrap(), false, &mut end);
    for l in &end {
        call(&mut a, l);
    }
    mix(&mut a, &mut tick, 60);
    // the next match: its fighters' one-shots
    let mut lines = Vec::new();
    let _next = MatchAudio::with_opts(w, known.clone(), &mut lines, opts());
    for l in &lines {
        call(&mut a, l);
    }
    mix(&mut a, &mut tick, 30);
    // (the music and the ambience stopped: only the effects are measured)
    call(&mut a, &Line { tick, call: Call::StopAll { fade: 0.0, keep: None } });
    mix(&mut a, &mut tick, 30);
    let mut peaks = Vec::new();
    for ev in SHARED_SFX {
        assert!(known.has(ev), "no event {ev}");
        let post = Call::Post { ev: (*ev).to_owned(), owner: Some(-1), key: None, stop_key: None, at: None };
        call(&mut a, &Line { tick, call: post });
        peaks.push(mix(&mut a, &mut tick, 40));
    }
    peaks
}

/// Two of the match scope's shared sounds whose every pick goes into the buses mix_pause_on mutes (a random pick
/// of sfx_shared_combat_temp_hit has a layer outside them).
const SHARED_SFX: &[&str] = &["sfx_shared_combat_attack_charge_startup", "sfx_shared_item_generic_pickup"];

#[test]
fn a_left_paused_lets_the_pause_mix_go() {
    let Ok(store) = mvs_store::Store::open(mvs_store::dev_store_dir()) else {
        eprintln!("note: no store; nothing checked");
        return;
    };
    let init = mvs_data::sound::load_init(&store).expect("the InitBank");
    let known = Index::new(init.events.into_keys());
    let store = Arc::new(store);
    let Ok(trace) = mvs_trace::Trace::read(&mvs_sim_trace::run::default_traces().join(format!("{TRACE}.trace"))) else {
        eprintln!("note: no trace {TRACE}; nothing checked");
        return;
    };
    let (w, _) = mvs_sim_trace::world_for(&trace.header, &store).expect("the World of the header");
    let kept = session(&store, &known, &w, false);
    let let_go = session(&store, &known, &w, true);
    eprintln!("the next match's effects' peaks: {kept:?} with the pause mix kept, {let_go:?} with it let go");
    for ((ev, k), l) in SHARED_SFX.iter().zip(&kept).zip(&let_go) {
        // (muted: 20 dB down or more; what is left is a layer or a send outside the muted buses)
        assert!(*k < *l / 10.0, "{ev}: the pause mix's mute let it through (peak {k}, {l} let go)");
        assert!(*l > 1e-2, "{ev}: silent after the pause mix went (peak {l})");
    }
}
