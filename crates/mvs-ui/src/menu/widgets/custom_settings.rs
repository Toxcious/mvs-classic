//! (the port's own) A custom lobby's settings: a page of lobby options
//! (mvs_protocol::lobby_rules `OptionKey`), a row each, drawn on UI_QuickMatchSettings' tree (the art, the panel, a
//! drop menu per row, Done and the close button; its own three menus and the hazards toggle stay collapsed). Two
//! pages: the room's (modal id `customGameSettings`, the lobby screen's ROOM SETTINGS button, an online lobby's) and
//! the experimental rules (`customGameExperimental`, the button in Reset To Defaults' place). The leader's rows ask
//! the lobby for the change at once (`on_option`); the others' show the values, greyed. The lobby screen hands the
//! modal its lobby's state and page (`bind`) and calls `refresh` at each change of the options or the leader.
//!
//! Deviation: the modal holds no Lobby; the lobby is the lobby screen's, which is busy while it calls this
//! modal, so the modal takes a `LobbyView` copy at `bind` / `refresh` and asks for a change through `on_option`.

use std::any::Any;

use mvs_protocol::lobby_rules::{LobbyOptions, OptionKey, OptionValue};
use serde_json::{Value, json};

use super::common::{ModalBase, fill_page};
use super::controls::bar_buttons::{P_ID, P_LABEL};
use super::controls::buttons::{ButtonMain, IconButton};
use super::controls::drop_menu::DropMenu;
use super::controls::interactor::{self, batch_set_context};
use super::controls::{find, wd};
use super::input_binding_modal::CapturePlayersFocus;
use super::quick_match_settings::{MODE_BG, MODE_FG};
use crate::menu::controller::{Controller, Cx, Event, Handler};
use crate::menu::custom_text as text;
use crate::menu::manager::Modal;
use crate::slate::{Vis, WidgetId};

/// A page's words: a Game string key (the menu's text table) or the port's own text (custom_text).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Words {
    Game(&'static str),
    Own(String),
}

#[derive(Clone, Debug)]
pub struct Row {
    pub key: OptionKey,
    pub label: Words,
    /// The values in the order the drop menu lists them, with their names.
    pub values: Vec<(OptionValue, Words)>,
}

#[derive(Clone, Debug)]
pub struct SettingsPage {
    pub title: Words,
    pub rows: Vec<Row>,
    /// The art on the left: the lobby's mode art (None), or a texture of its own.
    pub art: Option<&'static str>,
}

fn on_off() -> Vec<(OptionValue, Words)> {
    vec![
        (OptionValue::Bool(false), Words::Own(text::OFF.into())),
        (OptionValue::Bool(true), Words::Own(text::ON.into())),
    ]
}

/// The room's page.
pub fn room_page() -> SettingsPage {
    SettingsPage {
        title: Words::Game("2EFD913A40EFFF6563B156AE62312528"), // Settings
        rows: vec![
            Row {
                key: OptionKey::Public,
                label: Words::Game("C51E56ED4E673B922BDE2AAFB938F8F2"), // Privacy
                values: vec![
                    (OptionValue::Bool(false), Words::Game("1164D2E24901624F6C0BF48A216689B8")), // Private
                    (OptionValue::Bool(true), Words::Game("37F7D5A94886770D20288BA8D91BCB3D")),  // Public
                ],
            },
            // (the port's own) only the players a member invited may join (invitedOnly)
            Row { key: OptionKey::InvitedOnly, label: Words::Own(text::INVITED_ONLY.into()), values: on_off() },
        ],
        art: None,
    }
}

/// The experimental rules, the port's own (not in the beta); the art is the arcade's hard card background.
pub fn experimental_page() -> SettingsPage {
    SettingsPage {
        title: Words::Own(text::EXPERIMENTAL_TITLE.into()),
        rows: vec![
            Row { key: OptionKey::Stocks, label: Words::Own(text::STOCK_MODE.into()), values: on_off() },
            // the match's mutator (LOBBY_MUTATORS, by value)
            Row {
                key: OptionKey::Mutator,
                label: Words::Own(text::MUTATOR.into()),
                values: text::MUTATORS
                    .iter()
                    .enumerate()
                    .map(|(i, t)| (OptionValue::Choice(i), Words::Own((*t).into())))
                    .collect(),
            },
        ],
        art: Some("GameMode_AXHard_BG"),
    }
}

/// What the modal reads of the lobby (`Lobby.amILeader()`, `teamStyle`, `options`).
#[derive(Clone, Copy, Debug)]
pub struct LobbyView {
    pub leader: bool,
    pub team_style: u8,
    pub options: LobbyOptions,
}

/// The leader's request of a row (the lobby's `setOption`).
pub type OptionRequest = (OptionKey, OptionValue);

const LONG_LIST: usize = 8;
const BUILD_ROWS: [&str; 4] = ["UI_DropMenu_TeamStyle", "UI_DropMenu_Ringouts", "UI_DropMenu_Time", "Toggle_Hazards"];

const H_DONE: u32 = 1;
const H_CLOSE: u32 = 2;
/// Row `i`'s drop menu click is `H_ROW + i`.
const H_ROW: u32 = 16;

