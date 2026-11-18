//! Jake's own bot tasks, ported from their bytecode: JakeStretchPunch_BTT and
//! JakeSkateboard_BTT. His decorator BTD_JakeHasEatenEnemy is in `ai/decorators.rs`. The shapes and the rules are
//! those of `chars/batman/bots.rs`; the Delays' resume offsets are the Linkage of each LatentInfo.

use crate::Real;
use crate::actors::ActorRef;
use crate::ai::cx::{Cx, Resume, Step, Task};
use crate::ai::pad::Btn;
use crate::ai::util::{actor, loc_y, loc_z};
use crate::fighter::grounded;
use crate::input::Dir;
use crate::math::ue::{normal, round_to_int};
use crate::math::{max, min, real};

/// JakeStretchPunch_BTT_C, Jake_StretchPunchAI and Jake_AboveOnPlatformAI (after
/// BotShortJump; aborted (Self) once he is on the ground). ReceiveExecute @1836: the owner cast to
/// Base_AIController (a stock controller: the cast fails, pop_flow, the task hangs), IsEasy = BotDifficulty == 2
/// (VeryEasy) OR == 3 (Easy) @1911-2055, SelfFighter, NormalAttackPressed(2 Fwd, false) @2210 (the side attack, held:
/// the stretch punch), Delay 0.1 @2249 (-> @1808): HoldTime = 1, then the test @84: HoldTime > 0 -> when NOT IsEasy
/// OR HoldTime > 0.8 (@132-233) the stick at the enemy: d = Normal(self - enemy, 0.0001) (the Enemy Actor key read
/// again each pass), MoveX(Round(d.Y x 2) / 2) @748, MoveY(Round(d.Z x -1 x 2) / 2) @1337 (halves: -1, -0.5, 0, 0.5,
/// 1); Delay 0.1 @1383 (-> @15: HoldTime -= 0.1, the test again). HoldTime <= 0: MoveX(0), MoveY(0),
/// BPI_NormalAttackReleased, FinishExecute(true). An easy bot aims only on the passes with HoldTime > 0.8 (the first
/// two) and holds that stick for the rest. HoldTime is a float variable: kept in single precision.
/// ReceiveAbortAI @2304: NormalAttackReleased @2407, FinishAbort (the stick is left as it is).
#[derive(Clone, Debug, Default)]
pub struct JakeStretchPunch {
    pc: u8,
    easy: bool,
    hold: Real,
}

impl Task for JakeStretchPunch {
    fn run(&mut self, cx: &mut Cx, r: Resume) -> Step {
        if r == Resume::Execute {
            self.pc = 0;
        }
        loop {
            match self.pc {
                0 => {
                    if cx.bot.stock {
                        return Step::Hang; // cast<Base_AIController> fails @1901: no FinishExecute
                    }
                    self.easy = matches!(cx.bot.behavior, "EBotBehavior::VeryEasy" | "EBotBehavior::Easy");
                    cx.bot.pad.press(Btn::Attack, Some(Dir::Fwd), false);
                    self.pc = 1;
                    return Step::Delay { secs: 0.1, node: 0 }; // Delay @2249
                }
                1 => {
                    self.hold = 1.0;
                    self.pc = 2;
                }
                // the test @84
                2 => {
                    let go = self.hold > 0.0;
                    if !go {
                        cx.bot.pad.set_x(0.0);
                        cx.bot.pad.set_y(0.0);
                        cx.bot.pad.release(Btn::Attack);
                        return Step::Finish(true);
                    }
                    if !self.easy || self.hold > 0.8 {
                        let e = actor(cx, "Enemy Actor");
                        let me = Some(ActorRef::Fighter(cx.me));
                        let dy = loc_y(cx.w, me) - loc_y(cx.w, e);
                        let dz = loc_z(cx.w, me) - loc_z(cx.w, e);
                        // Normal(d, 0.0001): GetSafeNormal by InvSqrt
                        let [_, ny, nz] = normal([0.0, dy, dz], 1e-4);
                        cx.bot.pad.set_x(real(round_to_int(ny * 2.0)) / 2.0);
                        cx.bot.pad.set_y(real(round_to_int(-nz * 2.0)) / 2.0);
                    }
                    self.pc = 3;
                    return Step::Delay { secs: 0.1, node: 1 }; // Delay @1383
                }
                _ => {
                    self.hold -= 0.1;
                    self.pc = 2;
                }
            }
        }
    }

