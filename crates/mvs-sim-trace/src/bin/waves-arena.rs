//! `waves-arena`: headless runs of the port's own Waves mode (`mvs_sim::modes::waves`) on the dev store, a bot in
//! the player's seat: how far a run gets and how long it takes, the numbers the director's first tuning was set from
//! (the module's doc), and a crash finder over the stages.
//!
//!   waves-arena [--variant 100|endless] [--stage <key>|all] [--runs <n>] [--char <key>] [--level <0..4>]
//!               [--minutes <n>] [--seed <n>] [--cast <a,b,..>] [--jobs <n>]
//!
//! Each run is its own World: the player's bot at `--level` (3, Hard, by default) playing `--char` (Superman), the
//! cast drawn from the run's seed over the roster (or `--cast`), up to `--minutes` of match time (10). `--stage all`
//! runs every stage of the store's map list. One line per run: the stage, the World's seed, the end (cleared, out, or
//! the time limit), the ring-outs, the run's time, the enemies spawned, the most fighters the World's arena held and
//! the player's damage at the end; a run whose sim
//! panics prints its message and seed. Runs at idle priority; `--jobs` runs at once (a quarter of the logical
//! processors by default).

use mvs_sim::modes::waves::{self, WavesEnd};
use mvs_sim::setup::WavesVariant;
use mvs_sim_trace::waves as run;
use std::process::ExitCode;
use std::sync::Mutex;

const USAGE: &str = "usage: waves-arena [--variant 100|endless] [--stage <key>|all] [--runs <n>] [--char <key>] \
                     [--level <0..4>] [--minutes <n>] [--seed <n>] [--cast <a,b,..>] [--jobs <n>]";

/// The roster (`mvs_protocol::roster::CHARACTERS`, which this crate does not depend on), the casts' pool.
const ROSTER: [&str; 23] = [
    "shaggy",
    "morty",
    "irongiant",
    "wonderwoman",
    "harley",
    "superman",
    "lebron",
    "arya",
    "batman",
    "creature",
    "blackadam",
    "bugs",
    "rick",
    "taz",
    "steven",
    "garnet",
    "finn",
    "jake",
    "tomandjerry",
    "velma",
    "marvin",
    "stripe",
    "gizmo",
];

/// The stages of the store's map list (the importer's `STAGES` but the tutorial's map).
const STAGES: [&str; 20] = [
    "trainingroomsmall",
    "m011_largenoplat",
    "m011_smallnoplat",
    "m011",
    "m011_small",
    "classic3platform_small",
    "classic3platform_large",
    "trainingroomlarge",
    "trophyroom_largeplatform",
    "trophyroom_2platforms",
    "treehouse",
    "treehousesmall",
    "treehouse_1plat_large",
    "m009",
    "m009_small",
    "scoobydoo",
    "scoobydoo_noroof",
    "batcave",
    "m008",
    "m010",
];

struct Args {
    variant: WavesVariant,
    stages: Vec<String>,
    runs: usize,
    character: String,
    level: i32,
    minutes: u32,
    seed: i32,
    cast: Option<Vec<String>>,
    jobs: usize,
}

fn args() -> Result<Args, String> {
    let mut a = Args {
        variant: WavesVariant::RingOuts100,
        stages: vec!["trainingroomsmall".to_owned()],
        runs: 4,
        character: "superman".to_owned(),
        level: 3,
        minutes: 10,
        seed: 1,
        cast: None,
        jobs: std::thread::available_parallelism().map_or(2, |n| (n.get() / 4).max(1)),
    };
    let mut it = std::env::args().skip(1);
    while let Some(k) = it.next() {
        let mut v = || it.next().ok_or_else(|| format!("{k} needs a value"));
        match k.as_str() {
            "--variant" => {
                a.variant = match v()?.as_str() {
                    "100" => WavesVariant::RingOuts100,
                    "endless" => WavesVariant::Endless,
                    s => return Err(format!("--variant {s}")),
                }
            }
            "--stage" => {
                let s = v()?;
                a.stages = if s == "all" { STAGES.iter().map(|s| (*s).to_owned()).collect() } else { vec![s] };
            }
            "--runs" => a.runs = v()?.parse().map_err(|e| format!("--runs: {e}"))?,
            "--char" => a.character = v()?,
            "--level" => a.level = v()?.parse().map_err(|e| format!("--level: {e}"))?,
            "--minutes" => a.minutes = v()?.parse().map_err(|e| format!("--minutes: {e}"))?,
            "--seed" => a.seed = v()?.parse().map_err(|e| format!("--seed: {e}"))?,
            "--cast" => a.cast = Some(v()?.split(',').map(str::to_owned).collect()),
            "--jobs" => a.jobs = v()?.parse().map_err(|e| format!("--jobs: {e}"))?,
            _ => return Err(USAGE.to_owned()),
        }
    }
    Ok(a)
}

