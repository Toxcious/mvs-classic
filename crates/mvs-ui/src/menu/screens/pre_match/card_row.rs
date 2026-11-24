//! (the port's own big matches) The pre-match's row of cards. The build's scene has four fighter points; past four
//! players (`setup::is_big`) the screen shows no line-up and lays the players out as a flat row of cards in player
//! order instead, over a dimmed scene: each card the fighter's portrait (the character select's tile,
//! UI_PM_CharacterItem), the player's name on its team's colour and its perk card (UI_Perks_Loadout, the scene's
//! cards' class), whose perks show from its lock or the reveal on. The row makes room for the perk page while it is
//! up.

use serde_json::{Value, json};

use mvs_view::skins::ffa_team_palette;

use crate::bundle::{Rgba, RosterEntry};
use crate::menu::controller::Cx;
use crate::menu::screens::lab_selectors::{Tile, TileData};
use crate::menu::view::MenuView;
use crate::menu::widgets::common::color_for_index;
use crate::menu::widgets::controls::grid_item::GridItem;
use crate::menu::widgets::controls::{find, wd};
use crate::menu::widgets::lobby_chat::plain;
use crate::menu::widgets::perks::PerksLoadout;
use crate::slate::{HAlign, Margin, VAlign, Vis, WidgetId};

use super::child_widgets::PMCharacterItem;

/// The row's box in the screen's 2560 x 1440 canvas: its sides, its top under the state header and the map's name.
const CANVAS_W: f64 = 2560.0;
const SIDE: f64 = 80.0;
const TOP: f64 = 250.0;
/// The row's left side while the perk page (UI_Perks_Equipper_P0, 620 pixels of 1920) is up.
const LEFT_BESIDE_PAGE: f64 = 860.0;
/// The space between two cards.
const GAP: f64 = 24.0;
/// The portrait's height at most, and its height for its width (the character select's tiles).
const PORTRAIT_H: f64 = 360.0;
const PORTRAIT_ASPECT: f64 = 1.29;
/// The name's bar and its text, and the perk card's box (the card scaled to fit it).
const NAME_H: f64 = 52.0;
const NAME_TEXT: f64 = 30.0;
const LOADOUT_H: f64 = 440.0;
/// The perk card's box for its width, so narrow cards' perks keep apart.
const LOADOUT_ASPECT: f64 = 1.2;
/// The scene behind the row: black at this opacity.
const DIM: f64 = 0.7;

/// A player's card.
#[derive(Debug)]
struct Card {
    /// The player's index in the screen's selections.
    sel: usize,
    /// The card's box, as wide as the row's share.
    cell: WidgetId,
    tile: WidgetId,
    /// The perk card's box.
    perks: WidgetId,
    loadout: WidgetId,
}

/// The row over the screen's canvas.
#[derive(Debug)]
pub struct CardRow {
    root: WidgetId,
    row: WidgetId,
    cards: Vec<Card>,
    left: f64,
}

/// What a card shows of its player.
pub struct CardPlayer<'a> {
    pub sel: usize,
    pub name: &'a str,
    pub character: Option<&'a RosterEntry>,
    pub color: Rgba,
}

