//! (the port's own) The Settings panel's CLASSIC page: the port's own settings (Chat in Match, Show Me in the
//! Online List, Invites), each a toggle of the Interface page's Knockback Influence row's class and props but for
//! its label and tip. The page is built from the Audio page's own nodes (its VerticalBox, its ScrollBox and its
//! title), so its title and rows sit where the Audio page's do. The panel (`preferences.rs`) owns it: it makes it,
//! shows it from the section bar and takes its toggles' changes.
// Deviation: the build has no such page or rows (the beta has no chat; its friends list and party invites were the
// backend's): the rows and their words are the port's.

use serde_json::{Value, json};

use super::controls::buttons::ToggleSwitch;
use super::controls::interactor::{batch_set_scroll_box, set_focused};
use super::controls::wd;
use crate::bundle::WData;
use crate::menu::controller::{Cx, Handler};
use crate::menu::custom_text;
use crate::menu::view::MenuView;
use crate::slate::WidgetId;
use mvs_settings::UiSettings;

/// The page's name in the panel's WidgetSwitcher.
pub const PAGE: &str = "PanelClassic";
/// The page's ScrollBox.
const SCROLL: &str = "classicScrollBox";

/// The page's rows, by the Settings field each writes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum OwnRow {
    ChatInMatch,
    ShowInOnlineList,
    Invites,
}
/// (field, widget name, label, tip), in the page's order.
const OWN_ROWS: [(OwnRow, &str, &str, &str); 3] = [
    (OwnRow::ChatInMatch, "UI_ToggleChatInMatch", custom_text::CHAT_SETTING, custom_text::CHAT_SETTING_TOOLTIP),
    (
        OwnRow::ShowInOnlineList,
        "UI_ToggleShowInOnlineList",
        custom_text::ONLINE_LIST_SETTING,
        custom_text::ONLINE_LIST_SETTING_TOOLTIP,
    ),
    (OwnRow::Invites, "UI_ToggleInvites", custom_text::INVITES_SETTING, custom_text::INVITES_SETTING_TOOLTIP),
];
/// The number of rows: the panel's handler ids `first..first + ROWS` are theirs.
pub const ROWS: u32 = OWN_ROWS.len() as u32;

fn own_field(u: &mut UiSettings, r: OwnRow) -> &mut bool {
    match r {
        OwnRow::ChatInMatch => &mut u.chat_in_match,
        OwnRow::ShowInOnlineList => &mut u.show_in_online_list,
        OwnRow::Invites => &mut u.invites,
    }
}

/// The node of a widget class's tree by its name.
fn node<'a>(n: &'a WData, name: &str) -> Option<&'a WData> {
    if n.n == name {
        return Some(n);
    }
    n.c.iter().find_map(|c| node(c, name))
}

/// The node `name` of class `class`, without its children, renamed `to`.
fn template(cx: &Cx<'_>, class: &str, name: &str, to: &str) -> WData {
    let n = cx.view.bundle.widgets.get(class).and_then(|c| node(&c.root, name));
    let n = n.unwrap_or_else(|| panic!("menu: no {name} in {class} for the Classic page"));
    WData { n: to.to_owned(), c: Vec::new(), ns: Default::default(), ..n.clone() }
}

#[derive(Clone, Debug)]
pub struct ClassicPage {
    pub root: WidgetId,
    scroll: WidgetId,
    rows: Vec<(OwnRow, WidgetId)>,
}

impl ClassicPage {
    /// The page made last into the panel's `switcher`, its toggles' changes sent to the panel (`cx.w`) as handler
    /// `first + row`.
    pub fn make(cx: &mut Cx<'_>, switcher: WidgetId, first: u32) -> ClassicPage {
        const PANEL: &str = "UI_PreferencesPanelRework";
        let bundle = cx.view.bundle.clone();
        let page = template(cx, PANEL, "PanelAudio", PAGE);
        let scroll = template(cx, PANEL, "audioScrollbox", SCROLL);
        let title = template(cx, PANEL, "TextBlock_145", "ClassicTitle");
        let t = cx.view.tree_mut();
        let root = t.create_node(&bundle, &page);
        let sb = t.create_node(&bundle, &scroll);
        let head = t.create_node(&bundle, &title);
        t.widget_mut(head).text = custom_text::CLASSIC_TITLE.to_owned();
        t.add(sb, head);
        t.add(root, sb);
        // the Knockback Influence row's props (UI_PreferencesInterfaceRework), its label and tip the row's own
        let like = template(cx, "UI_PreferencesInterfaceRework", "UI_ToggleShowDI", "");
        let me = cx.w;
        let mut rows = Vec::new();
        for (i, (row, name, label, tip)) in OWN_ROWS.into_iter().enumerate() {
            let w = MenuView::create(cx, "UI_ToggleSwitchRework");
            let t = cx.view.tree_mut();
            if let Value::Object(m) = &like.p {
                for (k, val) in m {
                    t.set_prop(w, k, val.clone());
                }
            }
            t.set_prop(w, "LabelText", json!({ "CultureInvariantString": label }));
            t.set_prop(w, "Tooltip", json!({ "CultureInvariantString": tip }));
            wd(cx, w).name = name.to_owned();
            cx.view.tree_mut().add(sb, w);
            cx.visit::<ToggleSwitch, _>(w, |t, _| t.on_toggle_changed.add(Handler { owner: me, id: first + i as u32 }));
            rows.push((row, w));
        }
        MenuView::add_child(cx, switcher, root, None);
        cx.view.manager.register_scrollbox(sb);
        let page = ClassicPage { root, scroll: sb, rows };
        let list = page.interactors(cx);
        batch_set_scroll_box(cx, &list, sb);
        page.refresh(cx);
        page
    }

    pub fn scrollbox(&self) -> WidgetId {
        self.scroll
    }

    /// Every toggle from the settings.
    pub fn refresh(&self, cx: &mut Cx<'_>) {
        let mut ui = cx.prefs.ui.clone();
        for &(r, w) in &self.rows {
            let on = *own_field(&mut ui, r);
            cx.visit::<ToggleSwitch, _>(w, |t, cx| t.set_on(cx, on));
        }
    }

    /// The toggles' UI_Interactor, in the page's order.
    pub fn interactors(&self, cx: &Cx<'_>) -> Vec<WidgetId> {
        self.rows.iter().map(|&(_, w)| cx.view.tree().find_in(w, "UI_Interactor")).collect()
    }

    pub fn focus_first(&self, cx: &mut Cx<'_>) {
        if let Some(&i) = self.interactors(cx).first() {
            set_focused(cx, i, false);
        }
    }

    /// Row `row`'s toggle changed to `on`: its Settings field written.
    pub fn toggled(cx: &mut Cx<'_>, row: u32, on: bool) {
        if let Some(&(r, ..)) = OWN_ROWS.get(row as usize) {
            *own_field(&mut cx.prefs.ui, r) = on;
        }
    }
}
