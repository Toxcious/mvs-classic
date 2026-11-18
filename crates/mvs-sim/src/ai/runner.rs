//! The bots' tree runner (the think, the search and the task runs): it runs a [`Tree`] the way
//! UBehaviorTreeComponent does.
//! The engine side follows the exe's UBehaviorTreeComponent, UBTCompositeNode and UBTComposite_SimpleParallel:
//! - TickComponent 14321fbc0, the only caller of ProcessExecutionRequest 143219350: one search per component tick,
//!   after the aux nodes (services, observer decorators) and before the parallel tasks, the active (or aborting) task
//!   and the aborting task of a dropped subtree; the component sleeps until its next needed time (ticking tasks and
//!   decorators need 0, so it ticks every frame here); latent Delays of Blueprint tasks are counted by the world's
//!   leftover latent pass at the end of the same frame (here: the end of think, `latent_pass`);
//! - Blueprint decorators: per-tick re-evaluation and key-selector observers (143240d00);
//! - requests (RequestExecution 14321bdd0): the earliest requester wins (a tie goes to the new one), search ranges
//!   merge, requests from the branch being left are dropped while the old task is aborted and the aux changes are
//!   applied. A requester's place is its own execution index (a decorator's lies before its node, in the order of
//!   the link's decorators); a restart's is the first node of its link (GetChildExecutionIndex FirstNode: the
//!   link's first decorator), whichever decorator of the link asks;
//! - observers: Self removed when the branch is left with a result, Both / LowerPriority kept until a task with a
//!   lower execution index runs or a failing request starts before them; a higher-priority restart only counts when
//!   it finds a task inside the requesting branch;
//! - ApplySearchData 143209260 ticks every aux node the search added, also decorators, while the instance's active
//!   node is still the old task: a Blueprint decorator with Both / LowerPriority whose branch was entered from
//!   outside asks for a restart of that branch right there (`process`, `tick_dec`);
//! - services: NotifyParentActivation 143240820, first tick one interval after activation, the remaining time kept
//!   across activations; the root's services are never removed while the tree loops (ApplySearchUpdates 143209460);
//! - a decorator scope (SimpleParallel; OnNodeDeactivation 1432174f0 -> UnregisterAuxNodesInRange 143220cd0) drops
//!   its aux nodes with the search's other updates;
//! - UBTDecorator::ConditionalFlowAbort 14320b970 (BTDecorator_Blackboard's key observer 1432414e0): a change that
//!   leaves a branch that is not executing closed queues the range of the composite under the decorator
//!   (PendingUnregisterAuxNodesRequests, component +0x1b0). The next component tick starts by removing every aux
//!   node inside the queued ranges (14321fca0), and until then RequestExecution drops a requester inside one;
//! - timers (BTTask_Wait, Delay, service intervals) count whole 60 Hz ticks: max(1, ceil(s * 60)).
//!
//! BTComposite_Chooser and BTComposite_RandomSelector are game classes, decoded from the exe (`next_child`).

use crate::Real;
use crate::actors::ActorRef;
use crate::ai::blackboard::{BbKey, BbType, BbVal};
use crate::ai::bt::BtBot;
use crate::ai::classes::{DecSt, SvcSt, TaskSt};
use crate::ai::cx::{Cx, Res, Resume, Step};
use crate::ai::props;
use crate::ai::tree::*;
use crate::consts::DT;
use crate::fighter::FighterId;
use crate::fighter::common::{latent_frames, warn_parts, warn_text};
use crate::math::{max, min};
use crate::world::World;
use mvs_data::value::{Props, Value};
use std::cmp::Ordering;

impl BtBot {
    /// `think`'s tree part: SelfActor, the start request, the pending unregisters, the aux nodes' ticks, the
    /// request's search, the tasks' ticks and the latent pass.
    pub(super) fn run_tree(&mut self, w: &mut World, me: FighterId) {
        let self_actor = self.bb.key("SelfActor");
        self.set_bb(w, me, self_actor, BbVal::Obj(ActorRef::Fighter(me)));
        if self.t.path.is_empty() && self.t.req.is_none() {
            self.store_req(Req::new(Rq::Start, Vec::new(), Res::InProgress));
        }
        // ProcessPendingUnregister (TickComponent, exe 14321fca0): the aux nodes inside the queued branches go, the
        // branch root's own link decorators stay (UnregisterAuxNodesInRange 143220cd0: strictly inside the range).
        if !self.t.unreg.is_empty() {
            let mut k = self.t.aux.len();
            while k > 0 {
                k -= 1;
                let a = self.t.aux[k];
                if self.t.in_unreg(self.t.path_of(a), matches!(a, Aux::Svc(_))) {
                    self.remove_aux(w, me, k);
                }
            }
            self.t.unreg.clear();
        }
        // Aux nodes, in execution order.
        let mut k = 0;
        while k < self.t.aux.len() {
            match self.t.aux[k] {
                Aux::Svc(s) => self.tick_service(w, me, s),
                Aux::Dec(d) => self.tick_dec(w, me, d),
            }
            k += 1;
        }
        if self.t.req.is_some() {
            self.process(w, me);
        }
        // Parallel tasks, then the active task.
        for rn in self.t.runs() {
            let Some(r) = self.t.run_mut(rn) else { continue };
            if let Some(wait) = r.wait.as_mut() {
                *wait -= 1.0;
                if *wait <= 0.0 {
                    self.finish_run(w, me, rn, Res::Succeeded);
                }
            } else if r.def {
                // ReceiveTickAI: a class without one gives None.
                let v = self.with_task(w, me, rn, false, |t, cx| t.tick(cx, DT));
                if let Some(v) = v {
                    self.finish_run(w, me, rn, if v { Res::Succeeded } else { Res::Failed });
                }
            }
        }
        self.latent_pass(w, me);
    }

    /// `bb.set(key, v)` with the observers told (`Blackboard.onChange`): the runtime's own writes and a test's.
    pub(crate) fn set_bb(&mut self, w: &mut World, me: FighterId, k: Option<BbKey>, v: BbVal) {
        if let Some(k) = k
            && self.bb.put(k, v)
        {
            self.bb_changed(w, me, k);
        }
    }

