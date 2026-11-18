//! Tom and Jerry's own bot tasks (Panda_Main/Blueprints/AI/CharacterSubtrees/
//! BL_TomAndJerry), ported from their bytecode: TomAndJerry_GuideRocket_BTT, TomAndJerry_FireSlingshot_BTT and
//! TomAndJerry_TennisBallReturn_BTT. His decorators are in `ai/`; his six subtrees (Pawn_TomAndJerry BPI_ReadyForAI
//! @6619, SetDynamicSubtree @6814-7174) are built with the bot trees' data. Classes of the folder no bot runs:
//! TomAndJerry_PlayTennis_BTT (no tree of the build names it), the tree TomAndJerryRecovery_BTT (built, but nothing
//! injects it: his AI.CustomRecovery stays BaseFighter's DefaultRecovery_AI) and the tree
//! TomAndJerry_DynomateTennis_AI (set with the tag None @7102, which no RunBehaviorDynamic node of the build
//! carries). World Y / Z against the sim's x = -Y, y = Z; MoveX(v) is `pad.set_x(v)`, MoveY(v) `pad.set_y(v)`. The
//! Delays' resume offsets are the Linkage of each LatentInfo.

use crate::Real;
use crate::actors::ActorRef;
use crate::actors::world_actor::{actor_cls, actor_dead};
use crate::ai::cx::{Cx, Resume, Step, Task};
use crate::ai::pad::Btn;
use crate::ai::sweep::trace_sweep;
use crate::ai::util::{actor, clamp, facing_left, fighter, loc_y, loc_z, valid, vel};
use crate::combat::buffs::buff_of;
use crate::input::Dir;
use crate::name;
use crate::registry::props::all_fighters;

/// TomAndJerry_GuideRocket_BTT_C (TomAndJerry_Rocket: Jerry on Tom, the enemy at 100
/// damage or more, the bot over a platform; aborted (Self) once he is not). ReceiveExecute @3132: the pawn
/// (SelfFighter @3324), SpecialAttackPressed(0 Up, false) @3343 (the up special: Jerry's rocket, held: nothing of the
/// task releases it but the abort), Delay 1 @3382 (-> @15).
/// @15-726: over Comp_Spawner.SpawnedActors in order, the first whose class is exactly JerryRocket_C goes into the
/// member Projectile (a member: kept from an earlier execution when none is found). @727: Projectile and the Enemy
/// Actor key both valid -> MoveX(FClamp(rocket Y - enemy Y, -1, 1)) @1112, MoveY(FClamp(rocket Z - enemy Z, -1, 1)
/// x -1) @1547 (the stick, which rocket.rs steers by), Delay 0.05 @1593 (-> @2459 -> @727: the list is not walked
/// again). Else @1648: the Enemy Actor key cast to BaseFighter with Buff_TelekeneticHold (BPI_CheckIfHasBuff @1757)
/// -> RandomIntegerInRange(0, 3) @1925: 0 MoveX 0 / MoveY 1 (@2464), 1 MoveX 0 / MoveY -1 (@2631), 2 MoveX 1 / MoveY
/// 0 (@2798), 3 MoveX -1 / MoveY 0 (@2965); then (with the buff or not) Delay 1.5 @2156 (-> @2211): MoveX(0),
/// MoveY(0), FinishExecute(true) @2373.
/// ReceiveAbortAI @3437: SpecialAttackReleased @3540, FinishAbort (the stick is left as it is).
#[derive(Clone, Debug, Default)]
pub struct TomAndJerryGuideRocket {
    pc: u8,
    projectile: Option<ActorRef>,
}

