//! The Move List (UI_CharacterMoveList with its eight UI_CharacterMoveRow and the UI_CharacterPassiveMove rows it
//! creates): the modal the Lab's MOVE LIST button shows (UI_LAB_Panel @15747 -> TrainingGameState.ToggleMovelist,
//! modal "training_movelist").
//!
//! Its texts are the rows of the CharacterData's MoveList table (the bundle's `moves`); the descriptions are
//! RichTextBlocks.

use std::any::Any;

use serde_json::Value;

use crate::bundle::Rgba;
use crate::menu::controller::{Controller, Cx, Event, Handler};
use crate::menu::manager::{Manager, Modal};
use crate::menu::screens::lab_selectors::{CharacterTypeInfo, run_on, set_text, set_vis};
use crate::menu::scroll::scroll_list;
use crate::menu::view::MenuView;
use crate::menu::widgets::common::{ModalBase, Quadrilateral, fill_page};
use crate::menu::widgets::controls::buttons::IconButton;
use crate::menu::widgets::controls::input_label::{InputLabel, InputLabelParams};
use crate::menu::widgets::controls::interactor::{batch_set_context, set_focused};
use crate::menu::widgets::controls::{find, wd};
use crate::slate::{Margin, Vis, WidgetId};

/// EInputActionModifier 1..4 and the arrow's render angle (UI_CharacterMoveRow @1065-1391: the arrow texture points
/// right).
const MODIFIERS: [&str; 5] = ["None", "Neutral", "Side", "Up", "Down"];
const ARROW_ANGLE: [f64; 5] = [0.0, 0.0, 0.0, -90.0, 90.0];

/// A row of a MoveList table (Title, Description, OutOfDate).
#[derive(Clone, Debug, Default)]
struct Move {
    title: String,
    description: String,
    out_of_date: bool,
}

fn move_of(moves: &Value, name: &str) -> Option<Move> {
    let r = moves.get(name)?;
    let s = |k: &str| r.get(k).and_then(Value::as_str).unwrap_or("").to_string();
    Some(Move {
        title: s("title"),
        description: s("description"),
        out_of_date: r.get("outOfDate").and_then(Value::as_bool).unwrap_or(false),
    })
}

// ---------- UI_CharacterMoveRow ----------

const ROW_ENTER: u32 = 1;

/// Init @67's arguments: Direction, Ground Title, Air Title, Ground Description, Air Description, Input Modifier,
/// Input Action (0 GameAttack, 1 GameSpecial), Ground OOD, Air OOD.
struct RowInit {
    direction: &'static str,
    ground: Move,
    air: Move,
    modifier: usize,
    action: u8,
}

#[derive(Debug, Default)]
pub struct CharacterMoveRow;

impl CharacterMoveRow {
    pub fn make(_cx: &mut Cx<'_>) -> Box<dyn Controller> {
        Box::new(CharacterMoveRow)
    }

    fn init(&mut self, cx: &mut Cx<'_>, a: &RowInit) {
        let shown = |on: bool| if on { Vis::SelfHitTestInvisible } else { Vis::Collapsed };
        set_vis(cx, "GroundError", shown(a.ground.out_of_date));
        set_vis(cx, "AirError", shown(a.air.out_of_date));
        set_text(cx, "directionLabel", a.direction);
        // @241-848: the label's Params are written whole and it is refreshed (Label empty, PlayerIndex 0, automatic
        // input type; TextFontInfo Display 1 16, KeyFontInfo NormsCond-ExtraBoldItalic 16)
        let button = find(cx, "inputButton");
        let params = InputLabelParams {
            action: if a.action == 1 { "GameSpecial" } else { "GameAttack" }.to_string(),
            modifier: MODIFIERS.get(a.modifier).copied().unwrap_or("None").to_string(),
            label: String::new(),
            player_index: 0,
            automatic_input_type: true,
            fixed_input_type: 0,
            tint: [0.68, 0.8, 0.85, 1.0],
            text_size: 16.0,
            key_size: 16.0,
            icon_size: [32.0, 32.0],
            custom_icon: None,
        };
        cx.visit::<InputLabel, _>(button, |l, cx| {
            l.params = params;
            l.force_refresh(cx);
        });
        set_text(cx, "GroundTitle", &a.ground.title);
        set_text(cx, "AirTitle", &a.air.title);
        set_text(cx, "groundDescription", &a.ground.description);
        set_text(cx, "airDescription", &a.air.description);
        // @1065-2078: the arrow and the plus show for Side, Up and Down
        let arrow = find(cx, "inputDirection");
        wd(cx, arrow).transform.angle = ARROW_ANGLE.get(a.modifier).copied().unwrap_or(0.0);
        let v = shown((2..=4).contains(&a.modifier));
        wd(cx, arrow).vis = v;
        set_vis(cx, "inputPlus", v);
    }

