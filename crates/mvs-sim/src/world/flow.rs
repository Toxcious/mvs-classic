//! The match's flow: the intro, the start, the score, respawns, the end and the game mode's timers
//! (the World's match fields), the set-up's tables,
//! the run-time delegates and the tutorial event bus.

use crate::Real;
use crate::State;
use crate::actors::ActorRef;
use crate::combat::buffs;
use crate::combat::pop_text::{self, GraphPopText};
use crate::events::{Event, PopKind};
use crate::fighter::match_side::TutorialEvent;
use crate::fighter::{FighterId, anim, match_side, most_recent_assist_attacker};
use crate::math;
use crate::names::Name;
use crate::registry::signals::BuffSignal;
use crate::setup::{self as S, MatchSetup, TeamStyle};
use crate::world::{World, blast, mutators, pawns, spawning};
use mvs_data::stage::ItemsData;
use mvs_data::{ArcadeBuffs, MutatorsData};

/// A pending game mode or game state timer or Delay that nothing clears (`timers`): what a closure would hold
/// is the call it makes.
#[derive(State, Copy, PartialEq, Debug)]
pub enum TimerCmd {
    /// The end-of-match warning's next step.
    EndWarning {
        step: i32,
    },
    StartBlastShrink,
    IntroReveal {
        fighter: FighterId,
    },
    IntroMatchStartVo,
    IntroPlay {
        index: i32,
    },
    IntroCameraAll,
    IntroVisibleAll,
    /// `resultVo`'s Delay(MatchCompleteVODelay): the winning team as the call had it.
    VictoryVo {
        winner: i32,
    },
    MatchPointShow {
        team: i32,
    },
    RemoveCameraFocus {
        fighter: FighterId,
    },
    AbductTail {
        fighter: FighterId,
    },
    BonusSurvived {
        fighter: FighterId,
    },
    RingOutAnchorEnd,
}

/// A `timers` entry: due at world tick `at`; `key`, a timer set by name on one object (K2_SetTimer), which a new one
/// of the same key replaces.
#[derive(State, Copy, PartialEq, Debug)]
pub struct Timer {
    pub at: i32,
    pub cmd: TimerCmd,
    pub key: Option<i32>,
}

/// A `bonusTrackers` entry: Buff_FFABonusObjectiveTrackers per fighter. `alive`: the buff actor exists.
#[derive(State, Copy, PartialEq, Eq, Debug, Default)]
pub struct BonusTracker {
    pub alive: bool,
    pub spree: i32,
    pub spree_done: bool,
}

/// `voShared`: PandaGameState's VO speaker count.
#[derive(State, Copy, PartialEq, Eq, Debug, Default)]
pub struct VoShared {
    pub speakers: i32,
}

/// The PandaGameMode / PandaGameState match state.
#[derive(State, Debug, Default)]
pub struct MatchState {
    pub tick: i32,
    /// `introTicks`: steps counted for `setup.flow` (the countdown card's timeline).
    pub intro_ticks: i32,
    /// Lab "Same Attack Penalty" (TrainingConfiguration.bSameAttackPenalty, default true; matches always on).
    pub same_attack_penalty: bool,
    /// TeamKnockouts by team: the score, or under the stock rule the team's lives left.
    pub team_knockouts: Vec<Real>,
    /// PandaGameState Team Damage (FreeForAll only: `update_ffa_team_damage`), float32 sums.
    pub team_damage: Vec<Real>,
    /// PandaGameMode TiedFFATeams.
    pub tied_ffa_teams: Vec<i32>,
    /// The experimental stock rule's lives left, by fighter id.
    pub stocks: Option<Vec<i32>>,
    /// MatchStarted?: the MatchTimer runs.
    pub match_started: bool,
    pub match_over: bool,
    /// WinningTeamIndex.
    pub winning_team: i32,
    pub ko_point_multiplier: Real,
    pub match_start_tick: i32,
    pub match_over_tick: i32,
    /// The live RingOutVfxAnchor actors (`remove_camera_focus`): one per ring-out until its own timer.
    pub step_ring_outs: i32,
    pub timers: Vec<Timer>,
    /// BlastBoxPlayerExit.ShrinkPercent (0..1).
    pub blast_shrink: Real,
    /// The world tick of BPI_ShrinkBlastBox, -1 before it.
    pub blast_shrink_tick: i32,
    pub blast_shrink_time: Option<Real>,
    pub blast_shrink_by: Option<Name>,
    pub bonus_trackers: Vec<BonusTracker>,
    pub vo_shared: VoShared,
    /// The game state's Comp_Actor_VO speaks until this tick (IsCurrentlySpeaking), -1 none.
    pub announcer_until: i32,
    /// Announcer VO Victory's Delay is pending.
    pub victory_vo: bool,
    /// CatchUpSpeedBuffEnabled, its timer and the players' catch-up loops.
    pub catch_up: crate::world::catch_up::CatchUp,
}

/// `WorldMutatorHandler`: WorldMutatorHandler_C's Comp_State_Manager list.
/// Deviation: the port's handler is no actor: a plain holder on the World, made by its reset, with the world
/// mutators' latent actions run in the world's leftover latent pass.
#[derive(State, Debug, Default)]
pub struct WorldMutatorHandler {
    pub buffs: Vec<mutators::WorldMutator>,
}

/// `PawnDelegates`: a pawn's multicast delegates, each the list of its bound calls. Kept on
/// the fighter. Multicasts a sim object binds on another one at run time and
/// unbinds again (the tutorial minigames and demo controllers); the pawn's own classes and buffs answer the same
/// broadcasts through their hooks (CharHooks, BuffSignal).
/// FMulticastScriptDelegate::ProcessMulticastDelegate (14079aa20): the invocation list is copied and called index 0
/// upward (bind order); a handler bound during the broadcast is not called, one unbound during it still is; a stale
/// entry (its object destroyed) is removed from the live list by RemoveAtSwap and not called. AddUnique appends,
/// Remove swaps the last entry into the removed one's slot (4.26).
#[derive(State, Debug)]
pub struct PawnDelegates {
    pub hit_by_enemy: Vec<Bound>,
    pub knocked_back_include_self: Vec<Bound>,
    pub evaded_attack: Vec<Bound>,
    pub exited_arena: Vec<Bound>,
    pub movement_mode_changed: Vec<Bound>,
    pub after_spawned_actor: Vec<Bound>,
    pub charge_end: Vec<Bound>,
    pub ability_cooldown_updated: Vec<Bound>,
    /// AActor OnTakeAnyDamage.
    pub take_any_damage: Vec<Bound>,
    /// `LAST_MODE`: the movement mode the last MovementModeChanged broadcast, -1 none.
    pub last_mode: i32,
}

impl Default for PawnDelegates {
    fn default() -> Self {
        PawnDelegates {
            hit_by_enemy: Vec::new(),
            knocked_back_include_self: Vec::new(),
            evaded_attack: Vec::new(),
            exited_arena: Vec::new(),
            movement_mode_changed: Vec::new(),
            after_spawned_actor: Vec::new(),
            charge_end: Vec::new(),
            ability_cooldown_updated: Vec::new(),
            take_any_damage: Vec::new(),
            last_mode: -1,
        }
    }
}

/// `ActorDelegates`: a spawned actor's delegates:
/// `World::actor_delegates` by the actor, dropped once the actor is gone from its arena (`end_projectiles`, each
/// frame).
#[derive(State, Debug, Default)]
pub struct ActorDelegates {
    pub destroyed: Vec<Bound>,
    pub new_buff_spawned: Vec<Bound>,
    /// AActor OnTakeAnyDamage.
    pub take_any_damage: Vec<Bound>,
    /// AActor OnActorHit (a blocking hit).
    pub actor_hit: Vec<Bound>,
    /// Item_BatarangV2's PickupArea OnComponentBeginOverlap.
    pub pickup_overlap: Vec<Bound>,
}

/// A delegate's bound call: a variant per binding, which come with the ports of the classes that bind (`None` until
/// then: no binding).
#[derive(State, Copy, PartialEq, Eq, Debug, Default)]
pub enum Bound {
    #[default]
    None,
    /// A tutorial object's handler (`modes/tutorial/classes.rs`).
    Tutorial(crate::modes::tutorial::TutorialBind),
}

/// A pawn delegate without arguments, as a broadcast names it.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PawnDelegate {
    /// BaseFighter OnExitedArena (the blast box exit).
    ExitedArena,
    /// Comp_Pawn_ChargeAttack ChargeEnd.
    ChargeEnd,
}

/// A pawn's delegate by name (`PawnDelegates`' fields).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PawnList {
    HitByEnemy,
    KnockedBackIncludeSelf,
    EvadedAttack,
    ExitedArena,
    MovementModeChanged,
    AfterSpawnedActor,
    ChargeEnd,
    AbilityCooldownUpdated,
    TakeAnyDamage,
}

/// An actor's delegate by name (`ActorDelegates`' fields).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ActorList {
    Destroyed,
    NewBuffSpawned,
    TakeAnyDamage,
    ActorHit,
    PickupOverlap,
}

/// One run-time delegate: a pawn's or a spawned actor's.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DelegateList {
    Pawn(FighterId, PawnList),
    Actor(ActorRef, ActorList),
}

