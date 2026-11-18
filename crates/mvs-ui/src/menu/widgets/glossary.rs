//! The main menu's Glossary: UI_GlossaryTooltipContainer and its rows,
//! UI_GlossaryTooltipText.

use std::any::Any;

use super::common::ModalBase;
use super::controls::interactor::set_focused;
use super::controls::{find, wd};
use super::nav::GlobalSlideOut;
use super::section_bar::SectionBar;
use crate::menu::controller::{Controller, Cx, Handler};
use crate::menu::manager::Modal;
use crate::menu::scroll::{scroll_list, scroll_to_start};
use crate::menu::view::MenuView;
use crate::slate::{HAlign, VAlign, Vis, WidgetId};

// ---------- UI_GlossaryTooltipText (one row of a list) ----------

/// UI_GlossaryTooltipText. Of its parent UI_GridItemBase nothing shows (no interactor, no animation, and the
/// container never calls Enter): Init is the whole of it.
#[derive(Debug, Default)]
pub struct GlossaryTooltipText;

/// Init's backing rule (@176): the backing shows on the rows of odd index.
pub fn backing_vis(index: usize) -> Vis {
    if !index.is_multiple_of(2) { Vis::SelfHitTestInvisible } else { Vis::Hidden }
}

impl GlossaryTooltipText {
    pub fn make(_cx: &mut Cx<'_>) -> Box<dyn Controller> {
        Box::new(GlossaryTooltipText)
    }

    /// Init(Name, Description, EntryIndex) @10.
    pub fn init(&mut self, cx: &mut Cx<'_>, name: &str, description: &str, index: usize) {
        let n = find(cx, "GlossaryName");
        wd(cx, n).text = name.to_string();
        let d = find(cx, "GlossaryDescription");
        wd(cx, d).text = description.to_string();
        let b = find(cx, "Backing");
        wd(cx, b).vis = backing_vis(index);
    }
}

impl Controller for GlossaryTooltipText {}

// ---------- UI_GlossaryTooltipContainer (the modal `glossary`) ----------

/// PreConstruct's three fills in their order: the DataTable and the VerticalBox it fills.
const FILLS: [(&str, &str); 3] = [
    ("DT_GameplayGlossary", "GameplayVertBox"),
    ("DT_ModifierGlossary", "ModifiersVertBox"),
    ("DT_EconomyGlossary", "EconomyVertBox"),
];

/// ModeTypeSelected's switch on the tab's ID (case-insensitive): the list it shows.
pub fn list_of(id: &str) -> Option<&'static str> {
    match id.to_lowercase().as_str() {
        "modifiers" => Some("ModifiersVertBox"),
        "gameplay" => Some("GameplayVertBox"),
        "economy" => Some("EconomyVertBox"),
        _ => None,
    }
}

const H_MODAL_DISMISSED: u32 = 1;
const H_SCROLL: u32 = 2;
const H_MODE: u32 = 3;
const H_FOCUS_FIRST: u32 = 4;
const H_DISMISS_TAIL: u32 = 5;

/// UI_GlossaryTooltipContainer: a WBP_GlobalSlideOut panel with a header, three tabs and one ScrollBox over a
/// WidgetSwitcher of three lists. The lists are filled once from the bundle's tables in row order; nothing is read
/// from the backend. Not ported: the analytics calls (Record UIFlow Event @3198 and in ModeTypeSelected, Record
/// UIFlow Dismissal Event @417).
#[derive(Debug, Default)]
pub struct GlossaryTooltipContainer {
    base: ModalBase,
    filled: bool,
}

impl GlossaryTooltipContainer {
    pub fn make(_cx: &mut Cx<'_>) -> Box<dyn Controller> {
        Box::new(GlossaryTooltipContainer::default())
    }

    fn slide_out(cx: &Cx<'_>) -> WidgetId {
        find(cx, "WBP_GlobalSlideOut")
    }

    fn bar(cx: &Cx<'_>) -> WidgetId {
        find(cx, "UI_SectionBar_Type")
    }