    /// Enter(Delay) @2138: hidden, then (at once for a delay of 0, else after the Delay) shown with EnterAnim at 1.25.
    fn enter(&mut self, cx: &mut Cx<'_>, delay: f64) {
        let me = cx.w;
        wd(cx, me).vis = Vis::Hidden;
        if delay == 0.0 {
            self.show(cx);
        } else {
            cx.view.delay(delay, Handler { owner: me, id: ROW_ENTER });
        }
    }

    fn show(&mut self, cx: &mut Cx<'_>) {
        let me = cx.w;
        wd(cx, me).vis = Vis::SelfHitTestInvisible;
        cx.view.play(me, "EnterAnim", false, 1.25);
    }
}

impl Controller for CharacterMoveRow {
    fn resume(&mut self, cx: &mut Cx<'_>, id: u32) {
        if id == ROW_ENTER {
            self.show(cx);
        }
    }
}

// ---------- UI_CharacterPassiveMove ----------

#[derive(Debug, Default)]
pub struct CharacterPassiveMove;

impl CharacterPassiveMove {
    pub fn make(_cx: &mut Cx<'_>) -> Box<dyn Controller> {
        Box::new(CharacterPassiveMove)
    }

    /// Init(Title, Description, Icon) @54: without an icon (every caller of the table path passes none) the image is
    /// collapsed.
    fn init(&mut self, cx: &mut Cx<'_>, title: &str, description: &str, icon: Option<&str>) {
        set_text(cx, "titleLabel", title);
        set_text(cx, "descriptionLabel", description);
        let img = find(cx, "icon");
        match icon {
            Some(i) => {
                let w = wd(cx, img);
                w.tex = Some(i.to_string());
                w.vis = Vis::SelfHitTestInvisible;
            }
            None => wd(cx, img).vis = Vis::Collapsed,
        }
    }
}

impl Controller for CharacterPassiveMove {}

// ---------- UI_CharacterMoveList ----------

/// OnInitialized @10493: moveRows, with the direction text and modifier each gets in UpdateAppearance.
/// (widget, kind, direction, modifier, action)
const ROWS: [(&str, &str, &str, usize, u8); 8] = [
    ("moveNeutralAttacks", "Normal", "Neutral", 1, 0),
    ("moveSideAttacks", "Normal", "Side", 2, 0),
    ("moveUpAttacks", "Normal", "Up", 3, 0),
    ("moveDownAttacks", "Normal", "Down", 4, 0),
    ("moveNeutralSpecials", "Special", "Neutral", 1, 1),
    ("moveSideSpecials", "Special", "Side", 2, 1),
    ("moveUpSpecials", "Special", "Up", 3, 1),
    ("moveDownSpecials", "Special", "Down", 4, 1),
];

const ML_EXIT: u32 = 1;
const ML_SCROLL: u32 = 2;
const ML_SUBMIT2: u32 = 3;
const ML_FOCUS: u32 = 4;

#[derive(Debug, Default)]
pub struct CharacterMoveList {
    base: ModalBase,
    /// OnDismiss (broadcast from Destruct).
    pub on_dismiss: Event<()>,
    character: Option<String>,
    bound: bool,
}