impl CardRow {
    /// The row under every other widget of the canvas that holds the scene's cards (`under`: one of them), a card
    /// per player, hidden until `show`.
    pub fn new(cx: &mut Cx<'_>, under: WidgetId, players: &[CardPlayer<'_>]) -> CardRow {
        let canvas = cx.view.tree().widget(under).parent.expect("the scene's cards sit in the screen's canvas");
        let root = plain(cx, "Overlay", "BigMatchCards", json!({}), json!({}), "CanvasPanelSlot");
        {
            let w = wd(cx, root);
            w.anchors = [0.0, 0.0, 1.0, 1.0];
            w.offsets = Margin::default();
            w.vis = Vis::Collapsed;
        }
        let dim = plain(
            cx,
            "Border",
            "BigMatchCardsDim",
            json!({ "Background": {}, "BrushColor": linear([0.0, 0.0, 0.0, DIM]) }),
            json!({}),
            "OverlaySlot",
        );
        {
            let w = wd(cx, dim);
            w.h_align = HAlign::Fill;
            w.v_align = VAlign::Fill;
        }
        cx.view.tree_mut().add(root, dim);
        let row = plain(cx, "HorizontalBox", "BigMatchCardRow", json!({}), json!({}), "OverlaySlot");
        {
            let w = wd(cx, row);
            w.pad = Margin { l: SIDE, t: TOP, r: SIDE, b: 0.0 };
            w.v_align = VAlign::Top;
            w.h_align = HAlign::Fill;
        }
        cx.view.tree_mut().add(root, row);
        MenuView::add_child(cx, canvas, root, Some(0));
        let font = name_font(cx);
        let like = Like::of(cx, under);
        let mut cards = Vec::new();
        for (i, p) in players.iter().enumerate() {
            let last = i + 1 == players.len();
            cards.push(card(cx, row, p, &font, &like, last));
        }
        let mut r = CardRow { root, row, cards, left: SIDE };
        r.fit(cx);
        r
    }

    /// The row shown or hidden.
    pub fn show(&self, cx: &mut Cx<'_>, shown: bool) {
        wd(cx, self.root).vis = if shown { Vis::SelfHitTestInvisible } else { Vis::Collapsed };
    }

    /// The row beside the perk page while it is up, over the whole width when it is not.
    pub fn beside_page(&mut self, cx: &mut Cx<'_>, page: bool) {
        self.left = if page { LEFT_BESIDE_PAGE } else { SIDE };
        wd(cx, self.row).pad.l = self.left;
        self.fit(cx);
    }

    /// The perk card of the player at `sel`.
    pub fn loadout(&self, sel: usize) -> Option<WidgetId> {
        self.cards.iter().find(|c| c.sel == sel).map(|c| c.loadout)
    }

    /// The perk cards with their players.
    pub fn loadouts(&self) -> Vec<(usize, WidgetId)> {
        self.cards.iter().map(|c| (c.sel, c.loadout)).collect()
    }

    /// The cards and their portraits sized to the row's share of the canvas' width.
    fn fit(&mut self, cx: &mut Cx<'_>) {
        let n = self.cards.len().max(1) as f64;
        let width = ((CANVAS_W - self.left - SIDE - GAP * (n - 1.0)) / n).max(1.0);
        let height = (width * PORTRAIT_ASPECT).min(PORTRAIT_H);
        for c in &self.cards {
            wd(cx, c.cell).w_override = width;
            wd(cx, c.perks).h_override = (width * LOADOUT_ASPECT).min(LOADOUT_H);
            cx.visit::<PMCharacterItem, _>(c.tile, |t, cx| t.set_size(cx, width, height));
        }
    }
}

