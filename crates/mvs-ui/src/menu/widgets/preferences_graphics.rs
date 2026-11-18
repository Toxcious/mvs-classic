//! UI_PreferencesGraphicsRework, the Settings panel's Graphics page.
//! The rules of each row (what it lists, what a pick writes) are `menu::prefs`; this places them on the rows.
//!
//! The window's rows (mode, resolution, display) show when the program has a window to give (`MenuSink::displays`);
//! Resolution Scale and Anti-aliasing (at once: the app diffs `Settings` each frame), Texture Quality (the level's
//! anisotropy), Character Occlusion (the next match) and Show Framerate (at once).
// Deviation: the rows the port cannot honour are not shown (Vertical Sync, One Frame Thread Lag, Quality Presets,
// Material, Visual FX and Shadow Quality, Constrain Mouse Cursor, Autodetect), nor a section header whose rows are
// all gone; the exclusive full screen is not a window-mode item (platform.rs set_window_mode).

use std::any::Any;

use serde_json::{Value, json};

use super::controls::bar_buttons::{P_ID, P_LABEL};
use super::controls::buttons::ToggleSwitch;
use super::controls::drop_menu::DropMenu;
use super::controls::interactor::{batch_set_context, batch_set_scroll_box, set_focused};
use super::controls::{find, prop, wd};
use crate::menu::controller::{Controller, Cx, Event, Handler};
use crate::menu::custom_text;
use crate::menu::manager::NULL_GUID;
use crate::menu::prefs::{self, DisplayInfo, WindowInfo};
use crate::menu::scroll::scroll_list;
use crate::menu::view::MenuView;
use crate::slate::{Margin, Vis, WidgetId};
use mvs_settings::WindowMode;

/// The rows that are not shown (GRAPHICS_UNUSED).
const UNUSED: [&str; 8] = [
    "UI_ToggleVerticalSync",
    "UI_ToggleOneFrameThreadLag",
    "DropdownQualityPresets",
    "DropdownMaterialQuality",
    "DropdownVisualFXQuality",
    "DropdownShadowQuality",
    "UI_ToggleLockMouseToViewport",
    "AutodetectGraphicsSettingsButton",
];
/// The window's rows: collapsed where the program has no window to give.
const DISPLAY_ROWS: [&str; 3] = ["DropdownWindowMode", "DropDownResolution", "DropdownDisplayMonitor"];
/// The headers by section, top to bottom, each with its rows.
const SECTIONS: [(&str, &[&str]); 3] = [
    (
        "TextBlock_145",
        &[
            "DropdownWindowMode",
            "DropDownResolution",
            "DropdownResolutionScale",
            "DropdownDisplayMonitor",
            "UI_ToggleVerticalSync",
            "UI_ToggleOneFrameThreadLag",
        ],
    ),
    (
        "TextBlock",
        &[
            "UI_ToggleCharacterOcclusion",
            "DropdownQualityPresets",
            "DropdownAntialiasingQuality",
            "DropdownMaterialQuality",
            "DropdownTextureQuality",
            "DropdownVisualFXQuality",
            "DropdownShadowQuality",
        ],
    ),
    ("TextBlock_1", &["UI_ToggleLockMouseToViewport", "UI_ToggleFramerate", "AutodetectGraphicsSettingsButton"]),
];
/// GetInteractors: the toggles first (Fn@15-212), then each drop menu's (Fn@309-754).
const TOGGLES: [&str; 6] = [
    "AutodetectGraphicsSettingsButton",
    "UI_ToggleCharacterOcclusion",
    "UI_ToggleOneFrameThreadLag",
    "UI_ToggleVerticalSync",
    "UI_ToggleLockMouseToViewport",
    "UI_ToggleFramerate",
];
const DROPDOWNS: [&str; 10] = [
    "DropDownResolution",
    "DropdownResolutionScale",
    "DropdownWindowMode",
    "DropdownAntialiasingQuality",
    "DropdownTextureQuality",
    "DropdownVisualFXQuality",
    "DropdownDisplayMonitor",
    "DropdownQualityPresets",
    "DropdownShadowQuality",
    "DropdownMaterialQuality",
];
/// " (Primary)" (ubergraph @13107).
const PRIMARY: &str = "CF27B82D4A964E8C9C6DC6A20004BED4";

