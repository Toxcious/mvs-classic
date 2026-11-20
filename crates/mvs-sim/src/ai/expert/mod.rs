//! The Expert bot's look-ahead: the port's own level above the beta's Hard (`docs/expert-bot-plan.md`, "X3: the
//! look-ahead, as built"). Not in the build: no level of the beta plans ahead.
//!
//! An Expert bot is the Hard bot ([`BtBot`] at level 3) carrying an [`Expert`]. The tree thinks every tick, so its
//! blackboard stays current and its input is the floor. When the bot can act and nothing is queued past the reaction
//! delay, the Expert copies the world into a scratch World of its own and decides there: it steps the copy
//! [`REACTION`] ticks with the inputs the bot will give meanwhile, then plays each candidate of a shortlist
//! ("what the tree does" and the five input scripts of the action list, `actions`, its network rates best: `net`,
//! `features`; [`Lead`]) [`HORIZON`] ticks from there, the other players
//! played by Hard trees standing in for them, scores each end (`score`), and queues the best from `REACTION` ticks
//! after the tick it decided on. The bot gives a queued plan's input where one covers the tick, else the tree's.
//! A decision is a resumable job (`job`): each think from `t` on advances it by a share of the budget, so it is done
//! by the think of `t + REACTION - 1` and no single think carries it whole; it is taken at that think, the last
//! before its plan starts, whenever it finished. One job runs at a time. Given workers (`worker`), the job runs whole
//! on one instead, and that think waits for it if it is not back.
//!
//! The decision is a function of the world's copy and the Expert's own state alone (its random stream, its queue,
//! its stand-ins and scratch worlds): the real World is read once per tick by a copy and never written, and its
//! random stream is never drawn. The cost is a fixed count of simulated ticks ([`BUDGET`]), the same on every PC.
//! The scratch steps' events are dropped and the bot log is held off while the scratch trees think.

mod actions;
mod addons;
mod features;
mod job;
pub mod net;
mod plan;

pub use plan::air_out_of_reach;
mod score;
mod worker;

pub use worker::{MAX_THREADS, Workers};

use crate::Real;
use crate::ai::bt::BtBot;
use crate::fighter::FighterId;
use crate::input::{InputState, NO_INPUT};
use crate::rng::Rng;
use crate::world::World;
use job::Job;
use net::Net;
use plan::Queued;
use std::sync::Arc;

/// The action list's length (the network's outputs) and the network's input count.
pub const ACTIONS: usize = actions::COUNT;
pub const FEATURES: usize = features::COUNT;

/// The action list's entries by name, in its order.
pub fn action_names() -> Vec<String> {
    actions::ACTIONS.iter().map(|a| format!("{a:?}")).collect()
}

/// `MatchPlayer::difficulty` / `BtBot::match_bot`'s level of the Expert bot (0..3 are the beta's).
pub const EXPERT_LEVEL: i32 = 4;
/// The reaction delay in ticks: a decision made from the world at tick `t` is given from `t + REACTION`.
pub const REACTION: i32 = 10;
/// The ticks each candidate is played ahead, from the plan's first tick.
pub const HORIZON: i32 = 15;
/// The most candidates a decision weighs ("what the tree does" included).
pub const MAX_CANDIDATES: usize = 6;
/// The guesses of the other players a candidate is played against at most: their Hard trees, and the attacker
/// (`plan::chase`), where it can matter (`job`).
pub const GUESSES: i32 = 2;
/// The simulated ticks a decision costs at most: the reaction delay's and every candidate's under every guess.
pub const BUDGET: i32 = REACTION + HORIZON * 6 * GUESSES;
const _: () = assert!(BUDGET <= 200 && MAX_CANDIDATES == 6);
/// The level of the trees standing in for the other players: Hard.
const STAND_IN_LEVEL: i32 = 3;
/// The Expert's random stream is the bot's seed with this mixed in, apart from the tree's own stream.
const SEED_MIX: i32 = 0x2f6b_1d37;

/// Who picks the candidates a decision weighs.
#[derive(Clone, Debug)]
pub enum Lead {
    /// The hand-made shortlist by situation (`plan::shortlist`): the Expert before its network, kept for the
    /// arena (`bot-arena --lead-b x4`) and the network's training.
    Shortlist,
    /// The network's best actions (`net`): the Expert's own.
    Net(Arc<Net>),
}

