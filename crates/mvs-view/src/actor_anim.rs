//! A small AnimBP runner for a spawned actor's own skeleton (the views of
//! [`crate::actor_view::SkinnedActor`]): the nodes the item helpers' AnimBPs place, as a table per AnimBP that a view
//! writes from its decode and inputs it reads through callbacks. Updated once per sim tick (UpdateAnimation once a
//! frame) and evaluated after it.
//!
//! The engine's rules it follows:
//! - FAnimNode_StateMachine (Update_AnyThread 142867fa0, FindValidTransition 142852f00, SetState 1428634c0,
//!   UpdateState 142867780, GetStateWeight 142856d90, Evaluate 142852a80): the transition loop first, up to
//!   MaxTransitionsPerFrame (a search, and one more after each taken one), rules read at that moment in the state's
//!   baked order; a conduit's entry rule is read at the start of each search through it, and a transition into a
//!   conduit is taken only when a search from the conduit finds a valid exit (else the next transition is tried);
//!   each taken transition appends an active entry with duration = CrossfadeDuration x InverseAlpha(option, 1 -
//!   GetStateWeight(target)) (HermiteCubic 4/3 x^3 - 2 x^2 + 5/3 x, else clamped x), then SetState: the entered
//!   state's players' cached weights go to 0, and its pose link is initialised (players back to their start) only
//!   when its weight read after the append is <= 0 or it resets on entry. On the first update with
//!   bSkipFirstUpdateTransition the active entries are dropped (the transitions stand, without a blend). Then each
//!   entry: Elapsed += dt, Alpha = AlphaToBlendOption(Elapsed / duration; 1 at duration 0); the states are updated
//!   per entry in order, previous at 1 - Alpha then next at Alpha, each state once (the first entry naming it gives
//!   its weight), the current state alone at 1 when there is none; conduits are never updated; finished entries are
//!   dropped up to the last finished. Evaluate: the current state alone, or the entries in order, each blending what
//!   came before it (the first: its previous state) to its next state.
//! - GetRelevantAnimTime / Remaining (142856260, 142856350): the state's player with the highest cached weight
//!   strictly above 0; remaining = length - time, FLT_MAX with none.
//! - Sequence players: time += dt x rate whatever the weight (UAnimSequenceBase::TickAssetPlayer 142878830);
//!   AdvanceTime (142816220) wraps a looping one and clamps another to [0, length].
//! - FAnimNode_BlendListBase (Update 1424936b0): on a change of child the blend time is BlendTime[child] x
//!   min(|1 - weight[child]|, 1), 0 with no previous child (after an initialise); the weights move by FAlphaBlend
//!   from where they are; an instant switch updates the departed child once at weight 0; only children above 1e-5
//!   are updated and evaluated.
//! - FAnimNode_LayeredBoneBlend (14249e6b0 ff.) in local space: per bone, the base pose blended to the layer's by the
//!   mask weight (CreateMaskWeights 14281de60: branch filter bone and descendants, clamp((depth + 1) x inc)).
//! - FAnimNode_Slot / montage: the montage's pose over the slot's source by the instance's weight; the instance is
//!   the sim's (mvs_sim::actors::actor_montage: a MontageState per actor mesh with its blend-out rule, advanced by the
//!   mesh's tick with its notifies).
//!
//! Blending: FAnimationRuntime::BlendPosesTogether's kernels ([`crate::anim`] weight_pose / add_weighted_pose /
//! normalize_rotations). A transition through a conduit takes the crossfade (time, option) of the chain's last
//! transition (into the state it ends in): FindValidTransition 142852f00 sets the rule only at the chain's last exit,
//! Update_AnyThread 142867fa0 builds the blend from it.
//!
//! Not ported (no AnimBP of the runner's users has them): sync groups, custom blend graphs and inertialisation, blend
//! profiles, curves, notifies of the state players, mesh-space layered blends, the automatic rule on a conduit's
//! exits. A nested machine ([`PoseDef::Machine`]: Meeseeks_AnimBP's IdleState in Movement's Idle) is its own
//! [`ActorMachine`], updated with its node's weight above 1e-5 and evaluated in its place.

use mvs_anim::montage::Montage;

use crate::anim::{Clips, Pose, Rig, add_weighted_pose, normalize_rotations, sample_clip, weight_pose};
use crate::blend::{BlendSpacePlayer, BlendSpacePlayerData};

// Unverified: GetRelevantAnimTime with no relevant player is 0 (UE 4.26 source; only Remaining was read in the exe).
// Deviation: a nested machine's players are not among the outer state's PlayerNodeIndices here
// (GetRelevantAnimTime of the outer state reads them in the build): no rule of its users reads that.

