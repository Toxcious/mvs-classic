//! The bots' BehaviorTree controller: a bot's controller ([`BtBot`]) and the Lab's and the
//! match's bot entries. Its tree instance is in `tree.rs`, the runner that runs it (the search, the aux nodes, the
//! task runs and the latent pass) in `runner.rs`: a node's call takes its state out of the tree for the call, so the
//! node sees the whole bot through its [`Cx`](crate::ai::cx::Cx).

use crate::Real;
use crate::ai::blackboard::Blackboard;
use crate::ai::expert::{EXPERT_LEVEL, Expert};
use crate::ai::pad::Pad;
use crate::ai::tree::Tree;
use crate::ai::util::BEHAVIORS;
use crate::consts::DELTA_SECONDS;
use crate::fighter::FighterId;
use crate::input::InputState;
use crate::name;
use crate::names::Name;
use crate::rng::Rng;
use crate::world::World;
use mvs_data::BotTrees;
use std::sync::{Arc, LazyLock};

/// `BotController`: the AIController class a bot runs under.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BotController {
    Base,
    Idle,
    TargetDummy,
    RecoverOnly,
    TutorialRecoverOnly,
}

/// `CONTROLLERS`: a controller class's tree (`None`: Idle_AIController runs none) and whether its ReadyForAI runs
/// (Base_AIController's ReadyForAI: the pawn's BPI_ReadyForAI and its SetDynamicSubtree calls).
/// TargetDummy_AIController's ReceiveBeginPlay / ReceivePossess (ubergraph @10-271) load and run
/// Patrolling_BehaviorTree and call neither the parent's events nor ReadyForAI, and nothing else reaches it there
/// (its callers: @15 from SetDebugDifficulty, @1356 from the parent's BeginPlay load). RecoverOnly_AIController
/// calls BPI_ReadyForAI at its possess (@115). Idle_AIController runs no tree, so its flag is never read.
/// TutorialRecoverOnly_AIController (a Base_AIController child: its ReceivePossess calls the parent's) calls
/// BPI_ReadyForAI when its pawn is a BaseFighter, then RunBehaviorTree(TutorialRecoverOnly_BehaviorTree).
impl BotController {
    pub fn tree(self) -> Option<Name> {
        match self {
            BotController::Base => Some(name!("Base_BehaviorTree")),
            BotController::Idle => None,
            BotController::TargetDummy => Some(name!("Patrolling_BehaviorTree")),
            BotController::RecoverOnly => Some(name!("RecoverOnly_BehaviorTree")),
            BotController::TutorialRecoverOnly => Some(name!("TutorialRecoverOnly_BehaviorTree")),
        }
    }

    pub fn ready_for_ai(self) -> bool {
        self != BotController::TargetDummy
    }
}

/// `LabBot`: an entry of the Lab's bot menu: its id, its EBotBehavior and its controller class.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct LabBot {
    pub id: &'static str,
    pub behavior: &'static str,
    pub controller: BotController,
}

const fn lab(id: &'static str, behavior: &'static str, controller: BotController) -> LabBot {
    LabBot { id, behavior, controller }
}

const REC: BotController = BotController::RecoverOnly;

/// `LAB_BOTS`: the Lab's bot entries, in the menu's order (`fight/veryeasy` .. `fight/hard` at 1..4).
pub const LAB_BOTS: [LabBot; 28] = [
    lab("recovery", "EBotBehavior::RecoveryOnly", REC),
    lab("fight/veryeasy", "EBotBehavior::VeryEasy", BotController::Base),
    lab("fight/easy", "EBotBehavior::Easy", BotController::Base),
    lab("fight/medium", "EBotBehavior::Medium", BotController::Base),
    lab("fight/hard", "EBotBehavior::Hard", BotController::Base),
    lab("idle", "EBotBehavior::Idle", BotController::Idle),
    lab("jumping", "EBotBehavior::RecoveryJumping", REC),
    lab("evade/evadeNeutral", "EBotBehavior::RecoveryEvadeNeutral", REC),
    lab("evade/evadeBack", "EBotBehavior::RecoveryEvadeBack", REC),
    lab("evade/evadeForward", "EBotBehavior::RecoveryEvadeForward", REC),
    lab("evade/evadePostHit", "EBotBehavior::RecoveryEvadeAfterFirstHit", REC),
    lab("normalAttack/neutral", "EBotBehavior::RecoveryAttackNeutral", REC),
    lab("normalAttack/down", "EBotBehavior::RecoveryAttackDown", REC),
    lab("normalAttack/up", "EBotBehavior::RecoveryAttackUp", REC),
    lab("normalAttack/forward", "EBotBehavior::RecoveryAttackForward", REC),
    lab("specialAttack/neutral", "EBotBehavior::RecoverySpecialNeutral", REC),
    lab("specialAttack/down", "EBotBehavior::RecoverySpecialDown", REC),
    lab("specialAttack/up", "EBotBehavior::RecoverySpecialUp", REC),
    lab("specialAttack/forward", "EBotBehavior::RecoverySpecialForward", REC),
    lab("stun_di/forward", "EBotBehavior::RecoveryDIForward", REC),
    lab("stun_di/back", "EBotBehavior::RecoveryDIBack", REC),
    lab("stun_di/up", "EBotBehavior::RecoveryDIUp", REC),
    lab("stun_di/down", "EBotBehavior::RecoveryDIDown", REC),
    lab("evade_di/forward", "EBotBehavior::RecoveryEvadePostHitDIForward", REC),
    lab("evade_di/back", "EBotBehavior::RecoveryEvadePostHitDIBack", REC),
    lab("evade_di/up", "EBotBehavior::RecoveryEvadePostHitDIUp", REC),
    lab("evade_di/down", "EBotBehavior::RecoveryEvadePostHitDIDown", REC),
    lab("patrolling", "EBotBehavior::Patrolling", BotController::TargetDummy),
];

