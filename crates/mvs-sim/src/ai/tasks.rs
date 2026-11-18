//! The bot Blueprint tasks: the BTTask_BlueprintBase classes under Panda_Main/Blueprints/AI,
//! ported from their bytecode. BotMoveTo and the waypoint tasks are in `moves.rs`.
//!
//! A task's graph is a struct with a program counter: `run` goes from the graph's start (a new execution) or from the
//! Delay it last returned, up to the next Delay (`Step::Delay`), FinishExecute (`Step::Finish`) or the end of a path
//! that calls neither (`Step::Hang`). The Blueprints work in world Y (side) and Z (up); sim x = -Y, y = Z. MoveX(+1) is
//! toward -Y, so the pad's x is the sim direction. FacingLeft is facing +Y: facing < 0, and the forward stick is
//! `facing`. Casts of the controlled pawn cannot fail here and are left out. Float variables that are counted down
//! are kept in single precision like the Blueprint's.
//!
//! EvadeButtonPressed(Direction) only overrides the stick for Direction 4 (Neutral Evade, see `evadeForce`);
//! BPI_EvadeButtonPressed passes 5, BotEvadeJumpFlee 2 (Forward) and BotMoveTo 5, so every bot evade takes the stick.
//! JumpPressed is JumpPressedLocal + CancelInvuln(true); BPI_JumpPressedLocal presses pass `local`.
//! MoveX / MoveY store MoveXAxisValue / MoveYAxisValue in the call, so GetInputDirection (GetDirection of the two)
//! reads what the task has just set. CharacterMovement IsWalking (IsMovingOnGround: Walking) is the sim's grounded
//! (the knockback mode is left on landing, Flying clears it); BranchManager IsAttacking (AttackIsActive > 0 or
//! BeginAttack) is `fighter::is_attacking`.
//!
//! FinishExecute inside the execute event only stores the result (UBTTask_BlueprintBase::FinishExecute 143237630:
//! with bStoreFinishResult, set by ExecuteTask 143235540 around the event, CurrentCallResult = the argument), so the
//! last call of the event decides; a task without a call stays InProgress.
//!
//! A task executed again in the call that aborted it in a Delay does not get that Delay when it is the first one the
//! new execution reaches (`Cx::stale`: the old action is still in the latent list, found by the node's UUID); the
//! runtime leaves such a task waiting until it is aborted. A task with more than one Delay node names the ones after
//! its first (`Step::Delay`'s `node`), one number per K2Node_Delay (the ubergraph offsets are in the comments).

use crate::Real;
use crate::actors::ActorRef;
use crate::ai::blackboard::{BbKey, BbVal, BbVec};
use crate::ai::cx::{Cx, Resume, Step, Task};
use crate::ai::pad::Btn;
use crate::ai::props;
use crate::ai::util::{
    actor, by_difficulty, clamp, dir_of, facing_left, fighter, index_of, loc, loc_y, loc_z, player_team,
    release_jump_if_held, sim_xy, stick_for, sub, valid, vec, vel,
};
use crate::fighter::FighterId;
use crate::input::Dir;
use crate::math::{max, ue};

// ------------------------------------------------------------------ the graph's calls

fn delay(secs: Real) -> Step {
    Step::Delay { secs, node: 0 }
}

fn delay_at(secs: Real, node: u8) -> Step {
    Step::Delay { secs, node }
}

/// `p.X ?? d`.
fn pnum(cx: &Cx, k: &str, d: Real) -> Real {
    props::num(cx.props, k).unwrap_or(d)
}

fn truthy(cx: &Cx, k: &str) -> bool {
    props::truthy(cx.props, k)
}

/// `c.me.facing`.
fn facing(cx: &Cx) -> Real {
    cx.w.fighter(cx.me).body.facing_dir
}

fn me_left(cx: &Cx) -> bool {
    facing_left(cx.w, cx.me)
}

/// `locY(c.me)`.
fn my_y(cx: &Cx) -> Real {
    loc_y(cx.w, Some(ActorRef::Fighter(cx.me)))
}

/// `locZ(c.me)`.
fn my_z(cx: &Cx) -> Real {
    loc_z(cx.w, Some(ActorRef::Fighter(cx.me)))
}

fn grounded(cx: &Cx) -> bool {
    crate::fighter::grounded(cx.w, cx.me)
}

fn set_x(cx: &mut Cx, v: Real) {
    cx.bot.pad.set_x(v);
}

fn set_y(cx: &mut Cx, v: Real) {
    cx.bot.pad.set_y(v);
}

/// `pad.x = pad.y = 0`: MoveY(0), then MoveX(0).
fn zero_stick(cx: &mut Cx) {
    cx.bot.pad.set_y(0.0);
    cx.bot.pad.set_x(0.0);
}

fn press(cx: &mut Cx, b: Btn, d: Option<Dir>) {
    cx.bot.pad.press(b, d, false);
}

/// BPI_JumpPressedLocal.
fn press_jump_local(cx: &mut Cx) {
    cx.bot.pad.press(Btn::Jump, None, true);
}

fn release(cx: &mut Cx, b: Btn) {
    cx.bot.pad.release(b);
}

/// `pad.inputDir(c.me.facing)`.
fn input_dir(cx: &Cx) -> Dir {
    cx.bot.pad.input_dir(facing(cx))
}

/// `locY(me) < locY(e)` against the facing: the enemy is behind.
fn facing_off(cx: &Cx, e: Option<ActorRef>) -> bool {
    me_left(cx) != (my_y(cx) < loc_y(cx.w, e))
}

// ------------------------------------------------------------------ attacks

/// BotAttack_BTT_C.
#[derive(Clone, Debug, Default)]
pub struct BotAttack;

impl Task for BotAttack {
    fn run(&mut self, cx: &mut Cx, r: Resume) -> Step {
        let hold = truthy(cx, "HoldStickInDirection");
        match r {
            Resume::Execute => {
                let d = dir_of(pnum(cx, "InputDirection", 0.0));
                if hold {
                    let (x, y) = stick_for(d, facing(cx));
                    set_x(cx, x);
                    set_y(cx, y);
                }
                press(cx, Btn::Attack, Some(d));
                let (t, dev) = (pnum(cx, "ChargeTime", 0.0), pnum(cx, "ChargeTimeRandomDeviation", 0.0));
                delay(cx.bot.rfir(t - dev, t + dev))
            }
            Resume::Delay => {
                release(cx, Btn::Attack);
                if hold {
                    zero_stick(cx);
                }
                Step::Finish(true)
            }
        }
    }

    fn abort(&mut self, cx: &mut Cx) {
        release(cx, Btn::Attack);
        if truthy(cx, "HoldStickInDirection") {
            zero_stick(cx);
        }
    }
}

/// The two forward tasks: the stick forward, 0.05 s, the button held for ChargeTime +- RandomDeviation.
fn forward_press(cx: &mut Cx, r: Resume, pc: &mut u8, b: Btn) -> Step {
    if r == Resume::Execute {
        *pc = 0;
    }
    match *pc {
        0 => {
            let f = facing(cx);
            set_x(cx, f);
            *pc = 1;
            delay(0.05)
        }
        1 => {
            press(cx, b, Some(Dir::Fwd));
            let dev = pnum(cx, "RandomDeviation", 0.0);
            let secs = pnum(cx, "ChargeTime", 0.0) + cx.bot.rfir(-dev, dev);
            *pc = 2;
            delay_at(secs, 1)
        }
        _ => {
            release(cx, b);
            set_x(cx, 0.0);
            Step::Finish(true)
        }
    }
}

/// BotAttackForward_BTT_C: the second Delay is @322 (the first is @15).
#[derive(Clone, Debug, Default)]
pub struct BotAttackForward {
    pc: u8,
}

impl Task for BotAttackForward {
    fn run(&mut self, cx: &mut Cx, r: Resume) -> Step {
        forward_press(cx, r, &mut self.pc, Btn::Attack)
    }

    fn abort(&mut self, cx: &mut Cx) {
        set_x(cx, 0.0);
        release(cx, Btn::Attack);
    }
}

/// BotSpecialForward_BTT_C: as BotAttackForward with the special button
/// (C019_IncomingProjectileAI); the second Delay is @243 (the first is @15).
#[derive(Clone, Debug, Default)]
pub struct BotSpecialForward {
    pc: u8,
}

impl Task for BotSpecialForward {
    fn run(&mut self, cx: &mut Cx, r: Resume) -> Step {
        forward_press(cx, r, &mut self.pc, Btn::Special)
    }

    fn abort(&mut self, cx: &mut Cx) {
        set_x(cx, 0.0);
        release(cx, Btn::Special);
    }
}