    /// `bbChanged(k)`: the blackboard's observers of `k` (the decorators that abort on a change), told at once, in
    /// the write's call, as UBlackboardComponent's NotifyObservers. BTDecorator_Blackboard::OnBlackboardKeyValueChange
    /// -> ConditionalFlowAbort. Blueprint decorators observe the keys of their key selector properties while relevant
    /// (OnBecomeRelevant, exe 143240d00) and request at once (OnBlackboardKeyValueChange 143241590: GetShouldAbort
    /// 14323c340, then RequestExecution).
    pub(crate) fn bb_changed(&mut self, w: &mut World, me: FighterId, k: BbKey) {
        let mut i = 0;
        while i < self.t.aux.len() {
            let a = self.t.aux[i];
            i += 1;
            let Aux::Dec(d) = a else { continue };
            let s = self.t.decs[d].s.clone();
            if s.kind == DecKind::Blueprint && s.has_impl && s.abort != 0 {
                if !s.observes.contains(&k) {
                    continue;
                }
                // Every Blueprint decorator class has a condition check (PerformConditionCheckAI).
                let ex = self.t.executing(d);
                let keep = if ex {
                    s.abort & SELF == 0 || self.can_execute(w, me, d)
                } else {
                    s.abort & LOWER_PRIORITY == 0 || !self.can_execute(w, me, d)
                };
                if keep {
                    continue;
                }
                if ex {
                    if s.abort & SELF != 0 {
                        self.request_fail(w, me, d);
                    }
                } else if s.abort & LOWER_PRIORITY != 0 {
                    self.request_restart(w, me, d);
                }
                continue;
            }
            if s.kind != DecKind::Blackboard || s.abort == 0 || s.bb_key != Some(k) {
                continue;
            }
            let ex = self.t.executing(d);
            let pass = self.can_execute(w, me, d);
            if ex != pass {
                if ex {
                    if s.abort & SELF != 0 {
                        self.request_fail(w, me, d);
                    }
                } else if s.abort & LOWER_PRIORITY != 0 {
                    self.request_restart(w, me, d);
                }
            } else if ex {
                if s.value_change {
                    self.request_restart(w, me, d);
                }
            } else if self.t.nodes[s.node].s.composite {
                // RequestUnregisterAuxNodesInBranch(ChildComposite)
                self.t.unreg.push(s.path.clone());
            }
        }
    }

    /// A runner event for a test's log and the bot log (`ai::log`).
    fn log(&mut self, ev: impl FnOnce() -> String) {
        if self.t.log.is_none() && !crate::ai::log::on() {
            return;
        }
        let e = ev();
        crate::ai::log::line(|| e.clone());
        if let Some(l) = self.t.log.as_mut() {
            l.push(e);
        }
    }

    fn store_req(&mut self, mut r: Req) {
        self.t.serial = self.t.serial.wrapping_add(1);
        r.serial = self.t.serial;
        self.t.req = Some(r);
    }

    /// `noClass`: a node class nobody entered: the task fails, the decorator reads false, the service does nothing,
    /// and the class is named once (registry rule: no default without a warning).
    fn no_class(w: &World, ty: &str) {
        warn_parts(w, &["bot tree node class not entered: ", ty]);
    }

    // --- the node calls: the node's state is out of the arena for the call ---

    fn with_task<R>(
        &mut self,
        w: &mut World,
        me: FighterId,
        n: usize,
        stale: bool,
        f: impl FnOnce(&mut TaskSt, &mut Cx) -> R,
    ) -> R {
        let shape = self.t.nodes[n].s.clone();
        let mut st = self.t.nodes[n].task.take().expect("a Blueprint task's state");
        let r = f(&mut st, &mut Cx { w, me, bot: self, props: &shape.props, stale });
        self.t.nodes[n].task = Some(st);
        r
    }

    /// A decorator's call. A decorator's test can reach its own test again (it writes a key it observes: the
    /// change's observers run inside the write): that inner call runs on the class's starting state. The classes
    /// whose test writes the blackboard keep no variables, so it sees the shared state the class holds.
    fn with_dec<R>(&mut self, w: &mut World, me: FighterId, d: usize, f: impl FnOnce(&mut DecSt, &mut Cx) -> R) -> R {
        let shape = self.t.decs[d].s.clone();
        let taken = self.t.decs[d].st.take();
        let outer = taken.is_some();
        let mut st = taken.or_else(|| DecSt::of(&shape.ty)).expect("a ported decorator class");
        let r = f(&mut st, &mut Cx { w, me, bot: self, props: &shape.props, stale: false });
        if outer {
            self.t.decs[d].st = Some(st);
        }
        r
    }

    fn with_svc(&mut self, w: &mut World, me: FighterId, s: usize, f: impl FnOnce(&mut SvcSt, &mut Cx)) {
        let shape = self.t.svcs[s].s.clone();
        let Some(mut st) = self.t.svcs[s].st.take() else { return };
        f(&mut st, &mut Cx { w, me, bot: self, props: &shape.props, stale: false });
        self.t.svcs[s].st = Some(st);
    }

    // --- requests (UBehaviorTreeComponent::RequestExecution) ---

    /// A request that is not a restart: an earlier pending request stays (a tie goes to the new one) and either way
    /// the search range becomes unlimited.
    fn request(&mut self, w: &mut World, me: FighterId, mut r: Req) {
        if !self.t.unreg.is_empty() {
            let inside = match r.d {
                Some(d) => self.t.in_unreg(&self.t.decs[d].s.path, false),
                None if r.t == Rq::Cont => self.t.in_unreg(&r.key[..r.key.len() - 1], true),
                None if r.t == Rq::Par => self.t.in_unreg(&r.key, true),
                None => false,
            };
            if inside {
                return;
            }
        }
        if let Some(q) = self.t.req.as_mut()
            && cmp(&q.key, &r.key) == Ordering::Less
        {
            q.end = None;
            return;
        }
        if let Some(d) = r.d {
            let s = self.t.decs[d].s.clone();
            if self.t.filter && self.t.dead.as_ref().is_some_and(|dead| is_prefix(dead, &s.path)) {
                return;
            }
            // A decorator of a link with logic operations that still lets the link pass.
            let parent = self.t.nodes[s.parent].s.clone();
            let l = &parent.links[self.t.nodes[s.node].s.idx];
            if l.ops.is_some() && self.allows(w, me, l) {
                return;
            }
        }
        r.end = None;
        self.store_req(r);
    }

    fn request_fail(&mut self, w: &mut World, me: FighterId, d: usize) {
        let mut r = Req::new(Rq::Fail, self.t.decs[d].s.key.clone(), Res::Failed);
        r.d = Some(d);
        self.request(w, me, r);
    }

    /// UBTDecorator_BlueprintBase::TickNode: ReceiveTickAI, then EvaluateAbortType.
    fn tick_dec(&mut self, w: &mut World, me: FighterId, d: usize) {
        let s = self.t.decs[d].s.clone();
        if s.kind != DecKind::Blueprint || !s.has_impl {
            return;
        }
        self.with_dec(w, me, d, |st, cx| st.tick(cx, DT));
        if s.abort == 0 {
            return;
        }
        if self.t.executing(d) {
            if s.abort & SELF != 0 && !self.can_execute(w, me, d) {
                self.request_fail(w, me, d);
            }
        } else if s.abort & LOWER_PRIORITY != 0 && self.can_execute(w, me, d) {
            self.request_restart(w, me, d);
        }
    }

