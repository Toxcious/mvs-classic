//! A decision as a resumable job: made from the world of tick `t`, advanced by each think from `t` on by at most
//! [`STEPS_PER_THINK`] simulated ticks, so it is done by the think of `t + REACTION - 1` and no think carries the
//! whole budget. The job holds everything it reads (its worlds, its trees, the plans queued over its delay, its
//! random stream), so it could run on a worker thread; it runs inline, and how far each think takes it is a count of
//! ticks, never a clock.

use super::plan::{self, Plan, Queued};
use super::score;
use super::{BUDGET, HORIZON, MAX_CANDIDATES, REACTION, copy_bot, copy_bots, copy_world, queued_input, step};
use crate::Real;
use crate::ai::bt::BtBot;
use crate::fighter::FighterId;
use crate::input::InputState;
use crate::rng::Rng;
use crate::world::World;

/// The simulated ticks a think advances a job by: the budget spread over the reaction delay.
pub const STEPS_PER_THINK: i32 = (BUDGET + REACTION - 1) / REACTION;
/// Up to this many points are added to each candidate's score from the job's random stream, so near-equal
/// candidates are picked at random and the bot cannot be farmed by one fixed answer.
const TIE_NOISE: Real = 1.0;

/// Where a job is.
#[derive(Clone, Copy, PartialEq, Debug)]
enum Phase {
    /// The delay's tick `t + k` comes next.
    Delay {
        k: i32,
    },
    /// Candidate `c`'s tick `k` comes next.
    Candidate {
        c: usize,
        k: i32,
    },
    Done,
}

/// A decision under way.
#[derive(Clone)]
pub struct Job {
    /// The tick the decision is made from, the bot's slot and fighter.
    pub t: i32,
    slot: usize,
    me: Option<FighterId>,
    phase: Phase,
    /// The plans queued over the delay, as they were at `t`.
    queue: Vec<Queued>,
    /// The inputs of tick `t` (the bot's and the stand-ins').
    first: Vec<InputState>,
    /// `world`: the world the job steps; `base`: the world at the plan's first tick, each candidate's start.
    world: Option<World>,
    base: Option<World>,
    /// The trees of the run that steps, and at `base`.
    mine: Option<BtBot>,
    others: Vec<Option<BtBot>>,
    base_mine: Option<BtBot>,
    base_others: Vec<Option<BtBot>>,
    cands: Vec<Plan>,
    inputs: Vec<InputState>,
    before: score::Before,
    best: Option<(Real, Plan)>,
    pub rng: Rng,
}

impl std::fmt::Debug for Job {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Job").field("t", &self.t).field("phase", &self.phase).finish_non_exhaustive()
    }
}

impl Job {
    pub fn new() -> Job {
        Job {
            t: 0,
            slot: 0,
            me: None,
            phase: Phase::Done,
            queue: Vec::new(),
            first: Vec::new(),
            world: None,
            base: None,
            mine: None,
            others: Vec::new(),
            base_mine: None,
            base_others: Vec::new(),
            cands: Vec::new(),
            inputs: Vec::new(),
            before: score::Before::default(),
            best: None,
            rng: Rng::new(0),
        }
    }

    /// The job made again for a decision at tick `t` (its memory reused): `w` the world of `t` with the stand-ins'
    /// thinks of `t` made, `first` the inputs of `t` by slot, `bot` the bot's tree after its think of `t`.
    #[allow(clippy::too_many_arguments)] // the decision's whole input, taken once
    pub fn start(
        &mut self,
        t: i32,
        slot: usize,
        w: &World,
        first: &[InputState],
        bot: &BtBot,
        stand_ins: &[Option<BtBot>],
        queue: &[Queued],
        rng: &Rng,
    ) {
        self.t = t;
        self.slot = slot;
        self.me = Some(w.players[slot]);
        self.phase = Phase::Delay { k: 0 };
        self.queue.clear();
        self.queue.extend_from_slice(queue);
        self.first.clear();
        self.first.extend_from_slice(first);
        copy_world(&mut self.world, w);
        copy_bot(&mut self.mine, Some(bot));
        copy_bots(&mut self.others, stand_ins);
        self.best = None;
        self.rng = Rng::new(rng.s);
    }

    pub fn done(&self) -> bool {
        self.phase == Phase::Done
    }

    /// The decision, once done.
    pub fn plan(&self) -> Plan {
        self.best.map_or(Plan::Tree { len: 1 }, |(_, p)| p)
    }

    /// One think's share: at most [`STEPS_PER_THINK`] simulated ticks. The steps that cost no tick (the base kept, a
    /// candidate scored) are taken past the share, so a whole budget ends within `REACTION` thinks.
    pub fn advance(&mut self) {
        let mut steps = 0;
        loop {
            match self.phase {
                Phase::Done => return,
                Phase::Delay { k } => {
                    let w = self.world.as_mut().expect("started");
                    if k >= REACTION || w.flow.match_over {
                        self.at_base();
                        continue;
                    }
                    if steps >= STEPS_PER_THINK {
                        return;
                    }
                    if k == 0 {
                        w.step(&self.first);
                        w.events.clear();
                    } else {
                        let mine = self.mine.as_mut().expect("started");
                        let tree = mine.think_tree(w, self.me.expect("started"));
                        let input = queued_input(&self.queue, self.t + k, tree);
                        step(w, self.slot, input, &mut self.others, &mut self.inputs);
                    }
                    steps += 1;
                    self.phase = Phase::Delay { k: k + 1 };
                }
                Phase::Candidate { c, k } => {
                    let w = self.world.as_mut().expect("started");
                    if k >= HORIZON || w.flow.match_over {
                        let s = score::score(w, &self.before, self.slot) + self.rng.next() * TIE_NOISE;
                        if self.best.is_none_or(|(b, _)| s > b) {
                            self.best = Some((s, self.cands[c]));
                        }
                        if c + 1 < self.cands.len() {
                            copy_world(&mut self.world, self.base.as_ref().expect("the base"));
                            copy_bot(&mut self.mine, self.base_mine.as_ref());
                            copy_bots(&mut self.others, &self.base_others);
                            self.phase = Phase::Candidate { c: c + 1, k: 0 };
                        } else {
                            self.phase = Phase::Done;
                        }
                        continue;
                    }
                    if steps >= STEPS_PER_THINK {
                        return;
                    }
                    let mine = self.mine.as_mut().expect("started");
                    let tree = mine.think_tree(w, self.me.expect("started"));
                    let input = self.cands[c].input(k, tree);
                    step(w, self.slot, input, &mut self.others, &mut self.inputs);
                    steps += 1;
                    self.phase = Phase::Candidate { c, k: k + 1 };
                }
            }
        }
    }

    /// The delay is stepped: the world and the trees kept as every candidate's start, the shortlist made.
    fn at_base(&mut self) {
        let w = self.world.as_ref().expect("started");
        copy_world(&mut self.base, w);
        copy_bot(&mut self.base_mine, self.mine.as_ref());
        copy_bots(&mut self.base_others, &self.others);
        plan::shortlist(w, self.me.expect("started"), MAX_CANDIDATES, &mut self.cands);
        self.before.take(w);
        self.phase = Phase::Candidate { c: 0, k: 0 };
    }
}