const H_MODE: u32 = 1;
const H_RESOLUTION: u32 = 2;
const H_DISPLAY: u32 = 3;
const H_SCALE: u32 = 4;
const H_AA: u32 = 5;
const H_TEXTURE: u32 = 6;
const H_OCCLUSION: u32 = 7;
const H_FPS: u32 = 8;
const H_FRAME_RATE: u32 = 9;
const H_SCROLL: u32 = 10;

/// A drop menu item (a UI_ButtonParams: label and id).
fn item(label: &str, id: &str) -> Value {
    json!({ P_LABEL: { "CultureInvariantString": label }, P_ID: id })
}

#[derive(Debug, Default)]
pub struct PreferencesGraphics {
    pub preferences_changed: Event<()>,
    pub texture_settings_changed: Event<()>,
    initialized: bool,
    context: String,
    /// ScreenResolutionOptions: the resolution menu's id is an index into it.
    resolutions: Vec<[u32; 2]>,
    window: Option<WindowInfo>,
    displays: Vec<DisplayInfo>,
    frame_rate: Option<WidgetId>,
}

impl PreferencesGraphics {
    pub fn make(_cx: &mut Cx<'_>) -> Box<dyn Controller> {
        Box::new(PreferencesGraphics { context: NULL_GUID.to_string(), ..Default::default() })
    }

    fn scrollbox(cx: &Cx<'_>) -> WidgetId {
        find(cx, "graphicsScrollBox")
    }