/// A broadcast's arguments, a variant per delegate (the delegates' signatures), and the tutorial's own
/// delegates' (`modes::tutorial::classes::RunList`).
#[derive(Clone, Copy, Debug)]
pub enum DelegateCall<'a> {
    /// `HitByEnemyArgs`: the packet, LocalHitInformation.KnockBackDirectionVector in UE axes (X world Y = the sim's
    /// -x, Y world Z; 0, 0 for a hit with none), the DamageDealer fighter and the dealer actor.
    HitByEnemy {
        hit: Option<&'a crate::combat::packet::HitPacket>,
        dir_x: Real,
        dir_y: Real,
        dealer: Option<FighterId>,
        by: Option<ActorRef>,
    },
    /// `KnockedBackArgs`: BaseKnockBack, the direction as a raw Vector2D (X = world Y, Y = world Z), the DamageDealer
    /// and Damage.
    KnockedBackIncludeSelf {
        base_knock_back: Real,
        dir_x: Real,
        dir_y: Real,
        dealer: Option<ActorRef>,
        damage: Real,
    },
    /// EvadedAttack(AttackingActor).
    EvadedAttack {
        by: ActorRef,
    },
    ExitedArena,
    /// MovementModeChangedDelegate: the new mode and the one before (EMovementMode bytes).
    MovementModeChanged {
        mode: i32,
        prev: i32,
    },
    /// AfterSpawnedActor(SpawnedActor).
    AfterSpawnedActor {
        actor: ActorRef,
    },
    ChargeEnd,
    /// AbilityCooldownUpdated(AbilitySlot, Complete).
    AbilityCooldownUpdated {
        slot: i32,
        complete: bool,
    },
    /// OnDestroyed.
    Destroyed,
    /// NewBuffSpawned(NewBuff): the buff's class.
    NewBuffSpawned {
        cls: crate::combat::BuffClass,
    },
    /// Parent_Game_Mode PlayerRespawned(PlayerIndex).
    PlayerRespawned {
        index: i32,
    },
    /// OnTakeAnyDamage(DamagedActor, Damage, DamageType, InstigatedBy, DamageCauser): the damage and the causer, the
    /// actor that owns the hitting hit box (pfgDoDamage's GetOwner; `None` for a level hazard's). InstigatedBy is
    /// None on every path (DamageAndLaunch's ApplyDamage).
    TakeAnyDamage {
        damage: Real,
        causer: Option<ActorRef>,
    },
    /// OnActorHit(SelfActor, OtherActor, NormalImpulse, Hit): a blocking hit on a stage solid (no actor of the port's).
    ActorHit,
    /// OnComponentBeginOverlap of a pickup area: the other actor, and whether the item was Idle when it began.
    PickupOverlap {
        other: ActorRef,
        idle: bool,
    },
    /// TutorialStepComplete, TutorialStepCleanup, TutorialDemoComplete.
    StepComplete,
    StepCleanup,
    DemoComplete,
}

fn pawn_list(d: &mut PawnDelegates, l: PawnList) -> &mut Vec<Bound> {
    match l {
        PawnList::HitByEnemy => &mut d.hit_by_enemy,
        PawnList::KnockedBackIncludeSelf => &mut d.knocked_back_include_self,
        PawnList::EvadedAttack => &mut d.evaded_attack,
        PawnList::ExitedArena => &mut d.exited_arena,
        PawnList::MovementModeChanged => &mut d.movement_mode_changed,
        PawnList::AfterSpawnedActor => &mut d.after_spawned_actor,
        PawnList::ChargeEnd => &mut d.charge_end,
        PawnList::AbilityCooldownUpdated => &mut d.ability_cooldown_updated,
        PawnList::TakeAnyDamage => &mut d.take_any_damage,
    }
}

fn actor_list(d: &mut ActorDelegates, l: ActorList) -> &mut Vec<Bound> {
    match l {
        ActorList::Destroyed => &mut d.destroyed,
        ActorList::NewBuffSpawned => &mut d.new_buff_spawned,
        ActorList::TakeAnyDamage => &mut d.take_any_damage,
        ActorList::ActorHit => &mut d.actor_hit,
        ActorList::PickupOverlap => &mut d.pickup_overlap,
    }
}

/// The live list of a delegate, `None` for an actor nothing has bound.
pub fn delegate_list_mut(w: &mut World, at: DelegateList) -> Option<&mut Vec<Bound>> {
    match at {
        DelegateList::Pawn(f, l) => Some(pawn_list(&mut w.fighter_mut(f).delegates, l)),
        DelegateList::Actor(a, l) => w.actor_delegates.get_mut(&a).map(|d| actor_list(d, l)),
    }
}

/// `pawnDelegates` / `actorDelegates`: the live list of a delegate, an actor's delegates made at the first bind.
pub fn delegate_list_or_new(w: &mut World, at: DelegateList) -> &mut Vec<Bound> {
    match at {
        DelegateList::Pawn(f, l) => pawn_list(&mut w.fighter_mut(f).delegates, l),
        DelegateList::Actor(a, l) => actor_list(w.actor_delegates.entry(a).or_default(), l),
    }
}

/// `Multicast.broadcast`: the copy of the list, called in bind order; a stale entry (its binder destroyed) is
/// swapped out of the live list and not called.
pub fn broadcast_bound(w: &mut World, at: DelegateList, call: &DelegateCall<'_>) {
    let Some(l) = delegate_list_mut(w, at) else { return };
    if l.is_empty() {
        return;
    }
    let src = match at {
        DelegateList::Pawn(f, _) => ActorRef::Fighter(f),
        DelegateList::Actor(a, _) => a,
    };
    for b in l.clone() {
        match b {
            Bound::None => {}
            Bound::Tutorial(t) => {
                if crate::modes::tutorial::classes::stale(w, t.owner) {
                    if let Some(l) = delegate_list_mut(w, at)
                        && let Some(i) = l.iter().position(|&x| x == b)
                    {
                        l.swap_remove(i);
                    }
                    continue;
                }
                crate::modes::tutorial::classes::delegate(w, t, Some(src), call);
            }
        }
    }
}

/// AActor::TakeDamage's broadcast (14280cc90): ApplyDamage (142a71b60) returns on a damage of 0 and the generic
/// TakeDamage again, so a 0 broadcasts nothing; else OnTakeAnyDamage with the causer (ReceiveAnyDamage, first in the
/// build, is the actor's own: the port's damage path runs it).
pub fn broadcast_take_any_damage(w: &mut World, a: ActorRef, damage: Real, causer: Option<ActorRef>) {
    if damage == 0.0 {
        return;
    }
    let at = match a {
        ActorRef::Fighter(f) => DelegateList::Pawn(f, PawnList::TakeAnyDamage),
        _ => DelegateList::Actor(a, ActorList::TakeAnyDamage),
    };
    broadcast_bound(w, at, &DelegateCall::TakeAnyDamage { damage, causer });
}

/// `Multicast.broadcast` of a pawn's argument-free delegate: the copy of the list, called in bind order.
pub fn broadcast_pawn(w: &mut World, f: FighterId, d: PawnDelegate) {
    let (l, call) = match d {
        PawnDelegate::ExitedArena => (PawnList::ExitedArena, DelegateCall::ExitedArena),
        PawnDelegate::ChargeEnd => (PawnList::ChargeEnd, DelegateCall::ChargeEnd),
    };
    broadcast_bound(w, DelegateList::Pawn(f, l), &call);
}

// ---- the World's construction and the step's phases of the match

/// `World.step`'s intro count (`setup.flow`): the camera events, `introSequence`, `startMatch` at
/// `MATCH_START_TICKS`; true when no input is open this frame (`matchInputOpen`): the minions' too.
pub fn intro_tick(w: &mut World) -> bool {
    if !w.setup.flow {
        return false;
    }
    w.flow.intro_ticks += 1;
    let t = w.flow.intro_ticks;
    let at = |n: u32| i64::from(t) == i64::from(n);
    if at(S::INTRO_SEQUENCE_TICKS) {
        w.events.push(Event::CameraMovement { on: false });
        // GetColorIndex per pawn. Deviation: the offline rule (team 0 is 1) for every peer, so the order of the
        // two intro calls is the same on all of them; online the build takes the viewer's own team as 1.
        intro_sequence(w);
    }
    if at(S::INTRO_END_TICKS) {
        w.events.push(Event::CameraMovement { on: true });
    }
    if at(S::MATCH_START_TICKS) {
        start_match(w);
    }
    // No input before EndIntro's HardEnableInput or once CleanupBeforeSwitchMaps has run (`match_input_open`:
    // GameEnd / MatchEndImpl close nothing); the bots' trees run all the same. An abducted loser is dead from its
    // defeat tail on and takes none (Fighter.beginFrame).
    let since = if w.flow.match_over { w.flow.tick - w.flow.match_over_tick } else { -1 };
    !S::match_input_open(t, since)
}

/// `f.onFfaDamage ??= (d) => MatchFlow.updateFFATeamDamage(w, f, d)`.
pub fn bind_ffa_damage(w: &mut World, f: FighterId) {
    let l = &mut w.fighter_mut(f).life;
    if l.on_ffa_damage == match_side::FfaDamage::None {
        l.on_ffa_damage = match_side::FfaDamage::UpdateTeamDamage;
    }
}

/// `MatchFlow.respawn` for a player whose events of the step hold a `respawn` (ahead of the ring-out check, which
/// would otherwise see the KO position again).
pub fn respawn_if_owed(w: &mut World, f: FighterId) {
    if w.fighter(f).events.iter().any(|e| matches!(e, Event::Respawn { .. })) {
        respawn(w, f);
    }
}

/// The leftover pass's world objects: `mode.latent` (the game mode's own objects), `worldMutatorsLatent`,
/// `itemAreaLatent`, `stageLatent` (the stage's actors and level script: its Delays, then its tick).
pub fn latent(w: &mut World) {
    if let Some(l) = w.mode.and_then(|m| m.latent) {
        l(w);
    }
    if w.world_mutators.is_some() {
        mutators::world_mutators_latent(w);
    }
    if w.item_area.is_some() {
        crate::world::items::item_area_latent(w);
    }
    if w.stage_run.is_some() {
        crate::world::stage::stage_latent(w);
    }
}

/// A team index as a list index.
fn ti(team: i32) -> usize {
    usize::try_from(team).expect("a team index")
}