/// A teacher's sample (`Expert::teach`): the network's input at a decision's base and every action's score there.
#[derive(Clone, Debug)]
pub struct Sample {
    pub features: Vec<Real>,
    /// By action: the look-ahead's score (no tie noise).
    pub scores: Vec<Real>,
    /// By action: the hand-made shortlist picks it, and the decision's lead picked it.
    pub shortlist: Vec<bool>,
    pub lead: Vec<bool>,
}

/// The Expert layer of a bot. Cloning one clones its scratch worlds and its job whole: a clone plans exactly as the
/// original would (`World::clone_from` keeps a destination's own value of the fields the rollback leaves out, so a
/// scratch World made again could differ in them). A job on a worker is waited for and copied back.
pub struct Expert {
    seed: i32,
    rng: Rng,
    /// The bot's thinks so far.
    tick: i32,
    /// The plans queued, in order and without overlap, each from its first tick.
    queue: Vec<Queued>,
    /// Hard trees standing in for the other players, by slot (`None` at the bot's own slot), thinking every tick in
    /// `warm` so their blackboards stay current.
    stand_ins: Vec<Option<BtBot>>,
    /// The inputs of this tick by slot: the stand-ins' and the bot's own.
    inputs: Vec<InputState>,
    /// The real World copied each tick, where the stand-ins think.
    warm: Option<World>,
    /// The decision under way, if any (one at a time); a finished one is kept for its memory.
    job: Option<Box<Job>>,
    running: bool,
    /// The think the decision under way started at (its `t`).
    started: i32,
    /// The job, while it is on a worker (`job` is `None` then); a given-up one stays here until it comes back.
    away: Option<worker::Ticket>,
    workers: Option<Arc<Workers>>,
    lead: Lead,
    /// Every this many decisions (0: never) a teacher's sample is taken (`teach`), and the decisions so far.
    teach_every: u32,
    decisions: u32,
    samples: Vec<Sample>,
}

impl std::fmt::Debug for Expert {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Expert")
            .field("tick", &self.tick)
            .field("queue", &self.queue)
            .field("running", &self.running)
            .finish_non_exhaustive()
    }
}

impl Clone for Expert {
    fn clone(&self) -> Expert {
        // (a job on a worker comes home in the clone, finished or given up)
        let job = match &self.away {
            Some(away) => Some(away.copy()),
            None => self.job.clone(),
        };
        Expert {
            seed: self.seed,
            rng: self.rng.clone(),
            tick: self.tick,
            queue: self.queue.clone(),
            stand_ins: self.stand_ins.clone(),
            inputs: self.inputs.clone(),
            warm: self.warm.clone(),
            job,
            running: self.running,
            started: self.started,
            away: None,
            workers: self.workers.clone(),
            lead: self.lead.clone(),
            teach_every: self.teach_every,
            decisions: self.decisions,
            samples: self.samples.clone(),
        }
    }
}

/// `dst` made a copy of `src`: `clone_from` for the rollback's fields, and the fields it leaves out that a step
/// reads (the data, the hooks, the pawns' controllers); the copy's events are dropped.
fn copy_world(dst: &mut Option<World>, src: &World) {
    match dst {
        Some(d) => {
            d.clone_from(src);
            d.data.clone_from(&src.data);
            d.names.clone_from(&src.names);
            d.setup.clone_from(&src.setup);
            d.registry = src.registry;
            d.fixed.clone_from(&src.fixed);
            d.mode = src.mode;
            d.bot_trees.clone_from(&src.bot_trees);
            d.predicted = src.predicted;
            d.pawn_ctl.clone_from(&src.pawn_ctl);
            d.warned.borrow_mut().clone_from(&src.warned.borrow());
            d.checked.borrow_mut().clone_from(&src.checked.borrow());
        }
        None => *dst = Some(src.clone()),
    }
    if let Some(d) = dst {
        d.events.clear();
    }
}

/// `dst` made a copy of `src`.
fn copy_bot(dst: &mut Option<BtBot>, src: Option<&BtBot>) {
    match (dst.as_mut(), src) {
        (Some(d), Some(s)) => d.clone_from(s),
        (_, s) => *dst = s.cloned(),
    }
}

