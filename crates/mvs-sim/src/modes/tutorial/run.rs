//! The tutorial level's sim: TutorialGameMode (the World's `ModeHooks`) and the
//! TutorialActor step / task machine with TutorialActor_Intro's own graph. Every call the actor makes into the
//! widgets goes out as an `Event::TutorialUi` in call order.
//!
//! The call surface for the UI (`TutorialRun`): the widgets', the pause menu's and the end screen's calls, each run
//! between two world steps, and the getters the UI reads. Each call does nothing before the actor exists.
//!
//! The host interface (`host.rs` `TutorialHost`) is this module's public calls: `spawn_fighter`, `possess_bot`,
//! `data_team`, `spawn_idle_bag`, `despawn`, `increment_minigame_counter`, `minigame_counter`, `bus_event` and the
//! run lists of `classes.rs`.

use std::sync::Arc;

use crate::Real;
use crate::ai::bt::{BotController, BtBot};
use crate::consts::DELTA_SECONDS;
use crate::events::{Event, TutorialUiCall};
use crate::fighter::FighterId;
use crate::fighter::common::warn_text;
use crate::fighter::match_side::TutorialEvent;
use crate::math::{dbl, narrow, to_int32};
use crate::name;
use crate::names::Name;
use crate::setup::{MatchMode, team_count};
use crate::world::World;
use crate::world::flow::DelegateCall;
use crate::world::pawns::{self, FirstTicks, ModeHooks, PawnCtl, PawnSpawn};
use mvs_data::tutorial::TutorialStep;
use mvs_data::{GameData, TutorialActorData};

use super::classes::{self, Call, Fired, RunList};
use super::demo::callable;
use super::pawn::{BagSpawn, place_of_loc, starts_with_tag};
use super::{Timeline, TutorialActor, TutorialObj, TutorialRun, TutorialScore, latent, minigame};

const INTRO: &str = "TutorialActor_Intro";
const RESPAWN_TIMER: &str = "TutorialPlayerRespawn";
/// Pawn_WonderWomanV3, which the Combat / Damage / IntroFinale and AttackDecay_Basics demos spawn by class.
const DEMO_BAG: Name = name!("wonderwoman");
/// AudioStep's ui_tutorial_step_complete_0N.
const STEP_SOUNDS: [Name; 6] = [
    name!("ui_tutorial_step_complete_00"),
    name!("ui_tutorial_step_complete_01"),
    name!("ui_tutorial_step_complete_02"),
    name!("ui_tutorial_step_complete_03"),
    name!("ui_tutorial_step_complete_04"),
    name!("ui_tutorial_step_complete_05"),
];

/// The dummy controller classes a step names, as the port's bot controllers.
fn dummy_controller(cls: &str) -> Option<BotController> {
    match cls {
        "Idle_AIController" => Some(BotController::Idle),
        "TutorialRecoverOnly_AIController" => Some(BotController::TutorialRecoverOnly),
        "RecoverOnly_AIController" => Some(BotController::RecoverOnly),
        "Base_AIController" => Some(BotController::Base),
        _ => None,
    }
}

fn actor(w: &World) -> Option<&TutorialActor> {
    w.tutorial.as_ref().and_then(|t| t.actor.as_ref())
}

fn act(w: &World) -> &TutorialActor {
    actor(w).expect("the TutorialActor")
}

fn act_mut(w: &mut World) -> &mut TutorialActor {
    w.tutorial.as_mut().and_then(|t| t.actor.as_mut()).expect("the TutorialActor")
}

/// The running actor's data, from the World's data held apart (`d`: `w.data` cloned) so the World stays writable.
fn td<'a>(d: &'a GameData, w: &World) -> &'a TutorialActorData {
    &d.tutorials[&act(w).actor]
}

fn step_ix(w: &World) -> usize {
    usize::try_from(act(w).step_index).unwrap_or(usize::MAX)
}

fn ui(w: &mut World, call: TutorialUiCall) {
    w.events.push(Event::TutorialUi { call });
}

fn ui_sound(w: &mut World, ev: Name) {
    w.events.push(Event::Sound { fighter: -1, ev, x: None, y: None, hit: None, pooled: false, key: None });
}

fn vo(w: &mut World, alias: Option<&str>) {
    if let Some(a) = alias {
        crate::world::flow::game_state_vo(w, a);
    }
}

fn delay(w: &mut World, node: &'static str, s: Real) {
    latent::delay(w, TutorialObj::Actor, node, s);
}

fn len_i32(n: usize) -> i32 {
    i32::try_from(n).expect("a short list")
}

// ---- the host's calls

/// `dataTeam`: GetByPlayerIndex(i).GetTeamIndex: the set-up's player i.
/// Unverified: InitializePlayerDataManager's three empty player data (@1387-2095) are taken to hold the index's
/// default team (i mod the team count), as a set-up player with no team does.
pub fn data_team(w: &World, i: usize) -> i32 {
    match w.setup.players.get(i).and_then(|p| p.team) {
        Some(t) => i32::from(t),
        None => len_i32(i) % i32::from(team_count(&w.setup)),
    }
}

