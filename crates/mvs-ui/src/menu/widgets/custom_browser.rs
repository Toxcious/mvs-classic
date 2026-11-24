//! (the port's own) The custom game browser (modal id `customGames`, from the
//! PLAY screen's Join Lobby card and the home screen's CUSTOM GAMES). The build lists no custom lobbies (its custom
//! games are joined by invitation), so the screen is drawn on the trees of build screens it looks like: the lobby
//! screen's page (WBP_CustomGame: its header, the team area's scroll box as the list, its bottom row of buttons; the
//! settings column and the teams left out). The orange button makes a new lobby.
//!
//! The public lobbies of the backends: a search is `OnlineCmd::Browse` and its answer the next frames' `OnlineView.browse`
//! (the program clears it when it runs the command); a row is UI_MatchHistoryEntry's with the lobby's first map's
//! thumbnail before it, and joins as a pasted code does (the carry's pending join, then the custom game page).
//! A search with no lobby shows the port's own line (`status_line`), whatever the reason; the reason goes to the log.

use std::any::Any;

use serde_json::json;

use super::common::{ModalBase, fill_page};
use super::controls::buttons::{ButtonMain, ButtonSecondary};
use super::controls::interactor::{self, batch_set_context};
use super::controls::interactor::{batch_set_scroll_box, set_focused};
use super::controls::{bind, set_id};
use super::controls::{find, slot_z_order, wd};
use super::custom_game::CustomGameMapThumbnail;
use super::custom_game::{TOP_BUTTONS_PAD, TOP_GAP};
use super::dialogs::SetUsername;
use super::nav::{GlobalNav, live};
use super::social::SocialPanel;
use crate::menu::controller::{Controller, Cx, Handler};
use crate::menu::custom_text;
use crate::menu::hydra::picture_name;
use crate::menu::lobby;
use crate::menu::manager::{Manager, Modal};
use crate::menu::online::{BrowseAnswer, BrowseNote, BrowsedGame, OnlineCmd, OnlineLine};
use crate::menu::online_line::{LinePlace, LineWidget};
use crate::menu::own_profile::profile_icon_by_asset;
use crate::menu::pictures::map_pictures;
use crate::menu::view::MenuView;
use crate::slate::{HAlign, Justify, Margin, VAlign, Vis, WidgetId};
use mvs_net::backend::BackendKind;
use mvs_protocol::lobby_listing::ListingState;
use mvs_protocol::lobby_rules::LobbyStyle;

/// The Game strings the screen shows (the menu's text table).
mod texts {
    pub const TITLE: &str = "32F116FE401FD7FA44F89C8092021B79"; // CUSTOM GAMES
    pub const SEARCHING: &str = "EB42CDE54D3512A10657468E2200EF9D"; // Searching
    pub const BACK: &str = "236BBA9A4F869C25AEE457BB914D1B39"; // BACK
    pub const IN_GAME: &str = "1DC94E7D4A7CF17468D703A175F5D949"; // in game
    pub const JOIN_CODE: &str = "D55C2EE74ADE85593595F5AA556BEDB2"; // Join Lobby (the code box's title)
    /// The history row's Free For All mode name (ETeamStyle's; the lobby's Versus is the port's own word).
    pub const MODE_FFA: &str = "0ABB0A3F43B89BD6A9FB1E8029AF6935";
}