fn copy_bots(dst: &mut Vec<Option<BtBot>>, src: &[Option<BtBot>]) {
    dst.resize_with(src.len(), || None);
    for (d, s) in dst.iter_mut().zip(src) {
        copy_bot(d, s.as_ref());
    }
}

/// The input of fighter `me` queued for tick `t`, else the tree's.
fn queued_input(queue: &[Queued], t: i32, tree: InputState, w: &World, me: FighterId) -> InputState {
    queue.iter().find(|q| q.start <= t && t < q.end()).map_or(tree, |q| {
        let k = t - q.start;
        let chase = if matches!(q.plan, plan::Plan::Chase { .. }) { plan::chase(w, me, k) } else { NO_INPUT };
        q.plan.input(k, tree, chase)
    })
}

/// The bot can act on a plan: the match runs (with the flow: the input opens by the plan's first tick) and its
/// fighter is alive.
fn can_plan(w: &World, me: FighterId) -> bool {
    let open = !w.setup.flow
        || i64::from(w.flow.intro_ticks) + i64::from(REACTION) >= i64::from(crate::setup::INPUT_OPEN_TICKS);
    open && !w.flow.match_over && w.fighter(me).life.dead == 0.0
}

/// One step of a scratch world: the bot's input `mine`, each stand-in's think, then the step; its events dropped.
/// With `attacker` (the rollout's tick), the other teams' players chase the bot instead (`plan::chase`).
fn step(
    w: &mut World,
    slot: usize,
    mine: InputState,
    others: &mut [Option<BtBot>],
    inputs: &mut Vec<InputState>,
    attacker: Option<i32>,
) {
    inputs.clear();
    let team = w.fighter(w.players[slot]).team.team;
    for i in 0..w.players.len() {
        let me = w.players[i];
        let input = match (others.get_mut(i), attacker) {
            (Some(Some(_)), Some(k)) if i != slot && w.fighter(me).team.team != team => plan::chase(w, me, k),
            (Some(Some(b)), _) if i != slot => b.think(w, me),
            _ if i == slot => mine,
            _ => NO_INPUT,
        };
        inputs.push(input);
    }
    w.step(inputs);
    w.events.clear();
}

/// The teacher's sample of a job copied at its base.
fn teacher(mut job: Job) -> Sample {
    let v = job.view.expect("at the base");
    let lead_picks = job.cands().to_vec();
    let mut short = Vec::new();
    job.shortlist_at_base(&mut short);
    job.teach_all();
    job.run();
    let (cands, scores) = (job.cands(), &job.scores);
    let mut s = Sample {
        features: job.features.clone(),
        scores: Vec::with_capacity(ACTIONS),
        shortlist: Vec::with_capacity(ACTIONS),
        lead: Vec::with_capacity(ACTIONS),
    };
    for &a in &actions::ACTIONS {
        let p = actions::plan_of(a, v.t, v.s);
        let c = cands.iter().position(|&c| c == p).expect("every action is a candidate");
        s.scores.push(scores.get(c).copied().unwrap_or(Real::NAN));
        s.shortlist.push(short.contains(&p));
        s.lead.push(lead_picks.contains(&p));
    }
    s
}

impl Expert {
    pub fn new(seed: i32) -> Expert {
        Expert {
            seed,
            rng: Rng::new(seed ^ SEED_MIX),
            tick: 0,
            queue: Vec::new(),
            stand_ins: Vec::new(),
            inputs: Vec::new(),
            warm: None,
            job: None,
            running: false,
            started: 0,
            away: None,
            workers: None,
            lead: Lead::Net(net::built_in()),
            teach_every: 0,
            decisions: 0,
            samples: Vec::new(),
        }
    }

    /// The threads its decisions run on from the next one on (`None`: inline, on the thinking thread). The bot gives
    /// the same inputs either way.
    pub fn set_workers(&mut self, workers: Option<Arc<Workers>>) {
        self.workers = workers;
    }

    /// Who picks the candidates from the next decision on.
    pub fn set_lead(&mut self, lead: Lead) {
        self.lead = lead;
    }

    /// Every `every`-th decision from now (0: none) also gives a teacher's sample: the look-ahead, with no budget,
    /// scores every action of the list from the decision's base. A tool's (the network's training data): it costs
    /// some 35 decisions' work, and changes nothing the bot does.
    pub fn teach(&mut self, every: u32) {
        self.teach_every = every;
    }

