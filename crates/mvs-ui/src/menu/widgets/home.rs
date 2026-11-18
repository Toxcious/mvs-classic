//! The home screen's widgets: WBP_MainButtons (a home screen button), UI_Homescreen_v2
//! and UI_AccountLevel_XPBar.
//!
//! The online layout (the offline handler's bytecode path is not run; the live
//! capture shows the online layout). Deviation: UpdateBadges @10995 runs from OnResourcesLoaded (the online path)
//! and the social updates, never offline (the offline build shows both tutorial buttons and no badge); the port
//! runs its tutorial part on the local TutorialData save at each home screen (`update_badges`), so a player sees
//! their own progress.
//! Owed to the panels' track: the quest tabs', the promo ticker's and the social panel's Update Context in
//! `update_context`, the Arcade modal's SetPracticeEnabled.

use std::any::Any;

use serde_json::{Value, json};

use super::common::{apply_founder_name_treatment, text};
use super::controls::bar_buttons::{BarButton, P_ICON, P_ID, P_LABEL, P_TOOLTIP};
use super::controls::buttons::WHITE;
use super::controls::interactor::{self, batch_set_context, set_focused};
use super::controls::{bind, find, focus_frame, wd};
use super::modals::show_modal_generic;
use super::nav::{GlobalNav, ToolbarIconButton, live};
use super::play_online::PlayOnline;
use super::promo::PromoTickerSmall;
use super::quests::QuestTabs;
use super::section_bar::SectionBar;
use super::social::SocialPanel;
use crate::bundle::Rgba;
use crate::menu::controller::{Controller, Cx, Handler};
use crate::menu::custom_text;
use crate::menu::manager::{DialogParams, Manager, TransitionParams};
use crate::menu::own_profile::account_levels;
use crate::menu::pictures::{PICTURES_HOME, map_pictures, roster_pictures};
use crate::menu::profile_rules::{level_row, shown_level};
use crate::menu::scene::SceneCmd;
use crate::menu::tutorial::{INTRO_TUTORIAL, TUTORIAL_MAP, launch_tutorial};
use crate::menu::view::MenuView;
use crate::slate::{HAlign, Vis, WidgetId};

// ---------- WBP_MainButtons (a home screen button: THE LAB) ----------

const MAIN_BUTTON_ON: Rgba = [1.0, 0.223228, 0.002428, 1.0];

const MB_ON: u32 = 1;
const MB_OFF: u32 = 2;

/// The OnFocus animation's margin track sets the interactor's UUserWidget Padding (Right -40, one constant section): from
/// the first focus on, the interactor's hitbox reaches 40 past the button's right edge, where the text moves to. The
/// value stays after the animation (KeepState; PlayAnimation @454 passes bRestoreState false).
#[derive(Debug, Default)]
pub struct MainButton {
    is_on: bool,
}

impl MainButton {
    pub fn make(_cx: &mut Cx<'_>) -> Box<dyn Controller> {
        Box::new(MainButton::default())
    }
}

impl BarButton for MainButton {
    fn init_bar_button(&mut self, cx: &mut Cx<'_>, params: &Value) {
        let t = find(cx, "ButtonText");
        wd(cx, t).text = text(params.get(P_LABEL));
        self.set_on(cx, self.is_on);
    }

    fn set_on(&mut self, cx: &mut Cx<'_>, v: bool) {
        let changed = v != self.is_on;
        self.is_on = v;
        let i = find(cx, "UI_Interactor");
        if !cx.view.manager.interactor(i).is_some_and(|s| s.is_enabled()) {
            return;
        }
        let t = find(cx, "ButtonText");
        wd(cx, t).color = if v { MAIN_BUTTON_ON } else { WHITE };
        if changed {
            let me = cx.w;
            cx.view.play_animation(me, "OnFocus", !v, 1.0, 1, false, false);
        }
    }

    fn is_on(&self) -> bool {
        self.is_on
    }
}

impl Controller for MainButton {
    fn on_initialized(&mut self, cx: &mut Cx<'_>) {
        let i = find(cx, "UI_Interactor");
        bind(cx, i, |s| &mut s.on_focus, MB_ON);
        bind(cx, i, |s| &mut s.on_unfocus, MB_OFF);
        bind(cx, i, |s| &mut s.on_click, MB_ON);
    }