/// A fighter's id as a list index (a player's slot).
fn fi(w: &World, f: FighterId) -> usize {
    usize::try_from(w.fighter(f).id).expect("a player id")
}

/// PandaGameMode Get Recent Attacker Info (@51-320): MostRecentAttacker counts only when it is valid and
/// GetByPlayerState(its PlayerState) succeeds. A minion (World.spawnMinion: a stock controller, no PlayerState)
/// never does, live or destroyed: its ring-out is scored as one without an attacker (team -1). A mode pawn
/// (`pawns`) has a PlayerState only while PlayerController 0 holds it (APawn::PossessedBy 142c794f0 takes the
/// controller's; no demo controller wants one), so only a player-held one counts; a mode World's ring-outs go to its
/// own round end (`blast`), so only the ring-out asset reads it there.
/// Unverified: a pawn PlayerController 0 has left keeps the PlayerState PossessedBy gave it (stock 4.26: UnPossessed
/// does not clear it); taken here as counting only while held.
pub fn player_attacker(w: &World, f: Option<FighterId>) -> Option<FighterId> {
    let f = f?;
    if pawns::slot_of(w, f) < 0 {
        return None;
    }
    let held = matches!(w.pawn_ctl.ctl.get(&f), Some(pawns::PawnCtl::Player));
    (w.fighter(f).life.slot.is_none() || held).then_some(f)
}

/// FFAGameMode CDO RespawnCatchUpBuffs: class -> SpawnBuffActor's Duration, in the map's order.
const FFA_CATCH_UP_BUFFS: [(Name, Real); 3] = [
    (crate::name!("Buff_RespawnArmor_C"), 5.0),
    (crate::name!("Buff_Enlarge_C"), 7.5),
    (crate::name!("Buff_RespawnSpeed_C"), 10.0),
];

/// BPI_SetMatchStarted(true), 4.5 s into IntroSequence: the MatchTimer starts with the match duration, and in
/// FreeForAll the BonusPointsTimer with DoublePointsStartPercent of it.
/// The Countdown timer is set there too, with the duration - 10 s (a time of 0 or less arms nothing): Match End
/// Warning's sound chain. ClearTimersAndEventImpl clears the MatchTimer only, so the chain also runs after a match
/// that is over by then.
pub fn start_match(w: &mut World) {
    w.flow.match_started = true;
    w.flow.match_start_tick = w.flow.tick;
    let d = w.setup.rules.map_or(0.0, |r| r.duration);
    let lead = d - S::END_WARNING_LEAD;
    if lead > 0.0 {
        for i in 0..S::END_WARNING_STEPS {
            let step = i32::try_from(i).expect("a warning step");
            later(w, lead + crate::real(step), TimerCmd::EndWarning { step }, None);
        }
    }
    // FFAGameMode BPI_SetMatchStarted (@1973..2414): ShrinkBlastBoxTimer, the MatchTimer's remaining time x
    // ShrinkBlastBoxStartPercent. Nothing clears it. ShrinkBlastBox (@2415): BPI_ShrinkBlastBox(ShrinkBlastBoxTime)
    // on every blast box, the camera's AdjustCameraForShrinkingBlastBox and the pop text (the view reads
    // blast_shrink / blast_shrink_tick).
    if w.setup.team_style == TeamStyle::FreeForAll && d > 0.0 && w.stage.shrunk.is_some() {
        later(w, d * S::SHRINK_BLAST_BOX_START, TimerCmd::StartBlastShrink, None);
    }
    // @4590-5095: EnableCatchUpSpeedBuff's timer at FMax(Duration - 120, 0), set after the Countdown's.
    // Unverified: TutorialGameMode (type 4) is taken to reach it as it reaches the Countdown; it was not read.
    crate::world::catch_up::arm(w, d);
}

/// `introSequence`: the game mode's IntroSequence, from UI_Countdown's StartCountdown. At T every pawn gets
/// PlayIntroAnimation(1), at T + 1.5 s PlayIntroAnimation(0); a pawn acts on the call whose index is its
/// GetColorIndex (offline 1 for team 0, else 0): KeepOnCamera.Register, the intro montage, and a Delay(0.0) later
/// RemoveInvisibility with the reveal effects. A pawn with another index is unregistered from the camera, so each
/// call frames its own fighters only. At T + 0.6 s PlayMatchStartVO: the fighters shuffled, the first whose
/// matchstart line plays ends the loop. EndIntro at T + 3.0 s registers every pawn and removes any invisibility left
/// 0.65 s later (HardRemoveInvisibility); input opens there and the match starts at T + 4.5 s (both counted by the
/// caller, as the camera's SetCameraMovement at T and T + 3.0 s).
/// The colour index is the team's (team 0 is 1), read at each call: no team changes in a match.
pub fn intro_sequence(w: &mut World) {
    intro_play(w, 1);
    later(w, 0.6, TimerCmd::IntroMatchStartVo, None);
    later(w, 1.5, TimerCmd::IntroPlay { index: 0 }, None);
    later(w, 3.0, TimerCmd::IntroCameraAll, None);
    later(w, 3.65, TimerCmd::IntroVisibleAll, None);
}

/// `introSequence`'s `play(index)`.
fn intro_play(w: &mut World, index: i32) {
    for f in w.players.clone() {
        let color = i32::from(w.fighter(f).team.team == 0);
        let on = color == index;
        w.fighter_mut(f).vis.on_camera = on;
        if !on {
            continue;
        }
        anim::play_intro_montage(w, f);
        let id = w.fighter(f).id;
        w.events.push(Event::IntroFx { fighter: id });
        later(w, 1.0 / 60.0, TimerCmd::IntroReveal { fighter: f }, None);
    }
}

/// `later`: a timer `seconds` from now. A timer's command must only name values that never change (fighters,
/// constants): a rollback restores the timer list and runs the same command again.
pub fn later(w: &mut World, seconds: Real, cmd: TimerCmd, key: Option<i32>) {
    let at = w.flow.tick + blast::js_ticks(seconds);
    if let Some(k) = key
        && let Some(i) = w.flow.timers.iter().position(|t| t.key == Some(k))
    {
        w.flow.timers.remove(i);
    }
    w.flow.timers.push(Timer { at, cmd, key });
}

/// A due timer's call.
fn run_timer(w: &mut World, cmd: TimerCmd) {
    match cmd {
        TimerCmd::EndWarning { step } => {
            for &ev in S::end_warning(usize::try_from(step).expect("a warning step")) {
                w.events.push(Event::Sound { fighter: -1, ev, x: None, y: None, hit: None, pooled: false, key: None });
            }
        }
        TimerCmd::StartBlastShrink => {
            blast::start_blast_shrink(w, S::SHRINK_BLAST_BOX_TIME, S::FFA_SHRINK);
        }
        TimerCmd::IntroReveal { fighter } => {
            w.fighter_mut(fighter).vis.invisible = false;
            let id = w.fighter(fighter).id;
            w.events.push(Event::IntroReveal { fighter: id });
        }
        TimerCmd::IntroMatchStartVo => {
            let mut order = w.players.clone();
            for i in (1..order.len()).rev() {
                let j = w.rng.int(js_len(i + 1));
                order.swap(i, usize_of(j));
            }
            for f in order {
                if match_side::match_start_vo(w, f) {
                    break;
                }
            }
        }
        TimerCmd::IntroPlay { index } => intro_play(w, index),
        TimerCmd::IntroCameraAll => {
            for f in w.players.clone() {
                w.fighter_mut(f).vis.on_camera = true;
            }
        }
        TimerCmd::IntroVisibleAll => {
            for f in w.players.clone() {
                w.fighter_mut(f).vis.invisible = false;
            }
        }
        TimerCmd::VictoryVo { winner } => {
            w.flow.victory_vo = false;
            let standalone = w.setup.local_play;
            for f in w.players.clone() {
                if !w.fighter(f).team.bot_controlled {
                    let won = standalone || w.fighter(f).team.team == winner;
                    announce(w, if won { "announcer_victory" } else { "announcer_defeat" });
                }
            }
        }
        TimerCmd::MatchPointShow { team } => {
            if w.flow.match_over {
                return;
            }
            w.events.push(Event::MatchPointShow { team });
            announce(w, if team == 0 { "announcer_matchpoint_blue" } else { "announcer_matchpoint_red" });
        }
        TimerCmd::RemoveCameraFocus { fighter } => {
            let mut n = w.flow.step_ring_outs;
            for &o in w.pawns.as_deref().unwrap_or(&w.players) {
                let o = w.fighter(o);
                if o.vis.on_camera && !o.capsule.capsule_off {
                    n += 1;
                }
            }
            for (_, m) in w.actors.markers.iter() {
                if !m.dead {
                    n += 2;
                }
            }
            // (the stock rule: a fighter out of lives leaves the camera whatever the count)
            let out = w.flow.stocks.as_ref().is_some_and(|s| s.get(fi(w, fighter)) == Some(&0));
            if n > 2 || out {
                let fr = w.fighter_mut(fighter);
                fr.vis.on_camera = false;
                fr.life.cam_focus = false;
            }
        }
        TimerCmd::AbductTail { fighter: f } => abduct_tail(w, f),
        TimerCmd::BonusSurvived { fighter } => bonus_survived(w, fighter),
        TimerCmd::RingOutAnchorEnd => w.flow.step_ring_outs -= 1,
    }
}

/// `Rng.int`'s bound from a list length.
fn js_len(n: usize) -> Real {
    crate::real(u32::try_from(n).expect("a short list"))
}

/// `Rng.int`'s draw as an index (a whole number below the bound).
fn usize_of(r: Real) -> usize {
    usize::try_from(math::to_int32(r)).expect("a non-negative draw")
}

