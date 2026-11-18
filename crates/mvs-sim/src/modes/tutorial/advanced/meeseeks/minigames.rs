//! The Meeseeks minigame actors: TutorialMinigame_AerialCombat
//! (TutorialActor_AerialCombat step 1) and TutorialMinigame_PositioningTools (TutorialActor_DashAttack step 0, its task's
//! minigame counter saved as the score).

use crate::Real;
use crate::State;
use crate::actors::ActorRef;
use crate::consts::DELTA_SECONDS;
use crate::fighter::match_side::TutorialEvent;
use crate::math::{max, min, narrow};
use crate::name;
use crate::world::World;

use super::super::super::classes::{self, Call, Fired, RunList, minigame_mut};
use super::super::super::minigame::{self, MinigameState};
use super::super::super::pawn::{actors_with_tag, sound_at};
use super::super::super::{TutorialObj, latent};
use super::actor::{self, MeeseeksEnd};

/// TutorialMinigame_AerialCombat.
/// - ReceiveBeginPlay: Delay 1.0 (@2076, to @410): Spawn Off Ledge Fighter (@2285): for each actor with the tag
///   AerialCombat (GetAllActorsWithTag) a Tutorial_Meeseeks at its transform (@1349-1500) with TutorialMinigame_Actor =
///   self, TeamIndex 1, GravityScale 0, HitByEnemy += Meeseeks Hit, ConvertToFlyingMeeseeks (@1538-1850); then
///   TargetCount = the tagged actors' count (@2020). Then (@424-912) TutorialStepCleanup += CleanUpAerialMeeseeks.
/// - Meeseeks Hit (@2393): TargetCount - 1; at 0 End Game (@2131): BPI_ProcessTutorialEvent(28), K2_DestroyActor.
/// - CleanUpAerialMeeseeks (@15): K2_DestroyActor on every Tutorial_Meeseeks (no effect; not on self).
#[derive(State, Debug, Default)]
pub struct AerialCombat {
    /// TargetCount.
    pub targets: i32,
}

/// TutorialMinigame_PositioningTools (class defaults MeeseeksSurvivalTime 1.2, PositioningToolsMinTime 15).
/// - ReceiveBeginPlay (@2334): PositioningToolsStartTime = GetGameTimeInSeconds; Meeseeks Timer (@1569: a looping 2.0 s
///   timer to Spawn Meeseeks, MeeseeksSpawnTimer); a looping 10.0 s timer to Revise Difficulty (@2032); then
///   TutorialStepComplete += CleanUpPositioningMinigame and TutorialStepCleanup += PositioningToolsMinigameCleanup (@1204,
///   @1268). Deviation: the base class binds the two at the start of BeginPlay (`minigame.rs`), ahead of the timers; no
///   broadcast can come in between.
/// - Spawn Meeseeks (@2647): the first BaseFighter's location Y > 0 picks (0, -600, 0), else (0, 600, 0) (the side away
///   from it); TutorialMinigame_Actor = self, HitByEnemy += Meeseeks Hit, TeamIndex 1, a one-shot timer of
///   MeeseeksSurvivalTime to Despawn Meeseeks (Despawn Timer, replaced by each spawn), the cast sound at it (@4089).
/// - Despawn Meeseeks (@4374): the last spawned one, when valid: PS_MeeseeksSpawn (Team 1) at it, K2_DestroyActor.
/// - Meeseeks Hit (@4268): MeeseeksCaught + 1; both timers cleared (@4168-4188); Delay 0.3 (@4208, to @410): unless
///   MinigameOver, Meeseeks Timer and Spawn Meeseeks now. Then a DoOnce (@1673): a one-shot timer of
///   FClamp(PositioningToolsMinTime - (now - StartTime), 1, PositioningToolsMinTime) to End Game (@1894).
/// - Revise Difficulty: Stage + 1; stage 1 sets MeeseeksSurvivalTime 0.9 (@4816), stage 2 0.75 (@4840).
/// - End Game (@2409): MeeseeksSpawnTimer cleared, MinigameOver, BPI_ProcessTutorialEvent(28), every Tutorial_Meeseeks
///   PS_MeeseeksSpawn (Team 1) and K2_DestroyActor (@454-750), K2_DestroyActor on self (@999).
/// - CleanUpPositioningMinigame (StepComplete, @4864-4924): the four timers cleared. PositioningToolsMinigameCleanup
///   (StepCleanup, @15): every Tutorial_Meeseeks destroyed, no effect.
///
/// The score is the task's minigame counter, raised by each Meeseeks' own On Hit.
#[derive(State, Debug)]
pub struct PositioningTools {
    /// MeeseeksSurvivalTime.
    pub survival: Real,
    /// PositioningToolsMinTime.
    pub min_time: Real,
    /// PositioningToolsStartTime.
    pub start: Real,
    pub stage: i32,
    /// MeeseeksCaught.
    pub caught: i32,
    /// MinigameOver.
    pub over: bool,
    /// The DoOnce's IsClosed.
    pub once: bool,
    /// FinishSpawningActor's last return.
    pub last: Option<ActorRef>,
}