    fn construct(&mut self, cx: &mut Cx<'_>) {
        let (fc, i) = (find(cx, "UI_FocusedCommon"), find(cx, "UI_Interactor"));
        focus_frame(cx, fc, i);
    }

    fn handle(&mut self, cx: &mut Cx<'_>, id: u32, _arg: &dyn Any) {
        match id {
            MB_ON => self.set_on(cx, true),
            MB_OFF => self.set_on(cx, false),
            _ => {}
        }
    }

    fn as_bar_button(&mut self) -> Option<&mut dyn BarButton> {
        Some(self)
    }
}

// ---------- UI_Homescreen_v2 ----------

// Deviation: CUSTOM GAMES after THE LAB opens the custom game browser, as the PLAY screen's Join Lobby card does, and
// takes CUSTOM GAME's place: the browser's CREATE LOBBY makes a new lobby. The build's list has no such entry.
const BROWSER_ID: &str = "customgames";
const BROWSER_TEXT: &str = "32F116FE401FD7FA44F89C8092021B79"; // CUSTOM GAMES
const QUIT_TITLE: &str = "36924E7E411267B011CEF682C769F7D3"; // THANKS FOR PLAYING!
const QUIT_LEFT: &str = "B44373DE4DAF3C90FA88BCB3086BC546"; // Quit To Desktop
const QUIT_RIGHT: &str = "05C8AD994CF4B72D41E9D9B6FA5CC125"; // Back To Game
const H_CLICK: u32 = 1;
const H_CLICKABLE_FOCUS: u32 = 2;
const H_CLICKABLE_UNFOCUS: u32 = 3;
const H_CLICKABLE_CLICK: u32 = 4;
const H_CANCEL: u32 = 5;
const H_NAV_EXIT: u32 = 6;
const H_QUIT: u32 = 7;
const H_NAV: u32 = 8;
const D_NAV_EXIT: u32 = 11;
const D_PARTY: u32 = 12;
const D_CONTEXT: u32 = 13;
const D_READY: u32 = 14;
const D_TRAINING: u32 = 15;
const H_BACKGROUND: u32 = 16;
const D_BACKGROUND_TIP: u32 = 17;

/// The Game.locres keys of the backgrounds' names by `UiSettings::menu_background` value: Trophy Room
/// (UI_MatchDataTable map-trophyRoom's displayName), FESTIVERSUS (PGI_GlobalData EventMap event_002's
/// EOGProgressionDisplayName), VALENTINEON (UI_Promo_Valentines2023's title). A slug not listed shows as itself.
const BACKGROUND_NAMES: [(&str, &str); 3] = [
    (mvs_settings::MENU_BACKGROUND_OWN, "F4BA67664C2280A9E2C8D5A2AC254BCA"),
    ("event_002", "18239A364AF2BE46CE554790924984B9"),
    ("event_003", "171967A3463C9B032086F7A0543880D5"),
];
/// The background switch's icon (the menu bundle's) and its size, three quarters of the sidebar's 64.
const BACKGROUND_ICON: &str = "ui_icons_swap";
const BACKGROUND_ICON_SIZE: f64 = 48.0;
/// Its tip: centred above the button, clear of the window's right edge.
const BACKGROUND_TIP: ([f64; 2], u8) = ([0.0, -40.0], 7);

/// The background after `current` in `options`, the first after the last (and when `current` is none of them); None
/// with no option.
pub fn next_background<'a>(options: &'a [String], current: &str) -> Option<&'a str> {
    let next = options.iter().position(|o| o == current).map_or(0, |i| (i + 1) % options.len());
    options.get(next).map(String::as_str)
}

/// The name a background shows: its Game.locres line, else its slug.
fn background_name(cx: &Cx<'_>, slug: &str) -> String {
    BACKGROUND_NAMES
        .iter()
        .find(|(s, _)| *s == slug)
        .map(|(_, k)| cx.view.game_text(k))
        .filter(|t| !t.is_empty())
        .unwrap_or(slug)
        .to_owned()
}

