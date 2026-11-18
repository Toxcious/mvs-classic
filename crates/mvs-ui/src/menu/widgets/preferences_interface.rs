//! UI_PreferencesInterfaceRework, the Settings panel's Interface page, and its live preview (UI_IGv3_LivePreview
//! and its Character, HUDFixed and Scoreboard parts). The rules of
//! the rows are `menu::prefs`.
//!
//! The page: the live preview, the preset bar (Recommended, Legacy, Custom), the position rows, the five colour
//! pickers, the outline and team-colour toggles (turning one off asks first), the Knockback Influence toggle, cursor
//! speed and Restore Defaults (the port's own rows are on the Classic page, `preferences_classic.rs`). Every pick is
//! written into `Settings::ui` at once (the program saves it) and broadcast as PreferencesChangedDispatch. The
//! analytics events and the interface GUID (OnPreferencesChanged @25486) are not ported.
// Deviation: PM's OnUIPrefsChanged broadcast has no listener list here: the page refreshes its preview widgets after
// each write (the previews are the only readers while the Settings panel is open; a match reads the settings when it
// starts).

use std::any::Any;

use serde_json::Value;

use super::controls::buttons::{ButtonSecondary, PlusMinusHorizontal, ToggleSwitch};
use super::controls::color_picker::ColorPicker;
use super::controls::drop_menu::DropMenu;
use super::controls::interactor::{batch_set_context, batch_set_scroll_box, set_focused};
use super::controls::{find, wd};
use super::section_bar::SectionBar;
use crate::bundle::Rgba;
use crate::menu::controller::{Controller, Cx, Event, Handler};
use crate::menu::manager::{DialogParams, Manager, NULL_GUID};
use crate::menu::prefs::{self, Outline, UiElements};
use crate::menu::scroll::scroll_list;
use crate::menu::widgets::common::palette_entry;
use crate::slate::{HAlign, Margin, VAlign, Vis, WidgetId};

/// The position rows other than the scoreboard's, by the pair each writes (world = on the fighter, hud = on the
/// scoreboard).
const POSITION_ROWS: [(&str, UiElements); 4] = [
    ("CooldownDropdown", UiElements::Cooldowns),
    ("DamagePositionDropdown", UiElements::Percentage),
    ("PlayerNameDropdown", UiElements::Username),
    ("DodgeMeterDropdown", UiElements::DodgeMeters),
];
/// The colour pickers by Get-UI-Color Binding index (Refresh UI Toggles @3301-3857, the handlers @20441-22125).
const PICKERS: [(&str, usize); 5] = [
    ("UI_MeColorPicker", 2),
    ("UI_Player4ColorPicker", 4),
    ("UI_Player3ColorPicker", 3),
    ("UI_EnemyColorPicker_1", 1),
    ("UI_AllyColorPicker", 0),
];
/// GetInteractors: these toggles' and buttons' UI_Interactor (UI_CursorSpeed's own is left out: the port's
/// PlusMinusHorizontal takes the focus on its buttons only), then the drop menus', the pickers' and the plus / minus
/// buttons.
const TOGGLES: [&str; 6] = [
    "UI_ToggleAllyOutline",
    "ResetDefaultsButton",
    "UI_ToggleEnemyOutline",
    "UI_ToggleTeamColoredSkins",
    "UI_ToggleShowDI",
    "UI_ToggleMyOutline",
];
const DROPDOWNS: [&str; 5] = [
    "CooldownDropdown",
    "DamagePositionDropdown",
    "PlayerNameDropdown",
    "DodgeMeterDropdown",
    "ScoreboardPositionDropdown",
];
const PICKER_ORDER: [&str; 5] = [
    "UI_AllyColorPicker",
    "UI_EnemyColorPicker_1",
    "UI_MeColorPicker",
    "UI_Player3ColorPicker",
    "UI_Player4ColorPicker",
];
/// The "not recommended" dialogs' texts (ubergraph @5111-5619, @6585-6778, @10272-10465, @11628-11829).
const OUTLINE_BODY: &str = "067101DC468CE87E61FD548E33EA0682";
const TEAM_COLOR_BODY: &str = "62E22AAC42CD30AD7002C9BC31D14624";
const TITLE_A: &str = "72533455428B79A0959C078E3AD7F22E";
const TITLE_B: &str = "C12F28E7496A53D0BAD7A1BEBE343EF4";
const YES: &str = "424583CF48092C268CAA03BF14E171AD";
const NO: &str = "B612CFA447B6F0344995D0A6C5C40013";