impl CharacterMoveList {
    pub fn make(_cx: &mut Cx<'_>) -> Box<dyn Controller> {
        Box::<CharacterMoveList>::default()
    }

    /// OnInitialized @10493 / PostShow @11190 bind these; the port takes them off in both Exit and Destruct.
    fn unbind(&mut self, cx: &mut Cx<'_>) {
        if !self.bound {
            return;
        }
        self.bound = false;
        let me = cx.w;
        let m = &mut cx.view.manager;
        m.on_scroll.remove_owner(me);
        m.on_submit2.remove_owner(me);
    }

    /// Exit @10266 (UnbindEvents @11726, DismissModal(Modal ID)); the UI flow record is not ported.
    fn exit(&mut self, cx: &mut Cx<'_>) {
        self.unbind(cx);
        let id = self.base.modal_id.clone();
        Manager::dismiss_modal(cx, &id);
    }

    /// SetCharacter @2689: the type line, UpdateAppearance, Enter. `character` is the port's fighter name.
    pub fn set_character(&mut self, cx: &mut Cx<'_>, character: &str) {
        self.character = Some(character.to_string());
        let entry = cx.view.roster().iter().find(|r| r.fighter.as_deref() == Some(character)).cloned();
        if let Some(e) = entry {
            let t = find(cx, "UI_CharacterTypeInfo");
            cx.visit::<CharacterTypeInfo, _>(t, |t, cx| t.set_character(cx, &e));
        }
        self.update_appearance(cx);
        self.enter(cx);
    }

    /// UpdateAppearance @2782. Only the MoveList table's path is ported: every CharacterData of the port has one (the
    /// path without a table reads CharacterData.GetMoves, @4804 / @7769 / @599).
    fn update_appearance(&mut self, cx: &mut Cx<'_>) {
        let Some(name) = self.character.clone() else { return };
        let Some(c) = cx.view.fighters().get(&name).cloned() else { return };
        let experimental = cx
            .view
            .roster()
            .iter()
            .find(|r| r.fighter.as_deref() == Some(name.as_str()))
            .is_some_and(|r| r.experimental);
        set_text(cx, "characterLabel", &c.display_name);
        if c.background_color.is_object() {
            let g = |k: &str| c.background_color.get(k).and_then(Value::as_f64).unwrap_or(0.0);
            let colour: Rgba = [g("R"), g("G"), g("B"), g("A")];
            for n in ["WBP_Quadrilateral", "WBP_Quadrilateral_1"] {
                let q = find(cx, n);
                cx.visit::<Quadrilateral, _>(q, |q, cx| q.update_color(cx, colour));
            }
        }
        set_vis(cx, "UI_PM_Experimental", if experimental { Vis::SelfHitTestInvisible } else { Vis::Hidden });
        if let Some(moves) = c.moves.as_object().map(|_| c.moves.clone()) {
            // @3129 attacks, @6094 specials: a group stops at the first pair with a missing row
            for group in ["Normal", "Special"] {
                for (row, kind, direction, modifier, action) in ROWS {
                    if kind != group {
                        continue;
                    }
                    let (Some(ground), Some(air)) = (
                        move_of(&moves, &format!("{kind}Ground{direction}")),
                        move_of(&moves, &format!("{kind}Air{direction}")),
                    ) else {
                        break;
                    };
                    let w = find(cx, row);
                    let a = RowInit { direction, ground, air, modifier, action };
                    cx.visit::<CharacterMoveRow, _>(w, |r, cx| r.init(cx, &a));
                }
            }
            // @9059: a passive row for each table row whose name contains "Passives" (no case), in the table's order,
            // each in a slot with a top padding of 25
            let bx = find(cx, "passiveContainer");
            for old in cx.view.tree().widget(bx).children.clone() {
                MenuView::discard(cx, old);
            }
            if let Some(o) = moves.as_object() {
                for (name, _) in o {
                    if !name.to_lowercase().contains("passives") {
                        continue;
                    }
                    let Some(row) = move_of(&moves, name) else { continue };
                    let w = MenuView::create(cx, "UI_CharacterPassiveMove");
                    cx.visit::<CharacterPassiveMove, _>(w, |p, cx| p.init(cx, &row.title, &row.description, None));
                    wd(cx, w).pad = Margin { l: 0.0, t: 25.0, r: 0.0, b: 0.0 };
                    MenuView::add_child(cx, bx, w, None);
                }
            }
            // @9226: the title and the rows show with a row named Passives
            let v = if moves.get("Passives").is_some() { Vis::SelfHitTestInvisible } else { Vis::Collapsed };
            set_vis(cx, "passiveTitle", v);
            wd(cx, bx).vis = v;
        }
        set_vis(cx, "ClassPassiveContainer", Vis::Collapsed); // @10171
    }