    fn request_restart(&mut self, w: &mut World, me: FighterId, d: usize) {
        if self.t.path.is_empty() {
            return;
        }
        let s = self.t.decs[d].s.clone();
        let (key, br) = (&s.link_key, &s.path);
        if !self.t.unreg.is_empty() && self.t.in_unreg(br, false) {
            return;
        }
        if let Some(q) = self.t.req.as_mut()
            && cmp(&q.key, key) == Ordering::Less
        {
            // The earlier request stays; its range grows to hold this branch.
            if q.end.as_ref().is_some_and(|e| ends_later(br, e)) {
                q.end = Some(br.clone());
            }
            return;
        }
        if self.t.filter && self.t.dead.as_ref().is_some_and(|dead| is_prefix(dead, br)) {
            return;
        }
        let parent = self.t.nodes[s.parent].s.clone();
        if !self.t.executing(d) && !self.allows(w, me, &parent.links[self.t.nodes[s.node].s.idx]) {
            return;
        }
        // Decorators between the common parent (with the active node, or with the pending request's node) and the
        // restart parent.
        let cp = self.t.base.clone().unwrap_or_else(|| self.t.path.clone());
        let had = self.t.req.clone();
        let mut lim = i64::try_from(cp.len()).expect("a short path") - 1;
        if let Some(had) = &had {
            let e = match had.t {
                Rq::Restart => had.top,
                Rq::Fail => had.d.map(|d| self.t.decs[d].s.parent),
                Rq::Par => had.sp,
                Rq::Start => Some(self.t.root),
                _ => None,
            };
            let k = e.map_or(-1, |e| Tree::index_in(&cp, e));
            if k >= 0 {
                lim = k;
            }
        }
        let mut top = s.parent;
        let mut k = Tree::index_in(&cp, top);
        while k < 0 || k > lim {
            let mut child = top;
            let mut p = self.t.nodes[top].s.parent;
            if let Some(pp) = p
                && !self.t.nodes[pp].s.composite
            {
                child = pp;
                p = self.t.nodes[pp].s.parent;
            }
            let Some(p) = p else { return };
            let ps = self.t.nodes[p].s.clone();
            if !self.allows(w, me, &ps.links[self.t.nodes[child].s.idx]) {
                return;
            }
            top = p;
            k = Tree::index_in(&cp, top);
        }
        // The search range is limited to the branch unless another request is already pending. `had` is the object
        // the search held: what a nested request wrote into it since counts.
        let end = match had {
            Some(had) => {
                let had_end = match &self.t.req {
                    Some(q) if q.serial == had.serial => q.end.clone(),
                    _ => had.end,
                };
                match had_end {
                    Some(e) if ends_later(br, &e) => Some(br.clone()),
                    e => e,
                }
            }
            None => Some(br.clone()),
        };
        let mut r = Req::new(Rq::Restart, key.clone(), Res::Aborted);
        r.d = Some(d);
        r.top = Some(top);
        r.end = end;
        self.store_req(r);
    }

    // --- decorators ---

    fn can_execute(&mut self, w: &mut World, me: FighterId, d: usize) -> bool {
        let s = self.t.decs[d].s.clone();
        let raw = match s.kind {
            DecKind::Blackboard => self.eval_blackboard(&s.props),
            DecKind::Cooldown => {
                (self.now - self.t.decs[d].last_use) >= props::num(&s.props, "CoolDownTime").unwrap_or(5.0)
            }
            DecKind::TagCooldown => {
                let e = self.t.tag_cooldowns.get(&Tree::tag_of(&s.props)).copied();
                e.is_none_or(|e| self.now >= e)
            }
            DecKind::Blueprint => {
                if s.has_impl {
                    self.with_dec(w, me, d, |st, cx| st.test(cx))
                } else {
                    BtBot::no_class(w, &s.ty);
                    false
                }
            }
            DecKind::SetTagCooldown | DecKind::ForceSuccess => true,
        };
        let pass = raw != s.inverse;
        crate::ai::log::line(|| format!("dec {} {}", s.id, u8::from(pass)));
        pass
    }

    /// UBTDecorator_Blackboard::EvaluateOnBlackboard: Set / NotSet for Object, Vector and Bool keys, an arithmetic
    /// compare against IntValue / FloatValue for the others.
    fn eval_blackboard(&self, p: &Props) -> bool {
        let key = selector(p, "BlackboardKey").and_then(|n| self.bb.key(n));
        let Some(ty) = self.bb.ty(key) else { return false };
        let v = self.bb.get(key);
        // `p.OperationType ?? 0`: a value that is no number matches no operation.
        let op = match p.get("OperationType") {
            None | Some(Value::Null) => Some(0.0),
            Some(Value::Num(n)) => Some(*n),
            Some(_) => None,
        };
        match ty {
            BbType::Bool => (v == BbVal::Bool(true)) == (op == Some(0.0)),
            BbType::Int | BbType::Enum | BbType::Float => {
                let a = match v {
                    BbVal::Num(n) => n,
                    _ => Real::NAN,
                };
                let b = props::num(p, if ty == BbType::Float { "FloatValue" } else { "IntValue" }).unwrap_or(0.0);

                match op {
                    Some(0.0) => a == b,
                    Some(1.0) => a != b,
                    Some(2.0) => a < b,
                    Some(3.0) => a <= b,
                    Some(4.0) => a > b,
                    Some(5.0) => a >= b,
                    _ => false,
                }
            }
            _ => (v != BbVal::Null) == (op == Some(0.0)),
        }
    }

    /// DoDecoratorsAllowExecution: all decorators (the first failure stops), or the link's logic operations.
    fn allows(&mut self, w: &mut World, me: FighterId, l: &Link) -> bool {
        let Some(ops) = &l.ops else {
            return l.decs.iter().all(|&d| self.can_execute(w, me, d));
        };
        let mut k = 0;
        self.eval_ops(w, me, &l.decs, ops, &mut k)
    }

    fn eval_ops(&mut self, w: &mut World, me: FighterId, decs: &[usize], ops: &[Op], k: &mut usize) -> bool {
        let op = ops[*k];
        *k += 1;
        match op {
            Op::Test(n) => self.can_execute(w, me, decs[n]),
            Op::Not => !self.eval_ops(w, me, decs, ops, k),
            Op::And(n) => {
                let mut v = true;
                for _ in 0..n {
                    let x = self.eval_ops(w, me, decs, ops, k);
                    v = v && x;
                }
                v
            }
            Op::Or(n) => {
                let mut v = false;
                for _ in 0..n {
                    let x = self.eval_ops(w, me, decs, ops, k);
                    v = v || x;
                }
                v
            }
        }
    }

    /// NotifyDecoratorsOnDeactivation (exe 143216510): OnNodeProcessed (ForceSuccess; skipped when aborted),
    /// OnNodeDeactivation; with a result a Self observer goes and a LowerPriority one comes back.
    fn link_deactivation(&mut self, w: &mut World, me: FighterId, n: usize, i: i32, mut res: Res) -> Res {
        let shape = self.t.nodes[n].s.clone();
        let i = usize::try_from(i).expect("an active link");
        for &d in &shape.links[i].decs {
            let s = self.t.decs[d].s.clone();
            if res != Res::Aborted && s.kind == DecKind::ForceSuccess {
                res = Res::Succeeded;
            }
            match s.kind {
                DecKind::Cooldown => self.t.set_last_use(d, self.now),
                DecKind::SetTagCooldown => self.t.add_tag_cooldown(&s.props, self.now),
                DecKind::TagCooldown
                    if props::get(&s.props, "bActivatesCooldown").is_none()
                        || props::truthy(&s.props, "bActivatesCooldown") =>
                {
                    self.t.add_tag_cooldown(&s.props, self.now);
                }
                DecKind::Blueprint if s.has_impl => self.with_dec(w, me, d, |st, cx| st.finish(cx, res)),
                _ => {}
            }
            if res != Res::Aborted && s.abort == SELF {
                self.t.update(Aux::Dec(d), false);
            } else if res != Res::Aborted && s.abort == LOWER_PRIORITY {
                self.t.update(Aux::Dec(d), true);
            }
        }
        res
    }