/// C019_RushAttack_BTT_C: C019_RushAttackAI holds forward and the attack button
/// (NormalAttackPressed(1), the number 1 of the enum as the Blueprint passes it) and checks every 0.05 s (the Delay
/// resumes at the test, @481 -> 15): done once the enemy is no longer valid or is within RushToDistance (300) on the
/// horizontal axis. Timeout is set to 5 and never counted down.
#[derive(Clone, Debug, Default)]
pub struct C019RushAttack {
    enemy: Option<ActorRef>,
}

impl C019RushAttack {
    fn check(&mut self, cx: &mut Cx) -> Step {
        let far = (my_y(cx) - loc_y(cx.w, self.enemy)).abs() > pnum(cx, "RushToDistance", 0.0);
        if valid(cx.w, self.enemy) && far {
            return delay(0.05);
        }
        release(cx, Btn::Attack);
        set_x(cx, 0.0);
        Step::Finish(true)
    }
}

impl Task for C019RushAttack {
    fn run(&mut self, cx: &mut Cx, r: Resume) -> Step {
        if r == Resume::Execute {
            self.enemy = actor(cx, "Enemy");
            let f = facing(cx);
            set_x(cx, f);
            press(cx, Btn::Attack, Some(Dir::Down));
        }
        self.check(cx)
    }

    fn abort(&mut self, cx: &mut Cx) {
        set_x(cx, 0.0);
        release(cx, Btn::Attack);
    }
}

/// C017_Cannonball_BTT_C: tree C017_Cannonball, which Pawn_C017 sets and then replaces:
/// C017_Grab is injected, so no running tree reaches it. ReceiveExecuteAI @812: EnemyFighter = the Enemy key's
/// fighter (@812-921; a failed cast keeps the member of the last execution), SpecialAttackPressed(1) (@1034), the
/// stick forward (@1072-1141), Delay of 0.0004 s per unit of horizontal distance to the enemy (@1186-1503) -> @15:
/// stick 0, SpecialAttackReleased, Delay 0.2 (@92) -> @147: SafetyDuration = 4, then every 0.05 s (Delay @349 ->
/// @416) while not walking and the time is left (@170-335): the stick toward the enemy (@485-762).
/// FinishExecute(true) @404. No abort event.
#[derive(Clone, Debug, Default)]
pub struct C017Cannonball {
    pc: u8,
    enemy: Option<FighterId>,
    safety: Real,
}

impl C017Cannonball {
    fn enemy(&self) -> Option<ActorRef> {
        self.enemy.map(ActorRef::Fighter)
    }

    /// `while (!me.grounded && safety > 0)`.
    fn check(&mut self, cx: &mut Cx) -> Step {
        if !grounded(cx) && self.safety > 0.0 {
            self.pc = 3;
            return delay_at(0.05, 2);
        }
        Step::Finish(true)
    }
}

impl Task for C017Cannonball {
    fn run(&mut self, cx: &mut Cx, r: Resume) -> Step {
        if r == Resume::Execute {
            self.pc = 0;
        }
        match self.pc {
            0 => {
                if let Some(e) = fighter(actor(cx, "Enemy")) {
                    self.enemy = Some(e);
                }
                press(cx, Btn::Special, Some(Dir::Down));
                let f = facing(cx);
                set_x(cx, f);
                let secs = sub(loc_y(cx.w, self.enemy()), my_y(cx)).abs() * 0.0004;
                self.pc = 1;
                delay(secs)
            }
            1 => {
                set_x(cx, 0.0);
                release(cx, Btn::Special);
                self.pc = 2;
                delay_at(0.2, 1)
            }
            2 => {
                self.safety = 4.0;
                self.check(cx)
            }
            _ => {
                self.safety = sub(self.safety, 0.05);
                let x = if my_y(cx) > loc_y(cx.w, self.enemy()) { 1.0 } else { -1.0 };
                set_x(cx, x);
                self.check(cx)
            }
        }
    }
}

/// BotAttackWithAbort_BTT_C: holds the attack until the fighter stops attacking, the time
/// runs out or the enemy comes inside AbortDistance. The loop's Delay is @783 (the first is @1422).
#[derive(Clone, Debug, Default)]
pub struct BotAttackWithAbort {
    pc: u8,
    enemy: Option<ActorRef>,
    remain: Real,
}

impl BotAttackWithAbort {
    fn btn(cx: &Cx) -> Btn {
        if truthy(cx, "UseSpecialAttack") { Btn::Special } else { Btn::Attack }
    }

    fn check(&mut self, cx: &mut Cx) -> Step {
        let ad = pnum(cx, "AbortDistance", 0.0);
        let dy = my_y(cx) - loc_y(cx.w, self.enemy);
        let dz = my_z(cx) - loc_z(cx.w, self.enemy);
        if !crate::fighter::is_attacking(cx.w, cx.me) || self.remain < 0.0 || dy * dy + dz * dz < ad * ad {
            let b = Self::btn(cx);
            release(cx, b);
            return Step::Finish(true);
        }
        self.pc = 2;
        delay_at(0.05, 1)
    }
}

impl Task for BotAttackWithAbort {
    fn run(&mut self, cx: &mut Cx, r: Resume) -> Step {
        if r == Resume::Execute {
            self.pc = 0;
        }
        match self.pc {
            0 => {
                self.enemy = actor(cx, "Enemy");
                let btn = Self::btn(cx);
                press(cx, btn, Some(dir_of(pnum(cx, "InputDirection", 0.0))));
                self.pc = 1;
                delay(0.1)
            }
            1 => {
                let (t, dev) = (pnum(cx, "ChargeTime", 0.0), pnum(cx, "ChargeTimeRandomDeviation", 0.0));
                self.remain = sub(cx.bot.rfir(t - dev, t + dev), 0.1);
                self.check(cx)
            }
            _ => {
                self.remain = sub(self.remain, 0.05);
                self.check(cx)
            }
        }
    }

    fn abort(&mut self, cx: &mut Cx) {
        let b = Self::btn(cx);
        release(cx, b);
    }
}

/// BotHeavyAttack_BTT_C: NormalAttackPressed and NormalAttackReleased in one frame.
#[derive(Clone, Debug, Default)]
pub struct BotHeavyAttack;

impl Task for BotHeavyAttack {
    fn run(&mut self, cx: &mut Cx, _r: Resume) -> Step {
        press(cx, Btn::Attack, Some(dir_of(pnum(cx, "Attack Direction", 0.0))));
        release(cx, Btn::Attack);
        Step::Finish(true)
    }
}

/// BotStruggle_BTT_C.
#[derive(Clone, Debug, Default)]
pub struct BotStruggle;

impl Task for BotStruggle {
    fn run(&mut self, cx: &mut Cx, _r: Resume) -> Step {
        press(cx, Btn::Attack, Some(Dir::Neutral));
        release(cx, Btn::Attack);
        Step::Finish(true)
    }
}

/// BotDownSpike_BTT_C.
#[derive(Clone, Debug, Default)]
pub struct BotDownSpike;

impl Task for BotDownSpike {
    fn run(&mut self, cx: &mut Cx, r: Resume) -> Step {
        match r {
            Resume::Execute => {
                set_x(cx, 0.0);
                press(cx, Btn::Attack, Some(Dir::Down));
                delay(0.05)
            }
            Resume::Delay => {
                release(cx, Btn::Attack);
                Step::Finish(true)
            }
        }
    }
}

/// BotSpecial_BTT_C.
#[derive(Clone, Debug, Default)]
pub struct BotSpecial;

impl Task for BotSpecial {
    fn run(&mut self, cx: &mut Cx, r: Resume) -> Step {
        match r {
            Resume::Execute => {
                let dev = pnum(cx, "RandomDeviation", 0.0);
                press(cx, Btn::Special, Some(dir_of(pnum(cx, "InputDirection", 0.0))));
                delay(pnum(cx, "ChargeTime", 0.0) + cx.bot.rfir(-dev, dev))
            }
            Resume::Delay => {
                release(cx, Btn::Special);
                Step::Finish(true)
            }
        }
    }

    fn abort(&mut self, cx: &mut Cx) {
        release(cx, Btn::Special);
    }
}

/// BotUpSpecial_BTT_C: the stick stays up afterwards.
#[derive(Clone, Debug, Default)]
pub struct BotUpSpecial;

impl Task for BotUpSpecial {
    fn run(&mut self, cx: &mut Cx, r: Resume) -> Step {
        match r {
            Resume::Execute => {
                set_y(cx, 1.0);
                set_x(cx, 0.0);
                let d = input_dir(cx);
                press(cx, Btn::Special, Some(d));
                delay(0.2)
            }
            Resume::Delay => {
                release(cx, Btn::Special);
                Step::Finish(true)
            }
        }
    }

    fn abort(&mut self, cx: &mut Cx) {
        release(cx, Btn::Special);
    }
}