/// `spawnFighter`: the tutorial's own spawns: BeginDeferredActorSpawnFromClass + FinishSpawningActor of a fighter
/// class (`spawn_pawn`), PlayerDataReadyServer(slot), the team of player data `team` (a field write), every pawn the
/// tutorial makes. Its first tick: the frame's leftover pass, where every tutorial spawn is made inside a step.
pub fn spawn_fighter(
    w: &mut World,
    character: Name,
    x: Real,
    y: Real,
    facing_left: bool,
    slot: i32,
    team: i32,
) -> FighterId {
    let f = pawns::spawn_pawn(w, PawnSpawn { character, x, y, facing_left, at: FirstTicks::Leftover });
    pawns::bind_slot(w, f, slot);
    w.fighter_mut(f).team.team = team;
    f
}

/// `possessBot`: SpawnActor(controller class) + Possess for a bot controller (the tree by name),
/// BPI_SetDifficultyLevel(2) on the tutorial's own bags. Its first think reads the time as a minion's.
pub fn possess_bot(w: &mut World, f: FighterId, controller: BotController) {
    let Some(trees) = w.bot_trees.clone() else { panic!("tutorial: the bag's controller needs World::bot_trees") };
    let id = w.fighter(f).id;
    let seed = dbl::to_int32(f64::from(id) * 7919.0 + f64::from(w.flow.tick) * 104_729.0 + 12345.0);
    let mut bot = BtBot::controller_bot(seed, &trees, controller);
    let cl = &w.fighter(f).clock;
    bot.ticks = 1;
    bot.now = if w.scratch.step_at != 0 { cl.prev_time } else { cl.time };
    bot.fighter_born = cl.creation_time;
    pawns::possess(w, f, PawnCtl::Bot(Box::new(pawns::bot_ctl(bot, None, None))));
}

/// `spawnIdleBag`: a demo's own Wonder Woman bag (`pawn::spawn_punching_bag`) for player data 1.
pub fn spawn_idle_bag(
    w: &mut World,
    tag: &str,
    damage: Real,
    spawn_invulnerability_time: Option<Real>,
) -> Option<FighterId> {
    let team = data_team(w, 1);
    let b = BagSpawn { tag, character: DEMO_BAG, slot: 1, team, damage, spawn_invulnerability_time };
    super::pawn::spawn_punching_bag(w, b)
}

/// `despawn`: K2_DestroyActor on a pawn still held by its controller (AController::PawnPendingDestroy destroys it).
pub fn despawn(w: &mut World, f: FighterId) {
    pawns::despawn_pawn(w, f, false);
}

/// `minigameCounter`: TutorialSteps[step].TutorialTasks[task].TaskMinigameCounter.
pub fn minigame_counter(w: &World, step: i32, task: i32) -> i32 {
    let (Ok(s), Ok(t)) = (usize::try_from(step), usize::try_from(task)) else { return 0 };
    act(w).minigame_counters.get(s).and_then(|c| c.get(t)).copied().unwrap_or(0)
}

/// `incrementMinigameCounter`: TutorialMinigame_Parent IncrementMinigameCounter: TaskMinigameCounter + 1 (@102), the
/// task widget's BPI_SetMinigameCounter (@328), then with ShowRewardPopText the "+1" PopText attached to
/// TutorialManagedPawn (@434-973).
pub fn increment_minigame_counter(w: &mut World, step: i32, task: i32, pop: bool) {
    let a = act_mut(w);
    let c = usize::try_from(step).ok().and_then(|s| a.minigame_counters.get_mut(s));
    if let Some(c) = c
        && let Ok(t) = usize::try_from(task)
        && t < c.len()
    {
        c[t] += 1;
        let count = c[t];
        ui(w, TutorialUiCall::MinigameCount { task, count });
    }
    if pop {
        let fighter = act(w).managed.map_or(-1, |m| w.fighter(m).id);
        ui(w, TutorialUiCall::RewardPop { fighter });
    }
}

// ---- the World's side

/// `startTutorial`: the tutorial level's begin play at the World's construction (set-up mode Tutorial): the game
/// mode on `World::tutorial` and `World::mode`; the actor follows from its Delay.
/// TutorialGameMode ReceiveBeginPlay (@100): Delay 0.2 (@203) then Start Tutorial. Not ported: ToggleMultiInput,
/// PandaGameStateMatchStarted and SetUIContextMode (the UI's), ActivityBegin.
pub fn start_tutorial(w: &mut World) {
    w.tutorial = Some(Box::new(TutorialRun::new()));
    w.mode = Some(ModeHooks { round_end, latent: Some(latent) });
    latent::delay(w, TutorialObj::Mode, "@203", 0.2);
}

/// A call on the game mode (its latent links).
pub fn mode_call(w: &mut World, c: Call<'_>) {
    match c {
        Call::Fired(Fired::Delay("@203")) => mode_start_tutorial(w),
        Call::Fired(Fired::Delay("@3270")) => actor_start(w),
        _ => panic!("tutorial: TutorialGameMode has no handler for {c:?}"),
    }
}

