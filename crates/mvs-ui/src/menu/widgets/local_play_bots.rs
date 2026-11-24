//! (the port's own big matches) Local Play's bot seats: the seats past the pads' four (`LOCAL_PLAY_PADS`), up to
//! `LOCAL_PLAY_MAX`, each a bot with its fighter, skin and difficulty, in a row under the character grid. A seat is
//! a custom lobby's bot row (WBP_CustomGamePlayer: its fighter picked through the Lab's selectors, its difficulty
//! drop, its remove button) under a header in the Local Play slots' style ("P5: Team 1" on the team's colour); the
//! row's empty slot after them adds one. Shown in Teams and Free For All; the seats are the preferences'
//! (`LocalPlaySettings::players`), written as they change.

use mvs_protocol::lobby_rules::{LobbyPlayer, bot_name};
use mvs_settings::{LOCAL_PLAY_MAX, LOCAL_PLAY_PADS, LocalPlayPlayer, LocalPlaySettings};
use mvs_view::skins::team_index_palette;
use serde_json::{Value, json};

use super::common::color_for_index;
use super::controls::{find, wd};
use super::custom_game::{CustomGamePlayer, Facts};
use super::lobby_chat::plain;
use crate::bundle::Rgba;
use crate::menu::controller::{Cx, Handler};
use crate::menu::lobby::loadout::{picked_skin, skin_by_name};
use crate::menu::view::MenuView;
use crate::slate::{HAlign, Margin, VAlign, Vis, WidgetId};

/// The handler ids the seats' rows send to the page (`LocalPlay::handle`): AddBotDispatch (character, skin),
/// OnBotSettingsChanged (account, character, difficulty, skin) and KickPlayerClicked (account).
pub const B_ADD: u32 = 200;
pub const B_SETTINGS: u32 = 201;
pub const B_REMOVE: u32 = 202;

/// The pictures the rows show (their bots' icons).
const PICTURES: &str = "localPlay:bots";
/// The row's top in the page's units: under the character grid, over the slots' panels (which grow over it in their
/// later steps).
const TOP: f64 = 718.0;
/// The space between two seats.
const GAP: f64 = 20.0;
/// The header's height and its text's size (the slots' header bar, smaller), and a seat's row height: the three
/// fit between the grid and the slots' panels.
const HEADER_H: f64 = 32.0;
const HEADER_TEXT: f64 = 20.0;
const ROW_H: f64 = 118.0;
/// An empty seat's header: the team's colour at this opacity.
const EMPTY_ALPHA: f64 = 0.35;

/// A row account id of seat `i`.
fn account(i: usize) -> String {
    format!("seat{i}")
}

/// The seat of a row's account id.
pub fn seat_of(account: &str) -> Option<usize> {
    account.strip_prefix("seat")?.parse().ok().filter(|i| (LOCAL_PLAY_PADS..LOCAL_PLAY_MAX).contains(i))
}

/// The seats' row: made once over the page's tree, its columns made again on every change (`refresh`).
#[derive(Debug)]
pub struct BotSeats {
    root: WidgetId,
    columns: Vec<WidgetId>,
    /// The seats' rows and the empty slot.
    rows: Vec<WidgetId>,
}

impl BotSeats {
    /// The row in the page's Overlay, before (under) the slots' grid `players`, as wide as it.
    pub fn new(cx: &mut Cx<'_>, players: WidgetId) -> BotSeats {
        let parent = cx.view.tree().widget(players).parent.expect("the slots' grid has a parent");
        let at = cx.view.tree().widget(parent).children.iter().position(|&c| c == players);
        let pad = cx.view.tree().widget(players).pad;
        let root = plain(cx, "HorizontalBox", "LocalPlayBots", json!({}), json!({}), "OverlaySlot");
        {
            let w = wd(cx, root);
            w.pad = Margin { l: pad.l, t: TOP, r: pad.r, b: 0.0 };
            w.v_align = VAlign::Top;
            w.h_align = HAlign::Fill;
        }
        MenuView::add_child(cx, parent, root, at);
        BotSeats { root, columns: Vec::new(), rows: Vec::new() }
    }

    /// The row shown (Teams and Free For All) or collapsed, at `opacity` (the character grid's).
    pub fn show(&self, cx: &mut Cx<'_>, shown: bool, opacity: f64) {
        let w = wd(cx, self.root);
        w.vis = if shown { Vis::SelfHitTestInvisible } else { Vis::Collapsed };
        w.opacity = opacity;
    }