/// After each bonus score the owner's BPI_PopText (queued on its pawn): Format("+{SurvivalPoints} SURVIVAL!")
/// @648-1222, Format("+{SpreePoints} RINGOUT SPREE!!!") @2196-2774, the argument the class's points (an Int); lifetime
/// 1.0, text colour (1, 0.814345, 0, 1), location (0, 0, 300), space 3, scale 1, centred, no delay, no icons or
/// audio, team colour 0, attached to the owner. Text keys 8F0CC2FB4E111CC752D3679434C276BE (survival) and
/// 242A429B4D79D9F2094B669C2CB64DE1 (spree).
fn bonus_pop(key: Name, arg: Name, points: Real) -> GraphPopText {
    GraphPopText {
        text_key: key,
        source_string: None,
        localized_string: None,
        lifetime: 1.0,
        color: [1.0, 0.814345, 0.0, 1.0],
        location: [0.0, 0.0, 300.0],
        space: 3,
        scale: 1.0,
        justification: 1,
        delay: 0.0,
        audio: None,
        queued: true,
        local_only: false,
        args: vec![(arg, points)],
    }
}

fn survival_pop() -> GraphPopText {
    bonus_pop(
        crate::name!("8F0CC2FB4E111CC752D3679434C276BE"),
        crate::name!("SurvivalPoints"),
        S::BONUS_SURVIVAL_POINTS,
    )
}

fn spree_pop() -> GraphPopText {
    bonus_pop(crate::name!("242A429B4D79D9F2094B669C2CB64DE1"), crate::name!("SpreePoints"), S::BONUS_SPREE_POINTS)
}

/// `bonusKnockedOut`: PandaGameMode OnPlayerRingoutImpl broadcasts PlayerKnockedOut first, before its
/// AlreadyRungOut check and the scores, in FreeForAll to every live Buff_FFABonusObjectiveTrackers
/// (OnPlayerKnockedOut @4528..5994): when the loser's DamageManager.MostRecentAttacker is the buff owner's
/// controller, CurrentKnockOutSpreeCount goes up and from KnockOutsForSpree on OnSpreeAchieved (a DoOnce) scores
/// SpreePoints for the owner's team, without the multiplier. The buff is given at the first spawn only and has the
/// parent's DestroyBuffWhenOwnerDefeated: the owner's first defeat (the ring-out tail after this broadcast, or the
/// abduction) ends its objectives for the match. So OnFighterKnockedOut (count back to 0, the survival timer set
/// again) never reaches a live buff. OnBigDamageAchieved and ResetBonusObjectives have no caller in the build.
pub fn bonus_knocked_out(w: &mut World, loser: FighterId) {
    if let Some(att) = w.fighter(loser).attackers.most_recent_attacker
        && let Ok(i) = usize::try_from(w.fighter(att).id)
        && let Some(tr) = w.flow.bonus_trackers.get_mut(i)
        && tr.alive
    {
        tr.spree += 1;
        if tr.spree >= S::BONUS_SPREE_KNOCKOUTS && !tr.spree_done {
            tr.spree_done = true;
            let team = w.fighter(att).team.team;
            update_team_score(w, team, S::BONUS_SPREE_POINTS);
            pop_text::pop_text_on(w, att, &spree_pop()); // @2774
        }
    }
    let i = fi(w, loser);
    w.flow.bonus_trackers[i].alive = false;
}

/// `bonusSurvived`: OnSurvivalTimerComplete (@133-1222): the score, then the text.
pub fn bonus_survived(w: &mut World, f: FighterId) {
    if !w.flow.bonus_trackers[fi(w, f)].alive {
        return;
    }
    let team = w.fighter(f).team.team;
    update_team_score(w, team, S::BONUS_SURVIVAL_POINTS);
    pop_text::pop_text_on(w, f, &survival_pop()); // @1222
}

/// `gameStateVO`: the call by another class's graph (the tutorial actor's BPI_PlayVO on the game state), by the
/// alias as the data spells it.
pub fn game_state_vo(w: &mut World, alias: &str) {
    announce(w, &alias.to_lowercase());
}

/// Comp_Actor_VO.pfgPlayVO on the game state's own component (BPI_PlayVO; every caller passes AlwaysPlayVO and
/// PlayOnlyLocally). The owner is no pawn: of the gates only the component's IsCurrentlySpeaking is left, so a line
/// is dropped while the announcer speaks. The rows are untagged with one row per alias and one priority. VOLocal
/// posts the event on the game state (2D) and counts a VO speaker until its end-of-event callback, which holds back
/// the fighters' lines without AlwaysPlayVO.
/// The rows are DefaultAnnouncerPack's (DTVO_Announcer) on every machine; the view plays the event of the pack this
/// machine equipped.
// Deviation: the build's hold-back ends with the line played (VODonePlaying), so another pack's line holds back for
// its own length; here it stays the default line's, as it gates the fighters' playVO, whose pick draws from the
// random stream the animation picks share: a length per machine would split the peers online.
fn announce(w: &mut World, alias: &str) {
    if w.flow.announcer_until >= 0 {
        return;
    }
    let Some(row) = w.data.announcer.rows.iter().find(|r| r.0.to_lowercase() == alias && r.1.is_empty()) else {
        return;
    };
    let (ev, len) = (row.3.clone(), row.4.unwrap_or(0.0));
    let ev = w.lit(&ev);
    w.events.push(Event::Sound { fighter: -1, ev, x: None, y: None, hit: None, pooled: false, key: None });
    w.flow.vo_shared.speakers += 1;
    w.flow.announcer_until = w.flow.tick + blast::js_ticks(crate::math::narrow(len));
}

/// `DOUBLE_RINGOUT_S`.
const DOUBLE_RINGOUT_S: Real = 6.0;

/// `trackRingOut`: PandaGameMode TrackRingOutQuestsAndAchievements, from TryUpdateTeamScore @1849 once the killer's
/// PlayerState cast held (the stat trackers' part; the quests and achievements are not ported). A killer who is not
/// a bot gets TotalRingouts @8878 and, when IsValid(the loser's MostRecentAssistAttacker) @8925 (a minion's too: no
/// PlayerState cast there), TotalAssists @9099, both to the killer's account; TotalDoubleRingouts @3802 (not for a
/// bot @3674) when |GetGameTimeInSeconds - LastRingOutTime| < 6.0 @3374, bot or not, and LastRingOutTime = now @3879
/// after it. The sim counts for every fighter; the bot tests are the server's (it knows the bots), as is
/// TotalRingouts (the RingOuts the report carries already).
fn track_ring_out(w: &mut World, killer: FighterId, loser: FighterId) {
    let assisted = most_recent_assist_attacker(w, loser).is_some();
    let k = w.fighter_mut(killer);
    if assisted {
        k.stats.ring_outs_assisted += 1;
    }
    let now = k.clock.time; // GetGameTimeInSeconds: the world clock
    if (now - k.stats.last_ring_out_time).abs() < DOUBLE_RINGOUT_S {
        k.stats.double_ring_outs += 1;
    }
    k.stats.last_ring_out_time = now;
}

/// `mostRecentAssistAttacker`: the assist attacker while its window runs.
/// `startStocks`: the experimental stock rule (`setup.rules.stocks`; the port's own, not in the beta), at the
/// match's reset. Every fighter starts with knockoutsToWin lives, and its team's TeamKnockouts holds the team's
/// lives left, so what reads the score as a lead reads lives the same way: the time-up winner (`winners`: the most
/// lives, then the fewest falls or the lowest damage), FreeForAll's catch-up buffs at a respawn (teams with more
/// lives) and a bot's score-based difficulty. A ring-out takes a life (`stock_ring_out`); the point rules are off.
/// A fighter out of lives stays out, and the last team with lives wins (`stock_kill`).
pub fn start_stocks(w: &mut World) {
    let Some(r) = w.setup.rules.filter(|r| r.stocks) else { return };
    let n = math::max(r.knockouts_to_win, 1.0);
    w.flow.stocks = Some(vec![math::to_int32(n); w.players.len()]);
    for f in w.players.clone() {
        let t = ti(w.fighter(f).team.team);
        w.flow.team_knockouts[t] += n;
    }
}

/// `stockRingOut`: a ring-out under the stock rule: the attacker's and the assist's counts as in `score_ring_out`,
/// a life off the loser (the `stocks` event: the HUD's lives badge).
fn stock_ring_out(w: &mut World, loser: FighterId, att: Option<FighterId>, assist: Option<FighterId>) {
    let li = fi(w, loser);
    if w.flow.match_over || w.flow.stocks.as_ref().is_some_and(|s| s[li] <= 0) {
        return;
    }
    if let Some(a) = att {
        let off = w.fighter(loser).attackers.last_attacker_off_ledge;
        let af = w.fighter_mut(a);
        af.stats.ring_outs += 1;
        if off {
            af.stats.off_platform_ring_outs += 1;
        }
        if let Some(s) = assist {
            w.fighter_mut(s).stats.assists += 1;
        }
        track_ring_out(w, a, loser);
    }
    w.fighter_mut(loser).stats.kos += 1;
    let stocks = w.flow.stocks.as_mut().expect("the stock rule");
    stocks[li] -= 1;
    let left = stocks[li];
    let t = ti(w.fighter(loser).team.team);
    w.flow.team_knockouts[t] -= 1.0;
    let id = w.fighter(loser).id;
    w.events.push(Event::Stocks { fighter: id, stocks: left });
}

/// The teams with a fighter that has lives left, in first-seen fighter order (`[...new Set(...)]`).
fn teams_alive(w: &World) -> Vec<i32> {
    let stocks = w.flow.stocks.as_ref().expect("the stock rule");
    let mut out = Vec::new();
    for &f in &w.players {
        let fr = w.fighter(f);
        if stocks[usize::try_from(fr.id).expect("a player id")] > 0 && !out.contains(&fr.team.team) {
            out.push(fr.team.team);
        }
    }
    out
}

