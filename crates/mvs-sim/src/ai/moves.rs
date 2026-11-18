//! The bot steering and waypoint tasks: BotMoveTo, the waypoint finders, GetSafeAttackPoint and
//! BotPathToPoint, ported from their bytecode. Conventions as in `tasks.rs`.
// The waypoint finders keep the negated test `!(d < best)`: a NaN distance never replaces the best.
#![allow(clippy::neg_cmp_op_on_partial_ord, reason = "the negated comparisons keep the NaN result")]

use crate::Real;
use crate::actors::ActorRef;
use crate::ai::blackboard::{BbType, BbVal, BbVec};
use crate::ai::cx::{Cx, Resume, Step, Task};
use crate::ai::pad::Btn;
use crate::ai::props;
use crate::ai::sweep::trace_sweep;
use crate::ai::util::{
    INVALID, Waypoint, actor, clamp, corners, fighter, index_of, loc, loc_y, loc_z, sq, valid, vec, waypoints,
};
use crate::fighter::{FighterId, PlatformRef};
use crate::math::{max, min, ue};

const ZERO: BbVec = BbVec { x: 0.0, y: 0.0, z: 0.0 };

/// `locOf`: K2_GetActorLocation; None (or a destroyed actor) reads the zero vector.
fn loc_of(cx: &Cx, a: Option<ActorRef>) -> BbVec {
    match a {
        Some(a) if valid(cx.w, Some(a)) => loc(cx.w, a),
        _ => ZERO,
    }
}

/// `groundBelow`: LineTraceSingle straight down from a point (world Y, Z).
fn ground_below(cx: &mut Cx, y: Real, z: Real, dist: Real) -> bool {
    trace_sweep(cx.w, y, z, y, z - dist, 0.0, None).is_some()
}

/// `setVec`: the key's vector; the unset point (`INVALID`) is a null.
fn set_vec(cx: &mut Cx, prop: &str, v: BbVec) {
    let k = cx.key(prop);
    cx.set(k, if v == INVALID { BbVal::Null } else { BbVal::Vec(v) });
}

fn pnum(cx: &Cx, k: &str, d: Real) -> Real {
    props::num(cx.props, k).unwrap_or(d)
}

fn me_loc(cx: &Cx) -> (Real, Real) {
    let m = Some(ActorRef::Fighter(cx.me));
    (loc_y(cx.w, m), loc_z(cx.w, m))
}

// ------------------------------------------------------------------ BotMoveTo

/// The program counter of BotMoveTo's loop: the top, then the points a Delay resumes at.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
enum At {
    /// The top of the loop.
    #[default]
    Top,
    /// After the ground evade's Delay @12754.
    Evaded,
    /// The platform drop (POST 1, @8471).
    Drop,
    /// After the drop's Delay @9414.
    Dropped,
    /// The fast fall and the jump blocks.
    Fall,
    /// After the jump's Delay @1581.
    Jumped,
    /// The loop's end.
    Tail,
    /// After the loop's Delay @3408.
    Waited,
}

/// BotMoveTo_BTT_C: steers by stick, jump and evade toward a blackboard actor or point (or away
/// from it); no navmesh. One pass per execution, or with Blocking a pass every 0.2 s until the target is reached on
/// the ground or the time is up. A pass that does not reach the target reports failure and leaves the stick set.
/// ChaseInAir and FollowOverLedge are dead (TargetIsFighter is never written). The node's variables persist over its
/// executions: ReachedTargetWhileAirborn is never reset.
#[derive(Clone, Debug, Default)]
pub struct BotMoveTo {
    pc: At,
    time_left: Real,
    blast_bounds_initialized: bool,
    right: Real,
    left: Real,
    target_actor: Option<ActorRef>,
    target_is_actor: bool,
    target_base_fighter: Option<FighterId>,
    target_point: BbVec,
    reached_while_airborn: bool,
    move_fail_counter: Real,
    last_x: Real,
    evade_cooldown: Real,
    // The pass's locals, kept over its Delays.
    sy: Real,
    sz: Real,
    cur_x: Real,
    cur_y: Real,
    target_x: Real,
    target_y: Real,
    is_actor: bool,
}