    fn menu<R>(cx: &mut Cx<'_>, name: &str, f: impl FnOnce(&mut DropMenu, &mut Cx<'_>) -> R) -> Option<R> {
        let w = find(cx, name);
        cx.visit::<DropMenu, _>(w, f)
    }

    fn toggle<R>(cx: &mut Cx<'_>, name: &str, f: impl FnOnce(&mut ToggleSwitch, &mut Cx<'_>) -> R) -> Option<R> {
        let w = find(cx, name);
        cx.visit::<ToggleSwitch, _>(w, f)
    }

    fn set_items(cx: &mut Cx<'_>, name: &str, items: Vec<Value>) {
        let w = find(cx, name);
        cx.view.tree_mut().set_prop(w, "Items", Value::Array(items));
    }

    /// The window's rows exist when the program has displays to list.
    fn has_display(&self) -> bool {
        !self.displays.is_empty()
    }

    /// GetInteractors.
    pub fn interactors(&self, cx: &mut Cx<'_>) -> Vec<WidgetId> {
        let mut list = Vec::new();
        for n in TOGGLES {
            let w = find(cx, n);
            list.push(cx.view.tree().find_in(w, "UI_Interactor"));
        }
        for n in DROPDOWNS {
            list.extend(Self::menu(cx, n, |_, cx| DropMenu::interactors(cx)).unwrap_or_default());
        }
        if let Some(f) = self.frame_rate {
            list.extend(cx.visit::<DropMenu, _>(f, |_, cx| DropMenu::interactors(cx)).unwrap_or_default());
        }
        list
    }

    pub fn update_context(&mut self, cx: &mut Cx<'_>, ctx: &str) {
        self.context = ctx.to_string();
        let list = self.interactors(cx);
        batch_set_context(cx, &list, ctx);
    }

    /// FocusFirst @36011: the window mode row (where it is not shown: the first row that is).
    pub fn focus_first(&self, cx: &mut Cx<'_>) {
        let first = if self.has_display() {
            let w = find(cx, "DropdownWindowMode");
            cx.view.tree().find_in(w, "mainInteractor")
        } else {
            let w = find(cx, "UI_ToggleCharacterOcclusion");
            cx.view.tree().find_in(w, "UI_Interactor")
        };
        set_focused(cx, first, false);
    }

    /// The frame height the resolution scale is relative to.
    fn frame_height(cx: &Cx<'_>) -> f64 {
        f64::from(cx.view.size()[1])
    }

    /// Builder W (U@9162-10258) and Select (U@8444-9091).
    fn build_resolutions(&mut self, cx: &mut Cx<'_>) {
        let Some(w) = self.window.clone() else { return };
        let all: Vec<[u32; 2]> = prop(cx, "WindowedResolutions")
            .as_array()
            .map(|a| {
                a.iter()
                    .map(|r| {
                        let n = |k: &str| r.get(k).and_then(Value::as_f64).unwrap_or(0.0) as u32;
                        [n("X"), n("Y")]
                    })
                    .collect()
            })
            .unwrap_or_default();
        self.resolutions = prefs::resolution_options(&all, self.displays.get(w.display));
        let items =
            self.resolutions.iter().enumerate().map(|(i, r)| item(&prefs::resolution_label(*r), &i.to_string()));
        Self::set_items(cx, "DropDownResolution", items.collect());
    }

    fn select_resolution(&mut self, cx: &mut Cx<'_>) {
        let at = self.window.as_ref().and_then(|w| prefs::resolution_selected(&self.resolutions, w));
        Self::menu(cx, "DropDownResolution", |m, cx| {
            m.initialize(cx);
            if let Some(i) = at {
                m.set_active_by_id(cx, &i.to_string());
            }
        });
    }

    /// A @14269: the mode, then the resolution list and its selection; hidden in windowed fullscreen.
    // Deviation: collapsed, not Hidden (SetVisibility 2): the build leaves the row's empty space.
    fn init_window_mode(&mut self, cx: &mut Cx<'_>) {
        let Some(w) = self.window.clone() else { return };
        Self::menu(cx, "DropdownWindowMode", |m, cx| m.set_active_by_id(cx, prefs::mode_id(w.mode)));
        self.build_resolutions(cx);
        self.select_resolution(cx);
        if w.mode == WindowMode::WindowedFullscreen {
            let r = find(cx, "DropDownResolution");
            wd(cx, r).vis = Vis::Collapsed;
        }
    }

    /// B @12690: each display's name, " (Primary)" after the primary's, id its index.
    // Deviation: the selection is the display the window is on, not GraphicsPreferences.displayMonitor (which a
    // display pick writes 0, U@33424, so the build's row reopens on the first display).
    fn init_monitors(&mut self, cx: &mut Cx<'_>) {
        let primary = cx.view.game_text(PRIMARY).to_string();
        let items: Vec<Value> = self
            .displays
            .iter()
            .enumerate()
            .map(|(i, d)| item(&prefs::display_label(d, &primary), &i.to_string()))
            .collect();
        if items.is_empty() {
            return;
        }
        Self::set_items(cx, "DropdownDisplayMonitor", items);
        let at = self.window.as_ref().map_or(0, |w| w.display);
        Self::menu(cx, "DropdownDisplayMonitor", |m, cx| {
            m.initialize(cx);
            m.set_active_by_id(cx, &at.to_string());
        });
    }

    /// The limits offered on the window's display, and the saved one selected.
    fn init_frame_rate(&mut self, cx: &mut Cx<'_>) {
        let Some(f) = self.frame_rate else { return };
        let hz = self.window.as_ref().and_then(|w| self.displays.get(w.display)).map_or(0.0, |d| d.refresh_hz);
        let shown = prefs::frame_rates_offered(hz);
        let items: Vec<Value> = shown
            .iter()
            .map(|&r| {
                let label =
                    prefs::frame_rate_label(r, hz, custom_text::FRAME_RATE_NONE, custom_text::FRAME_RATE_UNLIMITED);
                item(&label, &r.to_string())
            })
            .collect();
        cx.view.tree_mut().set_prop(f, "Items", Value::Array(items));
        let sel = prefs::frame_rate_shown(cx.prefs.graphics.frame_rate_limit, &shown);
        cx.visit::<DropMenu, _>(f, |m, cx| {
            m.initialize(cx);
            m.set_active_by_id(cx, &sel.to_string());
        });
    }

    /// The Max Framerate row: a UI_DropMenuRework under the display monitor row, its props and slot the Texture
    /// Quality row's but for its label and items.
    // Deviation: the row and its text are the port's own (the build has no frame rate row): a drop menu of the page's
    // class with the Texture Quality row's look, under the display rows.
    fn add_frame_rate_row(&mut self, cx: &mut Cx<'_>) {
        let like = find(cx, "DropdownTextureQuality");
        let at = find(cx, "DropdownDisplayMonitor");
        let w = MenuView::create(cx, "UI_DropMenuRework");
        let props = cx.view.tree().props(like).clone();
        if let Value::Object(m) = props {
            for (k, v) in m {
                cx.view.tree_mut().set_prop(w, &k, v);
            }
        }
        let t = cx.view.tree_mut();
        t.set_prop(w, "LabelText", json!({ "CultureInvariantString": custom_text::FRAME_RATE_LABEL }));
        t.set_prop(w, "TopTooltipText", json!({ "CultureInvariantString": custom_text::FRAME_RATE_TOOLTIP }));
        t.set_prop(w, "Items", json!([]));
        let (pad, h, v, fill) = {
            let l = cx.view.tree().widget(like);
            (l.pad, l.h_align, l.v_align, l.fill)
        };
        let x = wd(cx, w);
        x.name = "DropdownFrameRateLimit".to_string();
        x.pad = pad;
        x.h_align = h;
        x.v_align = v;
        x.fill = fill;
        if let Some(parent) = cx.view.tree().widget(at).parent {
            let idx = cx.view.tree().widget(parent).children.iter().position(|&c| c == at);
            MenuView::add_child(cx, parent, w, idx.map(|i| i + 1));
        }
        let me = cx.w;
        cx.visit::<DropMenu, _>(w, |m, _| m.on_click.add(Handler { owner: me, id: H_FRAME_RATE }));
        self.frame_rate = Some(w);
    }

    /// SetupScrolling @35538.
    fn setup_scrolling(&mut self, cx: &mut Cx<'_>) {
        let sb = Self::scrollbox(cx);
        let me = cx.w;
        cx.view.manager.on_scroll.add(Handler { owner: me, id: H_SCROLL });
        cx.view.manager.register_scrollbox(sb);
        let list = self.interactors(cx);
        batch_set_scroll_box(cx, &list, sb);
    }

    /// InitGraphics @28423: the window's rows, then the quality and toggle reads.
    fn init_graphics(&mut self, cx: &mut Cx<'_>) {
        self.displays = cx.sink.displays();
        if self.has_display() {
            self.window = Some(cx.sink.window());
            self.init_window_mode(cx);
            self.init_monitors(cx);
        }
        self.init_frame_rate(cx);
        let g = cx.prefs.graphics.clone();
        // @11025 Anti-aliasing, @11157 Texture Quality, @11466 Init Resolution Scale
        Self::menu(cx, "DropdownAntialiasingQuality", |m, cx| {
            m.set_active_by_id(cx, &g.anti_aliasing_quality.to_string())
        });
        Self::menu(cx, "DropdownTextureQuality", |m, cx| m.set_active_by_id(cx, &g.texture_quality.to_string()));
        if let Some(id) = prefs::resolution_scale_id(&g, Self::frame_height(cx)) {
            Self::menu(cx, "DropdownResolutionScale", |m, cx| m.set_active_by_id(cx, &id));
        }
        // @11866 Character Occlusion shows the inverse of the preference; @12297 ui-showFPS
        Self::toggle(cx, "UI_ToggleCharacterOcclusion", |t, cx| t.set_on(cx, !g.skip_character_occlusion_sorting));
        Self::toggle(cx, "UI_ToggleFramerate", |t, cx| t.set_on(cx, g.show_fps));
        self.initialized = true;
    }

    fn changed(&self, cx: &mut Cx<'_>) {
        self.preferences_changed.fire(cx, &());
    }

    fn on_window_mode(&mut self, cx: &mut Cx<'_>, id: &str) {
        let Some(mode) = prefs::mode_of_id(id) else { return };
        if !self.initialized || !self.has_display() {
            return;
        }
        let windowed = prefs::pick_window_mode(cx.prefs, mode);
        if let Some(w) = &mut self.window {
            w.mode = mode;
            if windowed {
                w.size = prefs::WINDOWED_PICK_SIZE;
            }
        }
        let row = find(cx, "DropDownResolution");
        if windowed {
            self.build_resolutions(cx);
            self.select_resolution(cx);
            wd(cx, row).vis = Vis::Visible;
        } else {
            wd(cx, row).vis = Vis::Collapsed;
        }
        self.changed(cx);
    }

    fn on_resolution(&mut self, cx: &mut Cx<'_>, id: &str) {
        let r = id.parse::<usize>().ok().and_then(|i| self.resolutions.get(i)).copied();
        let (Some(r), Some(w)) = (r, self.window.clone()) else { return };
        if !self.initialized || !self.has_display() {
            return;
        }
        if prefs::pick_resolution(cx.prefs, r, &w) {
            self.changed(cx);
            if let Some(w) = &mut self.window {
                w.size = r;
            }
        }
    }

    fn on_display(&mut self, cx: &mut Cx<'_>, index: i32) {
        let Some(d) = usize::try_from(index).ok().and_then(|i| self.displays.get(i)).cloned() else { return };
        if !self.initialized {
            return;
        }
        prefs::pick_display(cx.prefs, &d);
        if let Some(w) = &mut self.window {
            w.display = index as usize;
        }
        self.build_resolutions(cx);
        self.select_resolution(cx);
        Self::menu(cx, "DropdownDisplayMonitor", |m, cx| m.set_active_by_id(cx, &index.to_string()));
        self.init_frame_rate(cx);
        self.changed(cx);
    }

    fn on_frame_rate(&mut self, cx: &mut Cx<'_>, id: &str) {
        if let Ok(v) = id.parse::<u32>() {
            cx.prefs.graphics.frame_rate_limit = v;
            self.changed(cx);
        }
    }
}

