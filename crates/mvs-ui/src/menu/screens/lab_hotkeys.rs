//! UI_LAB_Hotkeys: the Lab's hotkey strip over its match: the designer's UI_InputLabelSetCommon sets (UIReset: Reset)
//! shown, its two UI_InputLabelSetAdvanced sets (Toggle Slow Mo, ...) Collapsed, which its own graph never shows.
//! UI_Game adds it when the Lab opens (@2463-2870), after the pause menu and before the matchmaking timer (which is added over it), on the same root
//! canvas; nothing removes it until the level goes. Its words and keys are the
//! designer's UI_InputLabelSet / UI_InputLabel params (Game.locres text, the input actions' icons).

use crate::menu::controller::{Controller, Cx};
use crate::menu::screen::MenuScreen;
use crate::menu::view::MenuView;
use crate::menu::widgets::controls::{find, wd};
use crate::slate::{Margin, VAlign, Vis, WidgetId};

#[derive(Debug, Default)]
pub struct LabHotkeys {
    /// The hud-positionBottom the layout last followed.
    bottom: Option<bool>,
}

impl LabHotkeys {
    pub fn make(_cx: &mut Cx<'_>) -> Box<dyn Controller> {
        Box::new(LabHotkeys::default())
    }

    /// LayoutForPrefs @268-464: positionContainer's overlay slot SetVerticalAlignment(hud-positionBottom ? 1 Top :
    /// 3 Bottom), the edge the HUD does not use. OnInitialized @29-249 binds it to PM.OnUIPrefsChanged and calls it;
    /// here the preference is read each frame and applied when it changed (as UI_Lab_MatchmakingTimer).
    // Deviation: the build binds PM.OnUIPrefsChanged; the port reads the preference each frame.
    fn layout_for_prefs(&mut self, cx: &mut Cx<'_>) {
        let bottom = cx.prefs.ui.hud_position_bottom;
        if self.bottom == Some(bottom) {
            return;
        }
        self.bottom = Some(bottom);
        let c = find(cx, "positionContainer");
        wd(cx, c).v_align = if bottom { VAlign::Top } else { VAlign::Bottom };
    }
}

impl Controller for LabHotkeys {
    /// OnInitialized @29: LayoutForPrefs bound and called (@249). @139-208 also binds UI_Manager.OnResetKey to
    /// OnUIReset, whose only work is to broadcast the widget's ResetEvent (@554), which nothing in the build binds:
    /// not ported.
    fn on_initialized(&mut self, cx: &mut Cx<'_>) {
        self.layout_for_prefs(cx);
    }

    /// Construct @791 -> UpdateDebugVisibility @578-705: SetVisibility(UIManager.DebugHiddenScoreboard ? Hidden :
    /// SelfHitTestInvisible). The flag is a debug console command's (UI_Manager @19652): never set in the port.
    fn construct(&mut self, cx: &mut Cx<'_>) {
        let w = cx.w;
        wd(cx, w).vis = Vis::SelfHitTestInvisible;
    }

    fn tick(&mut self, cx: &mut Cx<'_>, _dt: f64) {
        self.layout_for_prefs(cx);
    }
}

/// UI_Game @2463-2870: Create(UI_LAB_Hotkeys), Root.AddChildToCanvas with anchors (0,0)-(1,1) and offsets 50 on
/// every side, no ZOrder, shown while the Lab's pause menu is collapsed (screen.alone). Returns the root it is on.
pub fn add_lab_hotkeys(screen: &mut MenuScreen, run: &mut crate::menu::controller::MenuRun<'_>) -> WidgetId {
    let root = screen.with_cx(run, |cx| {
        let w = MenuView::create(cx, "UI_LAB_Hotkeys");
        let bundle = cx.view.bundle.clone();
        let root = cx.view.tree_mut().create_plain(&bundle, "CanvasPanel", "RootCanvas");
        cx.view.tree_mut().add(root, w);
        let x = cx.view.tree_mut().widget_mut(w);
        x.anchors = [0.0, 0.0, 1.0, 1.0];
        x.offsets = Margin { l: 50.0, t: 50.0, r: 50.0, b: 50.0 };
        x.align = [0.0, 0.0];
        x.auto_size = false;
        MenuView::add_to_viewport(cx, root, 0);
        root
    });
    screen.alone.push(root);
    root
}
