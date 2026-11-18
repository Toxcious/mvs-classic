//! UI_PauseMenu: the match pause menu of an offline non-Lab match (UI_PauseMenu
//! offsets: PM below). The match's side is reached through `Cx::pause` only; UI_Game.MatchSetPause calls `on_paused`
//! and `init`.

use std::any::Any;

use super::common::{TeamColorMatch, color_for_team, fill_page};
use super::controls::buttons::ButtonSecondary;
use super::controls::interactor::{batch_set_context, set_focused, set_interactable};
use super::controls::{find, prop_str, wd};
use super::modals::show_modal_generic;
use crate::menu::controller::{Controller, Cx, Handler};
use crate::menu::manager::{Manager, NULL_GUID};
use crate::menu::match_pause::PauseMenuWidget;
use crate::slate::{Vis, WidgetId};

const BUTTONS: [&str; 5] = ["Btn_Resume", "Btn_RestartMatch", "Btn_LeaveMatch", "Btn_SkipStep", "Btn_Settings"];
const CONTROLS: [&str; 4] =
    ["UI_InputLabel_controlsP0", "UI_InputLabel_controlsP1", "UI_InputLabel_controlsP2", "UI_InputLabel_controlsP3"];

const H_RESUME: u32 = 1;
const H_SETTINGS: u32 = 2;
const H_SKIP: u32 = 3;
const H_LEAVE: u32 = 4;
const H_CANCEL: u32 = 10;
const H_TIMER: u32 = 11;
const H_SUBMIT2: u32 = 12;
const H_RESET_KEY: u32 = 13;

/// The match's side is PlayerController 0, the game instance and IsGamePaused. Without a session (the dev view) the
/// menu reads the build's values with no match: not paused, not Local Play, no local player controller with a pawn,
/// and the calls on PlayerController 0 and the game instance are skipped (their casts fail).
#[derive(Debug)]
pub struct PauseMenu {
    is_paused: bool,
    player_pausing: i32,
    is_controller_disconnect: bool,
    pause_time: f64,
    init_timer_id: String,
    /// The widget's SetIsEnabled (Leave Match @3335): the port's Slate has no enabled state of a user widget; the
    /// menu's buttons take no click while it is false (nothing sets it back: the level is left).
    enabled: bool,
}

impl PauseMenu {
    pub fn make(_cx: &mut Cx<'_>) -> Box<dyn Controller> {
        Box::new(PauseMenu {
            is_paused: false,
            player_pausing: -1,
            is_controller_disconnect: false,
            pause_time: 0.0,
            init_timer_id: NULL_GUID.to_string(),
            enabled: true,
        })
    }

    fn button(cx: &Cx<'_>, name: &str) -> WidgetId {
        find(cx, name)
    }

    /// GetInteractors @0-166.
    fn interactors(cx: &Cx<'_>) -> Vec<WidgetId> {
        BUTTONS.iter().map(|b| ButtonSecondary::interactor(cx, Self::button(cx, b))).collect()
    }

    /// UpdateContext @5367 -> @2456: BatchSetContext, then every interactor player 0's alone (only player 0
    /// focuses, navigates and presses them).
    fn update_context(cx: &mut Cx<'_>, ctx: &str) {
        let list = Self::interactors(cx);
        batch_set_context(cx, &list, ctx);
        for i in list {
            if let Some(s) = cx.view.manager.interactor_mut(i) {
                s.player_index = 0;
                s.all_players = false;
            }
        }
    }

