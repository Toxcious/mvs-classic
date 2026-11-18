//! Batman's own bot tasks, ported from their bytecode: BatmanThrowBatarang_BTT,
//! BatmanHookshot_BTT, BatmanHookshotForward_BTT. The shapes and the rules are `ai/tasks.rs`'s: a graph is a struct
//! with a program counter `pc` (back to 0 at each execution), `Step::Delay` the Blueprint Delay, `Finish` the
//! FinishExecute, `Hang` the end of a path with none; world Y / Z against the sim's x = -Y, y = Z; BPI_MoveX(v) is
//! `set_x(v)`, SelectFloat(-1, 1, FacingLeft) the stick forward (`facing_dir`). The Delays' resume offsets are the
//! Linkage of each LatentInfo (from the bytecode).

use crate::Real;
use crate::actors::ActorRef;
use crate::actors::item::ItemExt;
use crate::ai::cx::{Cx, Resume, Step, Task};
use crate::ai::pad::Btn;
use crate::ai::props;
use crate::ai::util::{actor, clamp, fighter, loc_z, sub, valid};
use crate::input::Dir;

/// The live Item_BatarangV2's CanControl; false for any other actor.
fn can_control(cx: &Cx, a: Option<ActorRef>) -> bool {
    let Some(ActorRef::Item(id)) = a else { return false };
    cx.w.actors.items.get(id).is_some_and(|it| matches!(&it.ext, ItemExt::Batarang(b) if b.can_control))
}

/// BatmanThrowBatarang_BTT_C, Batman_BatarangAI ("Enemy Actor" = EnemyActor).
/// ReceiveExecute @2451: SelfFighter, MoveX(0), MoveY(0), BPI_NormalAttackPressed(4 = Neutral) @2903 (the batarang),
/// Delay 0.5 (@2942 -> @2307): BPI_NormalAttackReleased, RemainingTime = 2.0, then the search @2060: while
/// RemainingTime > 0 and Projectile (a member: the last execution's value stays) is not valid, Delay 0.05 (@2204 ->
/// @1371), RemainingTime -= 0.05 (float), and the first GetAllActorsOfClass(Item_BatarangV2_C) entry whose GetOwner()
/// is SelfFighter becomes Projectile. The steering loop @15: Projectile valid and CanControl and the Enemy Actor
/// key's actor valid, else @1030; dZ = Projectile.Z - Enemy.Z; |dZ| > 150: MoveX(0), MoveY(-FClamp(dZ, -1, 1)); else
/// MoveX(0), MoveY(0); Delay 0.05 (@975 -> @15). @1030: MoveX(0), MoveY(0), FinishExecute(true). Abort @2997:
/// BPI_NormalAttackReleased, FinishAbort; the stick is not put back.
// Unverified: GetAllActorsOfClass takes child classes too (the build has none) in the world's item order, not read
// in the build; GetOwner is Item.owner (the thrower, then a grabber).
#[derive(Clone, Debug, Default)]
pub struct BatmanThrowBatarang {
    pc: u8,
    remaining: Real,
    projectile: Option<ActorRef>,
}

impl Task for BatmanThrowBatarang {
    fn run(&mut self, cx: &mut Cx, r: Resume) -> Step {
        if r == Resume::Execute {
            self.pc = 0;
        }
        loop {
            match self.pc {
                0 => {
                    cx.bot.pad.set_x(0.0);
                    cx.bot.pad.set_y(0.0);
                    cx.bot.pad.press(Btn::Attack, Some(Dir::Neutral), false);
                    self.pc = 1;
                    return Step::Delay { secs: 0.5, node: 0 }; // Delay @2942
                }
                1 => {
                    cx.bot.pad.release(Btn::Attack);
                    self.remaining = 2.0;
                    self.pc = 2;
                }
                // the search's test
                2 => {
                    if self.remaining > 0.0 && !valid(cx.w, self.projectile) {
                        self.pc = 3;
                        return Step::Delay { secs: 0.05, node: 1 }; // Delay @2204
                    }
                    self.pc = 4;
                }
                3 => {
                    self.remaining = sub(self.remaining, 0.05);
                    if let Some(b) = Self::find_batarang(cx) {
                        self.projectile = Some(b);
                    }
                    self.pc = 2;
                }
                4 => self.pc = if valid(cx.w, self.projectile) { 5 } else { 6 },
                // the steering loop's test
                5 => {
                    let b = self.projectile;
                    let e = actor(cx, "Enemy Actor");
                    if !valid(cx.w, b) || !can_control(cx, b) || !valid(cx.w, e) {
                        self.pc = 6;
                        continue;
                    }
                    let dz = sub(loc_z(cx.w, b), loc_z(cx.w, e));
                    cx.bot.pad.set_x(0.0);
                    cx.bot.pad.set_y(if dz.abs() > 150.0 { -clamp(dz, -1.0, 1.0) } else { 0.0 });
                    return Step::Delay { secs: 0.05, node: 2 }; // Delay @975
                }
                _ => {
                    cx.bot.pad.set_x(0.0);
                    cx.bot.pad.set_y(0.0);
                    return Step::Finish(true);
                }
            }
        }
    }

    fn abort(&mut self, cx: &mut Cx) {
        cx.bot.pad.release(Btn::Attack);
    }
}

