//! `bot-arena`: bot against bot, many matches, headless on the dev store: who wins, and what a tick costs. The
//! yardstick of the Expert bot (`docs/expert-bot-plan.md`): every layer has to beat the one before it here, and the
//! look-ahead's budget is sized from the costs it prints.
//!
//!   bot-arena --a <fighter|all> --b <fighter|all> [--level-a <0..4>] [--level-b <0..4>] [--stage <key>]
//!             [--games <n>] [--kos <n>] [--jobs <n>] [--seed <n>] [--lead-a x4|net] [--lead-b x4|net] [--net <file>]
//!
//! Level 4 is Expert (`mvs_sim::ai::expert`): its think column then holds the look-ahead's cost. An Expert's lead
//! (`--lead-a`, `--lead-b`) is the level's own by default, else the hand-made shortlist (`x4`) or the network
//! (`net`: the built-in weights or `--net`'s file), so the two can meet head to head; a run with a network lead
//! first prints the network's cost per decision.
//! A game is a 1v1 custom match (intro and countdown included) to `--kos` ring-outs (2 by default), capped at its
//! 420 s clock. Even games put A in slot 0, odd games in slot 1, so neither side's start decides the rate. Each game
//! has its own World and bot seeds. Per pairing it prints A's wins, B's wins and the games that ran out of time, the
//! mean ring-outs and length, then the costs per tick over every tick of every game: the World's step, A's think
//! and B's, and a `clone_from` of the World into a scratch World (the look-ahead's copy), sampled every 10th tick.
//! A game whose sim panics counts as a crash: the line names the crashes, and the first one's message and seed
//! (`--seed` and the game's number) are printed under the table, so the arena is a crash finder too.
//! Runs at idle priority; `--jobs` games at once over every pairing (a quarter of the logical processors by default).

use mvs_sim::ai::bt::BtBot;
use mvs_sim::input::InputState;
use mvs_sim::setup::{MatchPlayer, MatchSetup, Rules, TeamStyle};
use mvs_sim::world::World;
use std::process::ExitCode;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Instant;

const USAGE: &str = "usage: bot-arena --a <fighter|all> --b <fighter|all> [--level-a <0..4>] [--level-b <0..4>] \
                     [--stage <key>] [--games <n>] [--kos <n>] [--jobs <n>] [--seed <n>] \
                     [--lead-a x4|net] [--lead-b x4|net] [--net <file>]";

/// The match clock in seconds; a game is cut 30 s past it.
const DURATION: f64 = 420.0;
/// Every how many ticks the scratch copy is timed.
const CLONE_EVERY: usize = 10;

struct Args {
    a: String,
    b: String,
    level_a: i32,
    level_b: i32,
    stage: String,
    games: usize,
    kos: f64,
    jobs: usize,
    seed: i32,
    /// The Experts' leads, A's and B's (`None`: the level's own).
    leads: [Option<mvs_sim::ai::expert::Lead>; 2],
}

/// One game's outcome and its timings (microseconds).
#[derive(Default)]
struct Game {
    /// 0 A won, 1 B won, 2 the clock ran out, 3 the sim panicked.
    result: usize,
    /// The panic's message and the game's number (result 3).
    crash: Option<(String, usize)>,
    kos: [f64; 2],
    ticks: usize,
    step: Vec<f32>,
    /// A's thinks and B's.
    think: [Vec<f32>; 2],
    clone: Vec<f32>,
    /// A's habits and B's.
    habits: [Habits; 2],
}

/// The ticks after an attack's press within which a hit of the fighter's counts as the press's.
const WHIFF_WINDOW: usize = 40;

/// What a side did that a player can exploit (`docs/expert-bot-plan.md` "X7"): attacks and specials pressed in the
/// air, those of them that dealt no damage within `WHIFF_WINDOW` and the attacks among them pressed with the
/// target out of reach (`mvs_sim::ai::expert::air_out_of_reach`); stretches off the stage out of hitstun (a
/// recovery), the air dodges made in them and the hits taken in them.
#[derive(Default, Clone, Copy)]
struct Habits {
    air_presses: u32,
    air_whiffs: u32,
    far_attacks: u32,
    recoveries: u32,
    recovery_dodges: u32,
    recovery_hits: u32,
}

impl Habits {
    fn add(&mut self, o: &Habits) {
        self.air_presses += o.air_presses;
        self.air_whiffs += o.air_whiffs;
        self.far_attacks += o.far_attacks;
        self.recoveries += o.recoveries;
        self.recovery_dodges += o.recovery_dodges;
        self.recovery_hits += o.recovery_hits;
    }