/// MatchPlayerData Min/MaxBotDifficulty, read by BotUpdateDifficulty_BTS.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct LevelRange {
    pub min: Real,
    pub max: Real,
}

/// A bot's AIController: its BrainComponent's tree, its blackboard, its pad (the pawn's
/// input its tasks write) and its random stream.
#[derive(Clone, Debug)]
pub struct BtBot {
    /// The dev pause: the whole controller is frozen, its tasks' Delays with it.
    pub idle: bool,
    /// MatchPlayerData 'Bot Persona Key'. The build's training modes and custom game pass None to 'Set Is Bot'; the
    /// port gives every bot it builds Default (`lab_bot` / `match_bot`). None stays for a bare bot.
    pub persona_key: &'static str,
    /// MatchPlayerData Min/MaxBotDifficulty for BotUpdateDifficulty_BTS: [level, level] for a match bot and a Lab
    /// fight bot; `None` (the Lab's other entries): the service returns at once, as the build's training early-out.
    pub level_range: Option<LevelRange>,
    /// A stock AAIController, not Base_AIController (a minion's: read in the exe): none of Base_AIController's graph
    /// runs (no ReadyForAI, so no SetDynamicSubtree injections), every cast<Base_AIController_C> of a tree node
    /// fails, and there is no PlayerState (no persona, no level range).
    pub stock: bool,
    /// Base_AIController's ReadyForAI ran, so the pawn's BPI_ReadyForAI made its SetDynamicSubtree calls (false:
    /// every RunBehaviorDynamic has no tree and fails). False for a stock controller as well.
    pub ready_for_ai: bool,
    /// GetTimeSeconds: a float32 sum of DeltaSeconds, as the fighter's clock.
    pub now: Real,
    /// World time in frames: a new controller in the same Lab session takes over the old one's.
    pub ticks: i32,
    /// The world time the fighters were spawned at (GetGameTimeSinceCreation counts from it).
    pub fighter_born: Real,
    /// AIController NumericDifficulty (0 VeryEasy .. 3 Hard).
    pub difficulty: Real,
    /// AIController BotDifficulty (EBotBehavior), its full name.
    pub behavior: &'static str,
    pub bb: Blackboard,
    pub pad: Pad,
    rng: Rng,
    /// The bot trees of the match's data.
    pub data: Arc<BotTrees>,
    /// The tree it runs, by name (a `name!` literal: [`crate::names::code_str`]); `None`: a controller that runs
    /// none.
    pub tree: Option<Name>,
    pub(crate) t: Tree,
    /// The Expert level's look-ahead (the port's own, `ai::expert`): the tree runs at Hard and the layer may replace
    /// its input. `None` for every level of the beta.
    pub expert: Option<Box<Expert>>,
    /// The runner tests' flags and counts (`ai/runner_tests.rs`), read and written by their node classes.
    #[cfg(test)]
    pub(crate) test: crate::ai::runner_tests::TestState,
}

static NO_TREES: LazyLock<Arc<BotTrees>> = LazyLock::new(|| Arc::new(BotTrees::default()));

/// A bot of no tree and no blackboard: what a controller holds while its bot thinks (`std::mem::take`).
impl Default for BtBot {
    fn default() -> Self {
        BtBot {
            idle: false,
            persona_key: "None",
            level_range: None,
            stock: false,
            ready_for_ai: true,
            now: 0.0,
            ticks: 0,
            fighter_born: 0.0,
            difficulty: 0.0,
            behavior: BEHAVIORS[0],
            bb: Blackboard::default(),
            pad: Pad::default(),
            rng: Rng::new(0),
            data: NO_TREES.clone(),
            tree: None,
            t: Tree::default(),
            expert: None,
            #[cfg(test)]
            test: crate::ai::runner_tests::TestState::default(),
        }
    }
}

