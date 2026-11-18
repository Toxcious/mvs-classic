//! The arcade progress pips: UI_AXProgress and its ten UI_AXProgressStage, on
//! the pre-match header and on the end of game's UI_ArcadeEOGContinue (UI_AXProgress offsets AP@, UI_AXProgressStage
//! AS@ below).

use super::controls::{find, prop_f64, wd};
use crate::menu::arcade::{arcade_round_type, num_arcade_levels};
use crate::menu::controller::{Controller, Cx};
use crate::slate::{Vis, WidgetId};

const STAGES: usize = 10;
/// UpdateCharacterPortrait's soft textures of the Mini (AS@2085) and the bosses' rounds (AS@2179, AS@2273).
const MINI_ICON: &str = "UI_Stat_Reindog_Icon";
const BOSS_ICON: &str = "ui_icons_question";
/// OnTimerUpdateRoundNumber's looping timer (AS@105).
const POLL_S: f64 = 0.05;

/// GetArcadeInfo stands for the arcade run; the cast to the native
/// CoopVsAIManager (AS@2751-2862) for there being one.
#[derive(Debug, Default)]
pub struct AXProgressStage {
    arrow_visible: bool,
    acc: f64,
}

impl AXProgressStage {
    pub fn make(_cx: &mut Cx<'_>) -> Box<dyn Controller> {
        Box::new(AXProgressStage::default())
    }

    fn stage(cx: &Cx<'_>) -> i32 {
        prop_f64(cx, "StageNumber", 0.0) as i32
    }

    /// UpdateRoundNumber AS@2655 -> @2367 -> @204.
    pub fn update_round_number(&mut self, cx: &mut Cx<'_>) {
        let Some(run) = cx.view.session.arcade.info().cloned() else { return };
        let stage = Self::stage(cx);
        // @2583: Collapsed past the ladder, SelfHitTestInvisible otherwise
        let w = cx.w;
        wd(cx, w).vis =
            if stage > num_arcade_levels(run.difficulty) { Vis::Collapsed } else { Vis::SelfHitTestInvisible };
        // @370: the rounds already won show a filled pip
        let pip = find(cx, "PipScore");
        wd(cx, pip).vis = if stage >= run.round { Vis::Hidden } else { Vis::SelfHitTestInvisible };
        let left = find(cx, "LineLeft");
        wd(cx, left).vis = if stage == 1 { Vis::Hidden } else { Vis::SelfHitTestInvisible }; // @553
        // @706-853
        if run.round == stage {
            if self.arrow_visible {
                return;
            }
            self.arrow_visible = true;
            let w = cx.w;
            cx.view.play(w, "ArrowIn", false, 1.0);
        } else if self.arrow_visible {
            self.arrow_visible = false;
            let w = cx.w;
            cx.view.play(w, "ArrowOut", false, 1.0);
        }
    }

    /// UpdateCharacterPortrait(RivalSlug) AS@858: the stage's icon by ArcadeGetRoundType(difficulty, StageNumber).
    pub fn update_character_portrait(&mut self, cx: &mut Cx<'_>, rival_slug: Option<&str>) {
        let run = cx.view.session.arcade.info().cloned();
        let stage = Self::stage(cx);
        let kind = run.as_ref().map_or(0, |r| arcade_round_type(r.difficulty, stage));
        let icon = find(cx, "CharacterIcon");
        if kind == 0 || kind == 1 {
            // Invalid / Regular @1289-1327
            wd(cx, icon).vis = Vis::Hidden;
            let pip = find(cx, "PipImage");
            wd(cx, pip).vis = Vis::SelfHitTestInvisible;
        } else {
            // Rivals @1900-1929: the rival's ArcadeProgressCharacterIconTexture (GetCharacterDataFromSlug ignores case);
            // the Mini @2085, Reign Dog @2179, Giant Gizmo @2273
            let image: Option<String> = if kind == 2 {
                let slug = rival_slug.map(str::to_lowercase);
                slug.and_then(|s| {
                    cx.view.fighters().values().find(|f| f.slug.to_lowercase() == s).and_then(|f| f.arcade_icon.clone())
                })
            } else if kind == 3 {
                Some(MINI_ICON.to_string())
            } else {
                Some(BOSS_ICON.to_string())
            };
            // SetBrushFromSoftTexture @1366. Unverified: a fighter with no ArcadeProgressCharacterIconTexture (the
            // creature) is read as an icon with no picture.
            if let Some(i) = &image {
                cx.view.load_texture(i);
            }
            let x = wd(cx, icon);
            x.tex = image;
            x.vis = Vis::SelfHitTestInvisible; // @1978
            if self.arrow_visible {
                // @2016-2037
                self.arrow_visible = false;
                let w = cx.w;
                cx.view.play(w, "ArrowOut", false, 1.0);
            }
        }
        let last = run.as_ref().map_or(0, |r| num_arcade_levels(r.difficulty));
        let right = find(cx, "LineRight");
        wd(cx, right).vis = if stage == last { Vis::Hidden } else { Vis::SelfHitTestInvisible }; // @1628
        let left = find(cx, "LineLeft");
        wd(cx, left).vis = if stage == 1 { Vis::Hidden } else { Vis::SelfHitTestInvisible }; // @1810
    }
}

impl Controller for AXProgressStage {
    /// PreConstruct AS@63: UpdateRoundNumber, then the 0.05 s looping timer (the design-time portrait @159 is the
    /// editor's).
    fn pre_construct(&mut self, cx: &mut Cx<'_>) {
        self.update_round_number(cx);
    }

    /// OnTimerUpdateRoundNumber AS@2877: UpdateRoundNumber while the round is not 0.
    fn timers(&mut self, cx: &mut Cx<'_>, dt: f64) {
        self.acc += dt;
        if self.acc < POLL_S {
            return;
        }
        self.acc %= POLL_S;
        if cx.view.session.arcade.info().is_some_and(|r| r.round != 0) {
            self.update_round_number(cx);
        }
    }
}

/// UI_AXProgress.
#[derive(Debug, Default)]
pub struct AXProgress;

impl AXProgress {
    pub fn make(_cx: &mut Cx<'_>) -> Box<dyn Controller> {
        Box::new(AXProgress)
    }

    fn stages(cx: &Cx<'_>) -> Vec<WidgetId> {
        (1..=STAGES).map(|i| find(cx, &format!("UI_ArcadeProgressStage_{i}"))).collect()
    }

    /// UpdateRivalPortrait AP@240-645.
    pub fn update_rival_portrait(&mut self, cx: &mut Cx<'_>, rival_slug: Option<&str>) {
        for s in Self::stages(cx) {
            cx.visit::<AXProgressStage, _>(s, |c, cx| c.update_character_portrait(cx, rival_slug));
        }
    }

    /// UpdateStageNumber AP@695-1019.
    pub fn update_stage_number(&mut self, cx: &mut Cx<'_>) {
        for s in Self::stages(cx) {
            cx.visit::<AXProgressStage, _>(s, |c, cx| c.update_round_number(cx));
        }
    }
}

impl Controller for AXProgress {
    /// PreConstruct AP@10: UpdateRivalPortrait(CoOpVsAIManager.ArcadeRivalSlugs[0]) when there is a manager.
    fn pre_construct(&mut self, cx: &mut Cx<'_>) {
        if let Some(run) = cx.view.session.arcade.info().cloned() {
            self.update_rival_portrait(cx, run.rival_slug.as_deref());
        }
    }
}
