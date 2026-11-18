//! Velma's own bot tasks (Panda_Main/Characters/Velma/AI), ported from their
//! bytecode: VelmaBeam_BTT and Velma_CarryBTT. Her decorator Velma_IsCarryRunning_BTD and the police's tree nodes are
//! in `ai/`; the police minion itself is police.rs. World Z is the sim's y; MoveY(v) is `pad.set_y(v)`. The Delays'
//! resume offsets are the Linkage of each LatentInfo.
//! A task's variables live as long as the node instance (UE keeps a Blueprint task's instance for the tree's life),
//! so a variable the graph never sets again carries over to the next execution.

use crate::Real;
use crate::actors::world_actor::{actor_cls, actor_dead};
use crate::actors::{ActorId, ActorRef};
use crate::ai::cx::{Cx, Resume, Step, Task};
use crate::ai::pad::Btn;
use crate::ai::props;
use crate::ai::util::{actor, fighter_in_match, loc_z};
use crate::fighter::FighterId;
use crate::fighter::common::actor_is_a;
use crate::fighter::moves::attach_parent_of;
use crate::input::Dir;
use crate::name;
use crate::world::World;

use super::beam::last_hit;

/// Comp_Spawner.Get Spawned Actor(VelmaHealBeam_C, AllObjects false, LastObject true, FirstObject false): an array of
/// one, the last entry of SpawnedActors that IsA VelmaHealBeam (FilterArray; None when there is none, Array_Get past
/// the end). A destroyed actor has left the list (Comp_Spawner at its EndPlay).
fn last_heal_beam(w: &World, me: FighterId) -> Option<ActorId> {
    let heal = name!("VelmaHealBeam_C");
    w.fighter(me).items.spawned_actors.iter().rev().find_map(|&a| {
        let ActorRef::Actor(id) = a else { return None };
        let cls = actor_cls(w, a)?;
        (!actor_dead(w, a) && actor_is_a(w, ActorRef::Fighter(me), cls, heal).unwrap_or(false)).then_some(id)
    })
}

/// VelmaBeam_BTT_C (Velma_BeamAI's two branches, Enemy Actor = AllyActor / EnemyActor, and
/// Velma_ProjectileFromVialAI's second, each after BotTurnToward). ReceiveExecute @1765: EnemyFighter = the Enemy
/// Actor key cast to BaseFighter (@1984-2093; fails -> FinishExecute(false) @15), SpecialAttackPressed(4 Neutral,
/// false) @2112 (the heal beam, held), Delay 0.2 @2151 (-> @27): LightBeam = Get Spawned Actor's entry cast to
/// VelmaHealBeam (@27-347; None when the cast fails, @1094), then the aim, once: self Z - enemy Z > 50 -> MoveY(-1)
/// @731 (down), else < -50 -> MoveY(1) @1475 (up), else no stick (@366-1517). HoldTime = 1.5 @773; while HoldTime > 0
/// (@796): LightBeam.LastHitActor == EnemyFighter -> MoveY(0) @997 (on target: the stick back to centre; a None
/// LightBeam reads None, never the enemy), Delay 0.1 @1039 (-> @1691: HoldTime -= 0.1, float). Then
/// SpecialAttackReleased @1522, MoveY(0) @1637, FinishExecute(true) @1679.
/// ReceiveAbortAI @2206: SpecialAttackReleased @2309, FinishAbort (the stick is left).
/// LightBeam is read once, after the 0.2 s. @844 reads LastHitActor through ProcessContextOpcode, which skips the read
/// on a PendingKill object: a beam destroyed later reads None.
#[derive(Clone, Debug, Default)]
pub struct VelmaBeam {
    pc: u8,
    enemy: Option<FighterId>,
    beam: Option<ActorId>,
    hold: Real,
}