/// BotUpSpecialToPoint_BTT_C. The Delays: @848 (@15 is on the abort's CancelEvadeOnAbort
/// path only, and never fires), then @465.
#[derive(Clone, Debug, Default)]
pub struct BotUpSpecialToPoint {
    pc: u8,
}

impl Task for BotUpSpecialToPoint {
    fn run(&mut self, cx: &mut Cx, r: Resume) -> Step {
        if r == Resume::Execute {
            self.pc = 0;
        }
        match self.pc {
            0 => {
                set_y(cx, 1.0);
                let d = input_dir(cx);
                press(cx, Btn::Special, Some(d));
                self.pc = 1;
                delay(0.1)
            }
            1 => {
                let x = if my_y(cx) > vec(cx, "LastGroundPosition").y { 1.0 } else { -1.0 };
                set_x(cx, x);
                self.pc = 2;
                delay_at(pnum(cx, "HoldDuration", 0.0), 1)
            }
            _ => {
                release(cx, Btn::Special);
                Step::Finish(true)
            }
        }
    }

    /// ReceiveAbortAI @903: MoveY(0) @978, MoveX(0) @1019, SpecialAttackReleased @1060, then (push_flow 1152) with
    /// CancelEvadeOnAbort (C021_Recovery's BotUpSpecialToPoint_BTT_C_0, the only node of the built trees that sets
    /// it) BPI_EvadeButtonPressed @1111 (the stick already zeroed: Neutral), goto 15: Delay(0.1) registered, pop_flow
    /// @69 -> FinishAbort @1152 in the same call. The task's OnTaskFinished queues that Delay for removal before it
    /// ever ticks, so its BPI_EvadeButtonReleased @70 never runs: the evade stays held until another task releases
    /// it. The build's evade goes out Neutral; the port's pad is read when the think ends, so a task the same search
    /// starts may have written the stick first.
    fn abort(&mut self, cx: &mut Cx) {
        zero_stick(cx);
        release(cx, Btn::Special);
        if truthy(cx, "CancelEvadeOnAbort") {
            press(cx, Btn::Dodge, None);
        }
    }
}

/// BotSideSpecialToActor_BTT_C: no abort event, an aborted run leaves the button held. The
/// Delays: the follow loop's @15, or @1709.
#[derive(Clone, Debug, Default)]
pub struct BotSideSpecialToActor {
    pc: u8,
    target: Option<ActorRef>,
    remain: Real,
}

impl BotSideSpecialToActor {
    fn follow_check(&mut self, cx: &mut Cx) -> Step {
        if self.remain > 0.0 {
            self.pc = 1;
            return delay(0.05);
        }
        self.done(cx)
    }

    fn done(&mut self, cx: &mut Cx) -> Step {
        release(cx, Btn::Special);
        set_y(cx, 0.0);
        Step::Finish(true)
    }
}

impl Task for BotSideSpecialToActor {
    fn run(&mut self, cx: &mut Cx, r: Resume) -> Step {
        if r == Resume::Execute {
            self.pc = 0;
        }
        match self.pc {
            0 => {
                self.target = actor(cx, "Actor");
                let y = if my_z(cx) < 0.0 { 1.0 } else { 0.0 };
                set_y(cx, y);
                let x = if my_y(cx) > loc_y(cx.w, self.target) { 1.0 } else { -1.0 };
                set_x(cx, x);
                let d = input_dir(cx);
                press(cx, Btn::Special, Some(d));
                if truthy(cx, "FollowTargetVertically") {
                    self.remain = pnum(cx, "Duration", 0.0);
                    self.follow_check(cx)
                } else {
                    self.pc = 2;
                    delay_at(pnum(cx, "Duration", 0.0), 1)
                }
            }
            1 => {
                self.remain = sub(self.remain, 0.05);
                let y = if loc_z(cx.w, self.target) > my_z(cx) { 1.0 } else { -1.0 };
                set_y(cx, y);
                self.follow_check(cx)
            }
            _ => self.done(cx),
        }
    }
}

/// BotSideSpecialToPoint_BTT_C: the Delays are @15, then @161.
#[derive(Clone, Debug, Default)]
pub struct BotSideSpecialToPoint {
    pc: u8,
}

impl Task for BotSideSpecialToPoint {
    fn run(&mut self, cx: &mut Cx, r: Resume) -> Step {
        if r == Resume::Execute {
            self.pc = 0;
        }
        match self.pc {
            0 => {
                set_y(cx, 0.0);
                let x = if my_y(cx) > vec(cx, "TargetPosition").y { 1.0 } else { -1.0 };
                set_x(cx, x);
                self.pc = 1;
                delay(0.05)
            }
            1 => {
                let d = input_dir(cx);
                press(cx, Btn::Special, Some(d));
                self.pc = 2;
                delay_at(pnum(cx, "Delay", 0.0), 1)
            }
            _ => {
                release(cx, Btn::Special);
                Step::Finish(true)
            }
        }
    }
}

// ------------------------------------------------------------------ jumps

/// BotShortJump_BTT_C.
#[derive(Clone, Debug, Default)]
pub struct BotShortJump;

impl Task for BotShortJump {
    fn run(&mut self, cx: &mut Cx, r: Resume) -> Step {
        match r {
            Resume::Execute => {
                release_jump_if_held(&mut cx.bot.pad);
                press(cx, Btn::Jump, None);
                delay(pnum(cx, "Duration", 0.0))
            }
            Resume::Delay => {
                release(cx, Btn::Jump);
                Step::Finish(true)
            }
        }
    }

    fn abort(&mut self, cx: &mut Cx) {
        release(cx, Btn::Jump);
    }
}

/// BotJumpUp_BTT_C: two jump presses 0.2 s apart; the button is never released.
#[derive(Clone, Debug, Default)]
pub struct BotJumpUp;

impl Task for BotJumpUp {
    fn run(&mut self, cx: &mut Cx, r: Resume) -> Step {
        match r {
            Resume::Execute => {
                release_jump_if_held(&mut cx.bot.pad);
                zero_stick(cx);
                press_jump_local(cx);
                delay(0.2)
            }
            Resume::Delay => {
                press_jump_local(cx);
                Step::Finish(true)
            }
        }
    }
}

/// BotDropThroughPlatform_BTT_C (Blueprints/AI; WatermelonSteven_BehaviorTree).
/// ReceiveExecute @208: the pawn (BaseFighter), BPI_MoveX(0) @479, BPI_MoveY(-1) @600, BPI_DownPressed @721,
/// BPI_JumpPressedLocal @837 (the drop through a platform: the jump with down held), Delay 0.2 (@153 -> @27):
/// BPI_MoveY(0) @106, FinishExecute(true) @15. No JumpReleased (the button stays down, as BotJumpUp's) and no
/// ReceiveAbort (an abort leaves the stick and the down as they are).
// Deviation: the DownPressed is held as a button for the 0.2 s, its release the stick's own.
#[derive(Clone, Debug, Default)]
pub struct BotDropThroughPlatform;

impl Task for BotDropThroughPlatform {
    fn run(&mut self, cx: &mut Cx, r: Resume) -> Step {
        match r {
            Resume::Execute => {
                set_x(cx, 0.0);
                set_y(cx, -1.0);
                cx.bot.pad.down = true;
                press_jump_local(cx);
                delay(0.2)
            }
            Resume::Delay => {
                set_y(cx, 0.0);
                cx.bot.pad.down = false;
                Step::Finish(true)
            }
        }
    }
}

/// BotJumpRandomDirection_BTT_C.
#[derive(Clone, Debug, Default)]
pub struct BotJumpRandomDirection;

impl Task for BotJumpRandomDirection {
    fn run(&mut self, cx: &mut Cx, r: Resume) -> Step {
        match r {
            Resume::Execute => {
                release_jump_if_held(&mut cx.bot.pad);
                set_y(cx, 0.0);
                let x = if cx.bot.rand() < 0.5 { 1.0 } else { -1.0 };
                set_x(cx, x);
                press_jump_local(cx);
                delay(0.2)
            }
            Resume::Delay => {
                press_jump_local(cx);
                Step::Finish(true)
            }
        }
    }
}

/// `jumpSideways`: the jump with the stick `dir` times the facing, released after 0.2 s.
fn jump_sideways(cx: &mut Cx, r: Resume, dir: Real) -> Step {
    match r {
        Resume::Execute => {
            release_jump_if_held(&mut cx.bot.pad);
            set_y(cx, 0.0);
            let x = dir * facing(cx);
            set_x(cx, x);
            press(cx, Btn::Jump, None);
            delay(0.2)
        }
        Resume::Delay => {
            release(cx, Btn::Jump);
            Step::Finish(true)
        }
    }
}

/// BotJumpForward_BTT_C.
#[derive(Clone, Debug, Default)]
pub struct BotJumpForward;