/// UI_Homescreen_v2.
#[derive(Debug, Default)]
pub struct Homescreen {
    guid: String,
    pub ready: bool,
    /// The party's fullness as PlayOnline was last told (None: not yet; OnPartyChanged).
    party_full: Option<bool>,
    /// The background switch (`add_background_switch`).
    background_switch: Option<WidgetId>,
}

impl Homescreen {
    /// OnPartyChanged @13806: WBP_PlayOnline.PartyChange(party.full), on each change of the party and
    /// once on the way in.
    fn party_follows(&mut self, cx: &mut Cx<'_>) {
        let full = cx.view.session.online.party_full();
        if self.party_full == Some(full) {
            return;
        }
        self.party_full = Some(full);
        let p = Self::play_online(cx);
        cx.visit::<PlayOnline, _>(p, |p, cx| p.party_change(cx, full));
    }

    pub fn make(_cx: &mut Cx<'_>) -> Box<dyn Controller> {
        Box::new(Homescreen::default())
    }

    /// UpdateBadges @12626-13577 (its tutorial part; the save read by BPI_LoadTutorialQuestData, the stats file's
    /// tutorial record): with BasicTutorialComplete, tutorials Collapsed @12770 and advancedtuts SelfHitTestInvisible
    /// @12932 with its UI_Attention while TutorialsComplete has fewer than 5 entries (@13137 Conditional Visibility,
    /// @13385 Toggle Visibility: MainContainer HitTestInvisible or Collapsed); else advancedtuts Collapsed @13577.
    fn update_badges(&self, cx: &mut Cx<'_>, bar: WidgetId) {
        let r = cx.view.session.stats.tutorial();
        let (tutorials, advanced) = cx
            .visit::<SectionBar, _>(bar, |b, _| (b.button_by_id("tutorials"), b.button_by_id("advancedtuts")))
            .unwrap_or_default();
        if !r.basic_tutorial_complete {
            if let Some(a) = advanced {
                wd(cx, a).vis = Vis::Collapsed;
            }
            return;
        }
        if let Some(t) = tutorials {
            wd(cx, t).vis = Vis::Collapsed;
        }
        let Some(a) = advanced else { return };
        wd(cx, a).vis = Vis::SelfHitTestInvisible;
        // Deviation: the button's "!" badge (shown by the build while fewer than five
        // advanced tutorials are complete) stays hidden, as on the tutorials' own tiles.
        let att = cx.view.tree().find_in(a, "UI_Attention");
        wd(cx, att).vis = Vis::Collapsed;
    }

    /// HandleIfAdvTutorialShortcut @23214 -> @23037 (after Ready, @1922): with the game instance's AdvTutShortcut
    /// (UI_TutorialEndScreen's View More Tutorials) it is cleared @22994 and the advanced tutorials' modal opens
    /// (@22874 -> @19817, without the button's sound).
    fn handle_if_adv_tutorial_shortcut(cx: &mut Cx<'_>) {
        if !cx.view.session.adv_tut_shortcut {
            return;
        }
        cx.view.session.adv_tut_shortcut = false;
        show_modal_generic(cx, false, "WBP_SubOptionModal", "SubOption", "advancedtutorial|");
    }

    fn buttons(cx: &Cx<'_>) -> WidgetId {
        find(cx, "mainbuttonchoices")
    }

    fn play_online(cx: &Cx<'_>) -> WidgetId {
        find(cx, "WBP_PlayOnline")
    }

    /// The main menu widget's own sidebar (the home screen's sibling), None while it is not in the tree.
    fn nav(cx: &Cx<'_>) -> Option<WidgetId> {
        live(cx, "WBP_GlobalNav").into_iter().find(|&n| cx.view.tree().widget(n).name == "WBP_GlobalNav_MainMenu")
    }

    /// The play button's interactor (WBP_PlayOnline.UI_Interactor).
    fn play_button(cx: &Cx<'_>) -> WidgetId {
        let p = Self::play_online(cx);
        cx.view.tree().find_in(p, "UI_Interactor")
    }

    /// IsNothingOpen?: false while a modal or a dialog is up. Owed to the panels' track: the social panel's open state.
    fn is_nothing_open(cx: &mut Cx<'_>) -> bool {
        !Manager::is_modal_active(cx.view) && !Manager::is_dialog_active(cx.view)
    }