const H_SCOREBOARD: u32 = 1;
const H_POSITION: u32 = 2; // + row
const H_COLOR: u32 = 10; // + picker
const H_OUTLINE: u32 = 20; // + Outline
const H_TEAM_SKINS: u32 = 25;
const H_DI: u32 = 26;
const H_CURSOR: u32 = 27;
const H_PRESET: u32 = 28;
const H_RESTORE: u32 = 29;
const H_SCROLL: u32 = 30;
const H_REFRESH_PRESETS: u32 = 31;
const H_CONFIRM: u32 = 50; // + Outline (3: team skins)
const H_CANCEL: u32 = 60; // + Outline (3: team skins)

const OUTLINES: [(Outline, &str); 3] = [
    (Outline::Ally, "UI_ToggleAllyOutline"),
    (Outline::Enemy, "UI_ToggleEnemyOutline"),
    (Outline::My, "UI_ToggleMyOutline"),
];

#[derive(Debug)]
pub struct PreferencesInterface {
    pub preferences_changed_dispatch: Event<()>,
    /// Current Context.
    context: String,
    /// The live preview's readers of the preferences.
    previews: Vec<WidgetId>,
}

impl PreferencesInterface {
    pub fn make(_cx: &mut Cx<'_>) -> Box<dyn Controller> {
        Box::new(PreferencesInterface {
            preferences_changed_dispatch: Event::default(),
            context: NULL_GUID.to_string(),
            previews: Vec::new(),
        })
    }

    fn scrollbox(cx: &Cx<'_>) -> WidgetId {
        find(cx, "interfaceScrollBox")
    }

    fn bar(cx: &Cx<'_>) -> WidgetId {
        find(cx, "UI_PresetSectionBar")
    }

