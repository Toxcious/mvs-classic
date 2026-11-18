//! The match's sound glue against the post tapes of the oracle: each match
//! of `audio_tapes.json` whose tape exists has its World events fed to `MatchAudio`, its calls compared with the
//! oracle's, line by line and as counts.
//!
//! `MVS_AUDIO_TAPES` names the directory of the tapes (`<match>.tape`; `hud-run` is `hud.tape`, `lab-picture` is
//! `lab.tape`); the default is the main tree's `.scratch/audio-tapes`. A match without a tape is listed, not failed.
//! Every difference of every match is collected and printed in a summary table (run with `--nocapture`);
//! `MVS_AUDIO_TAPES_ONLY` (comma names) limits the run to those matches. The test fails only for a match named in
//! `MVS_AUDIO_TAPES_STRICT` (comma-separated), so a track turns a match strict when it has fixed it.
//! The tapes need the oracle-format store's InitBank (`CONTENT_STORE`): without one the run prints NOTHING CHECKED and
//! passes (other trees and fresh clones have no store).
//! The begin takes the oracle's flow (the Lab's or not: `mus_victory` in its music scope) and drawn music skin from
//! the tape's music scope, the World that steps keeping the trace header's.
//! What the glue cannot derive without a part not in this crate is fed from the tape or exempted:
//! - the HUD's posts (the countdown, the match point stinger, the results' cues, the pop texts' sounds) are the HUD's: given to
//!   `hud_sounds` as the tape has them;
//! - the listener's point of view is the camera's: given to `frame` as the tape has it;
//! - the podium scope's victory and defeat montage one-shots (the menuscene kind's podium entry, not read from the
//!   oracle-format store; `scope_lists` checks them on the dev store): the scope is compared by its first entry;
//! - a hit's place on the victim's nearest bone is the drawn pose's: no pose here, the hit's own place stands, and
//!   a hit sound that differs from the tape's in that place alone is counted and let pass.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::PathBuf;

use indexmap::IndexMap;
use mvs_app::audio::{BeginOpts, Index, MatchAudio, NoViews, Views};
use mvs_audio::tape::post::{Call, Line};
use mvs_sim::events::EndPhase;
use mvs_sim::{Event, World};
use mvs_view::skin_intro::IntroMute;
use mvs_view::skins::skin_of;
use serde::Deserialize;
use serde::de::IgnoredAny;

type V3 = [f64; 3];

#[derive(Deserialize)]
struct Bank {
    events: IndexMap<String, IgnoredAny>,
}

fn tapes() -> PathBuf {
    std::env::var_os("MVS_AUDIO_TAPES")
        .map_or_else(|| mvs_sim_trace::run::default_traces().join("../audio-tapes"), PathBuf::from)
}

/// The oracle's event index (the dev store holds the oracle's InitBank as JSON).
fn event_index() -> Option<Index> {
    let store = mvs_store::Store::open(mvs_store::dev_store_dir()).ok()?;
    let bytes = store.kind("soundbank").ok()?.bytes("/Game/WwiseAudio/InitBank.InitBank").ok()?;
    let bank: Bank = serde_json::from_slice(&bytes).ok()?;
    Some(Index::new(bank.events.into_keys()))
}

/// The views of a run without a drawn pose: a hit's bone is its own place, noted; a skin's own intro mutes the
/// default intro's sounds as the effects' copy does (mvs-render fx.rs start).
#[derive(Default)]
struct Rec {
    bones: std::cell::RefCell<Vec<V3>>,
    intros: HashMap<i32, IntroMute>,
}

impl Rec {
    fn new(w: &World) -> Rec {
        let mut intros = HashMap::new();
        for fid in w.all_ids() {
            let id = w.fighter(fid).id;
            let d = &w.char_data(fid).fighter;
            let pick = usize::try_from(id).ok().and_then(|i| w.setup.players.get(i)).and_then(|p| p.skin.as_deref());
            if let Some(m) = skin_of(d, pick).ok().as_ref().and_then(|s| IntroMute::of(d, s)) {
                intros.insert(id, m);
            }
        }
        Rec { intros, ..Rec::default() }
    }
}

