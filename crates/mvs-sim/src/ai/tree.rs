//! A bot's tree instance: the nodes the runner made, with
//! what never changes in a shape the bot's clones share, and the runner's lists, its pending request and its search
//! state. The search itself is in `runner.rs`.

use crate::Real;
use crate::ai::blackboard::{BbKey, Blackboard};
use crate::ai::classes::{DecSt, SvcSt, TaskSt};
use crate::ai::cx::Res;
use crate::ai::props;
use crate::fighter::common::latent_frames;
use crate::names::{Name, code_str};
use indexmap::IndexMap;
use mvs_data::BotTrees;
use mvs_data::tables::BtNodeDef;
use mvs_data::value::{Props, Value};
use std::cmp::Ordering;
use std::sync::Arc;

/// rand() & 0x7fff scale of the game's composites (DAT_143b527f0 = 1 / 32767 as a float).
#[allow(clippy::cast_possible_truncation, reason = "the float32 constant of the exe")]
pub(super) const RAND_SCALE: Real = (1.0 / 32767.0_f64) as f32 as Real;
/// BTSpecialChild::NotInitialized.
pub(super) const NOT_INIT: i32 = -1;
/// BTSpecialChild::ReturnToParent.
pub(super) const RETURN: i32 = -2;

/// EBTFlowAbortMode as bits: LowerPriority 1, Self 2, Both 3.
pub(super) const LOWER_PRIORITY: u8 = 1;
pub(super) const SELF: u8 = 2;

/// The node classes the runner itself runs, and the Blueprint task (`Task`) it hands to [`TaskSt`].
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub(super) enum NodeKind {
    #[default]
    Task,
    Selector,
    Sequence,
    RandomSelector,
    Chooser,
    SimpleParallel,
    /// A composite class with no handler here: it returns at once.
    Composite,
    Wait,
    FinishWithResult,
    SetTagCooldown,
    MoveDirectlyToward,
    RunBehavior,
    RunBehaviorDynamic,
}

impl NodeKind {
    pub(super) fn of(ty: &str) -> NodeKind {
        match ty {
            "BTComposite_Selector" => NodeKind::Selector,
            "BTComposite_Sequence" => NodeKind::Sequence,
            "BTComposite_RandomSelector" => NodeKind::RandomSelector,
            "BTComposite_Chooser" => NodeKind::Chooser,
            "BTComposite_SimpleParallel" => NodeKind::SimpleParallel,
            _ if ty.starts_with("BTComposite_") => NodeKind::Composite,
            "BTTask_Wait" => NodeKind::Wait,
            "BTTask_FinishWithResult" => NodeKind::FinishWithResult,
            "BTTask_SetTagCooldown" => NodeKind::SetTagCooldown,
            "BTTask_MoveDirectlyToward" => NodeKind::MoveDirectlyToward,
            "BTTask_RunBehavior" => NodeKind::RunBehavior,
            "BTTask_RunBehaviorDynamic" => NodeKind::RunBehaviorDynamic,
            _ => NodeKind::Task,
        }
    }
}

/// `DEC_KIND`: the engine's decorators the runner evaluates itself; the rest are Blueprint classes ([`DecSt`]).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub(super) enum DecKind {
    #[default]
    Blueprint,
    Blackboard,
    Cooldown,
    TagCooldown,
    SetTagCooldown,
    ForceSuccess,
}

impl DecKind {
    pub(super) fn of(ty: &str) -> DecKind {
        match ty {
            "BTDecorator_Blackboard" => DecKind::Blackboard,
            "BTDecorator_Cooldown" => DecKind::Cooldown,
            "BTDecorator_TagCooldown" => DecKind::TagCooldown,
            "BTDecorator_SetTagCooldown" => DecKind::SetTagCooldown,
            "BTDecorator_ForceSuccess" => DecKind::ForceSuccess,
            _ => DecKind::Blueprint,
        }
    }
}

/// A link's decorator logic, in prefix order (`ops`: And n / Or n / Not 1 / Test index).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(super) enum Op {
    And(usize),
    /// `Or`, and any other name: the loop treats every operation but And as an Or.
    Or(usize),
    Not,
    Test(usize),
}

/// `RLink`: a composite's child and the decorators on its link.
#[derive(Debug)]
pub(super) struct Link {
    pub(super) node: usize,
    pub(super) decs: Vec<usize>,
    pub(super) ops: Option<Vec<Op>>,
}

