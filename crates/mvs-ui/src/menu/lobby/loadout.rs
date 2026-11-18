//! This machine's loadout from the Hydra preferences, as the match set-ups take it, with the bot's random cosmetics
//! and its perks.
//!
//! Not ported: the profile icon of a net loadout (online).

use mvs_settings::HydraSettings;
use mvs_sim::MatchSetup;
use mvs_sim::setup::{InputPrefs, MatchPlayer};
use mvs_sim::world::DEFAULT_CHARACTER;

use crate::bundle::{Cosmetics, FighterInfo, MenuData, RosterEntry, SkinInfo};

/// An asset's name as the sim keys it: the basename of its path ("/Game/.../ROV_Default.ROV_Default" ->
/// "ROV_Default").
pub fn asset_name(path: &str) -> &str {
    let tail = path.rsplit('/').next().unwrap_or(path);
    tail.split('.').next().unwrap_or(tail)
}

/// A character's taunt slots 0-3 from the preferences (asset names, '' an empty slot); `character_slug`: its
/// HydraCharacterData slug (the CharacterPreferences key).
pub fn local_taunts(hydra: &HydraSettings, character_slug: Option<&str>) -> [String; 4] {
    match character_slug {
        Some(s) if !s.is_empty() => hydra
            .character_taunts(s)
            .map(|r| if r.slug.is_empty() { String::new() } else { asset_name(&r.path).to_string() }),
        _ => Default::default(),
    }
}

/// GlobalEquippables.RingOutVfx's asset name ('' none: LoadStoredRingOutAssetData's ROV_Default fallback).
pub fn local_ring_out(hydra: &HydraSettings) -> String {
    let r = &hydra.ring_out_vfx;
    if r.slug.is_empty() { String::new() } else { asset_name(&r.path).to_string() }
}

/// GlobalEquippables.Banner's slug ('' none): the local player's banner.
pub fn local_banner(hydra: &HydraSettings) -> String {
    hydra.banner.slug.clone()
}

/// The ring-out asset of a ring-out slug (PGI_GlobalData GetAssetFromLookUpTable, LocalPlayFacilitator
/// SetPlayerSelections @285-479). The cosmetics catalogue is not in the port's menu data yet: the Hydra
/// preferences' own ring-out is the one slug it can resolve; a slug found in neither is not found and the player
/// keeps ROV_Default.
/// Unverified: a Local Play ring-out pick of another slug draws ROV_Default until the catalogue is read.
pub fn ring_out_asset_of(hydra: &HydraSettings, slug: &str) -> String {
    if !slug.is_empty() && hydra.ring_out_vfx.slug == slug { local_ring_out(hydra) } else { String::new() }
}

/// A character's HydraCharacterData slug by the port's name (the roster's).
pub fn character_slug<'a>(roster: &'a [RosterEntry], character: &str) -> Option<&'a str> {
    roster.iter().find(|r| r.fighter.as_deref() == Some(character)).map(|r| r.slug.as_str())
}

/// withCosmetics on one player: the taunts when any is set, the ring-out and the banner when
/// not ''.
fn with_cosmetics(p: &mut MatchPlayer, taunts: [String; 4], ring_out: String, banner: String) {
    if taunts.iter().any(|t| !t.is_empty()) {
        p.taunts = taunts.to_vec();
    }
    if !ring_out.is_empty() {
        p.ring_out = Some(ring_out);
    }
    if !banner.is_empty() {
        p.banner = Some(banner);
    }
}

/// An offline set-up with this machine's loadout: every player's taunts by its character, `ring_out(p, i)` the
/// player's ring-out ('' none: the local player's own, a Local Play slot's ringoutSlug, a bot's rolled one) and
/// `banner(p, i)` its BannerData slug for the podium, `input(p, i)` the local player whose input preferences the
/// player takes (localInputPrefs: `prefs::sim_input_prefs`; None the defaults, as for a bot).
pub fn with_local_loadout(
    setup: &mut MatchSetup,
    hydra: &HydraSettings,
    roster: &[RosterEntry],
    ring_out: &mut dyn FnMut(&MatchPlayer, usize) -> String,
    banner: &mut dyn FnMut(&MatchPlayer, usize) -> String,
    input: &mut dyn FnMut(&MatchPlayer, usize) -> Option<InputPrefs>,
) {
    for (i, p) in setup.players.iter_mut().enumerate() {
        let slug = character_slug(roster, p.character.as_deref().unwrap_or(DEFAULT_CHARACTER));
        let taunts = local_taunts(hydra, slug);
        let r = ring_out(p, i);
        let b = banner(p, i);
        if let Some(prefs) = input(p, i) {
            p.input = prefs;
        }
        with_cosmetics(p, taunts, r, b);
    }
}

