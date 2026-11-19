//! `expert-data`: the Expert network's training data (`docs/expert-bot-plan.md`, "X5"). Plays bot games headless on
//! the dev store, an Expert bot in each against Hard or another Expert, and at every `--every`-th decision of each
//! Expert takes a teacher's sample (`mvs_sim::ai::expert::Expert::teach`): the network's input and the look-ahead's
//! score of every action of the list, with no budget.
//!
//!   expert-data --out <dir> [--games <n>] [--every <n>] [--lead x4|net] [--net <weights>] [--stages <a,b,..>]
//!               [--kos <n>] [--jobs <n>] [--seed <n>]
//!
//! Game `g` draws its two fighters, its stage and the other bot's level (Hard on even games, Expert on odd) from
//! `--seed` and `g`; the Experts' lead is `--lead` (the hand-made shortlist, or the network: the built-in weights or
//! `--net`'s file). Each game, as it ends, writes a folder `<dir>/g<game>` of its own: `features.npy` (samples x
//! inputs, f32), `scores.npy` (samples x actions, f32), `shortlist.npy` and `lead.npy` (samples x actions, u8: the
//! hand-made shortlist picks the action, the lead picked it) and `meta.npy` (samples x 4, i32: the game, the slot,
//! the bot's fighter and the stage, by their lists' order); `<dir>/actions.txt` names the actions. A game whose
//! folder is there already is skipped, so a stopped run started again with the same arguments goes on where it was.
//! Runs at below-normal priority, `--jobs` games at once.

use mvs_sim::ai::bt::BtBot;
use mvs_sim::ai::expert::{self, Lead, Sample};
use mvs_sim::input::InputState;
use mvs_sim::setup::{MatchPlayer, MatchSetup, Rules, TeamStyle};
use mvs_sim::world::World;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Instant;

const USAGE: &str = "usage: expert-data --out <dir> [--games <n>] [--every <n>] [--lead x4|net] [--net <weights>] \
                     [--stages <a,b,..>] [--kos <n>] [--jobs <n>] [--seed <n>]";

/// The match clock in seconds; a game is cut 30 s past it.
const DURATION: f64 = 420.0;

struct Args {
    out: PathBuf,
    games: usize,
    every: u32,
    lead: Lead,
    stages: Vec<String>,
    kos: f64,
    jobs: usize,
    seed: u32,
}

fn main() -> ExitCode {
    match main_() {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("expert-data: {e}");
            ExitCode::from(2)
        }
    }
}

fn args() -> Result<Args, String> {
    let mut a = Args {
        out: PathBuf::new(),
        games: 100,
        every: 4,
        lead: Lead::Shortlist,
        stages: vec![mvs_sim_trace::TRACE_STAGE.to_owned()],
        kos: 2.0,
        jobs: std::thread::available_parallelism().map_or(2, |n| (n.get() / 2).max(1)),
        seed: 1,
    };
    let mut lead = String::from("x4");
    let mut net: Option<String> = None;
    let mut it = std::env::args().skip(1);
    while let Some(s) = it.next() {
        let mut val = || it.next().ok_or(format!("{s} needs a value"));
        let num = |v: String| v.parse::<u32>().map_err(|_| format!("{s}: not a number: {v}"));
        match s.as_str() {
            "--out" => a.out = PathBuf::from(val()?),
            "--games" => a.games = num(val()?)? as usize,
            "--every" => a.every = num(val()?)?.max(1),
            "--lead" => lead = val()?,
            "--net" => net = Some(val()?),
            "--stages" => a.stages = val()?.split(',').map(str::to_owned).collect(),
            "--kos" => a.kos = f64::from(num(val()?)?),
            "--jobs" => a.jobs = (num(val()?)? as usize).max(1),
            "--seed" => a.seed = num(val()?)?,
            _ => return Err(USAGE.to_owned()),
        }
    }
    if a.out.as_os_str().is_empty() {
        return Err(USAGE.to_owned());
    }
    a.lead = mvs_sim_trace::expert_lead(&lead, net.as_deref())?;
    Ok(a)
}

/// A small random stream of the tool's own (the game's draw): splitmix32.
fn draw(seed: u32, g: usize, k: u32) -> u32 {
    let mut z = seed.wrapping_mul(0x9e37_79b9) ^ (g as u32).wrapping_mul(0x85eb_ca6b) ^ k.wrapping_mul(0xc2b2_ae35);
    z = (z ^ (z >> 16)).wrapping_mul(0x7feb_352d);
    z = (z ^ (z >> 15)).wrapping_mul(0x846c_a68b);
    z ^ (z >> 16)
}

/// One game's samples, each with the slot that took it.
type Samples = Vec<(usize, Sample)>;