impl BotMoveTo {
    fn grounded(cx: &Cx) -> bool {
        crate::fighter::grounded(cx.w, cx.me)
    }

    /// The cached actor's location; 0.75 half heights lower while the cached fighter walks. The half height is the
    /// pawn's PF_CapsuleHalfHeight (@2427, @3804): GetScaledCapsuleHalfHeight as its last walking pawn tick saved it
    /// (BaseFighter @33725), not the capsule's current one.
    // Unverified: taken as the template's (`scaled.cap_half`).
    fn actor_point(&self, cx: &Cx) -> BbVec {
        let l = loc_of(cx, self.target_actor);
        let z = match self.target_base_fighter {
            Some(f) if crate::fighter::grounded(cx.w, f) => l.z - 0.75 * cx.w.fighter(f).scaled.cap_half,
            _ => l.z,
        };
        BbVec { x: l.x, y: l.y, z }
    }

    /// IsTooCloseToSideBlastBounds: the bound in the travel direction.
    fn too_close(&self, prox: Real) -> bool {
        (if self.target_x > self.cur_x { self.left - self.cur_x } else { self.cur_x - self.right }) < prox
    }

    /// The top of the loop to the ground evade.
    fn top(&mut self, cx: &mut Cx) -> Option<Step> {
        self.is_actor = self.target_is_actor;
        if self.is_actor {
            self.target_point = self.actor_point(cx);
        }
        let tp = self.target_point;
        let (sy, sz) = me_loc(cx);
        self.sy = sy;
        self.sz = sz;
        self.cur_x = sy;
        self.cur_y = if self.is_actor { sz } else { sz - 0.75 * cx.w.fighter(cx.me).scaled.cap_half };
        self.target_x = tp.y;
        self.target_y = tp.z;
        if props::truthy(cx.props, "MoveAwayFrom") {
            // SetMoveAwayTarget: a point 20000 along Normal(self - target), not added to the own position. A vector
            // that overflows (unset point) normalizes to zero.
            // Deviation: the exe's Normal there multiplies by InvSqrt(inf), NaN; the port keeps the zero vector.
            let dx = -tp.x;
            let dy = sy - tp.y;
            let dz = self.cur_y - tp.z;
            let inf = ((dx * dx) + (dy * dy)) + (dz * dz) == Real::INFINITY;
            let n = if inf { [0.0; 3] } else { ue::normal([dx, dy, dz], 1e-4) };
            self.target_x = n[1] * 20000.0;
            self.target_y = n[2] * 20000.0;
        }
        let r2 = sq(pnum(cx, "Radius", 0.0));
        let thr = pnum(cx, "HorizontalThreshhold", 0.0);
        let reached = sq(tp.x) + sq(sy - tp.y) + sq(self.cur_y - tp.z) <= r2
            || (self.reached_while_airborn && props::is_true(cx.props, "Blocking"))
            || self.too_close(1000.0);
        if reached {
            if Self::grounded(cx) {
                cx.bot.pad.set_x(0.0);
                cx.bot.pad.set_y(0.0);
                return Some(Step::Finish(true));
            }
            self.reached_while_airborn = true;
        }
        let d = self.cur_x - self.target_x;
        let pad = |cx: &mut Cx, v: Real| cx.bot.pad.set_x(v);
        if Self::grounded(cx) {
            if d.abs() <= thr {
                pad(cx, 0.0);
                self.move_fail_counter = 0.0;
            } else {
                pad(cx, if d < 0.0 { -1.0 } else { 1.0 });
                // Ground evade: toward a point, or an actor at least 700 away; cooldown by difficulty.
                if props::truthy(cx.props, "AllowGroundEvade") && cx.bot.now > self.evade_cooldown {
                    let ta = loc_of(cx, if self.is_actor { self.target_actor } else { None });
                    // A stock controller: the cast @11889-11954 fails and the pass goes on at 8471 with no
                    // cooldown and no evade.
                    if !cx.bot.stock && (!self.is_actor || sq(ta.x) + sq(sy - ta.y) + sq(sz - ta.z) >= 490_000.0) {
                        let t = [20.0, 6.0, 1.5, 0.9];
                        let nd = cx.bot.difficulty;
                        let at = |i: Real| [0.0, 1.0, 2.0, 3.0].iter().position(|&x| x == i).map_or(0.0, |k| t[k]);
                        let (lo, hi) = (at(nd.floor()), at(nd.ceil()));
                        self.evade_cooldown = (lo + (hi - lo) * (nd - nd.floor())) + cx.bot.now;
                        cx.bot.pad.press(Btn::Dodge, None, false);
                        self.pc = At::Evaded;
                        return Some(Step::Delay { secs: 0.02, node: 2 }); // Delay @12754
                    }
                }
            }
        } else if self.too_close(1500.0) {
            pad(cx, if self.cur_x > self.target_x { -1.0 } else { 1.0 });
        } else if d.abs() > thr {
            pad(cx, if d < 0.0 { -1.0 } else { 1.0 });
        } else {
            // Over the target: brake.
            let vy = -cx.w.fighter(cx.me).body.vx;
            if vy < -150.0 {
                pad(cx, -1.0);
            } else if vy > 150.0 {
                pad(cx, 1.0);
            } else {
                pad(cx, 0.0);
                self.move_fail_counter = 0.0;
            }
        }
        self.pc = At::Drop;
        None
    }