    // --- services ---

    /// ScheduleNextTick (exe 143247350): lo + rand() / 32767 * (Interval + RandomDeviation - lo), lo =
    /// max(0, Interval - RandomDeviation), in single precision.
    fn schedule(&mut self, s: usize) {
        let p = &self.t.svcs[s].s.props;
        let iv = props::num(p, "Interval").unwrap_or(0.5);
        let dv = props::num(p, "RandomDeviation").unwrap_or(0.1);
        let t = self.timer(iv, dv);
        self.t.set_remain(s, t);
        crate::ai::log::line(|| format!("svc {} {}", self.t.svcs[s].s.id, t));
    }

    /// The draw of a service's next tick and of BTTask_Wait's time, in ticks.
    fn timer(&mut self, iv: Real, dv: Real) -> Real {
        let lo = max(0.0, iv - dv);
        let span = (dv + iv) - lo;
        ticks_for(((self.rand_int(32768.0) * RAND_SCALE) * span) + lo)
    }

    /// UBTService::NotifyParentActivation (exe 143240820): schedules only when the remaining time is used up
    /// (bRestartTimerOnEachActivation is unset in every tree), then OnSearchStart; bCallTickOnSearchStart is unset too.
    fn parent_activated(&mut self, w: &mut World, me: FighterId, s: usize) {
        if self.t.svcs[s].remain <= 0.0 {
            self.schedule(s);
        }
        self.with_svc(w, me, s, |st, cx| st.search_start(cx));
    }

    fn tick_service(&mut self, w: &mut World, me: FighterId, s: usize) {
        let sv = &mut self.t.svcs[s];
        sv.remain -= 1.0;
        sv.acc += crate::consts::DT;
        if sv.remain > 0.0 {
            return;
        }
        let dt = sv.acc;
        sv.acc = 0.0;
        self.schedule(s);
        if self.t.svcs[s].s.has_impl {
            self.with_svc(w, me, s, |st, cx| st.tick(cx, dt));
        } else {
            BtBot::no_class(w, &self.t.svcs[s].s.ty);
        }
    }

    // --- search (ProcessExecutionRequest) ---

    /// UBTCompositeNode::OnNodeActivation.
    fn activate_node(&mut self, w: &mut World, me: FighterId, n: usize) {
        self.t.set_cur(n, NOT_INIT);
        self.t.set_over(n, NOT_INIT);
        let shape = self.t.nodes[n].s.clone();
        for &s in &shape.services {
            self.t.update(Aux::Svc(s), true);
            self.parent_activated(w, me, s);
        }
    }

    /// UBTCompositeNode::OnNodeDeactivation (exe 1432174f0). A SimpleParallel reports its main task's result once that
    /// has finished (NotifyNodeDeactivation 1432407a0).
    fn deactivate_node(&mut self, n: usize, mut res: Res) -> Res {
        let shape = self.t.nodes[n].s.clone();
        let sp = shape.kind == NodeKind::SimpleParallel;
        if sp {
            if self.t.nodes[n].main_active {
                self.t.pend_par.push(n);
            } else {
                res = self.t.nodes[n].main_result;
            }
        }
        for &s in &shape.services {
            self.t.update(Aux::Svc(s), false);
        }
        // bApplyDecoratorScope (set by the SimpleParallel constructor, exe 143225370): the aux nodes under the node
        // are dropped with it, also the ones this search added.
        if sp || props::is_true(&shape.props, "bApplyDecoratorScope") {
            let pre = &shape.path;
            let t = &mut self.t;
            let inside = |t: &Tree, a: Aux| {
                let p = t.path_of(a);
                is_prefix(pre, p) && (matches!(a, Aux::Svc(_)) || p.len() > pre.len())
            };
            let pend = std::mem::take(&mut t.pend);
            t.pend = pend.into_iter().filter(|u| !(u.1 && inside(t, u.0))).collect();
            for a in t.aux.clone() {
                if inside(t, a) {
                    t.update(a, false);
                }
            }
        }
        res
    }

    /// The frame `n` was popped: deactivate it and tell the parent's link.
    fn leave(&mut self, w: &mut World, me: FighterId, n: usize, mut res: Res) -> Res {
        if self.t.nodes[n].s.composite {
            res = self.deactivate_node(n, res);
        } else {
            // The subtree instance is removed with all of its aux nodes.
            let sub = self.t.nodes[n].sub.expect("a pushed subtree");
            let pre = self.t.nodes[sub].s.clone();
            let t = &mut self.t;
            let pend = std::mem::take(&mut t.pend);
            t.pend = pend.into_iter().filter(|u| !is_prefix(&pre.path, t.path_of(u.0))).collect();
            for a in t.aux.clone() {
                if is_prefix(&pre.path, t.path_of(a)) {
                    t.update(a, false);
                }
            }
        }
        if let Some(p) = self.t.top()
            && self.t.nodes[p].s.composite
        {
            res = self.link_deactivation(w, me, p, self.t.nodes[p].cur, res);
        }
        res
    }

    /// DeactivateUpTo: from the active node up to the frame `t`.
    fn deactivate_up_to(&mut self, w: &mut World, me: FighterId, t: i64, mut res: Res) -> Res {
        if self.t.leaf.is_some() {
            let p = self.t.top().expect("a composite above the task");
            // SimpleParallel::CanNotifyDecoratorsOnDeactivation (exe 14322d540): not for the main task while it runs
            // (its decorators hear of its end from NotifyChildExecution).
            let pn = &self.t.nodes[p];
            if !(pn.s.kind == NodeKind::SimpleParallel && pn.cur == 0 && pn.main_active) {
                res = self.link_deactivation(w, me, p, pn.cur, res);
            }
            self.t.leaf = None;
        }
        while i64::try_from(self.t.path.len()).expect("a short path") - 1 > t {
            let n = self.t.path.pop().expect("a frame above t");
            res = self.leave(w, me, n, res);
        }
        res
    }

