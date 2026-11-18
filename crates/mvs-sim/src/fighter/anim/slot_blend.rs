//! An AnimBP's slot graph as the data states it (`FighterData.slot_holds.graph`), walked without a fighter: the
//! update of its stateful nodes ([`slot_blend_step`]) and the weighed parts of a montage under it
//! ([`slot_graph_parts`]).
//!
//! A BlendListByBool with a blend time (Finn_AnimBP: IsFalling [1.0, 0.1] over Moving [0.1, 0.1]; Jake, Tom and
//! Jerry, Velma, C023A, C023B the same kind) is FAnimNode_BlendListBase::Update 1424936b0 with the FAlphaBlend rule
//! (FAlphaBlend::Update 14282a910; GetActiveChildIndex 14247a540: true -> pose 0): on a change of the active child
//! each child's weight goes linearly from its weight at the switch to 1 / 0 over BlendTime[new] x (1 - the new
//! child's weight), the first step in the update of the switch, snapping when the remaining time is <= the delta;
//! the first update after the instance's Initialize snaps (LastActiveChildIndex INDEX_NONE). A child at or under
//! 1e-5 (ZERO_ANIMWEIGHT_THRESH) is not updated: a timed list under it holds its state (its input is not read, its
//! blend does not run) until it is updated again.
//! On a switch with no blend time the child left is updated once more at weight 0 (BlendListBase::Update
//! 1424936b0), and a list with bResetChildOnActivation initialises the child it switches to (C023A_AnimBP
//! BlendListByBool_6).
//! The update walk follows what the build updates from the root: a Slot node's source when the shown montage has
//! no track in its slot, or bAlwaysUpdateSourcePose (`slot_holds.nodes`), or the source's weight (1 - the
//! montage's) above 1e-5; a TwoWayBlend's A when alpha < 0.99999 and B when alpha > 1e-5 (FAnimNode_TwoWayBlend
//! 1424946e0); a list's children above 1e-5 after its own step; a LayeredBoneBlend's base always and a blend pose
//! when its weight is above 1e-5; a state machine's states as its crossfades weigh them. A SaveCachedPose's source
//! (the import follows UseCachedPose to it; `SlotGraph.cached`, an opted-in character's data) is not updated where
//! the walk reaches it: each use files its context, and after the root's walk each source is updated once with the
//! heaviest context filed (ties: the first), in the AnimGraph's OrderedSavedPoseNodeIndices order, none without a
//! context (FAnimNode_SaveCachedPose: UpdateAnimationNode_WithRoot's PostGraphUpdate). An Initialize that reaches a
//! cached source stops there (SaveCachedPose::Initialize 14285cc60 returns within the update).
//! Unverified: the OrderedSavedPoseNodeIndices order is taken as the data's list order.
//! Deviation: a graph without `cached` (a character not opted in) updates a node each time the walk reaches it: a
//! timed list under a cached pose reached on two paths in one walk is stepped twice that frame (Jake_AnimBP's
//! BlendListByBool_3 under Slot_21 and the TwoWayBlends over it; Tom and Jerry's lists the same).
//! A list with all blend times 0 is not stepped: it is read live where it is walked (C020_AnimBPV2's two), which is
//! the same pose as the snapped step except that the build holds an un-updated one.
//! Deviation: the pose ([`slot_graph_parts`]) takes a child at or under 1e-5 out and the other at full weight (the
//! update rule's threshold, taken for the evaluation too: at most 1e-5 of weight moves).
//!
//! SequencePlayers and state machines ([`SlotPlayers`], only for a graph that has one: [`has_players`]). A
//! SequencePlayer is a pose of its own ([`SlotPart::Seq`], [`slot_player_pose`]): its time advances by dt x PlayRate
//! when it is updated, wrapped (looping) or clamped to its length (FAnimationRuntime::AdvanceTime), and is held
//! while it is not (a player under a list's branch at weight 0 keeps its time and resumes there); the sequence's own
//! RateScale multiplies in (UAnimSequenceBase::TickAssetPlayer: RateScale x the record's play rate x delta); a
//! sequence missing from the box index warns and holds; an Initialize sets it to StartPosition. A state machine is
//! FAnimNode_StateMachine's update: the current state's transitions in
//! their baked order, a rule its result's input compared with bDesiredTransitionReturnValue or, with
//! bAutomaticRemainingTimeRule, the most relevant player's time left <= the crossfade; a conduit's entry rule first;
//! up to MaxTransitionsPerFrame a frame; a crossfade of xfade x CalculateInverseAlpha(1 - the target's weight);
//! bSkipFirstUpdateTransition drops the crossfades of the first update; bReinitializeOnBecomingRelevant starts it
//! over at its entry state when it missed an instance update; a state entered at weight 0 (or bAlwaysResetOnEntry)
//! initialises its pose. Its pose: each state's at GetStateWeight.
//! Deviation: the notifies of these players are not fired (the sim fires the locomotion's, CharDef.seqNotifies); a
//! player's sync group or bound play rate (opts) is refused with a warning; a LinkedAnimLayer (src) under a node
//! that is initialised is not (the locomotion's machines are the fighter's own).

use crate::Real;
use crate::State;
use crate::math::{max, min};
use mvs_data::fighter::{SlotGraph, SlotGraphNode, SlotMachineNode};
use mvs_data::value::{Props, Value};

/// ZERO_ANIMWEIGHT_THRESH.
pub const ZERO_W: Real = 0.00001;
/// FAnimNode_TwoWayBlend: B alone at and over.
const ALPHA_ONE: Real = 0.99999;
/// FLT_MAX.
pub const FLT_MAX: Real = 3.402_823_5e38;

/// The timed lists' state of one anim instance, per graph node (by node index, filled at the first update; an entry
/// of a node that is no timed list is unused). Sim state of the character's state record (`SlotMix::slot_blend`).
/// `last`: the active child (-1 before the first update: INDEX_NONE); `w`: child 0's weight (child 1's is 1 - w);
/// `from`: child 0's weight at the switch; `rem`: the blend's remaining time; `time`: its whole time.
#[derive(State, Debug, Default)]
pub struct SlotBlend {
    pub last: Vec<i32>,
    pub w: Vec<Real>,
    pub from: Vec<Real>,
    pub rem: Vec<Real>,
    pub time: Vec<Real>,
}

/// `slotBlendReset`: the anim instance's Initialize, every list back to INDEX_NONE (a character's matchReset, a new
/// pawn).
pub fn slot_blend_reset(s: &mut SlotBlend) {
    s.last.clear();
    s.w.clear();
    s.from.clear();
    s.rem.clear();
    s.time.clear();
}

