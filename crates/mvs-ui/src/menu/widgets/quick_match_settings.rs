//! Local Play's match settings modal (modal id "localPlayMatchSettings", UI_LocalPlay Show Match Settings @25330):
//! UI_QuickMatchSettings, three drop menus (Game Mode, Ringouts, Time),
//! the hazards toggle and Done / the close button.
//!
//! The settings are the LocalPlay preferences (mvs-settings `LocalPlaySettings`), read at PostShow (Load) and written
//! at PreDismiss (Save), whichever way the modal closes. UI_LocalPlay's OnModalDismissed (@26059) then runs its Handle
//! Match Settings. The program saves the settings it holds.

use std::any::Any;

use serde_json::Value;

use super::common::ModalBase;
use super::common::fill_page;
use super::controls::bar_buttons::P_ID;
use super::controls::buttons::{ButtonMain, IconButton, ToggleSwitch};
use super::controls::drop_menu::DropMenu;
use super::controls::interactor::{self, batch_set_context};
use super::controls::{find, wd};
use super::input_binding_modal::CapturePlayersFocus;
use crate::menu::controller::{Controller, Cx, Handler};
use crate::menu::manager::{Manager, Modal};
use crate::slate::WidgetId;

/// ETeamStyleMC (native: the name table in mvs.exe @0x3b00ca8 lists Solos, Duos, FFA, Other, COUNT; the graph's
/// switches take 0..3 in that order): 0 1v1, 1 Teams, 2 FFA, 3 Other, the class default of CurrentTeamStyle.
const OTHER: usize = 3;

/// Slug To Team Style: "1v1" 0, "2v2" 1, "ffa" 2; any other slug 1 (NotEqual_StriStri: no case).
pub fn slug_to_team_style(slug: &str) -> usize {
    match slug.to_lowercase().as_str() {
        "1v1" => 0,
        "ffa" => 2,
        _ => 1,
    }
}

/// SetModeArt @3972 / Validate @2998-3485: the mode's picture (TeamStyle_BG, SetBrushFromTexture without match size)
/// and its colour-blended art (TeamStyle_FG, SetBrushFromMaterial).
pub const MODE_BG: [Option<&str>; 4] =
    [Some("UI_GameMode_1vs1_BG"), Some("UI_GameMode_2vs2_BG"), Some("UI_GameMode_FFA_BG"), None];
pub const MODE_FG: [Option<&str>; 4] =
    [Some("MI_TextureColorBlend_1v1"), Some("MI_TextureColorBlend_2v2"), Some("MI_TextureColorBlend_FFA"), None];

const MENUS: [&str; 3] = ["UI_DropMenu_TeamStyle", "UI_DropMenu_Ringouts", "UI_DropMenu_Time"];

const H_SUBMIT3: u32 = 1;
const H_MENU_CLICK: u32 = 2;
const H_DONE: u32 = 3;
const H_CLOSE: u32 = 4;

#[derive(Debug)]
pub struct QuickMatchSettings {
    base: ModalBase,
    current_team_style: usize,
    config_changed: bool,
    default_time_selections: Vec<Value>,
}

impl QuickMatchSettings {
    pub fn make(_cx: &mut Cx<'_>) -> Box<dyn Controller> {
        Box::new(QuickMatchSettings {
            base: ModalBase::default(),
            current_team_style: OTHER,
            config_changed: false,
            default_time_selections: Vec::new(),
        })
    }

    fn menu(cx: &Cx<'_>, name: &str) -> WidgetId {
        find(cx, name)
    }

    /// GetInteractors: contextInteractor, buttonDone's, buttonClose's, Toggle_Hazards' interactors.
    fn interactors(cx: &Cx<'_>) -> Vec<WidgetId> {
        let t = cx.view.tree();
        vec![
            find(cx, "contextInteractor"),
            t.find_in(find(cx, "buttonDone"), "UI_Interactor"),
            t.find_in(find(cx, "buttonClose"), "UI_Interactor"),
            t.find_in(find(cx, "Toggle_Hazards"), "UI_Interactor"),
        ]
    }