const PICTURES: &str = "customGames";
/// The next search after an answer: a lobby set public shows within about this long (the relay lists it at once).
/// Unverified: the port's own number; 12 searches a minute, the relay allows 60, Epic's search limit is not known.
const AUTO_REFRESH_S: f64 = 5.0;
const SEARCH_REST_S: f64 = 2.0;
/// A row's map picture: the lobby grid's thumbnail at this size, the same 16:9.
const MAP_W: f64 = 320.0;
const MAP_H: f64 = 180.0;
const MAP_GAP: f64 = 32.0;
const ROW_GAP: f64 = 16.0;
/// A row's focus frame is this much wider on each side (FOCUS_ROOM less the tree's 5).
const FOCUS_OUT: f64 = 20.0;
/// Between the mode and the map (spaces, the user's choice).
const MODE_SEP: &str = "     ";
/// The row's text sizes, up from the history row's 18 / 26 / 40 for the taller row.
const MODE_FONT: f64 = 26.0;
const NAME_FONT: f64 = 44.0;
const RIGHT_FONT: f64 = 56.0;
const TAG_FONT: f64 = MODE_FONT;
const FADED: f64 = 0.4;
const CREAM: [f64; 4] = [0.89627, 0.973446, 0.871367, 1.0];
const CREAM_FADED: [f64; 4] = [0.89627, 0.973446, 0.871367, 0.4];
/// REFRESH's word: DT_ModifierGlossary row 17's name (the Refresh modifier), the build's only plain "Refresh".
const REFRESH_TABLE: &str = "DT_ModifierGlossary";
const REFRESH_ROW: usize = 16;
/// The lobby page's parts the browser leaves out: the settings column and its divider, the teams, the spectator rows
/// and the loading cover (its sidebar and social panel stay, as on the lobby page).
const LEFT_OUT: [&str; 7] = [
    "SizeBox_4",
    "Image",
    "WBP_CustomGameTeam_ffa",
    "HorizontalBox_408",
    "SpectatorDivider",
    "AddSpectatorsBox",
    "LoadingIndicator",
];
/// The buttons are BACK's size, TOP_W wide.
const TOP_W: f64 = 400.0;
/// Deviation: the port's own words (the build has no such strings): the code box's button and the orange button that
/// makes a lobby.
const CODE_TEXT: &str = "ENTER ROOM CODE";
const CREATE_TEXT: &str = "CREATE LOBBY";
/// A row's focus frame is this much wider on each side, past the map picture and the right-hand text.
const FOCUS_ROOM: f64 = 25.0;
/// The list's share of the page's column (the tree's).
const LIST_FILL: f64 = 3.0;
/// The list's status line (the lobby page's red error line) while there are no rows: in the header's face, cream,
/// centred in the list's space.
const STATUS_FACE: &str = "Display 1";
const STATUS_FONT: f64 = 36.0;
const CREAM_DIM: [f64; 4] = [0.89627, 0.973446, 0.871367, 0.7];

/// The online line (online_line.rs), while something is wrong: bottom left of the page under the header's icon, its
/// foot level with BACK and CREATE LOBBY's.
const LINE_PLACE: LinePlace = LinePlace {
    h_align: HAlign::Left,
    v_align: VAlign::Bottom,
    pad: Margin { l: 220.0, t: 0.0, r: 0.0, b: 49.0 },
    head_size: 26.0,
    advice_size: 18.0,
    wrap: 820.0,
    advice_lines: 0,
};

const H_CREATE: u32 = 1;
const H_BACK: u32 = 2;
const H_REFRESH: u32 = 3;
const H_CODE: u32 = 4;
const H_SCROLL: u32 = 5;
const H_BOX_CODE: u32 = 6;
const H_BOX_DISMISSED: u32 = 7;
/// A row's interactor events: `+ the row's index`.
const H_ROW_CLICK: u32 = 0x1_0000;
const H_ROW_FOCUS: u32 = 0x2_0000;
const R_REST: u32 = 1;
/// The automatic search of generation g: `R_AUTO + g`.
const R_AUTO: u32 = 0x1_0000;

/// A row of the list.
#[derive(Debug)]
struct Row {
    code: String,
    joinable: bool,
    entry: WidgetId,
    interactor: WidgetId,
}

/// Whether a lobby can be joined now, and the right-hand text that says so (rowState).
fn row_state(cx: &Cx<'_>, g: &BrowsedGame) -> (bool, String) {
    let l = &g.listing;
    if l.compat != cx.view.session.online.compat {
        return (false, l.app.clone());
    }
    if l.state == ListingState::Match {
        return (false, cx.view.game_text(texts::IN_GAME).to_string());
    }
    let full = l.players >= l.slots || g.members >= g.max;
    (!full, format!("{}/{}", l.players, l.slots))
}

/// The status line of an answer without lobbies: its reason's words (worded in online_text.rs)
/// with the first letter upper-cased (`toLocaleUpperCase`); '' when the answer has no reason.
fn browse_line(why: &BrowseNote) -> String {
    let text = crate::menu::online_text::browse_text(why);
    let mut cs = text.chars();
    match cs.next() {
        Some(c) => c.to_uppercase().chain(cs).collect(),
        None => text,
    }
}