/// FAlphaBlend::AlphaToBlendOption of a machine's crossfade: the two the port takes.
#[derive(State, Copy, PartialEq, Eq, Debug, Default)]
pub enum BlendOpt {
    #[default]
    Linear,
    HermiteCubic,
}

/// A state machine's active crossfade (six numbers: from, to, duration, elapsed, alpha, blend
/// option).
#[derive(State, Debug, Default)]
pub struct Crossfade {
    pub from: i32,
    pub to: i32,
    pub dur: Real,
    pub el: Real,
    pub alpha: Real,
    pub opt: BlendOpt,
}

/// The SequencePlayers' and state machines' state of one anim instance (and the untimed lists' active child, for
/// the 0 s switch and the reset on activation), per graph node, for a graph that has a player, a machine or a list
/// with bResetChildOnActivation ([`has_players`]). Sim state of the character's state record
/// (`SlotMix::slot_players`), [`slot_players_reset`] at its matchReset. `frame`: the instance's update count (a
/// machine's relevance); `time` / `w`: a player's position and the weight of its last update; `seen`: the frame a
/// machine was last updated (-1: never); `last`: an untimed list's active child (-1: INDEX_NONE); `cur` / `first` /
/// `act`: a machine's current state, true before its first update, its active crossfades.
#[derive(State, Debug, Default)]
pub struct SlotPlayers {
    pub frame: i32,
    pub time: Vec<Real>,
    pub w: Vec<Real>,
    pub seen: Vec<i32>,
    pub last: Vec<i32>,
    pub cur: Vec<i32>,
    pub first: Vec<bool>,
    pub act: Vec<Vec<Crossfade>>,
}

/// `to` made a copy of `p`, keeping `to`'s lists' memory.
pub fn slot_players_copy(to: &mut SlotPlayers, p: &SlotPlayers) {
    to.frame = p.frame;
    to.time.clone_from(&p.time);
    to.w.clone_from(&p.w);
    to.seen.clone_from(&p.seen);
    to.last.clone_from(&p.last);
    to.cur.clone_from(&p.cur);
    to.first.clone_from(&p.first);
    to.act.resize_with(p.act.len(), Vec::new);
    for (a, b) in to.act.iter_mut().zip(&p.act) {
        a.clone_from(b);
    }
}

/// `slotPlayersReset`.
pub fn slot_players_reset(p: &mut SlotPlayers) {
    p.frame = 0;
    p.time.clear();
    p.w.clear();
    p.seen.clear();
    p.last.clear();
    p.cur.clear();
    p.first.clear();
    p.act.clear();
}

/// A node index of the data.
fn ix(i: i32) -> usize {
    usize::try_from(i).expect("a slot graph node index")
}

/// The JavaScript truthiness of a data value.
fn truthy(v: &Value) -> bool {
    match v {
        Value::Null => false,
        Value::Bool(b) => *b,
        Value::Num(n) => *n != 0.0 && !n.is_nan(),
        Value::Str(s) => !s.is_empty(),
        Value::Arr(_) | Value::Obj(_) | Value::F32s(_) => true,
    }
}

/// A BlendListByBool with a blend time.
fn is_timed(n: &SlotGraphNode) -> bool {
    matches!(n, SlotGraphNode::Bool { blend, .. } if blend.iter().any(|&t| t != 0.0))
}

/// A BlendListByBool with bResetChildOnActivation.
fn resets(n: &SlotGraphNode) -> bool {
    matches!(n, SlotGraphNode::Bool { opts: Some(o), .. } if o.get("bResetChildOnActivation").is_some_and(truthy))
}

/// `hasTimedLists`: the graph has a BlendListByBool with a blend time.
pub fn has_timed_lists(g: &SlotGraph) -> bool {
    g.nodes.iter().any(is_timed)
}

/// `hasPlayers`: the graph has a SequencePlayer, a state machine or a list that resets its child ([`SlotPlayers`]).
pub fn has_players(g: &SlotGraph) -> bool {
    g.nodes.iter().any(|n| matches!(n, SlotGraphNode::Seq { .. } | SlotGraphNode::Machine(_)) || resets(n))
}

/// The node of an asset player index (a state's PlayerNodeIndices, GetInstanceAssetPlayerTimeFromEnd's argument):
/// the last SequencePlayer of that index, -1 with none.
fn player_node(g: &SlotGraph, player: i32) -> i32 {
    g.nodes
        .iter()
        .rposition(|n| matches!(n, SlotGraphNode::Seq { player: p, .. } if *p == player))
        .map_or(-1, |i| i32::try_from(i).expect("a slot graph node index"))
}

/// A sequence's length in seconds and its RateScale (`SeqInfo`).
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct SeqInfo {
    pub length: Real,
    pub rate_scale: Real,
}

/// `SlotGraphIn`: the walk's view of the instance, the graph's inputs (a bool as 0 / 1; NaN for one the character
/// does not give is the implementer's error).
pub trait SlotGraphIn {
    /// For the errors: the character's key.
    fn who(&self) -> &str;
    /// `warnOnce`: the walk's warnings, once a match (`fighter::common::warn_text`).
    fn warn(&self, msg: &str);
    /// An input of the graph by its name.
    fn input(&self, name: &str) -> Real;
    /// The walk's players as they stand, filed before a rule's input is read: a rule may read the instance's own
    /// players (GetRelevantAnimTimeRemaining, the player time from its end), which the walk holds out of the mix.
    fn live_players(&self, _p: &SlotPlayers) {}
    /// A LayeredBoneBlend or ApplyAdditive of a character not opted into the node kinds ('other') the character
    /// takes as its base pose alone (slotInput 0 for it).
    fn layered(&self, node: &str, kind: &str) -> bool;
    /// A state machine 'other' node of the character's own AnimBP that its code updates and poses: the walk ends
    /// there, taken as the locomotion part.
    fn source(&self, _node: &str, _kind: &str) -> bool {
        false
    }
    /// A LayeredBoneBlend ('layered'): the pose the shapes take whole (0 the base, i + 1 blend pose i;
    /// CharHooks.slotLayer). `None`: no such hook (NaN).
    fn layer(&self, _node: &str) -> Option<Real> {
        None
    }
    /// A sequence's length and RateScale; `None` for one the data lacks (the implementer warns).
    fn anim(&self, _anim: &str) -> Option<SeqInfo> {
        None
    }
}