/// A player's card in the row: its column, the portrait, the name, the perk card.
fn card(cx: &mut Cx<'_>, row: WidgetId, p: &CardPlayer<'_>, font: &Value, like: &Like, last: bool) -> Card {
    let cell = plain(
        cx,
        "SizeBox",
        "BigMatchCard",
        json!({}),
        json!({ "Padding": { "Right": if last { 0.0 } else { GAP } } }),
        "HorizontalBoxSlot",
    );
    let col = plain(cx, "VerticalBox", "BigMatchCardColumn", json!({}), json!({}), "SizeBoxSlot");
    cx.view.tree_mut().add(cell, col);
    MenuView::add_child(cx, row, cell, None);
    // the portrait
    let tile = MenuView::create(cx, "UI_PM_CharacterItem");
    wd(cx, tile).h_align = HAlign::Center;
    MenuView::add_child(cx, col, tile, None);
    let data = p.character.cloned().map(TileData::Roster);
    cx.visit::<PMCharacterItem, _>(tile, |t, cx| t.set_data(cx, data));
    // the name on the team's colour
    let size = plain(
        cx,
        "SizeBox",
        "BigMatchCardNameSize",
        json!({ "HeightOverride": NAME_H, "bOverride_HeightOverride": true }),
        json!({}),
        "VerticalBoxSlot",
    );
    let bar = plain(
        cx,
        "Border",
        "BigMatchCardName",
        json!({ "Background": {}, "BrushColor": linear(p.color), "Padding": { "Left": 12, "Right": 12 } }),
        json!({}),
        "SizeBoxSlot",
    );
    wd(cx, bar).v_align = VAlign::Fill;
    let mut f = font.clone();
    f["Size"] = json!(NAME_TEXT);
    let text = plain(
        cx,
        "TextBlock",
        "BigMatchCardNameLabel",
        json!({
            "Text": { "CultureInvariantString": p.name },
            "Font": f,
            "ColorAndOpacity": { "SpecifiedColor": linear([1.0, 1.0, 1.0, 1.0]) },
        }),
        json!({}),
        "ScaleBoxSlot",
    );
    // (a long name shrinks to the card's width)
    let fit = plain(
        cx,
        "ScaleBox",
        "BigMatchCardNameScale",
        json!({ "Stretch": "EStretch::ScaleToFit", "StretchDirection": "EStretchDirection::DownOnly" }),
        json!({ "VerticalAlignment": "VAlign_Center", "HorizontalAlignment": "HAlign_Center" }),
        "BorderSlot",
    );
    cx.view.tree_mut().add(fit, text);
    cx.view.tree_mut().add(bar, fit);
    cx.view.tree_mut().add(size, bar);
    MenuView::add_child(cx, col, size, None);
    // the perk card, scaled into its box
    let boxed = plain(
        cx,
        "SizeBox",
        "BigMatchCardPerksSize",
        json!({ "HeightOverride": LOADOUT_H, "bOverride_HeightOverride": true }),
        json!({ "Padding": { "Top": 12 } }),
        "VerticalBoxSlot",
    );
    let scale = plain(
        cx,
        "ScaleBox",
        "BigMatchCardPerksScale",
        json!({ "Stretch": "EStretch::ScaleToFit", "StretchDirection": "EStretchDirection::DownOnly" }),
        json!({ "HorizontalAlignment": "HAlign_Center", "VerticalAlignment": "VAlign_Top" }),
        "SizeBoxSlot",
    );
    cx.view.tree_mut().add(boxed, scale);
    MenuView::add_child(cx, col, boxed, None);
    let loadout = MenuView::create(cx, "UI_Perks_Loadout");
    like.apply(cx, loadout);
    MenuView::add_child(cx, scale, loadout, None);
    Card { sel: p.sel, cell, tile, perks: boxed, loadout }
}

/// A scene card's settings, which a row's card takes: the instance's properties (read at its Construct) and what
/// its controller read of them when it was made.
struct Like {
    props: Value,
    fields: (f64, f64, bool, bool),
}

impl Like {
    fn of(cx: &mut Cx<'_>, card: WidgetId) -> Like {
        let props = cx.view.tree().props(card).clone();
        let fields = cx
            .visit::<PerksLoadout, _>(card, |l, _| {
                (l.large_perk_size, l.perk_margin, l.show_stacked_callouts, l.hide_tooltips)
            })
            .unwrap_or((200.0, 20.0, false, false));
        Like { props, fields }
    }

    fn apply(&self, cx: &mut Cx<'_>, card: WidgetId) {
        if let Value::Object(m) = &self.props {
            for (k, v) in m {
                cx.view.tree_mut().set_prop(card, k, v.clone());
            }
        }
        let (size, margin, stacked, tooltips) = self.fields;
        cx.visit::<PerksLoadout, _>(card, |l, _| {
            l.large_perk_size = size;
            l.perk_margin = margin;
            l.show_stacked_callouts = stacked;
            l.hide_tooltips = tooltips;
        });
    }
}

/// The name's font: the scene's name plates' (UI_PM_PlayerInfo's nameLabel).
fn name_font(cx: &Cx<'_>) -> Value {
    let plate = find(cx, "UI_PM_PlayerInfo_0");
    let label = cx.view.tree().find_in(plate, "nameLabel");
    cx.view.tree().props(label).get("Font").cloned().unwrap_or_else(
        || json!({ "FontObject": "PFG", "TypefaceFontName": "NormsCond-ExtraBoldItalic", "Size": NAME_TEXT }),
    )
}

/// The name bar's colour of a player on `team` seen from the local player's `mine` (UI_Functions "Color for Team":
/// the own team's colour, the other side's in teams; in free for all the others by their place among the teams, past
/// the build's four the opponent palettes in turn: mvs-view skins.rs `ffa_team_palette`).
pub fn card_color(cx: &Cx<'_>, team_style: u8, mine: u8, team: u8) -> Rgba {
    if team_style != 2 {
        return color_for_index(cx, i32::from(team != mine));
    }
    color_for_index(cx, ffa_team_palette(team, mine) as i32)
}

fn linear(c: Rgba) -> Value {
    json!({ "R": c[0], "G": c[1], "B": c[2], "A": c[3] })
}
