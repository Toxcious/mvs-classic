//! The exe's post tape (`mvs --replay <match> --audio-tape <file>`) against the oracle's, per match.
//!
//! The oracle's tape holds the title screen's and the menus' calls first, then the
//! match's. The exe's tape starts at the match's begin, so the oracle's side is taken from the first line after the
//! last `scope` call named `items` at tick 0 (the oracle's match scopes end with it: stage, match, podium, ringout,
//! tutorial, items; the match's own first call follows) -- the lines before it are skipped and counted per call.
//! The exe side starts at its own `scope` lines for the match, so both sides drop their leading scope /
//! preload / release / music-scope calls (the scope contents are checked by the lists' own tests) and compare what
//! follows: the posts, the rtpc / switch / state calls, the emitters, the listener, the stops. The oracle's frames are
//! fewer than the exe's: [`align`] compares what each side made on a frame at the oracle's drawn ticks.
//!
//! Parsed lines are compared, numbers within 1e-3, null equal to null. Differences are printed per match (the first
//! ones and the count per call kind); a match fails only when named in `MVS_EXE_TAPES_STRICT` (comma separated).
//! Matches: `MVS_EXE_TAPES_MATCHES` (comma separated; `all`: every `stage-*` and `ffa-items-*` of
//! `audio_tapes.json`); unset, the test runs none (each match is an exe run of minutes: name one per kind). Page tapes
//! are read from `MVS_AUDIO_TAPES` (default the main tree's `.scratch/audio-tapes`). The exe
//! runs a `--replay` window on the dev store, so a run without a store, a tape or a window says so and passes.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::process::Command;

use indexmap::IndexMap;
use mvs_audio::tape::post::{Call, Line};
use serde_json::Value;

fn tapes() -> PathBuf {
    std::env::var_os("MVS_AUDIO_TAPES")
        .map_or_else(|| mvs_sim_trace::run::default_traces().join("../audio-tapes"), PathBuf::from)
}

fn matches() -> Vec<String> {
    let Ok(list) = std::env::var("MVS_EXE_TAPES_MATCHES") else {
        eprintln!("note: MVS_EXE_TAPES_MATCHES not set; no exe run (name the matches, or `all`)");
        return Vec::new();
    };
    if list.trim() != "all" {
        return list.split(',').map(|n| n.trim().to_owned()).filter(|n| !n.is_empty()).collect();
    }
    let text = include_str!("audio_tapes.json");
    let all: Vec<String> = serde_json::from_str(text).expect("audio_tapes.json");
    all.into_iter().filter(|m| m.starts_with("stage-") || m.starts_with("ffa-items-") || m == "garnet-combat").collect()
}

/// The calls a side's scope setup makes: not compared (the exe's scopes are built elsewhere and tested there).
fn setup(l: &Line) -> bool {
    matches!(
        l.call,
        Call::Scope { .. }
            | Call::ScopeMusic { .. }
            | Call::PreloadMusic { .. }
            | Call::ReleaseMusic { .. }
            | Call::Resume
    )
}

/// The oracle's lines of the match: after the last `items` scope of the tape's scope block, setup calls dropped.
/// Returns (the match's lines, the skipped lines' count per call name).
fn page_match(tape: &[Line]) -> (Vec<Line>, Vec<(String, usize)>) {
    let start =
        tape.iter().rposition(|l| matches!(&l.call, Call::Scope { name, .. } if name == "items")).map_or(0, |i| i + 1);
    let mut skipped: Vec<(String, usize)> = Vec::new();
    for l in &tape[..start] {
        let name = call_name(l);
        match skipped.iter_mut().find(|(n, _)| *n == name) {
            Some((_, c)) => *c += 1,
            None => skipped.push((name, 1)),
        }
    }
    (tape[start..].iter().filter(|l| !setup(l)).cloned().collect(), skipped)
}

fn call_name(l: &Line) -> String {
    let v = serde_json::to_value(l).expect("a line is a value");
    v["call"].as_str().unwrap_or("?").to_owned()
}

/// Equal as JSON, numbers within `tol`.
fn same(a: &Value, b: &Value, tol: f64) -> bool {
    match (a, b) {
        (Value::Number(x), Value::Number(y)) => match (x.as_f64(), y.as_f64()) {
            (Some(x), Some(y)) => (x - y).abs() <= tol,
            _ => x == y,
        },
        (Value::Array(x), Value::Array(y)) => x.len() == y.len() && x.iter().zip(y).all(|(p, q)| same(p, q, tol)),
        (Value::Object(x), Value::Object(y)) => {
            x.len() == y.len() && x.iter().all(|(k, p)| y.get(k).is_some_and(|q| same(p, q, tol)))
        }
        _ => a == b,
    }
}

