//! The cosmetics catalogue: what the Collection's choosers list and the preferences name, read from the build's reward
//! data assets and bound with the build's rules.
//! Not a kind of the store: the `ui` kind's menu data reads it for the tiles' pictures ([`super::ui::menu`]), and
//! later readers (the bot pool) take the same catalogue.
//!
//! Per type, the set the build finds by class (GetAssetsByClass; here the packages of that class under Panda_Main, in
//! the build's asset registry order: object paths compared upper-cased): ringout (RingOutVfxData), announcer
//! (AnnouncerPackData), banner (BannerData), profileIcon (ProfileIconData), statTracker (StatTrackingBundleData),
//! emote (EmoteData, the stickers), taunt (TauntData of the ported characters' AvailableTauntData, in roster order).
//!
//! Left out (the port's and the build's): assets IsAssetEnabledInCurrentHydraEnvironment? turns off
//! (EnabledForDebugging); ring-outs one of whose systems is not in the build; banners not in the Released
//! DataLookUpTable (BP_Banner_PM resolves a banner through it); taunts of characters the port lacks; stickers whose
//! particle system is not in the build, and those with no RewardThumbnailMaterial (no tile picture in the build).
//! Listed although the build's forced offline mode hides them: every IsUnavailableOffline item.

use std::cell::RefCell;
use std::collections::HashMap;

use indexmap::IndexMap;
use mvs_uasset::locres::LocresTable;
use mvs_uasset::value::{Props, TextValue, Value};
use serde::Serialize;

use super::text;
use super::ui::rows;
use crate::fighter_port::{CHARACTER_DATA, CHARACTERS, pkg_of};
use crate::kind::{Cx, KindError};
use crate::objview::{ObjView, field, num, string, truthy};

// ---------------------------------------------------------------------------------------------- the rules

/// The tail of an enum value or a name (`EFighter::Batman` -> `Batman`); `""` for none (`enumTail`).
pub fn enum_tail(v: Option<&Value>) -> String {
    match v {
        Some(Value::Str(s)) => s.rsplit("::").next().unwrap_or_default().to_owned(),
        _ => String::new(),
    }
}

/// HelperFunctionLibrary IsAssetEnabledInCurrentHydraEnvironment? for this build: IsShippingBuild is true
/// (140785690), so an EnabledForDebugging asset is off (@124-198). The rest of the graph: IsForcedOfflineMode is
/// always true (PandaGameInstance ReceiveInit @41190), so an IsUnavailableOffline asset is off too (@531-642).
// Deviation: every item the port can draw is listed, so the port keeps the first test alone. EnabledForDebugging
// absent: the class default, false.
pub fn asset_offered(p: &Props) -> bool {
    !matches!(field(p, "EnabledForDebugging"), Some(Value::Bool(true)))
}

/// An EmoteData (a sticker) a slot can hold: offered ([`asset_offered`]), and drawable: no ParticleEffect (the
/// pawn's own EmoteMontage alone) or one whose system the build holds (`held`, by the reference). One rule for the
/// Collection's sticker list and the match's taunt table.
pub fn sticker_offered(p: &Props, held: impl Fn(&Value) -> bool) -> bool {
    let ps = field(p, "ParticleEffect");
    asset_offered(p) && (!ps.is_some_and(truthy) || ps.is_some_and(held))
}

const REWARDS: &str = "/Game/Panda_Main/Blueprints/Rewards";

/// PreferencesManager's literal default assets: Set Ring Out Vfx(ROV_Default) @10282, SetAnnouncerPack
/// (DefaultAnnouncerPack) @8944, Set Banner(Banner_NewDefault) @8631, SetStatTracker(StatTrackingBundle_Default, 0)
/// @3215; Get Rand Default Profile Icon's fallback `Selected = ProfileIcon_Default` @5.
pub struct DefaultAssets {
    pub ringout: String,
    pub announcer: String,
    pub banner: &'static str,
    pub stat_tracker: String,
    pub profile_icon: String,
}

fn default_assets() -> DefaultAssets {
    DefaultAssets {
        ringout: format!("{REWARDS}/RingOutVfx/ROV_Default"),
        announcer: format!("{REWARDS}/AnnouncerPacks/DefaultAnnouncerPack"),
        banner: "/Game/Panda_Main/PreMatch/Banners/Default/Banner_NewDefault",
        stat_tracker: format!("{REWARDS}/StatTrackingBundle/StatTrackingBundle_Default"),
        profile_icon: format!("{REWARDS}/ProfileIcons/ProfileIcon_Default"),
    }
}

