//! UI_PreferencesPanelRework, the Settings panel: the section bar, the page
//! switcher and the Audio page. The Controls page (`controls/input_binding.rs`) is made at Construct and the panel
//! opens on CONTROLS > Controller; the Graphics (`preferences_graphics.rs`) and Interface
//! (`preferences_interface.rs`) pages are made on their first section click; the port's own CLASSIC page
//! (`preferences_classic.rs`) at OnInitialized. The rules of the rows are `menu::prefs`.
//!
//! Of the Audio page the five volume rows and Restore Defaults work and show; the other rows are collapsed
//! (AUDIO_UNUSED); Mute On Lost Focus works and shows.
// Deviation: the ONLINE/LEGAL section is not shown (SECTIONS_UNUSED).
// Deviation: the bar ends with the port's own CLASSIC section (the build has none): the port's own settings.

use std::any::Any;

use serde_json::{Value, json};

use super::controls::bar_buttons::{P_ID, P_LABEL};
use super::controls::buttons::{ButtonSecondary, PlusMinusHorizontal, ToggleSwitch};
use super::controls::input_binding::{InputBindingSet, PreferencesControls};
use super::controls::interactor::{batch_set_context, batch_set_scroll_box, set_focused};
use super::controls::{find, quad_color, wd};
use super::nav::GlobalSlideOut;
use super::preferences_classic::{self as classic, ClassicPage};
use super::preferences_graphics::PreferencesGraphics;
use super::preferences_interface::PreferencesInterface;
use super::section_bar::{SectionBar, suppress_focus};
use crate::menu::controller::{Controller, Cx, Handler};
use crate::menu::custom_text;
use crate::menu::manager::{DialogParams, Manager, Modal, NULL_GUID};
use crate::menu::prefs::{self, RESET_ORDER, VOLUME_ROWS};
use crate::menu::scroll::scroll_list;
use crate::menu::view::MenuView;
use crate::slate::{Margin, Vis, WidgetId};

/// What does nothing in the port is not shown: the Audio page's rows other than the volumes and Restore Defaults.
const AUDIO_UNUSED: [&str; 7] = [
    "AudioOutputDropdown",
    "AudioProfileDropdown",
    "UI_ToggleDMCAMode",
    "TextBlock",
    "DropdownLanguage",
    "SubtitlesDropdown",
    "UI_ToggleMatureAudioLanguage",
];
/// The sections of the bar the port does not show, by id.
const SECTIONS_UNUSED: [&str; 1] = ["online"];
/// The widget switcher's page by section id.
const PAGES: [(&str, &str); 5] = [
    ("graphics", "Graphics"),
    ("controls", "Controls"),
    ("audio", "PanelAudio"),
    ("interface", "PanelUIv2"),
    (CLASSIC, classic::PAGE),
];
/// The port's own section's id; its entry on the bar is a copy of INTERFACE's.
const CLASSIC: &str = "classic";
const CLASSIC_LIKE: &str = "interface";
/// The CONTROLS family's sub tabs.
const SUB_TABS: [&str; 2] = ["controls_keyboard", "controls_controller"];
/// CONTROLS' backing while a sub tab is open.
const CONTROLS_GROUP: crate::bundle::Rgba = [0.040915, 0.049707, 0.215861, 1.0];

/// The texture dialog's texts (@22357-22869): the title, the body and the button's strings.
const TEXTURE_TITLE: &str = "3156D1784EE459BFC2E495985DE37A81";
const TEXTURE_BODY: &str = "79B311A54CCAD5DDB843D9881B63C6E0";
const TEXTURE_BUTTON: &str = "5C8C41C347CB04C9066A9B8D1E29DD15";

const H_BAR_CLICK: u32 = 1;
const H_RESET: u32 = 2;
const H_PREFS_CHANGED: u32 = 3;
const H_TEXTURE_CHANGED: u32 = 4;
const H_SCROLL: u32 = 5;
const H_FOCUS_FIRST: u32 = 6;
const H_SUB_TABS_ON: u32 = 7;
const H_CONTROLS_GROUP: u32 = 8;
/// The Mute On Lost Focus toggle's handler.
const H_MUTE: u32 = 9;
/// The volume rows' handlers: `H_VOLUME + row`.
const H_VOLUME: u32 = 20;
/// The Classic page's toggles: `H_CLASSIC + row`.
const H_CLASSIC: u32 = 40;

