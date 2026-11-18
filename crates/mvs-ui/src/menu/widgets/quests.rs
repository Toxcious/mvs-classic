//! The home screen's missions: UI_QuestTabs and UI_QuestsContainer.
//!
//! Every quest, its tab and its counts come from the backend (ClientHydraManager.QuestManager), which the port does
//! not have: what shows is what the two classes' own graphs make of their trees before any quest data.

use serde_json::Value;

use super::common::{Quadrilateral, rgba, text};
use super::controls::{find, prop, prop_bool, wd};
use super::section_bar::SectionBar;
use crate::menu::controller::{Controller, Cx};
use crate::menu::manager::NULL_GUID;
use crate::menu::view::MenuView;
use crate::slate::{Vis, WidgetId};

/// EQuestType (Daily 0, Weekly 1, CalendarControlled 2, Onboarding 3: the order of ReadLocalQuestDataForPlayer's
/// arrays in the Construct switch @2396), from the property's `EQuestType::<name>`; another value is 0.
pub fn quest_type(v: &Value) -> i32 {
    let s = v.as_str().unwrap_or_default();
    match s.rsplit("::").next().unwrap_or_default() {
        "Weekly" => 1,
        "CalendarControlled" => 2,
        "Onboarding" => 3,
        _ => 0,
    }
}

// ---------- UI_QuestsContainer ----------

/// UI_QuestsContainer.
#[derive(Debug)]
pub struct QuestsContainer {
    pub guid: String,
    /// The widget variables PreConstruct @22375 clears and removes: they keep pointing at the widgets once these are
    /// out of the tree, so a later PreConstruct (each Construct runs one) clears and removes them again, to no
    /// effect.
    rows: Vec<WidgetId>,
}

impl QuestsContainer {
    pub fn make(_cx: &mut Cx<'_>) -> Box<dyn Controller> {
        Box::new(QuestsContainer { guid: NULL_GUID.to_string(), rows: Vec::new() })
    }

    /// UpdateCompletionVisibility @34892: TimerCompleteBox and CompletionSwitcher Collapsed with Hide Completion
    /// Count, else SelfHitTestInvisible for every quest type but weekly (Collapsed).
    fn update_completion_visibility(cx: &mut Cx<'_>) {
        let hide = prop_bool(cx, "Hide Completion Count") || quest_type(&prop(cx, "QuestType")) == 1;
        let v = if hide { Vis::Collapsed } else { Vis::SelfHitTestInvisible };
        for n in ["TimerCompleteBox", "CompletionSwitcher"] {
            let w = find(cx, n);
            wd(cx, w).vis = v;
        }
    }

    /// UpdateContext @27693: the GUID, then BatchSetContext(GetInteractors): the quest rows' interactors (none) and
    /// UI_PaginationPips' (not set by the port: the pips are never Init'd without quests, their container is
    /// Collapsed).
    pub fn update_context(&mut self, ctx: &str) {
        self.guid = ctx.to_string();
    }
}

impl Controller for QuestsContainer {
    /// OnInitialized @28144: the manager's OnUIScroll binding and RegisterScrollbox(scrollContainer).
    fn on_initialized(&mut self, cx: &mut Cx<'_>) {
        self.rows = ["Container", "Container_1", "HorizontalBox_2"].iter().map(|n| find(cx, n)).collect();
        let sb = find(cx, "scrollContainer");
        cx.view.manager.register_scrollbox(sb);
    }

    /// Destruct @22708: UnregisterScrollbox (the quest manager's unbinding has nothing to undo). OnInitialized does
    /// not run again on a later Construct, so the box stays unregistered then, as in the build.
    fn destruct(&mut self, cx: &mut Cx<'_>) {
        let sb = find(cx, "scrollContainer");
        cx.view.manager.unregister_scrollbox(sb);
    }