/// `refuseOpts`: a list's FAnimNode_BlendListBase settings other than the ones ported here (a standard transition, a
/// linear FAlphaBlend, no blend profile or custom curve; bResetChildOnActivation is ported) are refused with a
/// warning and the list is walked as a standard one.
fn refuse_opts(node: &str, opts: Option<&Props>, gi: &dyn SlotGraphIn) {
    if let Some(o) = opts
        && o.keys().any(|k| k != "bResetChildOnActivation")
    {
        gi.warn(&format!(
            "{}: {node} has {o:?}: only StandardBlend / Linear / no blend profile or curve is ported",
            gi.who()
        ));
    }
}

/// FAlphaBlend::AlphaToBlendOption: HermiteCubic is SmoothStep(0, 1, x); Linear the clamped x.
fn option_of(o: &str, who: &str) -> BlendOpt {
    match o {
        "Linear" => BlendOpt::Linear,
        "HermiteCubic" => BlendOpt::HermiteCubic,
        _ => panic!("{who}: a state machine crossfade of blend option {o} is not ported"),
    }
}

fn alpha_of(o: BlendOpt, x: Real) -> Real {
    let c = min(max(x, 0.0), 1.0);
    if o == BlendOpt::HermiteCubic { c * c * (3.0 - 2.0 * c) } else { c }
}

/// CalculateInverseAlpha (the crossfade's duration scale).
fn inverse_alpha(o: BlendOpt, x: Real) -> Real {
    if o == BlendOpt::HermiteCubic {
        return x * x * x * (4.0 / 3.0) - 2.0 * x * x + x * (5.0 / 3.0);
    }
    min(max(x, 0.0), 1.0)
}

/// Writes `v[i]`, growing the list with `fill` (a JavaScript array written past its end: its holes read as
/// `?? fill`).
fn put<T: Copy>(v: &mut Vec<T>, i: usize, x: T, fill: T) {
    if v.len() <= i {
        v.resize(i + 1, fill);
    }
    v[i] = x;
}

/// `stepList`: one timed list's update by dt (the active child from its input, the switch, the FAlphaBlend step).
/// Gives the child left on a switch with no blend time (updated once more at weight 0), else -1. Public for a view
/// that steps a list the sim walk does not reach (the list under a LayeredBoneBlend taken as its base).
pub fn step_list(s: &mut SlotBlend, id: usize, n: &SlotGraphNode, on: bool, dt: Real) -> i32 {
    let SlotGraphNode::Bool { node, blend, poses, .. } = n else {
        panic!("step_list: node {id} is no BlendListByBool")
    };
    assert!(blend.len() == poses.len(), "{node}: BlendTime has {} entries for {} poses", blend.len(), poses.len());
    let child: i32 = if on { 0 } else { 1 };
    let last = s.last.get(id).copied().unwrap_or(-1);
    let mut left = -1;
    if child != last {
        let w = s.w.get(id).copied().unwrap_or(0.0);
        let wc = if child == 0 { w } else { 1.0 - w };
        let t = if last < 0 { 0.0 } else { crate::math::narrow(blend[ix(child)]) * min((1.0 - wc).abs(), 1.0) };
        if t == 0.0 && last >= 0 {
            left = last;
        }
        put(&mut s.last, id, child, -1);
        put(&mut s.from, id, w, 0.0);
        put(&mut s.time, id, t, 0.0);
        put(&mut s.rem, id, t, 0.0);
    }
    let rem = s.rem[id];
    // BlendTimeRemaining, a float
    s.rem[id] = if rem > dt { rem - dt } else { 0.0 };
    let to = if child == 0 { 1.0 } else { 0.0 };
    let w = if s.time[id] > 0.0 { to + (s.from[id] - to) * (s.rem[id] / s.time[id]) } else { to };
    put(&mut s.w, id, w, 0.0);
    left
}

/// A machine's GetStateWeight.
fn state_weight(p: &SlotPlayers, id: usize, st: i32) -> Real {
    let a = &p.act[id];
    if a.is_empty() {
        return if st == p.cur[id] { 1.0 } else { 0.0 };
    }
    let mut w = 0.0;
    for (i, c) in a.iter().enumerate() {
        let al = c.alpha;
        if i == 0 {
            w = if c.from == st { 1.0 - al } else { 0.0 };
        } else {
            w *= 1.0 - al;
        }
        if c.to == st {
            w += al;
        }
    }
    min(max(w, 0.0), 1.0)
}

/// A machine state's most relevant player (the heaviest above 0 not bIgnoreForRelevancyTest, the first of
/// equals): its sequence's length minus its time; FLT_MAX with none or with no sequence
/// (GetRelevantAssetPlayerFromState 142856350). The automatic transition rule and GetRelevantAnimTimeRemaining read
/// the same.
fn relevant_remaining(
    g: &SlotGraph,
    p: &SlotPlayers,
    n: &SlotMachineNode,
    st: usize,
    who: &str,
    len: &dyn Fn(&str) -> Real,
) -> Real {
    let mut best = -1;
    let mut most = 0.0;
    for &pl in &n.states[st].players {
        let i = player_node(g, pl);
        assert!(
            i >= 0,
            "{who}: {} state {} has asset player {pl}, which is no SequencePlayer of the slot graph",
            n.node,
            n.states[st].name
        );
        let SlotGraphNode::Seq { ignore, .. } = &g.nodes[ix(i)] else { unreachable!() };
        if ignore != &Some(true) && p.w[ix(i)] > most {
            most = p.w[ix(i)];
            best = i;
        }
    }
    if best < 0 {
        return FLT_MAX;
    }
    let SlotGraphNode::Seq { anim, .. } = &g.nodes[ix(best)] else { unreachable!() };
    let l = anim.as_deref().map_or(0.0, len);
    // no sequence, or one missing from the box index (warned, length 0): FLT_MAX as the build with no asset; float32
    // L - t
    if l > 0.0 { l - p.time[ix(best)] } else { FLT_MAX }
}

fn two_alpha(n: &SlotGraphNode, gi: &dyn SlotGraphIn) -> Real {
    let SlotGraphNode::Two { node, input, ty, alpha, scale_bias, .. } = n else { unreachable!() };
    assert!(
        ty == "Float" && scale_bias.is_none(),
        "{}: {node}: a TwoWayBlend alpha of type {ty}{} is not ported",
        gi.who(),
        if scale_bias.is_some() { " with a scale / bias / clamp" } else { "" }
    );
    min(max(input.as_deref().map_or(crate::math::narrow(*alpha), |i| gi.input(i)), 0.0), 1.0)
}