    /// `whiff %, whiffs a minute, out-of-reach air attacks a minute, dodges and hits a recovery`, over `ticks`.
    fn line(&self, ticks: usize) -> String {
        let pc = |a: u32, b: u32| if b == 0 { 0.0 } else { 100.0 * f64::from(a) / f64::from(b) };
        let per = |a: u32, b: u32| if b == 0 { 0.0 } else { f64::from(a) / f64::from(b) };
        #[allow(clippy::cast_precision_loss)]
        let mins = (ticks as f64 / 3600.0).max(1e-9);
        format!(
            "whiff {:>3.0}% {:>5.1}/min, far {:>5.1}/min, recovery: {:>4.2} dodges {:>4.2} hits ({} recoveries)",
            pc(self.air_whiffs, self.air_presses),
            f64::from(self.air_whiffs) / mins,
            f64::from(self.far_attacks) / mins,
            per(self.recovery_dodges, self.recoveries),
            per(self.recovery_hits, self.recoveries),
            self.recoveries
        )
    }
}

/// One side's habit tracking over a game.
#[derive(Default)]
struct Watch {
    last: InputState,
    /// The presses not yet judged: their tick and the fighter's damage dealt then.
    pending: Vec<(usize, f32)>,
    recovering: bool,
    damage: f32,
    /// The fighter's air dodges used, last tick.
    evades: i32,
}

impl Watch {
    /// Tick `t`'s input `i` of fighter `f`, given before the step; `h` its side's counts.
    fn before(&mut self, w: &World, f: mvs_sim::fighter::FighterId, t: usize, i: InputState, h: &mut Habits) {
        let x = w.fighter(f);
        let dealt = x.stats.damage_dealt;
        self.pending.retain(|&(t0, d0)| {
            if t - t0 < WHIFF_WINDOW {
                return true;
            }
            if dealt <= d0 {
                h.air_whiffs += 1;
            }
            false
        });
        let air = !x.body.on_floor && x.life.dead == 0.0;
        if air && ((i.attack && !self.last.attack) || (i.special && !self.last.special)) {
            h.air_presses += 1;
            self.pending.push((t, dealt));
            if i.attack && !self.last.attack && mvs_sim::ai::expert::air_out_of_reach(w, f) {
                h.far_attacks += 1;
            }
        }
        let g = &w.stage.ground;
        let off = air
            && (x.body.x < mvs_sim::math::narrow(g.left)
                || x.body.x > mvs_sim::math::narrow(g.right)
                || x.body.y < mvs_sim::math::narrow(g.top) - 50.0);
        let recovering = off && !mvs_sim::fighter::in_hitstun(w, f);
        if recovering && !self.recovering {
            h.recoveries += 1;
        }
        // a stretch lasts through the hitstun of a hit taken in it, until the fighter is back or out
        self.recovering = recovering || (self.recovering && off);
        if self.recovering && x.air.air_evades > self.evades {
            h.recovery_dodges += 1;
        }
        if self.recovering && x.stats.damage > self.damage {
            h.recovery_hits += 1;
        }
        self.damage = x.stats.damage;
        self.evades = x.air.air_evades;
        self.last = i;
    }
}

fn main() -> ExitCode {
    match main_() {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("bot-arena: {e}");
            ExitCode::from(2)
        }
    }
}

fn args() -> Result<Args, String> {
    let mut a = Args {
        a: String::new(),
        b: String::new(),
        level_a: 3,
        level_b: 3,
        stage: mvs_sim_trace::TRACE_STAGE.to_owned(),
        games: 20,
        kos: 2.0,
        jobs: std::thread::available_parallelism().map_or(1, |n| (n.get() / 4).max(1)),
        seed: 1,
        leads: [None, None],
    };
    let mut lead: [Option<String>; 2] = [None, None];
    let mut net: Option<String> = None;
    let mut it = std::env::args().skip(1);
    while let Some(s) = it.next() {
        let mut val = || it.next().ok_or(format!("{s} needs a value"));
        let num = |v: String| v.parse::<i64>().map_err(|_| format!("{s}: not a number: {v}"));
        match s.as_str() {
            "--a" => a.a = val()?,
            "--b" => a.b = val()?,
            "--level-a" => a.level_a = i32::try_from(num(val()?)?).map_err(|e| e.to_string())?,
            "--level-b" => a.level_b = i32::try_from(num(val()?)?).map_err(|e| e.to_string())?,
            "--stage" => a.stage = val()?,
            "--games" => a.games = usize::try_from(num(val()?)?).map_err(|e| e.to_string())?,
            #[allow(clippy::cast_precision_loss)]
            "--kos" => a.kos = num(val()?)? as f64,
            "--jobs" => a.jobs = usize::try_from(num(val()?)?).map_err(|e| e.to_string())?.max(1),
            "--seed" => a.seed = i32::try_from(num(val()?)?).map_err(|e| e.to_string())?,
            "--lead-a" => lead[0] = Some(val()?),
            "--lead-b" => lead[1] = Some(val()?),
            "--net" => net = Some(val()?),
            _ => return Err(USAGE.to_owned()),
        }
    }
    if a.a.is_empty() || a.b.is_empty() {
        return Err(USAGE.to_owned());
    }
    for (l, s) in a.leads.iter_mut().zip(&lead) {
        if let Some(s) = s {
            *l = Some(mvs_sim_trace::expert_lead(s, net.as_deref())?);
        }
    }
    Ok(a)
}

