//! Superman's own bot tasks (Panda_Main/Blueprints/AI/CharacterSubtrees/
//! BL_Superman), ported from their bytecode: SupermanFlyToPoint_BTT, SupermanFlyGrab_BTT, SupermanSuperpunch_BTT. The
//! tasks press his buttons and read the fighter as any bot task does: the moves themselves are flight.rs and
//! aim_punch.rs. World Y / Z against the sim's x = -Y, y = Z; BPI_MoveX(v) is `pad.set_x(v)`;
//! CharacterMovement.IsWalking is `grounded`; a counted-down float is kept in single precision. The Delays' resume
//! offsets are the Linkage of each LatentInfo (from the bytecode).
//! An abort drops the task's pending Delay before ReceiveAbortAI (UBTTask_BlueprintBase::AbortTask 14322bc40 runs
//! AbortLatentActions 143208560 first), and a task executed again in the call that aborted it does not get the Delay
//! node it waited in when that is the first one it reaches (`Cx::stale`, the runtime's). For SupermanFlyToPoint that
//! node is its loop's Delay @642: started again in the air and outside the 200 it waits with the special held until
//! the next abort.

use crate::Real;
use crate::actors::ActorRef;
use crate::ai::blackboard::BbVec;
use crate::ai::cx::{Cx, Resume, Step, Task};
use crate::ai::pad::Btn;
use crate::ai::util::{INVALID, actor, clamp, fighter, loc_y, loc_z, sub, valid, vec};
use crate::fighter::{FighterId, facing, grounded};
use crate::input::Dir;
use crate::name;

/// Vector_DistanceSquared of two points of the play plane (X 0 on both sides), in single precision.
fn dist2(dy: Real, dz: Real) -> Real {
    (dy * dy) + (dz * dz)
}

/// SupermanFlyToPoint_BTT_C (SupermanRecovery_AI, TargetLocation = RecoveryPoint).
/// ReceiveExecute @1378: TargetPoint = the TargetLocation key's vector (@1269-1307, read once),
/// SpecialAttackPressed(2 = Forward, false) (@1334; MoveX is never written: the special goes the way he faces and the
/// direction is the call's own), then the loop @15: IsWalking -> SpecialAttackReleased, FinishExecute(true)
/// (@93-129). Else DistanceSquared(TargetPoint, self) > 40000 (@237; 200 units): dz = TargetPoint.Z - self.Z; > 50:
/// BPI_MoveY(1) (@600); else < -50: BPI_MoveY(-1) (@1072); between the two the stick stays as it is; Delay 0.05
/// (@642 -> @15). Inside the 200: Delay 2.0 (@697 -> @752 -> @93): release and FinishExecute(true) with no other test
/// in those 2 s. MoveY is never put back, at the end or at the abort. Abort @1119: SpecialAttackReleased,
/// FinishAbort.
/// An unset key (FLT_MAX): the squares overflow to infinity, dz > 50: he flies up until he walks or is aborted.
#[derive(Clone, Debug, Default)]
pub struct SupermanFlyToPoint {
    pc: u8,
    tp: Option<BbVec>,
}

impl Task for SupermanFlyToPoint {
    #[allow(
        clippy::neg_cmp_op_on_partial_ord,
        reason = "the graph's `!(d > 40000)` keeps a NaN distance on the inside branch"
    )]
    fn run(&mut self, cx: &mut Cx, r: Resume) -> Step {
        if r == Resume::Execute {
            self.pc = 0;
        }
        let me = cx.me;
        if self.pc == 0 {
            self.tp = Some(vec(cx, "TargetLocation"));
            cx.bot.pad.press(Btn::Special, Some(Dir::Fwd), false);
            self.pc = 1;
        }
        if self.pc == 1 && !grounded(cx.w, me) {
            let tp = self.tp.unwrap_or(INVALID);
            let at = Some(ActorRef::Fighter(me));
            let dz = sub(tp.z, loc_z(cx.w, at));
            let d2 = dist2(sub(tp.y, loc_y(cx.w, at)), dz) + (tp.x * tp.x);
            if !(d2 > 40000.0) {
                self.pc = 2;
                return Step::Delay { secs: 2.0, node: 1 }; // Delay @697
            }
            if dz > 50.0 {
                cx.bot.pad.set_y(1.0);
            } else if dz < -50.0 {
                cx.bot.pad.set_y(-1.0);
            }
            return Step::Delay { secs: 0.05, node: 0 }; // Delay @642
        }
        cx.bot.pad.release(Btn::Special);
        Step::Finish(true)
    }

    fn abort(&mut self, cx: &mut Cx) {
        cx.bot.pad.release(Btn::Special);
    }
}

