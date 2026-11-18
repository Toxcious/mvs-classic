//! The per-player controls modal of Local Play (modal id "inputSettings"): UI_InputBindingModal and its
//! UI_CapturePlayersFocus, hosting the Controls page
//! (UI_PreferencesControlsRework, controls/input_binding.rs).
//!
//! Not ported: the pad-connection listener (the device layer has no listener list yet).

use std::any::Any;

use super::common::ModalBase;
use super::controls::input_binding::{InputBindingSet, PreferencesControls};
use super::controls::interactor::{self, batch_set_context};
use super::controls::{find, wd};
use super::section_bar::SectionBar;
use crate::menu::controller::{Controller, Cx, Handler};
use crate::menu::manager::Modal;
use crate::slate::{Vis, WidgetId};

// ---------- UI_CapturePlayersFocus ----------

/// Four non-navigable interactors, one per player (captureFocusInteractorP0..P3, Player Index 0..3): Focus() takes
/// every player's context to the modal's while the one player whose modal it is navigates it; Destruct gives each
/// player back the interactor it had.
#[derive(Debug, Default)]
pub struct CapturePlayersFocus {
    previous: [Option<WidgetId>; 4],
}

impl CapturePlayersFocus {
    pub fn make(_cx: &mut Cx<'_>) -> Box<dyn Controller> {
        Box::new(CapturePlayersFocus::default())
    }

    /// GetInteractors.
    fn interactors(cx: &Cx<'_>) -> Vec<WidgetId> {
        (0..4).map(|i| find(cx, &format!("captureFocusInteractorP{i}"))).collect()
    }

    /// Focus @1136: Previous Focus[i] = GetCurrentlyFocused(i) for 0..3 (@745-1024), then SetFocused(true) (the
    /// context alone) on each capture interactor (@15-366).
    pub fn focus(&mut self, cx: &mut Cx<'_>) {
        for i in 0..4 {
            self.previous[i] = cx.view.manager.get_currently_focused(i as i32);
        }
        for w in Self::interactors(cx) {
            interactor::set_focused(cx, w, true);
        }
    }

    /// UpdateContext @1057.
    pub fn update_context(&self, cx: &mut Cx<'_>, ctx: &str) {
        let list = Self::interactors(cx);
        batch_set_context(cx, &list, ctx);
    }
}

impl Controller for CapturePlayersFocus {
    /// Destruct @1169 -> @427: SetFocused(true) on each stored interactor that IsValid (a widget the page still
    /// holds).
    fn destruct(&mut self, cx: &mut Cx<'_>) {
        for w in self.previous.into_iter().flatten() {
            if !cx.view.is_destroyed(w) {
                interactor::set_focused(cx, w, true);
            }
        }
    }
}

// ---------- UI_InputBindingModal ----------

const H_CANCEL_WITHIN_MODAL: u32 = 1;
const H_SECTION_BAR_CLICKED: u32 = 2;

/// The page is for the player of the modal's Instructions (Conv_IntToString(p), UI_LP_Player_ConfirmReady @654 and
/// UI_PauseMenu @9248): it edits that player's binding set "<action>-P<p>", the set his Local Play device reads.
#[derive(Debug, Default)]
pub struct InputBindingModal {
    base: ModalBase,
    pub player_index: i32,
    bindings_added: bool,
}

impl InputBindingModal {
    pub fn make(_cx: &mut Cx<'_>) -> Box<dyn Controller> {
        Box::new(InputBindingModal::default())
    }

    /// GetInteractors: [buttonDismisser] (non-navigable, Fill; no handler of its click in the graph).
    fn interactors(cx: &Cx<'_>) -> Vec<WidgetId> {
        vec![find(cx, "buttonDismisser")]
    }

    /// Remove Bindings @3011 (guarded by Bindings Added).
    fn remove_bindings(&mut self, cx: &mut Cx<'_>) {
        if !self.bindings_added {
            return;
        }
        self.bindings_added = false;
        let me = cx.w;
        cx.view.manager.on_cancel_within_modal.remove(Handler { owner: me, id: H_CANCEL_WITHIN_MODAL });
    }

    /// Update Context(Context, Player Index) @10.
    pub fn update_context(&mut self, cx: &mut Cx<'_>, ctx: &str, player: i32) {
        self.base.context = ctx.to_string();
        self.player_index = player;
        let own = Self::interactors(cx);
        batch_set_context(cx, &own, ctx);
        for i in own {
            if let Some(s) = cx.view.manager.interactor_mut(i) {
                s.player_index = player;
            }
        }
        let bar = find(cx, "UI_SectionBar_Type");
        cx.visit::<SectionBar, _>(bar, |b, cx| b.update_context(cx, ctx, Some(player)));
        // @274-491 Conditional Visibility(bar, (p != 0 OR IsConsole) OR (IsControllerIdForKeyboard(p) AND
        // GetIsMultikeyboardEnabled), Collapsed, SelfHitTestInvisible): the keyboard / controller bar shows for
        // player 0 alone (players 1..3 bind gamepad keys only). Not a console; multikeyboard is off in the port.
        // Unverified: native.
        wd(cx, bar).vis = if player != 0 { Vis::Collapsed } else { Vis::SelfHitTestInvisible };
        let page = find(cx, "UI_PreferencesControlsRework");
        let p = usize::try_from(player).unwrap_or(0);
        cx.visit::<PreferencesControls, _>(page, |c, cx| {
            c.update_context(cx, ctx, player);
            c.toggle_input_panel(cx, p);
        });
        let cap = find(cx, "UI_CapturePlayersFocus");
        cx.visit::<CapturePlayersFocus, _>(cap, |c, cx| c.update_context(cx, ctx));
    }

