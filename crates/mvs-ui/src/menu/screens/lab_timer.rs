//! UI_Lab_MatchmakingTimer: the search's clock on the Lab's HUD while the player waits in the Lab for a queue's
//! match. UI_Game adds it when the Lab opens with a search running and nothing
//! removes it: the caller (the Lab, app match_lab) adds it with `add_lab_matchmaking_timer` and calls the returned
//! `LabTimerHandle::remove` when the Lab's level goes.

use mvs_protocol::online::QueueMode;

use crate::menu::controller::{Controller, Cx, MenuRun};
use crate::menu::screen::MenuScreen;
use crate::menu::view::MenuView;
use crate::menu::widgets::controls::{find, wd};
use crate::slate::{Margin, VAlign, WidgetId};

/// UI_Functions TimeFormatter(MakeTimespan(0, 0, 0, s, 0), false, false, false): the minutes and seconds branch,
/// "M:SS" with the seconds padded below 10 (the pre-match count-up writes the same text inline). M is
/// BreakTimespan's Minutes (UI_Functions TimeFormatter @94 / @562), 0 to 59: with Show Hours false the hours are
/// dropped, so an hour reads 0:00.
pub fn time_formatter_m_ss(seconds: f64) -> String {
    let t = if seconds.is_nan() { 0 } else { seconds.max(0.0).floor() as u64 };
    let s = t % 60;
    format!("{}:{}{}", (t / 60) % 60, if s < 10 { "0" } else { "" }, s)
}

/// Construct @2046-2599: ModeLabel by GetAllMultiqueue()[0].GetQueueType(), the bare word (the designer's "Mode: "
/// prefix is replaced).
/// Unverified: the port's 1v1, 2v2 and FFA queues are read as queue type 1, "Normal"
/// {F8D006C243F227FFB85345BB21684DF8} (the type of the multiqueue configs is the backend's); Chaos as the Casual
/// card's queue type 3.
fn mode_word(m: QueueMode) -> &'static str {
    // The switch's TextConsts by queue type (Game.locres keys): Normal, Casual.
    match m {
        QueueMode::OneVOne | QueueMode::TwoVTwo | QueueMode::Ffa => "F8D006C243F227FFB85345BB21684DF8",
        QueueMode::Chaos => "064EFA804BB3847536ACF99C055B6145",
    }
}

/// Construct @2780 -> @653: QueueLabel is the multiqueue entries' ToDisplayText (native), joined by ", " (one per
/// queue the search waits in).
/// Unverified: read as the mode card's name, as the pre-match QueueLabel (WBP_MatchOptions PrintSelectedModes); the
/// tree's ToUpper shows it upper-cased. Game.locres keys: 1 VS 1, TEAMS, FFA; None for the port's Chaos queue
/// (`custom_text::CHAOS_QUEUE`).
fn queue_text(m: QueueMode) -> Option<&'static str> {
    match m {
        QueueMode::OneVOne => Some("C1413E164DB8DCE5DC2CC19A2E4AEBA5"),
        QueueMode::TwoVTwo => Some("720C33654366ACE4376C668A91B54234"),
        QueueMode::Ffa => Some("1789A04E4D313B3F325D3485EB970175"),
        QueueMode::Chaos => None,
    }
}

/// The search's start (the program's clock, s) and queue, given before Construct.
#[derive(Clone, Debug)]
pub struct TimerSource {
    pub start_s: f64,
    pub modes: Vec<QueueMode>,
}

#[derive(Debug, Default)]
pub struct LabMatchmakingTimer {
    src: Option<TimerSource>,
    start_s: f64,
    shown_s: Option<i64>,
    /// The hud-positionBottom the layout last followed.
    bottom: Option<bool>,
}

impl LabMatchmakingTimer {
    pub fn make(_cx: &mut Cx<'_>) -> Box<dyn Controller> {
        Box::new(LabMatchmakingTimer::default())
    }

    /// LayoutPrefs @3386: VerticalBox_0's overlay slot SetVerticalAlignment(hud-positionBottom ? Top : Bottom), the
    /// edge the HUD does not use (@3501 / @3582). OnInitialized @3261 binds it to PM.OnUIPrefsChanged and calls it
    /// (@3371); here the preference is read each frame and applied when it changed.
    fn layout_prefs(&mut self, cx: &mut Cx<'_>) {
        let bottom = cx.prefs.ui.hud_position_bottom;
        if self.bottom == Some(bottom) {
            return;
        }
        self.bottom = Some(bottom);
        let b = find(cx, "VerticalBox_0");
        wd(cx, b).v_align = if bottom { VAlign::Top } else { VAlign::Bottom };
    }

