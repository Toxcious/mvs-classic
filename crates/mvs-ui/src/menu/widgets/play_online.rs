//! WBP_PlayOnline: the PLAY button at the bottom centre of the home screen.
//!
//! Owed to the match options' track: the cards' pictures held while the home screen is up (`matchOptionsPictures`).

use std::any::Any;

use serde_json::{Value, json};

use super::controls::interactor::{batch_set_context, set_interactable};
use super::controls::{bind, find, focus_frame, prop_f64, wd};
use super::modals::show_modal_generic;
use crate::menu::controller::{Controller, Cx};
use crate::menu::manager::Manager;
use crate::slate::Vis;

/// PartyChange's NewModeText (@4116): FIGHT!
const FIGHT_TEXT: &str = "250CA85D4185D46D58D9FD8A24049621";

const H_FOCUS: u32 = 1;
const H_UNFOCUS: u32 = 2;
const H_CLICK: u32 = 3;
const H_CHANGE_MODE: u32 = 4;
const H_SUBMIT3: u32 = 5;

/// A Wobbler's TopLeft written before its Construct (OnInitialized @2847 / @2933): the struct variable the
/// quadrilateral reads at Construct.
fn set_wobbler_top_left(cx: &mut Cx<'_>, fc: &str, x: f64, y: f64) {
    let f = find(cx, fc);
    let q = cx.view.tree().find_in(f, "Wobbler");
    cx.view.tree_mut().set_prop(q, "TopLeft", json!({ "X": x, "Y": y }));
}

/// WBP_PlayOnline.
#[derive(Debug, Default)]
pub struct PlayOnline {
    interact_timer: f64,
}

impl PlayOnline {
    pub fn make(_cx: &mut Cx<'_>) -> Box<dyn Controller> {
        Box::new(PlayOnline::default())
    }

    pub fn interactor(cx: &Cx<'_>) -> crate::slate::WidgetId {
        find(cx, "UI_Interactor")
    }

    /// SetOn @2665: WBP_QuadButton SelfHitTestInvisible either way.
    fn set_on(cx: &mut Cx<'_>) {
        let q = find(cx, "WBP_QuadButton");
        wd(cx, q).vis = Vis::SelfHitTestInvisible;
    }

    /// E1 @2331: SetOn(true); nothing when the manager has a modal `matchoptions`, else ShowModal-Generic(true,
    /// WBP_MatchOptions, "matchoptions", "") (every modal is dismissed first).
    fn clicked(cx: &mut Cx<'_>) {
        Self::set_on(cx);
        if cx.view.manager.get_modal("matchoptions").is_some() {
            return;
        }
        show_modal_generic(cx, true, "WBP_MatchOptions", "matchoptions", "");
    }

    /// @3194: nothing without CanChange (true in the class defaults; PartyChange @3984 sets it to not InParty), else as
    /// E1 but without SetOn.
    fn change_mode_clicked(cx: &mut Cx<'_>) {
        if cx.view.tree().props(cx.w).get("CanChange").and_then(Value::as_bool) == Some(false) {
            return;
        }
        if cx.view.manager.get_modal("matchoptions").is_some() {
            return;
        }
        show_modal_generic(cx, true, "WBP_MatchOptions", "matchoptions", "");
    }

    /// UpdateFWOTDTime: the first win bonus inactive (Deviation: the port has no first-win bonus, so it takes the inactive
    /// branch where the live game's capture showed it active; the strip over PLAY! is hidden).
    fn update_fwotd_time(cx: &mut Cx<'_>) {
        for (n, v) in [
            ("FWOTD_Strip", Vis::Collapsed),
            ("FWOTD_TextContainer", Vis::Collapsed),
            ("UI_FocusedCommon_Play", Vis::SelfHitTestInvisible),
            ("UI_FocusedCommon_PlayFWOTD", Vis::Collapsed),
            ("Winter_Treatment", Vis::Hidden),
        ] {
            let w = find(cx, n);
            wd(cx, w).vis = v;
        }
    }

    /// CanShortcutPlay?: every input is evaluated (TimeIntervalCheck sets InteractTimer when it passes, whatever the
    /// others give).
    fn can_shortcut_play(&mut self, cx: &mut Cx<'_>) -> bool {
        let i = Self::interactor(cx);
        let modal = Manager::is_modal_active(cx.view);
        let now = cx.now();
        let d = now - self.interact_timer;
        let ready = d >= prop_f64(cx, "InteractInterval", 1.0) || d < 0.0;
        if ready {
            self.interact_timer = now;
        }
        let visible = cx.view.visible_in_hierarchy(cx.w);
        let in_context = Manager::is_in_current_context(cx, i, false, -1);
        let can = cx.view.manager.interactor(i).is_some_and(|s| s.get_interactable());
        !modal
            && in_context
            && Manager::is_focusable(cx, i)
            && can
            && visible
            && ready
            && !cx.view.manager.is_transitioning()
    }