/// TutorialGameMode Start Tutorial (5065): the music switch, the actor (its BeginPlay), step 0's player at each start
/// tagged its PlayerSpawnTag (PlayerDataReadyServer(0), the team of player data 0, TutorialManagedPawn,
/// ActiveInTutorialIntro; no controller), Delay 0.2 (@3270) then the actor's Start Tutorial.
/// Unverified: the async LoadAssetClass of the actor class (@4966) is taken to finish at once.
/// Not ported: BroadcastPlayerValues (@666).
fn mode_start_tutorial(w: &mut World) {
    w.events.push(Event::Switch { fighter: -1, group: name!("mus"), value: name!("mus_tutorial") });
    let Some(MatchMode::Tutorial { actor: key, .. }) = &w.setup.mode else { panic!("tutorial: no tutorial mode") };
    let key = key.clone();
    let d = Arc::clone(&w.data);
    let Some(data) = d.tutorials.get(&key) else { panic!("tutorial: no data for {key}") };
    let a = TutorialActor {
        actor: key,
        step_index: 0,
        concept_index: 1,
        concept_count: len_i32(data.steps.iter().filter(|s| s.new_concept).count()),
        audio_step: 0,
        transitioning: false,
        pawn_controlled_by_tutorial: true,
        all_tasks_complete: true,
        over: false,
        desaturation: 0.0,
        minigame_scores: Vec::new(),
        counters: data.steps.iter().map(|s| vec![0; s.tasks.len()]).collect(),
        queued: Vec::new(),
        managed: None,
        bag: None,
        bound: Vec::new(),
        timeline: None,
        minigame_counters: data.steps.iter().map(|s| vec![0; s.tasks.len()]).collect(),
        loop_task: 0,
        step_complete: Vec::new(),
        step_cleanup: Vec::new(),
        demo_complete: Vec::new(),
        minigames: Vec::new(),
    };
    w.tutorial.as_mut().expect("the game mode").actor = Some(a);
    actor_begin_play(w);
    if let Some(s0) = data.steps.first()
        && let Some(ch) = &s0.character
    {
        let ch = w.lit(ch);
        for s in starts_with_tag(&d.stage, &s0.player_tag) {
            let (x, y) = place_of_loc(&s.loc);
            let team = data_team(w, 0);
            let f = spawn_fighter(w, ch, x, y, false, 0, team);
            act_mut(w).managed = Some(f);
            w.fighter_mut(f).tutorial.active_in_tutorial_intro = true;
        }
    }
    latent::delay(w, TutorialObj::Mode, "@3270", 0.2);
}

/// TutorialGameMode RoundEnd(Loser, BlastBox) (@3502): the World's `ModeHooks::round_end`: only a loser whose mesh is
/// visible; BlastBoxDeathFXMulticast, then the bag, the managed pawn or anything else (its root hidden,
/// SetVisibility(false, true)).
/// Simplification: the hide is the pawn's invisible flag.
/// Not ported: GetLastAttackerIndex (read by nothing that follows).
pub fn round_end(w: &mut World, f: FighterId) {
    if w.fighter(f).vis.invisible {
        return;
    }
    crate::world::blast::blast_box_death_fx(w, f);
    let (bag, managed) = actor(w).map_or((None, None), |a| (a.bag, a.managed));
    if bag == Some(f) {
        bag_out(w, f);
    } else if managed == Some(f) {
        managed_out(w, f);
    } else {
        w.fighter_mut(f).vis.invisible = true;
    }
}

/// TutorialGameMode's leftover latent pass: the World's `ModeHooks::latent`: the scheduler (the run-time solids'
/// moves among its actions), the timeline, then the bound MovementModeChanged delegates.
pub fn latent(w: &mut World) {
    crate::world::stage::runtime_solids_pass(w, latent::run);
    if actor(w).is_some() {
        timeline_tick(w);
    }
    crate::world::flow::movement_mode_tick(w);
}

/// `TutorialBus.handle` as the game state routes an emitter's event (tutorialEvent ->
/// BPI_HandleTutorialEvent on the actor). Called only while the actor runs.
/// BPI_HandleTutorialEvent (@10378): each task of the current step whose TaskCompletionEvent is the event, the step
/// re-read at each one (a completion inside the loop moves the index: the rest are the new step's, queued while it
/// transitions). Not transitioning and the player in control: a counter below the requirement counts, and the
/// "reached" branch runs when that count reaches it (a counter already there pops to the next task, @4921 / @5035);
/// transitioning: queued once per matching task; the demo driving: dropped.
/// Not ported: the analytics (@5538-7022), TimeStartedStep and the TutorialEventTriggered broadcast (no listener).
pub fn bus_event(w: &mut World, ev: TutorialEvent) {
    let d = Arc::clone(&w.data);
    if td(&d, w).steps.len() <= step_ix(w) {
        return;
    }
    let mut i = 0;
    loop {
        let si = step_ix(w);
        let Some(step) = td(&d, w).steps.get(si) else { break };
        let Some(t) = step.tasks.get(i) else { break };
        let ti = len_i32(i);
        act_mut(w).loop_task = ti;
        if TutorialEvent::from_byte(i32::from(t.event)) != ev {
            i += 1;
            continue;
        }
        let a = act(w);
        if !a.transitioning && !a.pawn_controlled_by_tutorial {
            if a.counters[si][i] >= t.req {
                i += 1;
                continue;
            }
            let a = act_mut(w);
            a.counters[si][i] += 1;
            let (count, index, total) = (a.counters[si][i], a.concept_index, a.concept_count);
            ui(w, TutorialUiCall::TaskCount { task: ti, count, index, total });
            ui(w, TutorialUiCall::Jiggle { task: ti });
            audio_step_sound(w);
            if count >= t.req {
                ui(w, TutorialUiCall::FadeOut { task: ti });
                save_score(w, ti);
                act_mut(w).all_tasks_complete = true;
                check_complete(w);
            } else {
                ui_sound(w, name!("ui_menu_confirm"));
            }
        } else if a.transitioning {
            act_mut(w).queued.push(ev);
        }
        i += 1;
    }
}