    /// GetNextChild / GetNextChildHandler.
    fn next_child(&mut self, n: usize, prev: i32, last: Res) -> i32 {
        let shape = self.t.nodes[n].s.clone();
        let len = i32::try_from(shape.links.len()).expect("a short list of children");
        // Newly activated node above the search start: go to the branch that holds it.
        if prev == NOT_INIT
            && let Some(ss) = &self.t.search_start
            && shape.path.len() < ss.len()
            && is_prefix(&shape.path, ss)
        {
            return crate::math::to_int32(ss[shape.path.len()]);
        }
        // OverrideChild is taken (and cleared) unless a restart is pending (exe 143211ea0: IsRestartPending, the
        // request's bTryNextChild false).
        let o = self.t.nodes[n].over;
        if o != NOT_INIT && self.t.try_next {
            self.t.set_over(n, NOT_INIT);
            return if o < len { o } else { RETURN };
        }
        match shape.kind {
            NodeKind::Selector => {
                if prev == NOT_INIT {
                    0
                } else if last == Res::Failed && prev + 1 < len {
                    prev + 1
                } else {
                    RETURN
                }
            }
            NodeKind::Sequence => {
                if prev == NOT_INIT {
                    0
                } else if last == Res::Succeeded && prev + 1 < len {
                    prev + 1
                } else {
                    RETURN
                }
            }
            // GetNextChildHandler (exe 1410747e0). Node memory holds a bit mask of the children not yet tried, filled
            // only when the handler sees NotInitialized (not when the search start picks the branch). Any result but
            // Succeeded (Failed, also Aborted / InProgress) draws one of the remaining children: k = rand() / 32767 *
            // count (clamped to count - 1), the k-th set bit is taken and cleared. Succeeded returns, also on
            // activation.
            NodeKind::RandomSelector => {
                if prev == NOT_INIT {
                    self.t.set_mask(n, 1_i32.wrapping_shl(len.cast_unsigned()).wrapping_sub(1));
                }
                if last == Res::Succeeded {
                    return RETURN;
                }
                let mask = self.t.nodes[n].mask;
                let c = mask.count_ones();
                if c == 0 {
                    return RETURN;
                }
                let c = crate::real(c);
                let draw = ((self.rand_int(32768.0) * RAND_SCALE) * c).trunc();
                let mut k = crate::math::to_int32(min(draw, c - 1.0));
                let mut m = mask;
                let mut b = 0;
                while m != 0 {
                    if m & 1 != 0 {
                        if k == 0 {
                            self.t.set_mask(n, mask & !1_i32.wrapping_shl(b));
                            return if b.cast_signed() < len { b.cast_signed() } else { RETURN };
                        }
                        k -= 1;
                    }
                    m >>= 1;
                    b += 1;
                }
                RETURN
            }
            // GetNextChildHandler (exe 1410746a0), with the defaults of its two unset fields (count 1: draw only on
            // NotInitialized; the no-repeat zeroing of the drawn weight then has no effect). r = rand() / 32767 * the
            // float sum of ChoiceWeights; the first child whose running sum (truncated to an integer at each step)
            // exceeds r runs, and its result is the Chooser's. r = sum (rand() = 32767) draws nothing.
            // Its memory (int count, then the weights copy) starts at the node memory offset, over FBTCompositeMemory:
            // the count writes (1, then 0) leave OverrideChild = 0, so once the drawn child ends the Chooser runs
            // child 0 again (CurrentChild, written after the draw, only clobbers the count's low byte).
            NodeKind::Chooser => {
                if prev != NOT_INIT {
                    return RETURN;
                }
                self.t.set_over(n, 0);
                let ws: Vec<Real> = props::nums(&shape.props, "ChoiceWeights").collect();
                let mut total = 0.0;
                for &x in &ws {
                    total += x;
                }
                let r = (self.rand_int(32768.0) * RAND_SCALE) * total;
                let mut cum: Real = 0.0;
                for (i, &x) in ws.iter().enumerate() {
                    cum = (cum + x).trunc();
                    if r < cum {
                        let i = i32::try_from(i).expect("a short list of weights");
                        return if i < len { i } else { RETURN };
                    }
                }
                RETURN
            }
            // GetNextChildHandler (exe 14323b6d0): a new activation starts with the main task's result at Failed.
            NodeKind::SimpleParallel => {
                let mut nx = RETURN;
                let node = &self.t.nodes[n];
                if prev == NOT_INIT {
                    nx = 0;
                    self.t.set_main_result(n, Res::Failed);
                } else if (node.main_active || node.force_bg) && !self.t.restarting && len > 1 {
                    // The background tree loops while the main task runs.
                    nx = 1;
                    self.t.set_force_bg(n, false);
                }
                // Re-entering the same branch within one search: retry on the next tick.
                if prev == nx && self.t.nodes[n].last_search == self.t.search_id {
                    self.t.postpone = true;
                }
                self.t.set_last_search(n, self.t.search_id);
                nx
            }
            _ => RETURN,
        }
    }

    /// FindChildToExecute. The result of the last child is left in `sres`.
    fn find_child(&mut self, w: &mut World, me: FighterId, n: usize, last: Res) -> i32 {
        self.t.sres = last;
        let shape = self.t.nodes[n].s.clone();
        if shape.links.is_empty() {
            return RETURN;
        }
        let mut i = self.next_child(n, self.t.nodes[n].cur, last);
        while i >= 0 && !self.t.postpone {
            let l = &shape.links[usize::try_from(i).expect("a child index")];
            if self.allows(w, me, l) {
                // OnChildActivation.
                for &d in &l.decs {
                    let s = self.t.decs[d].s.clone();
                    if s.kind == DecKind::Blueprint && s.has_impl {
                        self.with_dec(w, me, d, |st, cx| st.start(cx));
                    }
                    if s.abort == LOWER_PRIORITY {
                        self.t.update(Aux::Dec(d), false);
                    } else if s.abort != 0 {
                        self.t.update(Aux::Dec(d), true);
                    }
                }
                if self.t.nodes[l.node].s.composite {
                    self.t.path.push(l.node);
                    self.activate_node(w, me, l.node);
                } else {
                    self.t.leaf = Some(l.node);
                }
                self.t.set_cur(n, i);
                return i;
            }
            // NotifyDecoratorsOnFailedActivation.
            self.t.sres = Res::Failed;
            for &d in &l.decs {
                let s = &self.t.decs[d].s;
                if s.kind == DecKind::ForceSuccess {
                    self.t.sres = Res::Succeeded;
                }
                if s.abort & LOWER_PRIORITY != 0 {
                    self.t.update(Aux::Dec(d), true);
                }
            }
            i = self.next_child(n, i, self.t.sres);
        }
        RETURN
    }

    fn rollback(&mut self, saved_path: Vec<usize>, saved_leaf: Option<usize>) {
        while let Some(u) = self.t.undo.pop() {
            let t = &mut self.t;
            match u {
                Undo::Cur(n, v) => t.nodes[n].cur = v,
                Undo::Over(n, v) => t.nodes[n].over = v,
                Undo::Mask(n, v) => t.nodes[n].mask = v,
                Undo::MainResult(n, v) => t.nodes[n].main_result = v,
                Undo::ForceBg(n, v) => t.nodes[n].force_bg = v,
                Undo::LastSearch(n, v) => t.nodes[n].last_search = v,
                Undo::LastUse(d, v) => t.decs[d].last_use = v,
                Undo::Remain(s, v) => t.svcs[s].remain = v,
            }
        }
        self.t.path = saved_path;
        self.t.leaf = saved_leaf;
        self.t.searching = false;
    }