/// A row's drop menu.
#[derive(Debug)]
struct MenuRow {
    key: OptionKey,
    menu: WidgetId,
    values: Vec<OptionValue>,
}

#[derive(Debug)]
pub struct CustomGameSettings {
    base: ModalBase,
    view: Option<LobbyView>,
    page: SettingsPage,
    menus: Vec<MenuRow>,
    /// (the port's own) the leader's change of an option, for the lobby screen.
    pub on_option: Event<OptionRequest>,
}

fn option_text(v: OptionValue) -> String {
    match v {
        OptionValue::Bool(b) => b.to_string(),
        OptionValue::Choice(n) => n.to_string(),
    }
}

impl CustomGameSettings {
    pub fn make(_cx: &mut Cx<'_>) -> Box<dyn Controller> {
        Box::new(CustomGameSettings {
            base: ModalBase::default(),
            view: None,
            page: room_page(),
            menus: Vec::new(),
            on_option: Event::default(),
        })
    }

    fn words(cx: &Cx<'_>, w: &Words) -> String {
        match w {
            Words::Game(k) => cx.view.game_text(k).to_string(),
            Words::Own(s) => s.clone(),
        }
    }

    fn interactors(cx: &Cx<'_>) -> Vec<WidgetId> {
        let t = cx.view.tree();
        vec![
            find(cx, "contextInteractor"),
            t.find_in(find(cx, "buttonDone"), "UI_Interactor"),
            t.find_in(find(cx, "buttonClose"), "UI_Interactor"),
        ]
    }

    /// The page's rows, made once (`bind`).
    fn make_rows(&mut self, cx: &mut Cx<'_>) {
        let like = find(cx, "UI_DropMenu_TeamStyle");
        let (parent, user, pad, h_align, v_align, fill, like_props) = {
            let t = cx.view.tree();
            let l = t.widget(like);
            (
                l.parent,
                t.user_class(like).map(str::to_string),
                l.pad,
                l.h_align,
                l.v_align,
                l.fill,
                t.props(like).clone(),
            )
        };
        let (Some(parent), Some(user)) = (parent, user) else { return };
        let done = find(cx, "buttonDone");
        let mut at = cx.view.tree().widget(parent).children.iter().position(|c| *c == done);
        let ctx = self.base.context.clone();
        for r in self.page.rows.clone() {
            let w = crate::menu::view::MenuView::create(cx, &user);
            let items: Vec<Value> = r
                .values
                .iter()
                .map(|(v, t)| json!({ P_LABEL: { "CultureInvariantString": Self::words(cx, t) }, P_ID: option_text(*v) }))
                .collect();
            let n = items.len();
            let mut props = like_props.clone();
            if let Value::Object(o) = &mut props {
                o.insert("LabelText".into(), json!({ "CultureInvariantString": Self::words(cx, &r.label) }));
                o.insert("TopTooltipText".into(), Value::Null);
                o.insert("Items".into(), Value::Array(items));
                // a long list (the mutators) scrolls past LONG_LIST rows, by the drop menu's own scroll bar properties
                if n > LONG_LIST {
                    o.insert("EnableScrollBar".into(), Value::Bool(true));
                    o.insert("ShowScrollBarAfterCount".into(), json!(LONG_LIST));
                }
                for (k, v) in o.clone() {
                    cx.view.tree_mut().set_prop(w, &k, v);
                }
            }
            {
                let x = wd(cx, w);
                x.name = format!("Option_{:?}", r.key);
                x.pad = pad;
                x.h_align = h_align;
                x.v_align = v_align;
                x.fill = fill;
            }
            crate::menu::view::MenuView::add_child(cx, parent, w, at);
            at = at.map(|a| a + 1);
            cx.visit::<DropMenu, _>(w, |m, cx| m.initialize(cx));
            let me = cx.w;
            let row = self.menus.len() as u32;
            cx.visit::<DropMenu, _>(w, |m, _| m.on_click.add(Handler { owner: me, id: H_ROW + row }));
            self.menus.push(MenuRow { key: r.key, menu: w, values: r.values.iter().map(|(v, _)| *v).collect() });
            cx.visit_dyn(w, |_, cx| DropMenu::update_context(cx, &ctx, Some(0)));
        }
    }

    /// The lobby whose settings these are, and the page of them shown.
    pub fn bind(&mut self, cx: &mut Cx<'_>, view: LobbyView, page: SettingsPage) {
        self.view = Some(view);
        if self.menus.is_empty() {
            self.page = page;
            let title = find(cx, "TextBlock_37");
            let t = Self::words(cx, &self.page.title);
            wd(cx, title).text = t;
            self.make_rows(cx);
            // a page's own art is no tree's texture: held while the modal is up
            if let Some(art) = self.page.art {
                cx.view.hold_pictures(&self.base.modal_id, [art]);
            }
        }
        self.refresh(cx, Some(view));
    }