    /// The page's binding set in gamepad or keyboard mode.
    fn page_mode(cx: &mut Cx<'_>, keyboard: bool) {
        let page = find(cx, "UI_PreferencesControlsRework");
        let set = cx.view.tree().find_in(page, "Bindings");
        cx.visit::<InputBindingSet, _>(set, |s, cx| s.set_mode(cx, keyboard));
    }
}

impl Modal for InputBindingModal {
    fn pre_show(&mut self, cx: &mut Cx<'_>, id: &str) {
        self.base.pre_show(cx, id);
    }

    /// PostShow @824: Update Context(NewGuid, p), the page's title "Controls: P{p+1}" (@949-1334), the players'
    /// focus captured, the page's Btn_Default focused for p, and the bar's "controller" selected
    /// (SetCurrentIndexByID fires no OnClick, UI_SectionBar @7802). The multikeyboard branch (@1619-1872) is not
    /// taken (multikeyboard off).
    fn post_show(&mut self, cx: &mut Cx<'_>, instructions: &str, _extra_data: &str) {
        let player = instructions.trim().parse::<i32>().unwrap_or(0); // Conv_StringToInt
        let guid = cx.view.manager.new_guid();
        self.update_context(cx, &guid, player);
        let page = find(cx, "UI_PreferencesControlsRework");
        cx.visit::<PreferencesControls, _>(page, |_, cx| {
            PreferencesControls::set_title(cx, &format!("Controls: P{}", player + 1))
        });
        let cap = find(cx, "UI_CapturePlayersFocus");
        cx.visit::<CapturePlayersFocus, _>(cap, |c, cx| c.focus(cx));
        let set = cx.view.tree().find_in(page, "Bindings");
        let def = cx.visit::<InputBindingSet, _>(set, |_, cx| InputBindingSet::default_button_interactor(cx));
        if let Some(i) = def {
            interactor::set_focused(cx, i, false);
        }
        let bar = find(cx, "UI_SectionBar_Type");
        cx.visit::<SectionBar, _>(bar, |b, cx| b.set_current_index_by_id(cx, "controller"));
    }

    /// PreDismiss @1991: PreferencesManager SavePreferences (the program saves the settings it holds), the
    /// multikeyboard conversion not taken, Remove Bindings.
    fn pre_dismiss(&mut self, cx: &mut Cx<'_>) {
        self.remove_bindings(cx);
    }

    fn post_dismiss(&mut self, cx: &mut Cx<'_>) {
        self.base.post_dismiss(cx);
    }

    /// RequestDismiss @699: the manager's DismissModal(Modal ID).
    fn request_dismiss(&mut self, cx: &mut Cx<'_>, reason: &str, from_cancel: bool) {
        self.base.request_dismiss(cx, reason, from_cancel);
    }

    fn get_modal_context(&self) -> String {
        self.base.get_modal_context()
    }
}

impl Controller for InputBindingModal {
    /// OnInitialized @2222 -> Add Bindings @2885: the manager's OnCancelWithinModal; the section bar's OnClick is a
    /// component-bound event.
    fn on_initialized(&mut self, cx: &mut Cx<'_>) {
        self.bindings_added = true;
        let me = cx.w;
        cx.view.manager.on_cancel_within_modal.add(Handler { owner: me, id: H_CANCEL_WITHIN_MODAL });
        let bar = find(cx, "UI_SectionBar_Type");
        cx.visit::<SectionBar, _>(bar, |b, _| b.on_click.add(Handler { owner: me, id: H_SECTION_BAR_CLICKED }));
    }

    /// Destruct @3140 -> Remove Bindings @3011.
    fn destruct(&mut self, cx: &mut Cx<'_>) {
        self.remove_bindings(cx);
    }

    fn handle(&mut self, cx: &mut Cx<'_>, id: u32, arg: &dyn Any) {
        match id {
            // OnCancelWithinModal(Player) @2241: this modal's player's Cancel (player 0's also dismisses the latest
            // modal in the manager's Cancel).
            H_CANCEL_WITHIN_MODAL => {
                if arg.downcast_ref::<i32>().copied() == Some(self.player_index) {
                    self.request_dismiss(cx, "", true);
                }
            }
            // Section bar OnClick @2315: "controller" -> Gamepadmode(p), "keyboard" -> KeyboardMode(p) (NotEqual_StriStri:
            // no case)
            H_SECTION_BAR_CLICKED => {
                if let Some((_, id)) = arg.downcast_ref::<(i32, String)>() {
                    match id.to_lowercase().as_str() {
                        "controller" => Self::page_mode(cx, false),
                        "keyboard" => Self::page_mode(cx, true),
                        _ => {}
                    }
                }
            }
            _ => {}
        }
    }

    fn as_modal(&mut self) -> Option<&mut dyn Modal> {
        Some(self)
    }
}