    /// UpdateContext(Context) @458: BatchSetContext(GetInteractors), the three menus' UpdateContext(Context, 0), the
    /// focus capture's.
    pub fn update_context(&mut self, cx: &mut Cx<'_>, ctx: &str) {
        self.base.context = ctx.to_string();
        let list = Self::interactors(cx);
        batch_set_context(cx, &list, ctx);
        for n in MENUS {
            let m = Self::menu(cx, n);
            cx.visit_dyn(m, |_, cx| DropMenu::update_context(cx, ctx, Some(0)));
        }
        let cap = find(cx, "UI_CapturePlayersFocus");
        cx.visit::<CapturePlayersFocus, _>(cap, |c, cx| c.update_context(cx, ctx));
    }

    fn set_active(cx: &mut Cx<'_>, menu: &str, id: &str) {
        let m = Self::menu(cx, menu);
        cx.visit::<DropMenu, _>(m, |d, cx| d.set_active_by_id(cx, id));
    }

    fn selected(cx: &mut Cx<'_>, menu: &str) -> String {
        let m = Self::menu(cx, menu);
        cx.visit::<DropMenu, _>(m, |d, _| d.current_selected_id.clone()).unwrap_or_default()
    }

    /// Load @759, in its push order: the Game Mode menu set from TeamStyle (0 "1v1", 1 "2v2", 2 "ffa", 3 "2v2"),
    /// Validate, Ringouts from the saved count for styles 0, 1 and 3 only (@1155-1409), Time from the saved seconds,
    /// the hazards toggle, SetModeArt (each step reads Get-LocalPlay-Settings again; nothing between writes them).
    fn load(&mut self, cx: &mut Cx<'_>) {
        let s = cx.prefs.local_play.clone();
        let slug = ["1v1", "2v2", "ffa", "2v2"].get(usize::from(s.team_style)).copied().unwrap_or("");
        Self::set_active(cx, MENUS[0], slug);
        self.validate(cx);
        if [0, 1, 3].contains(&s.team_style) {
            Self::set_active(cx, MENUS[1], &s.ringouts.to_string());
        }
        Self::set_active(cx, MENUS[2], &s.time.to_string());
        let t = find(cx, "Toggle_Hazards");
        cx.visit::<ToggleSwitch, _>(t, |t, cx| t.set_on(cx, s.hazards));
        self.set_mode_art(cx);
    }

    /// Save @1921: TeamStyle, Ringouts, Time (Conv_StringToInt of the menus' CurrentSelectedID) and Hazards written,
    /// then SavePreferences (@2485: the program saves the settings it holds).
    fn save(&mut self, cx: &mut Cx<'_>) {
        let team = slug_to_team_style(&Self::selected(cx, MENUS[0]));
        let ringouts = string_to_int(&Self::selected(cx, MENUS[1]));
        let time = string_to_int(&Self::selected(cx, MENUS[2]));
        let hazards = {
            let t = find(cx, "Toggle_Hazards");
            cx.visit::<ToggleSwitch, _>(t, |t, _| t.is_on).unwrap_or(false)
        };
        let l = &mut cx.prefs.local_play;
        l.team_style = team as u8;
        l.ringouts = ringouts;
        l.time = time;
        l.hazards = hazards;
    }

