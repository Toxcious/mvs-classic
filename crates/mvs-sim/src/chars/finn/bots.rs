//! Finn's own bot tasks, ported from their bytecode: Finn_RushAttack_BTT,
//! Finn_OpenBag_BTT, Finn_BuySomething_BTT and Finn_CloseBag_BTT. The shared BotMoveRandomSide_BTT (his BagBaitAI)
//! is in `ai/tasks.rs`, his two decorators in `ai/decorators.rs`. The shapes and the rules are those of
//! `chars/batman/bots.rs`; the Delays' resume offsets are the Linkage of each LatentInfo.

use crate::Real;
use crate::ai::cx::{Cx, Resume, Step, Task};
use crate::ai::pad::Btn;
use crate::ai::props;
use crate::ai::util::{actor, loc_y, valid};
use crate::consts::DELTA_SECONDS;
use crate::fighter::common::latent_frames;
use crate::input::Dir;
use crate::math::to_int32;

/// Finn_RushAttack_BTT's NormalAttackPressed direction by RandomInteger(4) (@882-1022, the Select of four byte
/// temps): 0 Up, 1 Forward, 2 Neutral, 3 Down.
const RUSH_DIRS: [Dir; 4] = [Dir::Up, Dir::Fwd, Dir::Neutral, Dir::Down];

/// A Delay that counts exactly k latent frames (k >= 1): the float32 countdown of `latent_frames` ends on the k-th
/// pass.
fn frames_secs(k: Real) -> Real {
    (k - 0.5) * DELTA_SECONDS
}

/// Finn_RushAttack_BTT_C, Finn_RushAttackAI (Enemy = EnemyActor, RushToDistance 300 by the
/// class default). ReceiveExecute @625: the pawn cast to BaseFighter, EnemyActor = the Enemy key (@719, read once),
/// MoveX(SelectFloat(-1, 1, FacingLeft)) @837 (the forward stick), NormalAttackPressed(the RandomInteger(4) pick,
/// false) @1022, Timeout = 5 @1156 (never counted down: the test's Timeout > 0 always holds), then the test @15 at
/// once: IsValid(EnemyActor) and |self Y - enemy Y| > RushToDistance, strict (@15-467) -> Delay 0.05 (@481 -> @15);
/// else NormalAttackReleased @536, MoveX(0) @572, FinishExecute(true) @613. ReceiveAbortAI @1184: MoveX(0),
/// NormalAttackReleased, FinishAbort. C019_RushAttack_BTT (`ai/tasks.rs`) is the same graph with a fixed direction.
#[derive(Clone, Debug, Default)]
pub struct FinnRushAttack {
    enemy: Option<crate::actors::ActorRef>,
}

impl FinnRushAttack {
    /// The test @15.
    fn step(&self, cx: &mut Cx) -> Step {
        let me = Some(crate::actors::ActorRef::Fighter(cx.me));
        let far = valid(cx.w, self.enemy)
            && (loc_y(cx.w, me) - loc_y(cx.w, self.enemy)).abs()
                > props::num(cx.props, "RushToDistance").unwrap_or(0.0);
        if far {
            return Step::Delay { secs: 0.05, node: 0 }; // Delay @481
        }
        cx.bot.pad.release(Btn::Attack);
        cx.bot.pad.set_x(0.0);
        Step::Finish(true)
    }
}

impl Task for FinnRushAttack {
    fn run(&mut self, cx: &mut Cx, r: Resume) -> Step {
        if r == Resume::Execute {
            self.enemy = actor(cx, "Enemy");
            let x = if cx.w.fighter(cx.me).body.facing_dir < 0.0 { -1.0 } else { 1.0 };
            cx.bot.pad.set_x(x);
            let i = usize::try_from(to_int32(cx.bot.rand_int(4.0))).expect("RandomInteger(4) is 0..3");
            cx.bot.pad.press(Btn::Attack, Some(RUSH_DIRS[i]), false);
        }
        self.step(cx)
    }

    fn abort(&mut self, cx: &mut Cx) {
        cx.bot.pad.set_x(0.0);
        cx.bot.pad.release(Btn::Attack);
    }
}

/// Finn_OpenBag_BTT_C, Finn_BagShopAI (after BotStop). ReceiveExecute @26: SelfFighter,
/// SpecialAttackPressed(1 Down, false) @124 (the down special: the backpack's shop, held: OpenBag's Loop section),
/// FinishExecute(true) @10. No abort event.
#[derive(Clone, Debug, Default)]
pub struct FinnOpenBag;