/// What a node instance never changes (`RNode`'s readonly fields): shared by the clones of the bot.
#[derive(Debug, Default)]
pub(super) struct NodeShape {
    /// The node's export name (`def.id`).
    pub(super) id: Box<str>,
    pub(super) ty: Box<str>,
    pub(super) kind: NodeKind,
    pub(super) composite: bool,
    pub(super) props: Props,
    /// The execution order: the child-index path from the root (a subtree root hangs under its RunBehavior task as
    /// child 0).
    pub(super) path: Vec<Real>,
    pub(super) parent: Option<usize>,
    pub(super) idx: usize,
    pub(super) links: Vec<Link>,
    pub(super) services: Vec<usize>,
}

/// `RNode`: one node of one tree instance. Its memory lives as long as the bot (UE keeps instance memory by tree
/// path).
#[derive(Clone, Debug)]
pub(super) struct RNode {
    pub(super) s: Arc<NodeShape>,
    /// Composite CurrentChild.
    pub(super) cur: i32,
    /// Composite OverrideChild (only the Chooser's memory overlap sets it).
    pub(super) over: i32,
    /// RandomSelector node memory: children not yet tried (kept across activations).
    pub(super) mask: i32,
    /// SimpleParallel memory.
    pub(super) main_active: bool,
    pub(super) main_result: Res,
    pub(super) force_bg: bool,
    pub(super) last_search: i64,
    /// RunBehavior: the root of the subtree instance.
    pub(super) sub: Option<usize>,
    /// A Blueprint task's state (`ctx`: made at its first execution).
    pub(super) task: Option<TaskSt>,
}

#[derive(Debug, Default)]
pub(super) struct DecShape {
    /// The decorator's export name (`def.id`).
    pub(super) id: Box<str>,
    pub(super) ty: Box<str>,
    pub(super) props: Props,
    pub(super) kind: DecKind,
    pub(super) inverse: bool,
    pub(super) abort: u8,
    /// A Blueprint class the classes list has (`impl`).
    pub(super) has_impl: bool,
    /// Place in the request order: the decorators of a link lie between the previous child's branch and their node,
    /// in their own order. `link_key`: the link's first node (its first decorator).
    pub(super) key: Vec<Real>,
    pub(super) link_key: Vec<Real>,
    /// The composite of the link, and the link's node (with its path).
    pub(super) parent: usize,
    pub(super) node: usize,
    pub(super) path: Vec<Real>,
    pub(super) idx: usize,
    /// The blackboard keys its key selector properties name (a Blueprint decorator observes them).
    pub(super) observes: Vec<BbKey>,
    /// BTDecorator_Blackboard's BlackboardKey.
    pub(super) bb_key: Option<BbKey>,
    /// BTDecorator_Blackboard NotifyObserver ValueChange.
    pub(super) value_change: bool,
}

/// `RDec`.
#[derive(Clone, Debug)]
pub(super) struct RDec {
    pub(super) s: Arc<DecShape>,
    pub(super) st: Option<DecSt>,
    /// BTDecorator_Cooldown LastUseTimestamp.
    pub(super) last_use: Real,
}

#[derive(Debug, Default)]
pub(super) struct SvcShape {
    /// The service's export name (`def.id`).
    pub(super) id: Box<str>,
    pub(super) ty: Box<str>,
    pub(super) props: Props,
    pub(super) has_impl: bool,
    pub(super) owner: usize,
    /// The owner's path.
    pub(super) path: Vec<Real>,
    pub(super) idx: usize,
}

/// `RSvc`.
#[derive(Clone, Debug)]
pub(super) struct RSvc {
    pub(super) s: Arc<SvcShape>,
    pub(super) st: Option<SvcSt>,
    /// NextTickRemainingTime, in ticks.
    pub(super) remain: Real,
    /// AccumulatedDeltaTime.
    pub(super) acc: crate::Real,
}

/// An aux node: an observer decorator or a service.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(super) enum Aux {
    Dec(usize),
    Svc(usize),
}