fn main_() -> Result<(), String> {
    let args = args()?;
    mvs_sim_trace::machine::lower_priority();
    std::fs::create_dir_all(&args.out).map_err(|e| format!("{}: {e}", args.out.display()))?;
    let dir = mvs_store::dev_store_dir();
    let store = mvs_store::Store::open(&dir).map_err(|e| format!("the store at {}: {e:?}", dir.display()))?;
    let roster = mvs_data::shared::<mvs_data::Index>(&store, mvs_data::path::INDEX)
        .map_err(|e| format!("the gamedata index: {e:?}"))?
        .characters
        .clone();
    println!(
        "expert-data: {} games on {}, a sample every {} decisions, lead {:?}, {} at once",
        args.games,
        args.stages.join(","),
        args.every,
        matches!(args.lead, Lead::Net(_)).then_some("net").unwrap_or("x4"),
        args.jobs
    );
    std::panic::set_hook(Box::new(|_| {}));
    let started = Instant::now();
    let next = AtomicUsize::new(0);
    let made = AtomicUsize::new(0);
    std::fs::write(args.out.join("actions.txt"), expert::action_names().join("\n"))
        .map_err(|e| format!("actions.txt: {e}"))?;
    let error = std::sync::Mutex::new(None);
    std::thread::scope(|s| {
        for _ in 0..args.jobs.min(args.games) {
            s.spawn(|| {
                loop {
                    let g = next.fetch_add(1, Ordering::Relaxed);
                    if g >= args.games || error.lock().expect("no job panicked").is_some() {
                        break;
                    }
                    let folder = args.out.join(format!("g{g:05}"));
                    if folder.exists() {
                        continue;
                    }
                    let pick = |k: u32, n: usize| draw(args.seed, g, k) as usize % n;
                    let (a, b) = (&roster[pick(1, roster.len())], &roster[pick(2, roster.len())]);
                    let stage = pick(3, args.stages.len());
                    let keys = if a == b { vec![a.as_str()] } else { vec![a.as_str(), b.as_str()] };
                    let data = match mvs_data::GameData::load(&store, &keys, &args.stages[stage]) {
                        Ok(d) => Arc::new(d),
                        Err(e) => {
                            *error.lock().expect("no job panicked") = Some(format!("{a} vs {b}: {e:?}"));
                            break;
                        }
                    };
                    let level_b = if g.is_multiple_of(2) { 3 } else { expert::EXPERT_LEVEL };
                    let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                        play(&data, &args, [a, b], level_b, g)
                    }));
                    let samples = match r {
                        Ok(Ok(s)) => s,
                        Ok(Err(e)) => {
                            *error.lock().expect("no job panicked") = Some(e);
                            break;
                        }
                        Err(_) => {
                            println!("game {g} ({a} vs {b}) crashed; its samples are dropped");
                            Vec::new()
                        }
                    };
                    let a_i = roster.iter().position(|r| r == a).unwrap_or(0);
                    let b_i = roster.iter().position(|r| r == b).unwrap_or(0);
                    let n = made.fetch_add(samples.len(), Ordering::Relaxed) + samples.len();
                    println!(
                        "game {g:>4} {a:>12} vs {b:<12} (level {level_b}) {:>5} samples, {n} in all, {:.0} s",
                        samples.len(),
                        started.elapsed().as_secs_f64()
                    );
                    if let Err(e) = write_game(&folder, g, &samples, [a_i, b_i], stage) {
                        *error.lock().expect("no job panicked") = Some(e);
                        break;
                    }
                }
            });
        }
    });
    if let Some(e) = error.into_inner().map_err(|e| e.to_string())? {
        return Err(e);
    }
    println!(
        "{} samples in {:.0} s, written to {}",
        made.into_inner(),
        started.elapsed().as_secs_f64(),
        args.out.display()
    );
    Ok(())
}

