//! Steven's own bot tasks and his Watermelon's (Panda_Main/Blueprints/AI/
//! CharacterSubtrees/BL_Steven and BL_WatermelonSteven), ported from their bytecode: StevenUpSpecialRecovery_BTT,
//! Steven_AbortShield_BTT, WatermelonStevenUpdateTarget_BTT, WatermelonStevenEvade_BTT. The decorators his trees add
//! and BotDropThroughPlatform_BTT are in `ai/`. World Y / Z against the sim's x = -Y, y = Z; BPI_MoveX(v) is
//! `pad.set_x(v)`. The Delays' resume offsets are the Linkage of each LatentInfo.

use crate::Real;
use crate::actors::ActorRef;
use crate::ai::cx::{Cx, Resume, Step, Task};
use crate::ai::pad::Btn;
use crate::ai::sweep::trace_sweep;
use crate::ai::util::{actor, fighter, loc_y, loc_z, vec};
use crate::fighter::body::def;
use crate::fighter::{FighterId, facing};
use crate::registry::props::all_fighters;

use super::state::steven_of;

/// StevenUpSpecialRecovery_BTT_C (StevenRecovery_AI, "Target Point" = RecoveryPoint).
/// ReceiveExecute @979: MoveY(1) @1190, MoveX(0) @1231, SpecialAttackPressed(GetInputDirection: Up, false) @1317 (the
/// up special, his shield ride), Delay 0.2 (@1363 -> @15). The loop @15: a SphereTraceSingle of TraceTypeQuery4 from
/// the actor location 20000 down, radius Capsule.CapsuleRadius + 100 (@115-201); BPI_MoveX(SelectFloat(1, -1, self Y >
/// TargetPoint Y)) @690 (an unset key is FLT_MAX: -1); a hit (@736): SpecialAttackReleased @750, BPI_MoveX(0) @865,
/// FinishExecute(true) @907; none: Delay 0.05 (@919 -> @974 -> @15). ReceiveAbortAI @1418: SpecialAttackReleased,
/// BPI_MoveX(0), FinishAbort; the stick's Y stays.
#[derive(Clone, Debug, Default)]
pub struct StevenUpSpecialRecovery {
    pc: u8,
}

impl Task for StevenUpSpecialRecovery {
    fn run(&mut self, cx: &mut Cx, r: Resume) -> Step {
        if r == Resume::Execute {
            self.pc = 0;
        }
        let me = cx.me;
        if self.pc == 0 {
            let face = facing(cx.w, me);
            cx.bot.pad.set_y(1.0);
            cx.bot.pad.set_x(0.0);
            let dir = cx.bot.pad.input_dir(face);
            cx.bot.pad.press(Btn::Special, Some(dir), false);
            self.pc = 1;
            return Step::Delay { secs: 0.2, node: 0 }; // Delay @1363
        }
        let y = loc_y(cx.w, Some(ActorRef::Fighter(me)));
        let z = loc_z(cx.w, Some(ActorRef::Fighter(me)));
        let radius = def(cx.w, me).inner_cap_radius + 100.0;
        let hit = trace_sweep(cx.w, y, z, y, z - 20000.0, crate::math::narrow(radius), None);
        let tp = vec(cx, "TargetPoint").y;
        cx.bot.pad.set_x(if y > tp { 1.0 } else { -1.0 });
        if hit.is_some() {
            cx.bot.pad.release(Btn::Special);
            cx.bot.pad.set_x(0.0);
            return Step::Finish(true);
        }
        Step::Delay { secs: 0.05, node: 1 } // Delay @919
    }

    fn abort(&mut self, cx: &mut Cx) {
        cx.bot.pad.release(Btn::Special);
        cx.bot.pad.set_x(0.0);
    }
}

/// Steven_AbortShield_BTT_C (StevenRecovery_AI, under BotHasAllEvades). ReceiveExecuteAI
/// @26: the pawn cast to Pawn_StevenUniverse (a failure: FinishExecute(false) @204); Comp_StevenShield.Shielding (@105)
/// -> BPI_EvadeButtonReleased @163 (the bubble's hold ends) and FinishExecute(true) @10; not shielding:
/// FinishExecute(false) @220.
#[derive(Clone, Debug, Default)]
pub struct StevenAbortShield;

impl Task for StevenAbortShield {
    fn run(&mut self, cx: &mut Cx, _r: Resume) -> Step {
        if !steven_of(cx.w, cx.me).is_some_and(|s| s.shield.shielding) {
            return Step::Finish(false);
        }
        cx.bot.pad.release(Btn::Dodge);
        Step::Finish(true)
    }
}