fn bool_on(input: Option<&str>, value: bool, gi: &dyn SlotGraphIn) -> bool {
    input.map_or(value, |i| gi.input(i) != 0.0)
}

/// BlendWeights[i]: the bound input (one blend pose) or the class default, clamped 0..1.
fn layer_weight(n: &SlotGraphNode, i: usize, gi: &dyn SlotGraphIn) -> Real {
    let SlotGraphNode::Layered { node, blends, input, weights, .. } = n else { unreachable!() };
    assert!(
        input.is_none() || blends.len() == 1,
        "{}: {node}: a bound BlendWeights over {} blend poses is not ported",
        gi.who(),
        blends.len()
    );
    let v = match input {
        Some(x) => gi.input(x),
        None => crate::math::narrow(weights.get(i).copied().unwrap_or(0.0)),
    };
    min(max(v, 0.0), 1.0)
}

/// The update walk of [`slot_blend_step`]: the instance's state and the walk's own marks (a node updated, a cached
/// source's context filed with its weight, its update done).
struct Step<'a> {
    g: &'a SlotGraph,
    s: Option<&'a mut SlotBlend>,
    p: Option<&'a mut SlotPlayers>,
    gi: &'a dyn SlotGraphIn,
    source_updated: &'a mut dyn FnMut(&str, &str) -> bool,
    dt: Real,
    sc: &'a mut SlotScratch,
}

/// [`slot_blend_step`]'s buffers, reused from walk to walk (never state: each walk writes them before it reads them):
/// the cached sources (`cachedOf`), the walk's marks (`stamp`, `filed`, `ctxW`, `ran`, numbered by
/// `walkNo`) and the machines' transition search (`chain`, `visited`) and updated states (`done`, a
/// stack: a nested machine's run above its parent's).
#[derive(Clone, Debug, Default)]
pub struct SlotScratch {
    cached: Vec<bool>,
    stamp: Vec<bool>,
    filed: Vec<bool>,
    ctx_w: Vec<Real>,
    ran: Vec<bool>,
    chain: Vec<usize>,
    visited: Vec<i32>,
    done: Vec<i32>,
}