/// `Run`: a task in progress.
#[derive(Clone, Copy, Debug)]
pub(super) struct Run {
    pub(super) rn: usize,
    /// A Blueprint task (`def`).
    pub(super) def: bool,
    /// Its graph waits in a Delay or may go on.
    pub(super) graph: bool,
    /// The pending Delay, in ticks.
    pub(super) delay: Option<Real>,
    /// BTTask_Wait RemainingWaitTime, in ticks.
    pub(super) wait: Option<Real>,
    /// The Delay node of the pending Delay.
    pub(super) dn: u8,
}

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub(super) enum Rq {
    /// (Re)start the tree from the root.
    Start,
    /// The first search of a pushed subtree.
    Sub,
    /// The active task finished.
    Cont,
    /// Self abort: the decorator's branch fails.
    Fail,
    /// Switch to the decorator's (higher priority) branch.
    Restart,
    /// A SimpleParallel main task finished.
    Par,
}

#[derive(Clone, Debug)]
pub(super) struct Req {
    pub(super) t: Rq,
    /// The execution order of the requester; the earliest wins.
    pub(super) key: Vec<Real>,
    pub(super) res: Res,
    pub(super) d: Option<usize>,
    pub(super) sp: Option<usize>,
    /// Restart: ExecuteNode, the common parent of the active node and the decorator's parent.
    pub(super) top: Option<usize>,
    /// SearchEnd: the found task has to lie before the end of this branch.
    pub(super) end: Option<Vec<Real>>,
    /// Which request object this is (a caller's held request is compared with the pending one).
    pub(super) serial: u32,
}

impl Req {
    pub(super) fn new(t: Rq, key: Vec<Real>, res: Res) -> Req {
        Req { t, key, res, d: None, sp: None, top: None, end: None, serial: 0 }
    }
}

/// A field a search changed, with its old value (`mut`'s undo list).
#[derive(Clone, Copy, Debug)]
pub(super) enum Undo {
    Cur(usize, i32),
    Over(usize, i32),
    Mask(usize, i32),
    MainResult(usize, Res),
    ForceBg(usize, bool),
    LastSearch(usize, i64),
    LastUse(usize, Real),
    Remain(usize, Real),
}

/// The tree runner's state: the node instances, the active composites,
/// the running task, the parallels, the aux nodes, the pending request, the search state and CooldownTagsMap.
#[derive(Clone, Debug)]
pub(crate) struct Tree {
    pub(super) nodes: Vec<RNode>,
    pub(super) decs: Vec<RDec>,
    pub(super) svcs: Vec<RSvc>,
    pub(super) root: usize,
    /// The active composites; a pushed RunBehavior task sits above its subtree root.
    pub(super) path: Vec<usize>,
    /// The task whose link is activated under the top composite.
    pub(super) leaf: Option<usize>,
    /// The active (or last) task.
    pub(super) act: Option<usize>,
    pub(super) run: Option<Run>,
    /// SimpleParallel main tasks.
    pub(super) par: Vec<Run>,
    /// The active services and observer decorators, in execution order.
    pub(super) aux: Vec<Aux>,
    pub(super) req: Option<Req>,
    pub(super) serial: u32,
    pub(super) searching: bool,
    pub(super) restarting: bool,
    /// ExecutionRequest.bTryNextChild: the request's result is not Aborted (exe 14321bdd0).
    pub(super) try_next: bool,
    pub(super) search_id: i64,
    pub(super) search_start: Option<Vec<Real>>,
    pub(super) postpone: bool,
    pub(super) sres: Res,
    pub(super) pend: Vec<(Aux, bool)>,
    pub(super) pend_par: Vec<usize>,
    pub(super) undo: Vec<Undo>,
    /// While a search is applied: the composites that were active before it (the instance's ActiveNode is still the
    /// old task), and the branch it left (SearchData.DeactivatedBranchStart / End) with its request filter.
    pub(super) base: Option<Vec<usize>>,
    pub(super) dead: Option<Vec<Real>>,
    pub(super) filter: bool,
    /// PendingUnregisterAuxNodesRequests: branches (composite paths) whose aux nodes go at the start of the next
    /// tick.
    pub(super) unreg: Vec<Vec<Real>>,
    /// CooldownTagsMap: lower-case tag -> end time.
    pub(super) tag_cooldowns: IndexMap<Box<str>, Real>,
    /// The runner's events (`trace`: "exec <id>", "abort <id>", "tree finished") while a test reads them.
    pub(crate) log: Option<Vec<String>>,
}

impl Default for Tree {
    fn default() -> Self {
        Tree {
            nodes: Vec::new(),
            decs: Vec::new(),
            svcs: Vec::new(),
            root: 0,
            path: Vec::new(),
            leaf: None,
            act: None,
            run: None,
            par: Vec::new(),
            aux: Vec::new(),
            req: None,
            serial: 0,
            searching: false,
            restarting: false,
            try_next: true,
            search_id: 0,
            search_start: None,
            postpone: false,
            sres: Res::Failed,
            pend: Vec::new(),
            pend_par: Vec::new(),
            undo: Vec::new(),
            base: None,
            dead: None,
            filter: false,
            unreg: Vec::new(),
            tag_cooldowns: IndexMap::new(),
            log: None,
        }
    }
}