/// The list's line for an answer without lobbies: the port's own line only; a reason (`browse_line`) goes to the
/// log.
// Deviation: not the build's Game string "None" {0D4B15CE} when a search finds no lobby, nor the search's reason; the
// beta has no custom game browser, so the line is the port's own and the reason is not shown.
fn status_line(why: &BrowseNote) -> String {
    let line = browse_line(why);
    if !line.is_empty() {
        eprintln!("menu: custom games: {line}");
    }
    custom_text::BROWSE_EMPTY.to_string()
}

#[derive(Debug)]
pub struct CustomGameBrowser {
    base: ModalBase,
    refresh_button: Option<WidgetId>,
    code_button: Option<WidgetId>,
    searching: bool,
    searched_at: f64,
    shown: bool,
    rows: Vec<Row>,
    /// The code of the row with the focus, kept across a refresh.
    focused_code: String,
    /// The number of searches asked; the automatic one of an older generation is dropped.
    asked: u32,
    /// The online line, made at the first show.
    online_line: Option<LineWidget>,
}

impl CustomGameBrowser {
    pub fn make(_cx: &mut Cx<'_>) -> Box<dyn Controller> {
        Box::new(CustomGameBrowser {
            base: ModalBase::default(),
            refresh_button: None,
            code_button: None,
            searching: false,
            searched_at: f64::NEG_INFINITY,
            shown: false,
            rows: Vec::new(),
            focused_code: String::new(),
            asked: 0,
            online_line: None,
        })
    }

    /// The lobbies listed now, in row order: (code, joinable).
    pub fn rows(&self) -> Vec<(String, bool)> {
        self.rows.iter().map(|r| (r.code.clone(), r.joinable)).collect()
    }

    fn scroll(cx: &Cx<'_>) -> WidgetId {
        find(cx, "AddPlayerScrollContainer")
    }

    fn button_interactor(cx: &Cx<'_>, w: WidgetId) -> WidgetId {
        cx.view.tree().find_in(w, "UI_Interactor")
    }

    /// A ButtonSecondary like BACK at the page's top, `right` in from its right edge.
    fn add_top_button(&self, cx: &mut Cx<'_>, name: &str, label: &str, right: f64, click: u32) -> WidgetId {
        let w = MenuView::create(cx, "UI_ButtonSecondary");
        let back = find(cx, "CancelCustomGameButton");
        let mut props = cx.view.tree().props(back).clone();
        if let serde_json::Value::Object(o) = &mut props {
            o.insert("buttonLabel".into(), json!({ "CultureInvariantString": label }));
            for (k, v) in o.clone() {
                cx.view.tree_mut().set_prop(w, &k, v);
            }
        }
        {
            let x = wd(cx, w);
            x.name = name.to_string();
            x.h_align = HAlign::Right;
            x.v_align = VAlign::Top;
            x.pad = Margin { r: right, ..TOP_BUTTONS_PAD };
        }
        let overlay = find(cx, "Overlay_128");
        MenuView::add_child(cx, overlay, w, None);
        let me = cx.w;
        cx.visit::<ButtonSecondary, _>(w, |b, cx| {
            b.set_label(cx, label);
            b.on_click.add(Handler { owner: me, id: click });
        });
        w
    }

    /// The list's line while it has no rows (the lobby page's error line), in the list's place: '' hides it.
    fn status(cx: &mut Cx<'_>, line: &str) {
        let t = find(cx, "ErrorInfoText");
        {
            let w = wd(cx, t);
            w.typeface = Some(STATUS_FACE.to_string());
            w.font_size = STATUS_FONT;
            w.color = CREAM_DIM;
            w.justify = Justify::Center;
            w.h_align = HAlign::Center;
            w.v_align = VAlign::Center;
            w.text = line.to_string();
            w.vis = if line.is_empty() { Vis::Collapsed } else { Vis::HitTestInvisible };
        }
        // (the line takes the list's share of the column while it shows)
        let b = find(cx, "Overlay_87");
        wd(cx, b).fill = if line.is_empty() { 0.0 } else { LIST_FILL };
        let s = Self::scroll(cx);
        wd(cx, s).vis = if line.is_empty() { Vis::Visible } else { Vis::Collapsed };
    }