    fn abort(&mut self, cx: &mut Cx) {
        cx.bot.pad.release(Btn::Attack);
    }
}

/// JakeSkateboard_BTT_C, Jake_Skateboard3HitAI (airborne, the enemy 400 below within 150 x
/// 350, not easy). ReceiveExecute @2729: the owner cast to AIController (every controller is one), SelfFighter,
/// Enemy = the Enemy Actor key (@2940, read once), then @1262: GetLastUpdateVelocity.Z < 0 AND self Z - enemy Z < 800
/// (@1262-1724) ->
///   MoveX(0) @1817, NormalAttackPressed(1 Down, false) @1859 (the down air: the skateboard, held), then the loop
///   @15: IsWalking -> NormalAttackReleased @93, MoveX(0) @208, FinishExecute(true) @250; else d = self Y - enemy Y:
///   d < -200 MoveX(-1) @627, d > 200 MoveX(1) @1089, else MoveX(0) @1215 (toward the enemy), Delay 0.2 @669 (-> @15).
/// else IsWalking -> MoveX(0) @2060, FinishExecute(true) @2102; else MoveX(FClamp(self Y - enemy Y, -1, 1)) @2478
///   (toward the enemy), Delay 0.05 @2524 (-> @1262).
/// ReceiveAbortAI @2579: NormalAttackReleased @2682, FinishAbort (the stick is left as it is).
// Unverified: GetLastUpdateVelocity is taken as the fighter's velocity of the frame (vy), as the port's stretch punch
// and house do.
#[derive(Clone, Debug, Default)]
pub struct JakeSkateboard {
    pc: u8,
    enemy: Option<ActorRef>,
}

impl Task for JakeSkateboard {
    fn run(&mut self, cx: &mut Cx, r: Resume) -> Step {
        if r == Resume::Execute {
            self.pc = 0;
            self.enemy = actor(cx, "Enemy Actor");
        }
        let me = Some(ActorRef::Fighter(cx.me));
        loop {
            match self.pc {
                // @1262
                0 => {
                    let falling = cx.w.fighter(cx.me).body.vy < 0.0;
                    let near = (loc_z(cx.w, me) - loc_z(cx.w, self.enemy)) < 800.0;
                    if near && falling {
                        self.pc = 1;
                        continue;
                    }
                    if grounded(cx.w, cx.me) {
                        cx.bot.pad.set_x(0.0);
                        return Step::Finish(true);
                    }
                    cx.bot.pad.set_x(min(1.0, max(-1.0, loc_y(cx.w, me) - loc_y(cx.w, self.enemy))));
                    return Step::Delay { secs: 0.05, node: 0 }; // Delay @2524
                }
                1 => {
                    cx.bot.pad.set_x(0.0);
                    cx.bot.pad.press(Btn::Attack, Some(Dir::Down), false);
                    self.pc = 2;
                }
                // @15
                _ => {
                    if grounded(cx.w, cx.me) {
                        cx.bot.pad.release(Btn::Attack);
                        cx.bot.pad.set_x(0.0);
                        return Step::Finish(true);
                    }
                    let d = loc_y(cx.w, me) - loc_y(cx.w, self.enemy);
                    cx.bot.pad.set_x(if d < -200.0 {
                        -1.0
                    } else if d > 200.0 {
                        1.0
                    } else {
                        0.0
                    });
                    return Step::Delay { secs: 0.2, node: 1 }; // Delay @669
                }
            }
        }
    }

    fn abort(&mut self, cx: &mut Cx) {
        cx.bot.pad.release(Btn::Attack);
    }
}