/// The set-up as the match takes it: a player's character is one the port has and can draw (`drawable`), or absent
/// (the default one). The lobby's value is a request's text.
pub fn ported_setup(setup: &mut MatchSetup, drawable: &dyn Fn(&str) -> bool) {
    for p in &mut setup.players {
        if p.character.as_deref().is_some_and(|c| !drawable(c)) {
            p.character = None;
        }
    }
}

/// What a bot's random cosmetics are drawn from: what a player can
/// equip as the pickers list it, a function of the cosmetics catalogue. An empty pool (no catalogue) draws none.
#[derive(Clone, Debug, Default)]
pub struct BotPool {
    /// RingOutVfxData asset names.
    pub ring_outs: Vec<String>,
    /// BannerData slugs.
    pub banners: Vec<String>,
    /// ProfileIconData slugs.
    pub icons: Vec<String>,
    /// The badges: StatTrackingBundle slug and its count ranges (each both ends inclusive).
    pub badges: Vec<(String, Vec<(f64, f64)>)>,
}

impl BotPool {
    /// The bot pool. The badges are the badge picker's (UI_Collection @23756-25089: the default bundle
    /// aside, which a plate never shows, and a character's badge only when its fighter is in
    /// FightersInCharacterSelect). A count is drawn from one of the ranges, each as likely: one range per star level
    /// past the first (the thresholds StarLevelThreshholds, or BadgeLevelThreshholds' where the bundle has them:
    /// (t[k-1], t[k]], and past the last one more step of the last's size), so the picture's stars vary; a badge
    /// whose slug names a place (evo2022_16th_place, the Placement badges) counts that place, as its text ("16th
    /// place") reads. A badge with no range is left out.
    /// Deviation: the build gives its bots none.
    pub fn of(cat: &Cosmetics) -> BotPool {
        let mut badges = Vec::new();
        for e in &cat.stat_tracker {
            if e.slug == cat.defaults.stat_tracker {
                continue;
            }
            if e.fighter.as_ref().is_some_and(|f| !f.is_empty() && !cat.select_fighters.contains(f)) {
                continue;
            }
            let mut ranges = Vec::new();
            if let Some(n) = place_of(&e.slug) {
                ranges.push((n, n));
            } else {
                let t: Vec<f64> = match &e.tiers {
                    Some(tiers) => tiers.iter().map(|x| x.0).collect(),
                    None => e.stars.clone().unwrap_or_default(),
                };
                for k in 1..t.len() {
                    if t[k] > t[k - 1] {
                        ranges.push((t[k - 1] + 1.0, t[k]));
                    }
                }
                if let [.., prev, last] = t[..]
                    && last > prev
                {
                    ranges.push((last + 1.0, 2.0 * last - prev));
                }
            }
            if !ranges.is_empty() {
                badges.push((e.slug.clone(), ranges));
            }
        }
        BotPool {
            ring_outs: cat.ringout.iter().map(|e| asset_name(&e.path).to_string()).collect(),
            banners: cat.banner.iter().map(|e| e.slug.clone()).collect(),
            icons: cat.profile_icon.iter().map(|e| e.slug.clone()).collect(),
            badges,
        }
    }
}

/// `/_(\d+)(?:st|nd|rd|th)_place$/`: the place a placement badge's slug names.
fn place_of(slug: &str) -> Option<f64> {
    let head = slug.strip_suffix("_place")?;
    let digits_end = ["st", "nd", "rd", "th"].iter().find_map(|s| head.strip_suffix(s))?;
    let at = digits_end.rfind('_')?;
    let n = &digits_end[at + 1..];
    if n.is_empty() || !n.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    n.parse().ok()
}

/// A stat tracker a plate shows: its slug and count (less the asset name).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct BadgeRef {
    pub slug: String,
    pub count: f64,
}

/// A bot's random cosmetics (the ring-out and the PlayerCosmetics).
/// Deviation: the build's offline modes give a bot none (LobbyGameMode AddBots @2837 gives an online one
/// ROV_OmegaBeam; PGI_CoOpVsAIManager @3558-3880 a bot's Banner null, no badge slugs, StatTrackerCount 100); the
/// port gives each a random ring-out, banner, profile icon and three distinct badges, drawn at the menu, never in
/// the sim.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct BotLoadout {
    pub ring_out: String,
    /// BannerData slug ('' none).
    pub banner: String,
    /// ProfileIconData slug ('' none).
    pub icon: String,
    /// BADGE_SLOTS of them ('' slug: none).
    pub badges: Vec<BadgeRef>,
}