    /// @2938-3182: TimeFormatter(MakeTimespan(0, 0, 0, trunc(BPI_GetTotalTimeSeconds - start), 0)). The total time is
    /// GetAccurateRealTime: the count goes on while the Lab is paused. The text is set when the second changes (its
    /// layout and paint stay as they were in the frames between).
    fn update_time(&mut self, cx: &mut Cx<'_>) {
        let s = (cx.now() - self.start_s).trunc().max(0.0) as i64;
        if self.shown_s == Some(s) {
            return;
        }
        self.shown_s = Some(s);
        let t = find(cx, "MatchmakingTimeText");
        wd(cx, t).text = time_formatter_m_ss(s as f64);
    }
}

impl Controller for LabMatchmakingTimer {
    fn on_initialized(&mut self, cx: &mut Cx<'_>) {
        self.layout_prefs(cx);
    }

    /// Construct @1778.
    fn construct(&mut self, cx: &mut Cx<'_>) {
        let src = self.src.clone().expect("UI_Lab_MatchmakingTimer: no search (add_lab_matchmaking_timer)");
        // @1824: Matchmaking Start Time = MatchmakingHandler.MatchmakingStartTime, a copy: a later reset of the
        // handler's start (the bot fallback's OnCancelForBots, MH@25434) is not seen here
        self.start_s = src.start_s;
        let word = src.modes.first().map_or("", |m| cx.view.game_text(mode_word(*m))).to_string();
        let queue = src
            .modes
            .iter()
            .map(|m| {
                queue_text(*m).map_or_else(
                    || crate::menu::custom_text::CHAOS_QUEUE.to_owned(),
                    |k| cx.view.game_text(k).to_string(),
                )
            })
            .collect::<Vec<_>>()
            .join(", ");
        let (ml, ql) = (find(cx, "ModeLabel"), find(cx, "QueueLabel"));
        wd(cx, ml).text = word;
        wd(cx, ql).text = queue;
        // @2813: the time text once
        self.update_time(cx);
    }

    /// Tick @3256 -> @2813.
    fn tick(&mut self, cx: &mut Cx<'_>, _dt: f64) {
        self.layout_prefs(cx);
        self.update_time(cx);
    }
}

/// A timer on a Lab screen: `remove` takes it off again.
#[derive(Debug)]
pub struct LabTimerHandle {
    root: WidgetId,
    timer: WidgetId,
    gone: bool,
}

impl LabTimerHandle {
    /// The call that removes it (idempotent).
    pub fn remove(&mut self, screen: &mut MenuScreen, run: &mut MenuRun<'_>) {
        if self.gone {
            return;
        }
        self.gone = true;
        screen.alone.retain(|&r| r != self.root);
        let timer = self.timer;
        screen.with_cx(run, |cx| MenuView::remove_from_parent(cx, timer));
    }
}

/// UI_Game @1932-2421: Create(UI_Lab_MatchmakingTimer), Root.AddChildToCanvas with anchors (0,0)-(1,1) and offsets
/// 50 on every side, no ZOrder: over the Lab's pause menu and UI_LAB_Hotkeys (`lab_hotkeys`), which UI_Game
/// added before it, and shown while that menu is collapsed (screen.alone).
pub fn add_lab_matchmaking_timer(screen: &mut MenuScreen, run: &mut MenuRun<'_>, src: TimerSource) -> LabTimerHandle {
    let (root, timer) = screen.with_cx(run, |cx| {
        let w = MenuView::create(cx, "UI_Lab_MatchmakingTimer");
        cx.visit::<LabMatchmakingTimer, _>(w, |t, _| t.src = Some(src));
        let bundle = cx.view.bundle.clone();
        let root = cx.view.tree_mut().create_plain(&bundle, "CanvasPanel", "RootCanvas");
        cx.view.tree_mut().add(root, w);
        let x = cx.view.tree_mut().widget_mut(w);
        x.anchors = [0.0, 0.0, 1.0, 1.0];
        x.offsets = Margin { l: 50.0, t: 50.0, r: 50.0, b: 50.0 };
        x.align = [0.0, 0.0];
        x.auto_size = false;
        MenuView::add_to_viewport(cx, root, 0);
        (root, w)
    });
    screen.alone.push(root);
    LabTimerHandle { root, timer, gone: false }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_clock_is_minutes_and_padded_seconds() {
        assert_eq!(time_formatter_m_ss(0.0), "0:00");
        assert_eq!(time_formatter_m_ss(9.9), "0:09");
        assert_eq!(time_formatter_m_ss(75.0), "1:15");
        // (hours are dropped)
        assert_eq!(time_formatter_m_ss(3600.0), "0:00");
        assert_eq!(time_formatter_m_ss(-4.0), "0:00");
    }
}