    /// UpdateFocus(Force) @14565: with nothing open or Force, the PLAY button takes the focus.
    fn update_focus(cx: &mut Cx<'_>, force: bool) {
        if !force && !Self::is_nothing_open(cx) {
            return;
        }
        let i = Self::play_button(cx);
        set_focused(cx, i, false);
    }

    /// PandaGameInstance.BPI_QuitConfirmation @92621: the dialog @92841 whose left button is OnExitConfirmed @93369:
    /// QuitGame.
    fn quit_confirmation(cx: &mut Cx<'_>) {
        let t = |cx: &Cx<'_>, k: &str| cx.view.game_text(k).to_string();
        let params = DialogParams {
            title: t(cx, QUIT_TITLE),
            button_left_label: t(cx, QUIT_LEFT),
            button_right_label: t(cx, QUIT_RIGHT),
            ..DialogParams::default()
        };
        let d = Manager::show_dialog(cx, params);
        let h = Handler { owner: cx.w, id: H_QUIT };
        if let Some(d) = cx.view.dialog(d) {
            d.button_left_clicked().add(h);
        }
    }

    /// Init(ResourcesLoaded): the flows in their order.
    pub fn init(&mut self, cx: &mut Cx<'_>) {
        cx.view.manager.handle_input = false;
        let me = cx.w;
        cx.view.delay(0.25, Handler { owner: me, id: D_CONTEXT });
        Manager::transition(
            cx,
            TransitionParams { next_level: String::new(), kind: 1, delay: 1.25, force: true, ..Default::default() },
        );
        cx.view.delay(1.25, Handler { owner: me, id: D_READY });
        // @7328 UI_AccountLevel_XPBar.Init(0, 0, GUID): the level and XP literals (the backend's never came).
        // Deviation: the level and XP the profile shows (profile_rules::shown_level), so the two agree
        let (level, xp) = shown_level(account_levels(cx.view));
        let x = find(cx, "UI_AccountLevel_XPBar");
        let guid = self.guid.clone();
        cx.visit::<AccountLevelXPBar, _>(x, |b, cx| b.init(cx, level, xp, &guid));
    }

    /// The background switch: a UI_ToolbarIconButton in the sidebar's Settings look (its square highlight, a smaller
    /// ui_icons_swap), right-aligned above the BUILD label. A click shows the next of the session's backgrounds
    /// (`UiSettings::menu_background`, which the program hands to the renderer), its name in the tip. None with fewer
    /// than two backgrounds.
    // Deviation: the button and its setting are the port's own; the build draws the background its server calendar's
    // events pick and has no switch.
    fn add_background_switch(&mut self, cx: &mut Cx<'_>) {
        if cx.view.session.menu_backgrounds.len() < 2 {
            return;
        }
        let build = find(cx, "BuildNumber");
        let Some(parent) = cx.view.tree().widget(build).parent else { return };
        let w = MenuView::create(cx, "UI_ToolbarIconButton");
        let t = cx.view.tree_mut();
        t.set_prop(w, "Show Amount", json!(false));
        t.set_prop(w, "Icon", json!(BACKGROUND_ICON));
        t.set_prop(w, "IconSize", json!({ "X": BACKGROUND_ICON_SIZE, "Y": BACKGROUND_ICON_SIZE }));
        t.set_prop(w, "UseSquareHighlight", json!(true));
        let x = wd(cx, w);
        x.name = "BackgroundSwitch".into();
        x.h_align = HAlign::Right;
        let at = cx.view.tree().widget(parent).children.iter().position(|&c| c == build);
        MenuView::add_child(cx, parent, w, at);
        let me = cx.w;
        cx.visit::<ToolbarIconButton, _>(w, |b, _| {
            b.tip_place = Some(BACKGROUND_TIP);
            b.on_click.add(Handler { owner: me, id: H_BACKGROUND });
        });
        self.background_switch = Some(w);
        self.background_tip(cx);
    }