/// `BEHAVIORS[Math.min(3, Math.max(0, Math.round(d)))]`.
fn behavior_of(d: Real) -> &'static str {
    let r = (d + 0.5).floor();
    let i = [0.0, 1.0, 2.0].iter().position(|&x| r <= x).unwrap_or(3);
    BEHAVIORS[i]
}

impl BtBot {
    /// `new BtBot(seed, data, difficulty, tree, behavior)`: `behavior` left out follows the numeric level
    /// (SetDebugDifficulty sets both).
    pub fn new(
        seed: i32,
        data: Arc<BotTrees>,
        difficulty: Real,
        tree: Option<Name>,
        behavior: Option<&'static str>,
    ) -> BtBot {
        let mut bot = BtBot {
            rng: Rng::new(seed),
            bb: Blackboard::new(&data.blackboard),
            behavior: behavior.unwrap_or_else(|| behavior_of(difficulty)),
            difficulty,
            data,
            tree,
            ..BtBot::default()
        };
        bot.reset();
        bot
    }

    /// `makeMinionCtl`'s bot: a stock controller running `tree` at NumericDifficulty 0. Its
    /// first think reads the time of the frame it runs in (`now`: the spawn frame's for a spawn in the step, the
    /// next frame's for one outside it); GetGameTimeSinceCreation counts from the spawn (`fighter_born`).
    pub fn minion(seed: i32, data: &Arc<BotTrees>, tree: Name, now: Real, fighter_born: Real) -> BtBot {
        let mut bot = BtBot::new(seed, data.clone(), 0.0, Some(tree), None);
        bot.stock = true;
        bot.ticks = 1;
        bot.now = now;
        bot.fighter_born = fighter_born;
        bot
    }

    /// `labBot`: a Lab bot. In the build its NumericDifficulty stays the class default 0: BPI_SetDifficultyLevel
    /// stores the enum only, SetDebugDifficulty runs in the editor only and BotUpdateDifficulty leaves training
    /// alone.
    /// Deviation: a fight entry's service runs with Min = Max = its level, as the build's non-custom matches do
    /// (HelperFunctionLibrary BotDifficultyRangeFromEnum's range); the recovery, evade, idle and patrolling entries
    /// keep no range and stay at 0, as the build has them.
    /// Deviation: every bot, Lab and match, carries the persona key Default: the build's custom game and Lab pass a
    /// literal None to MatchPlayerData 'Set Is Bot' (online the backend chose the key, which is not in the build);
    /// Default is PersonaDataTable's fallback row.
    pub fn lab_bot(seed: i32, data: &Arc<BotTrees>, b: &LabBot) -> BtBot {
        let c = b.controller;
        let mut bot = BtBot::new(seed, data.clone(), 0.0, c.tree(), Some(b.behavior));
        bot.persona_key = "Default";
        bot.ready_for_ai = c.ready_for_ai();
        if b.id.starts_with("fight/")
            && let Some(level) = BEHAVIORS.iter().position(|&x| x == b.behavior)
        {
            let level = crate::real(u8::try_from(level).expect("four levels"));
            bot.set_level_range(level, level);
        }
        bot
    }

    /// `matchBot`: a match bot (PandaGameMode SpawnAndInitPlayer -> GetOrCreateAIController): Base_AIController
    /// with the lobby's EBotBehavior (level 0 VeryEasy .. 3 Hard), a Lab fight entry's controller. The build leaves
    /// NumericDifficulty 0 in a custom game (BotUpdateDifficulty returns at once in context 3); the port gives it
    /// Min = Max = level, as `lab_bot` does. One per bot fighter, kept over respawns.
    /// Level 4 and up is Expert, the port's own level ([`EXPERT_LEVEL`]): the Hard bot with the look-ahead layer.
    pub fn match_bot(seed: i32, data: &Arc<BotTrees>, level: i32) -> BtBot {
        let i = usize::try_from(level.clamp(0, 3)).expect("a level in 0..3");
        let mut bot = BtBot::lab_bot(seed, data, &LAB_BOTS[1 + i]);
        if level >= EXPERT_LEVEL {
            bot.expert = Some(Box::new(Expert::new(seed)));
        }
        bot
    }

    /// An Expert bot's decisions run on `workers` (`ai::expert::Workers`; `None`: inline); nothing for another level.
    pub fn set_workers(&mut self, workers: Option<&Arc<crate::ai::expert::Workers>>) {
        if let Some(e) = &mut self.expert {
            e.set_workers(workers.cloned());
        }
    }