impl Task for TomAndJerryGuideRocket {
    fn run(&mut self, cx: &mut Cx, r: Resume) -> Step {
        if r == Resume::Execute {
            self.pc = 0;
        }
        let me = cx.me;
        if self.pc == 0 {
            cx.bot.pad.press(Btn::Special, Some(Dir::Up), false);
            self.pc = 1;
            return Step::Delay { secs: 1.0, node: 0 }; // Delay @3382
        }
        if self.pc == 1 {
            let rocket = name!("JerryRocket_C");
            let found =
                cx.w.fighter(me)
                    .items
                    .spawned_actors
                    .iter()
                    .copied()
                    .find(|&a| !actor_dead(cx.w, a) && actor_cls(cx.w, a) == Some(rocket));
            if found.is_some() {
                self.projectile = found;
            }
            self.pc = 2;
        }
        if self.pc == 2 {
            let pr = self.projectile;
            let e = actor(cx, "Enemy Actor");
            if valid(cx.w, pr) && valid(cx.w, e) {
                let x = clamp(loc_y(cx.w, pr) - loc_y(cx.w, e), -1.0, 1.0);
                let y = -clamp(loc_z(cx.w, pr) - loc_z(cx.w, e), -1.0, 1.0);
                cx.bot.pad.set_x(x);
                cx.bot.pad.set_y(y);
                return Step::Delay { secs: 0.05, node: 1 }; // Delay @1593
            }
            if let Some(ef) = fighter(actor(cx, "Enemy Actor"))
                && buff_of(cx.w, ef, name!("Buff_TelekeneticHold_C")).is_some()
            {
                let r = cx.bot.rand_int_range(0.0, 3.0);
                cx.bot.pad.set_x(if r == 2.0 {
                    1.0
                } else if r == 3.0 {
                    -1.0
                } else {
                    0.0
                });
                cx.bot.pad.set_y(if r == 0.0 {
                    1.0
                } else if r == 1.0 {
                    -1.0
                } else {
                    0.0
                });
            }
            self.pc = 3;
            return Step::Delay { secs: 1.5, node: 2 }; // Delay @2156
        }
        cx.bot.pad.set_x(0.0);
        cx.bot.pad.set_y(0.0);
        Step::Finish(true)
    }

    fn abort(&mut self, cx: &mut Cx) {
        cx.bot.pad.release(Btn::Special);
    }
}

/// TomAndJerry_FireSlingshot_BTT_C (TomAndJerry_SlingShot_Tennis, after BotStop:
/// Jerry on Tom, on the ground). ReceiveExecuteAI @244: a LineTraceSingle of TraceTypeQuery4 (3) from his location
/// 1500 forward (FacingLeft: +Y, else -Y) @523: a hit -> FinishExecute(false) @673 (a wall ahead); else one from that
/// point 300 down @928: no hit -> FinishExecute(false) @1172 (no floor ahead); a hit -> SpecialAttackPressed(4
/// Neutral, false) @1078 (the neutral special: the slingshot with Jerry), Delay 0.1 @1117 (-> @239 -> @15), Delay
/// 0.2 @15 (-> @70), SpecialAttackReleased @70, MoveY(0) @185, FinishExecute(true) @227. The traces ignore nothing:
/// the channel's blockers are `ai/sweep.rs`'s. No ReceiveAbortAI: the special stays held when aborted.
#[derive(Clone, Debug, Default)]
pub struct TomAndJerryFireSlingshot {
    pc: u8,
}

impl Task for TomAndJerryFireSlingshot {
    fn run(&mut self, cx: &mut Cx, r: Resume) -> Step {
        if r == Resume::Execute {
            self.pc = 0;
        }
        match self.pc {
            0 => {
                let at = Some(ActorRef::Fighter(cx.me));
                let y = loc_y(cx.w, at);
                let z = loc_z(cx.w, at);
                let fy = y + if facing_left(cx.w, cx.me) { 1500.0 } else { -1500.0 };
                if trace_sweep(cx.w, y, z, fy, z, 0.0, None).is_some() {
                    return Step::Finish(false);
                }
                if trace_sweep(cx.w, fy, z, fy, z - 300.0, 0.0, None).is_none() {
                    return Step::Finish(false);
                }
                cx.bot.pad.press(Btn::Special, Some(Dir::Neutral), false);
                self.pc = 1;
                Step::Delay { secs: 0.1, node: 0 } // Delay @1117
            }
            1 => {
                self.pc = 2;
                Step::Delay { secs: 0.2, node: 1 } // Delay @15
            }
            _ => {
                cx.bot.pad.release(Btn::Special);
                cx.bot.pad.set_y(0.0);
                Step::Finish(true)
            }
        }
    }
}