/// SupermanFlyGrab_BTT_C (Superman_FlyGrabAI, "Enemy Actor" = EnemyActor).
/// ReceiveExecute @1843: EnemyFighter = the key's actor cast to Base_Fighter, and a failed cast ends the event with
/// no FinishExecute (@1645: the task waits for an abort); BPI_MoveX(SelectFloat(-1, 1, FacingLeft)) (@1674-1743: -1
/// facing left, which is the stick forward); Delay 0.05 (@1788 -> @1362): SpecialAttackPressed(2 = Forward, false),
/// then the loop @15: IsWalking -> SpecialAttackReleased, MoveX(0), FinishExecute(true) (@93-170). Else
/// DistanceSquared(enemy, self) > 40000 (@328): dz = enemy.Z - self.Z; > 50: MoveY(1) (@741); else < -50: MoveY(-1)
/// (@1263); Delay 0.05 (@783 -> @15). Inside the 200: Delay 2.0 (@838 -> @893 -> @93): release, MoveX(0),
/// FinishExecute(true). Abort @1406: SpecialAttackReleased, MoveX(0) (@1310), FinishAbort. MoveY is never put back.
#[derive(Clone, Debug, Default)]
pub struct SupermanFlyGrab {
    pc: u8,
    enemy: Option<FighterId>,
}

impl Task for SupermanFlyGrab {
    #[allow(
        clippy::neg_cmp_op_on_partial_ord,
        reason = "the graph's `!(d > 40000)` keeps a NaN distance on the inside branch"
    )]
    fn run(&mut self, cx: &mut Cx, r: Resume) -> Step {
        if r == Resume::Execute {
            self.pc = 0;
        }
        let me = cx.me;
        if self.pc == 0 {
            let Some(e) = fighter(actor(cx, "Enemy Actor")) else { return Step::Hang };
            self.enemy = Some(e);
            let face = facing(cx.w, me);
            cx.bot.pad.set_x(face);
            self.pc = 1;
            return Step::Delay { secs: 0.05, node: 0 }; // Delay @1788
        }
        if self.pc == 1 {
            cx.bot.pad.press(Btn::Special, Some(Dir::Fwd), false);
            self.pc = 2;
        }
        if self.pc == 2 && !grounded(cx.w, me) {
            let enemy = Some(ActorRef::Fighter(self.enemy.expect("SupermanFlyGrab: the enemy of its execute")));
            let at = Some(ActorRef::Fighter(me));
            let dz = sub(loc_z(cx.w, enemy), loc_z(cx.w, at));
            if !(dist2(sub(loc_y(cx.w, enemy), loc_y(cx.w, at)), dz) > 40000.0) {
                self.pc = 3;
                return Step::Delay { secs: 2.0, node: 2 }; // Delay @838
            }
            if dz > 50.0 {
                cx.bot.pad.set_y(1.0);
            } else if dz < -50.0 {
                cx.bot.pad.set_y(-1.0);
            }
            return Step::Delay { secs: 0.05, node: 1 }; // Delay @783
        }
        cx.bot.pad.release(Btn::Special);
        cx.bot.pad.set_x(0.0);
        Step::Finish(true)
    }

    fn abort(&mut self, cx: &mut Cx) {
        cx.bot.pad.release(Btn::Special);
        cx.bot.pad.set_x(0.0);
    }
}