    /// UpdateAppearance @5912: the Leave / End label (@5927), Skip Step (@5450), the controls column (@8147, then the
    /// four labels @1974), the cause row (@6400).
    fn update_appearance(&mut self, cx: &mut Cx<'_>) {
        // @5927-6310: "End Match" when CustomGameState.GetLeader() == ClientHydraManager.GetClientGUID(), else "Leave
        // Match".
        // Unverified: both are native and unread for a standalone match; the port takes "Leave Match".
        let leave = Self::button(cx, "Btn_LeaveMatch");
        cx.visit::<ButtonSecondary, _>(leave, |b, cx| b.set_label(cx, "Leave Match"));
        // @5450-5811: Conditional Visibility(GameState BPI_IsTutorialStepSkippable AND mode type 4,
        // SelfHitTestInvisible, Collapsed)
        let skip = cx.pause.as_deref().is_some_and(|s| s.is_tutorial_step_skippable() && s.game_mode_type() == 4);
        let skip_btn = Self::button(cx, "Btn_SkipStep");
        wd(cx, skip_btn).vis = if skip { Vis::SelfHitTestInvisible } else { Vis::Collapsed };
        // @8147-8318: BPI_GetIsLocalPlay (a failed cast: false)
        let lp = cx.pause.as_deref().is_some_and(|s| s.is_local_play());
        let col = find(cx, "buttonContainerControls");
        wd(cx, col).vis = if lp { Vis::SelfHitTestInvisible } else { Vis::Collapsed };
        // @1363-1659: GetPlayerController(i) valid AND its pawn IsLocallyControlled -> Visible, else Collapsed
        for (i, name) in CONTROLS.iter().enumerate() {
            let local = cx.pause.as_deref().is_some_and(|s| s.is_local_player(i as i32));
            let w = find(cx, name);
            wd(cx, w).vis = if local { Vis::Visible } else { Vis::Collapsed };
        }
        // @6400-6434
        let p = self.player_pausing;
        let cause = find(cx, "HBox_Cause");
        wd(cx, cause).vis = if p >= 0 { Vis::SelfHitTestInvisible } else { Vis::Collapsed };
        if p < 0 {
            return;
        }
        // @6537-7075: "P1".."P4" by the index (another: the select's default, empty), Color for Team(index).
        let label = find(cx, "Text_PausingPlayer");
        wd(cx, label).text = ["P1", "P2", "P3", "P4"].get(p as usize).copied().unwrap_or("").to_string();
        // UI_Functions Color for Team with the player index passed as the team index (@6937): in Local Play (config
        // context 4) and with no match (no session: GetMyPlayerData fails, @3229) ColorForIndex(index) straight;
        // otherwise the team style / own team branches (@891-3224) with the session's team style and player 0's
        // team. Never Training: the Lab has its own pause menu.
        let m = cx.pause.as_deref().map(|s| TeamColorMatch {
            local_play: s.is_local_play(),
            training: false,
            team_style: s.team_style(),
            my_team: Some(s.local_team()),
        });
        let color = color_for_team(cx, p, m.as_ref());
        wd(cx, label).color = color;
        // @7116-7891: the select reads Temp_bool_Variable_1, literal false at @7198: the disconnect captions are never
        // taken (Is Controller Disconnect is stored and read nowhere else).
        let _ = self.is_controller_disconnect;
        let cause_text = find(cx, "Text_PauseCause");
        wd(cx, cause_text).text = "Paused!".to_string();
    }

    /// DelayedFocus @9051: Add Timer(InitTimerID, 0.01); the ID is the call's by-ref argument (a new GUID while
    /// invalid, UI_Manager Add Timer @153).
    fn delayed_focus(&mut self, cx: &mut Cx<'_>) {
        self.init_timer_id = Manager::add_timer(cx, &self.init_timer_id.clone(), 0.01);
    }

    /// ToggleInteractable @881 -> @517-927.
    fn toggle_interactable(cx: &mut Cx<'_>, on: bool) {
        for i in Self::interactors(cx) {
            set_interactable(cx, i, on);
        }
    }

    /// OnTimer @8622-9050. AssignInputToWidget(self, -1) (@8814, UI_Functions.SetInputToWidget:
    /// SetInputMode_GameAndUIEx for each player controller) has no counterpart: the port's menu view takes the
    /// window's keys while it is up.
    fn on_timer(&mut self, cx: &mut Cx<'_>, id: &str) {
        if id != self.init_timer_id {
            return;
        }
        Self::toggle_interactable(cx, true);
        cx.view.manager.set_handle_input(true);
        let resume = ButtonSecondary::interactor(cx, Self::button(cx, "Btn_Resume"));
        set_focused(cx, resume, false);
        let ctx = cx.view.manager.interactor(resume).map(|s| s.context.clone()).unwrap_or_default();
        cx.view.manager.set_root_context_for_level(&ctx);
    }