#[derive(Debug)]
pub struct PreferencesPanel {
    pub context: String,
    preferences_changed: bool,
    audio_initialized: bool,
    controls: Option<WidgetId>,
    /// GraphicsPreferencesPanel.
    graphics: Option<WidgetId>,
    /// The Interface page's variable.
    interface: Option<WidgetId>,
    texture_settings_changed: bool,
    /// The port's own Classic page.
    classic: Option<ClassicPage>,
    /// The bar's buttons of the CONTROLS family by id (the bar is busy while its OnClick runs: the owner reaches
    /// them through the tree).
    family: Vec<(String, WidgetId)>,
}

impl PreferencesPanel {
    pub fn make(_cx: &mut Cx<'_>) -> Box<dyn Controller> {
        Box::new(PreferencesPanel {
            context: NULL_GUID.to_string(),
            preferences_changed: false,
            audio_initialized: false,
            controls: None,
            graphics: None,
            interface: None,
            texture_settings_changed: false,
            classic: None,
            family: Vec::new(),
        })
    }

    fn family_button(&self, id: &str) -> Option<WidgetId> {
        self.family.iter().find(|(k, _)| k == id).map(|(_, w)| *w)
    }

    /// The Controls page's keyboard mode.
    fn keyboard_mode(&self, cx: &mut Cx<'_>) -> bool {
        let Some(c) = self.controls else { return false };
        cx.visit::<PreferencesControls, _>(c, |_, cx| PreferencesControls::keyboard_mode(cx)).unwrap_or(false)
    }

    /// The Controls page's binding set in gamepad or keyboard mode.
    fn controls_mode(&self, cx: &mut Cx<'_>, keyboard: bool) {
        let Some(c) = self.controls else { return };
        let set = cx.view.tree().find_in(c, "Bindings");
        cx.visit::<InputBindingSet, _>(set, |s, cx| s.set_mode(cx, keyboard));
    }

    /// A section bar button's SetOn.
    fn set_on(cx: &mut Cx<'_>, b: WidgetId, v: bool) {
        cx.visit_dyn(b, |c, cx| {
            if let Some(b) = c.as_bar_button() {
                b.set_on(cx, v);
            }
        });
    }

    /// The two sub tabs show while a tab of the CONTROLS family is the section; 0.05 s later the sub tab of the
    /// input mode is on and CONTROLS itself reads as the open group.
    fn handle_tab_subitems(&mut self, cx: &mut Cx<'_>, id: &str) {
        let family = id == "controls" || SUB_TABS.contains(&id);
        for sub in SUB_TABS {
            if let Some(b) = self.family_button(sub) {
                wd(cx, b).vis = if family { Vis::Visible } else { Vis::Collapsed };
            }
        }
        if !family {
            return;
        }
        if SUB_TABS.contains(&id) {
            let bar = Self::bar(cx);
            suppress_focus(cx, bar);
        }
        if id == "controls_controller" {
            self.controls_mode(cx, false);
        } else if id == "controls_keyboard" {
            self.controls_mode(cx, true);
        }
        let me = cx.w;
        cx.view.delay(0.05, Handler { owner: me, id: H_SUB_TABS_ON });
        cx.view.delay(0.05, Handler { owner: me, id: H_CONTROLS_GROUP });
    }

    fn sub_tabs_on(&mut self, cx: &mut Cx<'_>) {
        let kb = self.keyboard_mode(cx);
        if let Some(b) = self.family_button("controls_keyboard") {
            Self::set_on(cx, b, kb);
        }
        if let Some(b) = self.family_button("controls_controller") {
            Self::set_on(cx, b, !kb);
        }
    }

    fn controls_group(&mut self, cx: &mut Cx<'_>) {
        let Some(b) = self.family_button("controls") else { return };
        Self::set_on(cx, b, false);
        let backing = cx.view.tree().find_in(b, "Backing");
        quad_color(cx, backing, CONTROLS_GROUP);
        let border = cx.view.tree().find_in(b, "Border");
        wd(cx, border).vis = Vis::Collapsed;
    }