    /// Validate @2568, in its push order: Config Changed = the menu's style differs from CurrentTeamStyle, which
    /// takes it (@2583-2748); the Ringouts menu interactable for 1v1 and 2v2 alone (@3621); on a change the Time
    /// menu's Items = TimeSelectionsForTeamStyle and its Initialize (@3801-3882); the mode's art, and on a change
    /// its defaults (@2788: 1v1 2 ring-outs and 7 min, FFA 2.5 min and the Ringouts label "Highest Score Wins"
    /// always, else Teams' 4 and 7 min; an unknown slug takes Teams' branch); Config Changed = false (@2776).
    fn validate(&mut self, cx: &mut Cx<'_>) {
        let (team, ringouts, time) = (Self::menu(cx, MENUS[0]), Self::menu(cx, MENUS[1]), Self::menu(cx, MENUS[2]));
        let slug = Self::selected(cx, MENUS[0]).to_lowercase();
        let style = slug_to_team_style(&slug);
        let _ = team;
        self.config_changed = self.current_team_style != style;
        self.current_team_style = style;
        let main = cx.view.tree().find_in(ringouts, "mainInteractor");
        interactor::set_interactable(cx, main, slug == "1v1" || slug == "2v2");
        if self.config_changed {
            let items = self.time_selections_for_team_style(style);
            cx.view.tree_mut().set_prop(time, "Items", Value::Array(items));
            cx.visit::<DropMenu, _>(time, |d, cx| d.initialize(cx));
        }
        let (bg, fg) = (find(cx, "TeamStyle_BG"), find(cx, "TeamStyle_FG"));
        let art = |cx: &mut Cx<'_>, i: usize| {
            wd(cx, bg).tex = MODE_BG[i].map(str::to_string);
            wd(cx, fg).tex = MODE_FG[i].map(str::to_string);
        };
        if slug == "1v1" {
            art(cx, 0);
            if self.config_changed {
                Self::set_active(cx, MENUS[1], "2");
                Self::set_active(cx, MENUS[2], "420");
            }
        } else if slug == "ffa" {
            art(cx, 2);
            let l = cx.view.tree().find_in(ringouts, "WrapSelectionLabel");
            wd(cx, l).text = "Highest Score Wins".to_string();
            if self.config_changed {
                Self::set_active(cx, MENUS[2], "150");
            }
        } else {
            art(cx, 1);
            if self.config_changed {
                Self::set_active(cx, MENUS[1], "4");
                Self::set_active(cx, MENUS[2], "420");
            }
        }
        self.config_changed = false;
    }

    /// TimeSelectionsForTeamStyle(TeamStyle): Default Time Selections; for FFA without the entries whose ID is "0"
    /// (Unlimited).
    fn time_selections_for_team_style(&self, style: usize) -> Vec<Value> {
        let mut result = self.default_time_selections.clone();
        if style == 2 {
            result.retain(|p| p.get(P_ID).and_then(Value::as_str) != Some("0"));
        }
        result
    }

    /// SetModeArt @3972: the art by CurrentTeamStyle (Other: none).
    fn set_mode_art(&self, cx: &mut Cx<'_>) {
        let (bg, fg) = (find(cx, "TeamStyle_BG"), find(cx, "TeamStyle_FG"));
        wd(cx, bg).tex = MODE_BG[self.current_team_style].map(str::to_string);
        wd(cx, fg).tex = MODE_FG[self.current_team_style].map(str::to_string);
    }
}

/// Conv_StringToInt (FCString::Atoi: a leading integer, else 0).
fn string_to_int(s: &str) -> u32 {
    let t = s.trim_start();
    let end = t
        .char_indices()
        .find(|(i, c)| !(c.is_ascii_digit() || (*i == 0 && (*c == '-' || *c == '+'))))
        .map_or(t.len(), |(i, _)| i);
    t[..end].parse::<i64>().map_or(0, |n| n.max(0) as u32)
}

impl Modal for QuickMatchSettings {
    /// PreShow(Modal ID) @40: Default Time Selections = the Time menu's Items, SetupManualNavigation.
    fn pre_show(&mut self, cx: &mut Cx<'_>, id: &str) {
        self.base.pre_show(cx, id);
        let time = Self::menu(cx, MENUS[2]);
        self.default_time_selections =
            cx.view.tree().props(time).get("Items").and_then(Value::as_array).cloned().unwrap_or_default();
        // SetupManualNavigation @4579: buttonClose's interactor goes Down to the Game Mode menu
        let close = cx.view.tree().find_in(find(cx, "buttonClose"), "UI_Interactor");
        let down = cx.view.tree().find_in(Self::menu(cx, MENUS[0]), "mainInteractor");
        if let Some(s) = cx.view.manager.interactor_mut(close) {
            s.manual_down = Some(down);
        }
    }

    /// PostShow @244: EnterAnim, UpdateContext(NewGuid), every player's focus captured, the context interactor's
    /// focus (the context alone), Load.
    fn post_show(&mut self, cx: &mut Cx<'_>, _instructions: &str, _extra_data: &str) {
        let me = cx.w;
        cx.view.play(me, "EnterAnim", false, 1.0);
        let guid = cx.view.manager.new_guid();
        self.update_context(cx, &guid);
        let cap = find(cx, "UI_CapturePlayersFocus");
        cx.visit::<CapturePlayersFocus, _>(cap, |c, cx| c.focus(cx));
        let ctx = find(cx, "contextInteractor");
        interactor::set_focused(cx, ctx, true);
        self.load(cx);
    }