/// WatermelonStevenUpdateTarget_BTT_C (WatermelonSteven_BehaviorTree; its EnemyActor and
/// MoveToActor selectors name the keys of those names). ReceiveExecute @3252: MinionCreatorFighter = its
/// MinionCreator when that casts (@3463-3564: the member keeps an earlier value otherwise). With a valid MinionCreator
/// (@3583): LastAllyDamageAmt == 0 takes the creator's CurrentDamage (@3648-3696; nothing else writes it); the
/// creator's CurrentDamage above it (@3767) -> MoveToActor and EnemyActor = the creator's DamageManager
/// MostRecentAttacker's pawn (@3863-4166); else the minion's own MostRecentAttacker, when valid, into both
/// (@4200-4575). Then @3119: EnemyFighter = the EnemyActor key's actor when it casts (a member, kept otherwise),
/// Targets cleared (@3073; TargetScores is never cleared), and over GetAllActorsOfClass(BP_FighterCharacter) @3016
/// (World.all, the minions too) each live fighter (BPI_IsDead @94) of another team (Comp_Actor_Destructible
/// TeamIndex, @171-355; the minion itself fails it) is scored: |dY| < 500: 300, < 1200: 200, < 2500: 100, else 1
/// (@557-1811), + CurrentDamage (@711), + 100 for EnemyFighter (@828-922). @2931: RandomScore =
/// RandomFloatInRange(0, TotalScore), where TotalScore is a member that only the pick writes (@2188: RandomScore -
/// TargetScores[i]); the pick @1816 walks TargetScores: the first i with RandomScore - TargetScores[i] < 0 ->
/// EnemyActor and MoveToActor = Targets[i] (Array_Get out of range: None), break; then FinishExecute(true) @2521.
/// So the first execution rolls in [0, 0], and every later one in [TotalScore, 0] with TotalScore < 0, while every
/// score is >= 1: the pick is always index 0 of TargetScores, the first live enemy of the list (Targets[0]), or None
/// once an execution finds none after an earlier one did. The port keeps TargetScores' first entry alone (`score0`:
/// the walk never reaches a later one) and TotalScore, and draws the roll (the stream goes on as in the build).
/// MostRecentAttacker is a controller (K2_GetPawn): the port's `most_recent_attacker` is the pawn.
#[derive(Clone, Debug, Default)]
pub struct WatermelonStevenUpdateTarget {
    last_ally_damage: Real,
    enemy_fighter: Option<FighterId>,
    score0: Option<Real>,
    total_score: Real,
}

impl Task for WatermelonStevenUpdateTarget {
    fn run(&mut self, cx: &mut Cx, _r: Resume) -> Step {
        let me = cx.me;
        let move_to = cx.key("MoveToActor");
        let enemy_key = cx.key("EnemyActor");
        if let Some(creator) = cx.w.fighter(me).life.minion_creator {
            let dmg = cx.w.fighter(creator).stats.damage;
            if self.last_ally_damage == 0.0 {
                self.last_ally_damage = dmg;
            }
            let a: Option<Option<FighterId>> = if dmg > self.last_ally_damage {
                Some(cx.w.fighter(creator).attackers.most_recent_attacker)
            } else {
                cx.w.fighter(me).attackers.most_recent_attacker.map(Some)
            };
            if let Some(a) = a {
                let a = a.map(ActorRef::Fighter);
                cx.set_obj(move_to, a);
                cx.set_obj(enemy_key, a);
            }
        }
        if let Some(e) = fighter(actor(cx, "EnemyActor")) {
            self.enemy_fighter = Some(e);
        }
        let my_y = loc_y(cx.w, Some(ActorRef::Fighter(me)));
        let my_team = cx.w.fighter(me).team.team;
        let mut first: Option<FighterId> = None;
        for t in all_fighters(cx.w) {
            let x = cx.w.fighter(t);
            if x.life.dead != 0.0 || x.team.team == my_team {
                continue;
            }
            let dy = (my_y - loc_y(cx.w, Some(ActorRef::Fighter(t)))).abs();
            let mut s = if dy < 500.0 {
                300.0
            } else if dy < 1200.0 {
                200.0
            } else if dy < 2500.0 {
                100.0
            } else {
                1.0
            };
            s += x.stats.damage;
            if self.enemy_fighter == Some(t) {
                s += 100.0;
            }
            if first.is_none() {
                first = Some(t);
                self.score0.get_or_insert(s);
            }
        }
        let roll = cx.bot.rfir(0.0, self.total_score); // RandomScore
        if let Some(s0) = self.score0 {
            self.total_score = roll - s0;
            let f = first.map(ActorRef::Fighter);
            cx.set_obj(enemy_key, f);
            cx.set_obj(move_to, f);
        }
        Step::Finish(true)
    }
}

/// WatermelonStevenEvade_BTT_C (WatermelonSteven_BehaviorTree). ReceiveExecute @446:
/// BPI_MoveY(0) @717, BPI_MoveX(SelectInt(-1, 1, RandomBool)) @942 (RandomBool: FMath::RandBool, one draw), Delay 0.1
/// (@988 -> @275): BPI_EvadeButtonPressed @354, Delay 0.1 (@391 -> @26): BPI_EvadeButtonReleased @105, BPI_MoveX(0)
/// @221, FinishExecute(true) @263. ReceiveAbort @1043: BPI_EvadeButtonReleased @1322, BPI_MoveX(0) @1438,
/// FinishAbort.
#[derive(Clone, Debug, Default)]
pub struct WatermelonStevenEvade {
    pc: u8,
}

impl Task for WatermelonStevenEvade {
    fn run(&mut self, cx: &mut Cx, r: Resume) -> Step {
        if r == Resume::Execute {
            self.pc = 0;
        }
        match self.pc {
            0 => {
                cx.bot.pad.set_y(0.0);
                let left = cx.bot.rand_int(2.0) == 1.0;
                cx.bot.pad.set_x(if left { -1.0 } else { 1.0 });
                self.pc = 1;
                Step::Delay { secs: 0.1, node: 0 } // Delay @988
            }
            1 => {
                cx.bot.pad.press(Btn::Dodge, None, false);
                self.pc = 2;
                Step::Delay { secs: 0.1, node: 1 } // Delay @391
            }
            _ => {
                cx.bot.pad.release(Btn::Dodge);
                cx.bot.pad.set_x(0.0);
                Step::Finish(true)
            }
        }
    }

    fn abort(&mut self, cx: &mut Cx) {
        cx.bot.pad.release(Btn::Dodge);
        cx.bot.pad.set_x(0.0);
    }
}