impl Task for BotJumpForward {
    fn run(&mut self, cx: &mut Cx, r: Resume) -> Step {
        jump_sideways(cx, r, 1.0)
    }
}

/// BotJumpBackward_BTT_C.
#[derive(Clone, Debug, Default)]
pub struct BotJumpBackward;

impl Task for BotJumpBackward {
    fn run(&mut self, cx: &mut Cx, r: Resume) -> Step {
        jump_sideways(cx, r, -1.0)
    }
}

/// BotJumpToTarget_BTT_C: the stick is analog here, the clamped Y distance.
#[derive(Clone, Debug, Default)]
pub struct BotJumpToTarget;

impl Task for BotJumpToTarget {
    fn run(&mut self, cx: &mut Cx, r: Resume) -> Step {
        match r {
            Resume::Execute => {
                release_jump_if_held(&mut cx.bot.pad);
                set_y(cx, 0.0);
                let e = actor(cx, "EnemyActor");
                let x = -clamp(loc_y(cx.w, e) - my_y(cx), -1.0, 1.0);
                set_x(cx, x);
                press(cx, Btn::Jump, None);
                delay(pnum(cx, "HoldDuration", 0.0))
            }
            Resume::Delay => {
                release(cx, Btn::Jump);
                Step::Finish(true)
            }
        }
    }
}

/// BotJumpToPoint_BTT_C.
#[derive(Clone, Debug, Default)]
pub struct BotJumpToPoint;

impl Task for BotJumpToPoint {
    fn run(&mut self, cx: &mut Cx, r: Resume) -> Step {
        match r {
            Resume::Execute => {
                release_jump_if_held(&mut cx.bot.pad);
                let x = if my_y(cx) > vec(cx, "LastGroundPosition").y { 1.0 } else { -1.0 };
                set_x(cx, x);
                press(cx, Btn::Jump, None);
                delay(pnum(cx, "JumpHoldTime", 0.0))
            }
            Resume::Delay => {
                release(cx, Btn::Jump);
                Step::Finish(true)
            }
        }
    }

    fn abort(&mut self, cx: &mut Cx) {
        release(cx, Btn::Jump);
        zero_stick(cx);
    }
}

/// BotJumpToCenter_BTT_C: the centre is world Y 0, Z 0.
#[derive(Clone, Debug, Default)]
pub struct BotJumpToCenter;

impl Task for BotJumpToCenter {
    fn run(&mut self, cx: &mut Cx, r: Resume) -> Step {
        match r {
            Resume::Execute => {
                release_jump_if_held(&mut cx.bot.pad);
                let y = if my_z(cx) < 0.0 { 1.0 } else { 0.0 };
                set_y(cx, y);
                let x = if my_y(cx) > 0.0 { 1.0 } else { -1.0 };
                set_x(cx, x);
                press(cx, Btn::Jump, None);
                delay(0.2)
            }
            Resume::Delay => {
                release(cx, Btn::Jump);
                Step::Finish(true)
            }
        }
    }
}

/// BotReleaseJumpButton_BTT_C.
#[derive(Clone, Debug, Default)]
pub struct BotReleaseJumpButton;

impl Task for BotReleaseJumpButton {
    fn run(&mut self, cx: &mut Cx, _r: Resume) -> Step {
        release(cx, Btn::Jump);
        Step::Finish(true)
    }
}

// ------------------------------------------------------------------ stick, crouch, misc

/// BotFastFall_BTT_C.
#[derive(Clone, Debug, Default)]
pub struct BotFastFall;

impl Task for BotFastFall {
    fn run(&mut self, cx: &mut Cx, r: Resume) -> Step {
        match r {
            Resume::Execute => {
                set_x(cx, 0.0);
                set_y(cx, -1.0);
                delay(pnum(cx, "Duration", 0.0))
            }
            Resume::Delay => {
                set_y(cx, 0.0);
                Step::Finish(true)
            }
        }
    }

    fn abort(&mut self, cx: &mut Cx) {
        set_y(cx, 0.0);
    }
}

/// BotCrouch_BTT_C: crouches until the time runs out or the dodged fighter has not been
/// attacking for 0.4 s. EnemyFighter is a member: a key that holds no fighter leaves the one of an earlier run.
/// The loop's Delay is @337 / @426, the plain wait's @921 / @913.
#[derive(Clone, Debug, Default)]
pub struct BotCrouch {
    pc: u8,
    enemy: Option<FighterId>,
    quiet: Real,
    remain: Real,
}

impl BotCrouch {
    fn step(&mut self, cx: &mut Cx, enemy: FighterId) -> Step {
        if self.remain >= 0.0 && self.quiet > 0.0 {
            self.quiet = if crate::fighter::is_attacking(cx.w, enemy) { 0.4 } else { sub(self.quiet, 0.1) };
            self.remain = sub(self.remain, 0.1);
            return delay(0.1);
        }
        cx.bot.pad.down = false;
        Step::Finish(true)
    }
}

impl Task for BotCrouch {
    fn run(&mut self, cx: &mut Cx, r: Resume) -> Step {
        if r == Resume::Execute {
            if let Some(e) = fighter(actor(cx, "EnemyBeingDodged")) {
                self.enemy = Some(e);
            }
            cx.bot.pad.down = true;
            zero_x_then_y(cx);
            let dev = pnum(cx, "CrouchTimeRandomDeviation", 0.0);
            let t = pnum(cx, "CrouchTime", 0.0) + cx.bot.rfir(-dev, dev);
            return match self.enemy {
                Some(e) => {
                    self.pc = 0;
                    self.quiet = 0.4;
                    self.remain = t;
                    self.step(cx, e)
                }
                None => {
                    self.pc = 1;
                    delay_at(t, 1)
                }
            };
        }
        match (self.pc, self.enemy) {
            (0, Some(e)) => self.step(cx, e),
            _ => {
                cx.bot.pad.down = false;
                Step::Finish(true)
            }
        }
    }

    fn abort(&mut self, cx: &mut Cx) {
        cx.bot.pad.down = false;
    }
}

/// `pad.x = 0; pad.y = 0`.
fn zero_x_then_y(cx: &mut Cx) {
    set_x(cx, 0.0);
    set_y(cx, 0.0);
}

/// BotCrouchUnderProjectile_BTT_C: crouches while the projectile's velocity points at the
/// fighter. The loop's Delay is @337 / @426, the plain wait's @921 / @913.
#[derive(Clone, Debug, Default)]
pub struct BotCrouchUnderProjectile {
    pc: u8,
    pr: Option<ActorRef>,
    remain: Real,
}

impl BotCrouchUnderProjectile {
    fn check(&mut self, cx: &mut Cx) -> Step {
        // A projectile destroyed meanwhile reads zero vectors.
        let coming = self.remain >= 0.0 && valid(cx.w, self.pr) && {
            let pr = self.pr.expect("a valid actor");
            let (px, py) = sim_xy(cx.w, pr);
            let (vy_world, vz) = vel(cx.w, Some(pr));
            let b = &cx.w.fighter(cx.me).body;
            (b.x - px) * -vy_world + (b.y - py) * vz > 0.0
        };
        if coming {
            self.remain = sub(self.remain, 0.1);
            return delay(0.1);
        }
        cx.bot.pad.down = false;
        Step::Finish(true)
    }
}

impl Task for BotCrouchUnderProjectile {
    fn run(&mut self, cx: &mut Cx, r: Resume) -> Step {
        if r == Resume::Execute {
            self.pr = actor(cx, "ProjectileBeingDodged");
            cx.bot.pad.down = true;
            zero_x_then_y(cx);
            let dev = pnum(cx, "CrouchTimeRandomDeviation", 0.0);
            let t = pnum(cx, "CrouchTime", 0.0) + cx.bot.rfir(-dev, dev);
            return if valid(cx.w, self.pr) {
                self.pc = 0;
                self.remain = t;
                self.check(cx)
            } else {
                self.pc = 1;
                delay_at(t, 1)
            };
        }
        if self.pc == 0 {
            return self.check(cx);
        }
        cx.bot.pad.down = false;
        Step::Finish(true)
    }

    fn abort(&mut self, cx: &mut Cx) {
        cx.bot.pad.down = false;
    }
}

/// BotTaunt_BTT_C: StopMovement, BFTaunt(0) (TauntPressedLogic with slot 0, at the AI tick),
/// then the result is the fighter's IsAttacking. The port's bots steer through the pad, so StopMovement (path
/// following) has nothing to stop.
#[derive(Clone, Debug, Default)]
pub struct BotTaunt;

impl Task for BotTaunt {
    fn run(&mut self, cx: &mut Cx, _r: Resume) -> Step {
        crate::fighter::control::bf_taunt(cx.w, cx.me, 0);
        Step::Finish(crate::fighter::is_attacking(cx.w, cx.me))
    }
}