    /// One search, unless one is out or the last one is younger than SEARCH_REST_S; the next automatic one is set
    /// from its answer.
    fn search(&mut self, cx: &mut Cx<'_>) {
        let now = cx.now();
        if !self.shown || self.searching || now - self.searched_at < SEARCH_REST_S {
            return;
        }
        self.searching = true;
        if let Some(r) = self.refresh_button {
            let i = Self::button_interactor(cx, r);
            interactor::set_interactable(cx, i, false);
        }
        if self.rows.is_empty() {
            let searching = cx.view.game_text(texts::SEARCHING).to_string();
            Self::status(cx, &searching);
        }
        self.asked += 1;
        cx.online(OnlineCmd::Browse);
    }

    /// The answer of the search out: drawn, then the rest of the REFRESH button and the next automatic search.
    fn take_answer(&mut self, cx: &mut Cx<'_>) {
        if !self.shown || !self.searching {
            return;
        }
        let Some(a) = cx.view.session.online.browse.clone() else { return };
        self.searching = false;
        self.searched_at = cx.now();
        self.draw(cx, &a);
        let me = cx.w;
        cx.view.delay(SEARCH_REST_S, Handler { owner: me, id: R_REST });
        cx.view.delay(AUTO_REFRESH_S, Handler { owner: me, id: R_AUTO + self.asked });
    }

    /// The rows of an answer, made anew; the focus goes back to the row of the same lobby, else stays where it was
    /// (a row that is gone hands it to BACK).
    fn draw(&mut self, cx: &mut Cx<'_>, a: &BrowseAnswer) {
        let focused = cx.view.manager.get_currently_focused(0);
        let had_row_focus = self.rows.iter().any(|r| Some(r.interactor) == focused);
        for r in std::mem::take(&mut self.rows) {
            MenuView::discard(cx, r.entry);
        }
        let mut states: Vec<(&BrowsedGame, bool, String)> = a
            .games
            .iter()
            .map(|g| {
                let (j, right) = row_state(cx, g);
                (g, j, right)
            })
            .collect();
        // joinable first, then the more humans, then the name (Unverified: localeCompare as a case-folded compare)
        states.sort_by(|x, y| {
            y.1.cmp(&x.1)
                .then(y.0.listing.humans.cmp(&x.0.listing.humans))
                .then_with(|| x.0.listing.name.to_lowercase().cmp(&y.0.listing.name.to_lowercase()))
        });
        let mut pictures: Vec<String> = Vec::new();
        let mut maps: Vec<&str> = Vec::new();
        for (i, (g, joinable, right)) in states.iter().enumerate() {
            let (row, portrait) = self.add_row(cx, i, g, *joinable, right);
            self.rows.push(row);
            pictures.extend(portrait);
            if let Some(m) = g.listing.maps.first().filter(|m| !m.is_empty()) {
                maps.push(m);
            }
        }
        // (the portraits' and maps' pictures, held while the screen is up)
        pictures.extend(map_pictures(cx.view, &maps).into_iter().flatten());
        cx.view.hold_pictures(PICTURES, pictures.iter().map(String::as_str));
        if !self.rows.is_empty() {
            Self::status(cx, "");
        } else {
            Self::status(cx, &status_line(&a.why));
        }
        let ctx = self.base.context.clone();
        self.update_context(cx, &ctx);
        if !had_row_focus {
            return;
        }
        let again = self
            .rows
            .iter()
            .find(|r| r.code == self.focused_code && r.joinable)
            .or_else(|| self.rows.iter().find(|r| r.joinable))
            .map(|r| r.interactor);
        let target = again.unwrap_or_else(|| Self::button_interactor(cx, find(cx, "CancelCustomGameButton")));
        set_focused(cx, target, false);
    }

