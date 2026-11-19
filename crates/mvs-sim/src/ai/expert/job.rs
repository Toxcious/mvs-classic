//! A decision as a resumable job: made from the world of tick `t`, advanced by each think from `t` on by at most
//! [`STEPS_PER_THINK`] simulated ticks, so it is done by the think of `t + REACTION - 1` and no think carries the
//! whole budget. The job holds everything it reads (its worlds, its trees, the plans queued over its delay, its
//! random stream), so it runs on a worker thread as well as inline (`worker`); how far each think takes it is a count of
//! ticks, never a clock.

use super::plan::{self, Plan, Queued};
use super::score;
use super::{BUDGET, HORIZON, Lead, MAX_CANDIDATES, REACTION, copy_bot, copy_bots, copy_world, queued_input, step};
use super::{actions, features};
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
    /// Who picks the candidates.
    lead: Lead,
    /// The simulated ticks a think advances the job by.
    limit: i32,
    /// Each candidate's score, without the tie noise, as they are scored.
    pub scores: Vec<Real>,
    /// Set: at the base, the job is copied into `taught` for the teacher (`Expert::teach`).
    teach: bool,
    pub taught: Option<Box<Job>>,
    /// The network's input and outputs at the base (net-led), and its scratch.
    pub features: Vec<Real>,
    net_out: Vec<Real>,
    net_scratch: Vec<Real>,
    /// The view at the base.
    pub view: Option<plan::View>,
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
            lead: Lead::Shortlist,
            limit: STEPS_PER_THINK,
            scores: Vec::new(),
            teach: false,
            taught: None,
            features: Vec::new(),
            net_out: Vec::new(),
            net_scratch: Vec::new(),
            view: None,
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
        lead: &Lead,
        teach: bool,
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
        self.lead.clone_from(lead);
        self.limit = STEPS_PER_THINK;
        self.scores.clear();
        self.teach = teach;
        self.taught = None;
        self.view = None;
    }

    /// The job's candidates from the base on made every action of the list (`actions::ACTIONS`), each played once
    /// (an action whose plan another before it has is not played again), with no budget: the teacher's run. Called
    /// on a job copied at its base (`taught`); `run` then plays it to its end.
    pub fn teach_all(&mut self) {
        let v = self.view.expect("at the base");
        actions::plans(v.t, v.s, &mut self.cands);
        let mut distinct: Vec<Plan> = Vec::with_capacity(self.cands.len());
        for p in &self.cands {
            if !distinct.contains(p) {
                distinct.push(*p);
            }
        }
        self.cands = distinct;
        self.scores.clear();
        self.limit = i32::MAX;
        self.teach = false;
        self.best = None;
    }

    /// The hand-made shortlist at the base into `out`.
    pub fn shortlist_at_base(&self, out: &mut Vec<Plan>) {
        plan::shortlist(self.base.as_ref().expect("at the base"), self.me.expect("started"), MAX_CANDIDATES, out);
    }

    /// The candidates played (`teach_all`'s distinct plans, or the decision's).
    pub fn cands(&self) -> &[Plan] {
        &self.cands
    }

    /// Advanced to its end with no budget.
    pub fn run(&mut self) {
        self.limit = i32::MAX;
        self.advance();
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
                    if steps >= self.limit {
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
                        let raw = score::score(w, &self.before, self.slot);
                        self.scores.push(raw);
                        let s = raw + self.rng.next() * TIE_NOISE;
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
                    if steps >= self.limit {
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
        let me = self.me.expect("started");
        let v = plan::view(w, me);
        self.view = Some(v);
        if self.teach || matches!(self.lead, Lead::Net(_)) {
            features::features(w, me, &v, &mut self.features);
        }
        match &self.lead {
            Lead::Shortlist => plan::shortlist(w, me, MAX_CANDIDATES, &mut self.cands),
            Lead::Net(net) => {
                net.run(&self.features, &mut self.net_out, &mut self.net_scratch);
                net_shortlist(&self.net_out, v.t, v.s, MAX_CANDIDATES, &mut self.cands);
            }
        }
        self.before.take(w);
        self.phase = Phase::Candidate { c: 0, k: 0 };
        if self.teach {
            let mut t = Box::new(self.clone());
            t.teach = false;
            self.taught = Some(t);
        }
    }
}

/// The network-led shortlist into `out` (cleared first): "what the tree does" first, then the actions the network
/// (`scores`, by action) rates best, best first (the lower index on a tie), each plan once, at most `max` in all.
pub fn net_shortlist(scores: &[Real], t: Real, s: Real, max: usize, out: &mut Vec<Plan>) {
    out.clear();
    let tree = actions::plan_of(actions::Action::Tree, t, s);
    out.push(tree);
    let mut order: Vec<usize> = (0..actions::COUNT.min(scores.len())).collect();
    order.sort_by(|&a, &b| scores[b].total_cmp(&scores[a]).then(a.cmp(&b)));
    for i in order {
        if out.len() >= max {
            break;
        }
        let p = actions::plan_of(actions::ACTIONS[i], t, s);
        if !out.contains(&p) {
            out.push(p);
        }
    }
}