/// The badge slots of a plate.
pub const BADGE_SLOTS: usize = 3;

/// A bot's random cosmetics: the ring-out, the banner, the icon, then each badge
/// slot a badge not drawn yet and a count from one of its ranges. `random()`: [0, 1) as Math.random.
pub fn random_bot_loadout(pool: &BotPool, random: &mut dyn FnMut() -> f64) -> BotLoadout {
    let mut at = |n: usize| -> usize {
        // a draw in [0, 1) times a small count
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss, clippy::cast_precision_loss)]
        let i = (random() * n as f64).floor() as usize;
        i.min(n - 1)
    };
    let ring_out =
        if pool.ring_outs.is_empty() { String::new() } else { pool.ring_outs[at(pool.ring_outs.len())].clone() };
    let banner = if pool.banners.is_empty() { String::new() } else { pool.banners[at(pool.banners.len())].clone() };
    let icon = if pool.icons.is_empty() { String::new() } else { pool.icons[at(pool.icons.len())].clone() };
    let mut left = pool.badges.clone();
    let mut badges = Vec::with_capacity(BADGE_SLOTS);
    for _ in 0..BADGE_SLOTS {
        if left.is_empty() {
            badges.push(BadgeRef::default());
            continue;
        }
        let (slug, ranges) = left.remove(at(left.len()));
        let (lo, hi) = ranges[at(ranges.len())];
        // (the ranges are whole counts)
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss, clippy::cast_precision_loss)]
        let count = lo + at((hi - lo + 1.0) as usize) as f64;
        badges.push(BadgeRef { slug, count });
    }
    BotLoadout { ring_out, banner, icon, badges }
}

// ---------- bot perks (the parts the menus read) ----------

/// The recommended perks of a character (over the menu data):
/// GetRecommendedPerks @0-826, a CharacterSpecific (category 3, @427) entry the signature, the last such wins
/// (@427-553), any other appended to the general list (@757). None: the character has no perk data.
/// Unverified: the table's IsEnabledInDevelopmentEnvironment filter (GetRecommendedPerks @336) is not in the
/// bundle: every recommended perk counts.
pub fn recommended_perks(menu: &MenuData, character: &str) -> Option<(Option<String>, Vec<String>)> {
    let rec = &menu.fighters.get(character)?.perks.as_ref()?.recommended;
    let mut signature = None;
    let mut general = Vec::new();
    for id in rec {
        if menu.perks.get(id).is_some_and(|p| p.category == 3) {
            signature = Some(id.clone());
        } else {
            general.push(id.clone());
        }
    }
    Some((signature, general))
}

/// SimulatePerkUnlockSystem @0-1019: level >= 2 and General[0] -> slot 1,
/// >= 4 and General[1] -> slot 2, >= 7 and General[2] -> slot 3, >= 8 and the signature -> slot 0; '' an empty slot.
pub fn simulate_perk_unlock(menu: &MenuData, character: &str, level: i32) -> Vec<String> {
    let mut slots = vec![String::new(); 4];
    let Some((signature, general)) = recommended_perks(menu, character) else { return slots };
    for (i, min) in [(0, 2), (1, 4), (2, 7)] {
        if level >= min
            && let Some(g) = general.get(i)
        {
            slots[i + 1] = g.clone();
        }
    }
    if level >= 8
        && let Some(s) = signature
    {
        slots[0] = s;
    }
    slots
}

/// GetRandomMasteryLevelForBot @0-779: RandomIntegerInRange(Max(M - 4, 1),
/// Min(M + Min(M, 4), 8)) from the owner's mastery M of the character it selected; the port has no progression, so M
/// is 1 (OWNER_MASTERY) and the level is 1 or 2. `rand(n)`: an integer in 0..n-1.
/// Unverified: the build gives level 0 (no perk) when GetSelectionsForPlayer(ClientGUID) fails at Get Bots; read as
/// succeeding.
pub fn bot_mastery_level(rand: &mut dyn FnMut(i32) -> i32) -> i32 {
    const OWNER_MASTERY: i32 = 1;
    let m = OWNER_MASTERY;
    let (min, max) = ((m - 4).max(1), (m + m.min(4)).min(8));
    // UKismetMathLibrary::RandomIntegerInRange (exec 142f80320): Min + an even draw over Max - Min + 1
    min + rand((max - min).max(0) + 1)
}

// ---------- skins (the parts a set-up reads) ----------

/// Two SkinData names the same skin: FNames compare without case.
pub fn same_skin_name(a: &str, b: &str) -> bool {
    a.eq_ignore_ascii_case(b)
}