    /// One row; also the host's icon picture it shows (None: none).
    fn add_row(
        &mut self,
        cx: &mut Cx<'_>,
        index: usize,
        g: &BrowsedGame,
        joinable: bool,
        right: &str,
    ) -> (Row, Option<String>) {
        let l = &g.listing;
        let w = MenuView::create(cx, "UI_MatchHistoryEntry");
        // (before the thumbnail goes in: its own interactor would be found first)
        let interactor = cx.view.tree().find_in(w, "UI_Interactor");
        // the first map's picture before the row's portrait (no map: none)
        let map = l.maps.first().and_then(|m| cx.view.map(m));
        if let Some(m) = &map {
            let t = MenuView::create(cx, "WBP_CustomGameMapThumbnail");
            let data = Some(m.clone());
            cx.visit::<CustomGameMapThumbnail, _>(t, |c, _| c.map_data = data);
            // (the row's interactor takes the pointer and the focus; the thumbnail's own and its tooltip are off)
            {
                let x = wd(cx, t);
                x.vis = Vis::HitTestInvisible;
                x.v_align = VAlign::Center;
                x.pad = Margin { l: 0.0, t: 0.0, r: MAP_GAP, b: 0.0 };
            }
            let bx = cx.view.tree().find_in(t, "SizeBox_1");
            {
                let x = wd(cx, bx);
                x.w_override = MAP_W;
                x.h_override = MAP_H;
            }
            let row = cx.view.tree().find_in(w, "HorizontalBox_0");
            MenuView::add_child(cx, row, t, Some(0));
            let ti = cx.view.tree().find_in(t, "UI_Interactor");
            interactor::set_interactable(cx, ti, false);
        }
        // the host's icon as the lobby's player row draws it (its texture); none (or one this catalogue lacks):
        // none drawn
        let tex = profile_icon_by_asset(cx.view, &l.icon)
            .and_then(|i| picture_name(i.icon.as_deref()?))
            .filter(|t| cx.view.bundle.textures.contains_key(t));
        let portrait = cx.view.tree().find_in(w, "CharacterPortrait");
        {
            let x = wd(cx, portrait);
            x.tex = tex.clone();
            x.mid = None;
            x.vis = if tex.is_some() { Vis::SelfHitTestInvisible } else { Vis::Hidden };
        }
        // (its frame as tall as the icon, centred on the row; no icon: the frame left out too)
        let frame = cx.view.tree().find_in(w, "Overlay_0");
        {
            let x = wd(cx, frame);
            x.v_align = VAlign::Center;
            if tex.is_none() {
                x.vis = Vis::Collapsed;
            }
        }
        let first = map.as_ref().map(|m| m.data.display_name.clone()).unwrap_or_default();
        let more = i64::from(l.map_count) - i64::from(!first.is_empty());
        let mode = match l.style {
            LobbyStyle::Versus => custom_text::VERSUS.to_string(),
            LobbyStyle::FreeForAll => cx.view.game_text(texts::MODE_FFA).to_string(),
        };
        let map_part = format!("{first}{}", if more > 0 { format!(" +{more}") } else { String::new() });
        let line: Vec<String> = [mode, map_part].into_iter().filter(|p| !p.is_empty()).collect();
        let set = |cx: &mut Cx<'_>, name: &str, f: &dyn Fn(&mut crate::slate::Widget)| {
            let n = cx.view.tree().find_in(w, name);
            f(wd(cx, n));
        };
        set(cx, "GameModeText", &|x| {
            x.text = line.join(MODE_SEP);
            x.font_size = MODE_FONT;
        });
        set(cx, "UI_FocusedCommon", &|x| {
            x.pad.l -= FOCUS_OUT;
            x.pad.r -= FOCUS_OUT;
        });
        set(cx, "MapText", &|x| {
            x.text = l.name.clone();
            x.font_size = NAME_FONT;
        });
        set(cx, "VictoryDefeatText", &|x| {
            x.text = right.to_string();
            x.font_size = RIGHT_FONT;
            x.color = if joinable { CREAM } else { CREAM_FADED };
        });
        // the backend's tag: Spectate_VictoryTextHeader, the small line the history row has over its result
        let tag = g.backend.map(|b| match b {
            BackendKind::Relay => custom_text::BACKEND_RELAY,
            BackendKind::Epic => custom_text::BACKEND_EPIC,
            BackendKind::Lan => custom_text::BACKEND_LAN,
        });
        set(cx, "Spectate_VictoryTextHeader", &|x| {
            x.text = tag.unwrap_or_default().to_string();
            x.font_size = TAG_FONT;
            x.color = CREAM_DIM;
            x.vis = if tag.is_some() { Vis::HitTestInvisible } else { Vis::Collapsed };
        });
        let scroll = Self::scroll(cx);
        batch_set_scroll_box(cx, &[interactor], scroll);
        set_id(cx, interactor, &g.code);
        bind(cx, interactor, |s| &mut s.on_focus, H_ROW_FOCUS + index as u32);
        bind(cx, interactor, |s| &mut s.on_click, H_ROW_CLICK + index as u32);
        MenuView::add_child(cx, scroll, w, None);
        wd(cx, w).pad = Margin { l: FOCUS_ROOM, t: 0.0, r: FOCUS_ROOM, b: ROW_GAP };
        if !joinable {
            interactor::set_interactable(cx, interactor, false);
            wd(cx, w).opacity = FADED;
        }
        (Row { code: g.code.clone(), joinable, entry: w, interactor }, tex)
    }

    /// A row's lobby joined as a pasted code is (the lobby screen opens on the carry's pending join).
    fn join(&mut self, cx: &mut Cx<'_>, code: &str) {
        self.request_dismiss(cx, "", false);
        lobby::set_pending_join(Some(code.to_string()));
        Manager::show_modal(cx, "WBP_CustomGame", "customGame", "", "");
    }

    /// ENTER ROOM CODE: the lobby code box over the list; a code entered closes the list and joins.
    fn show_join_box(&mut self, cx: &mut Cx<'_>) {
        let title = cx.view.game_text(texts::JOIN_CODE).to_string();
        let Some(b) = Manager::show_modal(cx, "UI_SetUsername", "joinLobby", &title, "join") else { return };
        let me = cx.w;
        cx.visit::<SetUsername, _>(b, |b, _| b.code_entered.add(Handler { owner: me, id: H_BOX_CODE }));
        // (the box closed without a code: the focus back on its button, the list's context with it)
        cx.view.manager.modal_dismissed.add(Handler { owner: me, id: H_BOX_DISMISSED });
    }

    pub fn update_context(&mut self, cx: &mut Cx<'_>, ctx: &str) {
        self.base.context = ctx.to_string();
        let mut list = vec![
            Self::button_interactor(cx, find(cx, "Btn_StartMatch")),
            Self::button_interactor(cx, find(cx, "CancelCustomGameButton")),
        ];
        list.extend(self.rows.iter().map(|r| r.interactor));
        for b in [self.refresh_button, self.code_button].into_iter().flatten() {
            list.push(Self::button_interactor(cx, b));
        }
        batch_set_context(cx, &list, ctx);
        // (the page's own sidebar and social panel take the context; the other sidebars wait, as on the lobby page)
        let social = find(cx, "UI_SocialPanel_CustomGame");
        cx.visit::<SocialPanel, _>(social, |p, cx| p.update_context(cx, ctx));
        let nav = find(cx, "WBP_GlobalNav_CustomGame");
        cx.visit::<GlobalNav, _>(nav, |n, cx| n.update_context_global_nav(cx, ctx, true, false));
        for other in live(cx, "WBP_GlobalNav").into_iter().filter(|&o| o != nav) {
            cx.visit::<GlobalNav, _>(other, |n, _| n.refresh_context_paused = true);
        }
    }

    /// The online line (the port's own, as the home screen's), shown while the program has its net service.
    fn online_line_follows(&mut self, cx: &mut Cx<'_>) {
        let Some(l) = &mut self.online_line else { return };
        let online = &cx.view.session.online;
        // (online with nothing wrong shows nothing: the line is there only for a problem or a wait)
        let line = online.can_join.then(|| online.line.clone()).filter(|l| *l != OnlineLine::Online);
        l.show(cx, line.as_ref());
    }

    /// CREATE LOBBY: a new lobby, as the home screen's CUSTOM GAME opened it (its select sound).
    fn create_lobby(&mut self, cx: &mut Cx<'_>) {
        let id = self.base.modal_id.clone();
        Manager::dismiss_modal(cx, &id);
        lobby::set_pending_join(None);
        Manager::show_modal(cx, "WBP_CustomGame", "customGame", "", "");
        cx.post("ui_menu_global_select_customgame");
    }
}

