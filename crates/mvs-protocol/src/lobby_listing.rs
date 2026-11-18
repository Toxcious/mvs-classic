//! A public custom lobby's listing: what the custom game browser shows of it before
//! anyone joins. The host core builds it from its lobby; the net process puts it on the lobby as one text attribute,
//! and a search hands it back as that text. Anyone may list any text, so a reader refuses a listing of another shape
//! whole.

use std::sync::LazyLock;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::chat::js_space;
use crate::loadout::in_object;
use crate::lobby_rules::TEAM_STYLE_IDS;
use crate::names::{BAD_WORDS, NAME_LIMIT, Profanity, name_showable};
use crate::online::compat_of;
use crate::setup::int_in;

/// The listing's shape version: a listing of another one is not shown.
pub const LISTING_V: u32 = 1;
/// The most bytes of a listing's text (EOS_LOBBYMODIFICATION_MAX_ATTRIBUTE_LENGTH, which the relay keeps too).
pub const LISTING_MAX_BYTES: usize = 1000;
/// The most maps a listing names (the rest are counted in `map_count`).
pub const LISTING_MAPS: usize = 8;

/// Lobby: in the lobby screen; Match: its match or that match's pre-match is on (no one may join).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ListingState {
    Lobby,
    Match,
}

/// A listing. `name` / `icon` the hosting player's; `compat` / `app` the host's compatibility id and release;
/// `style` the ETeamStyle; `maps` its first map names and `map_count` all of them; `players` in it (bots too), of
/// which `humans`, of `slots`; `time` seconds, `ringouts` to win, `hazards`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LobbyListing {
    pub v: u32,
    pub name: String,
    pub icon: String,
    pub compat: String,
    pub app: String,
    pub style: u8,
    pub maps: Vec<String>,
    pub map_count: u32,
    pub players: u32,
    pub humans: u32,
    pub slots: u32,
    pub state: ListingState,
    pub time: u32,
    pub ringouts: u32,
    pub hazards: bool,
}

/// The listing's text, its map list cut until it fits LISTING_MAX_BYTES (listingText).
pub fn listing_text(l: &LobbyListing) -> String {
    let mut l = l.clone();
    l.maps.truncate(LISTING_MAPS);
    loop {
        let t = serde_json::to_string(&l).unwrap_or_default();
        if t.len() <= LISTING_MAX_BYTES || l.maps.pop().is_none() {
            return t;
        }
    }
}

/// An asset's name in a listing: `^[A-Za-z0-9_-]{0,96}$`, no name every object has.
fn asset(v: Option<&Value>) -> Option<String> {
    let s = v?.as_str()?;
    (s.len() <= 96 && s.bytes().all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-')) && !in_object(s))
        .then(|| s.to_owned())
}

fn int(v: Option<&Value>, lo: u32, hi: u32) -> Option<u32> {
    int_in(v?, f64::from(lo), f64::from(hi))
}

fn units(s: &str) -> usize {
    s.encode_utf16().count()
}

/// A listed name is read by the rules of a player's name (the port's own): one with a control or format character in
/// it, or a word of the bad-words list, refuses the listing (the host core takes no name with such a word at hello,
/// so a listing that names one was not made by a host core of the port).
static NAME_REFUSED: LazyLock<Profanity> = LazyLock::new(|| Profanity::new(&BAD_WORDS));

/// A listing's text read back; None when it is not a listing of this shape (listingOf).
pub fn listing_of(text: &Value) -> Option<LobbyListing> {
    let text = text.as_str().filter(|t| !t.is_empty() && units(t) <= LISTING_MAX_BYTES)?;
    let parsed: Value = serde_json::from_str(text).ok()?;
    let o = parsed.as_object()?;
    if int(o.get("v"), LISTING_V, LISTING_V) != Some(LISTING_V) {
        return None;
    }
    let name = o.get("name")?.as_str()?;
    let icon = asset(o.get("icon"))?;
    let compat = compat_of(o.get("compat").unwrap_or(&Value::Null));
    if name.trim_matches(js_space).is_empty()
        || units(name) > NAME_LIMIT
        || !name_showable(name)
        || NAME_REFUSED.refuses(name)
        || compat.is_empty()
    {
        return None;
    }
    let app = o.get("app")?.as_str().filter(|a| units(a) <= 32)?;
    let style = int(o.get("style"), 0, u32::try_from(TEAM_STYLE_IDS.len() - 1).ok()?)?;
    let maps = o.get("maps")?.as_array().filter(|m| m.len() <= LISTING_MAPS)?;
    let maps: Vec<String> = maps.iter().map(|m| asset(Some(m))).collect::<Option<_>>()?;
    let map_count = int(o.get("mapCount"), u32::try_from(maps.len()).ok()?, 999)?;
    let slots = int(o.get("slots"), 1, 8)?;
    let players = int(o.get("players"), 0, slots)?;
    let humans = int(o.get("humans"), 0, players)?;
    let state = match o.get("state")?.as_str()? {
        "lobby" => ListingState::Lobby,
        "match" => ListingState::Match,
        _ => return None,
    };
    let time = int(o.get("time"), 0, 3600)?;
    let ringouts = int(o.get("ringouts"), 1, 99)?;
    let hazards = o.get("hazards")?.as_bool()?;
    Some(LobbyListing {
        v: LISTING_V,
        name: name.to_owned(),
        icon,
        compat,
        app: app.to_owned(),
        style: u8::try_from(style).ok()?,
        maps,
        map_count,
        players,
        humans,
        slots,
        state,
        time,
        ringouts,
        hazards,
    })
}