    /// The switch's tip: "Menu Background: " and the shown background's name.
    fn background_tip(&self, cx: &mut Cx<'_>) {
        let Some(w) = self.background_switch else { return };
        let name = background_name(cx, &cx.prefs.ui.menu_background).to_uppercase();
        let tip = format!("{}: {name}", custom_text::MENU_BACKGROUND);
        cx.visit::<ToolbarIconButton, _>(w, |b, cx| b.update_texture(cx, BACKGROUND_ICON, &tip));
    }

    /// The switch's click: the next background; its name goes in the tip on the next tick (the button's own click
    /// handler is running: its controller is not reachable now).
    fn next_background(&mut self, cx: &mut Cx<'_>) {
        let options = &cx.view.session.menu_backgrounds;
        let Some(next) = next_background(options, &cx.prefs.ui.menu_background).map(str::to_owned) else { return };
        cx.prefs.ui.menu_background = next;
        let me = cx.w;
        cx.view.delay(0.0, Handler { owner: me, id: D_BACKGROUND_TIP });
    }

    /// The tip's new text, the tip opened again with it (an open tip keeps its text).
    fn background_tip_again(&mut self, cx: &mut Cx<'_>) {
        let Some(w) = self.background_switch else { return };
        self.background_tip(cx);
        let i = ToolbarIconButton::interactor(cx, w);
        if cx.view.manager.interactor(i).is_some_and(|s| s.is_focused) {
            interactor::toggle_tooltip(cx, i, false);
            interactor::toggle_tooltip(cx, i, true);
        }
    }

    /// UpdateContext @15696.
    pub fn update_context(&self, cx: &mut Cx<'_>) {
        let guid = self.guid.clone();
        // @15711
        let q = find(cx, "UI_QuestTabs");
        cx.visit::<QuestTabs, _>(q, |q, cx| q.update_context(cx, &guid));
        let v = find(cx, "VO_Clickable");
        batch_set_context(cx, &[v], &guid);
        // (the port's own background switch)
        if let Some(w) = self.background_switch {
            cx.visit::<ToolbarIconButton, _>(w, |b, cx| b.update_context(cx, &guid));
        }
        if let Some(n) = Self::nav(cx) {
            cx.visit::<GlobalNav, _>(n, |n, cx| n.update_context_global_nav(cx, &guid, true, true));
        }
        let b = Self::buttons(cx);
        cx.visit::<SectionBar, _>(b, |b, cx| b.update_context(cx, &guid, None));
        let p = Self::play_online(cx);
        cx.visit::<PlayOnline, _>(p, |p, cx| p.update_context(cx, &guid));
        // @16399
        let t = find(cx, "UI_PromoTickerSmall");
        cx.visit::<PromoTickerSmall, _>(t, |t, cx| t.update_context(cx, &guid));
        // @15903-15931 GetSocialPanel: the main menu's panel
        for p in live(cx, "UI_SocialPanel") {
            if cx.view.tree().widget(p).name == "UI_SocialPanel_MainMenu" {
                cx.visit::<SocialPanel, _>(p, |p, cx| p.update_context(cx, &guid));
            }
        }
    }

    /// PandaGameInstance.TransitionToTraining: the home screen owns the 0.25 s Delay, which outlives the modals the
    /// first DismissAllModals removes.
    pub fn transition_to_training(cx: &mut Cx<'_>) {
        // Homescreen.live?.transitionToTraining(): the home screen on the viewport
        if let Some(h) = live(cx, "UI_Homescreen_v2").pop() {
            cx.visit::<Homescreen, _>(h, |h, cx| h.run_transition_to_training(cx));
        }
    }

    fn run_transition_to_training(&mut self, cx: &mut Cx<'_>) {
        Manager::dismiss_all_modals(cx);
        Manager::transition(
            cx,
            TransitionParams {
                next_level: "TrainingContainer".into(),
                kind: 0,
                level_options: "Training".into(),
                ..Default::default()
            },
        );
        let me = cx.w;
        cx.view.delay(0.25, Handler { owner: me, id: D_TRAINING });
    }

