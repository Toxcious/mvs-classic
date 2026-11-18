//! A player's cosmetics loadout on its way into the match and to the other machines: its shape
//! and how a sent one is read. The bots' draw is the game's.
//!
//! What the build moves, and the port's stand-in for each:
//! - the ring-out VFX: PFGPlayerState LoadStoredRingOutAssetData (@1231, the owning client's global prefs) ->
//!   CreatePlayerData / InitPlayerOnServer -> OnServerMatchDataReady to every machine. Sim-relevant (its
//!   CameraHoldTime), so a per-player name in `MatchPlayerWire::ring_out`, in match.start's setup.
//! - the taunts: the server's MatchManager.GetMatchPlayerTaunts (`server_data.RegisteredPlayers.<id>` fields, the
//!   slot order the server sends) -> Comp_Pawn_TauntManager.LoadMatchTaunts. The port has no server-held profile:
//!   each game sends its own CharacterPreferences[character].TauntDatas with its pick, and the relay puts them in
//!   the setup.
//! - banner and stat-tracker badges: the client's select_character_and_skin request writes StatBadgeSlug / Count
//!   (and the 2 / 3 pairs), each count computed on the client (FUN_14106dc80 -> GetStatTrackerCount); the banner
//!   comes back to every machine as the server's echo of the player's GlobalEquippables.Banner. The game sends both
//!   with its pick; the relay hands them to the peers' pre-match screens.
//! - the profile icon: server-held in the build (set_profile_icon; no local store). The port keeps it in its
//!   preferences store and sends it the same way.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::online::{BadgeRef, CosmeticRef, PlayerCosmetics};
use crate::setup::{InputPrefsWire, MatchPlayerWire, int_in, read_input_prefs};

/// What a game sends with its pick: the pre-match part (PlayerCosmetics), its taunt slots 0-3 for the character it
/// picks (TauntData / EmoteData asset names, '' an empty slot), its RingOutVfxData asset name ('' none: ROV_Default)
/// and its input preferences. The keys are in the order readLoadout builds them.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NetLoadout {
    pub taunts: Vec<String>,
    pub ring_out: String,
    pub banner: CosmeticRef,
    pub badges: Vec<BadgeRef>,
    pub icon: CosmeticRef,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub input: Option<InputPrefsWire>,
}

pub const TAUNT_SLOTS: usize = 4;
pub const BADGE_SLOTS: usize = 3;

/// The names every JavaScript object has (`v in {}`): a name a game sends is refused when it is one.
pub(crate) fn in_object(s: &str) -> bool {
    matches!(
        s,
        "constructor"
            | "__proto__"
            | "__defineGetter__"
            | "__defineSetter__"
            | "__lookupGetter__"
            | "__lookupSetter__"
            | "hasOwnProperty"
            | "isPrototypeOf"
            | "propertyIsEnumerable"
            | "toLocaleString"
            | "toString"
            | "valueOf"
    )
}

/// An asset's name as a game sends it: letters, digits, underscores and '-' (ProfileIcon_AS_RAM_Morty_Infini-Morty),
/// at most 96 (the longest cosmetic's is 66; a longer one is refused, never cut), and no name every object has;
/// anything else is ''.
pub(crate) fn asset_name(v: Option<&Value>) -> String {
    match v.and_then(Value::as_str) {
        Some(s)
            if s.len() <= 96
                && s.bytes().all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-'))
                && !in_object(s) =>
        {
            s.to_owned()
        }
        _ => String::new(),
    }
}

/// A Hydra slug: letters, digits, '_', '-' and '.', at most 64, no object member's name.
fn slug_of(v: Option<&Value>) -> String {
    match v.and_then(Value::as_str) {
        Some(s)
            if s.len() <= 64
                && s.bytes().all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-' | b'.'))
                && !in_object(s) =>
        {
            s.to_owned()
        }
        _ => String::new(),
    }
}

fn ref_of(v: Option<&Value>) -> CosmeticRef {
    let r = v.filter(|v| v.is_object());
    CosmeticRef { slug: slug_of(r.and_then(|r| r.get("slug"))), asset: asset_name(r.and_then(|r| r.get("asset"))) }
}

fn count_of(v: Option<&Value>) -> u32 {
    v.and_then(|v| int_in(v, 0.0, f64::MAX)).map_or(0, |n| n.min(0x7fff_ffff))
}

/// A loadout as a game sent it, cleaned: None when there is none (readLoadout).
pub fn read_loadout(v: &Value) -> Option<NetLoadout> {
    let m = v.as_object()?;
    let taunts = m.get("taunts").and_then(Value::as_array).map_or(&[][..], Vec::as_slice);
    let badges = m.get("badges").and_then(Value::as_array).map_or(&[][..], Vec::as_slice);
    let input = m.get("input").and_then(read_input_prefs);
    Some(NetLoadout {
        taunts: (0..TAUNT_SLOTS).map(|i| asset_name(taunts.get(i))).collect(),
        ring_out: asset_name(m.get("ringOut")),
        banner: ref_of(m.get("banner")),
        badges: (0..BADGE_SLOTS)
            .map(|i| {
                let r = ref_of(badges.get(i));
                BadgeRef { slug: r.slug, asset: r.asset, count: count_of(badges.get(i).and_then(|b| b.get("count"))) }
            })
            .collect(),
        icon: ref_of(m.get("icon")),
        input,
    })
}

/// The pre-match part of a loadout, as sent to the peers (cosmeticsOf).
pub fn cosmetics_of(l: &NetLoadout) -> PlayerCosmetics {
    PlayerCosmetics { banner: l.banner.clone(), badges: l.badges.clone(), icon: l.icon.clone() }
}

/// A set-up player with its taunts, ring-out, banner, skin and input preferences, each only when it names something:
/// no taunt in any slot, no ring-out and no banner leave the fields absent, as every set-up before the cosmetics (the
/// same state and the same hash). The banner (its BannerData slug) is the podium's (BP_Banner_PM @3575), which no
/// sim code reads; the skin (its SkinData name) the same. The input preferences only for a human, absent when every
/// one is the default (withCosmetics).
pub fn with_cosmetics(
    p: &MatchPlayerWire,
    taunts: Option<&[String]>,
    ring_out: Option<&str>,
    banner: Option<&str>,
    skin: Option<&str>,
    input: Option<&InputPrefsWire>,
) -> MatchPlayerWire {
    let mut out = p.clone();
    if let Some(t) = taunts.filter(|t| t.iter().any(|x| !x.is_empty())) {
        out.taunts = Some((0..TAUNT_SLOTS).map(|i| t.get(i).cloned().unwrap_or_default()).collect());
    }
    let named = |s: Option<&str>| s.filter(|s| !s.is_empty()).map(str::to_owned);
    if let Some(r) = named(ring_out) {
        out.ring_out = Some(r);
    }
    if let Some(b) = named(banner) {
        out.banner = Some(b);
    }
    if let Some(s) = named(skin) {
        out.skin = Some(s);
    }
    if !p.bot
        && let Some(prefs) = input.and_then(|i| serde_json::to_value(i).ok()).as_ref().and_then(read_input_prefs)
    {
        out.input = Some(prefs);
    }
    out
}