impl Modal for CustomGameBrowser {
    fn pre_show(&mut self, cx: &mut Cx<'_>, id: &str) {
        self.base.pre_show(cx, id);
        let me = cx.w;
        cx.view.manager.on_scroll.add(Handler { owner: me, id: H_SCROLL });
        let s = Self::scroll(cx);
        cx.view.manager.register_scrollbox(s);
    }

    /// (after Construct: the buttons' PreConstruct has set their labels from the tree)
    fn post_show(&mut self, cx: &mut Cx<'_>, _instructions: &str, _extra_data: &str) {
        self.shown = true;
        let create = find(cx, "Btn_StartMatch");
        cx.visit::<ButtonMain, _>(create, |b, cx| b.set_label(cx, CREATE_TEXT));
        let ci = Self::button_interactor(cx, create);
        interactor::set_interactable(cx, ci, true);
        let back = find(cx, "CancelCustomGameButton");
        let back_label = cx.view.game_text(texts::BACK).to_string();
        cx.visit::<ButtonSecondary, _>(back, |b, cx| b.set_label(cx, &back_label));
        if self.refresh_button.is_none() {
            let label =
                cx.view.glossary(REFRESH_TABLE).get(REFRESH_ROW).map(|r| r.name.to_uppercase()).unwrap_or_default();
            self.refresh_button = Some(self.add_top_button(cx, "buttonRefresh", &label, TOP_BUTTONS_PAD.r, H_REFRESH));
            self.code_button =
                Some(self.add_top_button(cx, "buttonRoomCode", CODE_TEXT, TOP_BUTTONS_PAD.r + TOP_W + TOP_GAP, H_CODE));
        }
        if self.online_line.is_none() {
            let overlay = find(cx, "Overlay_128");
            self.online_line = Some(LineWidget::make(cx, overlay, None, LINE_PLACE));
        }
        self.online_line_follows(cx);
        let searching = cx.view.game_text(texts::SEARCHING).to_string();
        Self::status(cx, &searching);
        let guid = cx.view.manager.new_guid();
        self.update_context(cx, &guid);
        let bi = Self::button_interactor(cx, back);
        interactor::set_focused(cx, bi, false);
        self.search(cx);
    }