fn main_() -> Result<(), String> {
    let args = args()?;
    mvs_sim_trace::machine::lower_priority();
    let dir = mvs_store::dev_store_dir();
    let store = mvs_store::Store::open(&dir).map_err(|e| format!("the store at {}: {e:?}", dir.display()))?;
    let roster = mvs_data::shared::<mvs_data::Index>(&store, mvs_data::path::INDEX)
        .map_err(|e| format!("the gamedata index: {e:?}"))?
        .characters
        .clone();
    let side = |s: &str| if s == "all" { roster.clone() } else { vec![s.to_owned()] };
    println!(
        "bot-arena: level {} vs level {}, {} games a pairing to {} ring-outs on {}, {} at once",
        args.level_a, args.level_b, args.games, args.kos, args.stage, args.jobs
    );
    for (side, l) in ["A", "B"].iter().zip(&args.leads) {
        if let Some(mvs_sim::ai::expert::Lead::Net(net)) = l {
            println!("{side} is network-led: the network costs {:.1} us a decision", net_cost(net));
        }
    }
    println!(
        "{:<28} {:>5} {:>5} {:>5} {:>5} {:>9} {:>7} | {:>14} {:>14} {:>14} {:>14}",
        "A vs B", "A", "B", "time", "crash", "ko A:B", "secs", "step us", "think A us", "think B us", "clone us"
    );
    let pairs: Vec<(String, String)> =
        side(&args.a).into_iter().flat_map(|a| side(&args.b).into_iter().map(move |b| (a.clone(), b))).collect();
    // (a game's panic is caught and counted; the default hook would print each one with its backtrace)
    std::panic::set_hook(Box::new(|_| {}));
    let mut totals = Totals::default();
    let printed = std::sync::Mutex::new((0, &mut totals));
    run_all(&store, &args, &pairs, |done: &[Option<Vec<Game>>]| {
        let mut p = printed.lock().expect("no printer panicked");
        while let Some(Some(games)) = done.get(p.0) {
            let (a, b) = &pairs[p.0];
            print_pairing(a, b, games, p.1);
            p.0 += 1;
        }
    })?;
    let Totals { mut all_step, mut all_think, mut all_clone, crashes, habits, ticks } = totals;
    println!(
        "{:<28} {:>47} | {} {} {} {}",
        "all",
        "",
        stats(&mut all_step),
        stats(&mut all_think[0]),
        stats(&mut all_think[1]),
        stats(&mut all_clone)
    );
    println!("(each cost: mean / p99 in microseconds)");
    println!("A: {}", habits[0].line(ticks));
    println!("B: {}", habits[1].line(ticks));
    if let Some(first) = crashes.first() {
        println!("{} crashed games (seed {}); the first: {first}", crashes.len(), args.seed);
    }
    Ok(())
}

/// Every pairing's costs and crashes together.
#[derive(Default)]
struct Totals {
    all_step: Vec<f32>,
    all_think: [Vec<f32>; 2],
    all_clone: Vec<f32>,
    crashes: Vec<String>,
    habits: [Habits; 2],
    ticks: usize,
}