impl Views for Rec {
    fn nearest_bone(&self, _victim: i32, hit: V3) -> V3 {
        self.bones.borrow_mut().push(hit);
        hit
    }
    fn ko_sound(&mut self, world: &World, asset: &str) -> Option<(String, String)> {
        NoViews.ko_sound(world, asset)
    }
    fn muted_by_intro(&self, world: &World, e: &Event) -> bool {
        let id = match *e {
            Event::Sound { fighter, .. } | Event::SoundStart { fighter, .. } => fighter,
            _ => return false,
        };
        let Some(m) = self.intros.get(&id) else { return false };
        world.by_id(id).is_some_and(|f| m.muted(world, f, e))
    }
}

/// The HUD's posts on the game state, by prefix: UI_Game's (countdown, stinger, results' cues, ability ready) and
/// UI_PopText Show's (the item and backstab pops' sounds).
const HUD: [&str; 8] = [
    "ui_countdown",
    "ui_stinger_",
    "ui_match_complete",
    "ui_postmatch",
    "ui_ability_ready",
    "sfx_cha_finn_backpack_treasure_collect",
    "sfx_misc_silence_onesecond",
    "sfx_cha_arya_backstab_hit",
];

fn is_hud(l: &Line) -> bool {
    matches!(&l.call, Call::Post { ev, owner: Some(-1), .. } if HUD.iter().any(|p| ev.starts_with(p)))
}

/// What the tape holds of the match: its begin calls, its begin's scopes and the run from the first input to the
/// leave.
struct Expected {
    reverb: Line,
    /// The begin's calls but its scopes, from the music's scope to the first input's resume.
    begin: Vec<Line>,
    /// The match's scopes by name (the match's preload to the resume, the last of each name; the menu's, the base's
    /// and the title's left out): the oracle sets each as its load ends, so a scope whose load ends before the stage's
    /// comes before the music, and their order is its timing's.
    scopes: Vec<Line>,
    run: Vec<Line>,
}

const NOT_MATCH_SCOPES: [&str; 3] = ["menu", "base", "preload"];

fn is_scope(l: &Line) -> bool {
    matches!(l.call, Call::Scope { .. })
}

/// Scope lines sorted by name.
fn by_name(mut v: Vec<Line>) -> Vec<Line> {
    v.sort_by_key(|l| match &l.call {
        Call::Scope { name, .. } => name.clone(),
        _ => String::new(),
    });
    v
}

fn expected(tape: &[Line]) -> Result<Expected, String> {
    let music =
        tape.iter().position(|l| matches!(l.call, Call::ScopeMusic { .. })).ok_or("the tape has no scopeMusic")?;
    let resume = music
        + tape[music..]
            .iter()
            .position(|l| matches!(l.call, Call::Resume))
            .ok_or("the tape has no resume after the music")?;
    let reverb = tape[..music]
        .iter()
        .rev()
        .find(|l| matches!(l.call, Call::SetReverbVolumes { .. }))
        .ok_or("the tape has no reverb volumes before the music")?
        .clone();
    let begin = tape[music..=resume].iter().filter(|l| !is_scope(l)).cloned().collect();
    let preload = tape[..music]
        .iter()
        .rposition(|l| matches!(&l.call, Call::Scope { name, .. } if name == "preload"))
        .unwrap_or(music);
    let mut scopes: Vec<Line> = Vec::new();
    for l in &tape[preload..=resume] {
        if let Call::Scope { name, .. } = &l.call
            && !NOT_MATCH_SCOPES.contains(&name.as_str())
        {
            scopes.retain(|s| !matches!(&s.call, Call::Scope { name: n, .. } if n == name));
            scopes.push(l.clone());
        }
    }
    let mut run = Vec::new();
    let mut listener0 = false;
    for l in &tape[resume + 1..] {
        // (a match start draws a few frames on the first tick: one listener of them)
        if l.tick == 0 && matches!(l.call, Call::SetListener { .. }) {
            if listener0 {
                continue;
            }
            listener0 = true;
        }
        let stop = matches!(l.call, Call::StopAll { .. });
        run.push(l.clone());
        if stop {
            break;
        }
    }
    Ok(Expected { reverb, begin, scopes: by_name(scopes), run })
}