    fn menu<R>(cx: &mut Cx<'_>, name: &str, f: impl FnOnce(&mut DropMenu, &mut Cx<'_>) -> R) -> Option<R> {
        let w = find(cx, name);
        cx.visit::<DropMenu, _>(w, f)
    }

    fn toggle_on(cx: &mut Cx<'_>, name: &str, on: bool) {
        let w = find(cx, name);
        cx.visit::<ToggleSwitch, _>(w, |t, cx| t.set_on(cx, on));
    }

    fn picker<R>(cx: &mut Cx<'_>, name: &str, f: impl FnOnce(&mut ColorPicker, &mut Cx<'_>) -> R) -> Option<R> {
        let w = find(cx, name);
        cx.visit::<ColorPicker, _>(w, f)
    }

    fn cursor<R>(cx: &mut Cx<'_>, f: impl FnOnce(&mut PlusMinusHorizontal, &mut Cx<'_>) -> R) -> Option<R> {
        let w = find(cx, "UI_CursorSpeed");
        cx.visit::<PlusMinusHorizontal, _>(w, f)
    }

    pub fn interactors(&self, cx: &mut Cx<'_>) -> Vec<WidgetId> {
        let mut list = Vec::new();
        for n in TOGGLES {
            let w = find(cx, n);
            list.push(cx.view.tree().find_in(w, "UI_Interactor"));
        }
        for n in DROPDOWNS {
            list.extend(Self::menu(cx, n, |_, cx| DropMenu::interactors(cx)).unwrap_or_default());
        }
        for n in PICKER_ORDER {
            list.extend(Self::picker(cx, n, |p, cx| p.interactors(cx)).unwrap_or_default());
        }
        list.extend(Self::cursor(cx, |_, cx| PlusMinusHorizontal::interactors(cx)).unwrap_or_default());
        list
    }

    /// Update Context.
    pub fn update_context(&mut self, cx: &mut Cx<'_>, ctx: &str) {
        self.context = ctx.to_string();
        let list = self.interactors(cx);
        batch_set_context(cx, &list, ctx);
        let bar = Self::bar(cx);
        cx.visit::<SectionBar, _>(bar, |b, cx| b.update_context(cx, ctx, Some(0)));
    }

    /// FocusFirst @23522.
    pub fn focus_first(&self, cx: &mut Cx<'_>) {
        let m = find(cx, "ScoreboardPositionDropdown");
        let i = cx.view.tree().find_in(m, "mainInteractor");
        set_focused(cx, i, false);
    }

    /// SetupScrolling @22525.
    fn setup_scrolling(&mut self, cx: &mut Cx<'_>) {
        let sb = Self::scrollbox(cx);
        let me = cx.w;
        cx.view.manager.on_scroll.add(Handler { owner: me, id: H_SCROLL });
        cx.view.manager.register_scrollbox(sb);
        let list = self.interactors(cx);
        batch_set_scroll_box(cx, &list, sb);
    }

    /// The preview's readers refreshed (OnUIPrefsChanged's listeners).
    fn refresh_previews(&self, cx: &mut Cx<'_>) {
        for &w in &self.previews {
            cx.visit_dyn(w, |c, cx| {
                let any: &mut dyn Any = c;
                if let Some(p) = any.downcast_mut::<LivePreview>() {
                    p.refresh(cx);
                } else if let Some(p) = any.downcast_mut::<LivePreviewCharacter>() {
                    p.refresh(cx);
                } else if let Some(p) = any.downcast_mut::<LivePreviewHudFixed>() {
                    p.refresh(cx);
                } else if let Some(p) = any.downcast_mut::<LivePreviewScoreboard>() {
                    p.refresh(cx);
                }
            });
        }
    }

    /// A PM Set-UI-* call's broadcast (UpdateUIPrefs -> PrefsChanged-UI).
    fn broadcast(&self, cx: &mut Cx<'_>) {
        self.refresh_previews(cx);
    }

    /// The tail of the position, outline and team-colour handlers (@5876, @8907, @13622, @15503, @25631):
    /// RefreshPresets, SavePreferences, PreferencesChangedDispatch.
    fn tail_presets(&mut self, cx: &mut Cx<'_>) {
        Self::refresh_presets(cx);
        self.tail(cx);
    }

    /// The tail of the colour, Knockback Influence and cursor handlers (@20831): SavePreferences (the program
    /// saves the settings), PreferencesChangedDispatch.
    fn tail(&mut self, cx: &mut Cx<'_>) {
        cx.emit(self, |c: &PreferencesInterface| &c.preferences_changed_dispatch, &());
    }

    /// Refresh UI Toggles (@869): every row from the preferences, in the graph's order. UI_CursorSpeed.SetValue
    /// broadcasts OnValueChanged, so this runs the cursor handler (a write, a save, a dispatch).
    fn refresh_ui_toggles(&mut self, cx: &mut Cx<'_>) {
        let ui = cx.prefs.ui.clone();
        Self::toggle_on(cx, "UI_ToggleAllyOutline", ui.world_show_ally_outlines);
        Self::toggle_on(cx, "UI_ToggleEnemyOutline", ui.world_show_enemy_outlines);
        Self::toggle_on(cx, "UI_ToggleMyOutline", ui.world_show_my_outline);
        let pos = if ui.hud_position_bottom { "bottom" } else { "top" };
        Self::menu(cx, "ScoreboardPositionDropdown", |m, cx| m.set_active_by_id(cx, pos));
        Self::toggle_on(cx, "UI_ToggleTeamColoredSkins", ui.world_use_team_colored_skins);
        for (name, kind) in &POSITION_ROWS[..3] {
            let id = prefs::position_id(&ui, *kind);
            Self::menu(cx, name, |m, cx| m.set_active_by_id(cx, id));
        }
        for (name, b) in PICKERS {
            let row = prefs::color_binding(&ui, b).to_string();
            Self::picker(cx, name, |p, cx| p.set_selected_color_by_color_id(cx, &row));
        }
        // @4042-4225: steps 2, 4 and 8 come back one lower and the SetValue's broadcast writes and saves that step
        let step = prefs::cursor_speed_step(&ui);
        Self::cursor(cx, |r, cx| r.set_value(cx, step));
        Self::toggle_on(cx, "UI_ToggleShowDI", ui.world_show_di);
        let id = prefs::position_id(&ui, UiElements::DodgeMeters);
        Self::menu(cx, "DodgeMeterDropdown", |m, cx| m.set_active_by_id(cx, id));
    }

    /// RefreshPresets (@18242): the bar shows Legacy or Recommended when the compared fields match one, else
    /// Custom. (SetCurrentIndexByID does not broadcast: the bar moves only.)
    fn refresh_presets(cx: &mut Cx<'_>) {
        let id = prefs::preset_id(&cx.prefs.ui);
        let bar = Self::bar(cx);
        cx.visit::<SectionBar, _>(bar, |b, cx| b.set_current_index_by_id(cx, id));
    }

    /// Ally @4946, Enemy @6420, My @10107 outline: turned off (the preference on), the dialog asks first; turned on,
    /// the confirm path at once (@5992, @7355, @11038: Set-UI-ToggleWorld<X>Outlines flips the field). Ally and Enemy
    /// fall to the tail as the dialog shows (nothing written); My does not.
    fn on_outline(&mut self, cx: &mut Cx<'_>, k: usize) {
        let who = OUTLINES[k].0;
        if !prefs::outline(&cx.prefs.ui, who) {
            self.confirm_outline(cx, k);
            return;
        }
        let title = if who == Outline::Enemy { TITLE_B } else { TITLE_A };
        Self::not_recommended(cx, OUTLINE_BODY, title, k as u32);
        if who != Outline::My {
            self.tail_presets(cx);
        }
    }

    fn confirm_outline(&mut self, cx: &mut Cx<'_>, k: usize) {
        let who = OUTLINES[k].0;
        let cur = prefs::outline(&cx.prefs.ui, who);
        prefs::set_outline(&mut cx.prefs.ui, who, !cur);
        self.broadcast(cx);
        self.tail_presets(cx);
    }

    /// Team-coloured skins @11463: as the outlines (dialog @11628-11829), confirm @12406
    /// Set-World-UseTeamColoredSkins(!current), then the tail.
    fn on_team_colored_skins(&mut self, cx: &mut Cx<'_>) {
        if !cx.prefs.ui.world_use_team_colored_skins {
            self.confirm_team_skins(cx);
            return;
        }
        Self::not_recommended(cx, TEAM_COLOR_BODY, TITLE_B, 3);
        self.tail_presets(cx);
    }

    fn confirm_team_skins(&mut self, cx: &mut Cx<'_>) {
        let u = &mut cx.prefs.ui;
        u.world_use_team_colored_skins = !u.world_use_team_colored_skins;
        self.broadcast(cx);
        self.tail_presets(cx);
    }

    /// The "not recommended" dialog: left YES confirms (turns the option off), right NO draws the toggle on again. (A
    /// dialog closed by neither button leaves the toggle drawn off with the field on, as the build: no dismissed
    /// delegate is bound.)
    fn not_recommended(cx: &mut Cx<'_>, body: &str, title: &str, k: u32) {
        let t = |cx: &Cx<'_>, k: &str| cx.view.game_text(k).to_string();
        let params = DialogParams {
            title: t(cx, title),
            body: t(cx, body),
            button_left_label: t(cx, YES),
            button_right_label: t(cx, NO),
            ..DialogParams::default()
        };
        let d = Manager::show_dialog(cx, params);
        let me = cx.w;
        if let Some(d) = cx.view.dialog(d) {
            d.button_left_clicked().add(Handler { owner: me, id: H_CONFIRM + k });
            d.button_right_clicked().add(Handler { owner: me, id: H_CANCEL + k });
        }
    }

    /// The preset bar's OnClick @16695: recommended (@18015) and legacy (@16803) write their preset, then @17222;
    /// custom writes nothing.
    fn on_preset(&mut self, cx: &mut Cx<'_>, id: &str) {
        match id.to_lowercase().as_str() {
            "recommended" => prefs::set_recommended_preset(&mut cx.prefs.ui),
            "legacy" => prefs::set_legacy_preset(&mut cx.prefs.ui),
            _ => return,
        }
        self.broadcast(cx);
        self.preset_tail(cx);
    }

    /// @17222: colour bindings 0, 1, 2 to PM's defaults, every outline and team colours on, Refresh UI Toggles,
    /// SavePreferences, the dispatch.
    fn preset_tail(&mut self, cx: &mut Cx<'_>) {
        prefs::preset_tail(&mut cx.prefs.ui);
        self.broadcast(cx);
        self.refresh_ui_toggles(cx);
        self.tail(cx);
    }

    /// Restore Defaults @23030: the bar on Recommended, the outline and team-colour toggles on, the Recommended
    /// preset (@18015 -> @17222), then UI_CursorSpeed.ResetToDefault (its broadcast writes step 5, 1.0). Team3 /
    /// Team4 colours are left.
    fn on_restore_defaults(&mut self, cx: &mut Cx<'_>) {
        let bar = Self::bar(cx);
        cx.visit::<SectionBar, _>(bar, |b, cx| b.set_current_index_by_id(cx, "recommended"));
        for (_, n) in OUTLINES {
            Self::toggle_on(cx, n, true);
        }
        Self::toggle_on(cx, "UI_ToggleTeamColoredSkins", true);
        prefs::set_recommended_preset(&mut cx.prefs.ui);
        self.broadcast(cx);
        self.preset_tail(cx);
        Self::cursor(cx, |r, cx| r.reset_to_default(cx));
    }
}