    /// The jump: JumpPressed, Delay 0.3 (@1581), JumpReleased.
    fn jump(&mut self, cx: &mut Cx) -> Step {
        cx.bot.pad.press(Btn::Jump, None, false);
        self.pc = At::Jumped;
        Step::Delay { secs: 0.3, node: 1 }
    }
}

impl Task for BotMoveTo {
    fn run(&mut self, cx: &mut Cx, r: Resume) -> Step {
        if r == Resume::Execute {
            self.time_left = pnum(cx, "BlockingDuration", 0.0);
            if !self.blast_bounds_initialized {
                // Both bounds are minima over the blast boxes (from 1e8).
                let k = corners(cx.w);
                self.right = min(pnum(cx, "BlastBoundsRight", 0.0), k.bottom_right[0]);
                self.left = min(pnum(cx, "BlastBoundsLeft", 0.0), k.top_left[0]);
                self.blast_bounds_initialized = true;
            }
            let a = actor(cx, "Target");
            if valid(cx.w, a) {
                self.target_actor = a;
                self.target_is_actor = true;
                if let Some(f) = fighter(a) {
                    self.target_base_fighter = Some(f); // else the fighter of an earlier execution stays
                }
            } else {
                self.target_point = vec(cx, "Target");
                self.target_is_actor = false;
            }
            self.pc = At::Top;
        }
        loop {
            match self.pc {
                At::Top => {
                    if let Some(s) = self.top(cx) {
                        return s;
                    }
                }
                At::Evaded => {
                    cx.bot.pad.release(Btn::Dodge);
                    self.pc = At::Drop;
                }
                At::Drop => {
                    // POST 1 (8471): walking on a floor of object type Platform with the target more than 125
                    // below drops through it: MoveX(0), MoveY(-1), BPI_DownPressed, JumpPressed (the jump with down
                    // held); Delay(0.2), MoveY(0), JumpReleased, and on with the fast fall and jump blocks.
                    // Deviation: those blocks read this pass's positions; the direct DownPressed is held as a
                    // button for the 0.2 s (its release is the stick's own).
                    let on_stage_platform = matches!(cx.w.fighter(cx.me).platforms.base, Some(PlatformRef::Stage(_)));
                    if Self::grounded(cx)
                        && self.cur_y - self.target_y > 125.0
                        && props::is_true(cx.props, "AllowFallThroughFloor")
                        && on_stage_platform
                    {
                        cx.bot.pad.set_x(0.0);
                        cx.bot.pad.set_y(-1.0);
                        cx.bot.pad.down = true;
                        cx.bot.pad.press(Btn::Jump, None, false);
                        self.pc = At::Dropped;
                        return Step::Delay { secs: 0.2, node: 3 }; // Delay @9414
                    }
                    self.pc = At::Fall;
                }
                At::Dropped => {
                    cx.bot.pad.set_y(0.0);
                    cx.bot.pad.down = false;
                    cx.bot.pad.release(Btn::Jump);
                    self.pc = At::Fall;
                }
                At::Fall => {
                    let grounded = Self::grounded(cx);
                    if props::truthy(cx.props, "AllowFastFall") {
                        let y = if !grounded && self.target_y < self.cur_y { -1.0 } else { 0.0 };
                        cx.bot.pad.set_y(y);
                    }
                    // Jump: stuck, wall sliding, or the target is above and steeper than 0.7 (x / 0 = 0). From the
                    // ground only above difficulty 1.9 (or with a stock controller: Not(cast success) @1161 ORed
                    // in); in the air only while not rising.
                    let dy = self.target_y - self.cur_y;
                    let adx = (self.target_x - self.cur_x).abs();
                    let steep = dy > 50.0 && (if adx == 0.0 { 0.0 } else { dy / adx }) > 0.7;
                    let fm = cx.w.fighter(cx.me);
                    let (vx, vy, wall) = (fm.body.vx, fm.body.vy, fm.wall.wall_sliding);
                    let gate = props::is_true(cx.props, "AllowJumping")
                        && crate::fighter::can_jump(cx.w, cx.me)
                        && (self.move_fail_counter > 3.0 || wall || steep);
                    if gate && (!grounded || cx.bot.stock || cx.bot.difficulty > 1.9) {
                        if grounded || vy <= 0.0 {
                            return self.jump(cx);
                        }
                    } else if grounded && props::truthy(cx.props, "JumpAtLedge") {
                        // Jump off a ledge: the momentum carries the fighter closer and there is no ground 500
                        // below the point 0.4 s ahead (at most 350).
                        let ahead = clamp(-vx * 0.4, -350.0, 350.0);
                        if (self.cur_x - self.target_x).abs() > (self.cur_x + ahead - self.target_x).abs()
                            && !ground_below(cx, self.sy + ahead, self.cur_y, 500.0)
                        {
                            return self.jump(cx);
                        }
                    }
                    self.pc = At::Tail;
                }
                At::Jumped => {
                    cx.bot.pad.release(Btn::Jump);
                    self.pc = At::Tail;
                }
                At::Tail => {
                    self.move_fail_counter = if self.last_x == self.cur_x { self.move_fail_counter + 1.0 } else { 0.0 };
                    self.last_x = self.cur_x;
                    if !props::truthy(cx.props, "Blocking") {
                        return Step::Finish(false);
                    }
                    self.time_left -= 0.2;
                    if !(self.time_left > 0.0) {
                        cx.bot.pad.set_x(0.0);
                        return Step::Finish(false);
                    }
                    self.pc = At::Waited;
                    return Step::Delay { secs: 0.2, node: 4 }; // Delay @3408
                }
                At::Waited => self.pc = At::Top,
            }
        }
    }