/// ZERO_ANIMWEIGHT_THRESH (DAT_143b3a298).
const ZERO_W: f64 = 1e-5;
const FLT_MAX: f64 = 3.402_823_466_385_288_6e38;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BlendOption {
    Linear,
    HermiteCubic,
}

/// FAlphaBlend::AlphaToBlendOption: HermiteCubic is SmoothStep(0, 1, x); Linear the clamped x.
fn alpha_of(o: BlendOption, x: f64) -> f64 {
    let c = x.clamp(0.0, 1.0);
    if o == BlendOption::HermiteCubic { c * c * (3.0 - 2.0 * c) } else { c }
}

/// CalculateInverseAlpha (the crossfade's duration scale).
fn inverse_alpha(o: BlendOption, x: f64) -> f64 {
    if o == BlendOption::HermiteCubic {
        return x * x * x * (4.0 / 3.0) - 2.0 * x * x + x * (5.0 / 3.0);
    }
    x.clamp(0.0, 1.0)
}

// ---- the table a view writes

/// A sequence player (StartPosition 0, PlayRate 1 unless given).
#[derive(Clone, Debug)]
pub struct SeqDef {
    pub seq: &'static str,
    pub looped: bool,
    pub rate: f64,
    pub start: f64,
    pub ignore_for_relevancy: bool,
}

impl SeqDef {
    pub const fn new(seq: &'static str, looped: bool) -> SeqDef {
        SeqDef { seq, looped, rate: 1.0, start: 0.0, ignore_for_relevancy: false }
    }
}

/// BlendListByBool: BlendPose[0] (`t`) when the input is true, [1] (`f`) when false; BlendTime per pose.
pub struct BoolListDef<I> {
    pub test: fn(&I) -> bool,
    pub t: Box<PoseDef<I>>,
    pub f: Box<PoseDef<I>>,
    pub blend_time: [f64; 2],
    /// Linear: the only one the runner's users have.
    pub option: BlendOption,
}

/// A BlendSpacePlayer over a full-pose blend space ([`crate::blend`]: select, then the leader's advance, each update
/// whatever the weight, as a sequence player's time; initialised with its state). x / y: its bound inputs.
// Deviation: the player is not among its state's relevant players (GetRelevantAnimTime reads asset
// players, a blend space's too, in the build): no rule of the runner's users reads such a state's time.
pub struct SpaceDef<I> {
    pub space: BlendSpacePlayerData,
    pub x: fn(&I) -> f64,
    pub y: Option<fn(&I) -> f64>,
}

pub enum PoseDef<I> {
    Seq(SeqDef),
    BoolList(BoolListDef<I>),
    /// A state machine nested as a pose (FAnimNode_StateMachine under a state or a blend list).
    Machine(MachineDef<I>),
    Space(SpaceDef<I>),
}

pub struct StateDef<I> {
    pub name: &'static str,
    /// None: a conduit.
    pub pose: Option<PoseDef<I>>,
    /// A conduit's entry rule.
    pub entry_rule: Option<fn(&I) -> bool>,
    /// bAlwaysResetOnEntry.
    pub always_reset: bool,
}

/// What a rule may ask of the machine besides the inputs.
pub trait MachineQuery {
    /// GetRelevantAnimTime.
    fn time(&self, state: &str) -> f64;
    /// GetRelevantAnimTimeRemaining.
    fn remaining(&self, state: &str) -> f64;
}

pub struct TransitionDef<I> {
    pub from: &'static str,
    pub to: &'static str,
    pub xfade: f64,
    /// HermiteCubic unless the decode says otherwise.
    pub option: BlendOption,
    pub rule: Option<fn(&I, &dyn MachineQuery) -> bool>,
    /// bAutomaticRemainingTimeRule: remaining(from) <= xfade.
    pub auto: bool,
}

pub struct MachineDef<I> {
    pub entry: &'static str,
    /// bReinitializeOnBecomingRelevant, for a nested machine: an update after a missed one (the anim instance's update
    /// counter, [`ActorMachine::update_at`]'s frame) starts it over at its entry state (Update 142867fa0). The machine
    /// a view updates itself is updated every frame.
    pub reinit_on_becoming_relevant: bool,
    pub max_transitions_per_frame: u32,
    pub skip_first_update_transition: bool,
    pub states: Vec<StateDef<I>>,
    /// Each state's in its baked (priority) order.
    pub transitions: Vec<TransitionDef<I>>,
}