impl Controller for PreferencesInterface {
    fn on_initialized(&mut self, cx: &mut Cx<'_>) {
        // the component bound events (bound before OnInitialized runs)
        let me = cx.w;
        let h = |id| Handler { owner: me, id };
        Self::menu(cx, "ScoreboardPositionDropdown", |m, _| m.on_click.add(h(H_SCOREBOARD)));
        for (i, (name, _)) in POSITION_ROWS.iter().enumerate() {
            Self::menu(cx, name, |m, _| m.on_click.add(h(H_POSITION + i as u32)));
        }
        for (i, (name, _)) in PICKERS.iter().enumerate() {
            Self::picker(cx, name, |p, _| p.on_color_selected.add(h(H_COLOR + i as u32)));
        }
        for (i, (_, name)) in OUTLINES.iter().enumerate() {
            let w = find(cx, name);
            cx.visit::<ToggleSwitch, _>(w, |t, _| t.on_toggle_changed.add(h(H_OUTLINE + i as u32)));
        }
        for (name, id) in [("UI_ToggleTeamColoredSkins", H_TEAM_SKINS), ("UI_ToggleShowDI", H_DI)] {
            let w = find(cx, name);
            cx.visit::<ToggleSwitch, _>(w, |t, _| t.on_toggle_changed.add(h(id)));
        }
        Self::cursor(cx, |r, _| r.on_value_changed.add(h(H_CURSOR)));
        let bar = Self::bar(cx);
        cx.visit::<SectionBar, _>(bar, |b, _| b.on_click.add(h(H_PRESET)));
        let reset = find(cx, "ResetDefaultsButton");
        cx.visit::<ButtonSecondary, _>(reset, |b, _| b.on_click.add(h(H_RESTORE)));
        // the live preview's readers (made before this: OnInitialized runs children first)
        let all = cx.view.tree().post_order(cx.w);
        self.previews = all
            .into_iter()
            .filter(|&w| {
                w != cx.w
                    && cx.view.tree().user_class(w).is_some_and(|c| {
                        matches!(
                            c,
                            "UI_IGv3_LivePreview"
                                | "UI_IGv3_LivePreview_Character"
                                | "UI_IGv3_LivePreview_HUDFixed"
                                | "UI_IGv3_LivePreview_Scoreboard"
                        )
                    })
            })
            .collect();
        // OnInitialized @864 -> InitInterfacePanel @81: Refresh UI Toggles, RefreshPresets 0.05 s later (@105 ->
        // @15), the pickers' swatches (@160), then SetupScrolling (@446)
        self.refresh_ui_toggles(cx);
        cx.view.delay(0.05, h(H_REFRESH_PRESETS));
        let choices = cx.view.palette_choices().to_vec();
        for name in [
            "UI_MeColorPicker",
            "UI_AllyColorPicker",
            "UI_EnemyColorPicker_1",
            "UI_Player3ColorPicker",
            "UI_Player4ColorPicker",
        ] {
            Self::picker(cx, name, |p, cx| p.populate_colors(cx, &choices));
        }
        self.setup_scrolling(cx);
    }