    /// PreConstruct @21190, the run-time path (not IsDesignTime).
    fn pre_construct(&mut self, cx: &mut Cx<'_>) {
        Self::update_completion_visibility(cx);
        let div = find(cx, "Div");
        if prop_bool(cx, "IsHorizontalContainer") {
            wd(cx, div).vis = Vis::Collapsed;
            let q = find(cx, "QuestListSizeBox");
            wd(cx, q).h_override = 110.0;
        } else {
            wd(cx, div).vis = Vis::SelfHitTestInvisible;
        }
        let bp = find(cx, "BPLength");
        wd(cx, bp).vis = if prop_bool(cx, "ShowDate") { Vis::SelfHitTestInvisible } else { Vis::Collapsed };
        if !prop_bool(cx, "TwoColumns") {
            // @22560 WBP_Quadrilateral.UpdateColor(SelectColor(LighterBlue, DarkerBlue, IsBattlePassPanel))
            // (TwoColumns, @21366, moves Container_1 into HorizontalBox_2 for a two-column container: no instance of
            // the bundle sets it, and @22375 below takes both containers out of the tree anyway.)
            let c = prop(cx, if prop_bool(cx, "IsBattlePassPanel") { "LighterBlue" } else { "DarkerBlue" });
            if !c.is_null() {
                let q = find(cx, "WBP_Quadrilateral");
                cx.visit::<Quadrilateral, _>(q, |q, cx| q.update_color(cx, rgba(&c)));
            }
        }
        // @22375: the designer's quest rows go: Container and Container_1 cleared, HorizontalBox_2, Container and
        // Container_1 removed from their parents (the rows of a quest list are made from the quest data, which never
        // comes)
        let (container, container1, hbox) = (self.rows[0], self.rows[1], self.rows[2]);
        for bx in [container, container1] {
            for c in cx.view.tree().widget(bx).children.clone() {
                MenuView::discard(cx, c);
            }
        }
        for w in [hbox, container, container1] {
            if cx.view.tree().widget(w).parent.is_some() {
                MenuView::remove_child(cx, w);
            }
        }
        // @21702
        let title = find(cx, "Title");
        let p = cx.view.tree().props(cx.w).clone();
        wd(cx, title).text = text(p.get("TitleText"));
        if let Some(c) = p.get("Text Color") {
            wd(cx, title).color = rgba(c);
        }
        if let Some(c) = p.get("Time Color") {
            let c = rgba(c);
            for n in ["TimeLeft", "Date", "TimeIcon"] {
                let w = find(cx, n);
                wd(cx, w).color = c;
            }
        }
        // SetMonthAndDayText @22041 writes Date ("{Month} {Day}" for daily quests) inside TodaysDate, which is
        // Collapsed in the tree and set visible by no statement of the class: not ported.
        // @22055-22333: Spacer_1 and Spacer_193 20 wide for every quest type
        for n in ["Spacer_1", "Spacer_193"] {
            let s = find(cx, n);
            let s = wd(cx, s);
            s.brush_w = 20.0;
            s.brush_h = 0.0;
        }
    }

    // Construct @1904 waits with Delay(0) until ClientHydraManager and its QuestManager are valid and the player's
    // quest data has quests of the container's type: never, without the backend (the port does not poll for it).
    // Deviation.
}

// ---------- UI_QuestTabs ----------

const CONTAINERS: [&str; 3] = ["UI_QuestsContainer_Event", "UI_QuestsContainer_Daily", "UI_QuestsContainer_Onboarding"];

/// UI_QuestTabs.
#[derive(Debug, Default)]
pub struct QuestTabs;

impl QuestTabs {
    pub fn make(_cx: &mut Cx<'_>) -> Box<dyn Controller> {
        Box::new(QuestTabs)
    }

    /// UpdateContext @1718 (the home screen's Update Context calls it).
    pub fn update_context(&mut self, cx: &mut Cx<'_>, ctx: &str) {
        for n in CONTAINERS {
            let c = find(cx, n);
            cx.visit::<QuestsContainer, _>(c, |q, _| q.update_context(ctx));
        }
        let bar = find(cx, "UI_SectionBar");
        cx.visit::<SectionBar, _>(bar, |b, cx| b.update_context(cx, ctx, None));
    }
}

impl Controller for QuestTabs {
    /// OnInitialized @1904 -> @1339: every button of the tab bar Collapsed (UpdateQuadShape before it: the button's
    /// shape for its place in the row, not ported as nothing shows it), the containers' populated / claimed events
    /// bound (the quest manager never fires them), then MissionsTitle Collapsed in the three containers. The tabs
    /// come back only from UpdateInitialTabFocus, which those events run.
    fn on_initialized(&mut self, cx: &mut Cx<'_>) {
        let bar = find(cx, "UI_SectionBar");
        let buttons = cx.visit::<SectionBar, _>(bar, |b, _| b.buttons.clone()).unwrap_or_default();
        for b in buttons {
            wd(cx, b).vis = Vis::Collapsed;
        }
        for n in ["UI_QuestsContainer_Daily", "UI_QuestsContainer_Onboarding", "UI_QuestsContainer_Event"] {
            let c = find(cx, n);
            let t = cx.view.tree().find_in(c, "MissionsTitle");
            wd(cx, t).vis = Vis::Collapsed;
        }
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::quest_type;

    #[test]
    fn quest_type_from_the_enum_name() {
        assert_eq!(quest_type(&json!("EQuestType::Weekly")), 1);
        assert_eq!(quest_type(&json!("EQuestType::Onboarding")), 3);
        assert_eq!(quest_type(&json!("Daily")), 0);
        assert_eq!(quest_type(&json!(null)), 0);
    }
}