/// Execution order of two paths: a prefix comes first.
pub(super) fn cmp(a: &[Real], b: &[Real]) -> Ordering {
    for (x, y) in a.iter().zip(b) {
        if x != y {
            return if x < y { Ordering::Less } else { Ordering::Greater };
        }
    }
    a.len().cmp(&b.len())
}

pub(super) fn is_prefix(a: &[Real], b: &[Real]) -> bool {
    a.len() <= b.len() && a.iter().zip(b).all(|(x, y)| x == y)
}

/// A task at path `t` lies past the end of the branch `e`.
pub(super) fn after_branch(t: &[Real], e: &[Real]) -> bool {
    !is_prefix(e, t) && cmp(t, e) == Ordering::Greater
}

/// The branch `a` ends later than the branch `b`.
pub(super) fn ends_later(a: &[Real], b: &[Real]) -> bool {
    if a.len() < b.len() && is_prefix(a, b) {
        true
    } else if is_prefix(b, a) {
        false
    } else {
        cmp(a, b) == Ordering::Greater
    }
}

pub(super) fn path_of(p: &[Real], tail: Real) -> Vec<Real> {
    let mut v = Vec::with_capacity(p.len() + 1);
    v.extend_from_slice(p);
    v.push(tail);
    v
}

/// `String(v)` of a property, for a tag or a name looked up by its text.
pub(super) fn js_string(v: Option<&Value>) -> String {
    match v {
        None => "undefined".into(),
        Some(Value::Null) => "null".into(),
        Some(Value::Str(s)) => s.clone(),
        Some(Value::Bool(b)) => b.to_string(),
        Some(Value::Num(n)) => n.to_string(),
        Some(_) => "[object Object]".into(),
    }
}

/// A key selector property's key name (`p[k]?.key`).
pub(super) fn selector<'a>(p: &'a Props, k: &str) -> Option<&'a str> {
    props::get(p, k)?.as_obj()?.get("key")?.as_str()
}

/// `ticksFor` / `latentFrames`: the length in ticks of a native tree timer (a service's NextTickRemainingTime,
/// WrappedTickNode 143222920, and BTTask_Wait's RemainingWaitTime, TickTask 14324a0e0, are float32 countdowns by the
/// tick's DeltaSeconds that fire once <= 0: the same count as a latent Delay).
pub(super) fn ticks_for(s: Real) -> Real {
    latent_frames(s)
}

impl Tree {
    /// `reset`'s runner part: a new instance of `tree`'s root, every list and the tag cooldowns emptied.
    pub(super) fn reset(&mut self, data: &BotTrees, bb: &Blackboard, tree: Option<Name>) {
        let log = self.log.take();
        *self = Tree { log, ..Tree::default() };
        if let Some(tree) = tree {
            let name = code_str(tree).expect("a bot tree named by a name! literal");
            let t = data.trees.get(name).unwrap_or_else(|| panic!("bt: no tree {name}"));
            self.root = self.build(data, bb, &t.root, None, 0, Vec::new());
        }
    }

    pub(super) fn path_of(&self, a: Aux) -> &[Real] {
        match a {
            Aux::Dec(d) => &self.decs[d].s.path,
            Aux::Svc(s) => &self.svcs[s].s.path,
        }
    }

