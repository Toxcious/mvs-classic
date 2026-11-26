//! The match's sound scopes against the oracle's post tapes: for each match of
//! `audio_tapes.json` whose tape exists, the scope lines `MatchAudio` begins with (stage, match, podium, ringout,
//! tutorial, items) equal the oracle's by name, event list (as a set and in order) and held-as-files list.
//!
//! The tapes are read from `MVS_AUDIO_TAPES` (default: `<main>/.scratch/audio-tapes`); a match without a tape or a
//! trace is skipped after a printed line.

use std::path::PathBuf;

use mvs_app::audio::{BeginOpts, Index, MatchAudio, podium_montages};
use mvs_audio::tape::post::{Call, Line};

/// Buff_CatchUpSpeedBuff's start sound (its class default), in every match scope since Catch Up is ported.
const CATCH_UP_START: &str = "sfx_shared_combat_speed_buff_start";

/// The default OnHitSound of an AnimNotify_Hitframe (its class default), named by every Hitframe whose notify sets
/// none.
const HITFRAME_DEFAULT_SOUND: &str = "sfx_shared_combat_temp_hit";

fn dirs() -> Vec<PathBuf> {
    if let Some(d) = std::env::var_os("MVS_AUDIO_TAPES") {
        return vec![PathBuf::from(d)];
    }
    let traces = mvs_sim_trace::run::default_traces();
    vec![traces.join("../audio-tapes")]
}

type Scope = (String, Vec<String>, Vec<String>);

/// The tape's scope lines of the match's begin (the match's preload to the first input's resume, the last line of
/// each name: a scope whose load ends before the stage's is set before the music's), the menu's, the base's and the
/// title's left out.
fn tape_scopes(tape: &[Line]) -> Option<Vec<Scope>> {
    let music = tape.iter().position(|l| matches!(l.call, Call::ScopeMusic { .. }))?;
    let resume = music + tape[music..].iter().position(|l| matches!(l.call, Call::Resume))?;
    let preload =
        tape[..music].iter().rposition(|l| matches!(&l.call, Call::Scope { name, .. } if name == "preload"))?;
    let mut out: Vec<Scope> = Vec::new();
    for l in &tape[preload..=resume] {
        if let Call::Scope { name, keep, lazy } = &l.call
            && !["menu", "base", "preload"].contains(&name.as_str())
        {
            out.retain(|s| &s.0 != name);
            out.push((name.clone(), keep.clone(), lazy.clone()));
        }
    }
    Some(out)
}

fn sorted(v: &[String]) -> Vec<String> {
    let mut v = v.to_vec();
    v.sort();
    v
}