/// A line in the form the comparison takes: scopes sorted, the podium scope by its first entry.
fn norm(l: &Line) -> Line {
    let mut l = l.clone();
    if let Call::Scope { name, keep, lazy } = &mut l.call {
        if name == "podium" {
            keep.truncate(1);
        }
        keep.sort();
        lazy.sort();
    }
    l
}

/// The kind of a call as the tape names it ("post", "setRtpc", ...), a post by its event.
fn kind(l: &Line) -> String {
    let v = serde_json::to_value(&l.call).unwrap_or_default();
    let k = v["call"].as_str().unwrap_or("?").to_string();
    match &l.call {
        Call::Post { ev, .. } => format!("{k} {ev}"),
        _ => k,
    }
}

fn call_kind(l: &Line) -> String {
    kind(l).split(' ').next().unwrap_or("?").to_string()
}

/// The multiset key of a line: tick and call, a post's place left out (the pose's bones are not here).
fn key(l: &Line) -> String {
    let mut l = norm(l);
    if let Call::Post { at, .. } = &mut l.call {
        *at = None;
    }
    serde_json::to_string(&l).unwrap_or_else(|_| format!("{l:?}"))
}

/// One match's differences.
#[derive(Default)]
struct Report {
    /// Why nothing was compared.
    skipped: Option<String>,
    glue_lines: usize,
    /// The listener lines of the run, apart.
    glue_listeners: usize,
    tape_listeners: usize,
    tape_lines: usize,
    equal: usize,
    bone_exempt: usize,
    hud_fed: usize,
    listeners: usize,
    /// The first differing line, positionally: where, the glue's, the tape's.
    first: Option<String>,
    /// The first positional difference of each call kind.
    examples: BTreeMap<String, String>,
    /// Positional differences per call kind.
    positional: BTreeMap<String, usize>,
    /// Lines only the glue has / only the tape has, per call kind (post by event): shift-proof.
    only_glue: BTreeMap<String, usize>,
    only_tape: BTreeMap<String, usize>,
}

impl Report {
    fn differs(&self) -> bool {
        self.skipped.is_none()
            && (self.glue_lines != self.tape_lines
                || !self.positional.is_empty()
                || !self.only_glue.is_empty()
                || !self.only_tape.is_empty())
    }
}

fn tape_file(name: &str) -> &str {
    match name {
        "hud-run" => "hud.tape",
        "lab-picture" => "lab.tape",
        _ => "",
    }
}

fn find_tape(name: &str) -> Option<PathBuf> {
    let dir = tapes();
    let own = dir.join(format!("{name}.tape"));
    if own.exists() {
        return Some(own);
    }
    let alias = tape_file(name);
    (!alias.is_empty()).then(|| dir.join(alias)).filter(|p| p.exists())
}

fn skip(why: impl Into<String>) -> Report {
    Report { skipped: Some(why.into()), ..Report::default() }
}

