//! (dev) `--fake-lobbies`: the custom games list answers with made-up lobbies, no relay asked, to see how the
//! list draws with many rows. Each is joinable on this game's compat unless it is full or in a match; a join fails as
//! any unknown code does.

use mvs_protocol::lobby_listing::{LISTING_V, ListingState, LobbyListing};
use mvs_protocol::lobby_rules::LobbyStyle;
use mvs_ui::bundle::Cosmetics;
use mvs_ui::menu::lobby::loadout::asset_name;
use mvs_ui::menu::online::BrowsedGame;

use super::tables::Tables;

/// The made-up lobbies' count and the profile icons they carry (set once the menu data is in).
#[derive(Clone, Debug, Default)]
pub struct FakeLobbies {
    pub count: u32,
    /// ProfileIconData asset names, as a host's listing carries its icon.
    pub icons: Vec<String>,
}

/// The catalogue's profile icons that have a picture, as asset names, a spread of them: every player has one (the
/// default is given on the first run), so every made-up host has one too.
pub fn icons_of(c: &Cosmetics) -> Vec<String> {
    let all: Vec<&str> = c.profile_icon.iter().filter(|e| e.icon.is_some()).map(|e| asset_name(&e.path)).collect();
    let step = (all.len() / 12).max(1);
    all.iter().step_by(step).map(|s| (*s).to_owned()).collect()
}

/// One row's shape: the lobby's style, players in it (bots included), humans, in a match.
const SHAPES: [(LobbyStyle, u32, u32, bool); 10] = [
    (LobbyStyle::Versus, 1, 1, false),
    (LobbyStyle::Versus, 2, 1, false),
    (LobbyStyle::Versus, 3, 2, false),
    (LobbyStyle::FreeForAll, 2, 2, false),
    (LobbyStyle::Versus, 4, 4, false),
    (LobbyStyle::Versus, 2, 2, true),
    (LobbyStyle::FreeForAll, 3, 1, false),
    (LobbyStyle::Versus, 3, 1, true),
    (LobbyStyle::Versus, 2, 2, false),
    (LobbyStyle::Versus, 4, 3, true),
];

/// `f.count` made-up lobbies: "Player 2" up, their maps taken in turn from the build's map pools.
pub fn listings(f: &FakeLobbies, compat: &str, tables: &Tables) -> Vec<BrowsedGame> {
    let mut maps: Vec<&String> = tables.maps.values().flatten().collect();
    maps.sort();
    maps.dedup();
    (0..f.count as usize)
        .map(|i| {
            let (style, players, humans, in_match) = SHAPES[i % SHAPES.len()];
            let slots = 4;
            let map = maps.get((i * 3) % maps.len().max(1)).map(|m| (*m).clone());
            let listing = LobbyListing {
                v: LISTING_V,
                name: format!("Player {}", i + 2),
                icon: f.icons.get((i * 5) % f.icons.len().max(1)).cloned().unwrap_or_default(),
                compat: compat.to_owned(),
                app: String::new(),
                style,
                map_count: u32::from(map.is_some()),
                maps: map.into_iter().collect(),
                players: players.min(slots),
                humans: humans.min(slots),
                slots,
                state: if in_match { ListingState::Match } else { ListingState::Lobby },
                time: 420,
                ringouts: 4,
                hazards: true,
            };
            BrowsedGame { code: format!("FAKE{i:02}"), backend: None, members: humans, max: 4, listing }
        })
        .collect()
}
