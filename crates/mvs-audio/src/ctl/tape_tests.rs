//! The voice tapes (the oracle's recordings, one per set of events: `VOICE_SET`) replayed through the
//! interpreter: their posts made as the game makes them (`Audio::call`), taken on the audio clock as the oracle's run
//! took them, the oracle's random draws drawn again; what the interpreter says (the voices it starts, their parameter
//! changes, the draws in between) must be the tape's lines, one for one. Then the same posts through an offline
//! Audio with its mixer: no medium of a voice it starts is missing (a scope's lazy media decoded at their post).
//!
//! The tapes are not in the repo: `MVS_VOICE_TAPES` names the folder holding `<set>.jsonl` (default `.scratch/tapes/voice`
//! at the main tree's root, a worktree's too); the store is the developer's (`mvs_store::dev_store_dir`). With no folder the test says so and
//! passes; a missing set or store fails it. `MVS_VOICE_TAPES_SKIP=1` makes it pass without comparing.
//!
//! At each quantum boundary T: the voices that ran out by T (in start order), the commands of the sim ticks whose
//! time is T (tick N at ceil(N * 800 / 256) * 256 samples), then the quantum. The buses' changes are not on the tape.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use mvs_data::sound::InitBank;

use super::{Ctl, Out, Rng, secs};
use crate::spec::{Control, Target};
use crate::tape::voice::Line;
use crate::tape::{self, post};
use crate::{Audio, Backend, Note, QUANTUM, RtpcMirror, Stamped, out};

/// The sets every run must find (the vo-<set> tapes: at least one).
const SETS: [&str; 3] = ["r2", "amb", "ui"];

fn tape_dir() -> PathBuf {
    match std::env::var_os("MVS_VOICE_TAPES") {
        Some(d) => PathBuf::from(d),
        None => {
            // (the main tree's: a worktree has no recordings of its own)
            let here = Path::new(env!("CARGO_MANIFEST_DIR"));
            let common = std::process::Command::new("git")
                .args(["rev-parse", "--path-format=absolute", "--git-common-dir"])
                .current_dir(here)
                .output()
                .ok()
                .filter(|o| o.status.success())
                .and_then(|o| String::from_utf8(o.stdout).ok());
            match common.as_deref().map(str::trim).and_then(|c| Path::new(c).parent()) {
                Some(main) => main.join(".scratch/tapes/voice"),
                None => here.join("../../.scratch/tapes/voice"),
            }
        }
    }
}

fn skip() -> bool {
    std::env::var_os("MVS_VOICE_TAPES_SKIP").is_some_and(|v| v == "1")
}

/// The tapes (set name, path) and the store.
struct Inputs {
    tapes: Vec<(String, PathBuf)>,
    store: Arc<mvs_store::Store>,
    init: Arc<InitBank>,
}