/// `TutorialBus.hitAnything`: OnHitAnything (@10819) for a pawn's HitAnything (`combat/hits.rs`
/// fighter_hit_anything): the pawn's LastAttackTutorialEvent handled, then cleared. Called only while the actor
/// runs.
pub fn hit_anything(w: &mut World, f: FighterId) {
    if !act(w).bound.contains(&f) {
        return;
    }
    let ev = w.fighter(f).tutorial.last_attack_tutorial_event.unwrap_or(TutorialEvent::None);
    bus_event(w, ev);
    w.fighter_mut(f).tutorial.last_attack_tutorial_event = Some(TutorialEvent::None);
}

// ---- TutorialActor and TutorialActor_Intro

/// ReceiveBeginPlay (8865): the respawn timer's clear bound to TutorialStepComplete, BPI_AddTutorialActor(self),
/// BPI_SetAttackDecayEnabled(false), Delay 0.2 (@8613) then the scoreboard collapsed (LoadTutorialQuestData and
/// CountTutorialConcepts: the counts are the data's). TutorialActor_Intro's: Check For Attack Decay bound and called
/// once (@308, @350); then a child class's own ReceiveBeginPlay (`classes::actor_begin_play`).
fn actor_begin_play(w: &mut World) {
    w.flow.same_attack_penalty = false;
    delay(w, "@8613", 0.2);
    if is_intro(w) {
        check_attack_decay(w);
    }
    let cls = act(w).actor.clone();
    classes::actor_begin_play(w, &cls);
}

fn is_intro(w: &World) -> bool {
    act(w).actor == INTRO
}

/// A call on the TutorialActor (its latent links).
pub fn actor_call(w: &mut World, c: Call<'_>) {
    match c {
        Call::Fired(Fired::Delay("@8613")) => ui(w, TutorialUiCall::Scoreboard { visible: false }),
        Call::Fired(Fired::Delay("@9390")) => ui(w, TutorialUiCall::Focus { on: true }),
        Call::Fired(Fired::Delay("@4311")) => step_start(w, true),
        Call::Fired(Fired::Delay("@4389")) => finish(w),
        Call::Fired(Fired::Delay("@2391")) => {
            let d = Arc::clone(&w.data);
            let has_next = td(&d, w).next.is_some();
            ui(w, TutorialUiCall::EndScreen { has_next });
        }
        Call::Fired(Fired::Timer(RESPAWN_TIMER)) => uncontrol_tutorial_pawn(w, false, false, true),
        _ => panic!("tutorial: the TutorialActor has no handler for {c:?}"),
    }
}

/// Start Tutorial (9480): the intro screen, IntroVOSoundAlias.
fn actor_start(w: &mut World) {
    ui(w, TutorialUiCall::Intro);
    let d = Arc::clone(&w.data);
    vo(w, td(&d, w).intro_vo.as_deref());
}

/// @7295-7495: a SaveMinigameScore task adds (MinigameCounterText, TaskMinigameCounter) to TutorialMinigameScores.
fn save_score(w: &mut World, task: i32) {
    let d = Arc::clone(&w.data);
    let si = step_ix(w);
    let Some(t) = td(&d, w).steps.get(si).and_then(|s| s.tasks.get(usize::try_from(task).ok()?)) else { return };
    if !t.save_score {
        return;
    }
    let a = act_mut(w);
    let score = a.minigame_counters[si][usize::try_from(task).expect("a task index")];
    a.minigame_scores.push(TutorialScore { step: a.step_index, task, score });
}

/// AudioStep: ui_tutorial_step_complete_0N, N cycling 0..5.
fn audio_step_sound(w: &mut World) {
    let n = act(w).audio_step;
    ui_sound(w, STEP_SOUNDS[usize::try_from(n).expect("0..5")]);
    act_mut(w).audio_step = if n >= 5 { 0 } else { n + 1 };
}