/// BotExecutePause_BTT_C: spends up to 0.5 s of the PauseTime key standing still and facing
/// the enemy. A turn is a 0.05 s tap away from the facing; its own Delay takes another 0.05 s off TimeLeft.
#[derive(Clone, Debug, Default)]
pub struct BotExecutePause {
    /// 1: the turn Delay of a stale execution; 2: the loop's Delay.
    pc: u8,
    time_left: Real,
    /// A turn Delay is pending (read by an execution started in the abort's call).
    pend_t: bool,
    start: Real,
    enemy: Option<FighterId>,
    /// The latent action slots of this execution: 1 = T, 2 = L, 0 = free.
    slots: Vec<u8>,
    free: Vec<usize>,
    t_after: bool,
}

impl BotExecutePause {
    fn key(cx: &Cx) -> Option<BbKey> {
        cx.key("PauseTimeBB")
    }

    fn add(&mut self, id: u8) {
        match self.free.pop() {
            Some(i) => self.slots[i] = id,
            None => self.slots.push(id),
        }
    }

    /// The top of the loop.
    fn top(&mut self, cx: &mut Cx, enemy: FighterId) -> Step {
        let go = self.time_left > sub(self.start, 0.5);
        if go {
            if facing_off(cx, Some(ActorRef::Fighter(enemy))) {
                set_y(cx, 0.0);
                let x = if me_left(cx) { 1.0 } else { -1.0 };
                set_x(cx, x);
                self.add(1);
                self.pend_t = true;
            }
            self.add(2);
        } else {
            let k = Self::key(cx);
            cx.set(k, BbVal::Num(max(self.time_left, 0.0)));
        }
        if self.t_after {
            // T's link after L's: also after FinishExecute, so its 0.05 does not reach the blackboard.
            self.t_after = false;
            self.time_left = sub(self.time_left, 0.05);
            set_x(cx, 0.0);
        }
        if !go {
            return Step::Finish(true);
        }
        self.pc = 2;
        delay(0.05)
    }
}

impl Task for BotExecutePause {
    const OWN_STALE: bool = true;

    fn run(&mut self, cx: &mut Cx, r: Resume) -> Step {
        if r == Resume::Execute {
            let Some(enemy) = fighter(actor(cx, "TargetEnemyBB")) else {
                return Step::Finish(false);
            };
            self.enemy = Some(enemy);
            zero_stick(cx);
            let k = Self::key(cx);
            self.start = cx.bot.bb.num(k);
            self.time_left = self.start;
            if cx.stale {
                // Executed again in the call that aborted it: the loop Delay of the old execution is still in the
                // latent list, so this one's is not started and the task waits for an abort. The turn Delay starts
                // (and ends the tap) only when the old execution had none.
                if facing_off(cx, Some(ActorRef::Fighter(enemy))) {
                    let x = if me_left(cx) { 1.0 } else { -1.0 };
                    set_x(cx, x);
                    if !self.pend_t {
                        self.pc = 1;
                        return delay(0.05);
                    }
                }
                return Step::Hang;
            }
            self.pend_t = false;
            self.slots.clear();
            self.free.clear();
            self.t_after = false;
            return self.top(cx, enemy);
        }
        if self.pc == 1 {
            self.time_left = sub(self.time_left, 0.05);
            set_x(cx, 0.0);
            return Step::Hang;
        }
        let enemy = self.enemy.expect("the execution's enemy");
        // The loop Delay (L) and TurnTowardEnemy's Delay (T, same 0.05 s, started just before L) are due in the same
        // frame. TickLatentActionForObject 142b4ef00 updates the task's actions in the slot order of its action list
        // (a sparse array: a new action takes the most recently freed slot, else a new one), removes the finished
        // ones in that order and then runs their links in that order. The list is dropped when it is empty after the
        // pass (142b45500), so every execution starts with a fresh one: T runs before L for the pair started by the
        // execute event (or after a lone L in slot 0), after L once freed slots are reused (L takes slot 0).
        self.pend_t = false;
        let due: Vec<u8> = self.slots.iter().copied().filter(|&id| id > 0).collect();
        for (i, s) in self.slots.iter_mut().enumerate() {
            if *s > 0 {
                self.free.push(i);
            }
            *s = 0;
        }
        if due.first() == Some(&1) {
            self.time_left = sub(self.time_left, 0.05);
            set_x(cx, 0.0);
        } else {
            self.t_after = due.len() > 1;
        }
        self.time_left = sub(self.time_left, 0.05);
        self.top(cx, enemy)
    }

    fn abort(&mut self, cx: &mut Cx) {
        let k = Self::key(cx);
        cx.set(k, BbVal::Num(self.time_left));
    }
}

/// BotPickUpItem_BTT_C: ItemButtonPressed(Neutral), released 0.1 s later.
#[derive(Clone, Debug, Default)]
pub struct BotPickUpItem;

impl Task for BotPickUpItem {
    fn run(&mut self, cx: &mut Cx, r: Resume) -> Step {
        match r {
            Resume::Execute => {
                press(cx, Btn::Item, Some(Dir::Neutral));
                delay(0.1)
            }
            Resume::Delay => {
                release(cx, Btn::Item);
                Step::Finish(true)
            }
        }
    }
}

/// BotThrowItemAtAnyEnemy_C: airborne with an enemy within 1500 that lies 70..110 degrees off
/// the facing axis (above or below): throws the item down. The loop runs over every fighter (each FinishExecute is
/// followed by the loop's next step, @993 -> 1686), so each enemy in the cone presses again, and the
/// FinishExecute(false) after the loop (@1168) is the event's last call: the task always reports failure.
/// The list is GetAllActorsOfClass(BP_FighterCharacter) @1191: the minions too (`World::all_ids`). The team is the
/// PlayerDataManager's (@1776-2038, `player_team`): a minion reads the value the last call left, and the out value is
/// an ubergraph variable, so it carries over from the last execution (0 for a new node instance).
#[derive(Clone, Debug, Default)]
pub struct BotThrowItemAtAnyEnemy {
    team_index: i32,
}

impl Task for BotThrowItemAtAnyEnemy {
    fn run(&mut self, cx: &mut Cx, _r: Resume) -> Step {
        for t in cx.w.all_ids() {
            // BPI_IsDead; then the team index (which also leaves the bot itself out).
            if cx.w.fighter(t).life.dead > 0.0 {
                continue;
            }
            self.team_index = player_team(cx.w, t, self.team_index);
            if self.team_index == cx.w.fighter(cx.me).team.team {
                continue;
            }
            let tr = Some(ActorRef::Fighter(t));
            let dy = my_y(cx) - loc_y(cx.w, tr);
            let dz = my_z(cx) - loc_z(cx.w, tr);
            let len = ue::size2d([dy, dz]);
            if len >= 1500.0 || len.is_nan() {
                continue;
            }
            if grounded(cx) {
                continue; // IsWalking: FinishExecute(false)
            }
            // DegAcos(Dot(Normal(v, 0.0001), SelectVector((0, -1, 0), (0, 1, 0), FacingLeft))).
            let ny = ue::normal([0.0, dy, dz], 1e-4)[1];
            let angle = ue::deg_acos(ny * if me_left(cx) { -1.0 } else { 1.0 });
            if angle > 70.0 && angle < 110.0 {
                press(cx, Btn::Item, Some(Dir::Down)); // ItemButtonPressed(Down); never released
            }
        }
        Step::Finish(false)
    }
}

/// BotBeCarried_BTT_C: holds the attack while attached to Velma (GetAttachParentActor == the
/// key's actor, also when both are None). ReceiveExecuteAI @311: VelmaFighter = the Velma key (Base_BehaviorTree's
/// node names AllyActor), read once; NormalAttackPressed(4) @213, Delay 0.1 @251 (-> @15); then @15:
/// SelfFighter.GetAttachParentActor() == VelmaFighter -> Delay 0.1 @109 (-> @15), else NormalAttackReleased @164,
/// FinishExecute(true). The attach parent is the Instigator of a Buff_AttachToAlly family buff (`attach_parent_of`:
/// Buff_VelmaAttachToAlly, which VelmaCarryRun_Actor's ButtonPressed puts on the ally that pressed in its
/// JumpOnCollision).
#[derive(Clone, Debug, Default)]
pub struct BotBeCarried {
    velma: Option<ActorRef>,
}

impl Task for BotBeCarried {
    fn run(&mut self, cx: &mut Cx, r: Resume) -> Step {
        if r == Resume::Execute {
            self.velma = actor(cx, "Velma");
            press(cx, Btn::Attack, Some(Dir::Neutral));
            return delay(0.1);
        }
        if crate::fighter::moves::attach_parent_of(cx.w, cx.me) == self.velma {
            return delay_at(0.1, 1);
        }
        release(cx, Btn::Attack);
        Step::Finish(true)
    }