    fn abort(&mut self, cx: &mut Cx) {
        cx.bot.pad.set_y(0.0);
        cx.bot.pad.set_x(0.0);
        cx.bot.pad.down = false;
        cx.bot.pad.release(Btn::Jump);
    }
}

/// BotPathToPoint_BTT_C: CreateMoveToProxyObject without a navmesh: the proxy's fail event
/// stops the stick and fails the task. The tick copies the target into RecoveryPoint.
/// CreateMoveToProxyObject (exe 14322db80): AlreadyAtGoal -> OnAtGoal and Failed -> OnNoPath (OnFail) on a 0.1 s
/// timer (DAT_143a4f21c), RequestSuccessful binds OnMoveCompleted. The map has no navmesh, so the goal projection
/// fails; and with no navigation system, HasReached tests the XY distance against Radius, which the |dY| < Radius
/// check already ruled out. Either way the request fails and OnFail runs 0.1 s later: MoveX(0), StopMovement,
/// failure.
#[derive(Clone, Debug, Default)]
pub struct BotPathToPoint;

impl Task for BotPathToPoint {
    fn run(&mut self, cx: &mut Cx, r: Resume) -> Step {
        if r == Resume::Delay {
            cx.bot.pad.set_x(0.0);
            return Step::Finish(false);
        }
        let (y, _) = me_loc(cx);
        if (vec(cx, "TargetLocation").y - y).abs() < pnum(cx, "Radius", 0.0) {
            return Step::Finish(false);
        }
        Step::Delay { secs: 0.1, node: 0 }
    }