    /// The teacher's samples taken since the last call.
    pub fn take_samples(&mut self) -> Vec<Sample> {
        std::mem::take(&mut self.samples)
    }

    /// The bot's fighters were spawned again or its tree reset: the queued plans and the decision under way go, the
    /// stand-ins hear of it.
    pub fn respawned(&mut self) {
        self.queue.clear();
        self.give_up();
        for b in self.stand_ins.iter_mut().flatten() {
            b.respawned();
        }
    }

    /// The bot's think: the tree's, then the Expert's share of its decision; the input of the bot's next frame.
    /// `bot` is the bot without its Expert.
    pub(crate) fn think(&mut self, bot: &mut BtBot, w: &mut World, me: FighterId) -> InputState {
        let t = self.tick;
        self.tick += 1;
        let tree = bot.think_tree(w, me);
        // (the dev pause freezes the whole bot, its plans with it)
        let slot = w.players.iter().position(|&f| f == me);
        let Some(slot) = slot.filter(|_| !bot.idle) else {
            self.queue.clear();
            self.give_up();
            return tree;
        };
        let _quiet = crate::ai::log::quiet();
        if self.stand_ins.len() != w.players.len() {
            self.stand_ins = (0..w.players.len())
                .map(|i| {
                    let i = i32::try_from(i).expect("a few players");
                    (i != i32::try_from(slot).expect("a few players")).then(|| {
                        BtBot::match_bot(self.seed.wrapping_mul(31).wrapping_add(i), &bot.data, STAND_IN_LEVEL)
                    })
                })
                .collect();
        }
        self.queue.retain(|q| q.end() > t);
        let can = can_plan(w, me);
        if !can {
            self.queue.clear();
            self.give_up();
        }
        let out = queued_input(&self.queue, t, tree, w, me);
        // the stand-ins' think of this tick, on the copy
        copy_world(&mut self.warm, w);
        let warm = self.warm.as_mut().expect("copied");
        self.inputs.clear();
        for i in 0..warm.players.len() {
            let f = warm.players[i];
            let input = match self.stand_ins.get_mut(i) {
                Some(Some(b)) => b.think(warm, f),
                _ => out,
            };
            self.inputs.push(input);
        }
        // a new decision when none is under way and nothing is queued past the delay
        let last_end = self.queue.last().map_or(0, Queued::end);
        if can && !self.running && last_end <= t + REACTION {
            if let Some(away) = self.away.take() {
                self.job = Some(away.take());
            }
            let job = self.job.get_or_insert_with(|| Box::new(Job::new()));
            let teach = self.teach_every != 0 && self.decisions.is_multiple_of(self.teach_every);
            self.decisions = self.decisions.wrapping_add(1);
            job.start(t, slot, warm, &self.inputs, bot, &self.stand_ins, &self.queue, &self.rng, &self.lead, teach);
            self.running = true;
            self.started = t;
            // (a teacher's decision stays inline: its sample is taken at the think its base is reached)
            if let Some(workers) = self.workers.as_deref().filter(|_| !teach) {
                self.away = Some(worker::Ticket::send(workers, self.job.take().expect("started")));
            }
        }
        if self.running {
            self.advance(t);
        }
        out
    }

    /// The decision under way at the think of tick `t`: its share advanced inline (none on a worker); taken at the
    /// think of `started + REACTION - 1`, the last before its plan starts, which waits for a worker not done by then.
    fn advance(&mut self, t: i32) {
        let due = t == self.started + REACTION - 1;
        match self.away.take() {
            Some(away) if due => self.job = Some(away.take()),
            Some(away) => {
                self.away = Some(away);
                return;
            }
            None => {
                let job = self.job.as_mut().expect("a decision under way");
                job.advance();
                if let Some(taught) = job.taught.take() {
                    self.samples.push(teacher(*taught));
                }
            }
        }
        if due {
            let job = self.job.as_ref().expect("a decision under way");
            assert!(job.done(), "a decision ends by the think of t + REACTION - 1");
            self.rng = Rng::new(job.rng.s);
            self.queue.push(Queued { start: job.t + REACTION, plan: job.plan() });
            self.running = false;
        }
    }

    /// The decision under way given up (one on a worker is told to stop, and comes back at the next start).
    fn give_up(&mut self) {
        self.running = false;
        if let Some(away) = &self.away {
            away.drop_job();
        }
    }
}
