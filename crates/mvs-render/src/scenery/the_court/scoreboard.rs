//! The Court's shot clock's end and its JumboTron's scoreboard, as the sim's CourtScore has them
//! (mvs-sim stages/the_court/score.rs; the graphs of M010_ShotClock,
//! M010_JumboTron, UI_SpaceJam_Scoreboard and UI_SpaceJam_ScoreboardTeam):
//! - ShotClock's Delay(24) resumed @35: SpawnEmitterAttached(PS_M010_Clock1, PS_Spacejam_Clock: offset 0, rot 0,
//!   scale 1) once (one Delay node for both hoops), at CourtScore.finished_at, at the placed clock emitter
//!   (`shotClock.at`). Its Delay(20)'s tick is a sound alone, posted by the sim.
//! - M010_JumboTron_C's UI (a World-space WidgetComponent, the plane geometry; the importer's `jumbotron.ui`:
//!   UI_SpaceJam_Scoreboard, DrawSize 950 x 950, pivot 0.5, BlendMode Transparent, its world place; one-sided and
//!   untinted by the component's defaults): a world-space widget (../widget.rs) the UI paints (mvs-ui
//!   world_widget.rs). The quad is the component's local Y-Z plane, its top left at (+Y, +Z) (FWidget3DSceneProxy's
//!   vertices: u along -Y, v along -Z), read from +X.
//!   BeginPlay @685 hides it (SetVisibility 2); at CourtScore.shown_at (OnShotClockFinished's Delay(2))
//!   SetVisibility(0) and Enter() (EnterAnim: the box's RenderOpacity 0 -> 1 over 0.5 s). SetTeamScore(score,
//!   TeamIndex) @879 on each TeamScoreChangedEvent: TeamIndex (the hoop's HoopIndex) 0 is
//!   UI_SpaceJam_ScoreboardTeam_Right, 1 _Left; SetScore writes labelScore and plays ScoreChangeAnim. The animations
//!   run on game time (TimingPolicy GameTime): the world's ticks here. A side's animation starts at the tick its
//!   number changes (CourtScore.by_hoop), so a rollback that takes a score back plays it again.
//!
//! The widget is painted only in a frame whose picture changed (the painter's: the 0.5 s animations, a score).

use mvs_vfx::{Attachment, SystemId};
use serde::Deserialize;
use serde_json::Value;

use super::super::host::SceneryHost;
use super::super::widget::{WidgetPaint, WorldWidget};
use super::rows_of;
use crate::Result;
use crate::stage::bad;
use mvs_sim::world::stage::StageOwn;
use mvs_view::M4;

/// Ticks: a Finished older than this when first seen is not shown (a rollback's replay, a Lab reset).
const LATE: i32 = 6;
/// The widget's key among the scenery's.
const KEY: &str = "court.scoreboard";
const CLASS: &str = "UI_SpaceJam_Scoreboard";

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Data {
    shot_clock: ShotClock,
    jumbotron: Jumbotron,
}

#[derive(Debug, Deserialize)]
struct ShotClock {
    at: Place,
    finished: Finished,
}

#[derive(Debug, Deserialize)]
struct Finished {
    system: String,
}

#[derive(Debug, Deserialize)]
struct Place {
    loc: [f64; 3],
    rot: [f64; 3],
    scale: [f64; 3],
}

#[derive(Debug, Deserialize)]
struct Jumbotron {
    ui: Ui,
}

#[derive(Debug, Deserialize)]
struct Ui {
    widget: String,
    size: [f64; 2],
    pivot: [f64; 2],
    blend: String,
    loc: [f64; 3],
    rot: [f64; 3],
    scale: [f64; 3],
}

/// The shot clock's Finished and the scoreboard.
#[derive(Debug)]
pub(super) struct Scoreboard {
    finished_system: String,
    clock_at: Attachment,
    size: [u32; 2],
    place: M4,
    finished: Option<SystemId>,
    finished_at: i32,
    /// What the picture shows: the sim's numbers as of the last tick.
    shown_at: i32,
    scores: [i32; 2],
    changed_at: [i32; 2],
    tick: i32,
}