/// The skeleton a pose is laid out on ([`crate::actor_view::SkinnedActor::skeleton`]): the rig bound to the clips,
/// the reference pose, the top bone and each bone's parent.
pub struct ActorSkeleton<'a> {
    pub rig: &'a Rig,
    pub rest: &'a Pose,
    pub root: i32,
    pub parents: &'a [i32],
}

// ---- the running nodes

#[derive(Debug)]
struct Player {
    /// The clip and its length (None: the player shows the reference pose, and its time still runs).
    clip: Option<(usize, f64)>,
    looped: bool,
    rate: f64,
    start: f64,
    ignore: bool,
    time: f64,
    /// The cached BlendWeight of the latest update.
    weight: f64,
}

struct BoolList<I> {
    test: fn(&I) -> bool,
    blend_time: [f64; 2],
    option: BlendOption,
    kids: [Node<I>; 2],
    w: [f64; 2],
    /// -1 after an initialise.
    last: i32,
    from: [f64; 2],
    to: [f64; 2],
    /// The running blend's time, 0 when none runs.
    blend: f64,
    elapsed: f64,
}

enum Node<I> {
    /// A player of the machine's list.
    Player(usize),
    BoolList(Box<BoolList<I>>),
    /// seen: the frame of its last update, -1 never.
    Nested {
        m: Box<ActorMachine<I>>,
        seen: i64,
    },
    Space {
        x: fn(&I) -> f64,
        y: Option<fn(&I) -> f64>,
        bs: Box<BlendSpacePlayer>,
    },
}

struct State<I> {
    name: &'static str,
    root: Option<Node<I>>,
    /// Its players, in the machine's list.
    players: Vec<usize>,
    entry_rule: Option<fn(&I) -> bool>,
    always_reset: bool,
    out: Vec<Trans<I>>,
}

struct Trans<I> {
    to: usize,
    xfade: f64,
    option: BlendOption,
    rule: Option<fn(&I, &dyn MachineQuery) -> bool>,
    auto: bool,
}

#[derive(Debug)]
struct Entry {
    prev: usize,
    next: usize,
    dur: f64,
    option: BlendOption,
    elapsed: f64,
    alpha: f64,
}

/// One state machine of one view (FAnimNode_StateMachine).
pub struct ActorMachine<I> {
    states: Vec<State<I>>,
    players: Vec<Player>,
    entry: usize,
    reinit_on_becoming_relevant: bool,
    max_transitions: u32,
    skip_first: bool,
    current: usize,
    first: bool,
    active: Vec<Entry>,
    updated: Vec<bool>,
    visited: Vec<bool>,
    /// The chain a search found: (state, transition index in its `out`).
    chain: Vec<(usize, usize)>,
    /// Poses for the evaluation: the chain's result, a state's pose, and two per blend list level nested under it.
    tmp: Vec<Pose>,
    /// A blend space's sample pose.
    space_tmp: Pose,
    /// The anim instance's update counter (a nested machine's relevance).
    frame: i64,
}

impl<I> std::fmt::Debug for ActorMachine<I> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ActorMachine").field("state", &self.state()).finish_non_exhaustive()
    }
}

/// The depth of blend lists nested in a pose (a nested machine has its own poses).
fn list_depth<I>(d: &PoseDef<I>) -> usize {
    match d {
        PoseDef::BoolList(b) => 1 + list_depth(&b.t).max(list_depth(&b.f)),
        _ => 0,
    }
}