    fn scroll_box(cx: &Cx<'_>) -> WidgetId {
        find(cx, "ScrollBox_0")
    }

    /// UpdateContext @3571 (GetInteractors returns an empty array).
    pub fn update_context(&mut self, cx: &mut Cx<'_>, ctx: &str) {
        self.base.context = ctx.to_string();
        let bar = Self::bar(cx);
        cx.visit::<SectionBar, _>(bar, |b, cx| b.update_context(cx, ctx, None));
        let so = Self::slide_out(cx);
        cx.visit::<GlobalSlideOut, _>(so, |s, cx| s.update_context(cx, ctx));
    }

    fn set_active_list(cx: &mut Cx<'_>, name: &str) {
        let sw = find(cx, "WidgetSwitcher_0");
        let t = cx.view.tree();
        if let Some(at) = t.widget(sw).children.iter().position(|&c| t.widget(c).name == name) {
            wd(cx, sw).active = at as i32;
        }
    }

    /// ModeTypeSelected(Index, ID) @3772: the tab's list, then ScrollToStart @4106; another ID does nothing.
    /// Unverified: the scroll range follows the active list (the switcher's desired size is its active child's, as
    /// slate has it).
    fn mode_type_selected(cx: &mut Cx<'_>, id: &str) {
        let Some(name) = list_of(id) else { return };
        Self::set_active_list(cx, name);
        let sb = Self::scroll_box(cx);
        scroll_to_start(cx.view.tree_mut(), sb);
    }

    /// The tail @106-326. (A shadow-tint cache has nothing to release here.)
    fn unbind(cx: &mut Cx<'_>) {
        let me = cx.w;
        cx.view.manager.modal_dismissed.remove(Handler { owner: me, id: H_MODAL_DISMISSED });
        cx.view.manager.on_scroll.remove(Handler { owner: me, id: H_SCROLL });
        let sb = Self::scroll_box(cx);
        cx.view.manager.unregister_scrollbox(sb);
    }
}

impl Modal for GlossaryTooltipContainer {
    fn pre_show(&mut self, cx: &mut Cx<'_>, id: &str) {
        self.base.pre_show(cx, id);
    }

    /// PostShow @2677.
    fn post_show(&mut self, cx: &mut Cx<'_>, _instructions: &str, _extra_data: &str) {
        let me = cx.w;
        let id = self.base.modal_id.clone();
        let so = Self::slide_out(cx);
        cx.visit::<GlobalSlideOut, _>(so, |s, cx| s.open_slide_out(cx, &id));
        cx.view.manager.on_scroll.add(Handler { owner: me, id: H_SCROLL });
        let sb = Self::scroll_box(cx);
        cx.view.manager.register_scrollbox(sb);
        let g = cx.view.manager.new_guid();
        self.update_context(cx, &g);
        let bar = Self::bar(cx);
        cx.visit::<SectionBar, _>(bar, |b, cx| {
            b.on_click.add(Handler { owner: me, id: H_MODE });
            b.set_current_index(cx, 0);
        });
        Self::set_active_list(cx, "ModifiersVertBox");
        // @3264 Delay(0.2) -> @454: SetFocused(ContextOnly true) on the first tab's interactor
        cx.view.delay(0.2, Handler { owner: me, id: H_FOCUS_FIRST });
    }

