//! (the port's own big matches) The results' list of the players the podium has no place for: the podium stands the
//! winning team's fighters (up to its four places), and past four players the others line up in a row at the top of
//! the screen, each with its name on its team's colour and its stat sheet's lines (UI_EOG_StatItem).
//! The row shows with the stat sheets ("ShowStats").

use serde_json::{Value, json};

use crate::bundle::Rgba;
use crate::menu::controller::Cx;
use crate::menu::view::MenuView;
use crate::menu::widgets::common::color_for_index;
use crate::menu::widgets::controls::wd;
use crate::menu::widgets::end_of_game::EOGStatItem;
use crate::menu::widgets::lobby_chat::plain;
use crate::results::{EndOfGame, stat_rows};
use crate::slate::{HAlign, Margin, VAlign, WidgetId};

/// The podium's places.
pub const PODIUM_PLACES: usize = 4;
/// The row's box in the 2560 x 1440 canvas: its top under the winner's title, its sides clear of the mode
/// label and the toast counter.
const TOP: f64 = 120.0;
const SIDE: f64 = 270.0;
/// An entry's width at most, and the space between two.
const ENTRY_W: f64 = 420.0;
const GAP: f64 = 24.0;
/// The name's bar and its text, and the stat lines' box (the lines scaled down into it).
const NAME_H: f64 = 48.0;
const NAME_TEXT: f64 = 30.0;
const STATS_H: f64 = 90.0;

/// Whether a match's results take the big layout: more players than the podium has places.
pub fn is_big(eog: &EndOfGame) -> bool {
    eog.players.len() > PODIUM_PLACES
}

/// The podium's two lists of a big match from the winners (your own first): the sequence's winner places (two, one in
/// free for all) and, on its losers' places, the winners past them up to the podium's four; the rest of the winners
/// go to the row.
pub fn podium_lists(team_style: u8, winners: &[usize]) -> (Vec<usize>, Vec<usize>, Vec<usize>) {
    let first = if team_style == 2 { 1 } else { 2 };
    let a = winners.len().min(first);
    let b = winners.len().min(PODIUM_PLACES);
    (winners[..a].to_vec(), winners[a..b].to_vec(), winners[b..].to_vec())
}