    /// CanUnpause? @25-389: IsGamePaused, visible in hierarchy, no dialog, and 1 s of real time since Init.
    fn can_unpause(&self, cx: &mut Cx<'_>) -> bool {
        if !cx.pause.as_deref().is_some_and(|s| s.is_game_paused()) {
            return false;
        }
        if !cx.view.visible_in_hierarchy(cx.w) {
            return false;
        }
        if Manager::is_dialog_active(cx.view) {
            return false;
        }
        cx.now() - self.pause_time >= 1.0
    }

    /// CanShowInputBindingModal? @0-812: Local Play, visible in hierarchy, neither "settings" nor "inputSettings"
    /// among the manager's active modals, no dialog.
    fn can_show_input_binding_modal(&self, cx: &mut Cx<'_>) -> bool {
        if !cx.pause.as_deref().is_some_and(|s| s.is_local_play()) {
            return false;
        }
        if !cx.view.visible_in_hierarchy(cx.w) {
            return false;
        }
        for id in ["settings", "inputSettings"] {
            if let Some(m) = cx.view.manager.get_modal(id)
                && cx.view.visible_in_hierarchy(m)
            {
                return false;
            }
        }
        !Manager::is_dialog_active(cx.view)
    }

    /// Unpause @5362 -> @2621-3000: PC0.BPI_ResetInputs, SetHandleInput(false), ToggleInteractable(false),
    /// PC0.BPI_Pause(false).
    fn unpause(cx: &mut Cx<'_>) {
        if let Some(s) = cx.pause.as_deref_mut() {
            s.reset_inputs();
        }
        cx.view.manager.set_handle_input(false);
        Self::toggle_interactable(cx, false);
        if let Some(s) = cx.pause.as_deref_mut() {
            s.pause(false);
        }
    }

    /// Leave Match @3153-4172. Step 1 (@3158-3302: FTUEManager.GetIsFTUEActive -> ExitFTUETutorial): the port has no
    /// FTUE, so nothing. Steps 4-6 (BPI_RecreateParty, the transition to New_MainMenu "Lobby", BPI_PlayMusOST) are
    /// the session's `leave_match`; without a session the game instance's casts fail and the transition is the
    /// match's to make, so nothing.
    fn leave_match(&mut self, cx: &mut Cx<'_>) {
        self.enabled = false;
        if let Some(s) = cx.pause.as_deref_mut() {
            s.reset_inputs();
            s.pause(false);
            s.leave_match();
        }
    }
}

impl Controller for PauseMenu {
    /// The ComponentDelegateBindings: Resume @9163, Settings @9168, Skip Step @9173, Leave Match @3153. Restart Match
    /// (@4173: GameState.BPI_Rematch, the Unpause body, BPI_SetMusicMapState(1)) is Collapsed in the tree and never
    /// shown, so its click is left unbound.
    fn on_initialized(&mut self, cx: &mut Cx<'_>) {
        let me = cx.w;
        for (name, id) in [
            ("Btn_Resume", H_RESUME),
            ("Btn_Settings", H_SETTINGS),
            ("Btn_SkipStep", H_SKIP),
            ("Btn_LeaveMatch", H_LEAVE),
        ] {
            let b = Self::button(cx, name);
            cx.visit::<ButtonSecondary, _>(b, |b, _| b.on_click.add(Handler { owner: me, id }));
        }
    }

    /// Port: the 2560 x 1440 SizeBox_3 under the ScaleBox (Stretch None) is widened or heightened to the page's
    /// aspect (fill_page, as the other full-screen menus), so the backdrop reaches the page's edges.
    fn construct(&mut self, cx: &mut Cx<'_>) {
        let b = find(cx, "SizeBox_3");
        fill_page(cx, b);
    }

    fn tick(&mut self, cx: &mut Cx<'_>, _dt: f64) {
        let b = find(cx, "SizeBox_3");
        fill_page(cx, b);
    }

    /// Destruct @4909-5098: the Cancel and Timer bindings are removed. Port: Submit2's too (the port's Event holds the
    /// controller, so it is taken out here). What a player sees is unchanged.
    fn destruct(&mut self, cx: &mut Cx<'_>) {
        let me = cx.w;
        let m = &mut cx.view.manager;
        m.on_cancel.remove(Handler { owner: me, id: H_CANCEL });
        m.on_timer_event.remove(Handler { owner: me, id: H_TIMER });
        m.on_submit2.remove(Handler { owner: me, id: H_SUBMIT2 });
        m.on_reset_key.remove(Handler { owner: me, id: H_RESET_KEY });
    }