    fn abort(&mut self, cx: &mut Cx) {
        release(cx, Btn::Attack);
    }
}

/// ClearBlackboardValue_BTT_C: reports failure.
#[derive(Clone, Debug, Default)]
pub struct ClearBlackboardValue;

impl Task for ClearBlackboardValue {
    fn run(&mut self, cx: &mut Cx, _r: Resume) -> Step {
        let k = cx.key("BlackboardKey");
        cx.clear(k);
        Step::Finish(false)
    }
}

/// DifficultyCancel_BTT_C: the chance table (DifficultyCancels) is never filled: Initialize is
/// not called, so the chance is 0 and the task succeeds. The MoveX / MoveY(0) go to the controller's empty stubs.
/// A stock controller: the cast @10-75 fails, FinishExecute(true) with no RandomFloat.
#[derive(Clone, Debug, Default)]
pub struct DifficultyCancel;

impl Task for DifficultyCancel {
    fn run(&mut self, cx: &mut Cx, _r: Resume) -> Step {
        if cx.bot.stock {
            return Step::Finish(true);
        }
        Step::Finish(cx.bot.rand() >= 0.0)
    }
}

/// DifficultyDelay_BTT_C. A stock controller: GetCooldownTime's cast @0-65 fails, its zeroed
/// output is the cooldown (0), and the task finishes at once.
#[derive(Clone, Debug, Default)]
pub struct DifficultyDelay;

impl Task for DifficultyDelay {
    fn run(&mut self, cx: &mut Cx, r: Resume) -> Step {
        if r == Resume::Delay {
            return Step::Finish(true);
        }
        let cd = if cx.bot.stock { 0.0 } else { by_difficulty(cx, cx.props, "BotDifficultyDelays") };
        if cd > 0.0 {
            zero_stick(cx);
            return delay(cd);
        }
        Step::Finish(true)
    }

    fn abort(&mut self, cx: &mut Cx) {
        zero_stick(cx);
    }
}

/// DifficultyPause_BTD_C: raises the PauseTime key to this difficulty's pause
/// (+- VariationPct). Keyed by BotDifficulty.
/// A stock controller: the cast @10-75 fails and the graph returns @638 with no FinishExecute, so the task stays in
/// progress until an abort.
#[derive(Clone, Debug, Default)]
pub struct DifficultyPause;

impl Task for DifficultyPause {
    fn run(&mut self, cx: &mut Cx, r: Resume) -> Step {
        if cx.bot.stock {
            // Waits 3600 s, again and again.
            return delay(3600.0);
        }
        debug_assert!(r == Resume::Execute, "a Delay of a non-stock DifficultyPause");
        let k = cx.key("PauseTimeBB");
        let v = props::map_find(cx.props, "BotDifficultyDelays", cx.bot.behavior);
        let spread = v.unwrap_or(0.0) * pnum(cx, "VariationPct", 0.0);
        let pause = v.unwrap_or(0.0) + cx.bot.rfir(-spread, spread);
        if v.is_some() && pause > cx.bot.bb.num(k) {
            cx.set(k, BbVal::Num(pause));
        }
        Step::Finish(true)
    }
}

/// WaitUntilOnGround_BTD_C.
#[derive(Clone, Debug, Default)]
pub struct WaitUntilOnGround;

impl Task for WaitUntilOnGround {
    fn run(&mut self, cx: &mut Cx, _r: Resume) -> Step {
        if grounded(cx) { Step::Finish(true) } else { delay(0.05) }
    }
}

/// BotDI_BTD_C: sets the DI stick and reports failure.
#[derive(Clone, Debug, Default)]
pub struct BotDI;

impl Task for BotDI {
    fn run(&mut self, cx: &mut Cx, _r: Resume) -> Step {
        let dx = pnum(cx, "DI_X", 0.0);
        let x = if me_left(cx) { -dx } else { dx };
        set_x(cx, x);
        let y = pnum(cx, "DI_Y", 0.0);
        set_y(cx, y);
        Step::Finish(false)
    }
}

/// UpdateTargetPosition_BTT_C.
#[derive(Clone, Debug, Default)]
pub struct UpdateTargetPosition;

impl Task for UpdateTargetPosition {
    fn run(&mut self, cx: &mut Cx, _r: Resume) -> Step {
        let e = actor(cx, "EnemyActor");
        if let Some(e) = e
            && valid(cx.w, Some(e))
        {
            let k = cx.key("TargetPoint");
            let v = loc(cx.w, e);
            cx.set(k, BbVal::Vec(v));
        }
        Step::Finish(true)
    }
}

// ------------------------------------------------------------------ evades

/// BotEvadeDown / BotEvadeUp: the evade button stays pressed.
fn evade_vertical(cx: &mut Cx, y: Real) -> Step {
    set_y(cx, y);
    set_x(cx, 0.0);
    press(cx, Btn::Dodge, None);
    Step::Finish(true)
}

/// BotEvadeDown_BTT_C.
#[derive(Clone, Debug, Default)]
pub struct BotEvadeDown;

impl Task for BotEvadeDown {
    fn run(&mut self, cx: &mut Cx, _r: Resume) -> Step {
        evade_vertical(cx, -1.0)
    }
}

/// BotEvadeUp_BTT_C.
#[derive(Clone, Debug, Default)]
pub struct BotEvadeUp;

impl Task for BotEvadeUp {
    fn run(&mut self, cx: &mut Cx, _r: Resume) -> Step {
        evade_vertical(cx, 1.0)
    }
}

/// BotEvadeNeutral_BTT_C.
#[derive(Clone, Debug, Default)]
pub struct BotEvadeNeutral;

impl Task for BotEvadeNeutral {
    fn run(&mut self, cx: &mut Cx, r: Resume) -> Step {
        match r {
            Resume::Execute => {
                let dev = pnum(cx, "DurationRandomDeviation", 0.0);
                set_y(cx, 0.0);
                set_x(cx, 0.0);
                press(cx, Btn::Dodge, None);
                delay(pnum(cx, "Duration", 0.0) + cx.bot.rfir(-dev, dev))
            }
            Resume::Delay => {
                release(cx, Btn::Dodge);
                Step::Finish(true)
            }
        }
    }

    fn abort(&mut self, cx: &mut Cx) {
        zero_stick(cx);
        release(cx, Btn::Dodge);
    }
}

/// BotEvadeForward_BTT_C.
#[derive(Clone, Debug, Default)]
pub struct BotEvadeForward;

impl Task for BotEvadeForward {
    fn run(&mut self, cx: &mut Cx, r: Resume) -> Step {
        match r {
            Resume::Execute => {
                let dev = pnum(cx, "DurationRandomDeviation", 0.0);
                set_y(cx, 0.0);
                let f = facing(cx);
                set_x(cx, f);
                press(cx, Btn::Dodge, None);
                delay(pnum(cx, "Duration", 0.0) + cx.bot.rfir(-dev, dev))
            }
            Resume::Delay => {
                release(cx, Btn::Dodge);
                set_x(cx, 0.0);
                Step::Finish(true)
            }
        }
    }
}

/// BotEvadeToCenter_BTT_C: press and release in one frame; the stick stays set.
#[derive(Clone, Debug, Default)]
pub struct BotEvadeToCenter;

impl Task for BotEvadeToCenter {
    fn run(&mut self, cx: &mut Cx, _r: Resume) -> Step {
        let y = if my_z(cx) < 0.0 { 1.0 } else { 0.0 };
        set_y(cx, y);
        let x = if my_y(cx) > 0.0 { 1.0 } else { -1.0 };
        set_x(cx, x);
        press(cx, Btn::Dodge, None);
        release(cx, Btn::Dodge);
        Step::Finish(true)
    }
}

/// BotEvadeToPoint_BTT_C.
#[derive(Clone, Debug, Default)]
pub struct BotEvadeToPoint;

impl Task for BotEvadeToPoint {
    fn run(&mut self, cx: &mut Cx, r: Resume) -> Step {
        match r {
            Resume::Execute => {
                let y = if my_z(cx) < 0.0 { 1.0 } else { 0.0 };
                set_y(cx, y);
                let x = if my_y(cx) > vec(cx, "TargetPoint").y { 1.0 } else { -1.0 };
                set_x(cx, x);
                press(cx, Btn::Dodge, None);
                delay(0.05)
            }
            Resume::Delay => {
                release(cx, Btn::Dodge);
                Step::Finish(true)
            }
        }
    }

    fn abort(&mut self, cx: &mut Cx) {
        set_x(cx, 0.0);
        release(cx, Btn::Dodge);
    }
}