impl<I> ActorMachine<I> {
    /// The machine `def` over a skeleton of `bones` bones; its sequences by their names in `clips`.
    pub fn new(def: MachineDef<I>, bones: usize, clips: &Clips) -> ActorMachine<I> {
        let MachineDef {
            entry,
            reinit_on_becoming_relevant,
            max_transitions_per_frame,
            skip_first_update_transition,
            states: state_defs,
            transitions,
        } = def;
        let mut players = Vec::new();
        let depth = state_defs.iter().filter_map(|s| s.pose.as_ref()).map(list_depth).max().unwrap_or(0);
        let mut states: Vec<State<I>> = Vec::with_capacity(state_defs.len());
        for s in state_defs {
            if states.iter().any(|x| x.name == s.name) {
                panic!("actor anim: state {} twice", s.name);
            }
            let mut own = Vec::new();
            let root = s.pose.map(|p| Self::make(p, bones, clips, &mut players, &mut own));
            states.push(State {
                name: s.name,
                root,
                players: own,
                entry_rule: s.entry_rule,
                always_reset: s.always_reset,
                out: Vec::new(),
            });
        }
        let index = |states: &[State<I>], n: &str| {
            states.iter().position(|s| s.name == n).unwrap_or_else(|| panic!("actor anim: no state {n}"))
        };
        for t in transitions {
            let to = index(&states, t.to);
            let from = index(&states, t.from);
            states[from].out.push(Trans { to, xfade: t.xfade, option: t.option, rule: t.rule, auto: t.auto });
        }
        let entry_idx = index(&states, entry);
        assert!(states[entry_idx].root.is_some(), "actor anim: the entry state {entry} is a conduit");
        let n = states.len();
        let mut m = ActorMachine {
            states,
            players,
            entry: entry_idx,
            reinit_on_becoming_relevant,
            max_transitions: max_transitions_per_frame,
            skip_first: skip_first_update_transition,
            current: entry_idx,
            first: true,
            active: Vec::new(),
            updated: vec![false; n],
            visited: vec![false; n],
            chain: Vec::new(),
            tmp: (0..2 + 2 * depth).map(|_| Pose::new(bones)).collect(),
            space_tmp: Pose::new(bones),
            frame: 0,
        };
        m.reinit();
        m
    }

    fn make(d: PoseDef<I>, bones: usize, clips: &Clips, players: &mut Vec<Player>, own: &mut Vec<usize>) -> Node<I> {
        match d {
            PoseDef::Machine(m) => Node::Nested { m: Box::new(ActorMachine::new(m, bones, clips)), seen: -1 },
            PoseDef::Space(s) => {
                assert!(
                    !s.space.space.additive,
                    "actor anim: the additive blend space {} is not ported here",
                    s.space.space.name
                );
                Node::Space { x: s.x, y: s.y, bs: Box::new(BlendSpacePlayer::new(&s.space, clips)) }
            }
            PoseDef::Seq(s) => {
                let clip = clips.get(s.seq).map(|c| (c, clips.clips[c].anim.length));
                let p = Player {
                    clip,
                    looped: s.looped,
                    rate: s.rate,
                    start: s.start,
                    ignore: s.ignore_for_relevancy,
                    time: 0.0,
                    weight: 0.0,
                };
                own.push(players.len());
                players.push(p);
                Node::Player(players.len() - 1)
            }
            PoseDef::BoolList(b) => {
                let t = Self::make(*b.t, bones, clips, players, own);
                let f = Self::make(*b.f, bones, clips, players, own);
                Node::BoolList(Box::new(BoolList {
                    test: b.test,
                    blend_time: b.blend_time,
                    option: b.option,
                    kids: [t, f],
                    w: [1.0, 0.0],
                    last: -1,
                    from: [0.0; 2],
                    to: [0.0; 2],
                    blend: 0.0,
                    elapsed: 0.0,
                }))
            }
        }
    }

    fn index(&self, name: &str) -> usize {
        self.states.iter().position(|s| s.name == name).unwrap_or_else(|| panic!("actor anim: no state {name}"))
    }

    /// The machine starts over at its entry state (bReinitializeOnBecomingRelevant; the view's first update).
    pub fn reinit(&mut self) {
        self.active.clear();
        self.current = self.entry;
        self.first = true;
        let ActorMachine { states, players, .. } = self;
        for s in states.iter_mut() {
            if let Some(r) = &mut s.root {
                init_node(r, players);
            }
        }
    }