/// The all-complete check (@4444, @2698-3196), then the step's completion (@3197).
fn check_complete(w: &mut World) {
    let d = Arc::clone(&w.data);
    let si = step_ix(w);
    let Some(step) = td(&d, w).steps.get(si) else { return };
    let a = act_mut(w);
    let c = &a.counters[si];
    a.all_tasks_complete = step.tasks.iter().enumerate().all(|(i, t)| c[i] >= t.req);
    if a.all_tasks_complete {
        step_completed(w);
    }
}

/// @3197: AudioStepCounter 0, the index moved on; then the congratulation, the sound, TutorialIsTransitioning and
/// the broadcast, and Delay 2.3 before a new concept or 1.0 (@4311) to the step start; past the last step the
/// finish (@4370). Not ported: AddToHomescreenQueue(8) (@3207).
fn step_completed(w: &mut World) {
    let d = Arc::clone(&w.data);
    let a = act_mut(w);
    a.audio_step = 0;
    a.step_index += 1;
    let si = step_ix(w);
    let data = td(&d, w);
    if let Some(next) = data.steps.get(si) {
        let n = data.congratulations.len();
        if n > 0 {
            let text = to_int32(w.random_int(crate::real(len_i32(n))));
            ui(w, TutorialUiCall::Congrats { text });
        }
        ui_sound(w, name!("ui_tutorial_task_complete"));
        act_mut(w).transitioning = true;
        step_complete_broadcast(w);
        delay(w, "@4311", if next.new_concept { 2.3 } else { 1.0 });
        return;
    }
    step_complete_broadcast(w);
    delay(w, "@4389", 1.5);
}

/// TutorialStepComplete: ClearTutorialPlayerRespawnTimer (bound first), then the Intro's Check For Attack Decay, then
/// the spawned objects' binds.
fn step_complete_broadcast(w: &mut World) {
    latent::clear_timer(w, TutorialObj::Actor, RESPAWN_TIMER);
    if is_intro(w) {
        check_attack_decay(w);
    }
    classes::broadcast_run(w, RunList::StepComplete, &DelegateCall::StepComplete);
}

/// TutorialActor_Intro Check For Attack Decay (@2925-3096): BPI_SetAttackDecayEnabled(the current step's header
/// English is "Attack Decay"); past the last step no step matches.
fn check_attack_decay(w: &mut World) {
    let d = Arc::clone(&w.data);
    let on = td(&d, w).steps.get(step_ix(w)).is_some_and(|s| s.header.source == "Attack Decay");
    w.flow.same_attack_penalty = on;
}

/// The step start (@295; the skips enter at @347 with `full` false).
/// Deviation: the queued events are replayed from a copy of the queue taken first, then the queue is cleared: the
/// build's ForEach would also replay an event a replay queued again (a completion inside the replay), which the port
/// drops.
fn step_start(w: &mut World, full: bool) {
    let d = Arc::clone(&w.data);
    let si = step_ix(w);
    let step = td(&d, w).steps.get(si);
    let concept = step.is_some_and(|s| s.new_concept);
    if full && concept {
        step_complete_broadcast(w);
    }
    act_mut(w).transitioning = false;
    ui(w, TutorialUiCall::Cleanup);
    classes::broadcast_run(w, RunList::StepCleanup, &DelegateCall::StepCleanup);
    if let Some(s) = step
        && s.new_concept
    {
        control_tutorial_pawn(w);
        let step = act(w).step_index;
        ui(w, TutorialUiCall::Concept { step, start_disabled: s.start_button_disabled_time });
        act_mut(w).concept_index += 1;
    }
    setup_widget(w);
    vo(w, step.and_then(|s| s.vo.as_deref()));
    ui(w, TutorialUiCall::FadeIn);
    let replay = act(w).queued.clone();
    for ev in replay {
        bus_event(w, ev);
    }
    act_mut(w).queued.clear();
}

fn setup_widget(w: &mut World) {
    let a = act(w);
    let (step, index, count) = (a.step_index, a.concept_index, a.concept_count);
    ui(w, TutorialUiCall::Setup { step, index, count });
}

/// TutorialActorTimeline's track: linear from (-0.00125, 0.00118) to (0.25, 1).
fn track(t: f64) -> Real {
    narrow(0.00118 + ((t + 0.00125) * (1.0 - 0.00118)) / (0.25 + 0.00125))
}

/// SaturateEnvironment: TutorialActorTimeline ReverseFromEnd (@10262; its update at the end's value at once).
fn saturate(w: &mut World) {
    let a = act_mut(w);
    a.timeline = Some(Timeline { t: 0.25, dir: -1 });
    a.desaturation = track(0.25);
}

/// The timeline's tick, in the mode's latent pass, to its end in either direction.
/// Unverified: the timeline component's tick (TG_PrePhysics of the actor) is taken at the leftover pass.
/// The position is float32 (FTimeline's float); the step is summed in f64 and narrowed.
#[allow(
    clippy::manual_clamp,
    reason = "Math.min(Math.max(t, 0), 0.25) summed in f64, narrowed to the timeline's float"
)]
fn timeline_tick(w: &mut World) {
    let a = act_mut(w);
    let Some(tl) = a.timeline else { return };
    let t = f64::from(tl.t) + f64::from(tl.dir) * f64::from(DELTA_SECONDS);
    let t = if t <= 0.0 {
        0.0
    } else if t > 0.25 {
        0.25
    } else {
        t
    };
    a.desaturation = track(t);
    let done = if tl.dir < 0 { t <= 0.0 } else { t >= 0.25 };
    a.timeline = if done { None } else { Some(Timeline { t: narrow(t), dir: tl.dir }) };
}