    fn pre_dismiss(&mut self, cx: &mut Cx<'_>) {
        self.base.pre_dismiss(cx);
    }

    fn post_dismiss(&mut self, cx: &mut Cx<'_>) {
        self.shown = false;
        for n in live(cx, "WBP_GlobalNav") {
            cx.visit::<GlobalNav, _>(n, |n, _| n.refresh_context_paused = false);
        }
        let s = Self::scroll(cx);
        cx.view.manager.unregister_scrollbox(s);
    }

    /// An open social panel would take the cancel, as on the lobby page (not ported).
    fn request_dismiss(&mut self, cx: &mut Cx<'_>, _reason: &str, _from_cancel: bool) {
        let id = self.base.modal_id.clone();
        Manager::dismiss_modal(cx, &id);
    }

    fn get_modal_context(&self) -> String {
        self.base.get_modal_context()
    }
}

impl Controller for CustomGameBrowser {
    fn on_initialized(&mut self, cx: &mut Cx<'_>) {
        // (taken out before the page is built: their controllers never construct)
        for n in LEFT_OUT {
            if let Some(w) = cx.view.tree().find_opt(cx.w, n) {
                cx.view.tree_mut().remove(w);
            }
        }
        let title = cx.view.game_text(texts::TITLE).to_string();
        let header = find(cx, "WBP_MenuHeader");
        let t = json!({ "CultureInvariantString": title });
        cx.view.tree_mut().set_prop(header, "PageTitle", t.clone());
        cx.view.tree_mut().set_prop(header, "BackgroundTitle", t);
        let me = cx.w;
        let create = find(cx, "Btn_StartMatch");
        cx.visit::<ButtonMain, _>(create, |b, _| b.on_click.add(Handler { owner: me, id: H_CREATE }));
        let back = find(cx, "CancelCustomGameButton");
        cx.visit::<ButtonSecondary, _>(back, |b, _| b.on_click.add(Handler { owner: me, id: H_BACK }));
        // the list FOCUS_ROOM wider on each side and its rows as far in, so the scroll box's clip leaves the focus
        // frame
        let s = Self::scroll(cx);
        let p = cx.view.tree().widget(s).pad;
        wd(cx, s).pad = Margin { l: p.l - FOCUS_ROOM, r: p.r - FOCUS_ROOM, ..p };
    }

