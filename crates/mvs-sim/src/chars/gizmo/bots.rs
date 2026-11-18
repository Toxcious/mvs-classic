//! Gizmo's own bot tasks, ported from their bytecode: C023A_ShootArrow_BTT. Every other
//! node his eight trees use is a shared one (`ai/tasks.rs`, `ai/decorators.rs`). The shapes and the rules are those
//! of `chars/batman/bots.rs`; the Delays' resume offsets are the Linkage of each LatentInfo.

use crate::Real;
use crate::actors::ActorRef;
use crate::ai::cx::{Cx, Resume, Step, Task};
use crate::ai::pad::Btn;
use crate::ai::util::{actor, fighter_in_match, loc_y, loc_z};
use crate::input::Dir;
use crate::math::ue::deg_asin;

/// Divide_FloatFloat: 0 for a zero divisor (exe 142f68da0).
fn div(a: Real, b: Real) -> Real {
    if b == 0.0 { 0.0 } else { a / b }
}

/// C023A_ShootArrow_BTT_C, C023A_Arrow's one branch (the bow ready (BotIsAbilityAvailable
/// 1), the enemy in front, Cooldown 2). ReceiveExecuteAI @1334: the pawn cast to BaseFighter (SelfFighter),
/// EnemyFighter = the Enemy key cast to BaseFighter (@1205-1310; fails -> pop_flow: no FinishExecute, the task
/// hangs), BPI_NormalAttackPressed(4 Neutral) @948 (the bow, held: Comp_HoldCharge counts while it is held), Delay
/// 0.5 @987 (-> @15): L_Offset = enemy - self, AngleToTarget = DegAsin(Offset.Z / Offset.Y) (@188-317). > 0 ->
/// MoveY(1) @392 and Delay Angle / 90 * 0.3 @517 (-> @856: MoveY(0)); else < 0 -> MoveY(-1) @624 and Delay Angle /
/// 90 * -0.3 @749 (-> @902: MoveY(0)); then (at once for 0) BPI_NormalAttackReleased @808 and FinishExecute(true)
/// @844. Offset.Y is signed: with the enemy on the -Y side the ratio's sign flips and the stick goes the other way
/// (the build's graph, kept). ReceiveAbortAI @1042: the pawn cast -> BPI_NormalAttackReleased @1117, MoveY(0)
/// @1153, FinishAbort.
#[derive(Clone, Debug, Default)]
pub struct C023AShootArrow {
    pc: u8,
    enemy: Option<ActorRef>,
}

impl Task for C023AShootArrow {
    fn run(&mut self, cx: &mut Cx, r: Resume) -> Step {
        if r == Resume::Execute {
            self.pc = 0;
        }
        match self.pc {
            0 => {
                let a = actor(cx, "Enemy");
                let Some(e) = fighter_in_match(cx.w, a) else { return Step::Hang }; // @1300
                self.enemy = Some(ActorRef::Fighter(e));
                cx.bot.pad.press(Btn::Attack, Some(Dir::Neutral), false);
                self.pc = 1;
                Step::Delay { secs: 0.5, node: 0 } // Delay @987
            }
            1 => {
                let me = Some(ActorRef::Fighter(cx.me));
                let oy = loc_y(cx.w, self.enemy) - loc_y(cx.w, me);
                let oz = loc_z(cx.w, self.enemy) - loc_z(cx.w, me);
                let angle = deg_asin(div(oz, oy));
                if angle > 0.0 {
                    cx.bot.pad.set_y(1.0);
                    self.pc = 2;
                    return Step::Delay { secs: (div(angle, 90.0) * 0.3), node: 1 }; // Delay @517
                }
                if angle < 0.0 {
                    cx.bot.pad.set_y(-1.0);
                    self.pc = 2;
                    return Step::Delay { secs: (div(angle, 90.0) * (-0.3)), node: 2 }; // Delay @749
                }
                self.pc = 3;
                self.run(cx, Resume::Delay)
            }
            2 => {
                cx.bot.pad.set_y(0.0); // @856 / @902
                self.pc = 3;
                self.run(cx, Resume::Delay)
            }
            _ => {
                cx.bot.pad.release(Btn::Attack);
                Step::Finish(true)
            }
        }
    }

    fn abort(&mut self, cx: &mut Cx) {
        cx.bot.pad.release(Btn::Attack);
        cx.bot.pad.set_y(0.0);
    }
}