    /// A page made into its canvas, filling it.
    fn make_page(cx: &mut Cx<'_>, class: &str, canvas: &str) -> WidgetId {
        let page = MenuView::create(cx, class);
        let p = wd(cx, page);
        p.anchors = [0.0, 0.0, 1.0, 1.0];
        p.offsets = Margin { l: 0.0, t: 0.0, r: 0.0, b: 0.0 };
        let canvas = find(cx, canvas);
        MenuView::add_child(cx, canvas, page, None);
        page
    }

    /// @1347-1751: the Interface page, made once, its dispatcher bound.
    fn page_interface(&mut self, cx: &mut Cx<'_>) {
        if self.interface.is_some() {
            return;
        }
        let page = Self::make_page(cx, "UI_PreferencesInterfaceRework", "InterfaceCanvasv2");
        let me = cx.w;
        cx.visit::<PreferencesInterface, _>(page, |p, _| {
            p.preferences_changed_dispatch.add(Handler { owner: me, id: H_PREFS_CHANGED })
        });
        self.interface = Some(page);
    }

    fn bar(cx: &Cx<'_>) -> WidgetId {
        find(cx, "mainMenuSectionBar")
    }

    fn scrollbox(cx: &Cx<'_>) -> WidgetId {
        find(cx, "audioScrollbox")
    }