fn check(name: &str, path: &std::path::Path, index: Index) -> Report {
    let Ok(tape) = mvs_audio::tape::read::<Line>(path) else { return skip("tape unreadable") };
    let trace_path = mvs_sim_trace::run::default_traces().join(format!("{name}.trace"));
    let Ok(trace) = mvs_trace::Trace::read(&trace_path) else { return skip("no trace") };
    let store = mvs_store::Store::open(mvs_store::dev_store_dir()).expect("the dev store");
    let (mut w, book) = match mvs_sim_trace::world_for(&trace.header, &store) {
        Ok(x) => x,
        Err(e) => return skip(format!("world_for: {e:?}")),
    };
    let want = match expected(&tape) {
        Ok(x) => x,
        Err(e) => return skip(e),
    };
    // The oracle's begin: its flow (a match that is not the Lab's holds mus_victory: the oracle's `lab`, not the trace
    // header's flow), its drawn music skin, the scenery's named events. The World that steps keeps the header's.
    let Some(Call::ScopeMusic { values, .. }) = want.begin.first().map(|l| &l.call) else {
        return skip("the tape's begin has no scopeMusic first");
    };
    let lab = !values.iter().any(|v| v == "mus_victory");
    let music_skin = values.get(1).cloned();
    let mut begin_world = w.clone();
    std::sync::Arc::make_mut(&mut begin_world.setup).flow = !lab;
    let scenery_events = mvs_render::scenery::sound_events(&store, &w.data.stage_key).unwrap_or_default();
    let mut got: Vec<Line> = Vec::new();
    let mut m = MatchAudio::with_opts(
        &begin_world,
        index,
        &mut got,
        BeginOpts { scenery_events, music_skin, ..BeginOpts::default() },
    );
    drop(begin_world);
    let begin_len = got.len();
    // (the begin as compared: its calls but its scopes in order, then its scopes by name)
    let got_begin: Vec<Line> = got
        .iter()
        .filter(|l| !is_scope(l))
        .cloned()
        .chain(by_name(got.iter().filter(|l| is_scope(l)).cloned().collect()))
        .collect();
    // the tape's begin: reverb volumes first, then the music's scope to the resume
    let want_begin: Vec<Line> = std::iter::once(want.reverb.clone())
        .chain(want.begin.iter().cloned())
        .chain(want.scopes.iter().cloned())
        .collect();

    // the run: a step's tick calls, then the frame's, then the HUD's
    let mut hud: Vec<&Line> = want.run.iter().filter(|l| is_hud(l)).collect();
    hud.reverse();
    let listeners: Vec<&Line> = want.run.iter().filter(|l| matches!(l.call, Call::SetListener { .. })).collect();
    if listeners.is_empty() {
        return skip("the tape's run has no listener");
    }
    let mut rec = Rec::new(&w);
    let mut hud_fed = 0;
    let mut frames = 0;
    // (the oracle writes a listener only when the point of view changes: a frame takes the latest of the tape's
    // listeners at or before its tick)
    let mut next = 0;
    let mut frame = |m: &mut MatchAudio, got: &mut Vec<Line>, tick: i32| {
        while next < listeners.len() && i64::from(listeners[next].tick) <= i64::from(tick) {
            next += 1;
        }
        let Call::SetListener { pos, right, up, front } = listeners[next.saturating_sub(1)].call else {
            unreachable!()
        };
        m.frame(tick, &mvs_view::camera::Pov { pos, right, up, front }, &mut NoViews, got);
        frames += 1;
        let mut ev = Vec::new();
        while hud.last().is_some_and(|l| i64::from(l.tick) == i64::from(tick)) {
            if let Call::Post { ev: e, .. } = &hud.pop().unwrap().call {
                ev.push(e.clone());
            }
        }
        hud_fed += ev.len();
        m.hud_sounds(tick, ev, got);
    };
    // (the match's first frame is drawn before its first step)
    frame(&mut m, &mut got, 0);
    let last_tick = (want.run.last().map_or(0, |l| l.tick) as usize).min(trace.len());
    for k in 0..last_tick {
        let inputs: Vec<_> =
            trace.frame(k).expect("a frame").inputs.iter().map(|i| mvs_sim_trace::input_of(i, &w, &book)).collect();
        w.events.clear();
        w.step(&inputs);
        m.tick_with(&w, &w.events, &[], &mut rec, &mut got);
        // (a match that ended on this step is left before its frame is drawn)
        let ended = w.events.iter().any(|e| matches!(e, Event::MatchEnd { phase: EndPhase::Travel, .. }));
        if !ended {
            frame(&mut m, &mut got, w.flow.tick);
        }
    }
    // the leave (only the hud run's tape has it)
    if want.run.last().is_some_and(|l| matches!(l.call, Call::StopAll { .. })) {
        m.end(w.flow.tick, false, &mut got);
    }

    let mut r = Report { hud_fed, listeners: frames, ..Report::default() };
    let note = |r: &mut Report, what: &str, i: usize, g: Option<&Line>, t: Option<&Line>| {
        let k = g.or(t).map_or_else(|| "length".to_string(), call_kind);
        *r.positional.entry(k.clone()).or_default() += 1;
        let text = format!("{what} line {i}:\n    glue: {}\n    tape: {}", brief(g, t), brief(t, g));
        r.examples.entry(k).or_insert_with(|| text.clone());
        if r.first.is_none() {
            r.first = Some(text);
        }
    };
    // begin
    for (i, (g, t)) in got_begin.iter().zip(&want_begin).enumerate() {
        if norm(g) != norm(t) {
            note(&mut r, "begin", i, Some(g), Some(t));
        }
    }
    if begin_len != want_begin.len() {
        note(&mut r, "begin length", begin_len.min(want_begin.len()), None, None);
    }
    // the run, positionally
    // (the oracle draws as many frames as its display gives, not one a tick: its listener lines are counted apart
    // and left out of the comparison, which would shift every line after the first frame that differs)
    let is_listener = |l: &&Line| matches!(l.call, Call::SetListener { .. });
    r.glue_listeners = got[begin_len..].iter().filter(is_listener).count();
    r.tape_listeners = want.run.iter().filter(is_listener).count();
    let got_run: Vec<Line> = got[begin_len..].iter().filter(|l| !is_listener(l)).cloned().collect();
    let want_run: Vec<Line> = want.run.iter().filter(|l| !is_listener(l)).cloned().collect();
    let (got_run, want_run) = (&got_run[..], &want_run[..]);
    let bones: HashSet<[u64; 3]> = rec.bones.borrow().iter().map(|b| b.map(f64::to_bits)).collect();
    for (i, (g, t)) in got_run.iter().zip(want_run).enumerate() {
        if norm(g) == norm(t) {
            r.equal += 1;
            continue;
        }
        if let (Call::Post { at: Some(ga), .. }, Call::Post { at: Some(_), .. }) = (&g.call, &t.call)
            && bones.contains(&ga.map(f64::to_bits))
        {
            let (mut a, mut b) = (g.clone(), t.clone());
            for l in [&mut a, &mut b] {
                if let Call::Post { at, .. } = &mut l.call {
                    *at = None;
                }
            }
            if a == b {
                r.bone_exempt += 1;
                continue;
            }
        }
        note(&mut r, "run", i, Some(g), Some(t));
    }
    if got_run.len() != want_run.len() {
        let n = got_run.len().min(want_run.len());
        note(&mut r, "run length", n, got_run.get(n), want_run.get(n));
    }
    // the run and the begin as multisets
    let mut pool: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for l in want_begin.iter().chain(want_run) {
        pool.entry(key(l)).or_default().push(kind(l));
    }
    for l in got_begin.iter().chain(got_run) {
        match pool.get_mut(&key(l)) {
            Some(v) if !v.is_empty() => {
                v.pop();
            }
            _ => *r.only_glue.entry(kind(l)).or_default() += 1,
        }
    }
    for k in pool.into_values().flatten() {
        *r.only_tape.entry(k).or_default() += 1;
    }
    r.glue_lines = begin_len + got_run.len();
    r.tape_lines = want_begin.len() + want_run.len();
    r
}