    /// The current state's name.
    pub fn state(&self) -> &'static str {
        self.states[self.current].name
    }

    // ---- queries the rules read

    fn relevant(&self, state: &str) -> Option<&Player> {
        let mut best = None;
        let mut max = 0.0;
        for &i in &self.states[self.index(state)].players {
            let p = &self.players[i];
            if !p.ignore && p.weight > max {
                max = p.weight;
                best = Some(p);
            }
        }
        best
    }

    /// GetStateWeight.
    pub fn weight(&self, s: usize) -> f64 {
        if self.active.is_empty() {
            return if s == self.current { 1.0 } else { 0.0 };
        }
        let mut w = 0.0;
        for (i, e) in self.active.iter().enumerate() {
            if i == 0 {
                w = if e.prev == s { 1.0 - e.alpha } else { 0.0 };
            } else {
                w *= 1.0 - e.alpha;
            }
            if e.next == s {
                w += e.alpha;
            }
        }
        w.clamp(0.0, 1.0)
    }

    // ---- update

    /// An update with the anim instance's update counter one more than the last.
    pub fn update(&mut self, inp: &I, dt: f64, clips: &Clips) {
        self.update_at(inp, dt, clips, self.frame + 1);
    }

    /// An update at `frame`, the anim instance's update counter (a nested machine gets its outer one's).
    pub fn update_at(&mut self, inp: &I, dt: f64, clips: &Clips, frame: i64) {
        self.frame = frame;
        let mut count = 0;
        loop {
            count += 1;
            let mut chain = std::mem::take(&mut self.chain);
            chain.clear();
            self.visited.fill(false);
            let found = self.find(inp, self.current, &mut chain);
            let last = chain.last().map(|&(s, t)| &self.states[s].out[t]);
            if let (true, Some(last)) = (found, last) {
                let (target, xfade, option) = (last.to, last.xfade, last.option);
                let existing = self.weight(target);
                let dur = xfade * inverse_alpha(option, 1.0 - existing);
                self.active.push(Entry { prev: self.current, next: target, dur, option, elapsed: 0.0, alpha: 0.0 });
                self.chain = chain;
                self.set_state(target);
            } else {
                self.chain = chain;
                break;
            }
            if count >= self.max_transitions {
                break;
            }
        }
        if self.first && self.skip_first {
            self.active.clear();
        }
        self.first = false;
        let mut last_done = None;
        for (i, e) in self.active.iter_mut().enumerate() {
            e.elapsed += dt;
            let q = if e.dur > 0.0 { e.elapsed / e.dur } else { 1.0 };
            e.alpha = alpha_of(e.option, q);
            if q >= 1.0 {
                last_done = Some(i);
            }
        }
        self.updated.fill(false);
        if self.active.is_empty() {
            self.update_state(inp, dt, clips, self.current, 1.0);
        } else {
            for i in 0..self.active.len() {
                let (prev, next, alpha) = (self.active[i].prev, self.active[i].next, self.active[i].alpha);
                self.update_state(inp, dt, clips, prev, 1.0 - alpha);
                self.update_state(inp, dt, clips, next, alpha);
            }
        }
        if let Some(i) = last_done {
            self.active.drain(..=i);
        }
    }

    /// FindValidTransition: the chain of transitions from `s` to a state that is no conduit, in `chain`.
    fn find(&mut self, inp: &I, s: usize, chain: &mut Vec<(usize, usize)>) -> bool {
        if self.visited[s] {
            return false;
        }
        self.visited[s] = true;
        if let Some(r) = self.states[s].entry_rule
            && !r(inp)
        {
            return false;
        }
        for k in 0..self.states[s].out.len() {
            let t = &self.states[s].out[k];
            let (to, xfade, auto, rule) = (t.to, t.xfade, t.auto, t.rule);
            let ok =
                if auto { self.remaining(self.states[s].name) <= xfade } else { rule.is_some_and(|r| r(inp, &*self)) };
            if !ok {
                continue;
            }
            chain.push((s, k));
            if self.states[to].root.is_none() {
                if self.find(inp, to, chain) {
                    return true;
                }
                chain.pop();
                continue;
            }
            return true;
        }
        false
    }

    fn set_state(&mut self, s: usize) {
        let w = self.weight(s);
        self.current = s;
        let ActorMachine { states, players, .. } = self;
        let st = &mut states[s];
        for &i in &st.players {
            players[i].weight = 0.0;
        }
        if let Some(r) = &mut st.root
            && (w <= 0.0 || st.always_reset)
        {
            init_node(r, players);
        }
    }

    fn update_state(&mut self, inp: &I, dt: f64, clips: &Clips, s: usize, w: f64) {
        if self.states[s].root.is_none() || self.updated[s] {
            return;
        }
        self.updated[s] = true;
        let frame = self.frame;
        let ActorMachine { states, players, .. } = self;
        let root = states[s].root.as_mut().expect("a state with a pose");
        update_node(root, players, inp, dt, clips, frame, w);
    }

    // ---- evaluate

    /// The machine's pose into `out`.
    pub fn evaluate(&mut self, clips: &mut Clips, sk: &ActorSkeleton<'_>, out: &mut Pose) {
        let ActorMachine { states, players, active, current, tmp, space_tmp, .. } = self;
        let mut cx = EvalCx { clips, sk, players, space_tmp };
        if active.is_empty() {
            let (_, free) = tmp.split_at_mut(2);
            eval_state(&mut states[*current], &mut cx, out, free);
            return;
        }
        let (res, rest) = tmp.split_first_mut().expect("the chain's pose");
        let (b, free) = rest.split_first_mut().expect("a state's pose");
        eval_state(&mut states[active[0].prev], &mut cx, res, free);
        for e in active.iter() {
            eval_state(&mut states[e.next], &mut cx, b, free);
            scale_pose(res, 1.0 - e.alpha);
            add_weighted_pose(b, e.alpha, res);
            normalize_rotations(res);
        }
        out.copy(res);
    }
}