    /// The rows' interactors in `ctx` (the page's Update Context).
    pub fn update_context(&self, cx: &mut Cx<'_>, ctx: &str) {
        for &r in &self.rows {
            cx.visit::<CustomGamePlayer, _>(r, |c, cx| c.update_context(cx, ctx));
        }
    }

    /// The columns made again from the preferences' seats: one per bot seat, then an empty one that adds the next
    /// while there is room; the rows' interactors in `ctx`.
    pub fn refresh(&mut self, cx: &mut Cx<'_>, ctx: &str) {
        for c in std::mem::take(&mut self.columns) {
            MenuView::discard(cx, c);
        }
        self.rows.clear();
        let prefs = cx.prefs.local_play.clone();
        let seats = prefs.players.len().clamp(LOCAL_PLAY_PADS, LOCAL_PLAY_MAX);
        let mut icons = Vec::new();
        for i in LOCAL_PLAY_PADS..seats {
            let p = seat_player(cx, &prefs, i);
            if let Some(f) = cx.view.fighters().get(&p.character) {
                icons.push(skin_by_name(Some(f), &p.skin).map_or(&f.skin.icon, |s| &s.icon).clone());
            }
            let row = MenuView::create(cx, "WBP_CustomGamePlayer");
            self.column(cx, &prefs, i, row, false);
            let facts = Facts { am_leader: true, ..Facts::default() };
            cx.visit::<CustomGamePlayer, _>(row, |c, cx| {
                c.update_data(cx, p, "", true, false, facts);
                c.update_context(cx, ctx);
            });
            let me = cx.w;
            cx.visit::<CustomGamePlayer, _>(row, |c, _| {
                c.on_bot_settings_changed.add(Handler { owner: me, id: B_SETTINGS });
                c.kick_player_clicked.add(Handler { owner: me, id: B_REMOVE });
            });
        }
        if seats < LOCAL_PLAY_MAX {
            let slot = MenuView::create(cx, "WBP_CustomGamePlayer");
            cx.visit::<CustomGamePlayer, _>(slot, |c, _| c.slot_is_empty = true);
            let invite = cx.view.tree().find_in(slot, "btn_InvitePlayer");
            wd(cx, invite).vis = Vis::Collapsed;
            self.column(cx, &prefs, seats, slot, true);
            let me = cx.w;
            cx.visit::<CustomGamePlayer, _>(slot, |c, cx| {
                c.update_appearance(cx);
                c.update_context(cx, ctx);
                c.add_bot_dispatch.add(Handler { owner: me, id: B_ADD });
            });
        }
        // the columns keep a quarter of the row each
        let used = self.columns.len();
        if used < LOCAL_PLAY_MAX - LOCAL_PLAY_PADS {
            let rest = (LOCAL_PLAY_MAX - LOCAL_PLAY_PADS - used) as f64;
            let s = plain(
                cx,
                "Spacer",
                "LocalPlayBotsRest",
                json!({}),
                json!({ "Size": { "SizeRule": "ESlateSizeRule::Fill", "Value": rest } }),
                "HorizontalBoxSlot",
            );
            MenuView::add_child(cx, self.root, s, None);
            self.columns.push(s);
        }
        icons.retain(|n| !n.is_empty());
        cx.view.hold_pictures(PICTURES, icons.iter().map(String::as_str));
    }

    /// Seat `i`'s column: its header over `row` (`empty`: the slot that adds it, the header dimmed).
    fn column(&mut self, cx: &mut Cx<'_>, prefs: &LocalPlaySettings, i: usize, row: WidgetId, empty: bool) {
        let col = plain(
            cx,
            "VerticalBox",
            "LocalPlayBotSeat",
            json!({}),
            json!({ "Size": { "SizeRule": "ESlateSizeRule::Fill", "Value": 1.0 }, "Padding": { "Right": GAP } }),
            "HorizontalBoxSlot",
        );
        let team = prefs.seat_team(i);
        let mut color = seat_color(cx, team);
        if empty {
            color[3] *= EMPTY_ALPHA;
        }
        let mut font = header_font(cx);
        font["Size"] = json!(HEADER_TEXT);
        let head = plain(
            cx,
            "Border",
            "LocalPlayBotHeader",
            json!({ "Background": {}, "BrushColor": linear(color), "Padding": { "Left": 16, "Right": 16 } }),
            json!({}),
            "VerticalBoxSlot",
        );
        let text = plain(
            cx,
            "TextBlock",
            "LocalPlayBotHeaderLabel",
            json!({
                "Text": { "CultureInvariantString": format!("P{}: Team {}", i + 1, u32::from(team) + 1) },
                "Font": font,
                "ColorAndOpacity": { "SpecifiedColor": linear([1.0, 1.0, 1.0, if empty { 0.6 } else { 1.0 }]) },
            }),
            json!({ "VerticalAlignment": "VAlign_Center", "HorizontalAlignment": "HAlign_Center" }),
            "BorderSlot",
        );
        cx.view.tree_mut().add(head, text);
        let size = plain(
            cx,
            "SizeBox",
            "LocalPlayBotHeaderSize",
            json!({ "HeightOverride": HEADER_H, "bOverride_HeightOverride": true }),
            json!({}),
            "VerticalBoxSlot",
        );
        wd(cx, head).v_align = VAlign::Fill;
        cx.view.tree_mut().add(size, head);
        cx.view.tree_mut().add(col, size);
        let sb = cx.view.tree().find_in(row, "SizeBox_0");
        wd(cx, sb).h_override = ROW_H;
        self.rows.push(row);
        MenuView::add_child(cx, col, row, None);
        MenuView::add_child(cx, self.root, col, None);
        self.columns.push(col);
    }