    /// The rows again from the lobby's options; interactable for its leader alone.
    pub fn refresh(&mut self, cx: &mut Cx<'_>, view: Option<LobbyView>) {
        if view.is_some() {
            self.view = view;
        }
        let Some(v) = self.view else { return };
        for m in &self.menus {
            let key = option_text(v.options.get(m.key));
            let main = cx.view.tree().find_in(m.menu, "mainInteractor");
            cx.visit::<DropMenu, _>(m.menu, |d, cx| d.set_active_by_id(cx, &key));
            interactor::set_interactable(cx, main, v.leader);
            if !v.leader {
                cx.visit::<DropMenu, _>(m.menu, |d, cx| d.close(cx));
            }
        }
        let style = usize::from(v.team_style).min(3);
        let art = self.page.art;
        let (bg, fg) = (find(cx, "TeamStyle_BG"), find(cx, "TeamStyle_FG"));
        wd(cx, bg).tex = art.or(MODE_BG[style]).map(str::to_string);
        // (an image with no texture draws white: a page's own art hides the mode's colour blend)
        let f = wd(cx, fg);
        f.vis = if art.is_some() { Vis::Hidden } else { Vis::SelfHitTestInvisible };
        if art.is_none() {
            f.tex = MODE_FG[style].map(str::to_string);
        }
    }

    pub fn update_context(&mut self, cx: &mut Cx<'_>, ctx: &str) {
        self.base.context = ctx.to_string();
        let list = Self::interactors(cx);
        batch_set_context(cx, &list, ctx);
        for m in &self.menus {
            cx.visit_dyn(m.menu, |_, cx| DropMenu::update_context(cx, ctx, Some(0)));
        }
        let cap = find(cx, "UI_CapturePlayersFocus");
        cx.visit::<CapturePlayersFocus, _>(cap, |c, cx| c.update_context(cx, ctx));
    }
}

impl Modal for CustomGameSettings {
    fn pre_show(&mut self, cx: &mut Cx<'_>, id: &str) {
        self.base.pre_show(cx, id);
    }

    fn post_show(&mut self, cx: &mut Cx<'_>, _instructions: &str, _extra_data: &str) {
        let title = find(cx, "TextBlock_37");
        let t = Self::words(cx, &self.page.title);
        wd(cx, title).text = t;
        let me = cx.w;
        cx.view.play(me, "EnterAnim", false, 1.0);
        let guid = cx.view.manager.new_guid();
        self.update_context(cx, &guid);
        let cap = find(cx, "UI_CapturePlayersFocus");
        cx.visit::<CapturePlayersFocus, _>(cap, |c, cx| c.focus(cx));
        let ctx = find(cx, "contextInteractor");
        interactor::set_focused(cx, ctx, true);
        self.refresh(cx, None);
    }

    fn pre_dismiss(&mut self, cx: &mut Cx<'_>) {
        self.base.pre_dismiss(cx);
    }

    fn post_dismiss(&mut self, cx: &mut Cx<'_>) {
        self.base.post_dismiss(cx);
    }

    fn request_dismiss(&mut self, cx: &mut Cx<'_>, reason: &str, from_cancel: bool) {
        self.base.request_dismiss(cx, reason, from_cancel);
    }

    fn get_modal_context(&self) -> String {
        self.base.get_modal_context()
    }
}

impl Controller for CustomGameSettings {
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

    fn on_initialized(&mut self, cx: &mut Cx<'_>) {
        for n in BUILD_ROWS {
            let w = find(cx, n);
            wd(cx, w).vis = Vis::Collapsed;
        }
        let me = cx.w;
        let done = find(cx, "buttonDone");
        cx.visit::<ButtonMain, _>(done, |b, _| b.on_click.add(Handler { owner: me, id: H_DONE }));
        let close = find(cx, "buttonClose");
        cx.visit::<IconButton, _>(close, |b, _| b.on_click.add(Handler { owner: me, id: H_CLOSE }));
    }

    fn destruct(&mut self, cx: &mut Cx<'_>) {
        if self.page.art.is_some() {
            cx.view.release_pictures(&self.base.modal_id);
        }
    }

    fn handle(&mut self, cx: &mut Cx<'_>, id: u32, arg: &dyn Any) {
        match id {
            H_DONE | H_CLOSE => self.request_dismiss(cx, "", false),
            id if id >= H_ROW => {
                let Some(row) = self.menus.get((id - H_ROW) as usize) else { return };
                let Some((i, _)) = arg.downcast_ref::<(i32, String)>() else { return };
                let i = *i;
                let leader = self.view.is_some_and(|v| v.leader);
                if leader
                    && i >= 0
                    && let Some(&value) = row.values.get(i as usize)
                {
                    let key = row.key;
                    cx.emit(self, |c| &c.on_option, &(key, value));
                }
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
    fn the_experimental_page_lists_every_mutator() {
        let p = experimental_page();
        assert_eq!(p.rows[1].values.len(), mvs_protocol::lobby_rules::MUTATOR_CHOICES);
        assert_eq!(p.art, Some("GameMode_AXHard_BG"));
        assert_eq!(room_page().rows.len(), 2);
    }

    #[test]
    fn option_values_name_the_menu_ids() {
        assert_eq!(option_text(OptionValue::Bool(true)), "true");
        assert_eq!(option_text(OptionValue::Choice(7)), "7");
    }
}