impl Controller for PreferencesGraphics {
    fn on_initialized(&mut self, cx: &mut Cx<'_>) {
        let display = !cx.sink.displays().is_empty();
        for n in UNUSED {
            let w = find(cx, n);
            wd(cx, w).vis = Vis::Collapsed;
        }
        if !display {
            for n in DISPLAY_ROWS {
                let w = find(cx, n);
                wd(cx, w).vis = Vis::Collapsed;
            }
        }
        // the first header shown takes the first header's top padding (the room under the panel's title)
        let mut top: Option<f64> = None;
        for (header, rows) in SECTIONS {
            let h = find(cx, header);
            let all_gone = rows.iter().all(|r| {
                let w = find(cx, r);
                cx.view.tree().widget(w).vis == Vis::Collapsed
            });
            if all_gone {
                if top.is_none() {
                    top = Some(cx.view.tree().widget(h).pad.t);
                }
                wd(cx, h).vis = Vis::Collapsed;
            } else if let Some(t) = top.take() {
                let w = wd(cx, h);
                w.pad = Margin { t, ..w.pad };
            }
        }
        // Deviation: the build's exclusive fullscreen (`fullscreen`, EWindowMode 0) is not an item (U@28468-28508:
        // its handler maps the pick's Index; here the item's id is the mode).
        let mode = find(cx, "DropdownWindowMode");
        let items: Vec<Value> = prop_of(cx, mode, "Items")
            .into_iter()
            .filter(|p| p.get(P_ID).and_then(Value::as_str) != Some("fullscreen"))
            .collect();
        cx.view.tree_mut().set_prop(mode, "Items", Value::Array(items));
        cx.visit::<DropMenu, _>(mode, |m, cx| m.initialize(cx));
        self.add_frame_rate_row(cx);
        self.setup_scrolling(cx);
        let me = cx.w;
        for (name, id) in [
            ("DropdownWindowMode", H_MODE),
            ("DropDownResolution", H_RESOLUTION),
            ("DropdownDisplayMonitor", H_DISPLAY),
            ("DropdownResolutionScale", H_SCALE),
            ("DropdownAntialiasingQuality", H_AA),
            ("DropdownTextureQuality", H_TEXTURE),
        ] {
            Self::menu(cx, name, |m, _| m.on_click.add(Handler { owner: me, id }));
        }
        Self::toggle(cx, "UI_ToggleCharacterOcclusion", |t, _| {
            t.on_toggle_changed.add(Handler { owner: me, id: H_OCCLUSION })
        });
        Self::toggle(cx, "UI_ToggleFramerate", |t, _| t.on_toggle_changed.add(Handler { owner: me, id: H_FPS }));
    }