/// `stockKill`: KillPlayerFighter's win check under the stock rule: one team left with lives (or none) ends the
/// match; a team down to its last life puts the other on match point (two-team styles). Returns whether the
/// fighter respawns.
fn stock_kill(w: &mut World, loser: FighterId) -> bool {
    let alive = teams_alive(w);
    if alive.len() <= 1 {
        let winner = alive.first().copied().or_else(|| winners(w).first().copied()).unwrap_or(NO_WINNER);
        handle_win_loss(w, winner);
        result_vo(w, winner);
        return false;
    }
    let t = w.fighter(loser).team.team;
    if w.setup.team_style != TeamStyle::FreeForAll && w.flow.team_knockouts[ti(t)] == 1.0 {
        match_point(w, S::other_team(t));
    }
    mutators::world_mutators_stock_life_lost(w);
    w.flow.stocks.as_ref().expect("the stock rule")[fi(w, loser)] > 0
}

/// `stockMatchPoint`: the match one ring-out from its end under the stock rule: two teams left with lives, one of
/// them on its last (in the two-team styles `stock_kill`'s match point; in FreeForAll the last two fighters). Read
/// by Mutator_ShrinkBlastBoxLastPoint at each life lost that the match goes on from, in place of its
/// KnockoutsToWin - 1 test, which reads the score.
pub fn stock_match_point(w: &World) -> bool {
    if w.flow.stocks.is_none() {
        return false;
    }
    let alive = teams_alive(w);
    alive.len() == 2 && alive.iter().any(|&t| w.flow.team_knockouts[ti(t)] == 1.0)
}

/// `scoreRingOut`: PandaGameMode OnPlayerRingoutImpl: TryUpdateTeamScore @513 and TryUpdateIndividualScore @537,
/// ahead of KillPlayerFighter @561 (`kill_player_fighter`: KillBaseFighter, then the win check).
pub fn score_ring_out(w: &mut World, loser: FighterId) {
    let style = w.setup.team_style;
    let ffa = style == TeamStyle::FreeForAll;
    let att = player_attacker(w, w.fighter(loser).attackers.most_recent_attacker); // team -1 without one
    // The assist's PlayerState casts (TryUpdateTeamScore @1061-1170, TryUpdateIndividualScore GetByPlayerState
    // @982-1089) fail for a minion: no Assists, no assist point, no pop text.
    let assist = player_attacker(w, most_recent_assist_attacker(w, loser));
    if w.flow.stocks.is_some() {
        stock_ring_out(w, loser, att, assist);
        return;
    }
    if w.flow.match_over {
        return;
    }
    let mult = w.flow.ko_point_multiplier;
    let loser_team = w.fighter(loser).team.team;
    // TryUpdateTeamScore. Without an attacker the other team scores in the 1v1 and 2v2 styles, nobody in
    // FreeForAll. With an attacker (@584..2279) its player state also counts RingOuts and, when the loser's
    // LastAttackerWasOffLedge is set, OffPlatformRingouts; MostRecentAssistAttacker's state counts Assists (every
    // team style; the assist attacker is the attacker before the last one while the 5 s AssistWindow of the last
    // attacker change runs, Fighter.registerHitFrom).
    if let Some(a) = att {
        let team = w.fighter(a).team.team;
        update_team_score(w, team, S::KO_POINTS[style as usize] * mult);
        let off = w.fighter(loser).attackers.last_attacker_off_ledge;
        let af = w.fighter_mut(a);
        af.stats.ring_outs += 1;
        if off {
            af.stats.off_platform_ring_outs += 1;
        }
        if let Some(s) = assist {
            w.fighter_mut(s).stats.assists += 1;
        }
        track_ring_out(w, a, loser);
        // TryUpdateTeamScore @3461-4402: "+{points} Ringout!" on the attacker's pawn, FreeForAll only
        if ffa {
            let id = w.fighter(a).id;
            let stacks = S::KO_POINTS[2] * mult;
            w.events.push(Event::PopText { fighter: id, kind: PopKind::FfaRingout, stacks, gain: None, params: None });
        }
    } else if !ffa {
        update_team_score(w, S::other_team(loser_team), mult);
    }
    // TryUpdateIndividualScore: Deaths; FreeForAll takes a point from the loser's team (not below 0) and gives the
    // assist attacker's team one.
    w.fighter_mut(loser).stats.kos += 1;
    if ffa {
        if w.flow.team_knockouts[ti(loser_team)] > 0.0 {
            update_team_score(w, loser_team, -1.0);
        }
        if let Some(s) = assist {
            let team = w.fighter(s).team.team;
            update_team_score(w, team, mult);
            // TryUpdateIndividualScore @2048-2843: "+{KOPointMultiplier} Assist!" on the assist attacker's pawn
            let id = w.fighter(s).id;
            w.events.push(Event::PopText {
                fighter: id,
                kind: PopKind::FfaAssist,
                stacks: mult,
                gain: None,
                params: None,
            });
        }
    }
}

/// `killPlayerFighter`: KillPlayerFighter after its KillBaseFighter @70 (the ring-out's defeat tail): the win
/// check. Returns whether the fighter respawns (a RespawnTimerActor is spawned).
pub fn kill_player_fighter(w: &mut World, loser: FighterId) -> bool {
    let rules = w.setup.rules.expect("a match with rules");
    let ffa = w.setup.team_style == TeamStyle::FreeForAll;
    if w.flow.stocks.is_some() {
        return stock_kill(w, loser);
    }
    let att = player_attacker(w, w.fighter(loser).attackers.most_recent_attacker); // @613 (team -1 without one)
    // The win check is not gated by MatchOver, and the winner comes from the two-team switch on the loser's team.
    // Deviation: FreeForAll has no ring-out win and ends by time only, since that switch would hand the win to
    // team 0 or 1 whoever scored.
    let loser_team = w.fighter(loser).team.team;
    let scoring = att.map_or_else(|| S::other_team(loser_team), |a| w.fighter(a).team.team);
    let score = w.flow.team_knockouts[ti(scoring)];
    if !ffa && rules.knockouts_to_win > 0.0 && score >= rules.knockouts_to_win {
        let winner = S::other_team(loser_team);
        handle_win_loss(w, winner);
        result_vo(w, winner);
        return false;
    }
    if !ffa && score == rules.knockouts_to_win - 1.0 {
        match_point(w, scoring);
    }
    true
}

/// `resultVo`: HandleWinLoss's announcer lines after a ring-out win. The loop over the players after it: a player
/// whose team won gets Announcer VO Victory on its controller, the others Announcer VO Defeat, unless the match is
/// off a dedicated server and no 1v1 bot match (then all get Victory). A custom game (the only ring-out win here) is
/// played on the build's dedicated server, so the line follows the player's team. Each controller runs
/// Delay(MatchCompleteVODelay), then the game state's line on its own client. Bots have no PFGPlayerController; the
/// port's machine hears its human players, the first in player order. Local Play is standalone, not on a dedicated
/// server, and no 1v1 bot match (Is1v1BotMatch needs queue type 5, a backend queue's), so PandaGameMode @1672-2205
/// gives every player Victory.
/// Unverified: the dedicated server's rule is taken from the lobby's server join, not captured.
fn result_vo(w: &mut World, winner: i32) {
    if !w.flow.victory_vo {
        w.flow.victory_vo = true;
        later(w, S::MATCH_COMPLETE_VO_DELAY, TimerCmd::VictoryVo { winner }, None);
    }
}

/// `matchPoint`: a team one ring-out from the win (IsMatchPoint). DisplayMatchPointServer: Delay(1.0), then (not
/// when the match is over) DisplayMatchPointClient: the warning texts and the announcer's line, offline team 0 =
/// blue.
fn match_point(w: &mut World, scoring: i32) {
    w.events.push(Event::MatchPoint { team: scoring });
    later(w, 1.0, TimerCmd::MatchPointShow { team: scoring }, None);
}

/// `updateFFATeamDamage`: Comp_Pawn_DamageManager RecordDamage's second branch (@549-655; it runs for every
/// amount, the @44 gate on a positive one only skips the DamageDealt branch): with IsFFA (set at the component's
/// start for a fighter in team style 2, ubergraph 6280-6521 and 95-439) BPI_UpdateFFATeamDamage(the victim's Team
/// Index, Damage). PandaGameMode (ubergraph 12994-13507): Team Damage[team] += Damage, never reset by a ring-out or
/// a respawn; then, when TiedFFATeams holds the team and is not that team alone, UpdateTeamScore(team, 0), which
/// picks the leader again. Team Damage is a TArray<float>: @13201 Add_FloatFloat(Damage, old sum) in 32-bit floats; the
/// amounts come in as the sim's float32.
pub fn update_ffa_team_damage(w: &mut World, vic: FighterId, damage: Real) {
    if w.setup.team_style != TeamStyle::FreeForAll || w.setup.rules.is_none() {
        return;
    }
    let team = w.fighter(vic).team.team;
    let t = ti(team);
    w.flow.team_damage[t] += damage;
    if w.flow.tied_ffa_teams.contains(&team) && w.flow.tied_ffa_teams.len() != 1 {
        update_team_score(w, team, 0.0);
    }
}