    /// The list's OnClick switch.
    fn on_click(&mut self, cx: &mut Cx<'_>, id: &str) {
        match id {
            "custom" => {
                Manager::show_modal(cx, "WBP_CustomGame", "customGame", "", "");
                cx.post("ui_menu_global_select_customgame");
            }
            BROWSER_ID => {
                Manager::show_modal(cx, "CustomGameBrowser", "customGames", "", "");
                cx.post("ui_menu_global_select_customgame");
            }
            "training" => {
                self.run_transition_to_training(cx);
                cx.post("ui_menu_global_select_lab");
            }
            "tutorials" => {
                // @20200 ui_menu_global_select_tutorial, then LaunchTutorial(false) @20255 -> @20824:
                // BPI_LaunchTutorial(TutorialActor_Intro, Map_TrophyRoom_Tutorial, Omit_Transition false) @21048
                cx.post("ui_menu_global_select_tutorial");
                launch_tutorial(cx, INTRO_TUTORIAL, TUTORIAL_MAP, false);
            }
            "advancedtuts" => {
                // @19762 the same event, then @19817 ShowModal-Generic(false, WBP_SubOptionModal, "SubOption",
                // "advancedtutorial|")
                cx.post("ui_menu_global_select_tutorial");
                show_modal_generic(cx, false, "WBP_SubOptionModal", "SubOption", "advancedtutorial|");
            }
            "arcade" => {
                // @19264 ShowModal-Generic(true, WBP_MatchOptions, "matchoptions", "Arcade"), then SetPracticeEnabled(false)
                // (owed to the panels' track with the modal)
                show_modal_generic(cx, true, "WBP_MatchOptions", "matchoptions", "Arcade");
            }
            "localplay" => {
                // @19130 BPI_ShowLocalPlayUI -> PandaGameInstance @35962: nothing when the manager has a modal "localPlay"
                if cx.view.manager.get_modal("localPlay").is_some() {
                    return;
                }
                show_modal_generic(cx, false, "UI_LocalPlay", "localPlay", "");
            }
            _ => {}
        }
    }
}

impl Controller for Homescreen {
    fn tick(&mut self, cx: &mut Cx<'_>, _dt: f64) {
        if self.party_full.is_some() {
            self.party_follows(cx);
        }
    }

    fn on_initialized(&mut self, cx: &mut Cx<'_>) {
        let me = cx.w;
        self.guid = cx.view.manager.new_guid();
        let guid = self.guid.clone();
        cx.view.manager.set_root_context_for_level(&guid);
        cx.view.manager.navigation_start.insert(guid, Handler { owner: me, id: H_NAV });
        // @8302: "BUILD: {changelist}"; the debug container goes (IsShippingBuild)
        let t = find(cx, "BuildNumber");
        wd(cx, t).text = "BUILD: 80480".into();
        let dbg = find(cx, "DebugContainer");
        MenuView::remove_child(cx, dbg);
        // The click target over the 3D character (the mouse only; OnFocus @14989, OnClick @15037).
        let c = find(cx, "VO_Clickable");
        bind(cx, c, |s| &mut s.on_focus, H_CLICKABLE_FOCUS);
        bind(cx, c, |s| &mut s.on_unfocus, H_CLICKABLE_UNFOCUS);
        bind(cx, c, |s| &mut s.on_click, H_CLICKABLE_CLICK);
        let bar = Self::buttons(cx);
        cx.visit::<SectionBar, _>(bar, |b, _| b.on_click.add(Handler { owner: me, id: H_CLICK }));
        // @8177 Delay(0) -> the sidebar's exit bound to OnGlobalNavExit, ToggleExitButton(true)
        cx.view.delay(0.0, Handler { owner: me, id: D_NAV_EXIT });
        // @9528 the manager's OnCancel bound to StepBack
        cx.view.manager.on_cancel.add(Handler { owner: me, id: H_CANCEL });
        // OnPartyChanged once after the PLAY button's Construct
        cx.view.delay(0.0, Handler { owner: me, id: D_PARTY });
        let label = json!({ "CultureInvariantString": cx.view.game_text(BROWSER_TEXT) });
        let bp =
            json!({ P_LABEL: label, P_ICON: null, P_ID: BROWSER_ID, P_TOOLTIP: { "CultureInvariantString": null } });
        cx.visit::<SectionBar, _>(bar, |b, cx| {
            b.add_option(cx, bp, "training");
            b.remove_option(cx, "custom");
        });
        self.update_badges(cx, bar);
        self.add_background_switch(cx);
        // (the port's own) the lobby's map thumbnail and the Collection's portraits, held before the first frame
        let mut names: Vec<String> = roster_pictures(cx.view);
        names.extend(map_pictures(cx.view, &["map_m011_largenoplat"]).into_iter().flatten());
        cx.view.hold_pictures(PICTURES_HOME, names.iter().map(String::as_str));
    }