/// PreferencesManager GetFirstDefaultTaunt's fallback `AsTaunt Data = Emote_Global` (@457).
pub const EMOTE_GLOBAL: &str = "/Game/Panda_Main/Blueprints/Taunts/Emotes/Emote_Global";

/// RewardInfo DetermineRewardTier (@0-1702): an asset whose UnlockLocation is CharacterMastery (3 of the exe's
/// enumerator table None, Shop, AccountLevel, CharacterMastery, Battlepass, ...) has the tier of the character's
/// MasteryRewardTable level whose first reward it is (index + 1); `mastery_tier` maps each such reward's package to
/// it ([`mastery_tiers`]).
// Unverified: Battlepass reads the current battlepass asset's tiers: not read; 0 as every other place.
pub fn reward_tier(p: &Props, pkg: &str, mastery_tier: &HashMap<String, u32>) -> u32 {
    if enum_tail(field(p, "UnlockLocation")) == "CharacterMastery" {
        mastery_tier.get(&pkg.to_lowercase()).copied().unwrap_or(0)
    } else {
        0
    }
}

/// RewardInfo DetermineRewardType's casts in its order (@10-1583): the asset's class -> its ERewardType byte (SkinData
/// 1, EmoteData 8, SkinXpBoostData 3, StatTrackingBundleData 4, ProfileIconData 5, RingOutVfxData 6, MapMusicData 7,
/// TauntData 2, CharacterQuipData 10, CharacterData 9, AnnouncerPackData 11, GuildBadgeData 14, BannerData 15,
/// BundleData 16, ProductData 16 (@1513 goto 1413), TimedBoostData 12).
const REWARD_TYPE_BY_CLASS: &[(&str, u32)] = &[
    ("SkinData", 1),
    ("EmoteData", 8),
    ("SkinXpBoostData", 3),
    ("StatTrackingBundleData", 4),
    ("ProfileIconData", 5),
    ("RingOutVfxData", 6),
    ("MapMusicData", 7),
    ("TauntData", 2),
    ("CharacterQuipData", 10),
    ("CharacterData", 9),
    ("AnnouncerPackData", 11),
    ("GuildBadgeData", 14),
    ("BannerData", 15),
    ("BundleData", 16),
    ("ProductData", 16),
    ("TimedBoostData", 12),
];

/// DetermineRewardType's slugs (@1618-2486, case-insensitive) that make a reward Currency (12).
const CURRENCY_SLUGS: &[&str] = &[
    "gold",
    "match_toasts",
    "character-xpboost",
    "gems-pc",
    "gems-playstation",
    "gems-xbox",
    "gems-pc-epic",
    "battlepass-premium-token",
    "winter-event-currency-2022",
    "event003-currency",
];

/// RewardInfo DetermineRewardType (@0-2902): the ERewardType byte of a reward asset by its class (`cls`, the class's
/// name without its package), then by its Slug: a currency slug 12, "empty" leaves the default 0 (@1910), a slug
/// containing "perk" 13 (Perk, @2573), any other 0 (the "Not a valid reward type" print, @2819).
// Unverified: each cast is matched by the exact class; a native subclass of a listed class is not resolved (none of
// the mastery tables' assets is one).
pub fn reward_type(cls: &str, slug: Option<&Value>) -> u32 {
    if let Some((_, t)) = REWARD_TYPE_BY_CLASS.iter().find(|(c, _)| *c == cls) {
        return *t;
    }
    let s = slug.and_then(Value::as_str).map(str::to_lowercase).unwrap_or_default();
    if CURRENCY_SLUGS.contains(&s.as_str()) {
        12
    } else if s == "empty" {
        0
    } else if s.contains("perk") {
        13
    } else {
        0
    }
}