/// The `n` biggest counts of a map as "kind xN".
fn top(m: &BTreeMap<String, usize>, n: usize) -> String {
    let mut v: Vec<_> = m.iter().collect();
    v.sort_by(|a, b| b.1.cmp(a.1).then(a.0.cmp(b.0)));
    v.iter().take(n).map(|(k, c)| format!("{k} x{c}")).collect::<Vec<_>>().join("; ")
}

fn by_call(m: &BTreeMap<String, usize>) -> BTreeMap<String, usize> {
    let mut out = BTreeMap::new();
    for (k, c) in m {
        *out.entry(k.split(' ').next().unwrap_or("?").to_string()).or_default() += c;
    }
    out
}

#[test]
fn audio_tapes() {
    let names: Vec<String> =
        serde_json::from_str(include_str!("audio_tapes.json")).expect("audio_tapes.json is a list of names");
    let strict: HashSet<String> = std::env::var("MVS_AUDIO_TAPES_STRICT")
        .unwrap_or_default()
        .split(',')
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect();
    let Some(index) = event_index() else {
        eprintln!(
            "NOTHING CHECKED: no store with the InitBank (set CONTENT_STORE to an oracle-format store that has it)"
        );
        return;
    };
    let mut missing = Vec::new();
    let mut rows: Vec<(String, Report)> = Vec::new();
    let only: Vec<String> =
        std::env::var("MVS_AUDIO_TAPES_ONLY").unwrap_or_default().split(',').map(str::trim).map(String::from).collect();
    for name in &names {
        if !only.iter().all(String::is_empty) && !only.contains(name) {
            continue;
        }
        let Some(path) = find_tape(name) else {
            missing.push(name.as_str());
            continue;
        };
        let (n, p, i) = (name.clone(), path, index.clone());
        let report = std::panic::catch_unwind(move || check(&n, &p, i))
            .unwrap_or_else(|e| skip(format!("panic: {}", panic_text(&*e))));
        rows.push((name.clone(), report));
    }
    eprintln!("\n=== audio tapes: {} compared of {} listed ===", rows.len(), names.len());
    eprintln!(
        "{:<24} {:>8} {:>8} {:>8} {:>6} {:>6} {:>13}  result",
        "match", "glue", "tape", "equal", "bone", "diff", "listeners g/t"
    );
    let mut failed = Vec::new();
    for (name, r) in &rows {
        let result = match (&r.skipped, r.differs()) {
            (Some(s), _) => format!("SKIPPED ({s})"),
            (None, true) => "DIFFERS".to_string(),
            (None, false) => "same".to_string(),
        };
        let diff: usize = r.positional.values().sum();
        eprintln!(
            "{name:<24} {:>8} {:>8} {:>8} {:>6} {diff:>6} {:>13}  {result}",
            r.glue_lines,
            r.tape_lines,
            r.equal,
            r.bone_exempt,
            format!("{}/{}", r.glue_listeners, r.tape_listeners)
        );
        if strict.contains(name) && (r.differs() || r.skipped.is_some()) {
            failed.push(name.clone());
        }
    }
    for (name, r) in rows.iter().filter(|(_, r)| r.differs()) {
        eprintln!("\n--- {name}: {} HUD posts and {} listeners fed from the tape", r.hud_fed, r.listeners);
        if let Some(f) = &r.first {
            eprintln!("  first: {f}");
        }
        for (k, e) in &r.examples {
            eprintln!("  first {k} difference: {e}");
        }
        eprintln!("  positional by call: {}", top(&by_call(&r.positional), 12));
        eprintln!("  only in glue: {}", top(&r.only_glue, 12));
        eprintln!("  only in tape: {}", top(&r.only_tape, 12));
    }
    if !missing.is_empty() {
        eprintln!("\nno tape yet ({}): {}", missing.len(), missing.join(", "));
    }
    for s in &strict {
        if !names.contains(s) {
            failed.push(format!("{s} (not in audio_tapes.json)"));
        }
    }
    assert!(failed.is_empty(), "strict matches differ from the oracle's tape: {}", failed.join(", "));
}

fn panic_text(e: &(dyn std::any::Any + Send)) -> String {
    e.downcast_ref::<String>()
        .cloned()
        .or_else(|| e.downcast_ref::<&str>().map(|s| (*s).to_string()))
        .unwrap_or_default()
}

/// A line as printed: a scope's entries only the line has, against the other.
fn brief(l: Option<&Line>, other: Option<&Line>) -> String {
    let (Some(l), Some(o)) = (l, other) else { return format!("{l:?}") };
    match (&l.call, &o.call) {
        (Call::Scope { name, keep, lazy }, Call::Scope { name: n2, keep: k2, lazy: z2 }) if name == n2 => {
            let only = |a: &[String], b: &[String]| a.iter().filter(|e| !b.contains(e)).cloned().collect::<Vec<_>>();
            format!(
                "tick {} scope {name}: {} keep, {} lazy; only here: keep {:?} lazy {:?}",
                l.tick,
                keep.len(),
                lazy.len(),
                only(keep, k2),
                only(lazy, z2)
            )
        }
        _ => format!("{l:?}"),
    }
}