#[test]
fn scope_lists_equal_the_page_tapes() {
    let names: Vec<String> = serde_json::from_str(
        &std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/audio_tapes.json")).unwrap(),
    )
    .unwrap();
    let Ok(store) = mvs_store::Store::open(mvs_store::dev_store_dir()) else {
        eprintln!("note: no dev store; nothing checked");
        return;
    };
    let Ok(init) = mvs_data::sound::load_init(&store) else {
        eprintln!("note: no InitBank; nothing checked");
        return;
    };
    let index = Index::new(init.events.into_keys());
    let podium = podium_montages(&store);
    let (mut checked, mut order_differs, mut bad) = (0, 0, 0);
    for name in &names {
        let file = format!("{name}.tape");
        let Some(path) = dirs().into_iter().map(|d| d.join(&file)).find(|p| p.exists()) else {
            eprintln!("note: no tape {file}; skipped");
            continue;
        };
        let trace_path = mvs_sim_trace::run::default_traces().join(format!("{name}.trace"));
        let (Ok(tape), Ok(trace)) = (mvs_audio::tape::read::<Line>(&path), mvs_trace::Trace::read(&trace_path)) else {
            eprintln!("note: {name}: the tape or its trace does not read; skipped");
            continue;
        };
        let Some(want) = tape_scopes(&tape) else {
            eprintln!("note: {name}: no match begin in the tape; skipped");
            continue;
        };
        let (mut w, _) = mvs_sim_trace::world_for(&trace.header, &store).expect("the World of the header");
        // (a match that is not the Lab's holds mus_victory: the oracle's flow, not the trace header's, decides)
        let lab = !tape
            .iter()
            .any(|l| matches!(&l.call, Call::ScopeMusic { values, .. } if values.iter().any(|v| v == "mus_victory")));
        std::sync::Arc::make_mut(&mut w.setup).flow = !lab;
        let scenery_events = mvs_render::scenery::sound_events(&store, &w.data.stage_key).unwrap_or_default();
        let mut got = Vec::new();
        let _ = MatchAudio::with_opts(
            &w,
            index.clone(),
            &mut got,
            BeginOpts {
                scenery_events,
                music_skin: Some("mus_base".into()),
                podium: Some(podium.clone()),
                ..BeginOpts::default()
            },
        );
        // (the Chaos Wheel's and Rising Lava's scopes are the port's own: the oracle has none)
        let got: Vec<Scope> = got
            .iter()
            .filter_map(|l| match &l.call {
                Call::Scope { name, keep, lazy }
                    if !["menu", "base", "preload", "wheel", "lava"].contains(&name.as_str()) =>
                {
                    Some((name.clone(), keep.clone(), lazy.clone()))
                }
                _ => None,
            })
            .collect();
        checked += 1;
        // (the oracle's scopes are set as each load ends: the order of the names is its timing's, so by name)
        let names_of = |v: &[Scope]| {
            let mut n = v.iter().map(|s| s.0.clone()).collect::<Vec<_>>();
            n.sort();
            n
        };
        if names_of(&got) != names_of(&want) {
            bad += 1;
            eprintln!("{name}: scope names differ: glue {:?}, tape {:?}", names_of(&got), names_of(&want));
            continue;
        }
        for g in &got {
            let t = want.iter().find(|t| t.0 == g.0).expect("a scope of the tape");
            let (mut gk, tk) = (g.1.clone(), t.1.clone());
            // Deviation: the port has Catch Up (Buff_CatchUpSpeedBuff, every player's Comp_State_Manager) and the
            // oracle has none, so the match scope keeps its start sound twice more than the oracle's tapes.
            if g.0 == "match" && !tk.iter().any(|e| e == CATCH_UP_START) {
                gk.retain(|e| e != CATCH_UP_START);
            }
            // Deviation: the port's gamedata keeps the hit-react clips' sequence Hitframes (B11 T1, 52af4ed80:
            // gamedata bind version 8: the oracle left them out), each with the class default OnHitSound, so a fighter data
            // names the Hitframe default sound earlier and in more fighters lists than the oracle's tapes. Left out
            // of both sides (the sound is held in the match scope all the same).
            gk.retain(|e| e != HITFRAME_DEFAULT_SOUND);
            let tk: Vec<String> = tk.into_iter().filter(|e| e != HITFRAME_DEFAULT_SOUND).collect();
            let only = |a: &[String], b: &[String]| a.iter().filter(|e| !b.contains(e)).cloned().collect::<Vec<_>>();
            if sorted(&gk) != sorted(&tk) || sorted(&g.2) != sorted(&t.2) {
                bad += 1;
                eprintln!(
                    "{name}: scope {}: keep {}/{} lazy {}/{}; only glue: keep {:?} lazy {:?}; only tape: keep {:?} lazy {:?}",
                    g.0,
                    gk.len(),
                    tk.len(),
                    g.2.len(),
                    t.2.len(),
                    only(&gk, &tk),
                    only(&g.2, &t.2),
                    only(&tk, &gk),
                    only(&t.2, &g.2)
                );
            } else if gk != tk || g.2 != t.2 {
                // Deviation: a stage's scenery block data is read as serde_json maps (sorted keys, not the file's
                // order), so the stage scope's events can come in another order: the same media, loaded in another
                // order. Printed, not counted.
                if g.0 != "stage" {
                    order_differs += 1;
                }
                let i = gk.iter().zip(&tk).position(|(a, b)| a != b).unwrap_or(0);
                eprintln!(
                    "{name}: scope {}: the same events, in another order; first at {i}: glue {:?}, tape {:?}",
                    g.0,
                    &gk[i..(i + 3).min(gk.len())],
                    &tk[i..(i + 3).min(tk.len())]
                );
            }
        }
    }
    eprintln!("scope lists: {checked} matches checked, {bad} scopes differ, {order_differs} scopes in another order");
    assert_eq!(bad, 0, "scope lists differ from the oracle's tapes");
    assert_eq!(order_differs, 0, "scope lists in another order than the oracle's");
}

/// The match scope holds the events of a buff a perk gives (a port-only addition: the oracle's scope reads no perk
/// tables): bugs-bugs's Perk_BugsBunny_LingeringLove gives Buff_ScoobySnack, whose loop the sim posts.
#[test]
fn match_scope_holds_perk_buff_events() {
    let Ok(store) = mvs_store::Store::open(mvs_store::dev_store_dir()) else {
        eprintln!("note: no dev store; nothing checked");
        return;
    };
    let Ok(init) = mvs_data::sound::load_init(&store) else {
        eprintln!("note: no InitBank; nothing checked");
        return;
    };
    let trace_path = mvs_sim_trace::run::default_traces().join("bugs-bugs.trace");
    let Ok(trace) = mvs_trace::Trace::read(&trace_path) else {
        eprintln!("note: no trace {}; nothing checked", trace_path.display());
        return;
    };
    let (w, _) = mvs_sim_trace::world_for(&trace.header, &store).expect("the World of the header");
    let mut got = Vec::new();
    let _ = MatchAudio::with_opts(&w, Index::new(init.events.into_keys()), &mut got, BeginOpts::default());
    let keep: Vec<&String> = got
        .iter()
        .filter_map(|l| match &l.call {
            Call::Scope { name, keep, .. } if name == "match" => Some(keep),
            _ => None,
        })
        .flatten()
        .collect();
    for e in ["sfx_cha_shaggy_rage_ally_loop", "sfx_cha_shaggy_rage_ally_loop_stop", "sfx_cha_shaggy_rage_ally_end"] {
        assert!(keep.iter().any(|k| *k == e), "{e} in the match scope");
    }
}