/// Every character's MasteryRewardTable: the package (lower-cased) of each level's first reward -> level index + 1, a
/// later character's over an earlier one's (`masteryTiers`).
pub fn mastery_tiers(cx: &Cx<'_>, characters: &[&Props]) -> HashMap<String, u32> {
    let mut out = HashMap::new();
    for c in characters {
        let Some(Value::Struct(table)) = field(c, "MasteryRewardTable") else { continue };
        let Some(Value::Array(levels)) = field(table, "LevelRewardThresholds") else { continue };
        for (i, l) in levels.iter().enumerate() {
            let first = match l {
                Value::Struct(l) => match field(l, "Rewards") {
                    Some(Value::Array(r)) => r.first(),
                    _ => None,
                },
                _ => None,
            };
            let pkg = match first {
                Some(Value::Struct(r)) => ref_pkg(cx, field(r, "AssetReward")),
                _ => String::new(),
            };
            if !pkg.is_empty() {
                out.insert(pkg.to_lowercase(), u32::try_from(i + 1).unwrap_or(u32::MAX));
            }
        }
    }
    out
}

/// UI_Functions LoadAllRingOutDatas (@706-911): the IsUnlockedByDefault ring-outs first, then the owned, then the
/// rest; every item is owned in the port (hydraprefs isCosmeticOwned), so the defaults, then the others, each in the
/// registry's order.
pub fn ring_out_order(list: Vec<Asset>) -> Vec<Asset> {
    let default = |a: &Asset| matches!(field(&a.p, "IsUnlockedByDefault"), Some(Value::Bool(true)));
    let (mut first, rest): (Vec<Asset>, Vec<Asset>) = list.into_iter().partition(default);
    first.extend(rest);
    first
}

/// EStatTrackerTotalVector's enumerators in byte order (read in the exe).
pub const TOTAL_VECTOR: &[&str] = &[
    "Invalid",
    "TotalWins",
    "TotalRingouts",
    "TotalDoubleRingouts",
    "TotalAssists",
    "TotalAttacksDodged",
    "TotalRingoutLeader",
    "TotalMaxedCharacters",
    "TotalPointsScored",
    "EVO2022_1stPlace",
    "EVO2022_2ndPlace",
    "EVO2022_3rdPlace",
    "EVO2022_4thPlace",
    "EVO2022_Top8",
    "EVO2022_Top16",
    "EVO2022_Top32",
    "EVO2022_Top64",
    "TotalArcadeHardCompletions",
    "HighestDamageDealt",
    "CharacterWins",
    "CharacterHighestDamageDealt",
    "CharacterTotalDamageDealt",
    "CharacterTotalRingouts",
    "Winter2022Currency",
    "Valentines2023Currency",
    "Season3Rank1v1",
    "Season3Rank2v2",
    "TotalToastsGiven",
    "TotalToastsReceived",
];

/// EStatTrackerShowCountMethod's enumerators in byte order (read in the exe): 0 Normal, 1 Placement, 2 Hide.
pub const SHOW_COUNT: &[&str] = &["Normal", "Placement", "Hide"];

/// A native enum value's byte by its table; absent: 0, the class default. An unknown name is an error.
pub fn enum_byte(table: &[&str], v: Option<&Value>, what: &str) -> Result<u32, KindError> {
    if matches!(v, None | Some(Value::Null)) {
        return Ok(0);
    }
    let tail = enum_tail(v);
    match table.iter().position(|t| *t == tail) {
        Some(i) => Ok(u32::try_from(i).unwrap_or(u32::MAX)),
        None => Err(KindError::Failed(format!("{what}: unknown enum value {v:?}"))),
    }
}

// ---------------------------------------------------------------------------------------------- the catalogue

/// A reference value's package (`/Game/A/B.B`, `/Game/A/B.B:Sub` -> `/Game/A/B`), `""` for
/// none.
pub fn ref_pkg(cx: &Cx<'_>, v: Option<&Value>) -> String {
    match v.and_then(|v| string(cx.cache, v)) {
        Some(s) if !s.is_empty() => pkg_of(&s).to_owned(),
        _ => String::new(),
    }
}

/// One of a badge's BadgeLevelThreshholds: Threshhold, StarLevel, IconOverride, BadgeOverride.
pub type BadgeTier = (f64, f64, Option<String>, Option<String>);