/// Builds the row of `rest` (record indices in the podium's order) under the progression's steps of `container`
/// (UI_CanvasProgressionContainer).
pub fn show_row(cx: &mut Cx<'_>, container: WidgetId, eog: &EndOfGame, rest: &[usize]) {
    if rest.is_empty() {
        return;
    }
    let overlay = cx.view.tree().find_in(container, "MainOverlay");
    let row = plain(cx, "HorizontalBox", "BigMatchResultsRow", json!({}), json!({}), "OverlaySlot");
    {
        let w = wd(cx, row);
        w.pad = Margin { l: SIDE, t: TOP, r: SIDE, b: 0.0 };
        w.h_align = HAlign::Center;
        w.v_align = VAlign::Top;
    }
    MenuView::add_child(cx, overlay, row, Some(0));
    let n = rest.len() as f64;
    let width = ((2560.0 - 2.0 * SIDE - GAP * (n - 1.0)) / n).min(ENTRY_W);
    let me = &eog.players[eog.me];
    for (k, &i) in rest.iter().enumerate() {
        let last = k + 1 == rest.len();
        let cell = plain(
            cx,
            "SizeBox",
            "BigMatchResultsEntry",
            json!({ "WidthOverride": width, "bOverride_WidthOverride": true }),
            json!({ "Padding": { "Right": if last { 0.0 } else { GAP } } }),
            "HorizontalBoxSlot",
        );
        let col = plain(cx, "VerticalBox", "BigMatchResultsColumn", json!({}), json!({}), "SizeBoxSlot");
        cx.view.tree_mut().add(cell, col);
        MenuView::add_child(cx, row, cell, None);
        // the name on its team's colour (the end-of-game name plate's palette: yours 0, the others 1)
        let p = eog.player(i);
        let name = if eog.local_play { p.info.local_name.clone().unwrap_or(p.username) } else { p.username };
        let your_team = eog.players[i].team_index == me.team_index;
        let colour = color_for_index(cx, if your_team { 0 } else { 1 });
        let size = plain(
            cx,
            "SizeBox",
            "BigMatchResultsNameSize",
            json!({ "HeightOverride": NAME_H, "bOverride_HeightOverride": true }),
            json!({}),
            "VerticalBoxSlot",
        );
        let bar = plain(
            cx,
            "Border",
            "BigMatchResultsName",
            json!({ "Background": {}, "BrushColor": linear(colour), "Padding": { "Left": 12, "Right": 12 } }),
            json!({}),
            "SizeBoxSlot",
        );
        wd(cx, bar).v_align = VAlign::Fill;
        let text = plain(
            cx,
            "TextBlock",
            "BigMatchResultsNameLabel",
            json!({
                "Text": { "CultureInvariantString": name },
                "Font": { "FontObject": "PFG", "TypefaceFontName": "NormsCond-ExtraBoldItalic", "Size": NAME_TEXT },
                "ColorAndOpacity": { "SpecifiedColor": linear([1.0, 1.0, 1.0, 1.0]) },
            }),
            json!({}),
            "ScaleBoxSlot",
        );
        // (a long name shrinks to the entry's width)
        let fit = plain(
            cx,
            "ScaleBox",
            "BigMatchResultsNameScale",
            json!({ "Stretch": "EStretch::ScaleToFit", "StretchDirection": "EStretchDirection::DownOnly" }),
            json!({ "VerticalAlignment": "VAlign_Center", "HorizontalAlignment": "HAlign_Center" }),
            "BorderSlot",
        );
        cx.view.tree_mut().add(fit, text);
        cx.view.tree_mut().add(bar, fit);
        cx.view.tree_mut().add(size, bar);
        MenuView::add_child(cx, col, size, None);
        // the stat sheet's lines side by side
        let ssize = plain(
            cx,
            "SizeBox",
            "BigMatchResultsStatsSize",
            json!({ "HeightOverride": STATS_H, "bOverride_HeightOverride": true }),
            json!({}),
            "VerticalBoxSlot",
        );
        let sscale = plain(
            cx,
            "ScaleBox",
            "BigMatchResultsStatsScale",
            json!({ "Stretch": "EStretch::ScaleToFit", "StretchDirection": "EStretchDirection::DownOnly" }),
            json!({ "HorizontalAlignment": "HAlign_Center", "VerticalAlignment": "VAlign_Top" }),
            "SizeBoxSlot",
        );
        cx.view.tree_mut().add(ssize, sscale);
        let stats = plain(cx, "HorizontalBox", "BigMatchResultsStats", json!({}), json!({}), "ScaleBoxSlot");
        cx.view.tree_mut().add(sscale, stats);
        MenuView::add_child(cx, col, ssize, None);
        for row in stat_rows(eog, i) {
            let w = MenuView::create(cx, "UI_EOG_StatItem");
            wd(cx, w).pad = Margin { l: GAP / 2.0, t: 0.0, r: GAP / 2.0, b: 0.0 };
            MenuView::add_child(cx, stats, w, None);
            let colour = row.highlight.map_or([0.0; 4], |h| color_for_index(cx, h));
            let title = cx.view.game_text(row.title).to_string();
            cx.visit::<EOGStatItem, _>(w, |c, cx| c.init(cx, &title, &row.value, colour));
        }
    }
}

fn linear(c: Rgba) -> Value {
    json!({ "R": c[0], "G": c[1], "B": c[2], "A": c[3] })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The winners split over the sequence's places: two (one in free for all), then the losers' places up to four.
    #[test]
    fn a_big_match_s_winners_fill_the_podium() {
        let w: Vec<usize> = (0..4).collect();
        assert_eq!(podium_lists(1, &w), (vec![0, 1], vec![2, 3], vec![]));
        assert_eq!(podium_lists(1, &w[..3]), (vec![0, 1], vec![2], vec![]));
        assert_eq!(podium_lists(2, &w[..1]), (vec![0], vec![], vec![]));
        let w: Vec<usize> = (0..6).collect();
        assert_eq!(podium_lists(2, &w), (vec![0], vec![1, 2, 3], vec![4, 5]));
    }
}