    /// Enter @10836: EnterAnim, and each row's Enter with a delay of its index x 0.05 s, or none where the game
    /// state's GetGameModeType is 3 (TrainingGameState, the Lab).
    fn enter(&mut self, cx: &mut Cx<'_>) {
        let me = cx.w;
        cx.view.play(me, "EnterAnim", false, 1.0);
        let lab = cx.lab.is_some();
        for (i, (name, ..)) in ROWS.iter().enumerate() {
            let w = find(cx, name);
            let delay = if lab { 0.0 } else { i as f64 * 0.05 };
            run_on::<CharacterMoveRow>(cx, w, move |r, cx| r.enter(cx, delay));
        }
    }
}

impl Modal for CharacterMoveList {
    fn pre_show(&mut self, cx: &mut Cx<'_>, id: &str) {
        self.base.pre_show(cx, id);
    }

    /// PostShow @11190: a new context, BindEvents @11414, and the focus on the next frame.
    fn post_show(&mut self, cx: &mut Cx<'_>, _instructions: &str, _extra_data: &str) {
        let ctx = cx.view.manager.new_guid();
        self.base.context = ctx.clone();
        let fc = find(cx, "focusContext");
        let ib = find(cx, "UI_IconButton");
        let ii = cx.view.tree().find_in(ib, "UI_Interactor");
        batch_set_context(cx, &[fc, ii], &ctx);
        let me = cx.w;
        let sc = find(cx, "scrollContainer");
        {
            let m = &mut cx.view.manager;
            m.on_scroll.add(Handler { owner: me, id: ML_SCROLL });
            m.register_scrollbox(sc);
            m.on_submit2.add(Handler { owner: me, id: ML_SUBMIT2 });
        }
        self.bound = true;
        cx.view.delay(0.0, Handler { owner: me, id: ML_FOCUS });
    }

    fn pre_dismiss(&mut self, cx: &mut Cx<'_>) {
        self.base.pre_dismiss(cx);
    }

    fn post_dismiss(&mut self, cx: &mut Cx<'_>) {
        self.base.post_dismiss(cx);
    }

    /// RequestDismiss @10223.
    fn request_dismiss(&mut self, cx: &mut Cx<'_>, _reason: &str, _from_cancel: bool) {
        self.exit(cx);
    }

    fn get_modal_context(&self) -> String {
        self.base.context.clone()
    }
}

impl Controller for CharacterMoveList {
    /// OnInitialized @10493.
    fn on_initialized(&mut self, cx: &mut Cx<'_>) {
        let shortcut = social_panel_uses_shortcut(cx);
        let v = if shortcut { Vis::Collapsed } else { Vis::SelfHitTestInvisible };
        for n in ["inputLabelShortcut", "inputLabelsDiv"] {
            set_vis(cx, n, v);
        }
        let ib = find(cx, "UI_IconButton");
        let me = cx.w;
        cx.visit::<IconButton, _>(ib, |b, _| b.on_click.add(Handler { owner: me, id: ML_EXIT })); // @11947
    }

    // Deviation: the 16:9 box follows the page.
    fn construct(&mut self, cx: &mut Cx<'_>) {
        let b = find(cx, "SizeBox_0");
        fill_page(cx, b);
    }