    fn construct(&mut self, cx: &mut Cx<'_>) {
        self.init_graphics(cx);
    }

    /// Destruct @35919: UnregisterScrollbox (and the OnScroll binding).
    fn destruct(&mut self, cx: &mut Cx<'_>) {
        let sb = Self::scrollbox(cx);
        let me = cx.w;
        cx.view.manager.unregister_scrollbox(sb);
        cx.view.manager.on_scroll.remove(Handler { owner: me, id: H_SCROLL });
    }

    fn handle(&mut self, cx: &mut Cx<'_>, id: u32, arg: &dyn Any) {
        let pick = arg.downcast_ref::<(i32, String)>();
        match (id, pick) {
            (H_MODE, Some((_, id))) => self.on_window_mode(cx, id),
            (H_RESOLUTION, Some((_, id))) => self.on_resolution(cx, id),
            (H_DISPLAY, Some((i, _))) => self.on_display(cx, *i),
            (H_SCALE, Some((_, id))) => {
                let h = Self::frame_height(cx);
                prefs::pick_resolution_scale(&mut cx.prefs.graphics, id, h);
                self.changed(cx);
            }
            (H_AA, Some((i, _))) => {
                prefs::pick_anti_aliasing(&mut cx.prefs.graphics, usize::try_from(*i).unwrap_or(0));
                self.changed(cx);
            }
            (H_TEXTURE, Some((i, _))) => {
                if prefs::pick_texture(&mut cx.prefs.graphics, usize::try_from(*i).unwrap_or(0)) {
                    self.texture_settings_changed.fire(cx, &());
                }
                self.changed(cx);
            }
            (H_FRAME_RATE, Some((_, id))) => self.on_frame_rate(cx, id),
            (H_OCCLUSION, _) => {
                let Some(&on) = arg.downcast_ref::<bool>() else { return };
                prefs::pick_character_occlusion(&mut cx.prefs.graphics, on);
                self.changed(cx);
            }
            (H_FPS, _) => {
                let Some(&on) = arg.downcast_ref::<bool>() else { return };
                cx.prefs.graphics.show_fps = on;
                self.changed(cx);
            }
            // OnScroll @35744: the page's context only
            (H_SCROLL, _) => {
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
}

/// The items of the property `key` of `w` as a list.
fn prop_of(cx: &Cx<'_>, w: WidgetId, key: &str) -> Vec<Value> {
    cx.view.tree().props(w).get(key).and_then(Value::as_array).cloned().unwrap_or_default()
}