    #[allow(clippy::too_many_lines, reason = "ProcessExecutionRequest and its apply, one function as in the build")]
    fn process(&mut self, w: &mut World, me: FighterId) {
        let r = self.t.req.take().expect("a pending request");
        let saved_path = self.t.path.clone();
        let saved_leaf = self.t.leaf;
        let t_ = &mut self.t;
        t_.searching = true;
        t_.restarting = r.t == Rq::Restart;
        t_.try_next = r.res != Res::Aborted;
        t_.search_id += 1;
        t_.search_start = None;
        t_.postpone = false;
        t_.pend.clear();
        t_.pend_par.clear();
        t_.undo.clear();
        let mut res = r.res;
        let mut t = i64::try_from(self.t.path.len()).expect("a short path") - 1;
        let mut base = saved_path.clone();
        let mut dead: Option<Vec<Real>> = None;
        match r.t {
            Rq::Start => {
                // OnTreeFinished left a Remove for every aux node (UnregisterAuxNodesUpTo((0, 0)), exe 143219b40).
                for a in self.t.aux.clone() {
                    self.t.update(a, false);
                }
                self.t.path = vec![self.t.root];
                self.t.leaf = None;
                self.activate_node(w, me, self.t.root);
                base.clone_from(&self.t.path);
            }
            Rq::Sub => {
                let task = self.t.top().filter(|&n| !self.t.nodes[n].s.composite).and_then(|n| self.t.nodes[n].sub);
                let Some(sub) = task else { return self.rollback(saved_path, saved_leaf) };
                self.t.path.push(sub);
                self.activate_node(w, me, sub);
                base.clone_from(&self.t.path);
            }
            Rq::Fail => {
                t = Tree::index_in(&self.t.path, self.t.decs[r.d.expect("a decorator's request")].s.parent);
            }
            Rq::Par => t = Tree::index_in(&self.t.path, r.sp.expect("a SimpleParallel's request")),
            Rq::Restart => {
                t = r.top.map_or(-1, |top| Tree::index_in(&self.t.path, top));
                if t < 0 {
                    let dp = &self.t.nodes[self.t.decs[r.d.expect("a decorator's request")].s.parent].s.path;
                    t = i64::try_from(self.t.path.len()).expect("a short path") - 1;
                    while t > 0 {
                        let n = &self.t.nodes[self.t.path[usize::try_from(t).expect("t > 0")]].s;
                        if n.composite && is_prefix(&n.path, dp) {
                            break;
                        }
                        t -= 1;
                    }
                }
            }
            Rq::Cont => {}
        }
        if r.t >= Rq::Cont {
            if t < 0 {
                return self.rollback(saved_path, saved_leaf);
            }
            let tu = usize::try_from(t).expect("t >= 0");
            dead = if self.t.path.len() - 1 > tu {
                Some(self.t.nodes[self.t.path[tu + 1]].s.path.clone())
            } else {
                self.t.leaf.map(|l| self.t.nodes[l].s.path.clone())
            };
            res = self.deactivate_up_to(w, me, t, res);
            if r.t != Rq::Restart && r.res == Res::Failed {
                // A failing request: the aux nodes past the requester go (UnregisterAuxNodesUpTo(SearchStart)).
                let p: Vec<Real> = match r.d {
                    Some(d) => self.t.decs[d].s.path.clone(),
                    None if r.t == Rq::Cont => r.key[..r.key.len() - 1].to_vec(),
                    None => r.key.clone(),
                };
                for a in self.t.aux.clone() {
                    let c = cmp(self.t.path_of(a), &p);
                    let same_link_after = match (a, r.d) {
                        (Aux::Svc(_), Some(_)) => true,
                        (Aux::Dec(ad), Some(rd)) => self.t.decs[ad].s.idx > self.t.decs[rd].s.idx,
                        (_, None) => false,
                    };
                    if c == Ordering::Greater || (c == Ordering::Equal && same_link_after) {
                        self.t.update(a, false);
                    }
                }
            }
        }
        if r.t == Rq::Restart {
            // Aux nodes from the search start on are removed (the search adds back what it enters); OnNodeRestart.
            let link = self.t.decs[r.d.expect("a decorator's request")].s.path.clone();
            for a in self.t.aux.clone() {
                if cmp(self.t.path_of(a), &link) != Ordering::Less {
                    self.t.update(a, false);
                }
            }
            let top = self.t.top().expect("a frame to restart under");
            self.t.set_cur(top, NOT_INIT);
            self.t.set_over(top, NOT_INIT);
            self.t.search_start = Some(link);
        }

        let mut next: Option<usize> = None;
        while let Some(n) = self.t.top() {
            let i = self.find_child(w, me, n, res);
            res = self.t.sres;
            if self.t.postpone {
                break;
            }
            if i == RETURN {
                self.t.path.pop();
                res = self.leave(w, me, n, res);
                if let Some(p) = self.t.top()
                    && !self.t.nodes[p].s.composite
                {
                    // Subtree finished: its RunBehavior task ends with the root's result.
                    self.t.path.pop();
                    res = self.leave(w, me, p, res);
                }
            } else {
                let child = self.t.nodes[n].s.links[usize::try_from(i).expect("a child index")].node;
                if !self.t.nodes[child].s.composite {
                    next = Some(child);
                    break;
                }
            }
        }
        if self.t.postpone {
            self.rollback(saved_path, saved_leaf);
            if self.t.req.is_none() {
                self.t.req = Some(r);
            }
            return;
        }
        // A limited search has to find its task inside the range, else the running task carries on.
        if let (Some(nx), Some(end)) = (next, &r.end)
            && after_branch(&self.t.nodes[nx].s.path, end)
        {
            return self.rollback(saved_path, saved_leaf);
        }

        // ProcessPendingExecution (exe 143219b40): aux nodes past the new task are dropped (UnregisterAuxNodesUpTo).
        for a in self.t.aux.clone() {
            if next.is_none_or(|nx| cmp(self.t.path_of(a), &self.t.nodes[nx].s.path) == Ordering::Greater) {
                self.t.update(a, false);
            }
        }
        self.t.searching = false;
        // Requests from the branch that was left are dropped while the old task is aborted and the changes are
        // applied (bFilterOutRequestFromDeactivatedBranch).
        self.t.filter = true;
        self.t.dead = dead;
        let mut stale: Option<(usize, u8)> = None;
        if let Some(run) = self.t.run.take() {
            if run.delay.is_some() && run.graph {
                stale = Some((run.rn, run.dn));
            }
            self.abort_run(w, me, run);
        }
        for sp in self.t.pend_par.clone() {
            let Some(k) = self.t.par.iter().position(|x| self.t.nodes[x.rn].s.parent == Some(sp)) else { continue };
            let run = self.t.par.remove(k);
            // The search re-entered the SimpleParallel and takes this main task again: its abort (the task's Remove
            // update, ApplySearchUpdates 143209460 -> WrappedAbortTask) and its new execution are in one call, so the
            // stale-Delay rule of the active task holds for it too (the latent list is per task object).
            if Some(run.rn) == next && run.delay.is_some() && run.graph {
                stale = Some((run.rn, run.dn));
            }
            self.abort_run(w, me, run);
            // NotifyChildExecution (exe 1432405e0) with Aborted.
            self.t.nodes[sp].main_active = false;
            self.t.nodes[sp].main_result = Res::Aborted;
            self.link_deactivation(w, me, sp, 0, Res::Aborted);
        }
        // ApplySearchUpdates (exe 143209460): removals and decorators in the order of the updates, then the new
        // services. An Add past the new task is skipped; the root's services stay while the tree loops.
        let mut fresh = Vec::new();
        let pend = self.t.pend.clone();
        for &(a, add) in &pend {
            let k = self.t.aux.iter().position(|&x| x == a);
            if add {
                let past = next.is_none_or(|nx| cmp(self.t.path_of(a), &self.t.nodes[nx].s.path) == Ordering::Greater);
                if k.is_some() || past {
                    continue;
                }
                match a {
                    Aux::Svc(s) => fresh.push(s),
                    Aux::Dec(_) => self.t.aux.push(a),
                }
            } else if let Some(k) = k
                && !matches!(a, Aux::Svc(s) if self.t.svcs[s].s.owner == self.t.root)
            {
                self.remove_aux(w, me, k);
            }
        }
        for s in fresh {
            self.t.aux.push(Aux::Svc(s));
            self.with_svc(w, me, s, |st, cx| st.activate(cx));
        }
        self.t.sort_aux();
        self.t.filter = false;
        self.t.dead = None;
        // ApplySearchData (exe 143209260) then ticks every aux node of an Add update, a node that was already active
        // too (the root services of a pushed subtree), with the old task still the instance's active node.
        if next.is_some() {
            self.t.base = Some(base);
            for &(a, add) in &pend {
                if !add || !self.t.aux.contains(&a) {
                    continue;
                }
                match a {
                    Aux::Svc(s) => self.tick_service(w, me, s),
                    Aux::Dec(d) => self.tick_dec(w, me, d),
                }
            }
            self.t.base = None;
        }
        if let Some(nx) = next {
            let st = stale.filter(|s| s.0 == nx).map(|s| s.1);
            self.execute(w, me, nx, st);
        } else {
            // OnTreeFinished: loop.
            self.log(|| "tree finished".into());
            self.t.path.clear();
            self.t.leaf = None;
            self.t.act = None;
            self.store_req(Req::new(Rq::Start, Vec::new(), Res::InProgress));
        }
    }

