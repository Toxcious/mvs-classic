//! The output level against the oracle's: a match's post tape through an offline
//! Audio and its mixer, RMS and peak per second of world time (60 ticks) for the output and three bus outputs, against
//! the oracle's levels of the same run (`AUDIO_LEVELS=1`: AnalyserNode taps on the same buses, sampled each drawn
//! frame,
//! written beside the tape as `<match>.levels.json`).
//!
//! Opt-in: `MVS_LEVELS` names the matches (comma-separated); without it the test says so and passes. The level run's
//! own tape and levels (its emitter and listener calls follow its drawn frames) are in `MVS_LEVEL_TAPES` (default: the
//! main tree's `.scratch/audio-tapes/levels`); the store is the developer's
//! (`mvs_store::dev_store_dir`). A missing tape, levels file or store fails a named match.
//!
//! Pass: for each tap, the median of the per-second differences within 1 dB and the worst second within 3 dB (the
//! combat HDR bus and the output: [`WORST_DB_COMBAT`]). A
//! second is compared where either side is above [`QUIET`]. The first second is left out (the tape's tick-0 calls hold
//! the title screen's and the menus', which the oracle played over real seconds before the match and the offline run
//! plays at once), and so is the last, partial one, a second the oracle read less than [`SEEN`] of, and the second
//! of a hold of the oracle's context (the oracle behind the clock) and the [`AFTER_HOLD`] after it.
//!
//! The random draws are not the oracle's (the oracle's `Math.random` has no seed, the mixer's is seeded from the
//! clock): a
//! random container's pick differs between runs, and the combat bus's level in the same second moved by up to 3.5 dB
//! between two offline runs of one tape (m5-combat).

use std::path::{Path, PathBuf};
use std::sync::Arc;

use mvs_data::sound::Id;

use super::bus::{Level, Meter};
use crate::tape::{self, post};
use crate::{Audio, Backend, out};

/// The buses compared besides the output: the combat HDR bus, the VO bus, the music bus.
const TAPS: [(&str, Id); 3] = [("combat-hdr", 931_844_945), ("vo", 1_199_225_261), ("music", 2_054_245_787)];
/// Ticks in a second of world time.
const SECOND: u32 = 60;
/// Below this (dBFS) a second is silence.
const QUIET: f64 = -70.0;
/// A silent side's level in a difference.
const FLOOR: f64 = -100.0;
/// The share of a second's samples the oracle must have read for the second to be compared (a frame later than its
/// analysers hold loses samples; a second whose ticks the oracle gave while it held the context has few).
const SEEN: f64 = 0.9;
/// Seconds after one the oracle held its context in that are not compared either: the oracle's levels there are off its
/// own (a burst of up to +28 dB on the combat bus, or seconds of silence on every tap, in three runs, at none of the
/// same seconds).
const AFTER_HOLD: u32 = 3;
const MEDIAN_DB: f64 = 1.0;
const WORST_DB: f64 = 3.0;
/// The worst second of the combat HDR bus and of the output (which carries it): both sides draw a random container's variant unseeded (the oracle's
/// `Math.random`, the mixer's clock seed), and on m5-combat two offline runs of one tape differed by up to 3.5 dB on
/// this bus in the same second; the worst second against the oracle was 2.4 to 4.2 dB on this bus and 2.75 to 3.82 dB
/// on the output over the runs.
const WORST_DB_COMBAT: f64 = 5.0;

fn tape_dir() -> PathBuf {
    if let Some(d) = std::env::var_os("MVS_LEVEL_TAPES") {
        return PathBuf::from(d);
    }
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
        Some(main) => main.join(".scratch/audio-tapes/levels"),
        None => here.join("../../.scratch/audio-tapes/levels"),
    }
}

/// A sim tick's audio clock time (the voice tapes' rule: tick N at ceil(N * 800 / 256) * 256 samples).
fn time_of(tick: u32) -> u64 {
    (u64::from(tick) * 800).div_ceil(256) * 256
}