/// Game `g`'s samples into the folder `dir` (written beside it, then renamed): A (`fighters[0]`) is in slot
/// `g % 2`.
fn write_game(dir: &Path, g: usize, samples: &Samples, fighters: [usize; 2], stage: usize) -> Result<(), String> {
    let tmp = dir.with_extension("part");
    std::fs::create_dir_all(&tmp).map_err(|e| format!("{}: {e}", tmp.display()))?;
    let (mut feats, mut scores, mut short, mut lead, mut meta) = (vec![], vec![], vec![], vec![], vec![]);
    for (slot, s) in samples {
        feats.extend_from_slice(&s.features);
        scores.extend_from_slice(&s.scores);
        short.extend(s.shortlist.iter().map(|&b| u8::from(b)));
        lead.extend(s.lead.iter().map(|&b| u8::from(b)));
        let fighter = if *slot == g % 2 { fighters[0] } else { fighters[1] };
        meta.extend([g as i32, *slot as i32, fighter as i32, stage as i32]);
    }
    let (n, f, a) = (samples.len(), expert::FEATURES, expert::ACTIONS);
    write_npy(&tmp.join("features.npy"), "<f4", n, f, &as_bytes_f32(&feats))?;
    write_npy(&tmp.join("scores.npy"), "<f4", n, a, &as_bytes_f32(&scores))?;
    write_npy(&tmp.join("shortlist.npy"), "|u1", n, a, &short)?;
    write_npy(&tmp.join("lead.npy"), "|u1", n, a, &lead)?;
    let meta_b: Vec<u8> = meta.iter().flat_map(|v: &i32| v.to_le_bytes()).collect();
    write_npy(&tmp.join("meta.npy"), "<i4", n, 4, &meta_b)?;
    std::fs::rename(&tmp, dir).map_err(|e| format!("{}: {e}", dir.display()))
}

fn as_bytes_f32(v: &[f32]) -> Vec<u8> {
    v.iter().flat_map(|x| x.to_le_bytes()).collect()
}

/// A 2-d `.npy` file (version 1.0).
fn write_npy(path: &Path, descr: &str, rows: usize, cols: usize, data: &[u8]) -> Result<(), String> {
    let mut h = format!("{{'descr': '{descr}', 'fortran_order': False, 'shape': ({rows}, {cols}), }}");
    while (10 + h.len() + 1) % 64 != 0 {
        h.push(' ');
    }
    h.push('\n');
    let mut f = std::io::BufWriter::new(std::fs::File::create(path).map_err(|e| format!("{}: {e}", path.display()))?);
    let len = u16::try_from(h.len()).map_err(|e| e.to_string())?;
    let mut head = b"\x93NUMPY\x01\x00".to_vec();
    head.extend_from_slice(&len.to_le_bytes());
    head.extend_from_slice(h.as_bytes());
    f.write_all(&head).and_then(|()| f.write_all(data)).map_err(|e| format!("{}: {e}", path.display()))
}

/// Game `g`: A (always an Expert) in slot `g % 2`, B at `level_b`; the samples of every Expert in it.
fn play(data: &Arc<mvs_data::GameData>, args: &Args, ab: [&str; 2], level_b: i32, g: usize) -> Result<Samples, String> {
    let a_slot = g % 2;
    let chars = if a_slot == 0 { ab } else { [ab[1], ab[0]] };
    let levels = if a_slot == 0 { [expert::EXPERT_LEVEL, level_b] } else { [level_b, expert::EXPERT_LEVEL] };
    let player = |i: usize| MatchPlayer {
        bot: true,
        difficulty: Some(mvs_sim::real(levels[i])),
        character: Some(chars[i].to_owned()),
        team: Some(u8::try_from(i).expect("two slots")),
        ..MatchPlayer::default()
    };
    let setup = MatchSetup {
        team_style: TeamStyle::OneVsOne,
        players: vec![player(0), player(1)],
        rules: Some(Rules { knockouts_to_win: mvs_sim::math::narrow(args.kos), duration: 420.0, stocks: false }),
        flow: true,
        ..MatchSetup::default()
    };
    let seed = (args.seed as i32).wrapping_mul(7919).wrapping_add(g as i32);
    let mut w = World::new(data.clone(), seed, &setup).map_err(|e| format!("{ab:?}: the bind: {e:?}"))?;
    let trees = Arc::new(data.bot_trees.clone());
    w.bot_trees = Some(trees.clone());
    let mut bots: Vec<BtBot> = (0..2)
        .map(|i| {
            let mut b = BtBot::match_bot(seed.wrapping_mul(31).wrapping_add(i), &trees, levels[i as usize]);
            if let Some(e) = &mut b.expert {
                e.set_lead(args.lead.clone());
                e.teach(args.every);
            }
            b
        })
        .collect();
    let cap = ((DURATION + 30.0) * 60.0) as usize + usize::try_from(mvs_sim::setup::MATCH_START_TICKS).unwrap_or(0);
    let mut inputs = [InputState::default(); 2];
    let mut out = Vec::new();
    for _ in 0..cap {
        for (i, bot) in bots.iter_mut().enumerate() {
            let me = w.players[i];
            inputs[i] = bot.think(&mut w, me);
            if let Some(e) = &mut bot.expert {
                out.extend(e.take_samples().into_iter().map(|s| (i, s)));
            }
        }
        w.step(&inputs);
        w.events.clear();
        if w.flow.match_over {
            break;
        }
    }
    Ok(out)
}