    fn tick(&mut self, cx: &mut Cx, _dt: Real) -> Option<bool> {
        let t = vec(cx, "TargetLocation");
        set_vec(cx, "RecoveryPoint", t);
        let k = cx.key("Last Update Recovery Point Time");
        if cx.bot.bb.ty(k) == Some(BbType::Float) {
            let now = cx.bot.now;
            cx.set(k, BbVal::Num(now));
        }
        None
    }
}

// ------------------------------------------------------------------ waypoints

/// The level's waypoints (GetAllActorsOfClass), as the list a finder walks while it traces.
fn all_waypoints(cx: &Cx) -> Vec<Waypoint> {
    waypoints(cx.w).collect()
}

fn vec_of(w: &Waypoint) -> BbVec {
    BbVec { x: w.x, y: w.y, z: w.z }
}

/// FindClosestWaypointToActor_BTT_C: the waypoint nearest to the actor (None: the world
/// origin) with ground below it.
#[derive(Clone, Debug, Default)]
pub struct FindClosestWaypointToActor;

impl Task for FindClosestWaypointToActor {
    fn run(&mut self, cx: &mut Cx, _r: Resume) -> Step {
        let a = actor(cx, "Actor");
        let t = loc_of(cx, a);
        let edge_only = props::truthy(cx.props, "EdgeOnly");
        let mut best: Option<BbVec> = None;
        let mut bd = 0.0;
        for w in all_waypoints(cx) {
            if edge_only && !w.edge {
                continue;
            }
            let d = sq(w.x - t.x) + sq(w.y - t.y) + sq(w.z - t.z);
            if best.is_some() && !(d < bd) {
                continue;
            }
            if !ground_below(cx, w.y, w.z, 9999.0) {
                continue;
            }
            best = Some(vec_of(&w));
            bd = d;
        }
        let Some(best) = best else {
            return Step::Finish(false);
        };
        set_vec(cx, "Waypoint", best);
        Step::Finish(true)
    }
}

/// FindFurthestWaypointFromActor_BTT_C: the waypoint furthest from the actor; with
/// PreventCrossingActor not one that has the actor between it and the bot.
#[derive(Clone, Debug, Default)]
pub struct FindFurthestWaypointFromActor {
    self_y: Real,
}

impl Task for FindFurthestWaypointFromActor {
    fn run(&mut self, cx: &mut Cx, _r: Resume) -> Step {
        let a = actor(cx, "Actor");
        if !valid(cx.w, a) {
            return Step::Finish(false);
        }
        let t = loc_of(cx, a);
        let prevent = props::truthy(cx.props, "PreventCrossingActor");
        if prevent {
            self.self_y = me_loc(cx).0;
        }
        let mut best: Option<BbVec> = None;
        let mut bd = 0.0;
        for w in all_waypoints(cx) {
            let d = sq(w.x - t.x) + sq(w.y - t.y) + sq(w.z - t.z);
            if best.is_some() && !(d > bd) {
                continue;
            }
            if prevent && t.y >= min(w.y, self.self_y) && t.y <= max(w.y, self.self_y) {
                continue;
            }
            if !ground_below(cx, w.y, w.z, 20000.0) {
                continue;
            }
            best = Some(vec_of(&w));
            bd = d;
        }
        let Some(best) = best else {
            return Step::Finish(false);
        };
        set_vec(cx, "Waypoint", best);
        Step::Finish(true)
    }
}

/// FindRandomWaypoint_BTT_C: RandomIntegerInRange(0, Length) includes Length: one roll in N + 1
/// reads past the array, the world origin.
#[derive(Clone, Debug, Default)]
pub struct FindRandomWaypoint;