impl Step<'_> {
    fn p(&mut self) -> &mut SlotPlayers {
        self.p.as_deref_mut().expect("a slot graph's players: CharHooks.slotMix needs a slotPlayers")
    }

    fn s(&mut self) -> &mut SlotBlend {
        self.s.as_deref_mut().expect("a slot graph's timed list: CharHooks.slotMix needs a slotBlend")
    }

    /// A node's Initialize (a list's bResetChildOnActivation, a machine's state entered at weight 0 or its
    /// restart).
    fn init(&mut self, id: usize) {
        if self.sc.cached[id] {
            return; // SaveCachedPose::Initialize within the update: returns
        }
        let g = self.g;
        let n = &g.nodes[id];
        match n {
            SlotGraphNode::Seq { start, .. } => {
                let p = self.p();
                p.time[id] = crate::math::narrow(*start);
                p.w[id] = 0.0;
            }
            SlotGraphNode::Slot { src, .. } => self.init(ix(*src)),
            SlotGraphNode::Two { a, b, .. } => {
                self.init(ix(*a));
                self.init(ix(*b));
            }
            SlotGraphNode::Bool { poses, .. } => {
                if let Some(s) = self.s.as_deref_mut()
                    && is_timed(n)
                {
                    put(&mut s.last, id, -1, -1);
                    put(&mut s.w, id, 0.0, 0.0);
                    put(&mut s.from, id, 0.0, 0.0);
                    put(&mut s.rem, id, 0.0, 0.0);
                    put(&mut s.time, id, 0.0, 0.0);
                } else if let Some(p) = self.p.as_deref_mut() {
                    put(&mut p.last, id, -1, -1);
                }
                for &c in poses {
                    self.init(ix(c));
                }
            }
            SlotGraphNode::Layered { base, blends, .. } => {
                self.init(ix(*base));
                for &c in blends {
                    self.init(ix(c));
                }
            }
            SlotGraphNode::Machine(m) => self.restart(id, m),
            SlotGraphNode::Src { .. } => {}
            SlotGraphNode::Other { poses, .. } => {
                for &c in poses {
                    self.init(ix(c));
                }
            }
        }
    }

    fn restart(&mut self, id: usize, n: &SlotMachineNode) {
        let p = self.p();
        p.cur[id] = n.entry;
        p.first[id] = true;
        p.act[id].clear();
        for st in &n.states {
            if let Some(pose) = st.pose {
                self.init(ix(pose));
            }
        }
    }

    fn walk(&mut self, id: usize, w: Real, post: bool) {
        let g = self.g;
        if self.sc.cached[id] && !post {
            // a use of a cached pose: its context filed, the source updated after the root (below)
            if self.sc.ran[id] {
                self.gi.warn(&format!(
                    "{}: {}: a cached pose is used after its update in this walk (OrderedSavedPoseNodeIndices order): the use is not updated",
                    self.gi.who(),
                    node_name(&g.nodes[id])
                ));
            } else if !self.sc.filed[id] {
                self.sc.filed[id] = true;
                self.sc.ctx_w[id] = w;
            } else if w > self.sc.ctx_w[id] {
                self.sc.ctx_w[id] = w;
            }
            return;
        }
        let n = &g.nodes[id];
        if g.cached.is_some() {
            if self.sc.stamp[id] {
                return; // (a guard: a node reached twice is a cached source, filed above)
            }
            self.sc.stamp[id] = true;
        }
        match n {
            SlotGraphNode::Slot { node, slot, src } => {
                if (self.source_updated)(node, slot) {
                    self.walk(ix(*src), w, false);
                }
            }
            SlotGraphNode::Two { a, b, .. } => {
                let al = two_alpha(n, self.gi);
                if al < ALPHA_ONE {
                    self.walk(ix(*a), w * (1.0 - al), false);
                }
                if al > ZERO_W {
                    self.walk(ix(*b), w * al, false);
                }
            }
            SlotGraphNode::Bool { node, input, value, poses, opts, .. } => {
                refuse_opts(node, opts.as_ref(), self.gi);
                let on = bool_on(input.as_deref(), *value, self.gi);
                let child: i32 = if on { 0 } else { 1 };
                if !is_timed(n) {
                    if let Some(p) = self.p.as_deref_mut() {
                        let last = p.last[id];
                        if child != last {
                            p.last[id] = child;
                            if last >= 0 {
                                self.walk(ix(poses[ix(last)]), 0.0, false);
                            }
                            if resets(n) {
                                self.init(ix(poses[ix(child)]));
                            }
                        }
                    }
                    self.walk(ix(poses[ix(child)]), w, false);
                    return;
                }
                let dt = self.dt;
                let s = self.s();
                let switched = s.last[id] != child;
                let left = step_list(s, id, n, on, dt);
                if left >= 0 {
                    self.walk(ix(poses[ix(left)]), 0.0, false);
                }
                if switched && resets(n) {
                    self.init(ix(poses[ix(child)]));
                }
                let w0 = self.s().w[id];
                if w0 > ZERO_W {
                    self.walk(ix(poses[0]), w * w0, false);
                }
                if 1.0 - w0 > ZERO_W {
                    self.walk(ix(poses[1]), w * (1.0 - w0), false);
                }
            }
            SlotGraphNode::Src { .. } => {}
            SlotGraphNode::Seq { .. } => self.advance(id, n, w),
            SlotGraphNode::Layered { base, blends, .. } => {
                self.walk(ix(*base), w, false);
                for (i, &b) in blends.iter().enumerate() {
                    let bw = layer_weight(n, i, self.gi);
                    if bw > ZERO_W {
                        self.walk(ix(b), w * bw, false);
                    }
                }
            }
            SlotGraphNode::Machine(m) => self.machine(id, m, w),
            SlotGraphNode::Other { node, kind, poses } => {
                if self.gi.layered(node, kind) {
                    self.walk(ix(poses[0]), w, false);
                    return;
                }
                if self.gi.source(node, kind) {
                    return; // the character's own update
                }
                panic!("{}: {node} ({kind}) on the slot graph is not ported", self.gi.who());
            }
        }
    }

    fn info_of(&self, anim: Option<&str>) -> Option<SeqInfo> {
        anim.and_then(|a| self.gi.anim(a))
    }

    fn advance(&mut self, id: usize, n: &SlotGraphNode, w: Real) {
        let SlotGraphNode::Seq { node, anim, rate, looping, opts, .. } = n else { unreachable!() };
        if let Some(o) = opts {
            self.gi.warn(&format!(
                "{}: {node} has {o:?}: only a SequencePlayer with a constant play rate and no sync group is ported",
                self.gi.who()
            ));
        }
        self.p().w[id] = w;
        let Some(info) = self.info_of(anim.as_deref()) else { return }; // no sequence (or none in the data: warned): the time holds
        let len = info.length;
        let mv = (info.rate_scale * crate::math::narrow(*rate)) * self.dt;
        if mv == 0.0 {
            return;
        }
        // the player's FAnimationRuntime::AdvanceTime (142816220): the Fmod wrap (+ End below 0) when looping, else
        // clamped to the clip
        crate::math::ue::advance_time(*looping, mv, &mut self.p().time[id], len);
    }

    fn rule(&mut self, n: &SlotMachineNode, t: usize, from: usize) -> bool {
        self.file_players();
        let tr = &n.trans[t];
        if let Some(o) = &tr.opts {
            self.gi.warn(&format!(
                "{}: {} transition {t} has {o:?}: only a standard transition is ported",
                self.gi.who(),
                n.node
            ));
        }
        if tr.auto {
            let gi = self.gi;
            let len = |a: &str| gi.anim(a).map_or(0.0, |x| x.length);
            let g = self.g;
            return relevant_remaining(g, self.p(), n, from, gi.who(), &len) <= crate::math::narrow(tr.xfade);
        }
        bool_on(tr.input.as_deref(), tr.value, self.gi) == tr.want
    }

    /// [`SlotGraphIn::live_players`] with the walk's own.
    fn file_players(&self) {
        if let Some(p) = self.p.as_deref() {
            self.gi.live_players(p);
        }
    }

    /// FindValidTransition: the chain of transitions from st to a state that is no conduit.
    fn find(&mut self, n: &SlotMachineNode, st: i32) -> bool {
        if self.sc.visited.contains(&st) {
            return false;
        }
        self.sc.visited.push(st);
        let s0 = &n.states[ix(st)];
        if let Some(entry) = &s0.entry_in {
            self.file_players();
            let ok = match entry {
                None => s0.entry_value == Some(true),
                Some(i) => self.gi.input(i) != 0.0,
            };
            if !ok {
                return false;
            }
        }
        for &t in &s0.out {
            let t = ix(t);
            if !self.rule(n, t, ix(st)) {
                continue;
            }
            self.sc.chain.push(t);
            let to = n.trans[t].to;
            if n.states[ix(to)].pose.is_none() {
                if self.find(n, to) {
                    return true;
                }
                self.sc.chain.pop();
                continue;
            }
            return true;
        }
        false
    }

    fn machine(&mut self, id: usize, n: &SlotMachineNode, w: Real) {
        let who = self.gi.who();
        let pp = self.p();
        if n.reinit && pp.seen[id] >= 0 && pp.seen[id] != pp.frame - 1 {
            self.restart(id, n);
        }
        let pp = self.p();
        pp.seen[id] = pp.frame;
        let mut count = 0;
        loop {
            count += 1;
            self.sc.chain.clear();
            self.sc.visited.clear();
            let cur = self.p().cur[id];
            if !self.find(n, cur) {
                break;
            }
            let t = &n.trans[*self.sc.chain.last().expect("a transition chain")];
            let o = option_of(&t.blend, who);
            let pp = self.p();
            let dur = crate::math::narrow(t.xfade) * inverse_alpha(o, 1.0 - state_weight(pp, id, t.to));
            pp.act[id].push(Crossfade { from: pp.cur[id], to: t.to, dur, el: 0.0, alpha: 0.0, opt: o });
            // SetState: the state's players at weight 0; its pose initialised when it had no weight or always resets
            let wt = state_weight(pp, id, t.to);
            pp.cur[id] = t.to;
            let st = &n.states[ix(t.to)];
            for &pl in &st.players {
                let i = player_node(self.g, pl);
                if i >= 0 {
                    self.p().w[ix(i)] = 0.0;
                }
            }
            if let Some(pose) = st.pose
                && (wt <= 0.0 || st.always_reset)
            {
                self.init(ix(pose));
            }
            if count >= n.max {
                break;
            }
        }
        let dt = self.dt;
        let pp = self.p();
        if pp.first[id] && n.skip_first {
            pp.act[id].clear();
        }
        pp.first[id] = false;
        let mut last_done = None;
        for (i, c) in pp.act[id].iter_mut().enumerate() {
            c.el += dt;
            let q = if c.dur > 0.0 { c.el / c.dur } else { 1.0 };
            c.alpha = alpha_of(c.opt, q);
            if q >= 1.0 {
                last_done = Some(i);
            }
        }
        // this machine's updated states: the top of the stack from `base` (a nested machine's above, gone at its end)
        let base = self.sc.done.len();
        let upd = |me: &mut Self, st: i32, sw: Real| {
            let Some(pose) = n.states[ix(st)].pose else { return };
            if me.sc.done[base..].contains(&st) {
                return;
            }
            me.sc.done.push(st);
            me.walk(ix(pose), w * sw, false);
        };
        if self.p().act[id].is_empty() {
            let cur = self.p().cur[id];
            upd(self, cur, 1.0);
        } else {
            let mut i = 0;
            while i < self.p().act[id].len() {
                let (from, al) = (self.p().act[id][i].from, self.p().act[id][i].alpha);
                upd(self, from, 1.0 - al);
                let Some(c) = self.p().act[id].get(i) else { break };
                let (to, al) = (c.to, c.alpha);
                upd(self, to, al);
                i += 1;
            }
        }
        self.sc.done.truncate(base);
        if let Some(d) = last_done {
            let a = &mut self.p().act[id];
            let end = (d + 1).min(a.len());
            a.drain(..end);
        }
    }
}