    /// The pictures go with the page.
    pub fn release(cx: &mut Cx<'_>) {
        cx.view.release_pictures(PICTURES);
    }
}

/// The slots' header text's font (UI_LP_Player0's headerLabel), so the seats' headers read as theirs.
fn header_font(cx: &Cx<'_>) -> Value {
    let slot = find(cx, "UI_LP_Player0");
    let label = cx.view.tree().find_in(slot, "headerLabel");
    cx.view
        .tree()
        .props(label)
        .get("Font")
        .cloned()
        .unwrap_or_else(|| json!({ "FontObject": "PFG", "TypefaceFontName": "NormsCond-ExtraBoldItalic", "Size": 24 }))
}

/// The header's colour of a seat on `team`: the Local Play slots' team colours (Ally, Enemy, Team3, Team4).
/// A team past the build's four takes the opponent palettes in turn (mvs-view skins.rs `team_index_palette`).
fn seat_color(cx: &Cx<'_>, team: u8) -> Rgba {
    color_for_index(cx, team_index_palette(team) as i32)
}

fn linear(c: Rgba) -> Value {
    json!({ "R": c[0], "G": c[1], "B": c[2], "A": c[3] })
}

/// Seat `i`'s bot as a lobby row reads it: its fighter (the port's name, '' none), its skin's SkinData name ('' the
/// default) and its difficulty.
fn seat_player(cx: &Cx<'_>, prefs: &LocalPlaySettings, i: usize) -> LobbyPlayer {
    let no_seat = LocalPlayPlayer::default();
    let s = prefs.players.get(i).unwrap_or(&no_seat);
    let fighter = cx
        .view
        .roster()
        .iter()
        .find(|r| !s.character_slug.is_empty() && r.slug == s.character_slug)
        .and_then(|r| r.fighter.clone())
        .unwrap_or_default();
    let fi = cx.view.fighters().get(&fighter);
    let skin = fi
        .and_then(|f| {
            f.skins.iter().find(|k| !s.skin_reference.is_empty() && k.path.eq_ignore_ascii_case(&s.skin_reference))
        })
        .map_or(String::new(), |k| picked_skin(fi, &k.name));
    LobbyPlayer {
        account_id: account(i),
        username: bot_name(&fighter),
        is_bot: true,
        character: fighter,
        skin,
        difficulty: i32::from(s.bot_difficulty),
        icon: String::new(),
    }
}

/// The seat preferences of a bot of `fighter` (the port's name) in `skin` (a SkinData name, '' the default) at
/// `difficulty`: the character's slug, the skin's slug and path.
pub fn set_seat(cx: &mut Cx<'_>, seat: &mut LocalPlayPlayer, fighter: &str, skin: &str, difficulty: i32) {
    let slug = cx.view.roster().iter().find(|r| r.fighter.as_deref() == Some(fighter)).map(|r| r.slug.clone());
    seat.character_slug = slug.unwrap_or_default();
    let k = cx.view.fighters().get(fighter).and_then(|f| skin_by_name(Some(f), skin)).filter(|_| !skin.is_empty());
    seat.skin_slug = k.map(|k| k.slug.clone()).unwrap_or_default();
    seat.skin_reference = k.map(|k| k.path.clone()).unwrap_or_default();
    seat.bot_difficulty = u8::try_from(difficulty.clamp(2, 6)).unwrap_or(4);
}