    fn construct(&mut self, cx: &mut Cx<'_>) {
        // (WBP_CustomGame's slot ZOrders, as on the lobby page)
        slot_z_order(cx, &["WBP_GlobalNav_CustomGame", "UI_SocialPanel_CustomGame"], 1);
        let b = find(cx, "SizeBox_242");
        fill_page(cx, b);
    }

    fn tick(&mut self, cx: &mut Cx<'_>, _dt: f64) {
        let b = find(cx, "SizeBox_242");
        fill_page(cx, b);
        self.take_answer(cx);
        self.online_line_follows(cx);
    }

    fn destruct(&mut self, cx: &mut Cx<'_>) {
        self.shown = false;
        let me = cx.w;
        cx.view.manager.on_scroll.remove(Handler { owner: me, id: H_SCROLL });
        cx.view.release_pictures(PICTURES);
    }

    fn resume(&mut self, cx: &mut Cx<'_>, id: u32) {
        if id == R_REST
            && let Some(r) = self.refresh_button
        {
            let i = Self::button_interactor(cx, r);
            interactor::set_interactable(cx, i, true);
        } else if id >= R_AUTO && id - R_AUTO == self.asked {
            self.search(cx);
        }
    }

    fn handle(&mut self, cx: &mut Cx<'_>, id: u32, arg: &dyn Any) {
        match id {
            H_CREATE => self.create_lobby(cx),
            H_BACK => self.request_dismiss(cx, "", false),
            H_REFRESH => self.search(cx),
            H_CODE => self.show_join_box(cx),
            H_BOX_CODE => {
                if let Some(code) = arg.downcast_ref::<String>() {
                    self.join(cx, code);
                }
            }
            H_BOX_DISMISSED => {
                if arg.downcast_ref::<String>().is_some_and(|i| i == "joinLobby") {
                    let me = cx.w;
                    cx.view.manager.modal_dismissed.remove(Handler { owner: me, id: H_BOX_DISMISSED });
                    if self.shown
                        && let Some(c) = self.code_button
                    {
                        let i = Self::button_interactor(cx, c);
                        set_focused(cx, i, false);
                    }
                }
            }
            id if (H_ROW_FOCUS..H_ROW_FOCUS + 0x1_0000).contains(&id) => {
                if let Some(r) = self.rows.get((id - H_ROW_FOCUS) as usize) {
                    self.focused_code = r.code.clone();
                }
            }
            id if (H_ROW_CLICK..H_ROW_CLICK + 0x1_0000).contains(&id) => {
                if let Some(code) = self.rows.get((id - H_ROW_CLICK) as usize).map(|r| r.code.clone()) {
                    self.join(cx, &code);
                }
            }
            // the manager's OnUIScroll(Player, Direction, Amount)
            H_SCROLL => {
                let Some((p, _, amount)) = arg.downcast_ref::<(i32, i32, f64)>() else { return };
                if cx.view.manager.get_current_context(*p) != self.base.context {
                    return;
                }
                let s = Self::scroll(cx);
                crate::menu::scroll::scroll_list(cx.view.tree_mut(), s, *amount);
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
    use mvs_net::service::api::BrowseWhy;

    #[test]
    fn an_empty_answer_shows_only_the_ports_line() {
        assert_eq!(status_line(&BrowseNote::Net(BrowseWhy::All)), custom_text::BROWSE_EMPTY);
        assert_eq!(status_line(&BrowseNote::NoApp), custom_text::BROWSE_EMPTY);
    }
}