impl Task for FindRandomWaypoint {
    fn run(&mut self, cx: &mut Cx, _r: Resume) -> Step {
        let mut list = Vec::new();
        for w in all_waypoints(cx) {
            if ground_below(cx, w.y, w.z, 9999.0) {
                list.push(w);
            }
        }
        let n = crate::real(u32::try_from(list.len()).expect("a few waypoints"));
        let pick = cx.bot.rand_int_range(0.0, n);
        let v = list.get(index_of(pick)).map_or(ZERO, vec_of);
        set_vec(cx, "Waypoint", v);
        Step::Finish(true)
    }
}

/// SetWaypointForRecoveryOnly_BTT_C: the waypoint whose RecoveryWaypoint is the player index +
/// 1, cached on the node. The player index of MatchPlayerData 'Get Player Index' is `Fighter::id`: the world makes
/// its fighters in player-index order.
#[derive(Clone, Debug, Default)]
pub struct SetWaypointForRecoveryOnly {
    cached: Option<BbVec>,
}

impl Task for SetWaypointForRecoveryOnly {
    fn run(&mut self, cx: &mut Cx, _r: Resume) -> Step {
        if self.cached.is_none() {
            let want = crate::real(cx.w.fighter(cx.me).id) + 1.0;
            // InitializeCachedWaypoint @753: the first match returns
            self.cached = waypoints(cx.w).find(|w| w.recovery == want).map(|w| vec_of(&w));
            // Deviation: a stage with no waypoint numbered for this player (Trophy's E.D.G.E. 2: its
            // RecoveryOnlyWaypoint actors keep RecoveryWaypoint 0) gives the nearest waypoint with ground under it,
            // as UpdateRecoveryPoint's GoToWaypoint picks one. The build leaves WaypointBB unset, and the port's
            // unset vector (FLT_MAX) walks the bot off the stage. Unverified: what the build's unset key reads. A
            // tutorial's bots keep the build's rule (Map_TrophyRoom_Tutorial numbers none either; its steps are
            // scripted around them).
            if self.cached.is_none() && w_tutorial_free(cx) {
                let s = loc(cx.w, ActorRef::Fighter(cx.me));
                let list: Vec<_> = waypoints(cx.w).collect();
                let mut best: Option<(BbVec, Real)> = None;
                for p in list {
                    if trace_sweep(cx.w, p.y, p.z, p.y, p.z - 20000.0, 0.0, None).is_none() {
                        continue;
                    }
                    let d = sq(p.x - s.x) + sq(p.y - s.y) + sq(p.z - s.z);
                    if best.is_none_or(|(_, bd)| d < bd) {
                        best = Some((vec_of(&p), d));
                    }
                }
                self.cached = best.map(|b| b.0);
            }
        }
        if let Some(c) = self.cached {
            set_vec(cx, "WaypointBB", c);
        }
        Step::Finish(true)
    }
}

/// No tutorial runs on the bot's World.
fn w_tutorial_free(cx: &Cx) -> bool {
    !crate::modes::tutorial::running(cx.w)
}

/// GetSafeAttackPoint_BTT_C: 350 beyond the Safe actor, away from the enemy, when there is
/// ground under that point; else the Safe actor.
#[derive(Clone, Debug, Default)]
pub struct GetSafeAttackPoint;

impl Task for GetSafeAttackPoint {
    fn run(&mut self, cx: &mut Cx, _r: Resume) -> Step {
        let (safe, enemy) = (actor(cx, "Safe"), actor(cx, "EnemyActor"));
        let (Some(safe), Some(enemy)) = (safe, enemy) else {
            return Step::Finish(false);
        };
        if !valid(cx.w, Some(safe)) || !valid(cx.w, Some(enemy)) {
            return Step::Finish(false);
        }
        let s = loc(cx.w, safe);
        let off = if s.y > loc_y(cx.w, Some(enemy)) { 350.0 } else { -350.0 };
        let pt = BbVec { x: s.x, y: s.y + off, z: s.z };
        let v = if ground_below(cx, pt.y, pt.z, 20000.0) { pt } else { s };
        set_vec(cx, "TargetPoint", v);
        Step::Finish(true)
    }
}