    fn row<R>(cx: &mut Cx<'_>, name: &str, f: impl FnOnce(&mut PlusMinusHorizontal, &mut Cx<'_>) -> R) -> Option<R> {
        let w = find(cx, name);
        cx.visit::<PlusMinusHorizontal, _>(w, f)
    }

    fn row_interactors(cx: &mut Cx<'_>, name: &str) -> Vec<WidgetId> {
        Self::row(cx, name, |_, cx| PlusMinusHorizontal::interactors(cx).to_vec()).unwrap_or_default()
    }

    /// SetActiveWidget.
    fn set_active_widget(cx: &mut Cx<'_>, name: &str) {
        let sw = find(cx, "WidgetSwitcher");
        let at = {
            let t = cx.view.tree();
            t.widget(sw).children.iter().position(|&c| t.widget(c).name == name)
        };
        if let Some(i) = at {
            wd(cx, sw).active = i as i32;
        }
    }

    fn page_graphics(&mut self, cx: &mut Cx<'_>) {
        if self.graphics.is_some() {
            return;
        }
        let page = Self::make_page(cx, "UI_PreferencesGraphicsRework", "GraphicsCanvas");
        let me = cx.w;
        cx.visit::<PreferencesGraphics, _>(page, |g, _| {
            g.preferences_changed.add(Handler { owner: me, id: H_PREFS_CHANGED });
            g.texture_settings_changed.add(Handler { owner: me, id: H_TEXTURE_CHANGED });
        });
        self.graphics = Some(page);
    }

    /// The section's id picked: its page shown (the Graphics and Interface pages made once), the Audio page read
    /// again, the CONTROLS family's sub tabs.
    fn section_clicked(&mut self, cx: &mut Cx<'_>, id: &str) {
        let key = id.to_lowercase();
        let name = PAGES.iter().find(|(k, _)| *k == key).map(|(_, n)| *n);
        if let Some(n) = name {
            Self::set_active_widget(cx, n);
        }
        if name == Some("Graphics") {
            self.page_graphics(cx);
        }
        if name == Some("PanelUIv2") {
            self.page_interface(cx);
        }
        if name == Some("PanelAudio") {
            self.audio_initialized = false;
            self.init_audio_panel(cx);
        }
        if name == Some(classic::PAGE)
            && let Some(c) = &self.classic
        {
            c.refresh(cx);
        }
        if self.context != NULL_GUID {
            let ctx = self.context.clone();
            self.update_context(cx, &ctx);
        }
        self.handle_tab_subitems(cx, &key);
    }

    fn init_audio_panel(&mut self, cx: &mut Cx<'_>) {
        let a = cx.prefs.audio.clone();
        for (i, name) in VOLUME_ROWS.iter().enumerate() {
            Self::row(cx, name, |r, cx| r.set_percentage(cx, prefs::volume(&a, i)));
        }
        let w = find(cx, "UI_ToggleMuteOnLostFocus");
        cx.visit::<ToggleSwitch, _>(w, |t, cx| t.set_on(cx, a.mute_on_lost_focus));
        self.audio_initialized = true;
    }

    fn changed(&mut self) {
        self.preferences_changed = true;
    }

    /// The Restore Defaults button: each row to its default, in the build's order.
    fn reset_audio(&mut self, cx: &mut Cx<'_>) {
        for name in RESET_ORDER {
            Self::row(cx, name, |r, cx| r.reset_to_default(cx));
        }
    }

    fn first_in_section(&mut self, cx: &mut Cx<'_>) {
        let bar = Self::bar(cx);
        let id = cx.visit::<SectionBar, _>(bar, |b, _| b.current_selected_id()).unwrap_or_default().to_lowercase();
        if id == "audio" {
            let plus = Self::row(cx, "UI_MasterVolume", |_, cx| PlusMinusHorizontal::plus(cx));
            if let Some(p) = plus {
                let i = ButtonSecondary::interactor(cx, p);
                set_focused(cx, i, false);
            }
        } else if id == "controls" || SUB_TABS.contains(&id.as_str()) {
            if let Some(c) = self.controls {
                cx.visit::<PreferencesControls, _>(c, |_, cx| PreferencesControls::focus_first(cx));
            }
        } else if id == "graphics"
            && let Some(g) = self.graphics
        {
            cx.visit::<PreferencesGraphics, _>(g, |g, cx| g.focus_first(cx));
        } else if id == "interface"
            && let Some(p) = self.interface
        {
            cx.visit::<PreferencesInterface, _>(p, |p, cx| p.focus_first(cx));
        } else if id == CLASSIC
            && let Some(c) = &self.classic
        {
            c.focus_first(cx);
        }
    }

    /// Update Context: the Audio page's interactors, the Graphics page's, the Classic page's, the bar's; the bar's
    /// current tab focused.
    pub fn update_context(&mut self, cx: &mut Cx<'_>, ctx: &str) {
        self.context = ctx.to_string();
        let mut list = Vec::new();
        for name in RESET_ORDER {
            list.extend(Self::row_interactors(cx, name));
        }
        let b = find(cx, "ResetAudioButton");
        list.push(ButtonSecondary::interactor(cx, b));
        for name in ["UI_ToggleMuteOnLostFocus", "UI_ToggleMatureAudioLanguage", "UI_ToggleDMCAMode"] {
            let w = find(cx, name);
            list.push(cx.view.tree().find_in(w, "UI_Interactor"));
        }
        for name in ["AudioOutputDropdown", "AudioProfileDropdown", "SubtitlesDropdown"] {
            let w = find(cx, name);
            list.extend(
                cx.visit::<super::controls::drop_menu::DropMenu, _>(w, |_, cx| {
                    super::controls::drop_menu::DropMenu::interactors(cx)
                })
                .unwrap_or_default(),
            );
        }
        if let Some(c) = &self.classic {
            list.extend(c.interactors(cx));
        }
        batch_set_context(cx, &list, ctx);
        if let Some(g) = self.graphics {
            cx.visit::<PreferencesGraphics, _>(g, |g, cx| g.update_context(cx, ctx));
        }
        if let Some(p) = self.interface {
            cx.visit::<PreferencesInterface, _>(p, |p, cx| p.update_context(cx, ctx));
        }
        if let Some(c) = self.controls {
            cx.visit::<PreferencesControls, _>(c, |c, cx| c.update_context(cx, ctx, 0));
        }
        let lang = find(cx, "DropdownLanguage");
        cx.visit::<super::controls::drop_menu::DropMenu, _>(lang, |_, cx| {
            super::controls::drop_menu::DropMenu::update_context(cx, ctx, None)
        });
        let bar = Self::bar(cx);
        let focus = cx.visit::<SectionBar, _>(bar, |b, cx| {
            b.update_context(cx, ctx, None);
            b.interactors.get(b.current_index()).copied()
        });
        if let Some(Some(i)) = focus {
            set_focused(cx, i, true);
        }
    }

    /// Cleanup @25250: the Graphics page's PreferencesChangedDispatch cleared, and the texture dialog
    /// (@22357-22869) when its level was changed. (The save is the app's: it writes the settings a second after the
    /// last change and when the program ends.)
    pub fn cleanup(&mut self, cx: &mut Cx<'_>) {
        if let Some(g) = self.graphics {
            cx.visit::<PreferencesGraphics, _>(g, |g, _| g.preferences_changed.clear());
        }
        if self.texture_settings_changed {
            let t = |cx: &Cx<'_>, k: &str| cx.view.game_text(k).to_string();
            let params = DialogParams {
                title: t(cx, TEXTURE_TITLE),
                body: t(cx, TEXTURE_BODY),
                button_left_label: t(cx, TEXTURE_BUTTON),
                ..Default::default()
            };
            Manager::show_dialog(cx, params);
            self.texture_settings_changed = false;
        }
    }
}