/// The node's name, for the messages.
fn node_name(n: &SlotGraphNode) -> &str {
    match n {
        SlotGraphNode::Slot { node, .. }
        | SlotGraphNode::Two { node, .. }
        | SlotGraphNode::Bool { node, .. }
        | SlotGraphNode::Src { node, .. }
        | SlotGraphNode::Seq { node, .. }
        | SlotGraphNode::Layered { node, .. }
        | SlotGraphNode::Other { node, .. } => node,
        SlotGraphNode::Machine(m) => &m.node,
    }
}

/// `slotBlendStep`: the graph's update by dt (the instance's delta, taken as the build's float32 DeltaTime), from the
/// root along what is updated: the timed lists' FAlphaBlend (`s`), the players and machines (`p`, for a graph that
/// has one: [`has_players`]). `source_updated(node, slot)`: a Slot node's source is updated under the shown montage
/// (see the module's head). `sc`: the walk's buffers ([`SlotScratch`], the World's `scratch.slot_walk`, taken out for the walk).
pub fn slot_blend_step(
    g: &SlotGraph,
    mut s: Option<&mut SlotBlend>,
    dt: Real,
    gi: &dyn SlotGraphIn,
    source_updated: &mut dyn FnMut(&str, &str) -> bool,
    mut p: Option<&mut SlotPlayers>,
    sc: &mut SlotScratch,
) {
    let n0 = g.nodes.len();
    if let Some(s) = s.as_deref_mut()
        && s.last.len() != n0
    {
        // the first update of this instance: every list at INDEX_NONE
        s.last.clear();
        s.last.resize(n0, -1);
        for a in [&mut s.w, &mut s.from, &mut s.rem, &mut s.time] {
            a.clear();
            a.resize(n0, 0.0);
        }
    }
    if let Some(p) = p.as_deref_mut() {
        if p.time.len() != n0 {
            // the instance's Initialize: every player at its StartPosition, every machine at its entry state
            p.w.clear();
            p.w.resize(n0, 0.0);
            p.seen.clear();
            p.seen.resize(n0, -1);
            p.last.clear();
            p.last.resize(n0, -1);
            p.first.clear();
            p.first.resize(n0, true);
            p.act.clear();
            p.time.clear();
            p.cur.clear();
            for n in &g.nodes {
                p.time.push(if let SlotGraphNode::Seq { start, .. } = n { crate::math::narrow(*start) } else { 0.0 });
                p.cur.push(if let SlotGraphNode::Machine(m) = n { m.entry } else { 0 });
                p.act.push(Vec::new());
            }
        }
        p.frame += 1;
    } else if has_players(g) {
        panic!(
            "{}: its slot graph has a SequencePlayer, a state machine or a list that resets its child: CharHooks.slotMix needs a slotPlayers",
            gi.who()
        );
    }
    sc.cached.clear();
    sc.cached.resize(n0, false);
    for &id in g.cached.iter().flatten() {
        sc.cached[ix(id)] = true;
    }
    let marks = if g.cached.is_some() { n0 } else { 0 };
    for a in [&mut sc.stamp, &mut sc.filed, &mut sc.ran] {
        a.clear();
        a.resize(marks, false);
    }
    sc.ctx_w.clear();
    sc.ctx_w.resize(marks, 0.0);
    sc.done.clear();
    let mut st = Step { g, s, p, gi, source_updated, dt, sc };
    st.walk(ix(g.root), 1.0, false);
    // PostGraphUpdate: each cached source with a context, once, with its heaviest
    if let Some(c) = &g.cached {
        for &id in c {
            let id = ix(id);
            if st.sc.filed[id] {
                st.sc.ran[id] = true;
                let w = st.sc.ctx_w[id];
                st.walk(id, w, true);
            }
        }
    }
}

/// A part of [`slot_graph_parts`]' output: a Slot node's slot the montage has a track in, the locomotion target, or
/// a SequencePlayer by its node index (the `SEQ_PART` + index key).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SlotPart<'g> {
    Slot(&'g str),
    Loco,
    Seq(usize),
}

/// `slotPlayerPose`: a SequencePlayer's pose, the sequence and the time `p` holds (its StartPosition before the first
/// update). `None` for a node that is no player with a sequence.
pub fn slot_player_pose<'g>(g: &'g SlotGraph, p: Option<&SlotPlayers>, id: usize) -> Option<(&'g str, Real)> {
    let SlotGraphNode::Seq { anim: Some(anim), start, .. } = g.nodes.get(id)? else { return None };
    let time = match p {
        Some(p) if p.time.len() == g.nodes.len() => p.time[id],
        _ => crate::math::narrow(*start),
    };
    Some((anim, time))
}