    /// PreConstruct @18713 (WBP_PlayOnline 4).
    fn pre_construct(&mut self, cx: &mut Cx<'_>) {
        for (n, v) in [
            ("HorizontalBox_NewMenu", Vis::SelfHitTestInvisible),
            ("WBP_PlayOnline", Vis::SelfHitTestInvisible),
            // UI_QuestTabs Collapsed, as the offline handler's @18076 (Unverified: the capture's state)
            ("UI_QuestTabs", Vis::Collapsed),
            // Deviation: the double rewards' designer banners are Collapsed
            ("DoubleRewardContainer", Vis::Collapsed),
        ] {
            let w = find(cx, n);
            wd(cx, w).vis = v;
        }
    }

    fn destruct(&mut self, cx: &mut Cx<'_>) {
        let me = cx.w;
        cx.view.manager.on_cancel.remove(Handler { owner: me, id: H_CANCEL });
        cx.view.release_pictures(PICTURES_HOME);
        cx.view.manager.navigation_start.remove(&self.guid);
    }

    fn resume(&mut self, cx: &mut Cx<'_>, id: u32) {
        let me = cx.w;
        match id {
            D_NAV_EXIT => {
                if let Some(n) = Self::nav(cx) {
                    cx.visit::<GlobalNav, _>(n, |n, cx| {
                        n.exit_event.clear();
                        n.exit_event.add(Handler { owner: me, id: H_NAV_EXIT });
                        n.exit_bound = true;
                        n.toggle_exit_button(cx, true);
                    });
                }
            }
            D_PARTY => {
                self.party_follows(cx);
            }
            D_CONTEXT => self.update_context(cx),
            D_BACKGROUND_TIP => self.background_tip_again(cx),
            // The readiness loop @1321: a 0.25 s poll while a transition runs or the manager waits, then @1541
            // SetHandleInput(true), UpdateFocus(false), allowCursorMode.
            D_READY => {
                if cx.view.manager.is_transitioning() || cx.view.manager.is_waiting {
                    cx.view.delay(0.25, Handler { owner: me, id: D_READY });
                    return;
                }
                cx.view.manager.handle_input = true;
                Self::update_focus(cx, false);
                cx.view.manager.allow_cursor_mode = true;
                self.ready = true;
                Self::handle_if_adv_tutorial_shortcut(cx);
            }
            D_TRAINING => {
                Manager::dismiss_all_modals(cx);
                let mut root = cx.w;
                while let Some(p) = cx.view.tree().widget(root).parent {
                    root = p;
                }
                MenuView::remove_from_viewport(cx, root);
            }
            _ => {}
        }
    }

    fn handle(&mut self, cx: &mut Cx<'_>, id: u32, arg: &dyn Any) {
        match id {
            H_CLICK => {
                if let Some((_, sid)) = arg.downcast_ref::<(i32, String)>() {
                    self.on_click(cx, sid);
                }
            }
            // OnFocus @14989 / OnUnfocus @15648: UI_Functions.CharacterHover: the Player1 actor's hover light for the
            // first player, then a hover posts ui_menu_character_hover
            H_CLICKABLE_FOCUS | H_CLICKABLE_UNFOCUS => {
                let on = id == H_CLICKABLE_FOCUS;
                if arg.downcast_ref::<i32>() == Some(&0) {
                    cx.scene(SceneCmd::Hover { slot: 0, on });
                }
                if on {
                    cx.post("ui_menu_character_hover");
                }
            }
            // OnClick @15037: the Player1 preview's clickable voice line, then @13750 ui_menu_confirm.
            // Deviation: the program posts the line after the frame's posts, so after ui_menu_confirm (the same frame).
            H_CLICKABLE_CLICK => {
                cx.scene(SceneCmd::ClickVo);
                cx.post("ui_menu_confirm");
            }
            // StepBack(Player Index, Is Escape) @17278: with nothing open, ClickedQuit @14696
            H_CANCEL => {
                if Self::is_nothing_open(cx) {
                    Self::quit_confirmation(cx);
                }
            }
            H_NAV_EXIT => Self::quit_confirmation(cx),
            H_QUIT => cx.view.session.quit = true,
            H_BACKGROUND => self.next_background(cx),
            _ => {}
        }
    }

