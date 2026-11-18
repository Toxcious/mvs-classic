//! `mvs --load-bench`: the loads a player waits on, timed step by step with no input and no frame drawn: a match's
//! first load, the Lab's restart with the same line-up and with a new fighter, a stage change, and the character
//! select's hover (a fighter's preview and the pre-match voice's data). One `mvs: load` line per match load (its
//! steps, `MatchLoop::new`), one `mvs: load-bench` line per case, a summary at the end.
//!
//! A hover's view (with its warm-up) and its voice data are made on the loader as the menus make them, this thread
//! waiting in 2 ms polls; `stall` is this thread's part, the warm-up's recorded commands run.
//!
//! A match loads as the loading screen loads it: its job made on this thread (`start`), run on the loader while this
//! thread waits in 2 ms polls (`ready`: the job's whole time), then installed here (`install`, `MatchLoop::new`).
//! `stall` is the longest of this thread's two parts: what a frame waits on.
//!
//! The sound mixes to a buffer nobody hears; between two loads the mixer is pumped and its notes taken, as the frames
//! between two matches do, so a scope let go is freed before the next one loads.

use std::time::Instant;

use mvs_sim::setup::MatchPlayer;

use crate::app::{MatchKind, MatchStart, Shell};
use crate::prepare::{Pending, PrepareJob};
use crate::run::{MatchLoop, MatchOptions, default_setup};
use crate::{Error, Result};

/// One case: what loads.
enum Case {
    /// A match of these characters on this stage.
    Match { chars: [&'static str; 2], stage: &'static str },
    /// The character select's hover on a fighter: its preview and its data for the pre-match voice.
    Hover { key: &'static str },
}

/// The cases in order: each match after the one before it ended, as the Lab's restarts follow each other.
const CASES: &[(&str, Case)] = &[
    ("first", Case::Match { chars: ["shaggy", "shaggy"], stage: "trainingroomsmall" }),
    ("same line-up", Case::Match { chars: ["shaggy", "shaggy"], stage: "trainingroomsmall" }),
    ("new fighter", Case::Match { chars: ["batman", "shaggy"], stage: "trainingroomsmall" }),
    ("same line-up", Case::Match { chars: ["batman", "shaggy"], stage: "trainingroomsmall" }),
    ("new stage", Case::Match { chars: ["batman", "shaggy"], stage: "m010" }),
    ("back", Case::Match { chars: ["shaggy", "shaggy"], stage: "trainingroomsmall" }),
    ("hover", Case::Hover { key: "batman" }),
    ("hover", Case::Hover { key: "wonderwoman" }),
    ("hover again", Case::Hover { key: "batman" }),
];

/// Runs the cases on the shell's store and prints their lines.
pub fn run(sh: &mut Shell) -> Result<()> {
    let mut summary = Vec::new();
    for (i, (name, case)) in CASES.iter().enumerate() {
        let t0 = Instant::now();
        let what = match case {
            Case::Match { chars, stage } => {
                let mut setup = default_setup();
                for (p, c) in setup.players.iter_mut().zip(chars) {
                    *p = MatchPlayer { character: Some((*c).to_owned()), ..MatchPlayer::default() };
                }
                let mut start = MatchStart::new(setup, MatchKind::Default, 1);
                start.stage = (*stage).to_owned();
                let mut pending = PrepareJob::of(sh, &start, false).map(Pending::start);
                let started = t0.elapsed().as_secs_f64() * 1e3;
                start.prepared = match &mut pending {
                    Some(p) => loop {
                        if let Some(r) = p.take() {
                            break r;
                        }
                        std::thread::sleep(std::time::Duration::from_millis(2));
                    },
                    None => None,
                };
                let ready = t0.elapsed().as_secs_f64() * 1e3;
                let i0 = Instant::now();
                let mut m = MatchLoop::new(sh, start, &MatchOptions::default())?;
                let install = i0.elapsed().as_secs_f64() * 1e3;
                let ms = t0.elapsed().as_secs_f64() * 1e3;
                let laps = m.load_laps.clone();
                m.end(sh)?;
                drop(m);
                settle_audio(sh);
                let steps: Vec<String> = laps.laps.iter().map(|(n, t)| format!("{n} {t:.0}")).collect();
                let stall = started.max(install);
                format!(
                    "{} v {} on {stage}: {ms:.0} ms, stall {stall:.0} (start {started:.1}, ready {ready:.0}, install \
                     {install:.0}: {})",
                    chars[0],
                    chars[1],
                    steps.join(", ")
                )
            }
            Case::Hover { key } => {
                let store = sh.store().ok_or_else(|| Error::Match("no store is open".into()))?.clone();
                let r = sh.renderer.as_ref().ok_or_else(|| Error::Match("no renderer".into()))?;
                let warm = r.warm_job(None);
                let k = (*key).to_owned();
                let s2 = store.clone();
                let mut view = mvs_load::spawn(move || {
                    let p0 = Instant::now();
                    let made = mvs_render::menuscene::preview::make_view(&s2, &k, "", Some(warm));
                    (made, p0.elapsed().as_secs_f64() * 1e3)
                });
                let k = (*key).to_owned();
                let data = mvs_load::spawn(move || {
                    let d0 = Instant::now();
                    let d = mvs_data::GameData::load(&store, &[&k], mvs_sim_trace::TRACE_STAGE);
                    (d.is_ok(), d0.elapsed().as_secs_f64() * 1e3)
                });
                let started = t0.elapsed().as_secs_f64() * 1e3;
                let (made, preview) = loop {
                    if let Some(v) = view.try_take() {
                        break v;
                    }
                    std::thread::sleep(std::time::Duration::from_millis(2));
                };
                let (_, data) = data.wait();
                let ready = t0.elapsed().as_secs_f64() * 1e3;
                let i0 = Instant::now();
                let (v, recorded) = made.map_err(Error::Match)?;
                if let Some(rec) = &recorded {
                    r.gpu().run_recorded(rec);
                }
                drop(v);
                let install = i0.elapsed().as_secs_f64() * 1e3;
                let stall = started.max(install);
                format!(
                    "{key}: stall {stall:.1} (start {started:.1}, ready {ready:.0}: preview and warm-up {preview:.0}, \
                     voice data {data:.0}; install {install:.1})"
                )
            }
        };
        let line = format!("case {} {name}: {what} | mem {:.0} MB", i + 1, crate::perf::private_mb());
        eprintln!("mvs: load-bench {line}");
        summary.push(line);
    }
    eprintln!("mvs: load-bench summary");
    for l in &summary {
        eprintln!("  {l}");
    }
    Ok(())
}

/// The frames between two loads, for the sound: the mixer takes the commands sent (a scope let go among them) and
/// the game side drops what it hands back.
fn settle_audio(sh: &mut Shell) {
    let Some(a) = sh.audio.as_mut() else { return };
    for _ in 0..4 {
        a.pump(u32::MAX, mvs_audio::QUANTUM * mvs_audio::QUANTA_PER_FRAME);
        let _ = a.take_output();
        let _ = a.take_notes();
    }
}