    /// Destruct @24816: UnregisterScrollbox (the port lets go of its OnScroll binding too).
    fn destruct(&mut self, cx: &mut Cx<'_>) {
        let sb = Self::scrollbox(cx);
        let me = cx.w;
        cx.view.manager.unregister_scrollbox(sb);
        cx.view.manager.on_scroll.remove(Handler { owner: me, id: H_SCROLL });
    }

    fn handle(&mut self, cx: &mut Cx<'_>, id: u32, arg: &dyn Any) {
        let pick = arg.downcast_ref::<(i32, String)>().map(|(_, s)| s.clone());
        match id {
            // Scoreboard position @7784 (EqualEqual_StrStr): Set-UI-HUDPositionBottom(ID == "bottom")
            H_SCOREBOARD => {
                if let Some(s) = pick {
                    cx.prefs.ui.hud_position_bottom = s == "bottom";
                    self.broadcast(cx);
                    self.tail_presets(cx);
                }
            }
            // Cooldowns @8218, Damage @12935, Player name @14818, Dodge meter @25925 (NotEqual_StriStri): another ID
            // writes nothing
            n if (H_POSITION..H_POSITION + 4).contains(&n) => {
                if let Some((w, hud)) = pick.as_deref().and_then(prefs::position_pair) {
                    let kind = POSITION_ROWS[(n - H_POSITION) as usize].1;
                    prefs::set_ui_show_elements(&mut cx.prefs.ui, kind, w, hud);
                    self.broadcast(cx);
                    self.tail_presets(cx);
                }
            }
            // the pickers' OnColorSelected (@20441-22125): Set Color Binding, then @20831
            n if (H_COLOR..H_COLOR + 5).contains(&n) => {
                if let Some(row) = arg.downcast_ref::<String>() {
                    prefs::set_color_binding(&mut cx.prefs.ui, PICKERS[(n - H_COLOR) as usize].1, row);
                    self.broadcast(cx);
                    self.tail(cx);
                }
            }
            n if (H_OUTLINE..H_OUTLINE + 3).contains(&n) => self.on_outline(cx, (n - H_OUTLINE) as usize),
            H_TEAM_SKINS => self.on_team_colored_skins(cx),
            n if (H_CONFIRM..H_CONFIRM + 3).contains(&n) => self.confirm_outline(cx, (n - H_CONFIRM) as usize),
            n if n == H_CONFIRM + 3 => self.confirm_team_skins(cx),
            n if (H_CANCEL..H_CANCEL + 3).contains(&n) => {
                Self::toggle_on(cx, OUTLINES[(n - H_CANCEL) as usize].1, true)
            }
            n if n == H_CANCEL + 3 => Self::toggle_on(cx, "UI_ToggleTeamColoredSkins", true),
            // Knockback Influence @24196: Set-UI-WorldShowDI(isOn), then @20831
            H_DI => {
                if let Some(&on) = arg.downcast_ref::<bool>() {
                    cx.prefs.ui.world_show_di = on;
                    self.broadcast(cx);
                    self.tail(cx);
                }
            }
            // Cursor speed @23582: Set-UI-CursorSpeedMultiplier((NewValue + 5) / 10) (@24019-24103; the setter clamps
            // 0.5..1.5), then @20831. The cursor reads it (menu/cursor.rs).
            H_CURSOR => {
                if let Some(&(v, _)) = arg.downcast_ref::<(f64, f64)>() {
                    prefs::set_cursor_speed(&mut cx.prefs.ui, (v + 5.0) / 10.0);
                    self.broadcast(cx);
                    self.tail(cx);
                }
            }
            H_PRESET => {
                if let Some(s) = pick {
                    self.on_preset(cx, &s);
                }
            }
            H_RESTORE => self.on_restore_defaults(cx),
            H_REFRESH_PRESETS => Self::refresh_presets(cx),
            // OnScroll @22754: the page's context only
            H_SCROLL => {
                if let Some(&(player, _, amount)) = arg.downcast_ref::<(i32, i32, f64)>()
                    && cx.view.manager.get_current_context(player) == self.context
                {
                    let sb = Self::scrollbox(cx);
                    scroll_list(cx.view.tree_mut(), sb, amount);
                }
            }
            _ => {}
        }
    }