/// One pairing's line, its costs and crashes added to `t`.
fn print_pairing(a: &str, b: &str, games: &[Game], t: &mut Totals) {
    let mut wins = [0; 4];
    let (mut ko, mut ticks) = ([0.0; 2], 0);
    let (mut step, mut think, mut clone) = (Vec::new(), [Vec::new(), Vec::new()], Vec::new());
    for g in games {
        wins[g.result] += 1;
        if let Some((m, n)) = &g.crash {
            t.crashes.push(format!("{a} vs {b}, game {n}: {m}"));
        }
        ko[0] += g.kos[0];
        ko[1] += g.kos[1];
        ticks += g.ticks;
        step.extend_from_slice(&g.step);
        think[0].extend_from_slice(&g.think[0]);
        think[1].extend_from_slice(&g.think[1]);
        clone.extend_from_slice(&g.clone);
        t.habits[0].add(&g.habits[0]);
        t.habits[1].add(&g.habits[1]);
    }
    t.ticks += ticks;
    #[allow(clippy::cast_precision_loss)]
    let n = games.len().max(1) as f64;
    #[allow(clippy::cast_precision_loss)]
    let secs = ticks as f64 / n / 60.0;
    println!(
        "{:<28} {:>5} {:>5} {:>5} {:>5} {:>4.1}:{:<4.1} {:>7.0} | {} {} {} {}",
        format!("{a} vs {b}"),
        wins[0],
        wins[1],
        wins[2],
        wins[3],
        ko[0] / n,
        ko[1] / n,
        secs,
        stats(&mut step),
        stats(&mut think[0]),
        stats(&mut think[1]),
        stats(&mut clone)
    );
    t.all_step.extend(step);
    let [ta, tb] = think;
    t.all_think[0].extend(ta);
    t.all_think[1].extend(tb);
    t.all_clone.extend(clone);
}

/// `mean/p99` of `v`, sorted in place.
fn stats(v: &mut [f32]) -> String {
    if v.is_empty() {
        return format!("{:>14}", "-");
    }
    v.sort_by(f32::total_cmp);
    #[allow(clippy::cast_precision_loss)]
    let mean = v.iter().map(|&x| f64::from(x)).sum::<f64>() / v.len() as f64;
    let p99 = v[(v.len() * 99 / 100).min(v.len() - 1)];
    format!("{:>14}", format!("{mean:.0}/{p99:.0}"))
}

/// One pairing's games so far: its data (loaded by the first game to need it, dropped after its last) and results.
#[derive(Default)]
struct Slot {
    data: Option<Arc<mvs_data::GameData>>,
    games: Vec<Game>,
}

/// Every game of every pairing, `jobs` at once over the whole list (a pairing's games and the next pairing's run
/// together, so a run of few games a pairing still fills the machine). `done` is called after each finished pairing
/// with every pairing's games once all of its games are in (`None` before), in any order.
fn run_all(
    store: &mvs_store::Store,
    args: &Args,
    pairs: &[(String, String)],
    done: impl Fn(&[Option<Vec<Game>>]) + Sync,
) -> Result<(), String> {
    let slots: Vec<std::sync::Mutex<Slot>> = pairs.iter().map(|_| std::sync::Mutex::new(Slot::default())).collect();
    let finished = std::sync::Mutex::new((0..pairs.len()).map(|_| None).collect::<Vec<_>>());
    let error = std::sync::Mutex::new(None);
    let next = AtomicUsize::new(0);
    let items = pairs.len() * args.games;
    std::thread::scope(|s| {
        for _ in 0..args.jobs.min(items) {
            s.spawn(|| {
                loop {
                    let i = next.fetch_add(1, Ordering::Relaxed);
                    if i >= items || error.lock().expect("no job panicked").is_some() {
                        break;
                    }
                    let (p, g) = (i / args.games, i % args.games);
                    let (a, b) = (&pairs[p].0, &pairs[p].1);
                    let data = {
                        let mut slot = slots[p].lock().expect("no job panicked");
                        if slot.data.is_none() {
                            let keys = if a == b { vec![a.as_str()] } else { vec![a.as_str(), b.as_str()] };
                            match mvs_data::GameData::load(store, &keys, &args.stage) {
                                Ok(d) => slot.data = Some(Arc::new(d)),
                                Err(e) => {
                                    *error.lock().expect("no job panicked") = Some(format!("{a} vs {b}: {e:?}"));
                                    break;
                                }
                            }
                        }
                        slot.data.clone().expect("loaded")
                    };
                    let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| play(&data, args, a, b, g)))
                        .unwrap_or_else(|e| {
                            let m = e
                                .downcast_ref::<&str>()
                                .map(|s| (*s).to_owned())
                                .or_else(|| e.downcast_ref::<String>().cloned())
                                .unwrap_or_default();
                            Ok(Game { result: 3, crash: Some((m, g)), ..Game::default() })
                        });
                    let game = match r {
                        Ok(game) => game,
                        Err(e) => {
                            *error.lock().expect("no job panicked") = Some(e);
                            break;
                        }
                    };
                    let complete = {
                        let mut slot = slots[p].lock().expect("no job panicked");
                        slot.games.push(game);
                        (slot.games.len() == args.games).then(|| {
                            slot.data = None;
                            std::mem::take(&mut slot.games)
                        })
                    };
                    if let Some(games) = complete {
                        let mut f = finished.lock().expect("no job panicked");
                        f[p] = Some(games);
                        done(&f);
                    }
                }
            });
        }
    });
    error.into_inner().map_err(|e| e.to_string())?.map_or(Ok(()), Err)
}