/// An item of a chooser (`CosmeticItem`).
#[derive(Clone, Debug, Default, Serialize)]
pub struct CosmeticItem {
    pub slug: String,
    /// The asset's object path (`/Game/.../ROV_Default.ROV_Default`).
    pub path: String,
    /// DisplayName, in the game's culture.
    pub name: String,
    /// ERewardRarity tail, `None` when unset.
    pub rarity: String,
    /// The tile's brush, a build path (a texture or a material instance); `None`: its package is not in the build.
    pub icon: Option<String>,
    /// AssociatedCharacter's EFighter tail.
    pub fighter: Option<String>,
    /// IsUnlockedByDefault.
    pub def: bool,
    /// SortString (the badges set it).
    pub sort: Option<String>,
    /// RewardInfo DetermineRewardTier (taunts, stickers); `None`: 0.
    pub tier: Option<u32>,
    /// StarLevelThreshholds (badges).
    pub stars: Option<Vec<f64>>,
    // the badges' own (StatTrackingBundleData): the EStatTrackerTotalVector byte (`None` 0), CharacterSlug,
    // StarThreshholdsReversed, the ShowCountMethod byte (`None` 0 Normal), the TextureRef picture (a build path;
    // `None`: not in the build), BadgeLevelThreshholds [Threshhold, StarLevel, IconOverride, BadgeOverride].
    pub vec: Option<u32>,
    pub char_slug: Option<String>,
    pub rev: bool,
    pub show: Option<u32>,
    pub tex: Option<String>,
    pub tiers: Option<Vec<BadgeTier>>,
    /// What RewardInfo CreateFromAsset reads for the reward tooltips (@700, @749, @539): the RewardThumbnail picture
    /// and the ToolTipPreviewMaterial (build paths; `None`: unset or not in the build), the EUnlockLocation tail
    /// (`None`: unset or None).
    pub thumb: Option<String>,
    pub preview: Option<String>,
    pub unlock: Option<String>,
}

/// A slug and an asset's path (`AssetRef`).
#[derive(Clone, Debug, Default, Serialize)]
pub struct AssetRef {
    pub slug: String,
    pub path: String,
}

/// A ported character by CharacterData slug.
#[derive(Clone, Debug, Serialize)]
pub struct CosCharacter {
    pub key: String,
    pub fighter: String,
    pub taunts: Vec<String>,
    pub default_taunt: AssetRef,
}

/// The slugs of the default assets of each type (`defaults`).
#[derive(Clone, Debug, Default, Serialize)]
pub struct CosDefaults {
    pub ringout: String,
    pub announcer: String,
    pub banner: String,
    pub stat_tracker: String,
    pub profile_icon: String,
    pub profile_icon_pool: Vec<String>,
}

/// The catalogue (`Cosmetics`).
#[derive(Clone, Debug, Default, Serialize)]
pub struct Cosmetics {
    pub ringout: Vec<CosmeticItem>,
    pub announcer: Vec<CosmeticItem>,
    pub banner: Vec<CosmeticItem>,
    pub profile_icon: Vec<CosmeticItem>,
    pub stat_tracker: Vec<CosmeticItem>,
    pub emote: Vec<CosmeticItem>,
    pub taunt: Vec<CosmeticItem>,
    /// The ported characters by CharacterData slug.
    pub characters: IndexMap<String, CosCharacter>,
    /// PGI_GlobalData GetFightersInCharacterSelect through CharacterDataByFighter, as EFighter tails.
    pub select_fighters: Vec<String>,
    pub defaults: CosDefaults,
}

impl Cosmetics {
    /// Every chooser's list, in the order the menu bundle reads their pictures (ring-outs, announcers, banners,
    /// profile icons, badges, stickers, taunts).
    pub fn lists(&self) -> [&Vec<CosmeticItem>; 7] {
        [&self.ringout, &self.announcer, &self.banner, &self.profile_icon, &self.stat_tracker, &self.emote, &self.taunt]
    }
}

const PANDA_MAIN: &str = "/Game/Panda_Main";
const PGI: &str = "/Game/Panda_Main/Blueprints/PandaGameInstance/PGI_GlobalData.Default__PGI_GlobalData_C";
const RELEASED: &str = "/Game/Panda_Main/DataLookUpTables/Released/DataLookUpTable";