/// The managed pawn's teardown (@213-490): PlayerState, SetLifeSpan(0), brain, UnPossess, K2_DestroyActor.
fn drop_managed(w: &mut World) {
    let m = act(w).managed;
    if let (true, Some(m)) = (callable(w, m), m) {
        pawns::despawn_pawn(w, m, true);
    }
}

/// The starts tagged `tag` as sim places.
fn starts(w: &World, tag: &str) -> Vec<(Real, Real)> {
    starts_with_tag(&w.data.stage, tag).into_iter().map(|s| place_of_loc(&s.loc)).collect()
}

/// ControlTutorialPawn (the FirstTime pin is read nowhere): the step's PlayerClass at each PlayerStart tagged its
/// PlayerSpawnTag, ActiveInTutorialIntro, DisableCD from the step, AIControllerClass = DemoAIController,
/// SpawnDefaultController and Possess (@1881, @2044: the controller's ReceiveUnPossess between its two
/// possessions), PawnControlledByTutorial; the bag torn down last (@2330-2800).
/// Not ported: UIManager SetHandleInput (the UI's); the RTPC tutorial_controlled_pawn is the UI's, from
/// `pawn_controlled_by_tutorial`.
fn control_tutorial_pawn(w: &mut World) {
    let d = Arc::clone(&w.data);
    let step = td(&d, w).steps.get(step_ix(w));
    saturate(w);
    drop_managed(w);
    if let Some(step) = step {
        for at in starts(w, &step.player_tag) {
            let team = data_team(w, 0);
            let Some(f) = spawn_player(w, step, at, team) else { continue };
            if let Some(id) = classes::make_demo(w, step.demo.as_deref()) {
                pawns::possess(w, f, super::demo::ctl(id));
                pawns::possess(w, f, super::demo::ctl(id));
            }
        }
    }
    act_mut(w).pawn_controlled_by_tutorial = true;
    let bag = act(w).bag;
    if let (true, Some(bag)) = (callable(w, bag), bag) {
        pawns::despawn_pawn(w, bag, true);
    }
}

/// The player pawn of a step at a start: spawned FacingLeft false, PlayerDataReadyServer(0), the team,
/// ActiveInTutorialIntro, TutorialManagedPawn, DisableCD.
fn spawn_player(w: &mut World, step: &TutorialStep, at: (Real, Real), team: i32) -> Option<FighterId> {
    let Some(ch) = &step.character else {
        warn_text(w, &format!("tutorial: the player class {} is no fighter of the port", step.player));
        return None;
    };
    let ch = w.lit(ch);
    let f = spawn_fighter(w, ch, at.0, at.1, false, 0, team);
    w.fighter_mut(f).tutorial.active_in_tutorial_intro = true;
    act_mut(w).managed = Some(f);
    w.fighter_mut(f).abilities.disable_cd = step.disable_cooldowns;
    Some(f)
}

/// UncontrolTutorialPawn(SpawnCustomActors, SpawnPunchingBag, IsDeath): not on a death TutorialDemoComplete first
/// (@5 / @2632), the pawn again at the step's starts for player controller 0 (TeamIndex 0 literal @1349,
/// OnHitAnything bound @1475, the game mode's PlayerRespawned(0) after each, @2585); on a death that is all (@2656);
/// else the bags (@3351), the custom actors (@4030) and then the minigame counters (@2717-3346: the flow pushed at
/// @522 runs them after @4030 / @4202).
/// Not ported: SetCanPause and BPI_EnableGameInput (the UI's).
fn uncontrol_tutorial_pawn(w: &mut World, spawn_custom: bool, spawn_bag: bool, is_death: bool) {
    let d = Arc::clone(&w.data);
    let si = step_ix(w);
    let step = td(&d, w).steps.get(si);
    if !is_death {
        classes::broadcast_run(w, RunList::DemoComplete, &DelegateCall::DemoComplete);
    }
    saturate(w);
    drop_managed(w);
    let Some(step) = step else { return };
    for at in starts(w, &step.player_tag) {
        let Some(f) = spawn_player(w, step, at, 0) else { continue };
        act_mut(w).bound.push(f);
        pawns::possess(w, f, PawnCtl::Player);
        act_mut(w).pawn_controlled_by_tutorial = false; // (@2244, ahead of the broadcast @2585)
        classes::broadcast_run(w, RunList::PlayerRespawned, &DelegateCall::PlayerRespawned { index: 0 });
    }
    act_mut(w).pawn_controlled_by_tutorial = false;
    if is_death {
        return;
    }
    if spawn_bag && step.spawn_target_dummy {
        for at in starts(w, &step.bag_tag) {
            spawn_punching_bag(w, at);
        }
    }
    if spawn_custom {
        let si = step_ix(w);
        spawn_custom_actors(w, step, si);
    }
    for (i, t) in step.tasks.iter().enumerate() {
        if t.minigame_counter {
            // (the counter's text by its source string, minigameText; the UI finds the locres key on the
            // step's task)
            let text = w.lit(&t.minigame_text.source);
            ui(w, TutorialUiCall::MinigameCounter { task: len_i32(i), text, visible: true });
        }
    }
}