impl Task for VelmaBeam {
    fn run(&mut self, cx: &mut Cx, r: Resume) -> Step {
        if r == Resume::Execute {
            self.pc = 0;
        }
        let me = cx.me;
        match self.pc {
            0 => {
                let Some(e) = fighter_in_match(cx.w, actor(cx, "Enemy Actor")) else { return Step::Finish(false) }; // @15
                self.enemy = Some(e);
                cx.bot.pad.press(Btn::Special, Some(Dir::Neutral), false);
                self.pc = 1;
                return Step::Delay { secs: 0.2, node: 0 }; // Delay @2151
            }
            1 => {
                let enemy = self.enemy.expect("VelmaBeam: the enemy of its execute");
                self.beam = last_heal_beam(cx.w, me);
                let dz = loc_z(cx.w, Some(ActorRef::Fighter(me))) - loc_z(cx.w, Some(ActorRef::Fighter(enemy)));
                if dz > 50.0 {
                    cx.bot.pad.set_y(-1.0);
                } else if dz < -50.0 {
                    cx.bot.pad.set_y(1.0);
                }
                self.hold = 1.5;
                self.pc = 2;
            }
            3 => {
                self.hold -= 0.1;
                self.pc = 2;
            }
            _ => {}
        }
        if self.hold > 0.0 {
            let hit = self.beam.filter(|&id| !actor_dead(cx.w, ActorRef::Actor(id))).and_then(|id| last_hit(cx.w, id));
            if hit == self.enemy.map(ActorRef::Fighter) {
                cx.bot.pad.set_y(0.0);
            }
            self.pc = 3;
            return Step::Delay { secs: 0.1, node: 1 }; // Delay @1039
        }
        cx.bot.pad.release(Btn::Special);
        cx.bot.pad.set_y(0.0);
        Step::Finish(true)
    }

    fn abort(&mut self, cx: &mut Cx) {
        cx.bot.pad.release(Btn::Special);
    }
}

/// Velma_CarryBTT_C (Velma_CarryAI's ally branch: AllyBB = AllyActor, the ally within the
/// 1500 x 100 box ahead, both on the ground). ReceiveExecuteAI @740: Ally = the AllyBB key (@686),
/// BPI_SpecialAttackPressed(2 Fwd) @643 (the side special: the carry run), then Delay 0.1 @341 (-> @15):
/// WaitDuration -= 0.1 @15; WaitDuration <= 0 OR Ally's GetAttachParentActor == SelfFighter (@84-236: the ally has
/// jumped on) -> BPI_SpecialAttackReleased @250, then Delay 0.1 @286 (-> @396): RunDuration -= 0.1; RunDuration <= 0
/// OR Ally's attach parent != SelfFighter (@465-617) -> FinishExecute(true) @631, else Delay @286 again. Not yet:
/// Delay @341 again.
/// WaitDuration and RunDuration (class defaults 3 and 3) are only ever decremented: the node instance keeps them, so
/// from the second execution on (both at or below 0) the special is released after the first 0.1 s and the task
/// finishes 0.1 s later. Floats: kept in single precision. A None Ally: GetAttachParentActor on None is None, so the
/// first test is the time alone and the second finishes at once.
/// ReceiveAbortAI @839: BPI_SpecialAttackReleased @933, FinishAbort.
#[derive(Clone, Debug, Default)]
pub struct VelmaCarry {
    pc: u8,
    ally: Option<FighterId>,
    wait: Option<Real>,
    run: Option<Real>,
}

impl VelmaCarry {
    /// `parent() === me`.
    fn on_me(&self, cx: &Cx) -> bool {
        self.ally.and_then(|a| attach_parent_of(cx.w, a)) == Some(ActorRef::Fighter(cx.me))
    }
}

impl Task for VelmaCarry {
    fn run(&mut self, cx: &mut Cx, r: Resume) -> Step {
        if r == Resume::Execute {
            self.pc = 0;
        }
        match self.pc {
            0 => {
                self.ally = fighter_in_match(cx.w, actor(cx, "AllyBB"));
                self.wait.get_or_insert_with(|| props::num(cx.props, "WaitDuration").unwrap_or(0.0));
                self.run.get_or_insert_with(|| props::num(cx.props, "RunDuration").unwrap_or(0.0));
                cx.bot.pad.press(Btn::Special, Some(Dir::Fwd), false);
                self.pc = 1;
                Step::Delay { secs: 0.1, node: 0 } // Delay @341
            }
            1 => {
                let wait = self.wait.unwrap_or(0.0) - 0.1;
                self.wait = Some(wait);
                if wait <= 0.0 || self.on_me(cx) {
                    cx.bot.pad.release(Btn::Special);
                    self.pc = 2;
                    return Step::Delay { secs: 0.1, node: 1 }; // Delay @286
                }
                Step::Delay { secs: 0.1, node: 0 } // Delay @341
            }
            _ => {
                let run = self.run.unwrap_or(0.0) - 0.1;
                self.run = Some(run);
                if run <= 0.0 || !self.on_me(cx) {
                    return Step::Finish(true);
                }
                Step::Delay { secs: 0.1, node: 1 } // Delay @286
            }
        }
    }

    fn abort(&mut self, cx: &mut Cx) {
        cx.bot.pad.release(Btn::Special);
    }
}