    fn resume(&mut self, cx: &mut Cx<'_>, id: u32) {
        self.handle(cx, id, &());
    }
}

// ---------- UI_IGv3_LivePreview (the page's preview of the HUD) ----------

/// OnInitialized @29 binds OnUIPrefsChanged -> OnUIPrefs @144 -> Refresh -> RefreshAnim (@213): the orange frame
/// (flasher) fades from 1 to 0 over 0.25 s. Set UI Context Mode (@163, the scoreboard's, then the flash) has no
/// caller.
#[derive(Debug, Default)]
pub struct LivePreview;

impl LivePreview {
    pub fn make(_cx: &mut Cx<'_>) -> Box<dyn Controller> {
        Box::new(LivePreview)
    }

    pub fn refresh(&mut self, cx: &mut Cx<'_>) {
        let me = cx.w;
        cx.view.play(me, "RefreshAnim", false, 1.0);
    }
}

impl Controller for LivePreview {}

/// GetPaletteEntry of a Player Index (0 me, 1 ally, 2 and 3 enemies): Me 2, Ally 0, Enemy 1.
const INDEX_PALETTE: [u8; 4] = [2, 0, 1, 1];
const ME_DAMAGE_OUTLINE: Rgba = [0.006049, 0.009721, 0.021219, 1.0];
const SILHOUETTES: [&str; 4] = [
    "UI_Settings_HUD_Me_NoOutline",
    "UI_Settings_HUD_AllyNoOutline",
    "UI_Settings_HUD_Enemy1NoOutline",
    "UI_Settings_HUD_Enemy2NoOutline",
];
const OUTLINE_TEX: [&str; 4] = [
    "UI_Settings_HUD_Me_Outline",
    "UI_Settings_HUD_AllyOutline",
    "UI_Settings_HUD_Enemy1Outline",
    "UI_Settings_HUD_Enemy2Outline",
];

/// The Player Index's colour (the select's default, zero, for another index).
fn index_color(cx: &Cx<'_>, index: usize) -> Rgba {
    INDEX_PALETTE.get(index).map_or([0.0; 4], |&k| palette_entry(cx, k))
}

/// LinearColorLerp(colour, black, 0.9) (@2747 / HUDFixed @2228); Player Index 0 the dark constant (@3032 / @2513).
fn damage_outline(c: Rgba, index: usize) -> Rgba {
    if index == 0 { ME_DAMAGE_OUTLINE } else { [c[0] * 0.1, c[1] * 0.1, c[2] * 0.1, c[3] + (1.0 - c[3]) * 0.9] }
}

fn shown_if(cx: &mut Cx<'_>, name: &str, on: bool) {
    let w = find(cx, name);
    wd(cx, w).vis = if on { Vis::SelfHitTestInvisible } else { Vis::Hidden };
}