    fn handle(&mut self, cx: &mut Cx<'_>, id: u32, arg: &dyn Any) {
        match id {
            H_RESUME if self.enabled => Self::unpause(cx),
            H_SETTINGS if self.enabled => {
                show_modal_generic(cx, false, "UI_PreferencesRework", "settings", "isolated");
            }
            // @5337: BPI_SkipTutorialStep on every BPI_Tutorial of GameState.BPI_GetTutorialActors (@1251), then
            // Unpause. The button is shown in a tutorial alone (UpdateAppearance @5450).
            H_SKIP if self.enabled => {
                if let Some(s) = cx.pause.as_deref_mut() {
                    s.skip_tutorial_step();
                }
                Self::unpause(cx);
            }
            H_LEAVE if self.enabled => self.leave_match(cx),
            // OnUIInputCancel @8574-8607: any player's Cancel (the player is not read).
            H_CANCEL => {
                if self.can_unpause(cx) {
                    Self::unpause(cx);
                }
            }
            H_TIMER => {
                if let Some(t) = arg.downcast_ref::<String>() {
                    self.on_timer(cx, t);
                }
            }
            // OnSubmit2 @9417 -> ShowInputOptionsForPlayer @9178-9405: the player's own controls modal (the index as
            // the modal's Instructions: UI_InputBindingModal PostShow reads Conv_StringToInt(Instructions), IBM @880).
            H_SUBMIT2 => {
                if let Some(p) = arg.downcast_ref::<i32>()
                    && self.can_show_input_binding_modal(cx)
                {
                    show_modal_generic(cx, false, "UI_InputBindingModal", "inputSettings", &p.to_string());
                }
            }
            // OnResetKey @9441: BPI_SkipToTutorialEndStep on every tutorial actor (@2344), then Unpause.
            H_RESET_KEY => {
                if let Some(s) = cx.pause.as_deref_mut() {
                    s.skip_to_tutorial_end_step();
                }
                Self::unpause(cx);
            }
            _ => {}
        }
    }

    fn as_pause_menu(&mut self) -> Option<&mut dyn PauseMenuWidget> {
        Some(self)
    }
}

impl PauseMenuWidget for PauseMenu {
    /// OnPaused @3001-3152.
    fn on_paused(&mut self, cx: &mut Cx<'_>, paused: bool, player_pausing: i32, is_controller_disconnect: bool) {
        self.player_pausing = player_pausing;
        self.is_controller_disconnect = is_controller_disconnect;
        self.update_appearance(cx);
        if !paused {
            self.is_paused = false;
            return;
        }
        if self.is_paused {
            // the pause key while paused (the key toggles)
            Self::unpause(cx);
            self.is_paused = false;
            return;
        }
        self.is_paused = true;
        self.delayed_focus(cx);
    }

    /// Init @4319-4908: UpdateContext(Context), Pause Time, the manager's bindings (AddUnique: bound again at each
    /// Init, once), then UpdateAppearance and DelayedFocus. OnResetKey (@4768-4867) is bound in PIE alone.
    fn init(&mut self, cx: &mut Cx<'_>) {
        let ctx = prop_str(cx, "Context");
        Self::update_context(cx, if ctx.is_empty() { NULL_GUID } else { &ctx });
        self.pause_time = cx.now();
        let me = cx.w;
        let pie = cx.pause.as_deref().is_some_and(|s| s.is_play_in_editor());
        let m = &mut cx.view.manager;
        m.on_cancel.add(Handler { owner: me, id: H_CANCEL });
        m.on_timer_event.add(Handler { owner: me, id: H_TIMER });
        m.on_submit2.add(Handler { owner: me, id: H_SUBMIT2 });
        if pie {
            m.on_reset_key.remove(Handler { owner: me, id: H_RESET_KEY });
            m.on_reset_key.add(Handler { owner: me, id: H_RESET_KEY });
        }
        self.update_appearance(cx);
        self.delayed_focus(cx);
    }
}