/// One run's outcome.
struct Outcome {
    end: Option<WavesEnd>,
    ring_outs: u32,
    ticks: u32,
    spawned: i32,
    most_fighters: usize,
    /// The player's damage at the run's end.
    damage: f32,
}

fn play(store: &mvs_store::Store, a: &Args, stage: &str, seed: i32) -> Result<Outcome, String> {
    let roster: Vec<String> = ROSTER.iter().map(|s| (*s).to_owned()).collect();
    let cast = a.cast.clone().unwrap_or_else(|| waves::draw_cast(seed, &roster));
    let setup = run::bot_setup(a.variant, &a.character, a.level, cast);
    let data = run::data_for(store, &setup, stage)?;
    let mut w = run::world(&data, &setup, seed)?;
    let mut most = 0;
    for _ in 0..a.minutes * 3600 {
        run::step(&mut w);
        most = most.max(w.fighters.len());
        if waves::status(&w).is_some_and(|s| s.end.is_some()) {
            break;
        }
    }
    let s = waves::status(&w).ok_or("no Waves run")?;
    Ok(Outcome {
        end: s.end,
        ring_outs: s.ring_outs,
        ticks: s.ticks,
        spawned: w.waves.as_ref().map_or(0, |r| r.spawned),
        most_fighters: most,
        damage: w.waves.as_ref().and_then(|r| r.player).map_or(0.0, |p| w.fighter(p).stats.damage),
    })
}

fn main() -> ExitCode {
    let a = match args() {
        Ok(a) => a,
        Err(e) => {
            eprintln!("{e}");
            return ExitCode::FAILURE;
        }
    };
    mvs_sim_trace::machine::lower_priority();
    let store = match mvs_store::Store::open(mvs_store::dev_store_dir()) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("the dev store: {e:?}");
            return ExitCode::FAILURE;
        }
    };
    let jobs: Vec<(String, i32)> = a
        .stages
        .iter()
        .flat_map(|s| (0..a.runs).map(move |r| (s.clone(), i32::try_from(r).expect("a few runs"))))
        .map(|(s, r)| (s, a.seed.wrapping_mul(7919).wrapping_add(r)))
        .collect();
    let next = Mutex::new(0usize);
    let crashed = Mutex::new(false);
    // (a panic's message comes through the hook; the default one would print it with no seed)
    std::panic::set_hook(Box::new(|_| {}));
    std::thread::scope(|sc| {
        for _ in 0..a.jobs.max(1) {
            sc.spawn(|| {
                loop {
                    let i = {
                        let mut n = next.lock().expect("no job panicked");
                        let i = *n;
                        *n += 1;
                        i
                    };
                    let Some((stage, seed)) = jobs.get(i) else { break };
                    let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| play(&store, &a, stage, *seed)));
                    match r {
                        Ok(Ok(o)) => {
                            let end = match o.end {
                                Some(WavesEnd::Cleared) => "cleared",
                                Some(WavesEnd::PlayerOut) => "out",
                                None => "time",
                            };
                            println!(
                                "{stage:<26} seed {seed:>6}  {end:<7} {:>4} ring-outs  {:>2}:{:02}  {:>4} spawned  {:>3} fighters at most  player at {:.0}",
                                o.ring_outs,
                                o.ticks / 3600,
                                (o.ticks / 60) % 60,
                                o.spawned,
                                o.most_fighters,
                                o.damage,
                            );
                        }
                        Ok(Err(e)) => {
                            println!("{stage:<26} seed {seed:>6}  error: {e}");
                            *crashed.lock().expect("no job panicked") = true;
                        }
                        Err(p) => {
                            let msg = p
                                .downcast_ref::<String>()
                                .cloned()
                                .or_else(|| p.downcast_ref::<&str>().map(|s| (*s).to_owned()))
                                .unwrap_or_default();
                            println!("{stage:<26} seed {seed:>6}  CRASH: {msg}");
                            *crashed.lock().expect("no job panicked") = true;
                        }
                    }
                }
            });
        }
    });
    if *crashed.lock().expect("no job panicked") { ExitCode::FAILURE } else { ExitCode::SUCCESS }
}