/// `evadeTarget`: toward -1: to the target, +1: away from it. The stick is the clamped Y distance (analog inside
/// 1 cm).
fn evade_target(cx: &mut Cx, r: Resume, toward: Real, duration: Real) -> Step {
    match r {
        Resume::Execute => {
            set_y(cx, 0.0);
            let off = props::get(cx.props, "Offset")
                .and_then(|o| o.as_obj())
                .and_then(|o| o.get("Y"))
                .and_then(|v| v.as_f64())
                .unwrap_or(0.0);
            let e = actor(cx, "EnemyActor");
            let d = loc_y(cx.w, e) + crate::math::narrow(off) - my_y(cx);
            set_x(cx, clamp(d, -1.0, 1.0) * toward);
            press(cx, Btn::Dodge, None);
            delay(duration)
        }
        Resume::Delay => {
            release(cx, Btn::Dodge);
            zero_stick(cx);
            Step::Finish(true)
        }
    }
}

fn evade_target_abort(cx: &mut Cx) {
    set_x(cx, 0.0);
    release(cx, Btn::Dodge);
}

/// BotEvadeToTarget_BTT_C.
#[derive(Clone, Debug, Default)]
pub struct BotEvadeToTarget;

impl Task for BotEvadeToTarget {
    fn run(&mut self, cx: &mut Cx, r: Resume) -> Step {
        let d = pnum(cx, "Duration", 0.0);
        evade_target(cx, r, -1.0, d)
    }

    fn abort(&mut self, cx: &mut Cx) {
        evade_target_abort(cx)
    }
}

/// BotEvadeAwayFromTarget_BTT_C.
#[derive(Clone, Debug, Default)]
pub struct BotEvadeAwayFromTarget;

impl Task for BotEvadeAwayFromTarget {
    fn run(&mut self, cx: &mut Cx, r: Resume) -> Step {
        evade_target(cx, r, 1.0, 0.2)
    }

    fn abort(&mut self, cx: &mut Cx) {
        evade_target_abort(cx)
    }
}

/// `[-1, 0, 1][i]`.
fn minus_one_to_one(i: Real) -> Real {
    [-1.0, 0.0, 1.0].get(index_of(i)).copied().unwrap_or(Real::NAN)
}

/// BotEvadeRandomDirection_BTT_C: RandomInteger(n) is 0..n-1, so the vertical pick is down or
/// level with AllowUpEvade and always down without it.
#[derive(Clone, Debug, Default)]
pub struct BotEvadeRandomDirection;

impl Task for BotEvadeRandomDirection {
    fn run(&mut self, cx: &mut Cx, _r: Resume) -> Step {
        let a = cx.bot.rand_int(2.0);
        let b = cx.bot.rand_int(1.0);
        let ry = if truthy(cx, "AllowUpEvade") {
            minus_one_to_one(a)
        } else {
            [-1.0, 0.0].get(index_of(b)).copied().unwrap_or(Real::NAN)
        };
        let rx = minus_one_to_one(cx.bot.rand_int(3.0));
        if !truthy(cx, "AllowNeutralEvade") && rx == 0.0 && ry == 0.0 {
            return Step::Finish(false);
        }
        set_y(cx, ry);
        set_x(cx, rx);
        press(cx, Btn::Dodge, None);
        Step::Finish(true)
    }

    fn abort(&mut self, cx: &mut Cx) {
        zero_stick(cx);
        release(cx, Btn::Dodge);
    }
}

/// BotEvadeJumpFlee_BTD_C: runs from the enemy, evades (held), jumps and waits to land; with
/// NeutralAttackInAir a neutral attack 0.25 s into the jump. The Delays: @1596, @1084, @130, @556, @974.
#[derive(Clone, Debug, Default)]
pub struct BotEvadeJumpFlee {
    pc: u8,
    attack_pressed: bool,
    jump_time: Real,
    safety: Real,
}

impl BotEvadeJumpFlee {
    /// The loop of up to 1.5 s for the fighter to leave the ground.
    fn leave_ground(&mut self, cx: &mut Cx) -> Step {
        self.safety = sub(self.safety, 0.05);
        if self.safety <= 0.0 {
            return self.finish(cx);
        }
        if grounded(cx) {
            self.pc = 3;
            return delay_at(0.05, 2);
        }
        self.pc = 4;
        delay_at(0.01, 3)
    }

    /// Waits to land.
    fn land(&mut self, cx: &mut Cx) -> Step {
        if grounded(cx) {
            return self.finish(cx);
        }
        if truthy(cx, "NeutralAttackInAir") && (self.jump_time + 0.25) < cx.bot.now && !self.attack_pressed {
            press(cx, Btn::Attack, Some(Dir::Neutral));
            self.attack_pressed = true;
        }
        self.pc = 4;
        delay_at(0.05, 4)
    }

    fn finish(&mut self, cx: &mut Cx) -> Step {
        release(cx, Btn::Jump);
        if self.attack_pressed {
            release(cx, Btn::Attack);
        }
        release(cx, Btn::Dodge);
        set_x(cx, 0.0);
        Step::Finish(true)
    }
}

impl Task for BotEvadeJumpFlee {
    fn run(&mut self, cx: &mut Cx, r: Resume) -> Step {
        if r == Resume::Execute {
            self.pc = 0;
        }
        match self.pc {
            0 => {
                let Some(enemy) = fighter(actor(cx, "EnemyActor")) else {
                    return Step::Hang; // no FinishExecute
                };
                self.attack_pressed = false;
                let x = if loc_y(cx.w, Some(ActorRef::Fighter(enemy))) < my_y(cx) { -1.0 } else { 1.0 };
                set_x(cx, x);
                self.pc = 1;
                delay(0.05)
            }
            1 => {
                press(cx, Btn::Dodge, None); // EvadeButtonPressed(Forward = 2): the stick decides
                self.pc = 2;
                delay_at(0.1, 1)
            }
            2 => {
                press(cx, Btn::Jump, None);
                self.jump_time = cx.bot.now;
                self.safety = 1.5;
                self.pc = 3;
                delay_at(0.05, 2)
            }
            3 => self.leave_ground(cx),
            _ => self.land(cx),
        }
    }

    fn abort(&mut self, cx: &mut Cx) {
        release(cx, Btn::Dodge);
        set_x(cx, 0.0);
        release(cx, Btn::Jump);
        if self.attack_pressed {
            release(cx, Btn::Attack);
        }
    }
}

// ------------------------------------------------------------------ movement (BotMoveTo: moves.rs)

/// ChooseMovementMode_BTT_C: weighted pick of an Enum_BTMovementMode into the key; one random
/// call.
#[derive(Clone, Debug, Default)]
pub struct ChooseMovementMode;

impl Task for ChooseMovementMode {
    fn run(&mut self, cx: &mut Cx, _r: Resume) -> Step {
        let choices: Vec<(&str, Real)> = props::get(cx.props, "Choices")
            .and_then(|v| v.as_arr())
            .unwrap_or(&[])
            .iter()
            .filter_map(|e| e.as_obj())
            .map(|e| {
                let key = e.get("Key").and_then(|k| k.as_str()).unwrap_or("");
                (key, e.get("Value").and_then(|v| v.as_f64()).map_or(Real::NAN, crate::math::narrow))
            })
            .collect();
        let total = choices.iter().fold(0.0, |s, e| s + e.1);
        let mut r = cx.bot.rfir(0.0, total);
        for (key, value) in choices {
            r -= value;
            if r <= 0.0 {
                // Number(/(\d+)$/.exec(key)?.[1] ?? 0)
                let digits = key.len() - key.trim_end_matches(|c: char| c.is_ascii_digit()).len();
                let n = key[key.len() - digits..].parse::<Real>().unwrap_or(0.0);
                let k = cx.key("Key");
                cx.set(k, BbVal::Num(n));
                break;
            }
        }
        Step::Finish(true)
    }
}

/// SetTargetPointForSuicide_BTT_C: a point far off the side the bot is on.
#[derive(Clone, Debug, Default)]
pub struct SetTargetPointForSuicide;

impl Task for SetTargetPointForSuicide {
    fn run(&mut self, cx: &mut Cx, _r: Resume) -> Step {
        let k = cx.key("TargetPoint");
        let y = if my_y(cx) > 0.0 { 99999.0 } else { -99999.0 };
        cx.set(k, BbVal::Vec(BbVec { x: 0.0, y, z: 99999.0 }));
        Step::Finish(true)
    }
}

/// BotMoveUp_BTT_C: BotMoveUp_BTT / BotMoveDown_BTT ReceiveExecuteAI -> @26: BPI_MoveY(+1 /
/// -1) @105 (BaseFighter @97337: MoveY), FinishExecute(true) @10. No abort event; nothing puts the stick back.
#[derive(Clone, Debug, Default)]
pub struct BotMoveUp;

