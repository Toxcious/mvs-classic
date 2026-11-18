//! Garnet's own bot tasks, ported from their bytecode: GarnetGauntlet_BTT and
//! GarnetSing_BTT. The decorators her trees add are in `ai/decorators.rs`. The shapes and the rules are those of
//! `chars/batman/bots.rs`; the Delays' resume offsets are the Linkage of each LatentInfo.

use crate::actors::ActorRef;
use crate::ai::cx::{Cx, Resume, Step, Task};
use crate::ai::pad::Btn;
use crate::ai::util::{actor, clamp, loc_y, loc_z, valid};
use crate::input::Dir;
use crate::name;

/// GarnetGauntlet_BTT_C, Garnet_GauntletAI ("Enemy Actor" = EnemyActor). ReceiveExecute
/// @1901: the pawn cast to BaseFighter, SpecialAttackPressed(2 Forward, false) @2112 (the side special: her gloves),
/// Delay 0.5 (@2151 -> @15). @15-726: the first Comp_Spawner.SpawnedActors entry whose GetObjectClass is exactly
/// GarnetGauntletsProjectile_C (@487-554) -> Projectile (a member of the node: it keeps the last execution's value
/// when the list holds none), break. @727: Projectile and the Enemy Actor key both valid: BPI_MoveX(FClamp(Projectile
/// Y - enemy Y, -1, 1)) @1112, BPI_MoveY(-FClamp(Projectile Z - enemy Z, -1, 1)) @1547 (the stick the gloves' tick
/// steers by), Delay 0.05 (@1593 -> @1896 -> @727). Either invalid: BPI_MoveX(0), BPI_MoveY(0), FinishExecute(true).
/// The special stays held: only the abort releases it. ReceiveAbortAI @2206: SpecialAttackReleased, FinishAbort (the
/// stick stays). GetObjectClass is the class exactly: the Air child (GarnetGauntletsProjectileAir_C) is not it. A
/// destroyed glove is no longer valid; one that leaves SpawnedActors in its own destroy is passed over as a dead
/// entry.
#[derive(Clone, Debug, Default)]
pub struct GarnetGauntlet {
    pc: u8,
    projectile: Option<ActorRef>,
}

impl Task for GarnetGauntlet {
    fn run(&mut self, cx: &mut Cx, r: Resume) -> Step {
        if r == Resume::Execute {
            self.pc = 0;
        }
        if self.pc == 0 {
            cx.bot.pad.press(Btn::Special, Some(Dir::Fwd), false);
            self.pc = 1;
            return Step::Delay { secs: 0.5, node: 0 }; // Delay @2151
        }
        if self.pc == 1 {
            let found = cx.w.fighter(cx.me).items.spawned_actors.iter().copied().find(|&a| match a {
                ActorRef::Projectile(id) => {
                    cx.w.actors
                        .projectiles
                        .get(id)
                        .is_some_and(|p| !p.dead && p.cls == name!("GarnetGauntletsProjectile_C"))
                }
                _ => false,
            });
            if found.is_some() {
                self.projectile = found;
            }
            self.pc = 2;
        }
        let p = self.projectile;
        let e = actor(cx, "Enemy Actor");
        if valid(cx.w, p) && valid(cx.w, e) {
            cx.bot.pad.set_x(clamp(loc_y(cx.w, p) - loc_y(cx.w, e), -1.0, 1.0));
            cx.bot.pad.set_y(-clamp(loc_z(cx.w, p) - loc_z(cx.w, e), -1.0, 1.0));
            return Step::Delay { secs: 0.05, node: 1 }; // Delay @1593
        }
        cx.bot.pad.set_x(0.0);
        cx.bot.pad.set_y(0.0);
        Step::Finish(true)
    }

    fn abort(&mut self, cx: &mut Cx) {
        cx.bot.pad.release(Btn::Special);
    }
}

/// GarnetSing_BTT_C, Garnet_SingAI (behind GarnetIsReadyToSing_BTD, which is always
/// false: the tree never reaches it). ReceiveExecuteAI @454: the pawn cast to BaseFighter, SpecialAttackPressed(4
/// Neutral, false) @548 (the neutral special, her song), Delay 1 (@587 -> @15). @15 RandomBool (FMath::RandBool, one
/// draw): true -> BPI_MoveX(1) @128, Delay 0.2 (@170 -> @273), BPI_MoveX(0) @352, Delay 0.2 (@394 -> @449 -> @225);
/// false -> @225. @225: SpecialAttackReleased, FinishExecute(true) @261. ReceiveAbortAI @642: SpecialAttackReleased,
/// FinishAbort (the stick stays).
#[derive(Clone, Debug, Default)]
pub struct GarnetSing {
    pc: u8,
}

impl Task for GarnetSing {
    fn run(&mut self, cx: &mut Cx, r: Resume) -> Step {
        if r == Resume::Execute {
            self.pc = 0;
        }
        match self.pc {
            0 => {
                cx.bot.pad.press(Btn::Special, Some(Dir::Neutral), false);
                self.pc = 1;
                Step::Delay { secs: 1.0, node: 0 } // Delay @587
            }
            1 => {
                if cx.bot.rand_int(2.0) == 1.0 {
                    cx.bot.pad.set_x(1.0);
                    self.pc = 2;
                    return Step::Delay { secs: 0.2, node: 1 }; // Delay @170
                }
                self.pc = 3;
                self.run(cx, Resume::Delay)
            }
            2 => {
                cx.bot.pad.set_x(0.0);
                self.pc = 3;
                Step::Delay { secs: 0.2, node: 2 } // Delay @394
            }
            _ => {
                cx.bot.pad.release(Btn::Special);
                Step::Finish(true)
            }
        }
    }

    fn abort(&mut self, cx: &mut Cx) {
        cx.bot.pad.release(Btn::Special);
    }
}