impl Default for PositioningTools {
    fn default() -> Self {
        PositioningTools {
            survival: 1.2,
            min_time: 15.0,
            start: 0.0,
            stage: 0,
            caught: 0,
            over: false,
            once: false,
            last: None,
        }
    }
}

/// A call on one of this family's minigame actors.
pub fn call(w: &mut World, id: u32, c: Call<'_>) {
    // (neither class has an own ReceiveEndPlay)
    if matches!(c, Call::Destroyed) {
        return;
    }
    let Some(m) = classes::minigame_of(w, id) else { return };
    match &m.state {
        MinigameState::AerialCombat(_) => aerial_combat(w, id, c),
        MinigameState::PositioningTools(_) => positioning_tools(w, id, c),
        _ => unreachable!("a minigame of another family"),
    }
}

fn unhandled(w: &World, id: u32, c: Call<'_>) {
    let cls = classes::minigame_of(w, id).map_or("(gone)", |m| m.cls);
    panic!("tutorial: {cls} has no handler for {c:?}");
}

fn me(id: u32) -> TutorialObj {
    TutorialObj::Minigame(id)
}

/// `gameTime`: GetGameTimeInSeconds: the world clock.
/// Unverified: taken as the step count times the fixed delta (the build sums each frame's float delta).
fn game_time(w: &World) -> Real {
    narrow(f64::from(w.flow.tick) * f64::from(DELTA_SECONDS))
}

// ---- AerialCombat

fn aerial_combat(w: &mut World, id: u32, c: Call<'_>) {
    match c {
        Call::BeginPlay => latent::delay(w, me(id), "BeginPlay", 1.0),
        Call::Fired(Fired::Delay("BeginPlay")) => {
            // (sim x = -UE Y, y = UE Z; the transform's rotation and scale reach nothing the port draws from the sim)
            let tagged: Vec<(f64, f64)> =
                actors_with_tag(&w.data.stage, "AerialCombat").iter().map(|a| (a.loc[1], a.loc[2])).collect();
            for &(ue_y, ue_z) in &tagged {
                // (FinishSpawningActor's None: the writes on it do nothing)
                let Some(a) = actor::spawn(w, narrow(-ue_y), narrow(ue_z)) else { continue };
                actor::set_minigame(w, a, id);
                actor::bind_hit(w, a, me(id), "meeseeksHit");
                actor::convert_to_flying(w, a);
            }
            if let MinigameState::AerialCombat(s) = state_of(w, id) {
                s.targets = i32::try_from(tagged.len()).expect("a count");
            }
            classes::bind_run(w, RunList::StepCleanup, me(id), "cleanUp");
        }
        Call::Delegate { handler: "meeseeksHit", .. } => {
            let MinigameState::AerialCombat(s) = state_of(w, id) else { unreachable!() };
            s.targets -= 1;
            if s.targets != 0 {
                return;
            }
            minigame::process_event(w, TutorialEvent::CustomActorEvent);
            minigame::destroy(w, id);
        }
        // CleanUpAerialMeeseeks
        Call::Delegate { handler: "cleanUp", .. } => clean_up(w),
        _ => unhandled(w, id, c),
    }
}

fn state_of(w: &mut World, id: u32) -> &mut MinigameState {
    &mut minigame_mut(w, id).expect("a live minigame").state
}

/// K2_DestroyActor on every Tutorial_Meeseeks, no effect.
fn clean_up(w: &mut World) {
    for a in actor::live(w) {
        actor::destroy(w, a, MeeseeksEnd::Plain);
    }
}

// ---- PositioningTools

fn pt(w: &mut World, id: u32) -> &mut PositioningTools {
    match state_of(w, id) {
        MinigameState::PositioningTools(s) => s,
        _ => unreachable!(),
    }
}