    /// `RNode`'s constructor: the node, its children with their links' decorators, then its services.
    pub(super) fn build(
        &mut self,
        data: &BotTrees,
        bb: &Blackboard,
        def: &BtNodeDef,
        parent: Option<usize>,
        idx: usize,
        path: Vec<Real>,
    ) -> usize {
        let me = self.nodes.len();
        self.nodes.push(RNode {
            s: Arc::default(),
            cur: NOT_INIT,
            over: NOT_INIT,
            mask: 0,
            main_active: false,
            main_result: Res::Succeeded,
            force_bg: false,
            last_search: -1,
            sub: None,
            task: None,
        });
        let mut links = Vec::new();
        for (i, c) in def.children.iter().flatten().enumerate() {
            let node = self.build(data, bb, &c.node, Some(me), i, path_of(&path, idx_real(i)));
            let all = c.decorators.as_deref().unwrap_or(&[]);
            let count = idx_real(all.len());
            let mut decs = Vec::with_capacity(all.len());
            for (k, d) in all.iter().enumerate() {
                let props = props::instance_props(data.defaults.get(&d.ty), Some(&d.props));
                let ni = idx_real(i);
                let observes =
                    props.values().filter_map(|v| v.as_obj()?.get("key")?.as_str()).filter_map(|n| bb.key(n)).collect();
                let shape = DecShape {
                    id: d.id.as_str().into(),
                    ty: d.ty.as_str().into(),
                    kind: DecKind::of(&d.ty),
                    inverse: props::is_true(&props, "bInverseCondition"),
                    abort: match props::str(&props, "FlowAbortMode") {
                        Some("EBTFlowAbortMode::LowerPriority") => LOWER_PRIORITY,
                        Some("EBTFlowAbortMode::Self") => SELF,
                        Some("EBTFlowAbortMode::Both") => LOWER_PRIORITY | SELF,
                        _ => 0,
                    },
                    has_impl: DecSt::of(&d.ty).is_some(),
                    key: path_of(&path, ni - 1.0 + (idx_real(k) + 1.0) / (count + 1.0)),
                    link_key: path_of(&path, ni - 1.0 + 1.0 / (count + 1.0)),
                    parent: me,
                    node,
                    path: self.nodes[node].s.path.clone(),
                    idx: k,
                    observes,
                    bb_key: selector(&props, "BlackboardKey").and_then(|n| bb.key(n)),
                    value_change: props::str(&props, "NotifyObserver") == Some("EBTBlackboardRestart::ValueChange"),
                    props,
                };
                decs.push(self.decs.len());
                self.decs.push(RDec { st: DecSt::of(&shape.ty), s: Arc::new(shape), last_use: Real::NEG_INFINITY });
            }
            let ops = c.ops.as_ref().map(|ops| {
                ops.iter()
                    .map(|(op, n)| {
                        let n = usize::try_from(crate::math::dbl::to_int32(*n)).expect("a decorator operation's count");
                        match op.as_str() {
                            "Test" => Op::Test(n),
                            "Not" => Op::Not,
                            "And" => Op::And(n),
                            _ => Op::Or(n),
                        }
                    })
                    .collect()
            });
            links.push(Link { node, decs, ops });
        }
        let mut services = Vec::new();
        for (i, s) in def.services.iter().flatten().enumerate() {
            let props = props::instance_props(data.defaults.get(&s.ty), Some(&s.props));
            let shape = SvcShape {
                id: s.id.as_str().into(),
                ty: s.ty.as_str().into(),
                props,
                has_impl: SvcSt::of(&s.ty).is_some(),
                owner: me,
                path: path.clone(),
                idx: i,
            };
            services.push(self.svcs.len());
            self.svcs.push(RSvc { st: SvcSt::of(&shape.ty), s: Arc::new(shape), remain: 0.0, acc: 0.0 });
        }
        self.nodes[me].s = Arc::new(NodeShape {
            id: def.id.as_str().into(),
            ty: def.ty.as_str().into(),
            kind: NodeKind::of(&def.ty),
            composite: def.ty.starts_with("BTComposite_"),
            props: props::instance_props(data.defaults.get(&def.ty), Some(&def.props)),
            path,
            parent,
            idx,
            links,
            services,
        });
        me
    }
}

/// A child index as the paths hold it.
pub(super) fn idx_real(i: usize) -> Real {
    crate::real(u32::try_from(i).expect("a tree of fewer than 2^32 nodes"))
}

impl Tree {
    /// `runs`: the parallel main tasks, then the active task (by their node).
    pub(super) fn runs(&self) -> Vec<usize> {
        self.par.iter().chain(self.run.iter()).map(|r| r.rn).collect()
    }

    pub(super) fn run_mut(&mut self, rn: usize) -> Option<&mut Run> {
        self.par.iter_mut().chain(self.run.iter_mut()).find(|r| r.rn == rn)
    }

    /// A node at path `p` lies inside a branch whose aux nodes wait to be unregistered (exe 14321be5b: the range runs
    /// from the branch root's execution index to its last one, so the root's services are inside and the decorators
    /// of its own link are not). `own`: `p` is the node itself (a task, a service's composite), not a decorator's
    /// node.
    pub(super) fn in_unreg(&self, p: &[Real], own: bool) -> bool {
        self.unreg.iter().any(|pre| is_prefix(pre, p) && (own || p.len() > pre.len()))
    }