/// The HUD's posts (UI_Game's PF_Post_Global_Event, owner -1): made on a drawn frame, not in a tick (audio_tape.rs's
/// list).
const HUD: [&str; 5] = ["ui_countdown", "ui_stinger_", "ui_match_complete", "ui_postmatch", "ui_ability_ready"];

fn is_hud(l: &Line) -> bool {
    matches!(&l.call, Call::Post { ev, owner: Some(-1), .. } if HUD.iter().any(|p| ev.starts_with(p)))
}

/// The two sides made comparable. The oracle's trace-mode run draws one frame per run of ticks with equal inputs
/// (the exe one per tick), and both send the listener once per drawn frame.
/// So:
/// - the listener is compared at the oracle's drawn ticks only (the last of a tick on each side);
/// - a HUD post (made on a frame, its card or anim timed on the clock) is moved on the exe side to the oracle's first
///   drawn tick at or after its own: the oracle made it on that frame;
/// - the exe side ends with its frame of the oracle tape's last tick: the recorder never leaves the match, so the
///   exe's leave (its stopAll) has no page line.
///
/// Returns the two sides and the drawn ticks whose listener differs by the camera shake alone (see [`shaken`]),
/// left out of the compare.
fn align(page: Vec<Line>, exe: Vec<Line>) -> (Vec<Line>, Vec<Line>, Vec<u32>) {
    let listener = |l: &Line| matches!(l.call, Call::SetListener { .. });
    let drawn: BTreeSet<u32> = page.iter().filter(|l| listener(l)).map(|l| l.tick).collect();
    let last = page.iter().map(|l| l.tick).max().unwrap_or(0);
    let end = exe.iter().rposition(|l| listener(l) && l.tick <= last).map_or(exe.len(), |i| i + 1);
    let per_tick = |side: &[Line]| -> BTreeMap<u32, Line> {
        side.iter().filter(|l| listener(l) && drawn.contains(&l.tick)).map(|l| (l.tick, l.clone())).collect()
    };
    let (pl, el) = (per_tick(&page), per_tick(&exe[..end]));
    let mut page_out: Vec<Line> = page.into_iter().filter(|l| !listener(l)).collect();
    let mut exe_out: Vec<Line> = exe
        .into_iter()
        .take(end)
        .filter(|l| !listener(l))
        .map(|mut l| {
            if is_hud(&l)
                && let Some(&t) = drawn.range(l.tick..).next()
            {
                l.tick = t;
            }
            l
        })
        .collect();
    let mut shake = Vec::new();
    for (t, p) in pl {
        match el.get(&t) {
            Some(e) if shaken(&p, e) => shake.push(t),
            Some(e) => {
                page_out.push(p);
                exe_out.push(e.clone());
            }
            None => page_out.push(p),
        }
    }
    (page_out, exe_out, shake)
}

/// Two listeners apart only along the camera's forward and up axes (UE X / Z; Y and the axes equal): a camera shake's
/// random initial offset, which the oracle draws from Math.random and the exe from its CRT stream (the ledger's
/// "Camera shake random offset" row, crates/mvs-view/src/camera.rs CrtRand).
fn shaken(p: &Line, e: &Line) -> bool {
    let (
        Call::SetListener { pos: pp, right: pr, up: pu, front: pf },
        Call::SetListener { pos: ep, right: er, up: eu, front: ef },
    ) = (&p.call, &e.call)
    else {
        return false;
    };
    let near = |a: f64, b: f64| (a - b).abs() <= 1e-3;
    let eq = |a: &[f64; 3], b: &[f64; 3]| a.iter().zip(b).all(|(x, y)| near(*x, *y));
    !eq(pp, ep) && near(pp[1], ep[1]) && eq(pr, er) && eq(pu, eu) && eq(pf, ef)
}

/// A side's lines as values, per call kind (in order).
fn by_kind(lines: &[Line]) -> IndexMap<String, Vec<Value>> {
    let mut m: IndexMap<String, Vec<Value>> = IndexMap::new();
    for l in lines {
        let v = serde_json::to_value(l).expect("a line is a value");
        let kind = v["call"].as_str().unwrap_or("?").to_owned();
        m.entry(kind).or_default().push(v);
    }
    // (the order within a tick is not compared)
    for v in m.values_mut() {
        v.sort_by_cached_key(|x| (x["tick"].as_u64().unwrap_or(0), x.to_string()));
    }
    m
}