/// `slotPlayerTimeFromEnd`: UAnimInstance::GetInstanceAssetPlayerTimeFromEnd (read in the exe), the length minus the
/// time of the player of that asset player index, whatever its weight; FLT_MAX with no such player.
pub fn slot_player_time_from_end(
    g: &SlotGraph,
    p: Option<&SlotPlayers>,
    player: i32,
    len: &dyn Fn(&str) -> Real,
) -> Real {
    let id = player_node(g, player);
    if id < 0 {
        return FLT_MAX;
    }
    let id = ix(id);
    let SlotGraphNode::Seq { anim, start, .. } = &g.nodes[id] else { unreachable!() };
    let t = match p {
        Some(p) if p.time.len() == g.nodes.len() => p.time[id],
        _ => crate::math::narrow(*start),
    };
    anim.as_deref().map_or(0.0, len) - t
}

/// `slotPlayerTime`: UAnimInstance::GetInstanceAssetPlayerTime, the player's time (0 with no such player).
pub fn slot_player_time(g: &SlotGraph, p: Option<&SlotPlayers>, player: i32) -> Real {
    let id = player_node(g, player);
    if id < 0 {
        return 0.0;
    }
    let id = ix(id);
    match p {
        Some(p) if p.time.len() == g.nodes.len() => p.time[id],
        _ => {
            let SlotGraphNode::Seq { start, .. } = &g.nodes[id] else { unreachable!() };
            crate::math::narrow(*start)
        }
    }
}

/// `slotMachineTimeRemaining`: UAnimInstance::GetRelevantAnimTimeRemaining(MachineIndex, StateIndex) for a machine
/// of the slot graph (MachineIndex: the machine node's baked index, `SlotMachineNode::index`; StateIndex: the baked
/// state's): the state's most relevant player's time left by its weight of the last update, as the automatic rule
/// reads it; FLT_MAX with none (before the graph's first update too). A machine index the graph lacks, or a state
/// it lacks, is an error. C023A_AnimBP's TransitionResult_2 (@1409) and _3 (@3931) read (82, 1): Driving's
/// Turrning.
pub fn slot_machine_time_remaining(
    g: &SlotGraph,
    p: Option<&SlotPlayers>,
    machine: i32,
    state: i32,
    who: &str,
    len: &dyn Fn(&str) -> Real,
) -> Real {
    let found = g.nodes.iter().find_map(|n| match n {
        SlotGraphNode::Machine(m) if m.index == machine => Some(m),
        _ => None,
    });
    let Some(n) = found else {
        panic!(
            "{who}: GetRelevantAnimTimeRemaining({machine}, {state}): no state machine of node index {machine} on the slot graph"
        )
    };
    let st = usize::try_from(state).ok().filter(|&s| s < n.states.len());
    let Some(st) = st else {
        panic!("{who}: GetRelevantAnimTimeRemaining({machine}, {state}): {} has {} states", n.machine, n.states.len())
    };
    match p {
        Some(p) if p.time.len() == g.nodes.len() => relevant_remaining(g, p, n, st, who, len),
        _ => FLT_MAX,
    }
}

/// `slotMachineState`: a machine's current state's name (GetCurrentStateName) by the machine's name; its entry state
/// before its first update; `None` with no such machine on the graph.
pub fn slot_machine_state<'g>(g: &'g SlotGraph, p: Option<&SlotPlayers>, machine: &str) -> Option<&'g str> {
    let id = g.nodes.iter().position(|n| matches!(n, SlotGraphNode::Machine(m) if m.machine == machine))?;
    let SlotGraphNode::Machine(n) = &g.nodes[id] else { unreachable!() };
    let st = match p {
        Some(p) if p.cur.len() == g.nodes.len() => p.cur[id],
        _ => n.entry,
    };
    Some(&n.states[ix(st)].name)
}

/// `slotGraphParts`: the weighed parts of the pose from the root: a Slot node whose slot the montage has a track in
/// (`has_track`), the locomotion target, a SequencePlayer. A timed list is read from `s` (its weights after this
/// frame's update); one not updated yet (or with `s` `None`) by its input, at once. A machine by its states' weights
/// in `p` (its entry state before its first update).
pub fn slot_graph_parts<'g>(
    g: &'g SlotGraph,
    s: Option<&SlotBlend>,
    gi: &dyn SlotGraphIn,
    has_track: &mut dyn FnMut(&str) -> bool,
    parts: &mut Vec<(SlotPart<'g>, Real)>,
    p: Option<&SlotPlayers>,
) {
    let ready = p.filter(|p| p.time.len() == g.nodes.len());
    parts_walk(g, s, gi, has_track, parts, ready, ix(g.root), 1.0);
}

/// Whether the graph's update from the root reaches the locomotion target (a source node, or an 'other' node the
/// character takes as it): the update walk's rules ([`slot_blend_step`], the module's head) read without a step. A
/// Slot node passes its update on when `source_updated(node, slot)`; a TwoWayBlend updates A under 0.99999 and B over
/// 0.00001; a timed list the children over 0.00001 by its weights in `s` (by its input before its first update or
/// with no `s`), an untimed one its active child; a LayeredBoneBlend its base and each blend pose over 0.00001; a
/// machine each state with weight (its entry state before its first update).
// Unverified: a timed list's and a machine's weights are those of their last step, read wherever the caller stands
// in the frame (the build's update walk reads them as it steps them).
pub fn slot_graph_source_updated(
    g: &SlotGraph,
    s: Option<&SlotBlend>,
    gi: &dyn SlotGraphIn,
    source_updated: &mut dyn FnMut(&str, &str) -> bool,
    p: Option<&SlotPlayers>,
) -> bool {
    assert!(
        g.nodes.len() <= SEEN_MAX,
        "{}: a slot graph of {} nodes (the reach walk marks {SEEN_MAX})",
        gi.who(),
        g.nodes.len()
    );
    let mut seen = [0u64; SEEN_MAX / 64];
    let ready = p.filter(|p| p.time.len() == g.nodes.len());
    let s = s.filter(|s| s.last.len() == g.nodes.len());
    reach_walk(g, s, gi, source_updated, ready, &mut seen, ix(g.root))
}

/// The reach walk's marks (a bit a node).
const SEEN_MAX: usize = 1024;