    fn pre_dismiss(&mut self, _cx: &mut Cx<'_>) {}

    fn post_dismiss(&mut self, _cx: &mut Cx<'_>) {}

    /// RequestDismiss @3319: both arguments ignored; Delay(0) @3347 -> the tail @15.
    fn request_dismiss(&mut self, cx: &mut Cx<'_>, _reason: &str, _from_cancel: bool) {
        let me = cx.w;
        cx.view.delay(0.0, Handler { owner: me, id: H_DISMISS_TAIL });
    }

    fn get_modal_context(&self) -> String {
        self.base.get_modal_context()
    }
}

impl Controller for GlossaryTooltipContainer {
    fn as_modal(&mut self) -> Option<&mut dyn Modal> {
        Some(self)
    }

    /// @3566 -> @3455.
    fn on_initialized(&mut self, cx: &mut Cx<'_>) {
        let me = cx.w;
        cx.view.manager.modal_dismissed.add(Handler { owner: me, id: H_MODAL_DISMISSED });
    }

    /// PreConstruct @2622: per table, row "1" .. "N" in turn makes a UI_GlossaryTooltipText, Init(Name, Description,
    /// i) and AddChildToVerticalBox. Unverified: the loop's count is the length of GetDataTableColumnAsString(table,
    /// "key"), taken as one entry per row. (The widget is made once per ShowModal: the guard is for a second construct
    /// of the same widget, where the build would add the rows again; no path of the port constructs it twice.)
    fn pre_construct(&mut self, cx: &mut Cx<'_>) {
        if self.filled {
            return;
        }
        self.filled = true;
        for (table, box_name) in FILLS {
            let bx = find(cx, box_name);
            let rows = cx.view.glossary(table).to_vec();
            for (i, row) in rows.iter().enumerate() {
                let w = MenuView::create(cx, "UI_GlossaryTooltipText");
                cx.visit::<GlossaryTooltipText, _>(w, |t, cx| t.init(cx, &row.name, &row.description, i));
                // AddChildToVerticalBox: the slot's defaults (Fill, Fill, automatic size)
                let x = wd(cx, w);
                x.h_align = HAlign::Fill;
                x.v_align = VAlign::Fill;
                x.fill = 0.0;
                MenuView::add_child(cx, bx, w, None);
            }
        }
    }

    fn handle(&mut self, cx: &mut Cx<'_>, id: u32, arg: &dyn Any) {
        match id {
            // OnModalDismissed(Modal ID) @3402 (case-sensitive): the manager dismissed this modal itself (another
            // nav section's click). The build runs the same Delay(0) and tail. Unverified: whether the widget still
            // lives when that Delay fires is not read; the port's widget is destroyed by then (the delay would be
            // dropped), so the tail's unbinding runs here at once and CloseSlideOut (a second DismissModal, the
            // panel's slide back) does not run.
            H_MODAL_DISMISSED => {
                if arg.downcast_ref::<String>() == Some(&self.base.modal_id) {
                    Self::unbind(cx);
                }
            }
            // OnScroll(Player Index, Direction, Amount) @4494
            H_SCROLL => {
                if let Some(&(player, _, amount)) = arg.downcast_ref::<(i32, i32, f64)>()
                    && player == 0
                {
                    let sb = Self::scroll_box(cx);
                    scroll_list(cx.view.tree_mut(), sb, amount * 2.0);
                }
            }
            H_MODE => {
                if let Some((_, id)) = arg.downcast_ref::<(i32, String)>() {
                    Self::mode_type_selected(cx, id);
                }
            }
            _ => {}
        }
    }

    fn resume(&mut self, cx: &mut Cx<'_>, id: u32) {
        match id {
            H_FOCUS_FIRST => {
                let bar = Self::bar(cx);
                let first = cx.visit::<SectionBar, _>(bar, |b, _| b.interactors.first().copied()).flatten();
                if let Some(i) = first {
                    set_focused(cx, i, true);
                }
            }
            H_DISMISS_TAIL => {
                let id = self.base.modal_id.clone();
                let so = Self::slide_out(cx);
                cx.visit::<GlobalSlideOut, _>(so, |s, cx| s.close_slide_out(cx, &id));
                Self::unbind(cx);
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn backing_on_odd_rows() {
        assert_eq!(backing_vis(0), Vis::Hidden);
        assert_eq!(backing_vis(1), Vis::SelfHitTestInvisible);
        assert_eq!(backing_vis(2), Vis::Hidden);
    }

    #[test]
    fn list_by_tab_id() {
        assert_eq!(list_of("Modifiers"), Some("ModifiersVertBox"));
        assert_eq!(list_of("GAMEPLAY"), Some("GameplayVertBox"));
        assert_eq!(list_of("economy"), Some("EconomyVertBox"));
        assert_eq!(list_of("other"), None);
    }
}