/// The differences of the two sides per call kind: (total, the report lines).
fn compare(page: &[Line], exe: &[Line]) -> (usize, Vec<String>) {
    let (pm, em) = (by_kind(page), by_kind(exe));
    let mut kinds: Vec<&String> = pm.keys().chain(em.keys()).collect();
    kinds.sort();
    kinds.dedup();
    let mut total = 0;
    let mut report = Vec::new();
    for k in kinds {
        let (p, e) = (pm.get(k).map_or(&[][..], Vec::as_slice), em.get(k).map_or(&[][..], Vec::as_slice));
        let mut n = 0;
        let mut first = Vec::new();
        for i in 0..p.len().max(e.len()) {
            if !matches!((p.get(i), e.get(i)), (Some(x), Some(y)) if same(x, y, 1e-3)) {
                n += 1;
                if first.len() < 3 {
                    first.push(format!(
                        "    #{i}: page {} | exe {}",
                        short(&p.get(i).cloned()),
                        short(&e.get(i).cloned())
                    ));
                }
            }
        }
        total += n;
        report.push(format!("  {k}: page {} exe {} differ {n}", p.len(), e.len()));
        report.extend(first);
    }
    (total, report)
}

fn short(v: &Option<Value>) -> String {
    let s = v.as_ref().map_or_else(|| "(none)".to_owned(), Value::to_string);
    s.chars().take(220).collect()
}

fn run_exe(name: &str, store: &Path, out: &Path) -> Result<(), String> {
    let status = Command::new(env!("CARGO_BIN_EXE_mvs"))
        .args(["--replay", name, "--audio-tape"])
        .arg(out)
        .arg("--store")
        .arg(store)
        .status()
        .map_err(|e| format!("cannot start the exe: {e}"))?;
    if status.success() { Ok(()) } else { Err(format!("the exe ended with {status}")) }
}

#[test]
fn exe_tapes_match_the_page() {
    let store = mvs_store::dev_store_dir();
    if !store.exists() {
        eprintln!("note: no store {}; nothing checked", store.display());
        return;
    }
    let strict: Vec<String> = std::env::var("MVS_EXE_TAPES_STRICT")
        .map(|s| s.split(',').map(|n| n.trim().to_owned()).filter(|n| !n.is_empty()).collect())
        .unwrap_or_default();
    let dir = tapes();
    let out_dir = std::env::temp_dir().join(format!("mvs-exe-audio-tape-{}", std::process::id()));
    std::fs::create_dir_all(&out_dir).expect("the scratch folder");
    let mut failures = Vec::new();
    let mut ran = 0;
    for name in matches() {
        let page_path = dir.join(format!("{name}.tape"));
        let Ok(page) = mvs_audio::tape::read::<Line>(&page_path) else {
            eprintln!("{name}: no page tape {}; skipped", page_path.display());
            continue;
        };
        let out = out_dir.join(format!("{name}.tape"));
        let reuse = std::env::var_os("MVS_EXE_TAPES_REUSE").map(PathBuf::from).map(|d| d.join(format!("{name}.tape")));
        let out = reuse.filter(|p| p.exists()).unwrap_or(out);
        if let Err(e) = if std::env::var_os("MVS_EXE_TAPES_REUSE").is_some() && out.exists() {
            Ok(())
        } else {
            run_exe(&name, &store, &out)
        } {
            eprintln!("{name}: {e}");
            if strict.contains(&name) {
                failures.push(name);
            }
            continue;
        }
        ran += 1;
        let exe: Vec<Line> = match mvs_audio::tape::read(&out) {
            Ok(l) => l,
            Err(e) => {
                eprintln!("{name}: the exe's tape does not parse: {e}");
                failures.push(name);
                continue;
            }
        };
        let (page, skipped) = page_match(&page);
        let (exe, _) = page_match(&exe);
        let (page, exe, shake) = align(page, exe);
        let (n, report) = compare(&page, &exe);
        eprintln!(
            "{name}: page {} lines, exe {} lines, {n} differ; page lines skipped before the match: {skipped:?}",
            page.len(),
            exe.len()
        );
        eprintln!(
            "  listener at {} drawn ticks left out: the camera shake's random offset alone (first {:?})",
            shake.len(),
            &shake[..shake.len().min(5)]
        );
        for f in &report {
            eprintln!("{f}");
        }
        if n > 0 && strict.contains(&name) {
            failures.push(name);
        }
    }
    let _ = std::fs::remove_dir_all(&out_dir);
    eprintln!("exe audio tapes: {ran} matches compared");
    assert!(failures.is_empty(), "strict matches differ: {failures:?}");
}