fn db(l: &Level) -> (Option<f64>, Option<f64>) {
    if l.n == 0 || l.sq <= 0.0 {
        return (None, None);
    }
    let rms = 10.0 * (l.sq / l.n as f64).log10();
    let peak = (l.peak > 0.0).then(|| 20.0 * f64::from(l.peak).log10());
    (Some(rms), peak)
}

/// Per second, per tap (the output first, then [`TAPS`]): the levels of the offline run.
fn native(calls: &[post::Line], store: &Arc<mvs_store::Store>, buses: &[Id]) -> Vec<Vec<Level>> {
    let last = calls.iter().map(|p| p.tick).max().unwrap_or(0);
    let mut a = Audio::new(store.clone(), Backend::Offline).expect("the offline audio");
    {
        let out::Run::Offline { mixer, .. } = &mut a.run else { unreachable!() };
        let b = &mut mixer.dsp.buses;
        let taps = buses
            .iter()
            .map(|&id| (b.index(id).unwrap_or_else(|| panic!("no bus {id} in the InitBank")), Level::default()))
            .collect();
        b.meter = Some(Box::new(Meter { taps }));
    }
    let mut seconds: Vec<Vec<Level>> = Vec::new();
    let mut k = 0;
    let mut done = 0u64;
    for tick in 0..=last {
        while k < calls.len() && calls[k].tick <= tick {
            a.call(&calls[k]).unwrap();
            k += 1;
        }
        let to = time_of(tick + 1);
        a.pump(tick, usize::try_from(to - done).unwrap());
        done = to;
        let s = (tick / SECOND) as usize;
        if seconds.len() <= s {
            seconds.push(vec![Level::default(); buses.len() + 1]);
        }
        let row = &mut seconds[s];
        row[0].add(&a.take_output());
        let out::Run::Offline { mixer, .. } = &mut a.run else { unreachable!() };
        let m = mixer.dsp.buses.meter.as_mut().unwrap();
        for (j, (_, l)) in m.taps.iter_mut().enumerate() {
            let r = &mut row[j + 1];
            r.sq += l.sq;
            r.n += l.n;
            r.peak = r.peak.max(l.peak);
            *l = Level::default();
        }
        let _ = a.take_notes();
    }
    seconds
}

/// The oracle's levels: per tap name, RMS and peak (dB, None: silence) per second.
#[derive(serde::Deserialize)]
struct Page {
    rate: f64,
    /// Samples read per second, per channel.
    seen: Vec<f64>,
    /// The world ticks the oracle held its context at (it was behind the clock).
    #[serde(rename = "heldAt")]
    held_at: Vec<u32>,
    taps: std::collections::BTreeMap<String, PageTap>,
}

#[derive(serde::Deserialize)]
struct PageTap {
    rms: Vec<Option<f64>>,
}

/// The tap's differences (native - page, dB) over the compared seconds: (seconds compared, median, worst |d|, the
/// worst second).
fn compare(native: &[Option<f64>], page: &[Option<f64>], seen: &[bool]) -> (usize, f64, f64, usize) {
    let n = native.len().min(page.len()).saturating_sub(1);
    let mut d: Vec<(f64, usize)> = Vec::new();
    for s in 1..n {
        if !seen.get(s).copied().unwrap_or(false) {
            continue;
        }
        let (a, b) = (native[s].unwrap_or(FLOOR).max(FLOOR), page[s].unwrap_or(FLOOR).max(FLOOR));
        if a < QUIET && b < QUIET {
            continue;
        }
        d.push((a - b, s));
    }
    if d.is_empty() {
        return (0, 0.0, 0.0, 0);
    }
    let mut sorted: Vec<f64> = d.iter().map(|x| x.0).collect();
    sorted.sort_by(f64::total_cmp);
    let median = if sorted.len() % 2 == 1 {
        sorted[sorted.len() / 2]
    } else {
        (sorted[sorted.len() / 2 - 1] + sorted[sorted.len() / 2]) / 2.0
    };
    let worst = d.iter().copied().max_by(|a, b| a.0.abs().total_cmp(&b.0.abs())).unwrap();
    (d.len(), median, worst.0.abs(), worst.1)
}