    fn remove_aux(&mut self, w: &mut World, me: FighterId, k: usize) {
        let a = self.t.aux.remove(k);
        if let Aux::Svc(s) = a {
            self.with_svc(w, me, s, |st, cx| st.deactivate(cx));
        }
    }

    // --- tasks ---

    /// `stale`: the Delay node the execution aborted in this call waited in.
    fn execute(&mut self, w: &mut World, me: FighterId, n: usize, stale: Option<u8>) {
        self.t.act = Some(n);
        let shape = self.t.nodes[n].s.clone();
        self.log(|| format!("exec {}", shape.id));
        let p = &shape.props;
        let mut res = Res::InProgress;
        let mut run = Run { rn: n, def: false, graph: false, delay: None, wait: None, dn: 0 };
        match shape.kind {
            NodeKind::Wait => {
                // ExecuteTask (exe 143236ac0): the same draw as a service's ScheduleNextTick.
                let wt = props::num(p, "WaitTime").unwrap_or(5.0);
                let dv = props::num(p, "RandomDeviation").unwrap_or(0.0);
                run.wait = Some(self.timer(wt, dv));
            }
            NodeKind::FinishWithResult => {
                res = match props::str(p, "Result") {
                    Some("EBTNodeResult::Failed") => Res::Failed,
                    Some("EBTNodeResult::Aborted") => Res::Aborted,
                    _ => Res::Succeeded,
                };
            }
            NodeKind::SetTagCooldown => {
                self.t.add_tag_cooldown(p, self.now);
                res = Res::Succeeded;
            }
            // UBTTask_MoveDirectlyToward (native). The class (constructor 143227d60) overrides nothing of
            // UBTTask_MoveTo's execution: ExecuteTask 143235680 -> PerformMoveTask 143243150 -> AAIController::MoveTo
            // 143215bc0 (the game's controller does not override it). A Vector key is a location request with
            // bProjectGoalLocation at its class default 1: the world has a UNavigationSystemV1 and no map has
            // navigation data, so ProjectPointToNavigation 1431badc0 returns false and MoveTo refuses the request
            // before its "already at goal" test: Failed inside ExecuteTask, in the search that reached the node, every
            // time. Nothing is written to the pawn: no input, no rotation, no focus, and the failure path
            // (OnPathFinished 143265a30) leaves the velocity alone because the fighter's CharMoveComp has
            // bUseAccelerationForPaths. No tick (TickTask 143249d40 acts only while waiting for a path), no memory
            // kept. An actor request (an Object key holding an actor) skips the projection and would be a real
            // straight-line path-following move: no tree of this build gives the node one, so it is refused, not
            // modelled.
            NodeKind::MoveDirectlyToward => {
                let key = selector(p, "BlackboardKey");
                let ty = self.bb.ty(key.and_then(|k| self.bb.key(k)));
                if ty != Some(BbType::Vector) {
                    let ty = ty.map_or_else(|| "unknown".to_owned(), |t| format!("{t:?}"));
                    warn_text(
                        w,
                        &format!(
                            "BTTask_MoveDirectlyToward on the {ty} key {}: only the Vector key request is ported (it \
                             fails)",
                            key.unwrap_or("undefined")
                        ),
                    );
                }
                res = Res::Failed;
            }
            NodeKind::RunBehavior | NodeKind::RunBehaviorDynamic => {
                if self.push_subtree(w, me, n) {
                    return;
                }
                res = Res::Failed;
            }
            _ => {
                if self.t.nodes[n].task.is_none() {
                    self.t.nodes[n].task = TaskSt::of(&shape.ty);
                }
                match self.t.nodes[n].task.as_ref().map(TaskSt::own_stale) {
                    None => {
                        BtBot::no_class(w, &shape.ty);
                        res = Res::Failed;
                    }
                    Some(own_stale) => {
                        run.def = true;
                        let step = self.with_task(w, me, n, stale.is_some(), |t, cx| t.run(cx, Resume::Execute));
                        res = BtBot::step(&mut run, step);
                        if stale == Some(run.dn) && run.delay.is_some() && !own_stale {
                            run.delay = None; // the Delay was not started
                        }
                    }
                }
            }
        }
        // SimpleParallel::NotifyChildExecution: a running main task becomes a parallel task and the background tree
        // is searched on the next tick.
        let sp = shape.parent.expect("a task under a composite");
        if self.t.nodes[sp].s.kind == NodeKind::SimpleParallel && shape.idx == 0 {
            self.t.nodes[sp].main_result = res;
            if res == Res::InProgress {
                self.t.nodes[sp].main_active = true;
                self.t.nodes[sp].force_bg = false;
                self.t.par.push(run);
                self.request(w, me, Req::new(Rq::Cont, path_of(&shape.path, 0.5), Res::Succeeded));
                return;
            }
        }
        if res == Res::InProgress {
            self.t.run = Some(run);
        } else {
            self.request(w, me, Req::new(Rq::Cont, path_of(&shape.path, 0.5), res));
        }
    }