impl Controller for PreferencesPanel {
    fn on_initialized(&mut self, cx: &mut Cx<'_>) {
        let me = cx.w;
        let sb = Self::scrollbox(cx);
        cx.view.manager.on_scroll.add(Handler { owner: me, id: H_SCROLL });
        cx.view.manager.register_scrollbox(sb);
        // ConnectInteractorsToScrollboxes
        let mut list = Vec::new();
        for name in VOLUME_ROWS {
            list.extend(Self::row_interactors(cx, name));
        }
        batch_set_scroll_box(cx, &list, sb);
        // LocalizationPass: the sections the port shows (the build's AUDIO/LANGUAGE reads AUDIO)
        let bar = Self::bar(cx);
        let params: Vec<Value> =
            cx.view.tree().props(bar).get("Button Params").and_then(Value::as_array).cloned().unwrap_or_default();
        let id_of = |p: &Value| p.get(P_ID).and_then(Value::as_str).unwrap_or_default().to_lowercase();
        let classic_entry = params.iter().find(|p| id_of(p) == CLASSIC_LIKE).cloned().map(|mut p| {
            p[P_ID] = json!(CLASSIC);
            p[P_LABEL] = json!({ "CultureInvariantString": custom_text::CLASSIC_SECTION });
            p
        });
        let shown: Vec<Value> = params
            .into_iter()
            .filter(|p| !SECTIONS_UNUSED.contains(&id_of(p).as_str()))
            .map(|mut p| {
                if p.get(P_ID).and_then(Value::as_str) == Some("audio") {
                    p[P_LABEL] = json!({ "CultureInvariantString": "AUDIO" });
                }
                p
            })
            .chain(classic_entry)
            .collect();
        for n in AUDIO_UNUSED {
            let w = find(cx, n);
            wd(cx, w).vis = Vis::Collapsed;
        }
        let family = cx.visit::<SectionBar, _>(bar, |b, cx| {
            b.populate(cx, Some(shown));
            b.update_context(cx, NULL_GUID, None);
            ["controls", "controls_keyboard", "controls_controller"]
                .iter()
                .filter_map(|id| Some((id.to_string(), b.button_by_id(id)?)))
                .collect::<Vec<_>>()
        });
        self.family = family.unwrap_or_default();
        for (i, name) in VOLUME_ROWS.iter().enumerate() {
            Self::row(cx, name, |r, _| r.on_value_changed.add(Handler { owner: me, id: H_VOLUME + i as u32 }));
        }
        let mute = find(cx, "UI_ToggleMuteOnLostFocus");
        cx.visit::<ToggleSwitch, _>(mute, |t, _| t.on_toggle_changed.add(Handler { owner: me, id: H_MUTE }));
        let reset = find(cx, "ResetAudioButton");
        cx.visit::<ButtonSecondary, _>(reset, |b, _| b.on_click.add(Handler { owner: me, id: H_RESET }));
        let sw = find(cx, "WidgetSwitcher");
        self.classic = Some(ClassicPage::make(cx, sw, H_CLASSIC));
        cx.view.delay(0.1, Handler { owner: me, id: H_FOCUS_FIRST });
    }