impl<I> MachineQuery for ActorMachine<I> {
    fn time(&self, state: &str) -> f64 {
        self.relevant(state).map_or(0.0, |p| p.time)
    }

    fn remaining(&self, state: &str) -> f64 {
        match self.relevant(state) {
            Some(Player { clip: Some((_, len)), time, .. }) => len - time,
            _ => FLT_MAX,
        }
    }
}

fn init_node<I>(n: &mut Node<I>, players: &mut [Player]) {
    match n {
        Node::Player(i) => {
            let p = &mut players[*i];
            p.time = p.start;
        }
        // (Initialize_AnyThread: the entry state)
        Node::Nested { m, .. } => m.reinit(),
        Node::Space { bs, .. } => bs.init(),
        Node::BoolList(b) => {
            b.w = [1.0, 0.0];
            b.last = -1;
            b.blend = 0.0;
            let [t, f] = &mut b.kids;
            init_node(t, players);
            init_node(f, players);
        }
    }
}

fn update_node<I>(n: &mut Node<I>, players: &mut [Player], inp: &I, dt: f64, clips: &Clips, frame: i64, w: f64) {
    match n {
        Node::Player(i) => {
            let p = &mut players[*i];
            p.weight = w;
            let len = p.clip.map_or(0.0, |c| c.1);
            let mut t = p.time + dt * p.rate;
            if p.looped && len > 0.0 {
                t %= len;
                if t < 0.0 {
                    t += len;
                }
            } else {
                t = t.max(0.0).min(len);
            }
            p.time = t;
        }
        Node::Space { x, y, bs } => {
            if bs.ok && bs.select(x(inp), y.map_or(0.0, |y| y(inp)), dt) {
                bs.advance(clips, dt);
            }
        }
        Node::Nested { m, seen } => {
            if m.reinit_on_becoming_relevant && *seen >= 0 && *seen != frame - 1 {
                m.reinit();
            }
            *seen = frame;
            m.update_at(inp, dt, clips, frame);
        }
        Node::BoolList(b) => {
            let child = if (b.test)(inp) { 0 } else { 1 };
            if child != b.last {
                let diff = (1.0 - b.w[child as usize]).abs().min(1.0);
                let time = if b.last < 0 { 0.0 } else { b.blend_time[child as usize] * diff };
                if time == 0.0 && b.last >= 0 {
                    let last = b.last as usize;
                    update_node(&mut b.kids[last], players, inp, dt, clips, frame, 0.0);
                }
                for i in 0..2 {
                    b.from[i] = b.w[i];
                    b.to[i] = if i == child as usize { 1.0 } else { 0.0 };
                }
                b.blend = time;
                b.elapsed = 0.0;
                b.last = child;
            }
            b.elapsed += dt;
            let a = if b.blend > 0.0 { alpha_of(b.option, b.elapsed / b.blend) } else { 1.0 };
            let mut sum = 0.0;
            for i in 0..2 {
                b.w[i] = b.from[i] + (b.to[i] - b.from[i]) * a;
                sum += b.w[i];
            }
            if sum > ZERO_W && (sum - 1.0).abs() > ZERO_W {
                for i in 0..2 {
                    b.w[i] /= sum;
                }
            }
            for i in 0..2 {
                if b.w[i] > ZERO_W {
                    let wi = w * b.w[i];
                    update_node(&mut b.kids[i], players, inp, dt, clips, frame, wi);
                }
            }
        }
    }
}

/// `weightPose(p, w, p)`: [`weight_pose`] with its pose as its out (the same products, in place).
fn scale_pose(p: &mut Pose, w: f64) {
    let m = |v: &mut f32| *v = (f64::from(*v) * w) as f32;
    p.t.iter_mut().for_each(m);
    p.s.iter_mut().for_each(m);
    p.r.iter_mut().for_each(m);
}

struct EvalCx<'c, 's> {
    clips: &'c mut Clips,
    sk: &'c ActorSkeleton<'s>,
    players: &'c [Player],
    space_tmp: &'c mut Pose,
}

fn eval_state<I>(s: &mut State<I>, cx: &mut EvalCx<'_, '_>, out: &mut Pose, free: &mut [Pose]) {
    if let Some(r) = &mut s.root {
        eval_node(r, cx, out, free);
    }
}