    /// `controllerBot`: a bot of a controller class a game mode spawns itself (the tutorial's bags: TutorialActor
    /// SpawnPunchingBag, its demo controllers' Idle_AIController), after BPI_SetDifficultyLevel(2): BotDifficulty
    /// VeryEasy, NumericDifficulty the class default 0. Base_AIController is the Lab's fight/veryeasy entry (Min =
    /// Max = VeryEasy, as `lab_bot`'s deviation); the others keep no range, as the Lab's other entries.
    pub fn controller_bot(seed: i32, data: &Arc<BotTrees>, c: BotController) -> BtBot {
        let b = match c {
            BotController::Base => LAB_BOTS[1],
            _ => lab("mode", "EBotBehavior::VeryEasy", c),
        };
        BtBot::lab_bot(seed, data, &b)
    }

    /// `setLevelRange`: the service's range (floats in MatchPlayerData, compared with EqualEqual_FloatFloat @801).
    pub fn set_level_range(&mut self, min: Real, max: Real) {
        self.level_range = Some(LevelRange { min, max });
    }

    /// `reset`: the tree from its root, the blackboard's starting values and an idle pad.
    pub fn reset(&mut self) {
        self.t.reset(&self.data, &self.bb, self.tree);
        self.bb.reset();
        self.pad.reset();
        if let Some(e) = &mut self.expert {
            e.respawned();
        }
    }

    /// RandomFloat.
    pub fn rand(&mut self) -> Real {
        self.rng.next()
    }

    /// RandomFloatInRange (142f7fd80): RandomFloat x (Max - Min) + Min in float. The exe's RandomFloat can be 1.0
    /// (rand() & 0x7fff times 1 / 32767), so Max is reachable there as here.
    pub fn rfir(&mut self, a: Real, b: Real) -> Real {
        self.rng.next() * (b - a) + a
    }

    /// RandomInteger(n): 0..n-1.
    pub fn rand_int(&mut self, n: Real) -> Real {
        self.rng.int(n)
    }

    /// RandomIntegerInRange: a..b inclusive.
    pub fn rand_int_range(&mut self, a: Real, b: Real) -> Real {
        a + self.rng.int(b - a + 1.0)
    }

    /// `activeTask`: the node name (its export name) of the active task, for the HUD and tests.
    pub fn active_task(&self) -> Option<&str> {
        self.t.run.as_ref().map(|r| &*self.t.nodes[r.rn].s.id)
    }

    /// `respawned`: 'Spawn All Players' with this controller kept (the Lab's reset). Native: the pawn's destruction
    /// unpossesses (AController::PawnPendingDestroy; the controller has a PlayerState, so it lives on),
    /// bStopAILogicOnUnposses is false, so the tree and the blackboard go on, and RunBehaviorTree on the running
    /// tree does not restart it. The keys that held the old pawns and projectiles read null until they are written
    /// again; AAIController::SetPawn writes SelfActor (`think` does it), and that write is a change the observers
    /// hear of.
    pub fn respawned(&mut self) {
        self.fighter_born = if self.ticks != 0 { self.now + DELTA_SECONDS } else { 0.0 }; // the next think's clock
        self.bb.actors_destroyed();
        if let Some(e) = &mut self.expert {
            e.respawned();
        }
    }

    /// `think(me, world)`: the controller's tick (the BTComponent's): the pawn's input of its next frame. The app
    /// calls it for a player bot before the frame's step; a minion's runs in the step (`world::pawns`). An Expert
    /// bot's tree thinks as Hard's and its look-ahead may give another input (`ai::expert`).
    pub fn think(&mut self, w: &mut World, me: FighterId) -> InputState {
        let Some(mut e) = self.expert.take() else { return self.think_tree(w, me) };
        let i = e.think(self, w, me);
        self.expert = Some(e);
        i
    }

    /// The tree's think alone (`think` without the Expert layer).
    pub(crate) fn think_tree(&mut self, w: &mut World, me: FighterId) -> InputState {
        crate::ai::log::line(|| {
            let slot = |l: &[FighterId], c: char| l.iter().position(|&f| f == me).map(|i| format!("{c}{i}"));
            let who = slot(&w.players, 'p').or_else(|| w.pawns.as_deref().and_then(|l| slot(l, 'm')));
            format!("think {}", who.as_deref().unwrap_or("?"))
        });
        self.now = if self.ticks != 0 { self.now + DELTA_SECONDS } else { 0.0 };
        self.ticks += 1;
        // No latent pass here either: idle freezes the whole controller, its tasks' Delays with it, and a controller
        // without a tree has no task.
        if self.idle {
            self.pad.reset();
            return self.pad.out();
        }
        // No tree (Idle_AIController): no task writes the pad, so it holds what a graph's calls on the pawn put there.
        if self.tree.is_none() {
            return self.pad.out();
        }
        self.run_tree(w, me);
        self.pad.out()
    }
}