/// `updateTeamScore`: PandaGameMode UpdateTeamScore. 1v1 / 2v2: UpdateScoreUI(team, score, false, false, false).
/// FreeForAll: every team's UI is updated; the leader is the team with the most points, among equal points the
/// lowest Team Damage (nobody when that is equal too: then every tied team counts as leader), Tied = more than one
/// team on the top score. Team Damage (@892, 2716, 2852, 2981) is the game state's running total of the damage the
/// team's fighters took (`update_ffa_team_damage`), not their current damage as in GetWinners; no Falls here. The
/// teams on the top score stay in TiedFFATeams (@1139-1180).
pub fn update_team_score(w: &mut World, team: i32, addition: Real) {
    w.flow.team_knockouts[ti(team)] += addition;
    if w.setup.team_style != TeamStyle::FreeForAll {
        let score = w.flow.team_knockouts[ti(team)];
        w.events.push(Event::TeamScore { team, score, is_leader: false, tied: false });
        return;
    }
    let mut leading = -2147483648.0;
    let mut lowest: Real = 2147483600.0;
    let mut leader = -1;
    let mut tied: Vec<i32> = Vec::new();
    for (t, &score) in w.flow.team_knockouts.iter().enumerate() {
        let t32 = i32::try_from(t).expect("a team index");
        let damage = w.flow.team_damage[t];
        if score == leading {
            if !tied.contains(&t32) {
                tied.push(t32);
            }
            if damage == lowest {
                leader = -1;
            } else if damage < lowest {
                lowest = damage;
                leader = t32;
            }
        } else if score > leading {
            tied = vec![t32];
            leading = score;
            lowest = damage;
            leader = t32;
        }
    }
    let n = tied.len();
    for (t, &score) in w.flow.team_knockouts.iter().enumerate() {
        let t = i32::try_from(t).expect("a team index");
        let is_leader = (tied.contains(&t) && leader < 0) || leader == t;
        w.events.push(Event::TeamScore { team: t, score, is_leader, tied: n > 1 });
    }
    w.flow.tied_ffa_teams = tied;
}

/// The winning team of a match that ends with none (`winners` empty: no player).
// Unverified: the build's value for no winner is not read; -1 (the reset's winning team) matches no fighter's team:
// every live fighter is abducted.
const NO_WINNER: i32 = -1;

/// `winners`: PandaGameMode GetWinners. Per team (@323-1934): Points = TeamKnockouts, Damage = the summed
/// CurrentDamage of its players (GetPlayerDamage @1044), Falls = their summed PFGPlayerState.Deaths (@1847). The best
/// score starts as team 0's with an empty winner list (@2055); every team in index order, team 0 too (@2138-3356):
/// fewer points: skipped (@2386); more: the new best and the only winner (@2516, 2586-2713); equal points switch on
/// the team style (@2887-3008): 0 and 1 compare Damage (@3023: lower = new best, higher = skipped, equal = added to
/// the list @2713); 2 compares Falls first (@3467-3716: fewer = new best, more = skipped) and goes on to the Damage
/// compare when they are equal.
pub fn winners(w: &World) -> Vec<i32> {
    let ffa = w.setup.team_style == TeamStyle::FreeForAll;
    let sum = |t: usize, of: &dyn Fn(&crate::Fighter) -> Real| -> Real {
        w.players.iter().map(|&f| w.fighter(f)).fold(0.0, |s, f| s + if ti(f.team.team) == t { of(f) } else { 0.0 })
    };
    let n = w.flow.team_knockouts.len();
    let damage: Vec<Real> = (0..n).map(|t| sum(t, &|f| f.stats.damage)).collect();
    let falls: Vec<Real> = (0..n).map(|t| sum(t, &|f| crate::real(f.stats.kos))).collect();
    let k = &w.flow.team_knockouts;
    let mut b = 0;
    let mut best: Vec<i32> = Vec::new();
    for t in 0..n {
        let mut c = k[t] - k[b]; // above 0: better than the best
        if c == 0.0 {
            if ffa {
                c = falls[b] - falls[t];
            }
            if c == 0.0 {
                c = damage[b] - damage[t];
            }
        }
        let t32 = i32::try_from(t).expect("a team index");
        if c > 0.0 {
            b = t;
            best = vec![t32];
        } else if c == 0.0 {
            best.push(t32);
        }
    }
    best
}

/// `PodiumEntry`: PandaGameInstance AddPlayerPodiumData (ubergraph 75262), from HandleWinLoss per player: the
/// PodiumPlayerData the end-of-game stat sheets read. RingOuts, DamageDealt, Falls (Deaths) and Score (TeamScore =
/// the team's TeamKnockouts) come from the PFGPlayerState. `assists` and `off_platform_ring_outs` are the same player
/// state's Assists and OffPlatformRingouts: no PodiumPlayerData member and no end-of-game widget takes them in the
/// build; they are carried here for the port's own use. `assisted`, `doubles` and `dodges` are the fighter's
/// stat-tracker counts (`track_ring_out`, the evade's dodge quest), for the match report.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct PodiumEntry {
    pub player_index: i32,
    pub team_index: i32,
    pub ring_outs: i32,
    pub damage_dealt: Real,
    pub falls: i32,
    pub score: Real,
    pub is_winner: bool,
    pub assists: i32,
    pub off_platform_ring_outs: i32,
    pub assisted: i32,
    pub doubles: i32,
    pub dodges: i32,
}

/// `podiumData`.
pub fn podium_data(w: &World) -> Vec<PodiumEntry> {
    w.players
        .iter()
        .enumerate()
        .map(|(i, &f)| {
            let f = w.fighter(f);
            let s = &f.stats;
            PodiumEntry {
                player_index: i32::try_from(i).expect("a player index"),
                team_index: f.team.team,
                ring_outs: s.ring_outs,
                assists: s.assists,
                off_platform_ring_outs: s.off_platform_ring_outs,
                assisted: s.ring_outs_assisted,
                doubles: s.double_ring_outs,
                dodges: s.attacks_dodged,
                damage_dealt: s.damage_dealt,
                falls: s.kos,
                score: usize::try_from(f.team.team)
                    .ok()
                    .and_then(|t| w.flow.team_knockouts.get(t))
                    .copied()
                    .unwrap_or(0.0),
                is_winner: w.flow.match_over && f.team.team == w.flow.winning_team,
            }
        })
        .collect()
}

/// `handleWinLoss`: HandleWinLoss, once. Every fighter's DamageDisabled, and Terminate on the RespawnTimerActors
/// (fighters that are out stay out), then AbductPlayer on each fighter that is not dead and not on the winning team.
pub fn handle_win_loss(w: &mut World, team: i32) {
    if w.flow.match_over {
        return;
    }
    w.flow.winning_team = team;
    w.flow.match_over = true;
    w.flow.match_over_tick = w.flow.tick;
    for f in w.players.clone() {
        let fr = w.fighter_mut(f);
        fr.invuln.damage_disabled = true;
        if fr.life.dead != 0.0 {
            fr.life.dead = Real::INFINITY;
        } else if fr.team.team != team {
            abduct(w, f);
        }
    }
    w.events.push(Event::MatchOver { team });
}

/// `removeCameraFocus`: the defeat tail's timer (BaseFighter @19077): RemoveCameraFocus (@202..331) 0.5 s later
/// unregisters the fighter's Comp_Actor_KeepOnCamera only while PFGCamera's RegisteredOnscreenActors holds more than
/// 2 actors. ResetFighterLogic registers it again. The array holds every actor whose Comp_Actor_KeepOnCamera is
/// registered (AddOnscreenActor, AddUnique): the fighters (a C019Marker recall unregisters its owner for the travel:
/// capsuleOff), each live C019Marker and its C019MarkerCameraFocus actor (both KeepOnCamera? true, registered from
/// BeginPlay to EndPlay), and each live RingOutVfxAnchor (registered at its BeginPlay, unregistered at its EndPlay),
/// which lives the asset's max(CameraHoldTime, 0.01) s (ROV_Default's 0 gives the ring-out frame and the next one's
/// timer pass, where its own timer, 0.01 s after the ring-out frame, expires ahead of this one; 1 to 2.5 s for the
/// others, so in a 1v1 the loser's own 0.5 s timer meets 3 actors and unregisters it). K2_SetTimer by function name
/// on the fighter: a second defeat tail inside the 0.5 s sets the same timer again.
/// Unverified: other classes with the component (IntroCameraAdjuster, characters not ported) are not counted, and an
/// anchor's timer due in the same frame as this one is taken first (their expiry times, which FTimerManager orders
/// them by, are not kept: only the 0.01 s case was read).
pub fn remove_camera_focus(w: &mut World, f: FighterId) {
    let key = w.fighter(f).id;
    later(w, 0.5, TimerCmd::RemoveCameraFocus { fighter: f }, Some(key));
}

/// `abductAll`: TrainGameMode Destroy All Players: AbductPlayer on every spawned fighter. Only its effect and sound:
/// the pawns are replaced when the game mode's own 1.0 s delay ends (the Lab's unpause).
pub fn abduct_all(w: &mut World) {
    for f in w.players.clone() {
        let id = w.fighter(f).id;
        w.events.push(Event::Abduct { fighter: id });
    }
}

/// `abduct`: BaseFighter AbductPlayer (ubergraph 149740): PS_EndgameLosers on the capsule and
/// sfx_map_shared_endgamelosers at the actor's location (the `abduct` event), Delay(1.0), then the defeat tail
/// (18358): AddInvisibility, ShowOrHideUI(false), input and movement off, StopAnimMontage, isDead. Inside the second
/// nothing is closed: AbductPlayer sets bIsAbducting (no Blueprint reader), spawns the effect and the sound and
/// waits; BPI_DisableInput, StopAnimMontage and DisableMovement are the tail's (19141-19170), so the fighter still
/// acts until the Delay ends. The tail's first step (18380-19022) is K2_DestroyActor on every Buff_Parent_Actor
/// child of the fighter with DestroyBuffWhenOwnerDefeated.
pub fn abduct(w: &mut World, f: FighterId) {
    let id = w.fighter(f).id;
    w.events.push(Event::Abduct { fighter: id });
    later(w, 1.0, TimerCmd::AbductTail { fighter: f }, None);
}