/// The node's pose into `out`; `free`: the poses free for the blend lists under it.
fn eval_node<I>(n: &mut Node<I>, cx: &mut EvalCx<'_, '_>, out: &mut Pose, free: &mut [Pose]) {
    let sk = cx.sk;
    match n {
        Node::Player(i) => match cx.players[*i].clip {
            Some((c, _)) => sample_clip(cx.clips, c, sk.rig, sk.rest, cx.players[*i].time, out, sk.root),
            None => out.copy(sk.rest),
        },
        Node::Nested { m, .. } => m.evaluate(cx.clips, sk, out),
        Node::Space { bs, .. } => {
            if bs.ok {
                bs.pose(cx.clips, sk.rig, sk.rest, out, cx.space_tmp, sk.root);
            } else {
                out.copy(sk.rest);
            }
        }
        Node::BoolList(b) => {
            let (a, bb) = (b.w[0] > ZERO_W, b.w[1] > ZERO_W);
            if !(a && bb) {
                eval_node(&mut b.kids[if a { 0 } else { 1 }], cx, out, free);
                return;
            }
            let (pa, rest) = free.split_first_mut().expect("a blend list's pose");
            let (pb, deeper) = rest.split_first_mut().expect("a blend list's pose");
            let [t, f] = &mut b.kids;
            eval_node(t, cx, pa, deeper);
            eval_node(f, cx, pb, deeper);
            weight_pose(pa, b.w[0], out);
            add_weighted_pose(pb, b.w[1], out);
            normalize_rotations(out);
        }
    }
}

// ---- LayeredBoneBlend (local space)

/// One branch filter of a layer: a bone (any case) and its BlendDepth.
#[derive(Clone, Copy, Debug)]
pub struct BranchFilter {
    pub bone: &'static str,
    pub depth: i32,
}

/// CreateMaskWeights for one layer: the branch filters' bones and their descendants, clamp((depth + 1) x inc) with
/// inc = 1 / BlendDepth (1 at 0), a negative BlendDepth taking the branch out; filters add up.
pub fn branch_mask(names: &[String], parents: &[i32], filters: &[BranchFilter]) -> Vec<f32> {
    let n = names.len();
    let mut w = vec![0.0f32; n];
    for f in filters {
        let b = names
            .iter()
            .position(|s| s.eq_ignore_ascii_case(f.bone))
            .unwrap_or_else(|| panic!("actor anim: no bone {} for a branch filter", f.bone));
        let inc = if f.depth != 0 { 1.0 / f64::from(f.depth) } else { 1.0 };
        for (i, wi) in w.iter_mut().enumerate() {
            let (mut d, mut j) = (0.0, i as i32);
            while j >= 0 && j as usize != b {
                j = parents[j as usize];
                d += 1.0;
            }
            if j < 0 {
                continue;
            }
            *wi = if f.depth < 0 { 0.0 } else { (f64::from(*wi) + (d + 1.0) * inc).clamp(0.0, 1.0) as f32 };
        }
    }
    w
}

/// `p` = itself with each bone blended to `layer`'s by `mask` (local space; layeredBlend with out = base).
pub fn layered_blend_in_place(p: &mut Pose, layer: &Pose, mask: &[f32]) {
    let g = |v: f32| f64::from(v);
    for (i, &m) in mask.iter().enumerate() {
        let m = g(m);
        if !(m > ZERO_W) {
            continue;
        }
        let k = 1.0 - m;
        for c in 0..3 {
            let j = i * 3 + c;
            p.t[j] = (g(p.t[j]) * k + g(layer.t[j]) * m) as f32;
            p.s[j] = (g(p.s[j]) * k + g(layer.s[j]) * m) as f32;
        }
        let r = i * 4;
        let (br, lr) = (&p.r[r..r + 4], &layer.r[r..r + 4]);
        let dot = (0..4).map(|c| g(br[c]) * g(lr[c])).sum::<f64>();
        let s = if dot < 0.0 { -m } else { m };
        let mut q = [0.0f64; 4];
        for c in 0..4 {
            q[c] = g(br[c]) * k + g(lr[c]) * s;
        }
        let l2 = q[0] * q[0] + q[1] * q[1] + q[2] * q[2] + q[3] * q[3];
        if l2 < 1e-8 {
            q = [0.0, 0.0, 0.0, 1.0];
        } else {
            let l = l2.sqrt();
            for v in &mut q {
                *v /= l;
            }
        }
        for (o, v) in p.r[r..r + 4].iter_mut().zip(q) {
            *o = v as f32;
        }
    }
}

