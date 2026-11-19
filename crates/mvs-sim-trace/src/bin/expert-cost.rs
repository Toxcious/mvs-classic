//! `expert-cost`: what Expert bots cost a 60 Hz frame, headless on the dev store: a match of Expert bots stepped one
//! tick a frame at the real pace, the frame's thread's time per frame (the bots' thinks and the World's step), with
//! the decisions inline or on workers (`mvs_sim::ai::expert::Workers`), then one catch-up frame that runs many ticks
//! at once (a game that stood still).
//!
//!   expert-cost [--fighters <a,b,..>] [--workers <n>] [--throttle <k>] [--ticks <n>] [--catch-up <n>] [--seed <n>]
//!
//! Two fighters play a 1v1, three or four a free-for-all (Shaggy, Rick, Iron Giant by default; every bot Expert).
//! `--workers 0` decides inline (the default: the PC's pool, `Workers::for_this_pc`). `--throttle k` stretches each
//! frame's time k times by a spin on the frame's thread, as the game's `--throttle` (a weak PC's main thread; the
//! workers are not slowed). Prints the frame time's mean / p50 / p99 / max, the frames past 16.7 ms, and the
//! catch-up frame's time. The inputs are hashed: the same run inline and on workers prints the same hash.

use mvs_sim::ai::bt::BtBot;
use mvs_sim::ai::expert::{EXPERT_LEVEL, Workers};
use mvs_sim::input::InputState;
use mvs_sim::setup::{MatchPlayer, MatchSetup, Rules, TeamStyle};
use mvs_sim::world::World;
use std::process::ExitCode;
use std::sync::Arc;
use std::time::{Duration, Instant};

const USAGE: &str = "usage: expert-cost [--fighters <a,b,..>] [--workers <n>] [--throttle <k>] [--ticks <n>] \
                     [--catch-up <n>] [--seed <n>]";

/// A 60 Hz frame.
const FRAME: Duration = Duration::from_nanos(16_666_667);

struct Args {
    fighters: Vec<String>,
    workers: Option<usize>,
    throttle: f64,
    ticks: usize,
    catch_up: usize,
    seed: i32,
}

fn main() -> ExitCode {
    match main_() {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("expert-cost: {e}");
            ExitCode::from(2)
        }
    }
}

fn args() -> Result<Args, String> {
    let mut a = Args {
        fighters: ["shaggy", "rick", "irongiant"].map(str::to_owned).to_vec(),
        workers: None,
        throttle: 1.0,
        ticks: 3600,
        catch_up: 170,
        seed: 1,
    };
    let mut it = std::env::args().skip(1);
    while let Some(k) = it.next() {
        let mut v = || it.next().ok_or(format!("{k}: no value\n{USAGE}"));
        let bad = |k: &str| format!("{k}: not a number\n{USAGE}");
        match k.as_str() {
            "--fighters" => a.fighters = v()?.split(',').map(str::to_owned).collect(),
            "--workers" => a.workers = Some(v()?.parse().map_err(|_| bad(&k))?),
            "--throttle" => a.throttle = v()?.parse::<f64>().map_err(|_| bad(&k))?.max(1.0),
            "--ticks" => a.ticks = v()?.parse().map_err(|_| bad(&k))?,
            "--catch-up" => a.catch_up = v()?.parse().map_err(|_| bad(&k))?,
            "--seed" => a.seed = v()?.parse().map_err(|_| bad(&k))?,
            _ => return Err(USAGE.to_owned()),
        }
    }
    if !(2..=4).contains(&a.fighters.len()) {
        return Err(format!("two to four fighters\n{USAGE}"));
    }
    Ok(a)
}

