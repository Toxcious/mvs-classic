//! UI_Functions ShowModal-Generic, and a screen the port lacks.

use std::collections::HashSet;
use std::sync::{Mutex, OnceLock};

use crate::menu::controller::Cx;
use crate::menu::manager::Manager;
use crate::menu::registry;
use crate::slate::WidgetId;

/// The modal ids warned about once.
fn warned() -> &'static Mutex<HashSet<String>> {
    static W: OnceLock<Mutex<HashSet<String>>> = OnceLock::new();
    W.get_or_init(|| Mutex::new(HashSet::new()))
}

/// Port: a screen whose class has no tree in the bundle or no controller is not shown: one warning per modal id,
/// and the caller does what it does for a modal that did not open.
pub fn can_show_modal(cx: &Cx<'_>, cls: &str, id: &str) -> bool {
    let bundled = cx.view.bundle.widgets.contains_key(cls);
    if bundled && registry::make(cls).is_some() {
        return true;
    }
    let mut w = warned().lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    if w.insert(id.to_string()) {
        let why = if bundled { "has no controller" } else { "is not in the bundle" };
        eprintln!("menu: modal {id} ({cls}) {why}");
    }
    false
}

/// UI_Functions.ShowModal-Generic(DismissOthers, Class, ID, Instructions): DismissAllModals first when asked, then
/// the manager's ShowModal (None, and nothing shown, when the ID is open). A screen the port lacks shows nothing
/// (`can_show_modal`), and nothing is dismissed for it. The modal's controller: `MenuView::modal`.
pub fn show_modal_generic(
    cx: &mut Cx<'_>,
    dismiss_others: bool,
    cls: &str,
    id: &str,
    instructions: &str,
) -> Option<WidgetId> {
    if !can_show_modal(cx, cls, id) {
        return None;
    }
    if dismiss_others {
        Manager::dismiss_all_modals(cx);
    }
    Manager::show_modal(cx, cls, id, instructions, "")
}