/// `out` = `base` with each bone blended to `layer`'s by `mask` (local space).
pub fn layered_blend(base: &Pose, layer: &Pose, mask: &[f32], out: &mut Pose) {
    out.copy(base);
    layered_blend_in_place(out, layer, mask);
}

// ---- a Slot node with its montage

/// One Slot node of the actor's own anim instance over the sim's montage instance
/// (mvs_sim::actors::actor_montage: the actor's MontageState, played, advanced and its notifies run by the sim):
/// [`ActorSlot::apply`] is the Slot node's pose over its source.
#[derive(Debug)]
pub struct ActorSlot {
    slot: &'static str,
    pose: Pose,
}

impl ActorSlot {
    pub fn new(slot: &'static str, bones: usize) -> ActorSlot {
        ActorSlot { slot, pose: Pose::new(bones) }
    }

    /// `out` = `source` with the montage's pose of this slot over it by the instance's weight; false (`out`
    /// untouched) when no montage with a track for the slot plays. `inst`: the mesh's instance and its montage.
    pub fn apply<K>(
        &mut self,
        clips: &mut Clips,
        sk: &ActorSkeleton<'_>,
        inst: Option<(&mvs_anim::montage::MontageState<K>, &Montage)>,
        source: &Pose,
        out: &mut Pose,
    ) -> bool {
        let Some((st, m)) = inst else { return false };
        let w = st.shown_weight(m, &|s| eprintln!("warning: {s}"));
        if !(w > ZERO_W) {
            return false;
        }
        let Some((si, gi, time)) = st.pose_in(m, self.slot) else { return false };
        let Some(c) = clips.get(&m.slots[si].segments[gi].anim) else { return false };
        sample_clip(clips, c, sk.rig, sk.rest, time, &mut self.pose, sk.root);
        weight_pose(source, 1.0 - w, out);
        add_weighted_pose(&self.pose, w, out);
        normalize_rotations(out);
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Inp {
        go: bool,
    }

    fn seq(name: &'static str) -> Option<PoseDef<Inp>> {
        Some(PoseDef::Seq(SeqDef::new(name, true)))
    }

    fn machine() -> MachineDef<Inp> {
        let state = |name, pose| StateDef { name, pose, entry_rule: None, always_reset: false };
        MachineDef {
            entry: "A",
            reinit_on_becoming_relevant: false,
            max_transitions_per_frame: 3,
            skip_first_update_transition: true,
            states: vec![state("A", seq("a")), state("Via", None), state("B", seq("b"))],
            transitions: vec![
                TransitionDef {
                    from: "A",
                    to: "Via",
                    xfade: 0.0,
                    option: BlendOption::Linear,
                    rule: Some(|i, _| i.go),
                    auto: false,
                },
                TransitionDef {
                    from: "Via",
                    to: "B",
                    xfade: 0.2,
                    option: BlendOption::Linear,
                    rule: Some(|_, q| q.time("A") >= 0.0),
                    auto: false,
                },
            ],
        }
    }

    // A transition through a conduit takes the chain's last crossfade.
    #[test]
    fn crossfade_through_a_conduit() {
        let clips = Clips::default();
        let mut m = ActorMachine::new(machine(), 1, &clips);
        m.update(&Inp { go: false }, 0.1, &clips);
        assert_eq!(m.state(), "A");
        assert_eq!(m.time("A"), 0.0); // (no clip: length 0, the time held at 0)
        assert_eq!(m.remaining("A"), FLT_MAX);
        m.update(&Inp { go: true }, 0.1, &clips);
        assert_eq!(m.state(), "B");
        // (one 0.2 s entry, 0.1 s in: Linear alpha 0.5)
        assert!((m.weight(2) - 0.5).abs() < 1e-12);
        assert!((m.weight(0) - 0.5).abs() < 1e-12);
        m.update(&Inp { go: true }, 0.1, &clips);
        assert_eq!(m.weight(2), 1.0);
        assert_eq!(m.weight(0), 0.0);
    }

    // CreateMaskWeights: the filter bone and its descendants by depth, nothing above it.
    #[test]
    fn branch_mask_by_depth() {
        let names: Vec<String> = ["root", "spine", "arm", "hand"].iter().map(|s| s.to_string()).collect();
        let parents = [-1, 0, 1, 2];
        let w = branch_mask(&names, &parents, &[BranchFilter { bone: "Spine", depth: 2 }]);
        assert_eq!(w, vec![0.0, 0.5, 1.0, 1.0]);
    }
}