impl Task for FinnOpenBag {
    fn run(&mut self, cx: &mut Cx, _r: Resume) -> Step {
        cx.bot.pad.press(Btn::Special, Some(Dir::Down), false);
        Step::Finish(true)
    }
}

/// Finn_BuySomething_BTT_C, Finn_BagShopAI (Option 2 at 10 coins, 1 at 8, the class
/// default 0 at 5). ReceiveExecute @446: SelfFighter, the loop counter = 1, then a ForLoop to Option (@292-344) whose
/// body (@80-222) is MoveX(1) @85 and Delay(RandomFloatInRange(0.1, 0.25)) @164 (-> @26): the body's pop_flow comes
/// straight after the Delay is started, so the loop runs through in the event. Its Delay node is started by the
/// first pass alone (Delay starts nothing while its UUID's action is pending), though every pass draws its
/// RandomFloatInRange. After the loop Delay(RandomFloatInRange(0.2, 0.4)) @387 (-> @68). Both are due from the same
/// call: @26 MoveX(0); @68 FinishExecute(true). Due in one pass they run in start order (a fresh action list), so
/// MoveX(0) first. So the stick is held right (the shop's MoveSelection) for the first draw's time, whatever Option
/// is above 0, and the task ends after the second draw's. The shop's selection and prices are not read here: the
/// tree's Finn_HasTreasure_BTD picks the branch by the wallet. ReceiveAbortAI @568: MoveY(0), MoveX(0),
/// SpecialAttackReleased, FinishAbort.
/// When the second Delay is due first, the task finishes with the first still pending; the port drops it (its
/// MoveX(0) after the finish is not run), as the build does: OnTaskFinished 143242e50 removes the task's latent
/// actions for any result but InProgress. The stale-action rule covers the first Delay only.
#[derive(Clone, Debug, Default)]
pub struct FinnBuySomething {
    pc: u8,
    fa: Real,
    fb: Real,
}

impl Task for FinnBuySomething {
    fn run(&mut self, cx: &mut Cx, r: Resume) -> Step {
        if r == Resume::Execute {
            self.pc = 0;
        }
        match self.pc {
            0 => {
                let n = props::num(cx.props, "Option").unwrap_or(0.0);
                let mut a: Real = -1.0;
                let mut i: Real = 1.0;
                while i <= n {
                    cx.bot.pad.set_x(1.0);
                    let t = cx.bot.rfir(0.1, 0.25);
                    if a < 0.0 {
                        a = t;
                    }
                    i += 1.0;
                }
                let b = cx.bot.rfir(0.2, 0.4);
                self.fa = if a < 0.0 { -1.0 } else { latent_frames(a) };
                self.fb = latent_frames(b);
                if self.fa > 0.0 && self.fa <= self.fb {
                    self.pc = 1;
                    return Step::Delay { secs: a, node: 0 }; // Delay @164
                }
                self.pc = 3;
                Step::Delay { secs: b, node: 1 } // Delay @387
            }
            1 => {
                cx.bot.pad.set_x(0.0);
                if self.fb > self.fa {
                    self.pc = 2;
                    // Delay @387, started with the first
                    return Step::Delay { secs: frames_secs(self.fb - self.fa), node: 1 };
                }
                Step::Finish(true)
            }
            _ => Step::Finish(true),
        }
    }

    fn abort(&mut self, cx: &mut Cx) {
        cx.bot.pad.set_y(0.0);
        cx.bot.pad.set_x(0.0);
        cx.bot.pad.release(Btn::Special);
    }
}

/// Finn_CloseBag_BTT_C, Finn_BagShopAI (last). ReceiveExecute @144: SelfFighter, MoveY(0)
/// @10, MoveX(0) @51, SpecialAttackReleased @92 (the shop buys the selection), FinishExecute(true) @128. No abort
/// event.
#[derive(Clone, Debug, Default)]
pub struct FinnCloseBag;

impl Task for FinnCloseBag {
    fn run(&mut self, cx: &mut Cx, _r: Resume) -> Step {
        cx.bot.pad.set_y(0.0);
        cx.bot.pad.set_x(0.0);
        cx.bot.pad.release(Btn::Special);
        Step::Finish(true)
    }
}