    /// PreDismiss @15: Save.
    fn pre_dismiss(&mut self, cx: &mut Cx<'_>) {
        self.save(cx);
    }

    fn post_dismiss(&mut self, cx: &mut Cx<'_>) {
        self.base.post_dismiss(cx);
    }

    /// RequestDismiss @123: the manager's DismissModal(Modal ID).
    fn request_dismiss(&mut self, cx: &mut Cx<'_>, reason: &str, from_cancel: bool) {
        self.base.request_dismiss(cx, reason, from_cancel);
    }

    fn get_modal_context(&self) -> String {
        self.base.get_modal_context()
    }
}

impl Controller for QuickMatchSettings {
    /// Port: the 2560 x 1440 SizeBox_0 under the ScaleBox is widened or heightened to the page's aspect (fill_page,
    /// as the other full-screen menus), so the blur, the dimmer and the art reach the page's edges.
    fn construct(&mut self, cx: &mut Cx<'_>) {
        let b = find(cx, "SizeBox_0");
        fill_page(cx, b);
    }

    fn tick(&mut self, cx: &mut Cx<'_>, _dt: f64) {
        let b = find(cx, "SizeBox_0");
        fill_page(cx, b);
    }

    /// OnInitialized @4406: the manager's OnSubmit3 -> OnSubmit3 (@4452). Component-bound: each drop menu's OnClick
    /// -> Validate (@3919, @3934, @3939), buttonDone's OnClick -> RequestDismiss("", false) (@3949), buttonClose's
    /// OnClick -> the manager's DismissModal(Modal ID) (@3967 -> @151).
    fn on_initialized(&mut self, cx: &mut Cx<'_>) {
        let me = cx.w;
        cx.view.manager.on_submit3.add(Handler { owner: me, id: H_SUBMIT3 });
        for n in MENUS {
            let m = Self::menu(cx, n);
            cx.visit::<DropMenu, _>(m, |d, _| d.on_click.add(Handler { owner: me, id: H_MENU_CLICK }));
        }
        let done = find(cx, "buttonDone");
        cx.visit::<ButtonMain, _>(done, |b, _| b.on_click.add(Handler { owner: me, id: H_DONE }));
        let close = find(cx, "buttonClose");
        cx.visit::<IconButton, _>(close, |b, _| b.on_click.add(Handler { owner: me, id: H_CLOSE }));
    }

    /// (the port's own) the build leaves the OnSubmit3 binding to the widget's end (the engine drops a dead object's
    /// delegate); the port takes it out when the widget leaves.
    fn destruct(&mut self, cx: &mut Cx<'_>) {
        let me = cx.w;
        cx.view.manager.on_submit3.remove(Handler { owner: me, id: H_SUBMIT3 });
    }

    fn handle(&mut self, cx: &mut Cx<'_>, id: u32, arg: &dyn Any) {
        match id {
            // OnSubmit3(Player Index) @4517: player 0's RequestDismiss("", false)
            H_SUBMIT3 => {
                if arg.downcast_ref::<i32>().copied() == Some(0) {
                    self.request_dismiss(cx, "", false);
                }
            }
            H_MENU_CLICK => self.validate(cx),
            H_DONE => self.request_dismiss(cx, "", false),
            H_CLOSE => {
                let id = self.base.modal_id.clone();
                Manager::dismiss_modal(cx, &id);
            }
            _ => {}
        }
    }

    fn as_modal(&mut self) -> Option<&mut dyn Modal> {
        Some(self)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slugs_map_to_team_styles() {
        assert_eq!(slug_to_team_style("1v1"), 0);
        assert_eq!(slug_to_team_style("FFA"), 2);
        assert_eq!(slug_to_team_style("2v2"), 1);
        assert_eq!(slug_to_team_style("other"), 1);
    }

    #[test]
    fn string_to_int_reads_a_leading_integer() {
        assert_eq!(string_to_int("420"), 420);
        assert_eq!(string_to_int("12ab"), 12);
        assert_eq!(string_to_int("x"), 0);
        assert_eq!(string_to_int(""), 0);
    }
}
