//! Catch Up: the game mode's CatchUpSpeedBuffEnabled flag, every player's catch-up loop on its Comp_State_Manager and
//! Buff_CatchUpSpeedBuff's own graph. The decode is the build's.
//!
//! - The flag: PandaGameMode BPI_SetMatchStarted, past its BPI_GetGameModeType() != 3 test (@2874-2928: the Lab's
//!   TrainGameMode stops there), binds EnableCatchUpSpeedBuff (@4590) and sets a one-shot timer of FMax(Duration -
//!   120, 0) (@4891-5095) whose handle nothing keeps. A time of 0 arms nothing, so a match of 120 s or less (the
//!   lobby's unlimited 0 included) never turns it on. EnableCatchUpSpeedBuff (ubergraph @20285) sets
//!   CatchUpSpeedBuffEnabled; nothing clears it. FFAGameMode's BPI_SetMatchStarted runs the parent's first.
//! - The loop: Comp_State_Manager's BeginPlay (@4674-4792) binds HandleLoserChaseSpeedBuff to its owner's
//!   BFPlayerDataReady, which BaseFighter broadcasts in PlayerDataReadyServer (@158000), called by the game mode's
//!   SpawnCharacter (@2209) at the first spawn only (RespawnCharacter reuses the pawn): a player's, a minion never.
//!   HandleLoserChaseSpeedBuff (@6851) binds RemoveCatchUpSpeedBuff to the owner's BFHitPlayer (@6916 / @6939) and
//!   starts RetriggerableDelay(25.0) (@848). Each expiry (@39-786): the game mode's BPI_IsCatchUpSpeedBuffEnabled;
//!   on, BPI_GetWinners (@299: GetWinners, the time-up ranking) not empty and without the owner's team (@519-752):
//!   SpawnBuffActor(Buff_CatchUpSpeedBuff_C, the owner, the owner, 0 stacks, 0.0 s, BreakIfBlocked) (@786); then
//!   RetriggerableDelay(25.0) again whatever the branch (@848). A ring-out neither stops nor restarts it.
//! - BFHitPlayer: Comp_Pawn_HitBoxHandler.pfgDoDamage broadcasts it once a hit went through (LocalHasHit @21522) on
//!   an actor that implements BPI_BaseFighter (@21551-21589), on both paths of the BlockHitEnemyDispatch test
//!   (@21801; the other rejoins at @21815): on the handler's owner when that is a BP_FighterCharacter (@22038), else
//!   on its owner's owner when that is one (a spawned actor's handler, @22407-22653), right after
//!   ElementalBuffHitEnemy (@21835) and ahead of HitAnything (@22208). RemoveCatchUpSpeedBuff (@6642): CheckHasBuff,
//!   a live one destroyed (@6790), then ApplyCatchUpSpeedBuff (@6826 -> @848): the 25 s start over
//!   (execRetriggerableDelay resets TimeRemaining).
//! - Buff_CatchUpSpeedBuff (BuffParentActor's child; SpeedMod 1.3, SFX_Loop_Start
//!   sfx_shared_combat_speed_buff_start, the parent's DestroyBuffWhenOwnerDefeated): ReceiveBeginPlay runs the
//!   parent's (@10), then UI_Functions PopText (@549, not queued) of "Catch Up" on its owner. No life timer (the
//!   spawn's 0.0 s), and no override, stack or refresh: a spawn while one lives is refused by SweepBuffState.

use crate::Real;
use crate::State;
use crate::combat::buffs;
use crate::combat::pop_text::{GraphPopText, pop_text_on};
use crate::combat::{BuffClass, BuffRef};
use crate::consts::DELTA_SECONDS;
use crate::fighter::FighterId;
use crate::name;
use crate::registry::entries::BuffHooks;
use crate::registry::{BuffEntry, Registry, RegistryError};
use crate::world::{World, blast, flow};

/// BPI_SetMatchStarted @4891: the flag's timer is the match duration less this.
const LEAD: Real = 120.0;

/// Comp_State_Manager @848: RetriggerableDelay(25.0).
const DELAY: Real = 25.0;

/// The class the loop spawns.
const CLASS: &str = "Buff_CatchUpSpeedBuff_C";

/// The catch-up state of the game mode and of every player's Comp_State_Manager.
// Deviation: the trace record has no row for it: a World loaded from a trace frame has
// the delays of a fresh match and the flag off.
#[derive(State, Debug, Default)]
pub struct CatchUp {
    /// The world tick EnableCatchUpSpeedBuff's timer fires at: none before the match starts, without a timer, or
    /// once it fired.
    pub due: Option<i32>,
    /// PandaGameMode CatchUpSpeedBuffEnabled.
    pub on: bool,
    /// Each player's RetriggerableDelay(25.0): its TimeRemaining (s), by player index.
    pub delays: Vec<Real>,
}

