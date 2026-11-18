//! UI_SectionBar's button classes.

use std::any::Any;

use serde_json::Value;

use super::buttons::{DARK, WHITE};
use super::interactor::InteractorState;
use super::{bind, find, focus_frame, quad_color, quad_wobble, wd};
use crate::bundle::Rgba;
use crate::menu::controller::{Controller, Cx};
use crate::menu::widgets::common::text;
use crate::slate::{Vis, WidgetId};

// UI_ButtonParams members.
pub const P_LABEL: &str = "Label_2_541172134A06F700F38DA383BB56110B";
pub const P_ICON: &str = "Icon_5_45B2E61448507A9FAC1B7483DCD087D4";
pub const P_ID: &str = "ID_9_16EEAF8C41D133AD0AFDFF8B12025A07";
pub const P_TOOLTIP: &str = "Tooltip_12_EBF0EFEF4615659551D2D4AD5934793E";

/// A tab of a section bar (BPI InitBarButton / Set On): its controller returns itself from
/// `Controller::as_bar_button`.
pub trait BarButton {
    fn init_bar_button(&mut self, cx: &mut Cx<'_>, params: &Value);
    fn set_on(&mut self, cx: &mut Cx<'_>, v: bool);
    fn is_on(&self) -> bool;
    /// UI_SectionBarButtonInterface GetTooltipOnlyMode (false for the bar's own classes).
    fn tooltip_only_mode(&self) -> bool {
        false
    }
}

const H_FOCUS: u32 = 1;
const H_UNFOCUS: u32 = 2;

fn interactor(cx: &Cx<'_>) -> WidgetId {
    find(cx, "UI_Interactor")
}

fn set_vis(cx: &mut Cx<'_>, name: &str, v: Vis) {
    let w = find(cx, name);
    wd(cx, w).vis = v;
}

fn set_color(cx: &mut Cx<'_>, name: &str, c: Rgba) {
    let w = find(cx, name);
    wd(cx, w).color = c;
}

/// BarButton's own InitBarButton: the label's text.
fn init_label(cx: &mut Cx<'_>, params: &Value) {
    let l = find(cx, "Label");
    wd(cx, l).text = text(params.get(P_LABEL));
}

const TAB_BACKING: Rgba = [0.127438, 0.165132, 0.991102, 1.0];

/// UI_SectionBarVertMenusRework (a Settings tab) and UI_SectionBarVertSubmenusRework (a tab under CONTROLS). The
/// sub tab's class default UUserWidget Padding (Left 50) indents it.
#[derive(Debug)]
pub struct SectionBarVertButton {
    is_on: bool,
    colors_backing: bool,
}

impl SectionBarVertButton {
    pub fn make(_cx: &mut Cx<'_>) -> Box<dyn Controller> {
        Box::new(SectionBarVertButton { is_on: false, colors_backing: true })
    }

    pub fn make_sub(_cx: &mut Cx<'_>) -> Box<dyn Controller> {
        Box::new(SectionBarVertButton { is_on: false, colors_backing: false })
    }
}

impl BarButton for SectionBarVertButton {
    fn init_bar_button(&mut self, cx: &mut Cx<'_>, params: &Value) {
        let icon = params.get(P_ICON).and_then(Value::as_str).filter(|s| !s.is_empty()).map(str::to_string);
        let w = find(cx, "icon");
        let x = wd(cx, w);
        x.vis = if icon.is_some() { Vis::SelfHitTestInvisible } else { Vis::Collapsed };
        x.tex = icon;
        init_label(cx, params);
        self.set_on(cx, self.is_on);
    }

    fn set_on(&mut self, cx: &mut Cx<'_>, v: bool) {
        self.is_on = v;
        set_vis(cx, "OnIndic", if v { Vis::SelfHitTestInvisible } else { Vis::Collapsed });
        let off = if v { Vis::Collapsed } else { Vis::SelfHitTestInvisible };
        set_vis(cx, "Border", off);
        set_vis(cx, "Backing", off);
        if self.colors_backing {
            let b = find(cx, "Backing");
            quad_color(cx, b, TAB_BACKING);
        }
        let c = if v { DARK } else { WHITE };
        set_color(cx, "Label", c);
        set_color(cx, "icon", c);
    }

    fn is_on(&self) -> bool {
        self.is_on
    }
}

impl Controller for SectionBarVertButton {
    fn construct(&mut self, cx: &mut Cx<'_>) {
        let fc = find(cx, "UI_FocusedCommon");
        let i = interactor(cx);
        focus_frame(cx, fc, i);
    }

    fn as_bar_button(&mut self) -> Option<&mut dyn BarButton> {
        Some(self)
    }
}

/// UI_SectionBarButtonMenuRework: a row of a dropdown's list.
#[derive(Debug, Default)]
pub struct SectionBarMenuButton {
    is_on: bool,
}

impl SectionBarMenuButton {
    pub fn make(_cx: &mut Cx<'_>) -> Box<dyn Controller> {
        Box::new(SectionBarMenuButton::default())
    }
}

impl BarButton for SectionBarMenuButton {
    fn init_bar_button(&mut self, cx: &mut Cx<'_>, params: &Value) {
        init_label(cx, params);
        self.set_on(cx, self.is_on);
    }

    fn set_on(&mut self, cx: &mut Cx<'_>, v: bool) {
        self.is_on = v;
        set_vis(cx, "OnIndicator", if v { Vis::Visible } else { Vis::Hidden });
    }

    fn is_on(&self) -> bool {
        self.is_on
    }
}

impl Controller for SectionBarMenuButton {
    fn on_initialized(&mut self, cx: &mut Cx<'_>) {
        let i = interactor(cx);
        bind(cx, i, |s: &mut InteractorState| &mut s.on_focus, H_FOCUS);
        bind(cx, i, |s: &mut InteractorState| &mut s.on_unfocus, H_UNFOCUS);
    }

    fn handle(&mut self, cx: &mut Cx<'_>, id: u32, _arg: &dyn Any) {
        match id {
            H_FOCUS => set_vis(cx, "Hover", Vis::SelfHitTestInvisible),
            H_UNFOCUS => set_vis(cx, "Hover", Vis::Hidden),
            _ => {}
        }
    }

    fn as_bar_button(&mut self) -> Option<&mut dyn BarButton> {
        Some(self)
    }
}

// ---------- WBP_MO_HorizontalButton (a tab of a horizontal section bar) ----------

const MO_OFF: Rgba = [0.019382, 0.024158, 0.138432, 1.0];
const MO_ON: Rgba = [1.0, 0.527115, 0.0, 1.0];
const MO_LABEL_DARK: Rgba = [0.009721, 0.011612, 0.033105, 1.0];

/// Three quads under the label: OffIndicator (dark blue), OnIndicator (orange while the tab is on, light under the
/// focus) and FocusIndicator (light, wobbling, with the focus). InitBarButton @1249 (the label in upper case @185,
/// then Set On(isOn) @1225), Set On @554, the interactor's OnFocus @277 and OnUnfocus @1142.
/// Not ported: UpdateQuadShape @1281 (no caller on the Glossary: the callers are Leaderboards, Perks_Trainer,
/// QuestTabs, SocialPanel and MatchOptions) and UI_Attention (Collapsed in the tree, no graph of this class shows
/// it). SetPrimaryButtonIsEnabled @184 is empty and GetTooltipOnlyMode returns false.
#[derive(Debug, Default)]
pub struct MOHorizontalButton {
    is_on: bool,
}

impl MOHorizontalButton {
    pub fn make(_cx: &mut Cx<'_>) -> Box<dyn Controller> {
        Box::new(MOHorizontalButton::default())
    }
}

impl BarButton for MOHorizontalButton {
    fn init_bar_button(&mut self, cx: &mut Cx<'_>, params: &Value) {
        let l = find(cx, "Label");
        wd(cx, l).text = text(params.get(P_LABEL)).to_uppercase();
        self.set_on(cx, self.is_on);
    }

    fn set_on(&mut self, cx: &mut Cx<'_>, v: bool) {
        self.is_on = v;
        set_vis(cx, "OffIndicator", if v { Vis::Hidden } else { Vis::SelfHitTestInvisible });
        set_vis(cx, "OnIndicator", if v { Vis::SelfHitTestInvisible } else { Vis::Hidden });
        let (off, on, focus) = (find(cx, "OffIndicator"), find(cx, "OnIndicator"), find(cx, "FocusIndicator"));
        quad_color(cx, off, MO_OFF);
        quad_color(cx, on, MO_ON);
        set_color(cx, "Label", if v { MO_LABEL_DARK } else { WHITE });
        quad_wobble(cx, focus, false);
    }

    fn is_on(&self) -> bool {
        self.is_on
    }
}

impl Controller for MOHorizontalButton {
    fn on_initialized(&mut self, cx: &mut Cx<'_>) {
        let i = interactor(cx);
        bind(cx, i, |s: &mut InteractorState| &mut s.on_focus, H_FOCUS);
        bind(cx, i, |s: &mut InteractorState| &mut s.on_unfocus, H_UNFOCUS);
    }

    fn handle(&mut self, cx: &mut Cx<'_>, id: u32, _arg: &dyn Any) {
        let (on, focus) = (find(cx, "OnIndicator"), find(cx, "FocusIndicator"));
        match id {
            H_FOCUS => {
                set_vis(cx, "FocusIndicator", Vis::SelfHitTestInvisible);
                set_color(cx, "Label", MO_LABEL_DARK);
                quad_color(cx, on, WHITE);
                quad_wobble(cx, focus, true);
            }
            H_UNFOCUS => {
                if self.is_on {
                    return self.set_on(cx, true);
                }
                set_vis(cx, "FocusIndicator", Vis::Hidden);
                quad_wobble(cx, focus, false);
                set_color(cx, "Label", WHITE);
            }
            _ => {}
        }
    }

    fn as_bar_button(&mut self) -> Option<&mut dyn BarButton> {
        Some(self)
    }
}

// ---------- UI_SectionBarHorizontalMenusRework (a tab of the Interface page's preset bar) ----------

const HMR_ON_LABEL: Rgba = [0.006049, 0.009721, 0.021219, 1.0];
const HMR_OFF_LABEL: Rgba = [0.737255, 0.792157, 0.72549, 1.0];
const HMR_FOCUS_LABEL: Rgba = [0.007843, 0.007843, 0.027451, 1.0];

/// OnIndic shows while the tab is on (Set On @218: Visible, else Hidden) with the dark label, the light one off;
/// the focus darkens the label of a tab that is off (@16) and the unfocus gives it back (@597).
/// SetPrimaryButtonIsEnabled is empty and GetTooltipOnlyMode returns false.
#[derive(Debug, Default)]
pub struct SectionBarHorizontalMenusButton {
    is_on: bool,
}

impl SectionBarHorizontalMenusButton {
    pub fn make(_cx: &mut Cx<'_>) -> Box<dyn Controller> {
        Box::new(SectionBarHorizontalMenusButton::default())
    }
}

impl BarButton for SectionBarHorizontalMenusButton {
    fn init_bar_button(&mut self, cx: &mut Cx<'_>, params: &Value) {
        init_label(cx, params);
        self.set_on(cx, self.is_on);
    }

    fn set_on(&mut self, cx: &mut Cx<'_>, v: bool) {
        self.is_on = v;
        set_vis(cx, "OnIndic", if v { Vis::Visible } else { Vis::Hidden });
        set_color(cx, "Label", if v { HMR_ON_LABEL } else { HMR_OFF_LABEL });
    }

    fn is_on(&self) -> bool {
        self.is_on
    }
}

impl Controller for SectionBarHorizontalMenusButton {
    fn on_initialized(&mut self, cx: &mut Cx<'_>) {
        let i = interactor(cx);
        bind(cx, i, |s: &mut InteractorState| &mut s.on_focus, H_FOCUS);
        bind(cx, i, |s: &mut InteractorState| &mut s.on_unfocus, H_UNFOCUS);
    }

    fn construct(&mut self, cx: &mut Cx<'_>) {
        let fc = find(cx, "UI_FocusedCommon");
        let i = interactor(cx);
        focus_frame(cx, fc, i);
    }

    fn handle(&mut self, cx: &mut Cx<'_>, id: u32, _arg: &dyn Any) {
        match id {
            H_FOCUS if !self.is_on => set_color(cx, "Label", HMR_FOCUS_LABEL),
            H_UNFOCUS if !self.is_on => set_color(cx, "Label", HMR_OFF_LABEL),
            _ => {}
        }
    }

    fn as_bar_button(&mut self) -> Option<&mut dyn BarButton> {
        Some(self)
    }
}
