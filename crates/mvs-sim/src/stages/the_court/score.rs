//! The Court's shot clock and scoreboard (M010_ShotClock_C SJAM_ShotClock_5 of the VFX
//! level, M010_JumboTron_C of the gameplay level).
//! No game score: presentation state the drawing and the HUD read.
//! ShotClock BeginPlay (@805-845 -> 777 -> 202-689): GetAllActorsOfClass(M010_Hoop_A_01_BP_C), and per hoop: its
//! BasketScored bound to HandlePointScored @293-316, SpawnEmitterAttached(PS_M010_Clock, PS_Spacejam_Clock) @357,
//! Delay(24) @458 and Delay(20) @694 (one per node: the second hoop's calls start nothing). Delay(20) resumes @136:
//! PF_Post_Event(sfx_map_M010_shotclock_tick, self). Delay(24) resumes @35: SpawnEmitterAttached(PS_M010_Clock1) and
//! broadcast ShotClockFinishedEvent @15. The clock never starts again.
//! HandlePointScored(TeamIndex = the hoop's HoopIndex, PointValue) @850-1233: TeamIndexScoreMap[key] += points
//! (Map_Add of the points where the key is absent), broadcast TeamScoreChangedEvent(the new total, key).
//! JumboTron BeginPlay @652-763: the scoreboard widget hidden (SetVisibility 2), OnShotClockFinished and
//! OnTeamScoreChanged bound on every M010_ShotClock_C. OnShotClockFinished: Delay(2) @801 (resume @15):
//! SetVisibility(0) and Enter(). OnTeamScoreChanged: MulticastSetScore -> SetTeamScore(score, TeamIndex) @879
//! (UI_SpaceJam_Scoreboard: TeamIndex 0 is its right side, 1 its left).
//! Deviation: the JumboTron's Delay is kept with the shot clock's owner number (its own node key 801); the two
//! actors' places among the frame's latent passes are not read. Deviation: the shot clock's position is not in the
//! port's stage data (a VFX-level actor the stage bind does not place): its tick sound is posted with no position.

use super::ids::{SCORE_EV, SHOTCLOCK_OWNER};
use super::own_mut;
use crate::State;
use crate::events::Event;
use crate::name;
use crate::world::World;
use crate::world::stage::stage_delay;

const EV_TICK: i32 = SCORE_EV; // Delay(20) resumed (694 -> 136)
const EV_FINISHED: i32 = SCORE_EV + 1; // Delay(24) resumed (458 -> 35)
const EV_SHOWN: i32 = SCORE_EV + 2; // the JumboTron's Delay(2) resumed (801 -> 15)
const KEY_FINISHED: i32 = 458;
const KEY_TICK: i32 = 694;
const KEY_SHOWN: i32 = 801;

/// `CourtScore`: what the drawing and the HUD read (every tick a world tick, -1 before the event).
#[derive(State, Debug, Default)]
pub struct CourtScore {
    /// TeamIndexScoreMap by HoopIndex (0 where the key is absent): the scoreboard's two numbers.
    pub by_hoop: Vec<i32>,
    /// The tick of the last HandlePointScored (TeamScoreChangedEvent: SetTeamScore on the scoreboard).
    pub scored_at: i32,
    /// Its HoopIndex (-1 before the first).
    pub scored_hoop: i32,
    /// The shot clock's Delay(20): sfx_map_M010_shotclock_tick posted.
    pub tick_at: i32,
    /// The Delay(24): PS_M010_Clock1 on PS_Spacejam_Clock, ShotClockFinishedEvent.
    pub finished_at: i32,
    /// The JumboTron's Delay(2) after it: the scoreboard visible and its Enter() animation.
    pub shown_at: i32,
}

/// `scoreCreate`: ShotClock BeginPlay (the court's create, after the level script's). PS_M010_Clock is attached from
/// here on (the drawing's, from the match's start).
pub fn create(w: &mut World) -> CourtScore {
    let s =
        CourtScore { by_hoop: vec![0, 0], scored_at: -1, scored_hoop: -1, tick_at: -1, finished_at: -1, shown_at: -1 };
    stage_delay(w, SHOTCLOCK_OWNER, KEY_FINISHED, 24.0, EV_FINISHED);
    stage_delay(w, SHOTCLOCK_OWNER, KEY_TICK, 20.0, EV_TICK);
    s
}

/// `scoreTimer`: a due Delay of the shot clock or the JumboTron (ev `SCORE_EV` and up, owner `SHOTCLOCK_OWNER`).
pub fn timer(w: &mut World, ev: i32) {
    let tick = w.flow.tick;
    if ev == EV_TICK {
        own_mut(w).score.tick_at = tick;
        w.events.push(Event::Sound {
            fighter: -1,
            ev: name!("sfx_map_M010_shotclock_tick"),
            x: None,
            y: None,
            hit: None,
            pooled: false,
            // The key is the bare string `m010ShotClock`: n i32::MIN, which the record writes as the tag alone.
            key: Some(crate::events::SoundKey { tag: name!("m010ShotClock"), n: i32::MIN }),
        });
    } else if ev == EV_FINISHED {
        own_mut(w).score.finished_at = tick;
        // ShotClockFinishedEvent -> the JumboTron's OnShotClockFinished
        stage_delay(w, SHOTCLOCK_OWNER, KEY_SHOWN, 2.0, EV_SHOWN);
    } else if ev == EV_SHOWN {
        own_mut(w).score.shown_at = tick;
    }
}

/// `pointScored`: a hoop's BasketScored(HoopIndex, points) (`hoop` Handle Points Earned): HandlePointScored.
pub fn point_scored(w: &mut World, hoop_index: i32, points: i32) {
    let tick = w.flow.tick;
    let s = &mut own_mut(w).score;
    let at = usize::try_from(hoop_index).expect("a hoop index");
    while s.by_hoop.len() <= at {
        s.by_hoop.push(0);
    }
    s.by_hoop[at] += points;
    s.scored_at = tick;
    s.scored_hoop = hoop_index;
}