    /// The manager's navigationStart: the PLAY button (the port's own).
    fn navigation_start(&mut self, cx: &mut Cx<'_>, id: u32) -> Option<WidgetId> {
        (id == H_NAV).then(|| Self::play_button(cx))
    }
}

// ---------- UI_AccountLevel_XPBar (the account level bar over the list) ----------

/// UI_AccountLevel_XPBar.
#[derive(Debug, Default)]
pub struct AccountLevelXPBar;

impl AccountLevelXPBar {
    pub fn make(_cx: &mut Cx<'_>) -> Box<dyn Controller> {
        Box::new(AccountLevelXPBar)
    }

    /// Init(Current Level, Current XP, Context) @1330. Required XP @1672: the level-up table's row Level - 1 when that
    /// index is valid, else its last row (PGI_GlobalData's AccountLevelUpTable). AccountLevel (Hidden in the build) gets
    /// the level and is shown (Deviation: nothing in the build shows it); the bar's Percentage is XP / Required XP;
    /// XPNumbers (Collapsed) "{CurrentXP}/{RequiredXP}".
    pub fn init(&mut self, cx: &mut Cx<'_>, level: i64, xp: f64, ctx: &str) {
        let i = find(cx, "UI_Interactor");
        interactor::set_context(cx, i, ctx);
        let l = find(cx, "AccountLevel");
        let x = wd(cx, l);
        x.text = level.to_string();
        x.vis = Vis::HitTestInvisible;
        let required = level_row(account_levels(cx.view), level).xp;
        let pct = crate::menu::profile_rules::xp_fraction(xp, required) as f32;
        let bar = find(cx, "XPBar");
        let w = wd(cx, bar);
        if let Some(q) = &mut w.quad {
            match q.iter_mut().find(|e| e.0 == "Percentage") {
                Some(e) => e.1 = pct,
                None => q.push(("Percentage".to_string(), pct)),
            }
        }
        if let Some(m) = &mut w.mid {
            m.set_scalar("Percentage", pct);
        }
        let n = find(cx, "XPNumbers");
        wd(cx, n).text = format!("{xp}/{required}");
    }
}

impl Controller for AccountLevelXPBar {
    /// Construct @1284: Text_Name is GetName, the player's name with the founder treatment (Deviation: every player is a
    /// founder, common.rs).
    fn construct(&mut self, cx: &mut Cx<'_>) {
        let (fc, i) = (find(cx, "UI_FocusedCommon"), find(cx, "UI_Interactor"));
        focus_frame(cx, fc, i);
        let n = find(cx, "Text_Name");
        apply_founder_name_treatment(cx, n);
        wd(cx, n).text = cx.prefs.profile.name.clone();
    }

    /// (the name is written when it changes)
    fn tick(&mut self, cx: &mut Cx<'_>, _dt: f64) {
        let n = find(cx, "Text_Name");
        let name = cx.prefs.profile.name.clone();
        if wd(cx, n).text != name {
            wd(cx, n).text = name;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_background_switch_cycles_every_option() {
        let options: Vec<String> = ["", "event_002", "event_003"].map(str::to_owned).into();
        assert_eq!(next_background(&options, ""), Some("event_002"));
        assert_eq!(next_background(&options, "event_002"), Some("event_003"));
        assert_eq!(next_background(&options, "event_003"), Some(""));
        // a saved slug no longer in the store: the first
        assert_eq!(next_background(&options, "event_009"), Some(""));
        assert_eq!(next_background(&[], ""), None);
        // a name for each of the build's backgrounds
        assert_eq!(BACKGROUND_NAMES.map(|(s, _)| s), ["", "event_002", "event_003"]);
    }
}