    /// PartyChange(InParty) @3984: CanChange and interactorChangeMode's interactable are not InParty; in a party
    /// ButtonText "FIGHT!" (@4116) and GameModeChange plays at speed 1.65 (@4445).
    pub fn party_change(&mut self, cx: &mut Cx<'_>, in_party: bool) {
        cx.view.tree_mut().set_prop(cx.w, "CanChange", Value::Bool(!in_party));
        let m = find(cx, "interactorChangeMode");
        set_interactable(cx, m, !in_party);
        if !in_party {
            return;
        }
        let t = find(cx, "ButtonText");
        let text = cx.view.game_text(FIGHT_TEXT).to_string();
        wd(cx, t).text = text;
        let me = cx.w;
        cx.view.play_animation(me, "GameModeChange", false, 1.65, 1, false, false);
    }

    /// UpdateContext @3024: BatchSetContext(GetInteractors): interactorChangeMode, UI_Interactor and AutoPartyBoostBadges'
    /// six (UI_RestedXPBar is not in the menu bundle). The mode-suggestion callout has no controller (it stays
    /// Hidden), and CheckModeSuggestionTooltip shows it only for a new-content slug the port's preferences do not hold.
    pub fn update_context(&self, cx: &mut Cx<'_>, ctx: &str) {
        let badges = find(cx, "AutoPartyBoostBadges");
        let mut list = vec![find(cx, "interactorChangeMode"), Self::interactor(cx)];
        for n in [
            "BoostBadge_FWOTD",
            "BoostBadge_Party",
            "BoostBadge_TimedBoostXP",
            "BoostBadge_TimedBoostBP",
            "BoostBadge_TimedBoostGold",
            "UI_ButtonSecondary",
        ] {
            let b = cx.view.tree().find_in(badges, n);
            list.push(cx.view.tree().find_in(b, "UI_Interactor"));
        }
        batch_set_context(cx, &list, ctx);
    }
}

impl Controller for PlayOnline {
    fn on_initialized(&mut self, cx: &mut Cx<'_>) {
        let i = Self::interactor(cx);
        // OnFocus @2299 / OnUnfocus @2315: SetOn; OnClick @2331
        bind(cx, i, |s| &mut s.on_focus, H_FOCUS);
        bind(cx, i, |s| &mut s.on_unfocus, H_UNFOCUS);
        bind(cx, i, |s| &mut s.on_click, H_CLICK);
        let m = find(cx, "interactorChangeMode");
        bind(cx, m, |s| &mut s.on_click, H_CHANGE_MODE);
        // @2833 SetInputShortcutBindings @4708: the manager's OnSubmit3 (the UISubmit3 label beside PLAY!)
        let me = cx.w;
        cx.view.manager.on_submit3.add(crate::menu::controller::Handler { owner: me, id: H_SUBMIT3 });
        set_wobbler_top_left(cx, "UI_FocusedCommon_PlayFWOTD", 0.0, 0.15);
        set_wobbler_top_left(cx, "UI_FocusedCommon_Play", 0.0, 0.08);
        // Then @173 waits with Delay(0) until BP_FighterGameInstance.ClientHydraManager is valid, which the port never has
        // (Deviation: the calendar events, the first-win tooltip and Setup Manual Navigation are not run).
    }

    fn construct(&mut self, cx: &mut Cx<'_>) {
        let i = Self::interactor(cx);
        let m = find(cx, "interactorChangeMode");
        for (fc, iw) in [("UI_FocusedCommon_Play", i), ("UI_FocusedCommon_PlayFWOTD", i), ("UI_FocusedCommon_Mode", m)]
        {
            let f = find(cx, fc);
            focus_frame(cx, f, iw);
        }
        let t = find(cx, "ButtonText");
        wd(cx, t).text = "PLAY!".into();
        Self::update_fwotd_time(cx);
    }

    fn destruct(&mut self, cx: &mut Cx<'_>) {
        let me = cx.w;
        cx.view.manager.on_submit3.remove(crate::menu::controller::Handler { owner: me, id: H_SUBMIT3 });
    }

    fn handle(&mut self, cx: &mut Cx<'_>, id: u32, _arg: &dyn Any) {
        match id {
            H_FOCUS | H_UNFOCUS => Self::set_on(cx),
            H_CLICK => Self::clicked(cx),
            H_CHANGE_MODE => Self::change_mode_clicked(cx),
            // OnSubmit3 @4670: CanShortcutPlay? then the click's path @2331 (no interactor click, so no confirm sound)
            H_SUBMIT3 if self.can_shortcut_play(cx) => Self::clicked(cx),
            _ => {}
        }
    }
}