    fn pre_construct(&mut self, cx: &mut Cx<'_>) {
        // its visibility binding returns Hidden, always
        let w = find(cx, "ChangesSavedIndicator");
        wd(cx, w).vis = Vis::Hidden;
    }

    fn construct(&mut self, cx: &mut Cx<'_>) {
        self.preferences_changed = false;
        Self::set_active_widget(cx, "Controls");
        let me = cx.w;
        let bar = Self::bar(cx);
        cx.visit::<SectionBar, _>(bar, |b, _| b.on_click.add(Handler { owner: me, id: H_BAR_CLICK }));
        if self.controls.is_none() {
            let page = Self::make_page(cx, "UI_PreferencesControlsRework", "ControlsCanvas");
            self.controls = Some(page);
        }
        self.section_clicked(cx, "controls_controller");
        cx.visit::<SectionBar, _>(bar, |b, cx| b.set_current_index(cx, 1));
        if let Some(c) = self.controls {
            cx.visit::<PreferencesControls, _>(c, |c, _| {
                c.preferences_changed_dispatch.add(Handler { owner: me, id: H_PREFS_CHANGED })
            });
        }
        let h = find(cx, "SettingsHeader");
        wd(cx, h).vis = Vis::SelfHitTestInvisible;
    }

    fn destruct(&mut self, cx: &mut Cx<'_>) {
        let sb = Self::scrollbox(cx);
        let me = cx.w;
        cx.view.manager.unregister_scrollbox(sb);
        if let Some(c) = &self.classic {
            cx.view.manager.unregister_scrollbox(c.scrollbox());
        }
        cx.view.manager.on_scroll.remove(Handler { owner: me, id: H_SCROLL });
    }

    fn handle(&mut self, cx: &mut Cx<'_>, id: u32, arg: &dyn Any) {
        match id {
            H_BAR_CLICK => {
                if let Some((_, sid)) = arg.downcast_ref::<(i32, String)>() {
                    self.section_clicked(cx, sid);
                }
            }
            H_RESET => self.reset_audio(cx),
            H_PREFS_CHANGED => self.changed(),
            H_MUTE => {
                if let Some(&on) = arg.downcast_ref::<bool>()
                    && self.audio_initialized
                {
                    cx.prefs.audio.mute_on_lost_focus = on;
                    self.changed();
                }
            }
            H_TEXTURE_CHANGED => self.texture_settings_changed = true,
            // UI_Functions.ScrollList: player 0 while the Audio page shows (the Classic page alike)
            H_SCROLL => {
                if let Some(&(player, _, amount)) = arg.downcast_ref::<(i32, i32, f64)>() {
                    let page = find(cx, "PanelAudio");
                    if player == 0 && cx.view.tree().widget(page).vis != Vis::Collapsed && self.page_shown(cx, page) {
                        let sb = Self::scrollbox(cx);
                        scroll_list(cx.view.tree_mut(), sb, amount);
                    } else if player == 0
                        && let Some(c) = &self.classic
                        && self.page_shown(cx, c.root)
                    {
                        scroll_list(cx.view.tree_mut(), c.scrollbox(), amount);
                    }
                }
            }
            n if (H_CLASSIC..H_CLASSIC + classic::ROWS).contains(&n) => {
                if let Some(&on) = arg.downcast_ref::<bool>() {
                    ClassicPage::toggled(cx, n - H_CLASSIC, on);
                    self.changed();
                }
            }
            n if (H_VOLUME..H_VOLUME + VOLUME_ROWS.len() as u32).contains(&n) => {
                if let Some(&(_, frac)) = arg.downcast_ref::<(f64, f64)>()
                    && self.audio_initialized
                {
                    prefs::set_volume(&mut cx.prefs.audio, (n - H_VOLUME) as usize, frac);
                    self.changed();
                }
            }
            _ => {}
        }
    }

    fn resume(&mut self, cx: &mut Cx<'_>, id: u32) {
        match id {
            H_FOCUS_FIRST => self.first_in_section(cx),
            H_SUB_TABS_ON => self.sub_tabs_on(cx),
            H_CONTROLS_GROUP => self.controls_group(cx),
            _ => {}
        }
    }
}