/// `abduct`'s Delay(1.0) tail.
fn abduct_tail(w: &mut World, f: FighterId) {
    if let Ok(i) = usize::try_from(w.fighter(f).id)
        && let Some(tr) = w.flow.bonus_trackers.get_mut(i)
    {
        tr.alive = false;
    }
    buffs::defeat_buffs(w, f);
    // A ring-out inside the second (KillBaseFighter @159028 reaches the same tail at 18358): the Delay still resumes
    // and the tail runs a second time. On a fighter that is out already that leaves one thing to see: the
    // RemoveCameraFocus timer is set again (0.5 s from here, replacing one still pending).
    if w.fighter(f).life.dead != 0.0 {
        remove_camera_focus(w, f);
        return;
    }
    anim::stop_anim_montage(w, f); // the defeat tail's StopAnimMontage(null), BaseFighter @19155
    buffs::buff_signal(w, f, BuffSignal::BfDefeated, Default::default()); // OnBFDefeated @19264
    let fr = w.fighter_mut(f);
    fr.body.vx = 0.0;
    fr.body.vy = 0.0;
    fr.hit.hitpause = 0.0;
    fr.life.dead = Real::INFINITY;
    fr.life.abducted = true;
    fr.life.cam_focus = true;
    remove_camera_focus(w, f);
}

/// `matchTick`: the game mode's timers: the announcer's line end, the timers, the FreeForAll BonusPointsTimer, the
/// MatchTimer (TimeUpImpl: the first of GetWinners wins) and the steps after HandleWinLoss. The timers run in the
/// Lab too (TrainGameMode): there only a ring-out's RemoveCameraFocus sets one.
pub fn match_tick(w: &mut World) {
    let fl = &mut w.flow;
    if fl.announcer_until >= 0 && fl.tick >= fl.announcer_until {
        fl.announcer_until = -1;
        fl.vo_shared.speakers = (fl.vo_shared.speakers - 1).max(0);
    }
    if fl.timers.iter().any(|t| t.at <= fl.tick) {
        let tick = fl.tick;
        let mut due = std::mem::take(&mut w.scratch.timers_due);
        due.clear();
        w.flow.timers.retain(|t| {
            let d = t.at <= tick;
            if d {
                due.push(*t);
            }
            !d
        });
        for t in &due {
            run_timer(w, t.cmd);
        }
        w.scratch.timers_due = due;
    }
    crate::world::catch_up::timer(w);
    let Some(rules) = w.setup.rules else { return };
    let sec = blast::js_ticks;
    if w.flow.match_started && !w.flow.match_over {
        let t = w.flow.tick - w.flow.match_start_tick;
        if w.setup.team_style == TeamStyle::FreeForAll
            && !rules.stocks
            && rules.duration > 0.0
            && t == sec(rules.duration * S::DOUBLE_POINTS_START)
        {
            w.flow.ko_point_multiplier = S::BONUS_POINTS_MULTIPLIER;
            w.events.push(Event::DoublePoints);
        }
        // SetTimer with a time of 0 arms nothing (stock UE): the lobby's "Unlimited" (0) has no MatchTimer.
        if rules.duration > 0.0 && t == sec(rules.duration) {
            let winner = winners(w).first().copied().unwrap_or(NO_WINNER);
            handle_win_loss(w, winner);
            w.events.push(Event::TimeUp);
            announce(w, "announcer_timeout"); // TimeUpImpl: AnnouncerTimeoutVO, and no victory / defeat line
        }
    }
    if !w.flow.match_over {
        return;
    }
    let mut at = 0;
    for (phase, delay) in S::END_PHASES {
        at += sec(delay);
        if w.flow.tick - w.flow.match_over_tick != at {
            continue;
        }
        // MatchEndImpl: MatchStarted? = false.
        if phase == crate::events::EndPhase::GameEnd {
            w.flow.match_started = false;
        }
        let team = w.flow.winning_team;
        w.events.push(Event::MatchEnd { phase, team });
    }
}

/// `respawn`.
pub fn respawn(w: &mut World, f: FighterId) {
    let s = w.stage.respawns[fi(w, f)];
    // The registered passive buffs other fighters' managers gave this one since its defeat (`reset_passives`).
    let kept: Vec<FighterId> = w.players.iter().copied().filter(|&o| o != f && buffs::passive_from(w, f, o)).collect();
    match_side::spawn(w, f, s.x, s.y, s.facing);
    match_side::reset_for_respawn(w, f);
    crate::world::reset::reset_passives(w, f, &kept);
    mutators::world_mutators_fighter_reset(w, f); // the world mutators' bindings of OnFighterReset (@1106)
    mutators::apply_mutators_to_fighter(w, f); // RespawnCharacter @1560, after BPI_ResetFighter
    let ffa = w.setup.rules.is_some() && w.setup.team_style == TeamStyle::FreeForAll;
    // BPI_RespawnFighter @27180-27812: "-1 Rungout!" when the pawn's team has points, FreeForAll
    if ffa && w.flow.stocks.is_none() && w.flow.team_knockouts[ti(w.fighter(f).team.team)] > 0.0 {
        let id = w.fighter(f).id;
        w.events.push(Event::PopText { fighter: id, kind: PopKind::FfaRungout, stacks: 0.0, gain: None, params: None });
    }
    if ffa {
        ffa_catch_up(w, f);
    }
    // The same pawn, moved to its respawn point: its capsule begins its overlaps there, and the begin with the
    // CollisionComponent takes it out of NonOverlappingActors (BlastBoxPlayerExit ubergraph 7489).
    let b = blast::blast_overlaps(w, f);
    let fr = w.fighter_mut(f);
    fr.life.blast_in = b;
    if b & 1 != 0 {
        fr.life.blast_listed = false;
    }
    fr.vis.on_camera = true; // ResetFighterLogic: Comp_Actor_KeepOnCamera.Register
    // ResetFighterLogic -> Comp_Spawner.OnBaseFighterReset -> DestroyActorsOnEndPlay: K2_DestroyActor on every actor
    // in the spawner's SpawnedActors whose BPI_GetDiesWithInstigator ('Die With Instigator') is true. The actors add
    // themselves in BeginPlay (owner's spawner), so the item spawned at a projectile's death counts.
    spawning::destroy_with_instigator(w, f);
    mutators::world_mutators_respawned(w, f); // PlayerRespawned (BPI_RespawnFighter @26815)
}

/// `ffaCatchUp`: FFAGameMode SpawnAndInitPlayer (override) with Respawn, after the parent's spawn: counts the other
/// teams whose TeamKnockouts are above the pawn's team's (@340-1037); with more than one (@1406) it takes
/// Array_Random of the keys of RespawnCatchUpBuffs (@1465-1574; the second Array_Random @1593 only fills an index
/// nothing reads) and SpawnBuffActor(class, pawn, pawn, 0, the map's value as the duration) (@1730-1799). The CDO's
/// map, in order: Buff_RespawnArmor 5 s, Buff_Enlarge 7.5 s, Buff_RespawnSpeed 10 s.
// Deviation: the pick comes from the sim's own stream (the build's is FMath::Rand).
pub fn ffa_catch_up(w: &mut World, f: FighterId) {
    let team = w.fighter(f).team.team;
    let own = usize::try_from(team).ok().and_then(|t| w.flow.team_knockouts.get(t)).copied().unwrap_or(0.0);
    let ahead = w.flow.team_knockouts.iter().enumerate().filter(|&(t, &s)| ti(team) != t && s > own).count();
    if ahead <= 1 {
        return;
    }
    let i = usize_of(w.rng.int(js_len(FFA_CATCH_UP_BUFFS.len())));
    let (cls, duration) = FFA_CATCH_UP_BUFFS[i];
    buffs::spawn_buff(w, f, cls, duration, Some(0), Some(f), true, None);
}

// ---- the match set-up

/// `ffaItemsOn`: FFAGameMode's own spawner: StartNewMatch starts it (newItemArea). A
/// set-up of team style 2 is FreeForAll only with rules: the Lab's (TrainGameMode, no rules) takes style 2 for three
/// or four team indexes and has no items.
pub fn ffa_items_on(setup: &MatchSetup) -> bool {
    setup.team_style == TeamStyle::FreeForAll && setup.rules.is_some()
}

/// `itemsDataOf`: the items part a match reads (`GameData::items`): only in a match with items, so the class lookups
/// of a match without them are the same whether or not the data holds it.
pub fn items_data_of(w: &World) -> Option<&ItemsData> {
    if S::items_on(&w.setup) { w.data.items.as_ref() } else { None }
}

// ---- the run-time delegates

/// `boundDelegates`: the emit sites' read: `None` for a pawn nothing has bound (nothing to run).
pub fn bound_delegates(w: &World, f: FighterId) -> Option<&PawnDelegates> {
    let d = &w.fighter(f).delegates;
    let any = !(d.hit_by_enemy.is_empty()
        && d.knocked_back_include_self.is_empty()
        && d.evaded_attack.is_empty()
        && d.exited_arena.is_empty()
        && d.movement_mode_changed.is_empty()
        && d.after_spawned_actor.is_empty()
        && d.charge_end.is_empty()
        && d.ability_cooldown_updated.is_empty()
        && d.take_any_damage.is_empty());
    any.then_some(d)
}

/// `boundActorDelegates`.
pub fn bound_actor_delegates(w: &World, a: ActorRef) -> Option<&ActorDelegates> {
    w.actor_delegates.get(&a)
}

// ---- the tutorial event bus

/// `tutorialEvent`: BPI_HandleTutorialEvent with no instigator, as every emitter calls it (PandaGameState
/// BPI_HandleTutorialEvent -> TutorialActor). Outside a tutorial World.tutorial is unset.
pub fn tutorial_event(w: &mut World, ev: TutorialEvent) {
    if crate::modes::tutorial::running(w) {
        crate::modes::tutorial::run::bus_event(w, ev);
    }
}