    /// The pawn's injection map, under its character key (the pawn's SetDynamicSubtree calls, in the data's
    /// `inject`). A pawn without one fails every injected task: named once. A stock controller, or one whose
    /// ReadyForAI never runs, has none: Base_AIController's ReadyForAI never runs, so no SetDynamicSubtree
    /// (BaseFighter @176471 casts the controller to it), and a RunBehaviorDynamic node of this build names no
    /// DefaultBehaviorAsset.
    fn injection(&self, w: &World, me: FighterId, tag: &str) -> Option<String> {
        if self.stock || !self.ready_for_ai {
            return None;
        }
        let k = w.names.str(w.fighter(me).ch);
        let Some(map) = self.data.inject.get(k) else {
            warn_parts(w, &["bot trees: no injection map for the pawn ", k]);
            return None;
        };
        map.get(tag).cloned()
    }

    /// PushInstance: the subtree's root services start now, its first search runs on the next tick. A missing
    /// injection tag has no asset: the task fails.
    fn push_subtree(&mut self, w: &mut World, me: FighterId, n: usize) -> bool {
        let shape = self.t.nodes[n].s.clone();
        let name = if shape.kind == NodeKind::RunBehavior {
            props::str(&shape.props, "BehaviorAsset").map(str::to_owned)
        } else {
            let tag = js_string(shape.props.get("InjectionTag")).to_lowercase();
            self.injection(w, me, &tag)
        };
        let data = self.data.clone();
        let Some(def) = name.and_then(|nm| data.trees.get(&nm)) else { return false };
        let parent = shape.parent.expect("a task under a composite");
        if self.t.nodes[parent].s.kind == NodeKind::SimpleParallel && shape.idx == 0 {
            return false;
        }
        let sub = match self.t.nodes[n].sub {
            Some(s) => s,
            None => {
                let s = self.t.build(&data, &self.bb, &def.root, Some(n), 0, path_of(&self.t.nodes[n].s.path, 0.0));
                self.t.nodes[n].sub = Some(s);
                s
            }
        };
        self.t.leaf = None;
        self.t.path.push(n);
        let sub_shape = self.t.nodes[sub].s.clone();
        for &s in &sub_shape.services {
            if !self.t.aux.contains(&Aux::Svc(s)) {
                self.t.aux.push(Aux::Svc(s));
                self.with_svc(w, me, s, |st, cx| st.activate(cx));
            }
            self.parent_activated(w, me, s);
        }
        self.t.sort_aux();
        self.request(w, me, Req::new(Rq::Sub, path_of(&shape.path, 0.5), Res::InProgress));
        true
    }

    /// What a task's graph did up to its next Delay or its end (`advance`'s result).
    fn step(run: &mut Run, step: Step) -> Res {
        match step {
            Step::Finish(ok) => {
                run.graph = false;
                run.delay = None;
                if ok { Res::Succeeded } else { Res::Failed }
            }
            Step::Hang => {
                run.graph = false;
                run.delay = None;
                Res::InProgress
            }
            // A Blueprint Delay: a latent action counted in float32 by the world's latent pass (the Delay rule).
            Step::Delay { secs, node } => {
                run.graph = true;
                run.delay = Some(latent_frames(secs));
                run.dn = node;
                Res::InProgress
            }
        }
    }

    /// `advance`: runs the task's graph past its Delay, up to its next Delay or its end.
    fn advance(&mut self, w: &mut World, me: FighterId, rn: usize) -> Res {
        let step = self.with_task(w, me, rn, false, |t, cx| t.run(cx, Resume::Delay));
        let run = self.t.run_mut(rn).expect("the task in progress");
        BtBot::step(run, step)
    }

    /// The tasks' latent Delays. A Blueprint task object has no tick of its own that processes latent actions, so its
    /// Delays are counted by the world's leftover latent pass (UWorld::Tick 142b4e150: after the tick groups, before
    /// FTimerManager::Tick), in the frame of the tree's tick (TG_DuringPhysics, after the fighters): here, at the end
    /// of think, after the parallel and active tasks and before the pad goes out, so what a Delay's link writes to the
    /// pad reaches the next fighter step. A Delay made in this think's tick (an execution, a task tick) gets its first
    /// count here; one made in a link of this pass (the graph reaching its next Delay) is past the walk of the task's
    /// action list (TickLatentActionForObject 142b4ef00 runs the links after the walk) and is first counted by the
    /// next think's pass. A link's FinishExecute only requests: the next think's search takes it, as the next
    /// component tick does. A task aborted in this think's search has no pending Delay any more (`abort_run`): the
    /// removal RemoveActionsForObject queues is carried out by this same frame's pass, so the old action never fires,
    /// and the stale rule of execute only spans the call that aborted it.
    fn latent_pass(&mut self, w: &mut World, me: FighterId) {
        for rn in self.t.runs() {
            let Some(r) = self.t.run_mut(rn) else { continue };
            let Some(delay) = r.delay.as_mut() else { continue };
            *delay -= 1.0;
            if *delay > 0.0 {
                continue;
            }
            let res = self.advance(w, me, rn);
            if res != Res::InProgress {
                self.finish_run(w, me, rn, res);
            }
        }
    }

    /// OnTaskFinished for a latent task.
    fn finish_run(&mut self, w: &mut World, me: FighterId, rn: usize, mut res: Res) {
        if let Some(k) = self.t.par.iter().position(|r| r.rn == rn) {
            // Main task of a SimpleParallel (FinishMode Immediate): the background tree is aborted.
            self.t.par.remove(k);
            let sp = self.t.nodes[rn].s.parent.expect("a main task under its SimpleParallel");
            self.t.nodes[sp].main_result = res;
            self.t.nodes[sp].main_active = false;
            res = self.link_deactivation(w, me, sp, 0, res);
            let mut r = Req::new(Rq::Par, self.t.nodes[rn].s.path.clone(), res);
            r.sp = Some(sp);
            self.request(w, me, r);
        } else if self.t.run.is_some_and(|r| r.rn == rn) {
            self.t.run = None;
            let key = path_of(&self.t.nodes[rn].s.path, 0.5);
            self.request(w, me, Req::new(Rq::Cont, key, res));
        }
    }

    /// AbortTask: a Blueprint task's latent actions are dropped before ReceiveAbortAI.
    fn abort_run(&mut self, w: &mut World, me: FighterId, run: Run) {
        let shape = self.t.nodes[run.rn].s.clone();
        self.log(|| format!("abort {}", shape.id));
        if run.def {
            self.with_task(w, me, run.rn, false, |t, cx| t.abort(cx));
        }
    }
}