#[test]
fn levels_match_the_page() {
    let Some(names) = std::env::var("MVS_LEVELS").ok().filter(|s| !s.is_empty()) else {
        eprintln!("MVS_LEVELS names no match: the level check not run");
        return;
    };
    let dir = tape_dir();
    let store = mvs_store::Store::open(mvs_store::dev_store_dir())
        .unwrap_or_else(|e| panic!("no store at {} ({e:?})", mvs_store::dev_store_dir().display()));
    let store = Arc::new(store);
    let buses: Vec<Id> = TAPS.iter().map(|t| t.1).collect();
    let mut failed = Vec::new();
    for name in names.split(',').map(str::trim) {
        let calls: Vec<post::Line> = tape::read(&dir.join(format!("{name}.tape"))).unwrap();
        let levels = dir.join(format!("{name}.levels.json"));
        let page: Page = serde_json::from_slice(
            &std::fs::read(&levels).unwrap_or_else(|e| panic!("no levels at {} ({e})", levels.display())),
        )
        .unwrap();
        let seconds = native(&calls, &store, &buses);
        let held = |s: u32| page.held_at.iter().any(|&h| (h / SECOND..=h / SECOND + AFTER_HOLD).contains(&s));
        let seen: Vec<bool> = (0u32..).zip(&page.seen).map(|(s, &n)| n >= SEEN * page.rate && !held(s)).collect();
        eprintln!(
            "{name}: {} of {} seconds read whole and clear of a hold on the oracle ({} holds)",
            seen.iter().filter(|&&x| x).count(),
            seen.len(),
            page.held_at.len()
        );
        let names = std::iter::once("master").chain(TAPS.iter().map(|t| t.0));
        for (j, tap) in names.enumerate() {
            let ours: Vec<Option<f64>> = seconds.iter().map(|r| db(&r[j]).0).collect();
            let peaks: Vec<Option<f64>> = seconds.iter().map(|r| db(&r[j]).1).collect();
            let theirs = &page.taps.get(tap).unwrap_or_else(|| panic!("{name}: no tap {tap} in the levels")).rms;
            let (n, median, worst, at) = compare(&ours, theirs, &seen);
            let peak = peaks.iter().flatten().copied().fold(f64::NEG_INFINITY, f64::max);
            if std::env::var_os("MVS_LEVELS_PRINT").is_some() {
                let row = |v: &[Option<f64>]| {
                    v.iter().map(|x| x.map_or("-".into(), |x| format!("{x:.1}"))).collect::<Vec<_>>().join(" ")
                };
                eprintln!("{tap:>10} native {}\n{tap:>10} page   {}", row(&ours), row(theirs));
            }
            let bound = if tap == "combat-hdr" || tap == "master" { WORST_DB_COMBAT } else { WORST_DB };
            let ok = median.abs() <= MEDIAN_DB && worst <= bound;
            eprintln!(
                "{name} {tap:>10}: {n} s compared, median {median:+.2} dB, worst {worst:.2} dB (second {at}: {:?} / {:?}), peak {peak:.1} dBFS{}",
                ours.get(at).copied().flatten().map(|x| (x * 100.0).round() / 100.0),
                theirs.get(at).copied().flatten().map(|x| (x * 100.0).round() / 100.0),
                if ok { "" } else { "  FAIL" }
            );
            if !ok {
                failed.push(format!("{name} {tap}"));
            }
        }
    }
    assert!(failed.is_empty(), "levels off the oracle's: {failed:?}");
}