/// @4030-5026: each task's CustomTutorialActors in order, spawned deferred at the origin (FinishSpawningActor runs
/// the actor's BeginPlay), then BPI_SetupTutorialMinigameActor(self, TutorialStepIndex, the task's index).
fn spawn_custom_actors(w: &mut World, step: &TutorialStep, si: usize) {
    let step_index = len_i32(si);
    for (task, t) in step.tasks.iter().enumerate() {
        for cls in &t.custom_actors {
            let Some(id) = classes::make_minigame(w, cls) else { continue };
            classes::call(w, TutorialObj::Minigame(id), Call::BeginPlay);
            if classes::minigame_of(w, id).is_some_and(|m| !m.destroyed) {
                minigame::setup(w, id, step_index, len_i32(task));
            }
        }
    }
}

/// SpawnPunchingBag(Location): the step's DummyClass FacingLeft, PlayerDataReadyServer(1), the team of player data
/// 1, CurrentDamage = PunchingBagPercent, then its controller (BPI_SetDifficultyLevel(2), Possess) and
/// RemoveInvulnerability. Not ported: bUseControllerRotationYaw and the controller's SetRotation(true).
fn spawn_punching_bag(w: &mut World, at: (Real, Real)) {
    let d = Arc::clone(&w.data);
    let step = td(&d, w).steps.get(step_ix(w));
    let Some(step) = step.filter(|s| s.dummy.is_some()) else {
        act_mut(w).bag = None;
        return;
    };
    if act(w).transitioning {
        return;
    }
    let Some(ch) = &step.dummy_character else {
        let dummy = step.dummy.as_deref().unwrap_or_default();
        warn_text(w, &format!("tutorial: the dummy class {dummy} is no fighter of the port"));
        act_mut(w).bag = None;
        return;
    };
    let ch = w.lit(ch);
    let team = data_team(w, 1);
    let f = spawn_fighter(w, ch, at.0, at.1, true, 1, team);
    w.fighter_mut(f).stats.damage = step.bag_percent;
    act_mut(w).bag = Some(f);
    if let Some(c) = step.dummy_controller.as_deref() {
        match dummy_controller(c) {
            Some(ctl) => possess_bot(w, f, ctl),
            None => warn_text(w, &format!("tutorial: the dummy controller {c} is not ported")),
        }
    }
    let iv = &mut w.fighter_mut(f).invuln;
    iv.invuln_count = (iv.invuln_count - 1).max(0);
}

/// TutorialGameMode RoundEnd for the bag: RingOut handled, the teardown, a bag again at step 0's PunchBagSpawnTag.
fn bag_out(w: &mut World, f: FighterId) {
    bus_event(w, TutorialEvent::RingOut);
    pawns::despawn_pawn(w, f, true);
    let d = Arc::clone(&w.data);
    if let Some(s0) = td(&d, w).steps.first() {
        for at in starts(w, &s0.bag_tag) {
            spawn_punching_bag(w, at);
        }
    }
}

/// TutorialGameMode RoundEnd for the managed pawn: destroyed, TutorialPlayerRespawn 1.0 s later.
fn managed_out(w: &mut World, f: FighterId) {
    if act(w).over {
        return;
    }
    pawns::despawn_pawn(w, f, false);
    latent::set_timer(w, TutorialObj::Actor, RESPAWN_TIMER, 1.0, false);
}

/// The finish (@2076): StepCleanup, the widget removed, ui_match_complete, attack decay on, TutorialOver, the managed
/// pawn's BPI_DisableInput, FinishTutorial (every actor's: the UI saves the Intro's or the class's complete key,
/// `Finished`); Delay 0.2 then the end screen.
/// Simplification: BPI_DisableInput takes the pawn's input away (its controller's entry set to none).
/// Deviation: the waits for a modal and for the pause (@2446, @70) are the UI's; the world does not step while
/// paused.
fn finish(w: &mut World) {
    ui(w, TutorialUiCall::Cleanup);
    classes::broadcast_run(w, RunList::StepCleanup, &DelegateCall::StepCleanup);
    ui(w, TutorialUiCall::RemoveWidget);
    ui_sound(w, name!("ui_match_complete"));
    w.flow.same_attack_penalty = true;
    act_mut(w).over = true;
    let m = act(w).managed;
    if let (true, Some(m)) = (callable(w, m), m)
        && w.pawn_ctl.ctl.contains_key(&m)
    {
        w.pawn_ctl.ctl.insert(m, PawnCtl::None);
    }
    let actor = w.lit(&act(w).actor);
    ui(w, TutorialUiCall::Finished { actor });
    delay(w, "@2391", 0.2);
}