/// Game `g`: A in slot `g % 2`.
fn play(data: &Arc<mvs_data::GameData>, args: &Args, a: &str, b: &str, g: usize) -> Result<Game, String> {
    let a_slot = g % 2;
    let gi = i32::try_from(g).map_err(|e| e.to_string())?;
    let (chars, levels) =
        if a_slot == 0 { ([a, b], [args.level_a, args.level_b]) } else { ([b, a], [args.level_b, args.level_a]) };
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
    let seed = args.seed.wrapping_mul(7919).wrapping_add(gi);
    let mut w = World::new(data.clone(), seed, &setup).map_err(|e| format!("{a} vs {b}: the bind: {e:?}"))?;
    let trees = Arc::new(data.bot_trees.clone());
    w.bot_trees = Some(trees.clone());
    let leads = if a_slot == 0 { [&args.leads[0], &args.leads[1]] } else { [&args.leads[1], &args.leads[0]] };
    let mut bots: Vec<BtBot> = (0..2)
        .map(|i| {
            let mut b = BtBot::match_bot(seed.wrapping_mul(31).wrapping_add(i), &trees, levels[i as usize]);
            if let (Some(e), Some(l)) = (&mut b.expert, leads[i as usize]) {
                e.set_lead(l.clone());
            }
            b
        })
        .collect();
    let mut scratch = w.clone();
    let mut game = Game::default();
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let cap = ((DURATION + 30.0) * 60.0) as usize + usize::try_from(mvs_sim::setup::MATCH_START_TICKS).unwrap_or(0);
    let mut inputs = [InputState::default(); 2];
    let mut watch = [Watch::default(), Watch::default()];
    for t in 0..cap {
        for (i, bot) in bots.iter_mut().enumerate() {
            let me = w.players[i];
            let at = Instant::now();
            inputs[i] = bot.think(&mut w, me);
            let side = usize::from(i != a_slot);
            game.think[side].push(micros(at));
            watch[i].before(&w, me, t, inputs[i], &mut game.habits[side]);
        }
        let at = Instant::now();
        w.step(&inputs);
        game.step.push(micros(at));
        w.events.clear();
        if t % CLONE_EVERY == 0 {
            let at = Instant::now();
            scratch.clone_from(&w);
            game.clone.push(micros(at));
        }
        game.ticks = t + 1;
        if w.flow.match_over {
            break;
        }
    }
    let score = |slot: usize| w.flow.team_knockouts.get(slot).map_or(0.0, |&k| f64::from(k));
    game.kos = [score(a_slot), score(1 - a_slot)];
    game.result = if !w.flow.match_over {
        2
    } else if usize::try_from(w.flow.winning_team).ok() == Some(a_slot) {
        0
    } else {
        1
    };
    Ok(game)
}

/// The network's mean cost of one run in microseconds, over inputs of the right count.
fn net_cost(net: &mvs_sim::ai::expert::net::Net) -> f64 {
    const RUNS: u32 = 20_000;
    #[allow(clippy::cast_precision_loss)]
    let x: Vec<f32> = (0..net.inputs()).map(|i| if i % 3 == 0 { 0.0 } else { (i % 7) as f32 * 0.1 - 0.3 }).collect();
    let (mut out, mut scratch) = (Vec::new(), Vec::new());
    let at = Instant::now();
    let mut sum = 0.0;
    for _ in 0..RUNS {
        net.run(std::hint::black_box(&x), &mut out, &mut scratch);
        sum += out[0];
    }
    std::hint::black_box(sum);
    at.elapsed().as_secs_f64() * 1e6 / f64::from(RUNS)
}

#[allow(clippy::cast_possible_truncation)]
fn micros(at: Instant) -> f32 {
    (at.elapsed().as_secs_f64() * 1e6) as f32
}