    /// IsExecutingBranch: the active node lies under the decorator's child.
    pub(super) fn executing(&self, d: usize) -> bool {
        self.act.is_some_and(|a| is_prefix(&self.decs[d].s.path, &self.nodes[a].s.path))
    }

    pub(super) fn top(&self) -> Option<usize> {
        self.path.last().copied()
    }

    pub(super) fn index_in(cp: &[usize], n: usize) -> i64 {
        cp.iter().position(|&x| x == n).map_or(-1, |k| i64::try_from(k).expect("a short path"))
    }

    /// Decorators of a link come before the node, its services after it.
    pub(super) fn sort_aux(&mut self) {
        let Tree { aux, decs, svcs, .. } = self;
        let key = |a: &Aux| match *a {
            Aux::Dec(d) => (&decs[d].s.path, 0, decs[d].s.idx),
            Aux::Svc(s) => (&svcs[s].s.path, 1, svcs[s].s.idx),
        };
        aux.sort_by(|a, b| {
            let (pa, ka, ia) = key(a);
            let (pb, kb, ib) = key(b);
            cmp(pa, pb).then(ka.cmp(&kb)).then(ia.cmp(&ib))
        });
    }

    // --- `mut`: a field a search changes, undone by a rollback ---

    pub(super) fn set_cur(&mut self, n: usize, v: i32) {
        if self.searching {
            self.undo.push(Undo::Cur(n, self.nodes[n].cur));
        }
        self.nodes[n].cur = v;
    }

    pub(super) fn set_over(&mut self, n: usize, v: i32) {
        if self.searching {
            self.undo.push(Undo::Over(n, self.nodes[n].over));
        }
        self.nodes[n].over = v;
    }

    pub(super) fn set_mask(&mut self, n: usize, v: i32) {
        if self.searching {
            self.undo.push(Undo::Mask(n, self.nodes[n].mask));
        }
        self.nodes[n].mask = v;
    }

    pub(super) fn set_main_result(&mut self, n: usize, v: Res) {
        if self.searching {
            self.undo.push(Undo::MainResult(n, self.nodes[n].main_result));
        }
        self.nodes[n].main_result = v;
    }

    pub(super) fn set_force_bg(&mut self, n: usize, v: bool) {
        if self.searching {
            self.undo.push(Undo::ForceBg(n, self.nodes[n].force_bg));
        }
        self.nodes[n].force_bg = v;
    }

    pub(super) fn set_last_search(&mut self, n: usize, v: i64) {
        if self.searching {
            self.undo.push(Undo::LastSearch(n, self.nodes[n].last_search));
        }
        self.nodes[n].last_search = v;
    }

    pub(super) fn set_last_use(&mut self, d: usize, v: Real) {
        if self.searching {
            self.undo.push(Undo::LastUse(d, self.decs[d].last_use));
        }
        self.decs[d].last_use = v;
    }

    pub(super) fn set_remain(&mut self, s: usize, v: Real) {
        if self.searching {
            self.undo.push(Undo::Remain(s, self.svcs[s].remain));
        }
        self.svcs[s].remain = v;
    }

    /// FBehaviorTreeSearchData::AddUniqueUpdate: an add and a remove of the same node cancel out.
    pub(super) fn update(&mut self, a: Aux, add: bool) {
        if !self.searching {
            return;
        }
        if let Some(i) = self.pend.iter().position(|u| u.0 == a) {
            if self.pend[i].1 != add {
                self.pend.remove(i);
            }
            return;
        }
        if !add && !self.aux.contains(&a) {
            return;
        }
        self.pend.push((a, add));
    }

    /// A tag's map key: `String(p.CooldownTag).toLowerCase()`.
    pub(super) fn tag_of(p: &Props) -> Box<str> {
        js_string(p.get("CooldownTag")).to_lowercase().into()
    }

    pub(super) fn add_tag_cooldown(&mut self, p: &Props, now: Real) {
        let tag = Tree::tag_of(p);
        let dur = props::num(p, "CooldownDuration").unwrap_or(5.0);
        let cur = self.tag_cooldowns.get(&tag).copied();
        let end = match cur {
            Some(cur) if props::truthy(p, "bAddToExistingDuration") => cur + dur,
            _ => now + dur,
        };
        self.tag_cooldowns.insert(tag, end);
    }
}