fn reach_walk(
    g: &SlotGraph,
    s: Option<&SlotBlend>,
    gi: &dyn SlotGraphIn,
    source_updated: &mut dyn FnMut(&str, &str) -> bool,
    ready: Option<&SlotPlayers>,
    seen: &mut [u64; SEEN_MAX / 64],
    id: usize,
) -> bool {
    let bit = 1u64 << (id % 64);
    if seen[id / 64] & bit != 0 {
        return false; // (reached before on another path: its answer was given there)
    }
    seen[id / 64] |= bit;
    let walk = |id: i32, seen: &mut [u64; SEEN_MAX / 64], su: &mut dyn FnMut(&str, &str) -> bool| {
        reach_walk(g, s, gi, su, ready, seen, ix(id))
    };
    let n = &g.nodes[id];
    match n {
        SlotGraphNode::Slot { node, slot, src } => source_updated(node, slot) && walk(*src, seen, source_updated),
        SlotGraphNode::Two { a, b, .. } => {
            let al = two_alpha(n, gi);
            (al < ALPHA_ONE && walk(*a, seen, source_updated)) || (al > ZERO_W && walk(*b, seen, source_updated))
        }
        SlotGraphNode::Bool { input, value, poses, .. } => {
            let last = match s {
                Some(s) if is_timed(n) => s.last[id],
                _ => -1,
            };
            if last < 0 {
                let on = bool_on(input.as_deref(), *value, gi);
                return walk(poses[if on { 0 } else { 1 }], seen, source_updated);
            }
            let w0 = s.expect("a timed list's state").w[id];
            (w0 > ZERO_W && walk(poses[0], seen, source_updated))
                || (1.0 - w0 > ZERO_W && walk(poses[1], seen, source_updated))
        }
        SlotGraphNode::Src { .. } => true,
        SlotGraphNode::Seq { .. } => false,
        SlotGraphNode::Layered { base, blends, .. } => {
            if walk(*base, seen, source_updated) {
                return true;
            }
            for (i, &b) in blends.iter().enumerate() {
                if layer_weight(n, i, gi) > ZERO_W && walk(b, seen, source_updated) {
                    return true;
                }
            }
            false
        }
        SlotGraphNode::Machine(m) => {
            let Some(p) = ready else {
                return m.states[ix(m.entry)].pose.is_some_and(|pose| walk(pose, seen, source_updated));
            };
            for (st, state) in m.states.iter().enumerate() {
                let Some(pose) = state.pose else { continue };
                if state_weight(p, id, i32::try_from(st).expect("a machine state index")) > 0.0
                    && walk(pose, seen, source_updated)
                {
                    return true;
                }
            }
            false
        }
        SlotGraphNode::Other { node, kind, poses } => {
            if gi.layered(node, kind) {
                return walk(poses[0], seen, source_updated);
            }
            if gi.source(node, kind) {
                return true;
            }
            panic!("{}: {node} ({kind}) on the slot graph is not ported", gi.who());
        }
    }
}

#[allow(clippy::too_many_arguments, reason = "the closure's captures")]
fn parts_walk<'g>(
    g: &'g SlotGraph,
    s: Option<&SlotBlend>,
    gi: &dyn SlotGraphIn,
    has_track: &mut dyn FnMut(&str) -> bool,
    parts: &mut Vec<(SlotPart<'g>, Real)>,
    ready: Option<&SlotPlayers>,
    id: usize,
    w: Real,
) {
    let walk = |id: i32, w: Real, has_track: &mut dyn FnMut(&str) -> bool, parts: &mut Vec<(SlotPart<'g>, Real)>| {
        parts_walk(g, s, gi, has_track, parts, ready, ix(id), w);
    };
    let n = &g.nodes[id];
    match n {
        SlotGraphNode::Slot { slot, src, .. } => {
            if has_track(slot) {
                parts.push((SlotPart::Slot(slot), w));
            } else {
                walk(*src, w, has_track, parts);
            }
        }
        SlotGraphNode::Two { a, b, .. } => {
            let al = two_alpha(n, gi);
            if al <= ZERO_W {
                walk(*a, w, has_track, parts);
            } else if al >= ALPHA_ONE {
                walk(*b, w, has_track, parts);
            } else {
                walk(*a, w * (1.0 - al), has_track, parts);
                walk(*b, w * al, has_track, parts);
            }
        }
        SlotGraphNode::Bool { node, input, value, poses, opts, .. } => {
            refuse_opts(node, opts.as_ref(), gi);
            let last = match s {
                Some(s) if is_timed(n) => s.last.get(id).copied().unwrap_or(-1),
                _ => -1,
            };
            if last < 0 {
                let on = bool_on(input.as_deref(), *value, gi);
                walk(poses[if on { 0 } else { 1 }], w, has_track, parts);
                return;
            }
            let w0 = s.expect("a timed list's state").w[id];
            if w0 <= ZERO_W {
                walk(poses[1], w, has_track, parts);
            } else if 1.0 - w0 <= ZERO_W {
                walk(poses[0], w, has_track, parts);
            } else {
                walk(poses[0], w * w0, has_track, parts);
                walk(poses[1], w * (1.0 - w0), has_track, parts);
            }
        }
        SlotGraphNode::Src { .. } => parts.push((SlotPart::Loco, w)),
        SlotGraphNode::Seq { .. } => parts.push((SlotPart::Seq(id), w)),
        SlotGraphNode::Layered { node, base, blends, .. } => {
            let k = gi.layer(node);
            let pose = match k {
                Some(0.0) => Some(*base),
                Some(k) if k >= 1.0 && k.fract() == 0.0 => {
                    usize::try_from(crate::math::to_int32(k - 1.0)).ok().and_then(|i| blends.get(i).copied())
                }
                _ => None,
            };
            let Some(pose) = pose else {
                panic!(
                    "{}: {node}: CharHooks.slotLayer gives no pose ({}) for this LayeredBoneBlend",
                    gi.who(),
                    k.map_or("NaN".to_string(), |k| k.to_string())
                )
            };
            walk(pose, w, has_track, parts);
        }
        SlotGraphNode::Machine(m) => {
            let Some(p) = ready else {
                if let Some(pose) = m.states[ix(m.entry)].pose {
                    walk(pose, w, has_track, parts);
                }
                return;
            };
            for (st, state) in m.states.iter().enumerate() {
                let Some(pose) = state.pose else { continue };
                let sw = state_weight(p, id, i32::try_from(st).expect("a machine state index"));
                if sw > 0.0 {
                    walk(pose, w * sw, has_track, parts);
                }
            }
        }
        SlotGraphNode::Other { node, kind, poses } => {
            if gi.layered(node, kind) {
                walk(poses[0], w, has_track, parts);
                return;
            }
            if gi.source(node, kind) {
                parts.push((SlotPart::Loco, w));
                return;
            }
            panic!("{}: {node} ({kind}) on the slot graph is not ported", gi.who());
        }
    }
}