// ---- the arcade mode

/// `arcadeRoundType`: ArcadeGetRoundType (AXFunctions @0-531): the ladder of the difficulty (EArcadeDifficulty 1
/// Easy, 2 Normal, 3 Hard; 0 and anything else an empty array), indexed by the 1-based round; a round past the array
/// is 0 (@511). AXRoundType: 0 Invalid, 1 Regular, 2 Rivals, 3 Mini, 4 Reign Dog, 5 Giant Gizmo.
pub fn arcade_round_type(difficulty: u8, round: u8) -> u8 {
    const LADDERS: [&[u8]; 4] = [&[], &[0, 1, 1, 1], &[0, 1, 1, 5, 2, 4], &[0, 1, 1, 1, 5, 1, 1, 2, 4]];
    let l = LADDERS.get(usize::from(difficulty)).copied().unwrap_or(&[]);
    l.get(usize::from(round)).copied().unwrap_or(0)
}

/// `arcadeRoundBuff`: Arcade Get Round Buff(Difficulty, Round, TeamIndex) (AXFunctions @0-2335): the buff class, or
/// `None` (UseBuff false). Difficulty 0: none (@455-682). Team 0: Buff_AXBotAlly (@500-645). Otherwise by the round
/// type: Invalid and Regular (@1560-1965) Buff_AXBotEnemyHard on Hard, none on Easy and Normal; Rivals (@1023-1536)
/// Buff_AXBotMediumNemesis on Normal, Buff_AXBotEnemyHardNemesis on Hard, none on Easy; Mini Buff_Arcade_Miniature
/// (@1989); Reign Dog Buff_Arcade_Secret_Boss (@2106); Giant Gizmo Buff_Arcade_MidBoss (@2227).
pub fn arcade_round_buff(difficulty: u8, round: u8, team: i32) -> Option<Name> {
    if difficulty == 0 {
        return None;
    }
    if team == 0 {
        return Some(crate::name!("Buff_AXBotAlly_C"));
    }
    match arcade_round_type(difficulty, round) {
        0 | 1 => (difficulty == 3).then_some(crate::name!("Buff_AXBotEnemyHard_C")),
        2 => match difficulty {
            2 => Some(crate::name!("Buff_AXBotMediumNemesis_C")),
            3 => Some(crate::name!("Buff_AXBotEnemyHardNemesis_C")),
            _ => None,
        },
        3 => Some(crate::name!("Buff_Arcade_Miniature_C")),
        4 => Some(crate::name!("Buff_Arcade_Secret_Boss_C")),
        _ => Some(crate::name!("Buff_Arcade_MidBoss_C")),
    }
}

/// A Comp_Pawn_PassiveBuffManager Buffs row: the data's (`Buff`, `BuffSelf`, `BuffAllies`, `BuffOpponents`,
/// `Duration`, `StartingStacks`), or ApplyArcadeModeBuffs' (`PassiveRow`, @2024-2204: {Buff,
/// BuffSelf true, BuffAllies false, BuffOpponents false, Duration 0, StartingStacks 1}; Icon Avatar2 the HUD's).
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct PassiveRow {
    pub buff: Name,
    pub buff_self: bool,
    pub buff_allies: bool,
    pub buff_opponents: bool,
    pub duration: Real,
    pub starting_stacks: i32,
}

/// `arcadeRows`: PandaGameMode SpawnCharacter @1857-1955: with IsArcadeModeActive and an AIControlled pawn,
/// ApplyArcadeModeBuffs(the round's difficulty and stage, the player's team) at its first spawn; a human gets
/// nothing. The rows of each player (by player index), empty for a match without an arcade round.
pub fn arcade_rows(w: &World, teams: &[i32]) -> Vec<Vec<PassiveRow>> {
    let a = w.setup.arcade;
    w.setup
        .players
        .iter()
        .enumerate()
        .map(|(i, p)| {
            let Some(a) = a.filter(|_| p.bot) else { return Vec::new() };
            let b = arcade_round_buff(a.difficulty, a.stage, teams.get(i).copied().unwrap_or(0));
            b.map(|b| PassiveRow {
                buff: b,
                buff_self: true,
                buff_allies: false,
                buff_opponents: false,
                duration: 0.0,
                starting_stacks: 1,
            })
            .into_iter()
            .collect()
        })
        .collect()
}

/// `arcadeBuffDefs`: the arcade buff classes' defaults, read (`buffDef`, after the stage's) only by a World whose
/// set-up has an arcade round.
pub fn arcade_buff_defs(w: &World) -> Option<&ArcadeBuffs> {
    w.setup.arcade.map(|_| &w.data.arcade)
}

// ---- the mutators

/// `mutatorData`: the mutators' data, read only by a World whose set-up names mutators.
pub fn mutator_data(w: &World) -> Option<&MutatorsData> {
    (!w.setup.mutators.is_empty()).then_some(&w.data.mutators)
}

// ---- the call surface of the tutorial run

/// `startTutorial`.
pub fn start_tutorial(w: &mut World) {
    crate::modes::tutorial::run::start_tutorial(w);
}

/// `resetPass` (set and cleared by `World.reset`): inside World.reset's pass over the
/// spawned actors, whose end paths skip the parts that reach a fighter.
pub fn reset_pass(w: &World) -> bool {
    w.scratch.reset_pass
}

// ---- stubs a hook of the characters' and notifies' code calls

/// `boundDelegates(f)?.chargeEnd.broadcast()`: Comp_Pawn_ChargeAttack ChargeEnd to the
/// fighter's run-time binders (the tutorial's demo controllers, `modes/tutorial/advanced/fighters`).
pub fn broadcast_charge_end(w: &mut World, f: FighterId) {
    broadcast_pawn(w, f, PawnDelegate::ChargeEnd);
}

/// `movementModeByte`: the pawn's movement mode as the port holds it (EMovementMode): Walking on a floor, Flying
/// under a ControlSpeed command, Custom (the knockback mode, PhysKnockback) while in it, Falling otherwise; None once
/// destroyed.
pub fn movement_mode_byte(w: &World, f: FighterId) -> i32 {
    let fr = w.fighter(f);
    if fr.life.dead == Real::INFINITY {
        0
    } else if fr.body.on_floor {
        1
    } else if fr.moves.flying {
        5
    } else if fr.hit.kb_mode {
        6
    } else {
        3
    }
}

/// `movementModeTick`: the MovementModeChanged broadcast for the pawns whose delegate is bound: each pawn's mode
/// against the one this pass saw last (`PawnDelegates::last_mode`), over the match's fighters.
/// Deviation: the build broadcasts inside SetMovementMode (the CMC's tick, a launch, a landing); the port has no one
/// place that sets the mode, so a change is found here, in the pass the caller runs it from (a tutorial's leftover
/// latent pass), with the mode the frame ends in: a change and its change back within one frame are not seen.
pub fn movement_mode_tick(w: &mut World) {
    for f in w.all_ids() {
        let m = movement_mode_byte(w, f);
        let d = &mut w.fighter_mut(f).delegates;
        let prev = d.last_mode;
        d.last_mode = m;
        if d.movement_mode_changed.is_empty() || prev == -1 || prev == m {
            continue;
        }
        let l = DelegateList::Pawn(f, PawnList::MovementModeChanged);
        broadcast_bound(w, l, &DelegateCall::MovementModeChanged { mode: m, prev });
    }
}

impl World {
    /// The game state's RoundTimeRemaining (UI_IGv3_Clock reads it):
    /// RoundDuration until the match starts; then PandaGameMode Update Round Remaining Time (@22173, Delay(1.0)
    /// @22231 -> @10573) writes the MatchTimer's remaining time once a second. Its first pass (BPI_SetMatchStarted
    /// @2860) runs before the timer is set (@3239) and only starts the Delay; each Delay ends 60 frames later, in the
    /// latent pass before that frame's timer tick: sample k reads duration - (60 k - 1) / 60. A cleared timer is not
    /// read, so the value stays after the match.
    // Unverified: the 60-frame Delay by the port's DeltaSeconds 0.0166668.
    pub fn round_time_remaining(&self) -> Real {
        let d = self.setup.rules.map_or(0.0, |r| r.duration);
        let f = &self.flow;
        round_time_remaining(d, f.match_started, f.match_over, f.tick, f.match_start_tick, f.match_over_tick)
    }
}

/// `roundTimeRemaining` of a duration `d` and the flow's ticks.
fn round_time_remaining(d: Real, started: bool, over: bool, tick: i32, start: i32, over_tick: i32) -> Real {
    if !started && !over {
        return d;
    }
    let k = (crate::real((if over { over_tick } else { tick }) - start) / 60.0).floor();
    if d > 0.0 && k > 0.0 { d - (60.0 * k - 1.0) / 60.0 } else { d }
}

#[cfg(test)]
mod round_time_tests {
    use super::round_time_remaining;

    #[test]
    fn steps_once_a_second_and_holds_after_the_match() {
        // before the start, and the first second
        assert_eq!(round_time_remaining(60.0, false, false, 100, 0, 0), 60.0);
        assert_eq!(round_time_remaining(60.0, true, false, 479, 420, 0), 60.0);
        // sample k = 1 at 60 frames after the start: 60 - 59 / 60
        assert_eq!(round_time_remaining(60.0, true, false, 480, 420, 0), 60.0 - 59.0 / 60.0);
        assert_eq!(round_time_remaining(60.0, true, false, 545, 420, 0), 60.0 - 119.0 / 60.0);
        // after the match: the value at the match's end
        assert_eq!(round_time_remaining(60.0, true, true, 9000, 420, 545), 60.0 - 119.0 / 60.0);
        // no time limit
        assert_eq!(round_time_remaining(0.0, true, false, 9000, 420, 0), 0.0);
    }
}