impl Task for BotMoveUp {
    fn run(&mut self, cx: &mut Cx, _r: Resume) -> Step {
        set_y(cx, 1.0);
        Step::Finish(true)
    }
}

/// BotMoveDown_BTT_C.
#[derive(Clone, Debug, Default)]
pub struct BotMoveDown;

impl Task for BotMoveDown {
    fn run(&mut self, cx: &mut Cx, _r: Resume) -> Step {
        set_y(cx, -1.0);
        Step::Finish(true)
    }
}

/// BotMoveForward_BTT_C (C020_Meeseek only): ReceiveExecuteAI -> @26:
/// MoveX(SelectFloat(-1, 1, BPI_IsFacingLeft)) @105-198 (MoveX(-1) is toward +Y, the facing of FacingLeft: the
/// forward stick), FinishExecute(true) @10. No abort event; nothing puts the stick back.
#[derive(Clone, Debug, Default)]
pub struct BotMoveForward;

impl Task for BotMoveForward {
    fn run(&mut self, cx: &mut Cx, _r: Resume) -> Step {
        let x = if me_left(cx) { -1.0 } else { 1.0 };
        set_x(cx, x);
        Step::Finish(true)
    }
}

/// BotMoveSide_BTT_C.
#[derive(Clone, Debug, Default)]
pub struct BotMoveSide;

impl Task for BotMoveSide {
    fn run(&mut self, cx: &mut Cx, r: Resume) -> Step {
        match r {
            Resume::Execute => {
                let dev = pnum(cx, "DurationDeviation", 0.0);
                set_x(cx, if truthy(cx, "Right") { 1.0 } else { -1.0 });
                delay(pnum(cx, "Duration", 0.0) + cx.bot.rfir(-dev, dev))
            }
            Resume::Delay => {
                set_x(cx, 0.0);
                Step::Finish(true)
            }
        }
    }

    fn abort(&mut self, cx: &mut Cx) {
        set_x(cx, 0.0);
    }
}

/// BotMoveRandomSide_BTT_C (Blueprints/AI; Finn_BagBaitAI, Duration 0.1, DurationDeviation 0 by
/// the class default). ReceiveExecuteAI @79: MoveX(SelectFloat(1, -1, RandomBool)) @221 (RandomBool: FMath::RandBool,
/// one draw; true is MoveX(1)), Delay(Duration + RandomFloatInRange(0 - DurationDeviation, DurationDeviation))
/// (@400 -> @26; the range draws even at 0): MoveX(0) @26, FinishExecute(true) @67. ReceiveAbortAI @459: MoveX(0)
/// @534, FinishAbort @15.
#[derive(Clone, Debug, Default)]
pub struct BotMoveRandomSide;

impl Task for BotMoveRandomSide {
    fn run(&mut self, cx: &mut Cx, r: Resume) -> Step {
        match r {
            Resume::Execute => {
                let dev = pnum(cx, "DurationDeviation", 0.0);
                let x = if cx.bot.rand_int(2.0) == 1.0 { 1.0 } else { -1.0 };
                set_x(cx, x);
                let rf = cx.bot.rfir(0.0 - dev, dev);
                delay(pnum(cx, "Duration", 0.0) + rf)
            }
            Resume::Delay => {
                set_x(cx, 0.0);
                Step::Finish(true)
            }
        }
    }

    fn abort(&mut self, cx: &mut Cx) {
        set_x(cx, 0.0);
    }
}

/// BotMoveToward_BTT_C: one step: true while still further than ApproachToDistance, false on
/// arrival. The key is read as an actor, so the Vector key the trees pass (RecoveryPoint) reads None and the target
/// is the world origin.
#[derive(Clone, Debug, Default)]
pub struct BotMoveToward;

impl Task for BotMoveToward {
    fn run(&mut self, cx: &mut Cx, _r: Resume) -> Step {
        let t = actor(cx, "TargetPointBB");
        if truthy(cx, "AllowQuickFall") {
            let y = if my_z(cx) > loc_z(cx.w, t) { -1.0 } else { 0.0 };
            set_y(cx, y);
        }
        let dy = my_y(cx) - loc_y(cx.w, t);
        let dist = pnum(cx, "ApproachToDistance", 0.0);
        let x = if dy > dist {
            1.0
        } else if dy < -dist {
            -1.0
        } else {
            0.0
        };
        set_x(cx, x);
        Step::Finish(cx.bot.pad.x() != 0.0)
    }

    fn abort(&mut self, cx: &mut Cx) {
        zero_stick(cx);
    }
}

/// BotStop_BTT_C.
#[derive(Clone, Debug, Default)]
pub struct BotStop;

impl Task for BotStop {
    fn run(&mut self, cx: &mut Cx, _r: Resume) -> Step {
        zero_stick(cx);
        Step::Finish(true)
    }
}

/// BotTurnToward_BTT_C: a 0.05 s tap away from the facing when the enemy is behind and at
/// least MinDistanceForTurn away.
#[derive(Clone, Debug, Default)]
pub struct BotTurnToward;

impl Task for BotTurnToward {
    fn run(&mut self, cx: &mut Cx, r: Resume) -> Step {
        if r == Resume::Delay {
            set_x(cx, 0.0);
            return Step::Finish(true);
        }
        let e = actor(cx, "Enemy");
        if !valid(cx.w, e) {
            return Step::Finish(false);
        }
        if (my_y(cx) - loc_y(cx.w, e)).abs() >= pnum(cx, "MinDistanceForTurn", 0.0) && facing_off(cx, e) {
            set_y(cx, 0.0);
            let x = if me_left(cx) { 1.0 } else { -1.0 };
            set_x(cx, x);
            return delay(0.05);
        }
        Step::Finish(true)
    }
}

/// BotTurnAwayFromEnemy_BTT_C (Harley_DashShootAI only, a tree her pawn sets and then
/// replaces). ReceiveExecute -> ubergraph @80: the Enemy key's actor not valid: no FinishExecute (@331, the task
/// hangs until it is aborted; BotTurnToward fails there). |self.Y - enemy.Y| >= MinDistanceForTurn (@563-684; no
/// class default and no node value: 0, so always) and FacingLeft == (self.Y < enemy.Y) (@920-1018: the enemy is in
/// front, an equal Y counts facing right): MoveY(0), MoveX(FacingLeft ? 1 : -1) (@1032-1142, away from the facing),
/// Delay 0.05 (@1187) -> MoveX(0) (@27), success. Anything else: success at once (@15, @1242). No abort event: a
/// stick held when the Delay is cut stays.
#[derive(Clone, Debug, Default)]
pub struct BotTurnAwayFromEnemy;

impl Task for BotTurnAwayFromEnemy {
    fn run(&mut self, cx: &mut Cx, r: Resume) -> Step {
        if r == Resume::Delay {
            set_x(cx, 0.0);
            return Step::Finish(true);
        }
        let e = actor(cx, "Enemy");
        if !valid(cx.w, e) {
            return Step::Hang;
        }
        let in_front = me_left(cx) == (my_y(cx) < loc_y(cx.w, e));
        if sub(my_y(cx), loc_y(cx.w, e)).abs() >= pnum(cx, "MinDistanceForTurn", 0.0) && in_front {
            set_y(cx, 0.0);
            let x = if me_left(cx) { 1.0 } else { -1.0 };
            set_x(cx, x);
            return delay(0.05);
        }
        Step::Finish(true)
    }
}

/// BotTurnAround_BTT_C: the stick is left set.
#[derive(Clone, Debug, Default)]
pub struct BotTurnAround;

impl Task for BotTurnAround {
    fn run(&mut self, cx: &mut Cx, _r: Resume) -> Step {
        set_y(cx, 0.0);
        let x = if me_left(cx) { 1.0 } else { -1.0 };
        set_x(cx, x);
        Step::Finish(true)
    }
}

/// BotFlee_BTT_C.
#[derive(Clone, Debug, Default)]
pub struct BotFlee;

impl Task for BotFlee {
    fn run(&mut self, cx: &mut Cx, _r: Resume) -> Step {
        let v = pnum(cx, "DurationVariance", 0.0);
        let k = cx.key("FleeTime");
        let base = pnum(cx, "Duration", 0.0);
        let rf = cx.bot.rfir(-v, v);
        let t = (base + rf) + cx.bot.now;
        cx.set(k, BbVal::Num(t));
        Step::Finish(true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn random_direction_table() {
        assert_eq!(minus_one_to_one(0.0), -1.0);
        assert_eq!(minus_one_to_one(2.0), 1.0);
        assert!(minus_one_to_one(3.0).is_nan());
    }

    #[test]
    fn directions_of_the_enum() {
        assert_eq!(dir_of(2.0), Dir::Fwd);
        assert_eq!(dir_of(9.0), Dir::BackDown);
    }
}