impl BatmanThrowBatarang {
    /// The first live Item_BatarangV2 the pawn owns.
    fn find_batarang(cx: &Cx) -> Option<ActorRef> {
        cx.w.actors
            .items
            .iter()
            .find(|(_, it)| matches!(it.ext, ItemExt::Batarang(_)) && !it.dead && it.owner == Some(cx.me))
            .map(|(id, _)| ActorRef::Item(id))
    }
}

/// BatmanHookshot_BTT_C, Batman_HookshotAI (twice: "Enemy Actor" = AllyActor, =
/// EnemyActor). ReceiveExecute @1372: SelfFighter, BPI_MoveX(SelectFloat(-1, 1, FacingLeft)) @1652 (the stick
/// forward), Delay 0.02 (@15 -> @70): the key's actor cast to Base_Fighter, a failed cast ends the event with no
/// FinishExecute (@165: the task waits, stick forward, until it is aborted); EnemyFighter = it;
/// SpecialAttackPressed(2 = Forward, false) @194 (the grapple); Delay 0.2 (@233 -> @288): d = Self.Z -
/// EnemyFighter.Z; > 50: MoveY(-1) @653; else < -50: MoveY(1) @1115; between: no MoveY call; Delay 0.2 (@695 ->
/// @1162): SpecialAttackReleased, MoveY(0), MoveX(0), FinishExecute(true). Abort @1702: SpecialAttackReleased,
/// MoveX(0), FinishAbort; MoveY stays.
#[derive(Clone, Debug, Default)]
pub struct BatmanHookshot {
    pc: u8,
    enemy: Option<ActorRef>,
}

impl Task for BatmanHookshot {
    fn run(&mut self, cx: &mut Cx, r: Resume) -> Step {
        if r == Resume::Execute {
            self.pc = 0;
        }
        match self.pc {
            0 => {
                let facing = cx.w.fighter(cx.me).body.facing_dir;
                cx.bot.pad.set_x(facing);
                self.pc = 1;
                Step::Delay { secs: 0.02, node: 0 } // Delay @15
            }
            1 => {
                let Some(e) = fighter(actor(cx, "Enemy Actor")) else { return Step::Hang }; // @165
                self.enemy = Some(ActorRef::Fighter(e));
                cx.bot.pad.press(Btn::Special, Some(Dir::Fwd), false);
                self.pc = 2;
                Step::Delay { secs: 0.2, node: 1 } // Delay @233
            }
            2 => {
                let d = sub(loc_z(cx.w, Some(ActorRef::Fighter(cx.me))), loc_z(cx.w, self.enemy));
                if d > 50.0 {
                    cx.bot.pad.set_y(-1.0);
                } else if d < -50.0 {
                    cx.bot.pad.set_y(1.0);
                }
                self.pc = 3;
                Step::Delay { secs: 0.2, node: 2 } // Delay @695
            }
            _ => {
                cx.bot.pad.release(Btn::Special);
                cx.bot.pad.set_y(0.0);
                cx.bot.pad.set_x(0.0);
                Step::Finish(true)
            }
        }
    }

    fn abort(&mut self, cx: &mut Cx) {
        cx.bot.pad.release(Btn::Special);
        cx.bot.pad.set_x(0.0);
    }
}

/// BatmanHookshotForward_BTT_C, Batman_RecoveryAI (three nodes: straight, Up, Down).
/// ReceiveExecute @779: SelfFighter, MoveX(the stick forward) @1059, Delay 0.02 (@1104 -> @685):
/// SpecialAttackPressed(2, false), Delay 0.2 (@724 -> @15): Up -> MoveY(1), else Down -> MoveY(-1), else MoveY(0);
/// Delay HoldTime (class default 0.4, no node sets it; @150 -> @475): SpecialAttackReleased, MoveY(0), MoveX(0),
/// FinishExecute(true). Abort @1159: SpecialAttackReleased, MoveX(0), FinishAbort; MoveY stays. Nothing of the
/// grapple is read: the release fires it as a player's would.
#[derive(Clone, Debug, Default)]
pub struct BatmanHookshotForward {
    pc: u8,
}

impl Task for BatmanHookshotForward {
    fn run(&mut self, cx: &mut Cx, r: Resume) -> Step {
        if r == Resume::Execute {
            self.pc = 0;
        }
        match self.pc {
            0 => {
                let facing = cx.w.fighter(cx.me).body.facing_dir;
                cx.bot.pad.set_x(facing);
                self.pc = 1;
                Step::Delay { secs: 0.02, node: 0 } // Delay @1104
            }
            1 => {
                cx.bot.pad.press(Btn::Special, Some(Dir::Fwd), false);
                self.pc = 2;
                Step::Delay { secs: 0.2, node: 1 } // Delay @724
            }
            2 => {
                let y = if props::truthy(cx.props, "Up") {
                    1.0
                } else if props::truthy(cx.props, "Down") {
                    -1.0
                } else {
                    0.0
                };
                cx.bot.pad.set_y(y);
                self.pc = 3;
                Step::Delay { secs: props::num(cx.props, "HoldTime").unwrap_or(0.0), node: 2 } // Delay @150
            }
            _ => {
                cx.bot.pad.release(Btn::Special);
                cx.bot.pad.set_y(0.0);
                cx.bot.pad.set_x(0.0);
                Step::Finish(true)
            }
        }
    }

    fn abort(&mut self, cx: &mut Cx) {
        cx.bot.pad.release(Btn::Special);
        cx.bot.pad.set_x(0.0);
    }
}