// ---- the UI's calls

/// The intro screen's BPI_IntroComplete (10044): Desaturation 1.0 set directly, the TutorialWidget, then step 0
/// without the step start's concept card (@9104-9390).
pub fn intro_complete(w: &mut World) {
    if actor(w).is_none() {
        return;
    }
    let a = act_mut(w);
    a.timeline = None;
    a.desaturation = 1.0;
    ui(w, TutorialUiCall::Widget);
    let a = act_mut(w);
    a.step_index = 0;
    a.audio_step = 0;
    setup_widget(w);
    control_tutorial_pawn(w);
    ui(w, TutorialUiCall::Focus { on: false });
    let d = Arc::clone(&w.data);
    let s0 = td(&d, w).steps.first();
    vo(w, s0.and_then(|s| s.vo.as_deref()));
    delay(w, "@9390", s0.map_or(0.0, |s| s.start_button_disabled_time));
}

/// The concept card's start button: UncontrolTutorialPawn(true, true, false).
pub fn start_pressed(w: &mut World) {
    if actor(w).is_some() {
        uncontrol_tutorial_pawn(w, true, true, false);
    }
}

/// The widget's skip button: BPI_AdvanceTutorial (@10295 -> @7295): the score save of the handle loop's last task
/// index, AllTasksComplete, then the all-complete check.
pub fn advance(w: &mut World) {
    if actor(w).is_none() {
        return;
    }
    let t = act(w).loop_task;
    save_score(w, t);
    act_mut(w).all_tasks_complete = true;
    check_complete(w);
}

/// The pause menu's Skip Step: SkipTutorialStep (@10824): to the next step that starts a new concept, its broadcast
/// and the step start @347.
pub fn skip_step(w: &mut World) {
    if actor(w).is_none() {
        return;
    }
    let d = Arc::clone(&w.data);
    let n = len_i32(td(&d, w).steps.len());
    while act(w).step_index < n - 1 {
        act_mut(w).step_index += 1;
        if td(&d, w).steps[step_ix(w)].new_concept {
            step_complete_broadcast(w);
            step_start(w, false);
            return;
        }
    }
}

/// The pause menu's BPI_SkipToTutorialEndStep (@11223): the last step, the step start @347.
pub fn skip_to_end(w: &mut World) {
    if actor(w).is_none() {
        return;
    }
    let d = Arc::clone(&w.data);
    act_mut(w).step_index = len_i32(td(&d, w).steps.len()) - 1;
    step_start(w, false);
}

/// The end screen's next button: BPI_StartNextTutorial. Not ported: TutorialGameMode Start Next Tutorial (the Intro
/// has no NextTutorial).
pub fn start_next(w: &mut World) {
    if actor_data(w).is_some_and(|d| d.next.is_some()) {
        warn_text(w, "tutorial: Start Next Tutorial is not ported");
    }
}

/// BPI_DesaturateEnvironment (UI_TutorialEndScreen Init @2990; @8835): DesaturateEnvironment,
/// TutorialActorTimeline PlayFromStart (@10229; its update at the start's value at once).
pub fn desaturate_environment(w: &mut World) {
    if actor(w).is_none() {
        return;
    }
    let a = act_mut(w);
    a.timeline = Some(Timeline { t: 0.0, dir: 1 });
    a.desaturation = track(0.0);
}

// ---- the getters (`TutorialRun`'s readonly fields); defaults before the actor exists

/// The running actor's data (`TutorialRun.data`).
pub fn actor_data(w: &World) -> Option<&TutorialActorData> {
    actor(w).and_then(|a| w.data.tutorials.get(&a.actor))
}

pub fn step_index(w: &World) -> i32 {
    actor(w).map_or(0, |a| a.step_index)
}

pub fn over(w: &World) -> bool {
    actor(w).is_some_and(|a| a.over)
}

/// MPC TutorialEnvironmentDesat Desaturation.
pub fn desaturation(w: &World) -> Real {
    actor(w).map_or(0.0, |a| a.desaturation)
}

/// The RTPC tutorial_controlled_pawn's source: true while the demo drives (CDO true); `None` before the actor
/// exists (the UI then sets neither the RTPC nor the held input).
pub fn pawn_controlled_by_tutorial(w: &World) -> Option<bool> {
    actor(w).map(|a| a.pawn_controlled_by_tutorial)
}

/// BPI_IsSkippable: not while the demo drives or the step transitions; true when a later step starts a concept.
pub fn is_skippable(w: &World) -> bool {
    let (Some(a), Some(d)) = (actor(w), actor_data(w)) else { return false };
    if a.pawn_controlled_by_tutorial || a.transitioning {
        return false;
    }
    let from = usize::try_from(a.step_index + 1).unwrap_or(0);
    d.steps.iter().skip(from).any(|s| s.new_concept)
}

/// BPI_GetTutorialMinigameScores.
pub fn minigame_scores(w: &World) -> &[TutorialScore] {
    actor(w).map_or(&[], |a| a.minigame_scores.as_slice())
}