fn main_() -> Result<(), String> {
    let args = args()?;
    let store = mvs_store::Store::open(mvs_store::dev_store_dir()).map_err(|e| e.to_string())?;
    let mut keys: Vec<&str> = args.fighters.iter().map(String::as_str).collect();
    keys.sort_unstable();
    keys.dedup();
    let data = Arc::new(
        mvs_data::GameData::load(&store, &keys, mvs_sim_trace::TRACE_STAGE).map_err(|e| format!("the data: {e}"))?,
    );
    let n = args.fighters.len();
    let player = |i: usize| MatchPlayer {
        bot: true,
        difficulty: Some(mvs_sim::real(EXPERT_LEVEL)),
        character: Some(args.fighters[i].clone()),
        team: Some(u8::try_from(i).expect("four slots")),
        ..MatchPlayer::default()
    };
    let setup = MatchSetup {
        team_style: if n == 2 { TeamStyle::OneVsOne } else { TeamStyle::FreeForAll },
        players: (0..n).map(player).collect(),
        rules: Some(Rules { knockouts_to_win: 99.0, duration: 420.0, stocks: false }),
        flow: true,
        ..MatchSetup::default()
    };
    let mut w = World::new(data.clone(), args.seed, &setup).map_err(|e| format!("the bind: {e:?}"))?;
    let trees = Arc::new(data.bot_trees.clone());
    w.bot_trees = Some(trees.clone());
    let workers = match args.workers {
        Some(0) => None,
        Some(k) => Some(Arc::new(Workers::new(k))),
        None => Workers::for_this_pc().map(Arc::new),
    };
    let mut bots: Vec<BtBot> = (0..n)
        .map(|i| {
            let mut b = BtBot::match_bot(
                args.seed.wrapping_mul(31).wrapping_add(i32::try_from(i).expect("four slots")),
                &trees,
                EXPERT_LEVEL,
            );
            b.set_workers(workers.as_ref());
            b
        })
        .collect();
    let mut inputs = vec![InputState::default(); n];
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    let mut tick = |w: &mut World, bots: &mut [BtBot], hash: &mut u64| {
        for (i, bot) in bots.iter_mut().enumerate() {
            let me = w.players[i];
            inputs[i] = bot.think(w, me);
            for b in format!("{:?}", inputs[i]).bytes() {
                *hash = (*hash ^ u64::from(b)).wrapping_mul(0x0100_0000_01b3);
            }
        }
        w.step(&inputs);
        w.events.clear();
    };
    let throttle = |cpu: Duration| {
        if args.throttle > 1.0 {
            let until = Instant::now() + cpu.mul_f64(args.throttle - 1.0);
            while Instant::now() < until {
                std::hint::spin_loop();
            }
        }
    };
    let mut ms = Vec::with_capacity(args.ticks);
    let mut next = Instant::now();
    for _ in 0..args.ticks {
        let at = Instant::now();
        tick(&mut w, &mut bots, &mut hash);
        throttle(at.elapsed());
        ms.push(at.elapsed().as_secs_f64() * 1000.0);
        next += FRAME;
        let now = Instant::now();
        if next > now {
            std::thread::sleep(next - now);
        } else {
            next = now;
        }
    }
    let at = Instant::now();
    for _ in 0..args.catch_up {
        tick(&mut w, &mut bots, &mut hash);
    }
    throttle(at.elapsed());
    let catch_up = at.elapsed().as_secs_f64() * 1000.0;
    let over = ms.iter().filter(|&&m| m > 1000.0 / 60.0).count();
    ms.sort_by(f64::total_cmp);
    #[allow(clippy::cast_precision_loss, reason = "a count of frames")]
    let mean = ms.iter().sum::<f64>() / ms.len().max(1) as f64;
    let at_q = |q: usize| ms.get((ms.len() * q / 100).min(ms.len().saturating_sub(1))).copied().unwrap_or(0.0);
    println!(
        "{} Expert bots ({}), {}, throttle {}: {} frames {mean:.2} / {:.2} / {:.2} / {:.2} ms (mean / p50 / p99 / max), \
         {over} past 16.7 ms; catch-up of {} ticks {catch_up:.1} ms; inputs {hash:016x}",
        n,
        args.fighters.join(","),
        workers.as_ref().map_or("inline".to_owned(), |w| format!("{} workers", w.threads())),
        args.throttle,
        ms.len(),
        at_q(50),
        at_q(99),
        ms.last().copied().unwrap_or(0.0),
        args.catch_up,
    );
    Ok(())
}