impl Scoreboard {
    /// From the court block's `shotClock` and `jumbotron`.
    pub(super) fn make(data: &Value) -> Result<Scoreboard> {
        let what = "scenery: the court JumboTron";
        let d: Data = serde_json::from_value(data.clone()).map_err(|e| bad(what, e.to_string()))?;
        let u = &d.jumbotron.ui;
        if u.widget != CLASS || u.blend != "Transparent" || u.pivot != [0.5, 0.5] {
            return Err(bad(what, "the UI is not the scoreboard this draws"));
        }
        let c = &d.shot_clock.at;
        let r = rows_of(u.rot);
        // T(loc) R S(scale x (1, W, H)): the unit quad at the draw size, centred (pivot 0.5)
        let s = [u.scale[0], u.scale[1] * u.size[0], u.scale[2] * u.size[1]];
        let row = |k: usize| [r[k][0] * s[k], r[k][1] * s[k], r[k][2] * s[k], 0.0];
        let place = [row(0), row(1), row(2), [u.loc[0], u.loc[1], u.loc[2], 1.0]];
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let size = u.size.map(|x| x as u32);
        Ok(Scoreboard {
            finished_system: d.shot_clock.finished.system,
            clock_at: Attachment { loc: c.loc, rot: rows_of(c.rot), scale: Some(c.scale) },
            size,
            place,
            finished: None,
            finished_at: -1,
            shown_at: -1,
            scores: [0, 0],
            changed_at: [-1, -1],
            tick: 0,
        })
    }

    pub(super) fn begin(&mut self, host: &mut SceneryHost) {
        self.kill(host);
        self.finished_at = -1;
        self.shown_at = -1;
        self.scores = [0, 0];
        self.changed_at = [-1, -1];
        host.set_widget(self.widget(None));
    }

    pub(super) fn tick(&mut self, host: &mut SceneryHost) {
        let Some(w) = host.world() else { return };
        let Some(StageOwn::Court(own)) = w.stage_run.as_ref().map(|r| &r.s) else { return };
        let s = &own.score;
        let tick = w.flow.tick;
        let (finished_at, shown_at, scored_at, scored_hoop) = (s.finished_at, s.shown_at, s.scored_at, s.scored_hoop);
        let by_hoop = [0, 1].map(|k| s.by_hoop.get(k).copied().unwrap_or(0));
        if finished_at != self.finished_at {
            self.finished_at = finished_at;
            if finished_at < 0 {
                self.kill(host);
            } else if tick - finished_at <= LATE {
                self.kill(host);
                if let Some(vfx) = host.vfx() {
                    self.finished = vfx.spawn_level(&self.finished_system, &self.clock_at, false, &[], 0, false);
                }
            }
        }
        self.tick = tick;
        self.shown_at = shown_at;
        for (k, &v) in by_hoop.iter().enumerate() {
            if v != self.scores[k] {
                self.scores[k] = v;
                self.changed_at[k] =
                    if usize::try_from(scored_hoop) == Ok(k) && scored_at >= 0 { scored_at } else { tick };
            }
        }
    }

    /// The picture as of the last tick.
    pub(super) fn frame(&mut self, host: &mut SceneryHost) {
        let shown = self.shown_at >= 0 && self.tick >= self.shown_at;
        let secs = |from: i32| f64::from(self.tick - from) * f64::from(mvs_sim::consts::DELTA_SECONDS);
        let paint = shown.then(|| WidgetPaint::Scoreboard {
            enter: secs(self.shown_at),
            scores: self.scores,
            changed: self.changed_at.map(|at| (at >= 0).then(|| secs(at))),
        });
        host.set_widget(self.widget(paint));
    }

    pub(super) fn end(&mut self, host: &mut SceneryHost) {
        self.kill(host);
        host.set_widget(self.widget(None));
    }

    fn kill(&mut self, host: &mut SceneryHost) {
        if let Some(id) = self.finished.take()
            && let Some(vfx) = host.vfx()
        {
            vfx.kill(id);
        }
    }

    fn widget(&self, paint: Option<WidgetPaint>) -> WorldWidget {
        WorldWidget { key: KEY, class: CLASS, size: self.size, place: self.place, paint }
    }
}