fn player_index(cx: &Cx<'_>) -> usize {
    cx.view.tree().props(cx.w).get("Player Index").and_then(Value::as_u64).unwrap_or(0) as usize
}

/// UI_IGv3_LivePreview_Character: a fighter's silhouette with its outline and its attached HUD by the "world" fields.
#[derive(Debug, Default)]
pub struct LivePreviewCharacter;

impl LivePreviewCharacter {
    pub fn make(_cx: &mut Cx<'_>) -> Box<dyn Controller> {
        Box::new(LivePreviewCharacter)
    }

    /// Refresh @15.
    pub fn refresh(&mut self, cx: &mut Cx<'_>) {
        let u = cx.prefs.ui.clone();
        let i = player_index(cx);
        shown_if(cx, "cooldowns", u.world_show_cooldowns);
        shown_if(cx, "damage", u.world_show_percentage);
        shown_if(cx, "usernameContainer", u.world_show_username && i != 0);
        shown_if(cx, "dodgeContainer", u.world_show_dodge_meters && i != 0);
        let outline = (i == 0 && u.world_show_my_outline)
            || (i == 1 && u.world_show_ally_outlines)
            || ((i == 2 || i == 3) && u.world_show_enemy_outlines);
        shown_if(cx, "outline", outline);
        let c = index_color(cx, i);
        for n in ["cooldownsFill", "outline", "usernameBG", "meFill", "dodgeFill"] {
            let w = find(cx, n);
            wd(cx, w).color = c;
        }
        let evade = palette_entry(cx, 0);
        let w = find(cx, "evadeFill");
        wd(cx, w).color = evade;
        let w = find(cx, "damageOutline");
        wd(cx, w).color = damage_outline(c, i);
    }
}

impl Controller for LivePreviewCharacter {
    /// OnInitialized @3799: OnUIPrefsChanged, then Refresh.
    fn on_initialized(&mut self, cx: &mut Cx<'_>) {
        self.refresh(cx);
    }

    /// PreConstruct @3099: the silhouette and outline textures by Player Index; the "me" chevron for index 0 only.
    fn pre_construct(&mut self, cx: &mut Cx<'_>) {
        let i = player_index(cx);
        let s = find(cx, "silhouette");
        wd(cx, s).tex = SILHOUETTES.get(i).map(|t| t.to_string());
        let o = find(cx, "outline");
        wd(cx, o).tex = OUTLINE_TEX.get(i).map(|t| t.to_string());
        let me = find(cx, "me");
        wd(cx, me).vis = if i != 0 { Vis::Hidden } else { Vis::SelfHitTestInvisible };
    }
}

/// UI_Orientation (enum asset): NewEnumerator4 = 0 None, NewEnumerator0 = 1 Left, NewEnumerator1 = 2 Right.
fn orientation_of(v: Option<&Value>) -> u8 {
    match v {
        Some(Value::Number(n)) => n.as_u64().unwrap_or(0) as u8,
        Some(Value::String(s)) => match s.rsplit("::").next().unwrap_or_default() {
            "NewEnumerator0" => 1,
            "NewEnumerator1" => 2,
            _ => 0,
        },
        _ => 0,
    }
}

/// UI_IGv3_LivePreview_HUDFixed: a scoreboard panel by the "hud" fields, mirrored on the right side.
#[derive(Debug, Default)]
pub struct LivePreviewHudFixed;

impl LivePreviewHudFixed {
    pub fn make(_cx: &mut Cx<'_>) -> Box<dyn Controller> {
        Box::new(LivePreviewHudFixed)
    }

    /// SetOrientation @39: Right mirrors the panel, the damage number mirrored back.
    pub fn set_orientation(cx: &mut Cx<'_>, o: u8) {
        let c = find(cx, "contentContainer");
        wd(cx, c).transform.scale = [if o == 2 { -1.0 } else { 1.0 }, 1.0];
        let d = find(cx, "damage");
        wd(cx, d).transform.scale = [if o == 2 { -0.75 } else { 0.75 }, 0.75];
    }

    /// Refresh @723.
    pub fn refresh(&mut self, cx: &mut Cx<'_>) {
        let u = cx.prefs.ui.clone();
        let i = player_index(cx);
        shown_if(cx, "cooldowns", u.hud_show_cooldowns);
        shown_if(cx, "damage", u.hud_show_percentage);
        shown_if(cx, "username", u.hud_show_username);
        shown_if(cx, "dodgeContainer", u.hud_show_dodge_meters);
        let c = index_color(cx, i);
        for n in ["colorStrip", "cooldownsFill", "dodgeFill"] {
            let w = find(cx, n);
            wd(cx, w).color = c;
        }
        let w = find(cx, "damageOutline");
        wd(cx, w).color = damage_outline(c, i);
    }
}