fn positioning_tools(w: &mut World, id: u32, c: Call<'_>) {
    match c {
        Call::BeginPlay => {
            minigame::bind_steps(w, id, true, true);
            let now = game_time(w);
            pt(w, id).start = now;
            spawn_timer(w, id);
            latent::set_timer(w, me(id), "Revise Difficulty", 10.0, true);
        }
        Call::Fired(Fired::Timer("Spawn Meeseeks")) => spawn(w, id),
        Call::Fired(Fired::Timer("Revise Difficulty")) => {
            let s = pt(w, id);
            s.stage += 1;
            if s.stage == 1 {
                s.survival = 0.9;
            } else if s.stage == 2 {
                s.survival = 0.75;
            }
        }
        Call::Fired(Fired::Timer("Despawn Meeseeks")) => {
            let last = pt(w, id).last;
            if actor::valid(w, last)
                && let Some(last) = last
            {
                actor::destroy(w, last, MeeseeksEnd::Poof);
            }
        }
        Call::Fired(Fired::Timer("End Game")) => end_game(w, id),
        Call::Fired(Fired::Delay("Meeseeks Hit")) => {
            if pt(w, id).over {
                return;
            }
            spawn_timer(w, id);
            spawn(w, id);
        }
        Call::Delegate { handler: "meeseeksHit", .. } => meeseeks_hit(w, id),
        // CleanUpPositioningMinigame
        Call::Delegate { handler: "onStepComplete", .. } => {
            for t in ["Spawn Meeseeks", "Despawn Meeseeks", "Revise Difficulty", "End Game"] {
                latent::clear_timer(w, me(id), t);
            }
        }
        // PositioningToolsMinigameCleanup
        Call::Delegate { handler: "onStepCleanup", .. } => clean_up(w),
        _ => unhandled(w, id, c),
    }
}

/// Meeseeks Timer.
fn spawn_timer(w: &mut World, id: u32) {
    latent::set_timer(w, me(id), "Spawn Meeseeks", 2.0, true);
}

/// Spawn Meeseeks (@2647).
fn spawn(w: &mut World, id: u32) {
    // (GetAllActorsOfClass(BaseFighter)[0]: the first live pawn; none: Array_Get's None, its location zero)
    let first = w.pawns.as_ref().and_then(|ps| ps.iter().copied().find(|&p| w.fighter(p).life.dead != Real::INFINITY));
    let ue_y = first.map_or(0.0, |f| -w.fighter(f).body.x);
    let a = actor::spawn(w, if ue_y > 0.0 { 600.0 } else { -600.0 }, 0.0);
    pt(w, id).last = a;
    if let Some(a) = a {
        actor::set_minigame(w, a, id);
        actor::bind_hit(w, a, me(id), "meeseeksHit");
    }
    let survival = pt(w, id).survival;
    latent::set_timer(w, me(id), "Despawn Meeseeks", survival, false);
    if let Some(ActorRef::Stage(i)) = a {
        let (x, y) = {
            let s = &w.stage_run.as_ref().expect("a stage run").actors[usize::try_from(i).expect("an index")];
            (s.x, s.y)
        };
        sound_at(w, name!("sfx_cha_tutorial_meeseeks_cast"), x, y);
    }
}

/// Meeseeks Hit (@4268).
fn meeseeks_hit(w: &mut World, id: u32) {
    pt(w, id).caught += 1;
    latent::clear_timer(w, me(id), "Spawn Meeseeks");
    latent::clear_timer(w, me(id), "Despawn Meeseeks");
    latent::delay(w, me(id), "Meeseeks Hit", 0.3);
    if pt(w, id).once {
        return;
    }
    pt(w, id).once = true;
    let (start, min_time) = (pt(w, id).start, pt(w, id).min_time);
    let left = min_time - (game_time(w) - start);
    latent::set_timer(w, me(id), "End Game", min(max(left, 1.0), min_time), false);
}

/// End Game (@2409).
fn end_game(w: &mut World, id: u32) {
    latent::clear_timer(w, me(id), "Spawn Meeseeks");
    pt(w, id).over = true;
    minigame::process_event(w, TutorialEvent::CustomActorEvent);
    for a in actor::live(w) {
        actor::destroy(w, a, MeeseeksEnd::Poof);
    }
    minigame::destroy(w, id);
}