/// The tile of a SkinData name ('' or an unknown name: None).
pub fn skin_by_name<'a>(f: Option<&'a FighterInfo>, name: &str) -> Option<&'a SkinInfo> {
    if name.is_empty() {
        return None;
    }
    f?.skins.iter().find(|s| same_skin_name(&s.name, name))
}

/// UI_Functions GetDefaultSkinForCharacter: the IsDefaultSkin one.
/// Deviation: Iron Giant's default is Classic C017_S00; the build's CharacterData marks C017_S01
/// IsDefaultSkin.
pub fn default_skin(f: Option<&FighterInfo>) -> Option<&SkinInfo> {
    f?.skins.iter().find(|s| s.is_default)
}

/// UI_Functions RandomSkin(Character, Only Unlocked false) @0-474: RandomIntegerInRange(0, Length), both ends
/// inclusive, so the index past the end draws a null skin (the default at the spawn) once in Length + 1.
pub fn random_skin<'a>(f: Option<&'a FighterInfo>, random: &mut dyn FnMut() -> f64) -> Option<&'a SkinInfo> {
    let l = f.map_or(&[][..], |f| f.skins.as_slice());
    // (`Math.min(Math.floor(random() * (l.length + 1)), l.length)`: the draw is in [0, 1), cast to an index)
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss, clippy::cast_precision_loss)]
    let i = ((random() * (l.len() + 1) as f64).floor().max(0.0) as usize).min(l.len());
    l.get(i)
}

/// The local player's badges: GlobalEquippables.StatTrackers' slugs, each with
/// its count (statTrackerCount: a slug the catalogue lacks counts 0). `count(item)`: the catalogue item's count.
pub fn local_badges(
    hydra: &HydraSettings,
    cat: &Cosmetics,
    count: &mut dyn FnMut(&crate::bundle::CosmeticItem) -> f64,
) -> Vec<BadgeRef> {
    hydra
        .stat_trackers
        .iter()
        .map(|r| {
            let n = if r.slug.is_empty() {
                0.0
            } else {
                cat.stat_tracker.iter().find(|e| e.slug == r.slug).map_or(0.0, &mut *count)
            };
            BadgeRef { slug: r.slug.clone(), count: n }
        })
        .collect()
}

/// A pick's skin as the match takes it (MatchPlayer.skin): the SkinData name when the character's bundle has it and
/// it is not the port's default, else '' (the default: the port's FighterData default skin plays).
pub fn picked_skin(f: Option<&FighterInfo>, name: &str) -> String {
    match skin_by_name(f, name) {
        Some(s) if !s.is_default => s.name.clone(),
        _ => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mvs_settings::AssetRef;

    #[test]
    fn asset_names_are_the_path_basename() {
        assert_eq!(asset_name("/Game/Cosmetics/ROV_Default.ROV_Default"), "ROV_Default");
        assert_eq!(asset_name("Plain"), "Plain");
    }

    #[test]
    fn a_loadout_goes_into_the_players_that_have_one() {
        let mut h = HydraSettings {
            ring_out_vfx: AssetRef { slug: "r".into(), path: "/Game/ROV_X.ROV_X".into() },
            ..Default::default()
        };
        h.set_character_taunt("shaggy_slug", 1, AssetRef { slug: "t".into(), path: "/Game/T_A.T_A".into() });
        let roster =
            vec![RosterEntry { slug: "shaggy_slug".into(), fighter: Some("shaggy".into()), ..Default::default() }];
        let mut s = MatchSetup {
            players: vec![MatchPlayer::default(), MatchPlayer { bot: true, ..Default::default() }],
            ..Default::default()
        };
        let ro = local_ring_out(&h);
        with_local_loadout(
            &mut s,
            &h,
            &roster,
            &mut |_, i| if i == 0 { ro.clone() } else { String::new() },
            &mut |_, _| String::new(),
            &mut |_, _| None,
        );
        assert_eq!(s.players[0].ring_out.as_deref(), Some("ROV_X"));
        assert_eq!(s.players[0].taunts, vec!["", "T_A", "", ""]);
        assert_eq!(s.players[1].ring_out, None);
        assert_eq!(s.players[1].banner, None);
    }

    #[test]
    fn a_character_the_port_cannot_draw_is_the_default_one() {
        let mut s = MatchSetup {
            players: vec![
                MatchPlayer { character: Some("shaggy".into()), ..Default::default() },
                MatchPlayer { character: Some("batman".into()), ..Default::default() },
            ],
            ..Default::default()
        };
        ported_setup(&mut s, &|c| c == "shaggy");
        assert_eq!(s.players[0].character.as_deref(), Some("shaggy"));
        assert_eq!(s.players[1].character, None);
    }
}