const RINGOUT_CLASS: &str = "/Script/MultiVersus.RingOutVfxData";
const ANNOUNCER_CLASS: &str = "/Script/MultiVersus.AnnouncerPackData";
const BANNER_CLASS: &str = "/Script/MultiVersus.BannerData";
const PROFILE_ICON_CLASS: &str = "/Script/MultiVersus.ProfileIconData";
const STAT_TRACKER_CLASS: &str = "/Script/MultiVersus.StatTrackingBundleData";
const EMOTE_CLASS: &str = "/Script/MultiVersus.EmoteData";

/// A reward asset as the bind reads it: its object path (as the reference that named it spells it, else the
/// object's own), its package (bare path) and its properties (the class defaults over its properties).
#[derive(Clone, Debug)]
pub struct Asset {
    pub path: String,
    pub pkg: String,
    pub p: Props,
}

/// DisplayName in the game's culture (`localized`): the table's string, else the text's source (a key the culture's
/// file lacks shows the source, as UE's text localization manager does), `""` for none. A string table entry is not
/// looked up here (no cosmetic name is one): its source, or `""`.
pub fn localized(table: &LocresTable, t: Option<&Value>) -> String {
    match t {
        Some(Value::Text(TextValue::Base { source, namespace, key })) => {
            table.get(namespace).and_then(|ns| ns.get(key)).cloned().unwrap_or_else(|| source.clone())
        }
        Some(Value::Text(TextValue::Table { source, .. } | TextValue::Invariant { source })) => {
            source.clone().unwrap_or_default()
        }
        _ => String::new(),
    }
}

struct Reader<'a, 'c> {
    cx: &'c Cx<'a>,
    /// Which packages the build holds (lower-cased), asked once each.
    held: RefCell<HashMap<String, bool>>,
}

impl<'a> Reader<'a, '_> {
    fn has(&self, pkg: &str) -> bool {
        if pkg.is_empty() {
            return false;
        }
        *self.held.borrow_mut().entry(pkg.to_lowercase()).or_insert_with(|| self.cx.exists(pkg))
    }

    /// A reference's package, `""` for none.
    fn pkg(&self, v: Option<&Value>) -> String {
        ref_pkg(self.cx, v)
    }

    /// A picture: the soft reference's object path when its package is in the build, else none.
    fn pic(&self, v: Option<&Value>) -> Option<String> {
        let s = v.and_then(|v| string(self.cx.cache, v))?;
        self.has(&self.pkg(v)).then_some(s)
    }

    /// The asset a reference names (`at`).
    fn at(&self, r: &str) -> Result<Asset, KindError> {
        let pkg = pkg_of(r).to_owned();
        let o = self.cx.object(&pkg).map_err(|e| KindError::Failed(format!("cosmetics: {pkg} was not read: {e}")))?;
        Ok(Asset { path: if r == pkg { o.path().to_owned() } else { r.to_owned() }, p: props_of(&o)?, pkg })
    }

    fn listed(&self, class: &str) -> Result<Vec<Asset>, KindError> {
        self.cx.catalog.list_packages(PANDA_MAIN, true, Some(class)).iter().map(|p| self.at(p)).collect()
    }

    fn entry(&self, text: &LocresTable, a: &Asset, icon: Option<String>) -> CosmeticItem {
        let p = &a.p;
        let rarity = enum_tail(field(p, "Rarity"));
        CosmeticItem {
            slug: self.text_of(field(p, "Slug")),
            path: a.path.clone(),
            name: localized(text, field(p, "DisplayName")),
            rarity: if rarity.is_empty() { "None".into() } else { rarity },
            icon,
            fighter: field(p, "AssociatedCharacter")
                .is_some_and(truthy)
                .then(|| enum_tail(field(p, "AssociatedCharacter"))),
            def: matches!(field(p, "IsUnlockedByDefault"), Some(Value::Bool(true))),
            sort: field(p, "SortString").filter(|v| truthy(v)).and_then(|v| string(self.cx.cache, v)),
            thumb: self.pic(field(p, "RewardThumbnail")),
            preview: self.pic(field(p, "ToolTipPreviewMaterial")),
            unlock: Some(enum_tail(field(p, "UnlockLocation"))).filter(|u| !u.is_empty() && u != "None"),
            ..CosmeticItem::default()
        }
    }

    fn text_of(&self, v: Option<&Value>) -> String {
        v.and_then(|v| string(self.cx.cache, v)).unwrap_or_default()
    }
}