/// SupermanSuperpunch_BTT_C (Superman_SuperpunchAI, twice). No running tree reaches it:
/// Pawn_SuperMan's BPI_ReadyForAI sets AI.CustomSpecialLogic4 to Superman_SuperpunchAI (Pawn_SuperMan @2994) and, in
/// the next statement, to Superman_LeapAttackAI (@3066), so the leap tree is the injected one (as C017_Cannonball_BTT
/// and Harley's dash shot tree). Ported from the graph all the same.
/// ReceiveExecute @2552: SpecialAttackPressed(2 = Forward, false) (@2763), Delay 0.5 (@2802 -> @225): the first entry
/// of the pawn's Comp_Spawner.SpawnedActors whose class is SupermanAimPunch_C itself (@282-936) becomes Projectile;
/// none leaves the member of the last execution, which IsValid then refuses once it is destroyed. Duration = 0.4
/// (@937). Loop @960, while Projectile is valid (@989), Duration > 0 (@1037), the Enemy Actor key's actor is valid
/// (@1110) and d = marker - enemy has VSizeSquared > Square(200) (@1408): BPI_MoveX(clamp(d.Y, -1, 1)) (@1691: the
/// marker to the enemy's side), BPI_MoveY(SelectFloat(1 - clamp(d.Z, -1, 1), 0, |d.Z| > 250)) (@2037-2248: 2 with
/// the marker more than 250 under the enemy, 0 with it more than 250 above, 0 inside the 250), Delay 0.05 (@2294 ->
/// @2478): Duration -= 0.05 -> @960. At the loop's end: Delay 0.2 (@2349 -> @15): SpecialAttackReleased, MoveX(0),
/// MoveY(0), FinishExecute(true). Abort @2857: SpecialAttackReleased, FinishAbort (the stick stays).
#[derive(Clone, Debug, Default)]
pub struct SupermanSuperpunch {
    pc: u8,
    projectile: Option<ActorRef>,
    duration: Real,
}

impl Task for SupermanSuperpunch {
    fn run(&mut self, cx: &mut Cx, r: Resume) -> Step {
        if r == Resume::Execute {
            self.pc = 0;
        }
        let me = cx.me;
        match self.pc {
            0 => {
                cx.bot.pad.press(Btn::Special, Some(Dir::Fwd), false);
                self.pc = 1;
                return Step::Delay { secs: 0.5, node: 0 }; // Delay @2802
            }
            1 => {
                let class = name!("SupermanAimPunch_C");
                let found = cx.w.fighter(me).items.spawned_actors.iter().copied().find(|&a| {
                    crate::actors::world_actor::actor_cls(cx.w, a) == Some(class)
                        && !crate::actors::world_actor::actor_dead(cx.w, a)
                });
                if found.is_some() {
                    self.projectile = found;
                }
                self.duration = 0.4;
                self.pc = 2;
            }
            3 => {
                self.duration = sub(self.duration, 0.05);
                self.pc = 2;
            }
            _ => {}
        }
        if self.pc == 2 {
            let marker = self.projectile;
            let enemy = actor(cx, "Enemy Actor");
            if valid(cx.w, marker) && self.duration > 0.0 && valid(cx.w, enemy) {
                let dy = sub(loc_y(cx.w, marker), loc_y(cx.w, enemy));
                let dz = sub(loc_z(cx.w, marker), loc_z(cx.w, enemy));
                if dist2(dy, dz) > (200.0 * 200.0) {
                    cx.bot.pad.set_x(clamp(dy, -1.0, 1.0));
                    cx.bot.pad.set_y(if dz.abs() > 250.0 { sub(1.0, clamp(dz, -1.0, 1.0)) } else { 0.0 });
                    self.pc = 3;
                    return Step::Delay { secs: 0.05, node: 1 }; // Delay @2294
                }
            }
            self.pc = 4;
            return Step::Delay { secs: 0.2, node: 2 }; // Delay @2349
        }
        cx.bot.pad.release(Btn::Special);
        cx.bot.pad.set_x(0.0);
        cx.bot.pad.set_y(0.0);
        Step::Finish(true)
    }

    fn abort(&mut self, cx: &mut Cx) {
        cx.bot.pad.release(Btn::Special);
    }
}