    fn tick(&mut self, cx: &mut Cx<'_>, _dt: f64) {
        let b = find(cx, "SizeBox_0");
        fill_page(cx, b);
    }

    fn handle(&mut self, cx: &mut Cx<'_>, id: u32, arg: &dyn Any) {
        match id {
            ML_EXIT => self.exit(cx),
            // OnScroll @10738
            ML_SCROLL => {
                if let Some(&(_, _, amount)) = arg.downcast_ref::<(i32, i32, f64)>() {
                    let sc = find(cx, "scrollContainer");
                    scroll_list(cx.view.tree_mut(), sc, amount * 2.0);
                }
            }
            // OnSubmit2 @11338
            ML_SUBMIT2 if !social_panel_uses_shortcut(cx) => self.exit(cx),
            _ => {}
        }
    }

    fn resume(&mut self, cx: &mut Cx<'_>, id: u32) {
        if id == ML_FOCUS {
            let fc = find(cx, "focusContext");
            set_focused(cx, fc, true);
        }
    }

    /// Destruct @11976 (time_viewed is the analytics'). The port takes its delegates off here too: ToggleMovelist
    /// dismisses the modal without Exit, and the build's bindings end with the widget.
    fn destruct(&mut self, cx: &mut Cx<'_>) {
        self.unbind(cx);
        let sc = find(cx, "scrollContainer");
        cx.view.manager.unregister_scrollbox(sc);
        cx.emit(self, |c| &c.on_dismiss, &());
    }

    fn as_modal(&mut self) -> Option<&mut dyn Modal> {
        Some(self)
    }
}

/// UI_Functions IsSocialPanelUsingShortcut?: FindOrCreateSocialPanel(false) (the newest UI_SocialPanel visible in its
/// hierarchy), its Use Shortcut Binding.
/// Deviation: the social panel is not ported (R7), so none is up and the answer is false.
pub fn social_panel_uses_shortcut(_cx: &Cx<'_>) -> bool {
    false
}

/// The pre-match's ShowMoveList PM@103760-104051: ShowModal-Generic(false, UI_CharacterMoveList, "movelist"), then
/// BPI_SetCharacter(character) on it. `character` is the port's fighter name; the caller's checks come first.
pub fn show_movelist(cx: &mut Cx<'_>, character: &str) {
    let modal = Manager::show_modal(cx, "UI_CharacterMoveList", "movelist", "", "");
    if let Some(m) = modal
        && cx.view.fighters().contains_key(character)
    {
        let c = character.to_string();
        cx.visit::<CharacterMoveList, _>(m, |l, cx| l.set_character(cx, &c));
    }
}

/// TrainingGameState.ToggleMovelist (the MOVE LIST button, and the player controller's Tab key through
/// TrainingModePlayerState.ToggleMovelist): the modal is dismissed when it is up, else shown with the character of
/// the local player's slot of the training config (a Random slot has none: the list stays as designed).
pub fn toggle_movelist(cx: &mut Cx<'_>) {
    if cx.view.manager.get_modal("training_movelist").is_some() {
        Manager::dismiss_modal(cx, "training_movelist");
        return;
    }
    let modal = Manager::show_modal(cx, "UI_CharacterMoveList", "training_movelist", "", "");
    let character: Option<String> = cx.lab.as_ref().and_then(|s| {
        let me = s.me();
        s.config()
            .players
            .iter()
            .find(|p| !p.account_id.is_empty() && p.account_id == me)
            .and_then(|p| p.character.clone())
    });
    if let (Some(m), Some(c)) = (modal, character)
        && cx.view.fighters().contains_key(&c)
    {
        cx.visit::<CharacterMoveList, _>(m, |l, cx| l.set_character(cx, &c));
    }
}

/// The widget of the modal `training_movelist`, when it is up.
pub fn movelist_modal(cx: &Cx<'_>) -> Option<WidgetId> {
    cx.view.manager.get_modal("training_movelist")
}