/// The class defaults over an object's properties, else its properties (`o.defaults ?? o.properties`).
fn props_of(o: &ObjView<'_>) -> Result<Props, KindError> {
    Ok(match o.defaults()? {
        Some(d) => d,
        None => o.props().clone(),
    })
}

fn structs<'p>(p: &'p Props, name: &str) -> Vec<&'p Props> {
    match field(p, name) {
        Some(Value::Array(a)) => a.iter().filter_map(Value::as_struct).collect(),
        _ => Vec::new(),
    }
}

fn pairs<'p>(p: &'p Props, name: &str) -> &'p [(Value, Value)] {
    match field(p, name) {
        Some(Value::Map(m)) => m,
        _ => &[],
    }
}

/// The catalogue from the build's properties through the build's rules (`readCosmetics`, `bindCosmetics`).
pub fn read_cosmetics(cx: &Cx<'_>) -> Result<Cosmetics, KindError> {
    let text = text::table(cx, text::GAME_CULTURE)?;
    let r = Reader { cx, held: RefCell::new(HashMap::new()) };
    let pgi_obj = cx.object(PGI)?;
    let pgi = props_of(&pgi_obj)?;
    let global: Vec<Asset> = pairs(&pgi, "GlobalCharacterData")
        .iter()
        .map(|(_, v)| r.pkg(Some(v)))
        .filter(|p| !p.is_empty())
        .map(|p| r.at(&p))
        .collect::<Result<_, _>>()?;
    let mastery_tier = mastery_tiers(cx, &global.iter().map(|a| &a.p).collect::<Vec<_>>());
    let tiered = |a: &Asset| reward_tier(&a.p, &a.pkg, &mastery_tier);
    let shown = |a: &&Asset| asset_offered(&a.p);

    // ---- ring-outs: those whose systems are all in the build, defaults first (LoadAllRingOutDatas)
    let ringout_all: Vec<Asset> = r
        .listed(RINGOUT_CLASS)?
        .into_iter()
        .filter(|a| shown(&a))
        .filter(|a| structs(&a.p, "ParticleEffectData").iter().all(|d| r.has(&r.pkg(field(d, "ParticleEffect")))))
        .collect();
    let ringout: Vec<CosmeticItem> = ring_out_order(ringout_all)
        .iter()
        .map(|a| r.entry(&text, a, r.pic(field(&a.p, "DisplayTextureRef"))))
        .collect();
    if ringout.iter().any(|e| e.sort.is_some()) {
        return Err(KindError::Failed("a ring-out sets a SortString: the chooser would reorder them".into()));
    }

    // ---- announcer packs (the reward packs and the characters' own)
    let announcer: Vec<CosmeticItem> = r
        .listed(ANNOUNCER_CLASS)?
        .iter()
        .filter(shown)
        .map(|a| r.entry(&text, a, r.pic(field(&a.p, "DisplayTextureRef"))))
        .collect();

    // ---- banners: the Released lookup table's
    let released_table = cx.object(RELEASED).map_err(|e| KindError::Failed(format!("{RELEASED}: not loaded: {e}")))?;
    let released: Vec<Option<String>> =
        rows(&released_table)?.iter().map(|(_, row)| field(row, "Slug").and_then(|v| string(cx.cache, v))).collect();
    let banner: Vec<CosmeticItem> = r
        .listed(BANNER_CLASS)?
        .iter()
        .filter(shown)
        .filter(|a| released.contains(&field(&a.p, "Slug").and_then(|v| string(cx.cache, v))))
        .map(|a| r.entry(&text, a, r.pic(field(&a.p, "DisplayTextureRef"))))
        .collect();

    // ---- profile icons
    let profile_icon: Vec<CosmeticItem> = r
        .listed(PROFILE_ICON_CLASS)?
        .iter()
        .filter(shown)
        .map(|a| r.entry(&text, a, r.pic(field(&a.p, "TextureRef"))))
        .collect();

    // ---- badges. UI_BadgeInitItemData's icon (UI_BadgeGetStarsAndIcons @1541-1667 / @1862): TextureRef, or with
    // BadgeLevelThreshholds the count's tier's IconOverride when set (the count is 0 offline: the first tier at or
    // above it). `icon` stays the count-0 icon; the game picks the count's own by the same rule.
    let mut stat_tracker = Vec::new();
    for a in r.listed(STAT_TRACKER_CLASS)?.iter().filter(shown) {
        let p = &a.p;
        let tiers = structs(p, "BadgeLevelThreshholds");
        let threshold = |x: &Props| field(x, "Threshhold").and_then(num).unwrap_or(0.0);
        let t = tiers.iter().find(|x| threshold(x) >= 0.0).or(tiers.last());
        let icon_override = t.and_then(|t| field(t, "IconOverride"));
        let icon_ref = if r.pkg(icon_override).is_empty() { field(p, "TextureRef") } else { icon_override };
        let mut e = r.entry(&text, a, r.pic(icon_ref));
        e.stars = Some(match field(p, "StarLevelThreshholds") {
            Some(Value::Array(s)) => s.iter().filter_map(num).collect(),
            _ => Vec::new(),
        });
        let vec = enum_byte(TOTAL_VECTOR, field(p, "StatTrackerTotalVector"), &a.pkg)?;
        if vec != 0 {
            e.vec = Some(vec);
        }
        if field(p, "CharacterSlug").is_some_and(truthy) {
            e.char_slug = Some(r.text_of(field(p, "CharacterSlug")));
        }
        e.rev = matches!(field(p, "StarThreshholdsReversed"), Some(Value::Bool(true)));
        let show = enum_byte(SHOW_COUNT, field(p, "ShowCountMethod"), &a.pkg)?;
        if show != 0 {
            e.show = Some(show);
        }
        e.tex = r.pic(field(p, "TextureRef"));
        if !tiers.is_empty() {
            e.tiers = Some(
                tiers
                    .iter()
                    .map(|x| {
                        (
                            threshold(x),
                            field(x, "StarLevel").and_then(num).unwrap_or(0.0),
                            r.pic(field(x, "IconOverride")),
                            r.pic(field(x, "BadgeOverride")),
                        )
                    })
                    .collect(),
            );
        }
        stat_tracker.push(e);
    }

    // ---- stickers: every EmoteData the build offers whose particle system it has (none: the pawn's own; the
    // match's taunt table) and with a RewardThumbnailMaterial (the EmoteData tile's only picture, UI_EquipperItem_V2
    // @8768; the port's own choice)
    let emote: Vec<CosmeticItem> = r
        .listed(EMOTE_CLASS)?
        .iter()
        .filter(|a| sticker_offered(&a.p, |ps| r.has(&r.pkg(Some(ps)))))
        .filter(|a| !r.pkg(field(&a.p, "RewardThumbnailMaterial")).is_empty())
        .map(|a| {
            let mut e = r.entry(&text, a, r.pic(field(&a.p, "RewardThumbnailMaterial")));
            e.tier = Some(tiered(a)).filter(|t| *t != 0);
            e
        })
        .collect();

    // ---- taunts of the ported characters: CharacterData AvailableTauntData that the build has and offers
    // (GetCharacterTaunts @0-1378 with IsAssetEnabledInCurrentHydraEnvironment?), and GetFirstDefaultTaunt over them
    let mut taunt: Vec<CosmeticItem> = Vec::new();
    let mut characters = IndexMap::new();
    let emote_global = r.at(EMOTE_GLOBAL)?;
    for key in CHARACTERS {
        let data = CHARACTER_DATA.iter().find(|(k, _)| k == key).map_or("", |(_, p)| p);
        let c = r.at(data)?;
        let mut list = Vec::new();
        if let Some(Value::Array(refs)) = field(&c.p, "AvailableTauntData") {
            for v in refs {
                let s = string(cx.cache, v).unwrap_or_default();
                if r.has(pkg_of(&s)) {
                    list.push(r.at(&s)?);
                }
            }
        }
        let mut slugs = Vec::new();
        for a in list.iter().filter(shown) {
            let slug = r.text_of(field(&a.p, "Slug"));
            if !taunt.iter().any(|t| t.slug == slug) {
                let mut e = r.entry(&text, a, r.pic(field(&a.p, "RewardThumbnailMaterial")));
                e.tier = Some(tiered(a)).filter(|t| *t != 0);
                taunt.push(e);
            }
            slugs.push(slug);
        }
        // GetFirstDefaultTaunt: the first with IsUnlockedByDefault, else Emote_Global
        let first = list
            .iter()
            .find(|t| matches!(field(&t.p, "IsUnlockedByDefault"), Some(Value::Bool(true))))
            .unwrap_or(&emote_global);
        characters.insert(
            r.text_of(field(&c.p, "Slug")),
            CosCharacter {
                key: (*key).to_owned(),
                fighter: enum_tail(field(&c.p, "AssociatedCharacter")),
                taunts: slugs,
                default_taunt: AssetRef { slug: r.text_of(field(&first.p, "Slug")), path: first.path.clone() },
            },
        );
    }

    // ---- PGI_GlobalData GetFightersInCharacterSelect (@0) through CharacterDataByFighter, each one's
    // AssociatedCharacter
    let by_fighter: Vec<(Option<String>, String)> =
        pairs(&pgi, "CharacterDataByFighter").iter().map(|(k, v)| (string(cx.cache, k), r.pkg(Some(v)))).collect();
    let mut select_fighters = Vec::new();
    if let Some(Value::Array(list)) = field(&pgi, "FightersInCharacterSelect") {
        for f in list {
            let f = string(cx.cache, f);
            let pkg = by_fighter.iter().find(|(k, _)| *k == f).map(|(_, p)| p.as_str()).unwrap_or_default();
            if pkg.is_empty() {
                continue;
            }
            let tail = enum_tail(field(&r.at(pkg)?.p, "AssociatedCharacter"));
            if !tail.is_empty() {
                select_fighters.push(tail);
            }
        }
    }

    // ---- the defaults (DEFAULT_ASSETS) and Get Rand Default Profile Icon's pool (PGI_GlobalData ProfileIcons that
    // the build has)
    let slug_at = |pkg: &str| -> Result<String, KindError> { Ok(r.text_of(field(&r.at(pkg)?.p, "Slug"))) };
    let d = default_assets();
    let mut pool = Vec::new();
    if let Some(Value::Array(icons)) = field(&pgi, "ProfileIcons") {
        for v in icons {
            let pkg = r.pkg(Some(v));
            if r.has(&pkg) {
                pool.push(slug_at(&pkg)?);
            }
        }
    }
    let defaults = CosDefaults {
        ringout: slug_at(&d.ringout)?,
        announcer: slug_at(&d.announcer)?,
        banner: slug_at(d.banner)?,
        stat_tracker: slug_at(&d.stat_tracker)?,
        profile_icon: slug_at(&d.profile_icon)?,
        profile_icon_pool: pool,
    };

    Ok(Cosmetics {
        ringout,
        announcer,
        banner,
        profile_icon,
        stat_tracker,
        emote,
        taunt,
        characters,
        select_fighters,
        defaults,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rules() {
        assert_eq!(enum_tail(Some(&Value::Str("EFighter::Batman".into()))), "Batman");
        assert_eq!(enum_tail(None), "");
        assert_eq!(reward_type("SkinData", None), 1);
        let s = |v: &str| Value::Str(v.into());
        assert_eq!(reward_type("Other", Some(&s("Gold"))), 12);
        assert_eq!(reward_type("Other", Some(&s("perk_slot"))), 13);
        assert_eq!(reward_type("Other", Some(&s("empty"))), 0);
        assert_eq!(enum_byte(SHOW_COUNT, Some(&s("EStatTrackerShowCountMethod::Hide")), "x").unwrap(), 2);
        assert_eq!(enum_byte(SHOW_COUNT, None, "x").unwrap(), 0);
        assert!(enum_byte(SHOW_COUNT, Some(&s("Nope")), "x").is_err());
        let mut p = Props::new();
        assert!(asset_offered(&p));
        p.insert("EnabledForDebugging".into(), Value::Bool(true));
        assert!(!asset_offered(&p));
    }

    #[test]
    fn localized_falls_back_to_the_source() {
        let mut t = LocresTable::new();
        t.entry("ns".into()).or_default().insert("k".into(), "Hallo".into());
        let base =
            |ns: &str| Value::Text(TextValue::Base { source: "Hello".into(), namespace: ns.into(), key: "k".into() });
        assert_eq!(localized(&t, Some(&base("ns"))), "Hallo");
        assert_eq!(localized(&t, Some(&base("other"))), "Hello");
        assert_eq!(localized(&t, None), "");
    }
}