impl Controller for LivePreviewHudFixed {
    /// OnInitialized @577: OnUIPrefsChanged, then Refresh.
    fn on_initialized(&mut self, cx: &mut Cx<'_>) {
        self.refresh(cx);
    }

    /// PreConstruct @15: SetOrientation(UI Orientation).
    fn pre_construct(&mut self, cx: &mut Cx<'_>) {
        let o = orientation_of(cx.view.tree().props(cx.w).get("UI Orientation"));
        Self::set_orientation(cx, o);
    }
}

/// UI_IGv3_LivePreview_Scoreboard: the four panels, the pips and the clock, at the top or the bottom.
#[derive(Debug, Default)]
pub struct LivePreviewScoreboard;

impl LivePreviewScoreboard {
    pub fn make(_cx: &mut Cx<'_>) -> Box<dyn Controller> {
        Box::new(LivePreviewScoreboard)
    }

    /// Set UI Context Mode @2562: ClearChildren and the mode's children (1: 1v1, 2: 2v2, 3: free for all) with their
    /// orientations, then every slot centred at its desired size (@15-812), the clock's padding (@3142), Refresh.
    // Deviation: the 2v2 mode only (the class default's, and nothing calls it with another), whose children are the
    // tree's in the tree's order, so the children are kept in place rather than cleared and added again.
    fn set_ui_context_mode(&mut self, cx: &mut Cx<'_>, mode: u8) {
        if mode != 2 {
            return;
        }
        for n in 0..4 {
            let w = find(cx, &format!("UI_IGv3_LivePreview_HUDFixed{n}"));
            cx.visit::<LivePreviewHudFixed, _>(w, |_, cx| {
                LivePreviewHudFixed::set_orientation(cx, if n < 2 { 1 } else { 2 })
            });
        }
        let b = find(cx, "mainContainer");
        for c in cx.view.tree().widget(b).children.clone() {
            let x = wd(cx, c);
            x.h_align = HAlign::Center;
            x.v_align = VAlign::Center;
            x.fill = 0.0;
        }
        let clock = find(cx, "clock");
        wd(cx, clock).pad = Margin { l: -50.0, t: 0.0, r: -50.0, b: 0.0 };
        self.refresh(cx);
    }

    /// Refresh @1838: the container at the bottom with hud-positionBottom, else the top; the left pips in the ally
    /// colour, the right ones in the enemy colour.
    pub fn refresh(&mut self, cx: &mut Cx<'_>) {
        let bottom = cx.prefs.ui.hud_position_bottom;
        let b = find(cx, "mainContainer");
        wd(cx, b).v_align = if bottom { VAlign::Bottom } else { VAlign::Top };
        let ally = palette_entry(cx, 0);
        let enemy = palette_entry(cx, 1);
        for n in ["pipLeft1", "pipLeft2"] {
            let w = find(cx, n);
            wd(cx, w).color = ally;
        }
        for n in ["pipRight1", "pipRight2"] {
            let w = find(cx, n);
            wd(cx, w).color = enemy;
        }
    }
}

impl Controller for LivePreviewScoreboard {
    /// OnInitialized @2416: OnUIPrefsChanged, then Refresh.
    fn on_initialized(&mut self, cx: &mut Cx<'_>) {
        self.refresh(cx);
    }

    /// PreConstruct @4274: Set UI Context Mode with the instance's mode, then the Element Scale loop (@1787 -> @886):
    /// every child scaled by it (the sign of its own X scale kept), the container's top and bottom padding
    /// (1 - scale) x -100.
    fn pre_construct(&mut self, cx: &mut Cx<'_>) {
        // UI_ContextMode (enum asset): NewEnumerator2 is 2 (2v2), the class default's
        let mode = match cx.view.tree().props(cx.w).get("UI Context Mode") {
            Some(Value::Number(n)) => n.as_u64().unwrap_or(0) as u8,
            Some(Value::String(s)) if s.rsplit("::").next() == Some("NewEnumerator2") => 2,
            _ => 0,
        };
        self.set_ui_context_mode(cx, mode);
        let scale = cx.view.tree().props(cx.w).get("Element Scale").and_then(Value::as_f64).unwrap_or(1.0);
        let b = find(cx, "mainContainer");
        for c in cx.view.tree().widget(b).children.clone() {
            let x = wd(cx, c);
            let sign = if x.transform.scale[0] < 0.0 {
                -1.0
            } else if x.transform.scale[0] > 0.0 {
                1.0
            } else {
                0.0
            };
            x.transform.scale = [scale * sign, scale];
        }
        let pad = (1.0 - scale) * -100.0;
        wd(cx, b).pad = Margin { l: 0.0, t: pad, r: 0.0, b: pad };
    }
}