impl PreferencesPanel {
    /// `page` is the switcher's active one (`find('PanelAudio').shown`).
    fn page_shown(&self, cx: &Cx<'_>, page: WidgetId) -> bool {
        let sw = find(cx, "WidgetSwitcher");
        let t = cx.view.tree();
        t.widget(sw).children.get(t.widget(sw).active.max(0) as usize) == Some(&page)
    }
}

// ---------- UI_PreferencesRework (the Settings modal) ----------

const PR_CONTEXT: u32 = 1;

/// UI_PreferencesRework: the Settings modal, a slide-out holding the panel. (PreDismiss's mkbRevert is empty in the
/// build: not called.)
#[derive(Debug, Default)]
pub struct PreferencesRework {
    modal_id: String,
    context: String,
}

impl PreferencesRework {
    pub fn make(_cx: &mut Cx<'_>) -> Box<dyn Controller> {
        Box::new(PreferencesRework { modal_id: String::new(), context: NULL_GUID.to_string() })
    }

    fn slide_out(cx: &Cx<'_>) -> WidgetId {
        find(cx, "WBP_GlobalSlideOut")
    }

    fn panel(cx: &Cx<'_>) -> WidgetId {
        find(cx, "UI_PreferencesPanelRework")
    }

    fn update_context(&mut self, cx: &mut Cx<'_>, ctx: &str) {
        self.context = ctx.to_string();
        let (s, p) = (Self::slide_out(cx), Self::panel(cx));
        cx.visit::<GlobalSlideOut, _>(s, |s, cx| s.update_context(cx, ctx));
        cx.visit::<PreferencesPanel, _>(p, |p, cx| p.update_context(cx, ctx));
    }

    fn close(&mut self, cx: &mut Cx<'_>) {
        let (s, p) = (Self::slide_out(cx), Self::panel(cx));
        cx.visit::<PreferencesPanel, _>(p, |p, cx| p.cleanup(cx));
        let id = self.modal_id.clone();
        cx.visit::<GlobalSlideOut, _>(s, |s, cx| s.close_slide_out(cx, &id));
    }
}

impl Modal for PreferencesRework {
    fn pre_show(&mut self, _cx: &mut Cx<'_>, id: &str) {
        self.modal_id = id.to_string();
    }

    fn post_show(&mut self, cx: &mut Cx<'_>, instructions: &str, _extra_data: &str) {
        let s = Self::slide_out(cx);
        let id = self.modal_id.clone();
        let isolated = instructions.contains("isolated");
        if isolated {
            GlobalSlideOut::set_global_nav_available(cx, s, false);
        }
        cx.visit::<GlobalSlideOut, _>(s, |s, cx| s.open_slide_out(cx, &id));
    }

    fn pre_dismiss(&mut self, cx: &mut Cx<'_>) {
        self.close(cx);
    }

    fn post_dismiss(&mut self, _cx: &mut Cx<'_>) {}

    fn request_dismiss(&mut self, cx: &mut Cx<'_>, _reason: &str, _from_cancel: bool) {
        self.close(cx);
    }

    fn get_modal_context(&self) -> String {
        self.context.clone()
    }
}

impl Controller for PreferencesRework {
    fn on_initialized(&mut self, cx: &mut Cx<'_>) {
        self.context = cx.view.manager.new_guid();
        let me = cx.w;
        cx.view.delay(0.2, Handler { owner: me, id: PR_CONTEXT });
    }

    /// SlotAsCanvasSlot(Overlay_ClipBounds).SetSize((2000, 0)).
    fn pre_construct(&mut self, cx: &mut Cx<'_>) {
        let s = Self::slide_out(cx);
        let o = cx.view.tree().find_in(s, "Overlay_ClipBounds");
        let m = &mut wd(cx, o).offsets;
        m.r = 2000.0;
        m.b = 0.0;
    }

    fn resume(&mut self, cx: &mut Cx<'_>, id: u32) {
        if id == PR_CONTEXT {
            let ctx = self.context.clone();
            self.update_context(cx, &ctx);
        }
    }

    fn as_modal(&mut self) -> Option<&mut dyn Modal> {
        Some(self)
    }
}