/// A new match's (`reset`): the flag off, no timer, and every player's delay started, as at its first spawn's
/// PlayerDataReadyServer.
/// Unverified: the spawn is taken as the world's first tick, so the first expiry is 25 s of world time from there.
pub fn reset(w: &mut World) {
    let n = w.players.len();
    let c = &mut w.flow.catch_up;
    c.due = None;
    c.on = false;
    c.delays.clear();
    c.delays.resize(n, DELAY);
}

/// BPI_SetMatchStarted's timer (`start_match`): EnableCatchUpSpeedBuff `duration` - 120 s from now, none when that is
/// not above 0.
pub fn arm(w: &mut World, duration: Real) {
    let d = duration - LEAD;
    if d > 0.0 {
        w.flow.catch_up.due = Some(w.flow.tick + blast::js_ticks(d));
    }
}

/// The timer pass (`match_tick`, with the game mode's other timers): EnableCatchUpSpeedBuff once due.
pub fn timer(w: &mut World) {
    let c = &mut w.flow.catch_up;
    if c.due.is_some_and(|at| at <= w.flow.tick) {
        c.due = None;
        c.on = true;
    }
}

/// The player index of `f` (a minion has none: no PlayerDataReadyServer, no loop).
fn index(w: &World, f: FighterId) -> Option<usize> {
    w.players.iter().position(|&p| p == f)
}

/// The Comp_State_Manager's latent pass: the delay's TimeRemaining less the frame, and at its end the check (@39-786)
/// and RetriggerableDelay(25.0) again (@848).
// Unverified: the delay set again inside its own expiry counts from the next frame (25 s a cycle, as the first).
pub fn latent(w: &mut World, f: FighterId) {
    let Some(i) = index(w, f) else { return };
    let Some(t) = w.flow.catch_up.delays.get_mut(i) else { return };
    *t -= DELTA_SECONDS;
    if *t > 0.0 {
        return;
    }
    *t = DELAY;
    if !w.flow.catch_up.on {
        return;
    }
    let winners = flow::winners(w);
    let team = w.fighter(f).team.team;
    if !winners.is_empty() && !winners.contains(&team) {
        buffs::spawn_buff(w, f, name!("Buff_CatchUpSpeedBuff_C"), 0.0, Some(0), Some(f), true, None);
    }
}

/// BFHitPlayer on `f` (the fighter whose handler's hit went through, or the owner of the spawned actor whose did):
/// RemoveCatchUpSpeedBuff. A live buff is destroyed and the delay starts over.
pub fn hit_player(w: &mut World, f: FighterId) {
    let Some(i) = index(w, f) else { return };
    if let Some(b) = buffs::buff_of(w, f, name!("Buff_CatchUpSpeedBuff_C")) {
        buffs::end_buff(w, f, b);
    }
    if let Some(t) = w.flow.catch_up.delays.get_mut(i) {
        *t = DELAY;
    }
}

/// Buff_CatchUpSpeedBuff @40-549: "Catch Up" (its Game.locres key), lifetime 1.0, text colour (1, 1, 0.3, 1), team
/// colour 0, location (0, 0, 0), space 3, scale 1, centred, no delay, no icons or audio, attached to the owner,
/// UI_Functions PopText at once.
fn pop() -> GraphPopText {
    GraphPopText {
        text_key: name!("FF1C98D945275C70458F999E995F211F"),
        source_string: None,
        localized_string: None,
        lifetime: 1.0,
        color: [1.0, 1.0, 0.3, 1.0],
        location: [0.0, 0.0, 0.0],
        space: 3,
        scale: 1.0,
        justification: 1,
        delay: 0.0,
        audio: None,
        queued: false,
        local_only: false,
        args: Vec::new(),
    }
}

/// Buff_CatchUpSpeedBuff's ReceiveBeginPlay after the parent's: the pop text on its owner.
fn begin(w: &mut World, f: FighterId, _b: BuffRef, _from: Option<FighterId>, _dur: Option<Real>) {
    pop_text_on(w, f, &pop());
}

/// The buff's entry.
pub fn register(r: &mut Registry) -> Result<(), RegistryError> {
    let hooks = BuffHooks { begin: Some(begin), ..BuffHooks::default() };
    r.buffs.enter(CLASS, BuffEntry::new(BuffClass::BuffCatchUpSpeedBuff, hooks))
}