/// TomAndJerry_TennisBallReturn_BTT_C (TomAndJerry_SlingShot_Tennis, after
/// BotTurnToward the ball: the TennisBall key set by TomAndJerry_TennisBallInPlay within 1000 and in front of him).
/// ReceiveExecuteAI @2117: the member Projectile = the TennisBall key's actor @2241, NormalAttackPressed(4 Neutral,
/// false) @2260 (held), Delay 0.05 @2299 (-> @38). @38: over GetAllActorsOfClass(BaseFighter) (World.all, the
/// minions too) until the break: a fighter not dead (isDead @434) of another Comp_Actor_Destructible TeamIndex
/// (@1310), in front of him (FacingLeft: self Y < its Y, else NOT(self Y < its Y), @1594-1702) and more than 500
/// above him (its Z - self Z, @1948) -> above = true and the break (@15). MoveY(above ? 1 : 0) @518. Duration = 2
/// @563, then @586: the ball's location + its velocity x 0.1 farther than 300 from him (DistanceSquared >
/// Square(300), @828-907) and Duration > 0 -> Delay 0.05 @1031 (-> @2043: Duration -= 0.05, @586 again); else
/// NormalAttackReleased @1086 and FinishExecute(false) @1122 (always false). A None ball reads the zero vector for
/// both. Duration is a float variable: kept in single precision.
/// ReceiveAbortAI @2354: MoveY(0) @2448, NormalAttackReleased @2489, FinishAbort.
#[derive(Clone, Debug, Default)]
pub struct TomAndJerryTennisBallReturn {
    pc: u8,
    ball: Option<ActorRef>,
    dur: Real,
}

impl Task for TomAndJerryTennisBallReturn {
    fn run(&mut self, cx: &mut Cx, r: Resume) -> Step {
        if r == Resume::Execute {
            self.pc = 0;
        }
        let me = cx.me;
        let at = Some(ActorRef::Fighter(me));
        match self.pc {
            0 => {
                self.ball = actor(cx, "TennisBall");
                cx.bot.pad.press(Btn::Attack, Some(Dir::Neutral), false);
                self.pc = 1;
                return Step::Delay { secs: 0.05, node: 0 }; // Delay @2299
            }
            1 => {
                let mut above = false;
                let team = cx.w.fighter(me).team.team;
                let left = facing_left(cx.w, me);
                for t in all_fighters(cx.w) {
                    let x = cx.w.fighter(t);
                    if x.life.dead != 0.0 || x.team.team == team {
                        continue;
                    }
                    let tt = Some(ActorRef::Fighter(t));
                    let less = loc_y(cx.w, at) < loc_y(cx.w, tt);
                    if !(if left { less } else { !less }) {
                        continue;
                    }
                    if (loc_z(cx.w, tt) - loc_z(cx.w, at)) > 500.0 {
                        above = true;
                        break;
                    }
                }
                cx.bot.pad.set_y(if above { 1.0 } else { 0.0 });
                self.dur = 2.0;
                self.pc = 2;
            }
            3 => {
                self.dur -= 0.05;
                self.pc = 2;
            }
            _ => {}
        }
        // @586. A destroyed ball reads the zero vector too: EX_Context on a pending-kill actor (ProcessContextOpcode).
        let b = self.ball.filter(|&b| !actor_dead(cx.w, b));
        let (vy, vz) = vel(cx.w, b);
        let py = loc_y(cx.w, b) + (vy * 0.1);
        let pz = loc_z(cx.w, b) + (vz * 0.1);
        let dy = py - loc_y(cx.w, at);
        let dz = pz - loc_z(cx.w, at);
        let d2 = (dy * dy) + (dz * dz);
        if self.dur > 0.0 && d2 > 90000.0 {
            self.pc = 3;
            return Step::Delay { secs: 0.05, node: 1 }; // Delay @1031
        }
        cx.bot.pad.release(Btn::Attack);
        Step::Finish(false)
    }

    fn abort(&mut self, cx: &mut Cx) {
        cx.bot.pad.set_y(0.0);
        cx.bot.pad.release(Btn::Attack);
    }
}