/// What a tape's posts did through the mixer: the warns of a medium not there, and whether it made a sound.
type Heard = (Vec<(&'static str, u64)>, bool);

/// The tapes and the store, or None when skipped (asked, or no tapes recorded); a missing store panics.
fn inputs() -> Option<Inputs> {
    if skip() {
        eprintln!("MVS_VOICE_TAPES_SKIP=1: the voice tapes not compared");
        return None;
    }
    let dir = tape_dir();
    let Ok(entries) = std::fs::read_dir(&dir) else {
        eprintln!("no voice tapes at {}: skipped", dir.display());
        return None;
    };
    let mut tapes: Vec<(String, PathBuf)> = entries
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().is_some_and(|x| x == "jsonl"))
        .map(|p| (p.file_stem().unwrap().to_string_lossy().into_owned(), p))
        .collect();
    tapes.sort();
    for s in SETS {
        assert!(tapes.iter().any(|(n, _)| n == s), "no voice tape of the set {s} in {}", dir.display());
    }
    assert!(tapes.iter().any(|(n, _)| n.starts_with("vo-")), "no vo-<set> voice tape in {}", dir.display());
    let store = mvs_store::Store::open(mvs_store::dev_store_dir())
        .unwrap_or_else(|e| panic!("no store at {} ({e:?})", mvs_store::dev_store_dir().display()));
    let store = Arc::new(store);
    let init = Arc::new(mvs_data::sound::load_init(&store).unwrap());
    Some(Inputs { tapes, store, init })
}

/// JSON has no infinity: the tape writes the largest float in its place.
fn finite(x: f64) -> f64 {
    if x.is_infinite() { f64::MAX.copysign(x) } else { x }
}

/// A sim tick's audio clock time.
fn time_of(tick: u32) -> u64 {
    (u64::from(tick) * 800).div_ceil(256) * 256
}

/// The lines of what a call wrote: the voice starts and voice changes, the draws where they were made.
fn lines(controls: &mut Vec<Control>, trace: &mut Vec<(usize, Line)>, got: &mut Vec<Line>) {
    let mut k = 0;
    for (i, c) in controls.iter().enumerate() {
        while k < trace.len() && trace[k].0 <= i {
            got.push(trace[k].1.clone());
            k += 1;
        }
        match c {
            Control::Start(s) => {
                let mut s = *s;
                // (the tape does not carry the meters)
                s.meters = Default::default();
                for x in [&mut s.vol, &mut s.makeup, &mut s.norm, &mut s.dry, &mut s.cents] {
                    *x = finite(*x);
                }
                got.push(Line::Voice(Box::new(s)));
            }
            Control::Change(p) if matches!(p.target, Target::Voice(_)) => {
                got.push(Line::Param(crate::spec::ParamChange { value: finite(p.value), ..*p }));
            }
            _ => {}
        }
    }
    got.extend(trace.drain(k..).map(|x| x.1));
    trace.clear();
    controls.clear();
}

/// The posts of a tape and its last tick.
fn posts(tape: &[Line]) -> (Vec<post::Line>, u32) {
    let posts: Vec<post::Line> = tape
        .iter()
        .filter_map(|l| match l {
            Line::Post { tick, call } => Some(post::Line { tick: *tick, call: call.clone() }),
            _ => None,
        })
        .collect();
    let last = posts.iter().map(|p| p.tick).max().unwrap_or(0);
    (posts, last)
}

/// Runs posts through the interpreter on the audio clock, to 100 ticks past the last: the lines it says.
fn run(calls: &[post::Line], rng: Rng, store: &Arc<mvs_store::Store>, init: &Arc<InitBank>) -> Vec<Line> {
    // an Audio without a mixer: its commands are taken here
    let (tx, mut cmds) = rtrb::RingBuffer::<Stamped>::new(1 << 16);
    let (mut notes, rx) = rtrb::RingBuffer::new(1 << 16);
    let rtpc = Arc::new(RtpcMirror::new(init));
    let run = out::Run::Wasapi(out::wasapi::Wasapi { thread: None, stop: Arc::default() });
    let media = Arc::new(crate::decode::cache::MediaCache::new(0));
    let lazy = crate::ctl::scope::Lazy::new(false, media.clone());
    let mut audio = Audio {
        store: store.clone(),
        init: init.clone(),
        send: crate::send::Sender::new(tx, false),
        rx,
        inbox: Vec::new(),
        rtpc: rtpc.clone(),
        run,
        next_post: 0,
        lazy,
        music_banks: Vec::new(),
        scope_stats: Vec::new(),
        media,
        offered: crate::Offers::default(),
    };
    let mut ctl = Ctl::new(init.clone(), rtpc, rng);
    ctl.streaming = false;
    let last = calls.iter().map(|p| p.tick).max().unwrap_or(0);
    let mut stamped = std::collections::VecDeque::new();
    for p in calls {
        audio.call(p).unwrap();
        // (taken as it goes: the ring holds 65536)
        stamped.extend(std::iter::from_fn(|| cmds.pop().ok()));
    }
    let mut got = Vec::new();
    let mut controls = Vec::with_capacity(4096);
    let mut trace = Vec::new();
    let mut t = 0;
    while t <= time_of(last + 100) {
        let ended: Vec<_> = ctl.voices.list.iter().filter(|v| v.end <= secs(t)).map(|v| v.id).collect();
        let mut step = |ctl: &mut Ctl, f: &mut dyn FnMut(&mut Ctl, &mut Out)| {
            ctl.trace = Some(std::mem::take(&mut trace));
            f(ctl, &mut Out { controls: &mut controls, notes: &mut notes });
            trace = ctl.trace.take().unwrap();
            lines(&mut controls, &mut trace, &mut got);
        };
        for v in ended {
            step(&mut ctl, &mut |c, o| c.ended(v, t, o));
        }
        while stamped.front().is_some_and(|s: &Stamped| time_of(s.tick) <= t) {
            let s = stamped.pop_front().unwrap();
            let mut cmd = Some(s.cmd);
            step(&mut ctl, &mut |c, o| c.apply(cmd.take().unwrap(), t, o));
        }
        step(&mut ctl, &mut |c, o| c.quantum(t, o));
        while audio.rx.pop().is_ok() {}
        t += 256;
    }
    got
}

/// Replays one tape through the interpreter: Err with where it left the tape.
fn replay(set: &str, path: &Path, store: &Arc<mvs_store::Store>, init: &Arc<InitBank>) -> Result<String, String> {
    let tape: Vec<Line> = tape::read(path).unwrap();
    let draws = tape.iter().filter_map(|l| if let Line::Random { v } = l { Some(*v) } else { None }).collect();
    let (calls, _) = posts(&tape);
    let got = run(&calls, Rng::Replay { draws, at: 0 }, store, init);
    let want: Vec<Line> = tape.into_iter().filter(|l| !matches!(l, Line::Post { .. })).collect();
    let same = got.iter().zip(&want).take_while(|(a, b)| a == b).count();
    let head = format!("{set}: {} lines, {} made, the first {same} the same", want.len(), got.len());
    if same == want.len() && same == got.len() {
        return Ok(head);
    }
    let mut msg = head;
    for i in same.saturating_sub(3)..(same + 3).min(want.len().max(got.len())) {
        msg += &format!("\n{i}\n  tape {:?}\n  made {:?}", want.get(i), got.get(i));
    }
    Err(msg)
}

#[test]
fn voices_match_the_oracle() {
    let Some(Inputs { tapes, store, init }) = inputs() else { return };
    let results: Vec<Result<String, String>> = std::thread::scope(|s| {
        let runs: Vec<_> = tapes.iter().map(|(set, path)| s.spawn(|| replay(set, path, &store, &init))).collect();
        runs.into_iter().map(|r| r.join().unwrap()).collect()
    });
    let mut failed = 0;
    for r in &results {
        match r {
            Ok(m) => eprintln!("{m}"),
            Err(m) => {
                failed += 1;
                eprintln!("{m}");
            }
        }
    }
    assert!(failed == 0, "{failed} of {} voice tapes left the oracle's lines", results.len());
}

/// The media of the voices a tape's posts start, through an offline Audio and its mixer.
fn media_missing(path: &Path, store: &Arc<mvs_store::Store>) -> Heard {
    let tape: Vec<Line> = tape::read(path).unwrap();
    let (calls, last) = posts(&tape);
    let mut a = Audio::new(store.clone(), Backend::Offline).expect("the offline audio");
    let mut warns = Vec::new();
    let mut heard = false;
    let mut k = 0;
    let mut done = 0u64;
    for tick in 0..=last + 100 {
        while k < calls.len() && calls[k].tick == tick {
            a.call(&calls[k]).unwrap();
            k += 1;
        }
        let to = time_of(tick + 1);
        a.pump(tick, usize::try_from(to - done).unwrap());
        done = to;
        heard |= a.take_output().iter().any(|s| s.abs() > 1e-4);
        for n in a.take_notes() {
            if let Note::Warn { what, id } = n
                && (what.contains("medium") || what.contains("late"))
            {
                warns.push((what, id));
            }
        }
    }
    assert_eq!(done % QUANTUM as u64, 0);
    warns.sort_unstable();
    warns.dedup();
    (warns, heard)
}

#[test]
fn lazy_media_play() {
    let Some(Inputs { tapes, store, .. }) = inputs() else { return };
    let results: Vec<(String, Heard)> = std::thread::scope(|s| {
        let runs: Vec<_> = tapes
            .iter()
            .map(|(set, path)| {
                let store = &store;
                s.spawn(move || (set.clone(), media_missing(path, store)))
            })
            .collect();
        runs.into_iter().map(|r| r.join().unwrap()).collect()
    });
    let mut failed = 0;
    for (set, (w, heard)) in &results {
        eprintln!("{set}: {} media missing {:?}, heard {heard}", w.len(), &w[..w.len().min(8)]);
        failed += usize::from(!w.is_empty() || !heard);
    }
    assert!(failed == 0, "{failed} of {} voice tapes start voices whose media are missing (or silent)", results.len());
}

/// A continuous trigger-rate container with LoopCount 0 (the post-match level bar's tick, 70 ms apart) plays on
/// until its post is stopped: the tapes leave such events out (the oracle plays one sound).
#[test]
fn a_looping_container_plays_until_stopped() {
    let Some(Inputs { store, init, .. }) = inputs() else { return };
    let ev = "ui_postmatch_levelbar_tick";
    let line = |tick, call| post::Line { tick, call };
    let calls = [
        line(0, post::Call::Scope { name: "loop".into(), keep: Vec::new(), lazy: vec![ev.into()] }),
        line(1, post::Call::Post { ev: ev.into(), owner: Some(-1), key: Some("k".into()), stop_key: None, at: None }),
        line(61, post::Call::StopKey { key: "k".into(), fade: 0.0, curve: None }),
    ];
    let got = run(&calls, Rng::Seeded(7), &store, &init);
    let starts: Vec<u64> = got.iter().filter_map(|l| if let Line::Voice(v) = l { Some(v.at) } else { None }).collect();
    // one second of ticks 70 ms apart; none after the stop but those placed ahead of it (up to 0.2 s)
    assert!(starts.len() >= 12, "{} starts: {starts:?}", starts.len());
    assert!(starts.iter().all(|&a| a <= time_of(61) + 9600), "a start after the stop: {starts:?}");
}
