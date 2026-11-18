//! What a Blueprint node of a bot tree sees and how it runs: the
//! context of one call, the result of a task's graph up to its next Delay, and the three traits a node class
//! implements (BTTask_BlueprintBase, BTDecorator_BlueprintBase, BTService_BlueprintBase).

use crate::Real;
use crate::actors::ActorRef;
use crate::ai::blackboard::{BbKey, BbVal};
use crate::ai::bt::BtBot;
use crate::ai::props;
use crate::fighter::FighterId;
use crate::world::World;
use mvs_data::value::Props;

/// EBTNodeResult.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Res {
    Succeeded = 0,
    Failed = 1,
    Aborted = 2,
    InProgress = 3,
}

/// `NodeCtx`: one call of a node class. The node's own variables (kept for the life of the node
/// instance: UE keeps a Blueprint node's instance for the tree's life) are the fields of the class's struct, the
/// `&mut self` of the call; its instance properties (class defaults under the node's overrides) are `props`.
pub struct Cx<'a> {
    pub w: &'a mut World,
    /// The controlled pawn (`c.me`).
    pub me: FighterId,
    /// The running bot: its blackboard, pad, clock, random stream and difficulty. A node writes the blackboard
    /// through [`Cx::set`], [`Cx::set_obj`] and [`Cx::clear`], which tell the tree of the change (its observers);
    /// `bot.bb.put` would not.
    pub bot: &'a mut BtBot,
    pub props: &'a Props,
    /// `c.stale`: this execution starts in the call that aborted the node's last one while it waited in a Delay.
    /// AbortLatentActions (RemoveActionsForObject 142b49230) only queues the old actions for the next latent pass,
    /// and Delay (142b17040) starts nothing while FindExistingAction 1407a7210 still finds one with its UUID (one per
    /// Delay node). Read by a task with `OWN_STALE`; for the others the runtime drops the Delay itself.
    pub stale: bool,
}

impl Cx<'_> {
    /// `c.key(prop)`: the blackboard key a key selector property names (`None` for an unset selector, 'None', or a
    /// name the blackboard does not hold).
    pub fn key(&self, prop: &str) -> Option<BbKey> {
        self.bot.bb.key(props::key(self.props, prop))
    }

    /// `c.bb.set(key, v)`: the value, and the tree's observers hear of a change (`Blackboard.onChange`, BtBot
    /// bbChanged).
    pub fn set(&mut self, k: Option<BbKey>, v: BbVal) {
        if let Some(k) = k
            && self.bot.bb.put(k, v)
        {
            self.bot.bb_changed(self.w, self.me, k);
        }
    }

    /// `c.bb.setObj(key, v)`: SetValueAsObject: UBlackboardComponent::SetValue<UBlackboardKeyType_Object> writes
    /// only when the key's type is Object, so a selector that points at a key of another type (a tree's
    /// 'UFO = TargetPoint') leaves it alone.
    /// Unverified: the type test is UE 4.26 source, not re-read in the exe.
    pub fn set_obj(&mut self, k: Option<BbKey>, v: Option<ActorRef>) {
        if self.bot.bb.ty(k) == Some(crate::ai::blackboard::BbType::Object) {
            self.set(k, v.map_or(BbVal::Null, BbVal::Obj));
        }
    }

    /// `c.bb.clear(key)`: the key's initial value.
    pub fn clear(&mut self, k: Option<BbKey>) {
        if let Some(t) = self.bot.bb.ty(k) {
            self.set(k, BbVal::initial(t));
        }
    }
}

/// What a task's graph does when it stops: a Blueprint Delay it waits in (`yield seconds`, `yield c.d(node,
/// seconds)`), FinishExecute (`return true / false`), or the end of a path that calls neither, which leaves the task
/// running until it is aborted or its tick finishes it (`return` with no value, or a `run` that returns nothing).
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Step {
    /// A Delay of `secs`; `node` is the K2Node_Delay (`c.d(node, ...)`, 0 for the graph's first one): the same
    /// number is the same LatentInfo.UUID.
    Delay { secs: Real, node: u8 },
    /// FinishExecute(success).
    Finish(bool),
    /// No FinishExecute: the task stays InProgress.
    Hang,
}

/// Why a task's `run` is called: a new execution (ReceiveExecuteAI: the graph starts from its first node, the
/// struct's program counter back to its start; its variables keep what the last execution left), or the graph going
/// on past the Delay it last returned.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Resume {
    Execute,
    Delay,
}

/// A BTTask_BlueprintBase class (`TaskDef`). Its struct holds the graph's program counter and its variables.
pub trait Task: Clone + std::fmt::Debug + Default {
    /// `ownStale`: the task's graph reads [`Cx::stale`] itself; for the others the runtime does not start the first
    /// Delay of a stale execution when it is the node the old execution waited in.
    const OWN_STALE: bool = false;
    /// ReceiveExecuteAI and the graph past each Delay.
    fn run(&mut self, cx: &mut Cx, r: Resume) -> Step;
    /// ReceiveTickAI: `Some(result)` finishes the task.
    fn tick(&mut self, _cx: &mut Cx, _dt: Real) -> Option<bool> {
        None
    }
    /// ReceiveAbortAI (each calls FinishAbort). AbortTask (exe 14322bc40) drops the pending Delays first
    /// (AbortLatentActions): the runtime calls `run` no more for this execution.
    fn abort(&mut self, _cx: &mut Cx) {}
}

/// A BTDecorator_BlueprintBase class (`DecDef`). Every class of the build's trees has a condition check.
pub trait Dec: Clone + std::fmt::Debug + Default {
    /// PerformConditionCheckAI, before bInverseCondition (the runtime applies it and the abort mode).
    fn test(&mut self, cx: &mut Cx) -> bool;
    /// ReceiveExecutionStartAI.
    fn start(&mut self, _cx: &mut Cx) {}
    /// ReceiveExecutionFinishAI.
    fn finish(&mut self, _cx: &mut Cx, _res: Res) {}
    /// ReceiveTickAI.
    fn tick(&mut self, _cx: &mut Cx, _dt: Real) {}
}

/// A BTService_BlueprintBase class (`SvcDef`).
pub trait Svc: Clone + std::fmt::Debug + Default {
    /// ReceiveTickAI, every Interval +- RandomDeviation; `dt` is the time since its last tick.
    fn tick(&mut self, _cx: &mut Cx, _dt: Real) {}
    /// ReceiveActivationAI.
    fn activate(&mut self, _cx: &mut Cx) {}
    /// ReceiveDeactivationAI.
    fn deactivate(&mut self, _cx: &mut Cx) {}
    /// ReceiveSearchStartAI.
    fn search_start(&mut self, _cx: &mut Cx) {}
}
