//! A UI bundle of the ui kind (`hud`, `menu`): the widget classes with their trees, defaults and animations, the
//! typefaces and the faces' metrics, the central palette, the UI materials and textures, the portraits and the Game
//! target's strings the graphs set at run time.

use indexmap::IndexMap;
use serde::Deserialize;
use serde_json::Value;

use crate::text::FaceMetrics;
use crate::{Error, Result};

/// A linear RGBA colour.
pub type Rgba = [f64; 4];

pub use mvs_view::skins::{CENTRAL_PALETTE, CENTRAL_PALETTE_VECTORS};

/// A UI bundle.
#[derive(Debug, Default)]
pub struct Bundle {
    /// The widget classes by name.
    pub widgets: IndexMap<String, ClassData>,
    /// PFG's typefaces: typeface name -> its font face's build path (the font kind's entry).
    pub typefaces: IndexMap<String, String>,
    /// Each face's metrics by its build path.
    pub faces: IndexMap<String, FaceMetrics>,
    pub default_typeface: String,
    /// CentralPalette by row name: the UI colour.
    pub palette: IndexMap<String, Rgba>,
    /// CentralPalette by row name: the Game colour (MPC_CentralPalette's).
    pub palette_game: IndexMap<String, Rgba>,
    /// MPC_CentralPalette's vectors' defaults by parameter name, linear RGBA.
    pub palette_defaults: IndexMap<String, [f32; 4]>,
    /// Each ported character's CharacterSelectPortraitMaterial by the port's character name (the hud bundle).
    pub portraits: IndexMap<String, String>,
    /// The Game target's strings the graphs set at run time, by key.
    pub texts: IndexMap<String, String>,
    /// The UI materials a brush or a graph names: the bundle's name -> the material's object path.
    pub materials: IndexMap<String, String>,
    /// The textures a brush names: the bundle's name -> where it is.
    pub textures: IndexMap<String, TextureInfo>,
    /// The hud bundle's textures only a soft reference names, by bundle name (loaded when a widget is given one).
    pub hud_soft_textures: Vec<String>,
    /// The hud bundle's textures each character's HUD widgets draw, by the port's character name (bundle names).
    pub char_textures: IndexMap<String, Vec<String>>,
    /// DataTable InputButtonIconsTable (the menu bundle): a key name -> its icon-default texture's bundle name (the
    /// first row with the key, as PreferencesManager IconForInputButton takes it).
    pub input_icons: IndexMap<String, String>,
    /// The menu bundle's data (None: the hud bundle).
    pub menu: Option<MenuData>,
    /// The stages the build can load: (gameplay level's package name, stage key) each (stageOfLevel's table; the
    /// menu bundle's, read from the gamedata kind).
    pub stages: Vec<(String, String)>,
}

/// The menu bundle's data the menus read (the importer's `kinds/ui/menu.rs` and `kinds/ui/mod.rs` MenuBundle). The
/// whole roster and fighter list: the view keeps what the
/// port can draw (`MenuView::roster`, `MenuView::fighters`).
#[derive(Clone, Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MenuData {
    /// The Lab menu's fighters by the port's character name.
    #[serde(default)]
    pub fighters: IndexMap<String, FighterInfo>,
    /// The character select's tiles in order.
    #[serde(default)]
    pub roster: Vec<RosterEntry>,
    /// Each fighter class's display name and icon.
    #[serde(default)]
    pub class_info: IndexMap<String, ClassInfo>,
    /// The MapData assets by asset name.
    #[serde(default)]
    pub maps: IndexMap<String, MapData>,
    /// The training config's SelectedMap (map_training).
    #[serde(default)]
    pub lab_map: Option<String>,
    /// UI_Functions "Get Offline Soft Maps" without / with LocalPlay.
    #[serde(default)]
    pub offline_maps: Vec<String>,
    #[serde(default)]
    pub offline_maps_local: Vec<String>,
    /// Textures only soft references name (loaded when a widget asks).
    #[serde(default)]
    pub soft_textures: Vec<String>,
    /// The Glossary's DataTables by table name, each its rows in row order.
    #[serde(default)]
    pub glossary: IndexMap<String, Vec<GlossaryRow>>,
    /// UI_GameplayTipDataTable: each row's Tip as its Game.locres key, in row order.
    #[serde(default)]
    pub gameplay_tips: Vec<String>,
    /// The PerkData assets the fighters' perk pages name, by asset name.
    #[serde(default)]
    pub perks: IndexMap<String, PerkInfo>,
    /// The Collection's mastery track (`{ levels, rewards }`; its reader's track types it).
    #[serde(default)]
    pub mastery: Value,
    /// The home screen's promotions and their widgets (their reader's track types them).
    #[serde(default)]
    pub promotions: Value,
    #[serde(default)]
    pub promo_widgets: Value,
    /// The rich text styles by table (rich_text.rs types them).
    #[serde(default)]
    pub rich_styles: Value,
    /// The rich text image decorators' rows by decorator class (rich_text.rs types them).
    #[serde(default)]
    pub rich_images: Value,
    /// UI_Functions GetPaletteChoices: the CentralPalette rows a colour picker offers.
    #[serde(default)]
    pub palette_choices: Vec<String>,
    /// The cosmetics catalogue.
    #[serde(default)]
    pub cosmetics: Cosmetics,
    /// The shop's catalogue: the single items and the bundles its pages list (mvs-import `kinds/ui/shop.rs`).
    #[serde(default)]
    pub shop: ShopCatalogue,
    /// PGI_GlobalData's AccountLevelUpTable.LevelUpData, one row per level.
    #[serde(default)]
    pub account_levels: Vec<AccountLevelRow>,
    /// GetArcadeBotLoadouts' literals and the CoOpVsAI manager's boss skins.
    #[serde(default)]
    pub arcade_names: ArcadeNames,
}

// ---------- the cosmetics catalogue (mvs-import kinds/cosmetics.rs, its field names as they are written) ----------

/// One of a badge's BadgeLevelThreshholds: Threshhold, StarLevel, IconOverride, BadgeOverride.
pub type BadgeTier = (f64, f64, Option<String>, Option<String>);

/// An item of a chooser: a reward asset of the catalogue.
#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
#[serde(default)]
pub struct CosmeticItem {
    pub slug: String,
    /// The asset's object path (`/Game/.../ROV_Default.ROV_Default`).
    pub path: String,
    /// DisplayName, in the game's culture.
    pub name: String,
    /// ERewardRarity's tail, "None" when unset.
    pub rarity: String,
    /// The tile's brush, a build path (a texture or a material instance); None: its package is not in the build.
    pub icon: Option<String>,
    /// AssociatedCharacter's EFighter tail.
    pub fighter: Option<String>,
    /// IsUnlockedByDefault.
    pub def: bool,
    /// SortString (the badges set it).
    pub sort: Option<String>,
    /// RewardInfo DetermineRewardTier (taunts, stickers); None: 0.
    pub tier: Option<u32>,
    /// StarLevelThreshholds (badges).
    pub stars: Option<Vec<f64>>,
    /// The badges' own (StatTrackingBundleData): the EStatTrackerTotalVector byte (None: 0).
    pub vec: Option<u32>,
    /// CharacterSlug.
    pub char_slug: Option<String>,
    /// StarThreshholdsReversed.
    pub rev: bool,
    /// The ShowCountMethod byte (None: 0 Normal).
    pub show: Option<u32>,
    /// The TextureRef picture (a build path; None: not in the build).
    pub tex: Option<String>,
    /// BadgeLevelThreshholds.
    pub tiers: Option<Vec<BadgeTier>>,
    /// RewardThumbnail and ToolTipPreviewMaterial (build paths; None: unset or not in the build), the EUnlockLocation
    /// tail (None: unset or None): what the reward tooltips read (RewardInfo CreateFromAsset).
    pub thumb: Option<String>,
    pub preview: Option<String>,
    pub unlock: Option<String>,
}

/// A ported character of the catalogue (by CharacterData slug): the port's key, EFighter, the taunts its slots can
/// hold (slugs) and PreferencesManager GetFirstDefaultTaunt.
#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
#[serde(default)]
pub struct CosCharacter {
    pub key: String,
    pub fighter: String,
    pub taunts: Vec<String>,
    pub default_taunt: mvs_settings::AssetRef,
}

/// The slugs of each type's default asset.
#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
#[serde(default)]
pub struct CosDefaults {
    pub ringout: String,
    pub announcer: String,
    pub banner: String,
    pub stat_tracker: String,
    pub profile_icon: String,
    /// PGI_GlobalData.ProfileIcons (Get Rand Default Profile Icon's pool).
    pub profile_icon_pool: Vec<String>,
}

/// The catalogue.
#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
#[serde(default)]
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
    /// Every chooser's list: ring-outs, announcers, banners, profile icons, badges, stickers, taunts.
    pub fn lists(&self) -> [&Vec<CosmeticItem>; 7] {
        [&self.ringout, &self.announcer, &self.banner, &self.profile_icon, &self.stat_tracker, &self.emote, &self.taunt]
    }
}

// ---------- the shop catalogue (mvs-import kinds/ui/shop.rs, its field names as they are written) ----------

/// A text of the build: the localized string and its key (never typed in the code).
#[derive(Clone, Debug, Default, PartialEq, Eq, Deserialize)]
#[serde(default)]
pub struct ShopText {
    /// The string in the game's culture.
    pub text: String,
    pub namespace: String,
    pub key: String,
}

/// One price of a bundle: ECurrencyType's tail (`gems`) and the amount.
#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
#[serde(default)]
pub struct ShopPrice {
    pub currency: String,
    pub amount: f64,
}

/// A skin, ring-out, emote, taunt, banner, profile icon, announcer pack, badge or fighter the shop lists or a bundle
/// holds. The ShopProduct of a single item (ProductSlug = `slug`, ContentAsset = `path`, no price) is built from it.
#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
#[serde(default)]
pub struct ShopItem {
    /// The asset's Slug: the product slug, and what a bundle names the item by.
    pub slug: String,
    /// The asset's object path (the product's ContentAssetPath).
    pub path: String,
    /// The asset's class name (`SkinData`): the cell's Get Item Type.
    pub class: String,
    /// ERewardType's byte (`Get Item Type`): the type text, the cell's height (1, 15, 16 are tall), the owned wording.
    pub kind: u32,
    /// The stand-in's product tag (the tab table's IncludedTags name); empty for what no tab lists.
    pub tag: String,
    /// DisplayName: the cell's title.
    pub name: String,
    /// ERewardRarity's tail (`None` when unset): the rarity stripe and the type line.
    pub rarity: String,
    /// AssociatedCharacter's EFighter tail: the fighter line, the Has Character filter.
    pub fighter: Option<String>,
    /// AssociatedUniverse's tail: the filter grid's universe dropdown.
    pub universe: Option<String>,
    /// UnlockLocation's tail (`None` when unset).
    pub unlock: String,
    /// IsPurchasable is not false.
    pub purchasable: bool,
    /// On a tab's page: UnlockLocation Shop, offered, purchasable. An unlisted item is here for a bundle's details.
    pub listed: bool,
    /// ShopCellOverrideMaterial (a build path the bundle holds).
    pub cell_material: Option<String>,
    /// ShopModalOverrideMaterial: the details modal's art where set.
    pub modal_material: Option<String>,
    /// The picture the cell draws, resolved by the cell's rules; a thumbnail texture goes through
    /// [`ShopCatalogue::default_image`] when no material is set.
    pub image: Option<String>,
    /// SkinData VariantIconMaterial.
    pub variant_icon: Option<String>,
    /// SkinData VariantLocalPlayPortraitMaterial.
    pub local_play_portrait: Option<String>,
    /// RewardThumbnail (a texture).
    pub thumbnail: Option<String>,
    /// RewardThumbnailMaterial.
    pub thumbnail_material: Option<String>,
    /// A ring-out's ToolTipPreviewMaterial: the details modal's art.
    pub tooltip_preview: Option<String>,
}

/// An item of a bundle: its slug in [`ShopCatalogue::items`] and the quantity.
#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
#[serde(default)]
pub struct ShopBundleItem {
    pub slug: String,
    pub quantity: f64,
}

/// A ProductData bundle.
#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
#[serde(default)]
pub struct ShopBundle {
    /// The product slug.
    pub slug: String,
    /// The asset's object path.
    pub path: String,
    /// DisplayName: the cell's title and the details modal's.
    pub name: ShopText,
    /// BundleSubText: the cell's description line, `name` when None.
    pub sub_text: Option<ShopText>,
    /// ProductDescription.
    pub description: String,
    /// The gem price (CurrencyType `gems`).
    pub prices: Vec<ShopPrice>,
    /// The bundle's items, as slugs of the catalogue.
    pub items: Vec<ShopBundleItem>,
    /// AssociatedCharacter's EFighter tail.
    pub fighter: Option<String>,
    /// AssociatedUniverse's tail.
    pub universe: Option<String>,
    /// ShopCellOverrideMaterial: the cell's picture.
    pub cell_material: Option<String>,
    /// UnlockLocation's tail (`None` when unset).
    pub unlock: String,
    /// RequiresAllOwned: owned when all items are (else when any is).
    pub requires_all_owned: bool,
    pub purchase_once: bool,
    /// Visible, AvailableForPurchase, IsEnabledInDevelopmentEnvironment as the asset sets them (None: unset).
    pub visible: Option<bool>,
    pub available_for_purchase: Option<bool>,
    pub enabled_in_development: Option<bool>,
}

impl ShopBundle {
    /// The price in `currency` (`gems`), None when the bundle has none in it.
    pub fn price(&self, currency: &str) -> Option<f64> {
        self.prices.iter().find(|p| p.currency == currency).map(|p| p.amount)
    }
}

/// A ShopCellItemSettingsRowStructure's palette, linear RGBA each.
#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
#[serde(default)]
pub struct ShopPalette {
    pub background: [f64; 4],
    pub foreground: [f64; 4],
    pub button: [f64; 4],
    pub stripe: [f64; 4],
    pub text: [f64; 4],
}

/// A row of the ItemSettings DataTable (the product slug's row, else `DEFAULT`).
#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
#[serde(default)]
pub struct ShopItemSettings {
    /// UseCustomPalette: the cell's colours come from `palette`.
    pub use_custom_palette: bool,
    pub palette: ShopPalette,
    /// ImageOffsets (X, Y, Z, W): the picture's UV window in a normal cell.
    pub image_offsets: [f64; 4],
    /// DifferentOffsetsForLarge: a large cell takes `image_offsets_large`.
    pub different_offsets_for_large: bool,
    pub image_offsets_large: [f64; 4],
}

/// The shop catalogue of the menu bundle.
#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
#[serde(default)]
pub struct ShopCatalogue {
    /// The items the shop lists or a bundle holds: the listed ones in tab order, then the bundles' unlisted ones.
    pub items: Vec<ShopItem>,
    /// The real bundles, in the asset registry's order.
    pub bundles: Vec<ShopBundle>,
    /// The ItemSettings rows by row name.
    pub settings: IndexMap<String, ShopItemSettings>,
    /// `MI_WW_DefaultShopImage`: what a thumbnail texture is drawn through.
    pub default_image: Option<String>,
}

impl ShopCatalogue {
    /// The item of a slug.
    pub fn item(&self, slug: &str) -> Option<&ShopItem> {
        self.items.iter().find(|i| i.slug == slug)
    }

    /// The listed items of a product tag, in catalogue order.
    pub fn listed<'a>(&'a self, tag: &'a str) -> impl Iterator<Item = &'a ShopItem> {
        self.items.iter().filter(move |i| i.listed && i.tag == tag)
    }

    /// The bundles that hold the item of `slug` (the details modal's "Appears in a bundle").
    pub fn bundles_with<'a>(&'a self, slug: &'a str) -> impl Iterator<Item = &'a ShopBundle> {
        self.bundles.iter().filter(move |b| b.items.iter().any(|i| i.slug == slug))
    }

    /// GetItemSettingsFromItem: the row of `slug`, else `DEFAULT`.
    pub fn settings_of(&self, slug: &str) -> Option<&ShopItemSettings> {
        self.settings.get(slug).or_else(|| self.settings.get("DEFAULT"))
    }
}

/// One row of AccountLevelUpTable.LevelUpData: the XP of the level it stands for (row level - 1) and its gold reward.
#[derive(Clone, Copy, Debug, Default, PartialEq, Deserialize)]
#[serde(default)]
pub struct AccountLevelRow {
    pub xp: f64,
    pub gold: f64,
}

/// CharacterData ArcadeRivalSlug, ArcadeMapSlug1v1, ArcadeMapSlug2v2 ("" when the asset leaves one empty).
#[derive(Clone, Debug, Default, PartialEq, Eq, Deserialize)]
#[serde(default)]
pub struct ArcadeCharacter {
    pub rival: String,
    pub map1v1: String,
    pub map2v2: String,
}

/// PGI_CoOpVsAIManager GetArcadeBotLoadouts' literals, which have no locres line: the rival's name prefix (@5065,
/// its trailing spaces its own), the Mini (@5145), Reign Dog (@5182) and Giant Gizmo (@5216) names, the text for an
/// empty DisplayName (@4227); and the class default's boss skins (ReindogBossSkin, GizmoBossSkin: SkinData asset
/// names).
#[derive(Clone, Debug, Default, PartialEq, Eq, Deserialize)]
#[serde(default)]
pub struct ArcadeNames {
    pub nemesis_prefix: String,
    pub mini: String,
    pub reindog: String,
    pub giant_gizmo: String,
    pub empty_name: String,
    pub reindog_boss_skin: String,
    pub gizmo_boss_skin: String,
}

/// A CharacterData as the menus read it (FighterInfo).
#[derive(Clone, Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FighterInfo {
    #[serde(default)]
    pub slug: String,
    #[serde(default)]
    pub display_name: String,
    /// The default skin.
    #[serde(default)]
    pub skin: SkinRef,
    #[serde(default)]
    pub skins: Vec<SkinInfo>,
    /// The perk page's lists by EPerkCategory and the RecommendedPerkDatas, as perk ids.
    #[serde(default)]
    pub perks: Option<FighterPerks>,
    /// ArcadeProgressCharacterIconTexture (a soft texture of the bundle).
    #[serde(default)]
    pub arcade_icon: Option<String>,
    /// The Move List's rows by row name (`{ title, description, outOfDate }`).
    #[serde(default)]
    pub moves: Value,
    /// BackgroundColor (`{ R, G, B, A }`).
    #[serde(default)]
    pub background_color: Value,
    /// CharacterData Bio (its source string).
    #[serde(default)]
    pub bio: String,
    /// CollectionsPortraitMaterial (the Collection's big portrait: the bundle's name of the material).
    #[serde(default)]
    pub collections_portrait: Option<String>,
    /// The arcade's fields of the CharacterData.
    #[serde(default)]
    pub arcade: ArcadeCharacter,
}

/// A fighter's default skin.
#[derive(Clone, Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SkinRef {
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub slug: String,
    #[serde(default)]
    pub display_name: String,
    #[serde(default)]
    pub icon: String,
}

/// A SkinData as the skin selector's tile reads it.
#[derive(Clone, Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SkinInfo {
    /// The SkinData asset name (C020_S02).
    #[serde(default)]
    pub name: String,
    /// Its soft reference.
    #[serde(default)]
    pub path: String,
    #[serde(default)]
    pub slug: String,
    #[serde(default)]
    pub display_name: String,
    /// VariantIconMaterial.
    #[serde(default)]
    pub icon: String,
    #[serde(default)]
    pub rarity: String,
    #[serde(default)]
    pub is_default: bool,
    /// VariantLocalPlayPortraitMaterial.
    #[serde(default)]
    pub local_play_portrait: Option<String>,
    /// RewardThumbnail (a texture's bundle name) and AssociatedCharacter's EFighter tail: the skin tile's tooltip.
    #[serde(default)]
    pub thumb: Option<String>,
    #[serde(default)]
    pub character: Option<String>,
    /// UnlockLocation's EUnlockLocation tail (None: unset or None) and RewardInfo DetermineRewardTier.
    #[serde(default)]
    pub unlock: Option<String>,
    #[serde(default)]
    pub tier: u32,
}

#[derive(Clone, Debug, Default, Deserialize)]
pub struct FighterPerks {
    #[serde(default)]
    pub lists: Vec<Vec<String>>,
    #[serde(default)]
    pub recommended: Vec<String>,
}

/// A tile of the character selector.
#[derive(Clone, Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RosterEntry {
    #[serde(default)]
    pub slug: String,
    #[serde(default)]
    pub display_name: String,
    #[serde(default)]
    pub portrait: String,
    #[serde(default)]
    pub class: String,
    #[serde(default, rename = "type")]
    pub kind: String,
    #[serde(default)]
    pub difficulty: String,
    #[serde(default)]
    pub property: String,
    #[serde(default)]
    pub experimental: bool,
    /// The port's name of a ported character.
    #[serde(default)]
    pub fighter: Option<String>,
    /// CharacterData Bio (its source string).
    #[serde(default)]
    pub bio: String,
    /// CollectionsPortraitMaterial (the bundle's name of the material).
    #[serde(default)]
    pub collections_portrait: Option<String>,
    /// BackgroundColor (`{ R, G, B, A }`).
    #[serde(default)]
    pub background_color: Value,
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ClassInfo {
    #[serde(default)]
    pub display_name: String,
    #[serde(default)]
    pub icon: String,
}

/// A MapData asset as the menus read it.
#[derive(Clone, Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MapData {
    #[serde(default)]
    pub display_name: String,
    #[serde(default)]
    pub property: String,
    /// MinSupportedPlayers / MaxSupportedPlayers.
    #[serde(default)]
    pub min: f64,
    #[serde(default)]
    pub max: f64,
    #[serde(default)]
    pub experimental: bool,
    /// A texture of the bundle, loaded on demand.
    #[serde(default)]
    pub texture: String,
    /// Level and LabLevelsToLoad.
    #[serde(default)]
    pub level: String,
    #[serde(default)]
    pub levels: Vec<String>,
    /// EMapRotation names.
    #[serde(default)]
    pub rotations: Vec<String>,
    #[serde(default)]
    pub slug: String,
}

/// A MapData with its asset name (MapInfo).
#[derive(Clone, Debug, Default)]
pub struct MapInfo {
    pub name: String,
    pub data: MapData,
    /// The stage its first level opens (stageOfLevel), None where the build cannot load it.
    pub stage: Option<String>,
}

/// A glossary table's row: Name, and Description with its rich text tags.
#[derive(Clone, Debug, Default, Deserialize)]
pub struct GlossaryRow {
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub description: String,
}

/// A PerkData as the perk page reads it.
#[derive(Clone, Debug, Default, Deserialize)]
pub struct PerkInfo {
    #[serde(default)]
    pub slug: String,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub stacked: String,
    #[serde(default)]
    pub category: u32,
    #[serde(default)]
    pub level: String,
    #[serde(default)]
    pub icon: Option<String>,
    #[serde(default)]
    pub fighter: Option<String>,
}

/// A texture of the bundle.
#[derive(Clone, Debug, Default, Deserialize)]
pub struct TextureInfo {
    /// The texture's build object path (the texture kind's entry); None: an engine Slate brush file.
    #[serde(default)]
    pub path: Option<String>,
    /// An engine Slate brush file's `image` kind entry (a PNG).
    #[serde(default)]
    pub image: Option<String>,
    #[serde(default)]
    pub srgb: bool,
    /// TextureAddress X / Y (TA_Wrap, TA_Clamp, TA_Mirror).
    #[serde(default)]
    pub ax: String,
    #[serde(default)]
    pub ay: String,
    /// The filter the build samples the texture with (its Filter, TF_Default its LOD group's); None: an engine Slate
    /// brush file, which has no texture object.
    #[serde(default)]
    pub filter: Option<mvs_material::material::Filter>,
    #[serde(default)]
    pub w: u32,
    #[serde(default)]
    pub h: u32,
    #[serde(default)]
    pub mips: u32,
}

impl TextureInfo {
    /// The store entry: the texture kind's path, else the image kind's.
    pub fn entry(&self) -> &str {
        self.path.as_deref().or(self.image.as_deref()).unwrap_or("")
    }
}

/// A widget of a class's tree as the bundle stores it: type, name, widget properties, slot
/// type and slot properties, children, a user widget instance's named slot contents.
#[derive(Clone, Debug, Default, Deserialize)]
pub struct WData {
    pub t: String,
    pub n: String,
    #[serde(default)]
    pub p: Value,
    #[serde(default)]
    pub st: String,
    #[serde(default)]
    pub s: Value,
    #[serde(default)]
    pub c: Vec<WData>,
    #[serde(default)]
    pub ns: IndexMap<String, WData>,
}

/// A widget class: its tree, its defaults and its animations.
#[derive(Clone, Debug, Default, Deserialize)]
pub struct ClassData {
    pub root: WData,
    #[serde(default)]
    pub defaults: Value,
    #[serde(default)]
    pub anims: IndexMap<String, AnimData>,
}

/// A float channel (FMovieSceneFloatChannel as the bind writes it): key ticks; per key value, interp (0 linear,
/// 1 constant, 2 cubic), arrive, leave and, for a weighted key, arrive weight, leave weight, weight mode (1 arrive,
/// 2 leave, 3 both); the extrapolations; the default.
#[derive(Clone, Debug, Default, Deserialize)]
pub struct Channel {
    pub t: Vec<f64>,
    pub v: Vec<Vec<f64>>,
    #[serde(default)]
    pub pre: String,
    #[serde(default)]
    pub post: String,
    #[serde(default)]
    pub def: Option<f64>,
}

/// A track's section: its tick range (None: open), the upper bound exclusive, a 2D transform section's channel mask,
/// its channels by name.
#[derive(Clone, Debug, Default, Deserialize)]
pub struct Section {
    pub range: [Option<f64>; 2],
    #[serde(default)]
    pub excl: bool,
    #[serde(default)]
    pub mask: Option<u32>,
    #[serde(default)]
    pub ch: IndexMap<String, Channel>,
}

/// A track bound to a widget of the animation's owner (`$root`: the owner).
#[derive(Clone, Debug, Default, Deserialize)]
pub struct Track {
    pub widget: String,
    #[serde(rename = "type")]
    pub kind: String,
    #[serde(default)]
    pub prop: Option<String>,
    #[serde(default)]
    pub sections: Vec<Section>,
}

/// A widget animation (UWidgetAnimation): its tick resolution, playback range, tracks and event keys.
#[derive(Clone, Debug, Default, Deserialize)]
pub struct AnimData {
    /// The tick resolution (ticks a second).
    pub res: f64,
    /// The playback range, ticks.
    pub range: [f64; 2],
    #[serde(default)]
    pub tracks: Vec<Track>,
    /// The master event track's keys: tick, entry point function.
    #[serde(default)]
    pub events: Vec<(f64, String)>,
}

/// The Large social panel's width (the port's own choice; the build's is 1250).
pub const SOCIAL_LARGE_WIDTH: f64 = 500.0;

impl AnimData {
    /// The animation's length, seconds: its playback range over its tick resolution.
    pub fn duration(&self) -> f64 {
        (self.range[1] - self.range[0]) / self.res
    }
}

#[derive(Deserialize)]
struct Lc {
    #[serde(rename = "R", default)]
    r: f64,
    #[serde(rename = "G", default)]
    g: f64,
    #[serde(rename = "B", default)]
    b: f64,
    #[serde(rename = "A", default = "one")]
    a: f64,
}

fn one() -> f64 {
    1.0
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Raw {
    widgets: IndexMap<String, ClassData>,
    #[serde(default)]
    typefaces: IndexMap<String, String>,
    #[serde(default)]
    faces: IndexMap<String, FaceMetrics>,
    #[serde(default)]
    default_typeface: String,
    #[serde(default)]
    palette: IndexMap<String, Lc>,
    #[serde(default)]
    palette_game: IndexMap<String, Lc>,
    #[serde(default)]
    palette_defaults: IndexMap<String, Vec<f64>>,
    #[serde(default)]
    materials: IndexMap<String, String>,
    #[serde(default)]
    textures: IndexMap<String, TextureInfo>,
    #[serde(default)]
    portraits: IndexMap<String, String>,
    #[serde(default)]
    texts: IndexMap<String, String>,
    #[serde(default)]
    input_icons: IndexMap<String, String>,
    #[serde(default)]
    char_textures: IndexMap<String, Vec<String>>,
    #[serde(default)]
    soft_textures: Vec<String>,
}

/// The menu bundle has the roster (the hud bundle has none).
#[derive(Deserialize)]
struct MenuProbe {
    #[serde(default)]
    roster: Option<Value>,
}

/// A bundle's textures alone (`Bundle::texture_filters`).
#[derive(Deserialize)]
struct TexturesOnly {
    #[serde(default)]
    textures: IndexMap<String, TextureInfo>,
}

/// The ui kind's entry path of `entry` (`hud`, `menu`).
fn entry_path(pack: &mvs_store::Pack, entry: &str) -> Result<String> {
    let tail = format!("/{entry}.{entry}");
    pack.paths()
        .find(|p| p.ends_with(&tail))
        .map(str::to_string)
        .ok_or_else(|| Error::Format { what: format!("ui {entry}"), why: "the ui kind has no such entry".into() })
}

impl Bundle {
    /// The ui kind's entry `entry` (`hud`, `menu`): the bundle and its textures' paths.
    pub fn load(store: &mvs_store::Store, entry: &str) -> Result<Bundle> {
        let pack = store.kind("ui")?;
        let path = entry_path(&pack, entry)?;
        let mut b = Bundle::parse(&pack.bytes(&path)?, &path)?;
        if b.menu.is_some() {
            b.stages = mvs_data::stage_levels(store)
                .map_err(|e| Error::Format { what: "gamedata stages".into(), why: e.to_string() })?;
        }
        Ok(b)
    }

    /// UI_SocialPanel's two transitions that end at the build's Large width (AnimTransitionMediumToLarge,
    /// AnimTransitionLargeToSmall): mainPanel's RightCurve keys past SOCIAL_LARGE_WIDTH are clamped to it.
    fn narrow_social_panel(&mut self) {
        let Some(class) = self.widgets.get_mut("UI_SocialPanel") else { return };
        for name in ["AnimTransitionMediumToLarge", "AnimTransitionLargeToSmall"] {
            let Some(anim) = class.anims.get_mut(name) else { continue };
            for tr in anim.tracks.iter_mut().filter(|t| t.widget == "mainPanel") {
                for ch in tr.sections.iter_mut().filter_map(|s| s.ch.get_mut("RightCurve")) {
                    for key in &mut ch.v {
                        if let Some(v) = key.first_mut().filter(|v| **v > SOCIAL_LARGE_WIDTH) {
                            *v = SOCIAL_LARGE_WIDTH;
                        }
                    }
                }
            }
        }
    }

    /// The filter of every texture the hud and menu bundles name, by its texture kind entry (`TextureInfo::filter`):
    /// what the element batches sample a brush's texture with.
    pub fn texture_filters(
        store: &mvs_store::Store,
    ) -> Result<std::collections::HashMap<String, mvs_material::material::Filter>> {
        let pack = store.kind("ui")?;
        let mut out = std::collections::HashMap::new();
        for entry in ["hud", "menu"] {
            let path = entry_path(&pack, entry)?;
            let t: TexturesOnly = serde_json::from_slice(&pack.bytes(&path)?)
                .map_err(|e| Error::Format { what: path.clone(), why: e.to_string() })?;
            for info in t.textures.into_values() {
                if let (Some(p), Some(f)) = (info.path, info.filter) {
                    out.insert(p, f);
                }
            }
        }
        Ok(out)
    }

    /// The stage whose gameplay level is the package `level` (`Map_M011`), or None (stageOfLevel).
    pub fn stage_of_level(&self, level: &str) -> Option<&str> {
        self.stages.iter().find(|(l, _)| l == level).map(|(_, k)| k.as_str())
    }

    /// A bundle from its JSON bytes; `what` names it in errors.
    pub fn parse(bytes: &[u8], what: &str) -> Result<Bundle> {
        let r: Raw =
            serde_json::from_slice(bytes).map_err(|e| Error::Format { what: what.to_string(), why: e.to_string() })?;
        let rgba = |c: Lc| [c.r, c.g, c.b, c.a];
        let fail = |e: serde_json::Error| Error::Format { what: what.to_string(), why: e.to_string() };
        let probe: MenuProbe = serde_json::from_slice(bytes).map_err(fail)?;
        let menu = match probe.roster {
            Some(_) => Some(serde_json::from_slice::<MenuData>(bytes).map_err(fail)?),
            None => None,
        };
        let mut bundle = Bundle {
            widgets: r.widgets,
            typefaces: r.typefaces,
            faces: r.faces,
            default_typeface: r.default_typeface,
            palette: r.palette.into_iter().map(|(k, c)| (k, rgba(c))).collect(),
            palette_game: r.palette_game.into_iter().map(|(k, c)| (k, rgba(c))).collect(),
            palette_defaults: r
                .palette_defaults
                .into_iter()
                .map(|(k, v)| (k, std::array::from_fn(|i| v.get(i).copied().unwrap_or(0.0) as f32)))
                .collect(),
            // A DrawRef `<object path>?ctx=slate`: the material's object path.
            materials: r
                .materials
                .into_iter()
                .map(|(k, v)| {
                    let p = v.split('?').next().unwrap_or("").to_string();
                    (k, p)
                })
                .collect(),
            textures: r.textures,
            portraits: r.portraits,
            texts: r.texts,
            input_icons: r.input_icons,
            hud_soft_textures: if menu.is_none() { r.soft_textures } else { Vec::new() },
            char_textures: r.char_textures,
            menu,
            stages: Vec::new(),
        };
        if bundle.menu.is_some() {
            bundle.narrow_social_panel();
        }
        Ok(bundle)
    }

    /// The widget class `class`'s animation `name`; panics when the bundle has none.
    pub fn anim(&self, class: &str, name: &str) -> &AnimData {
        self.widgets
            .get(class)
            .and_then(|c| c.anims.get(name))
            .unwrap_or_else(|| panic!("ui bundle: no animation {name} of {class}"))
    }

    /// The bundle's materials table holds `path` (a material's object path, as a widget's `tex` names it).
    pub fn has_material(&self, path: &str) -> bool {
        self.materials.values().any(|p| p == path)
    }

    /// The object path of the material the bundle names `name`.
    pub fn material_path(&self, name: &str) -> Option<&str> {
        self.materials.get(name).map(String::as_str)
    }

    /// InputButtonIconsTable's icon of the key `key`: its texture's store entry (None: no row, or a texture the
    /// bundle does not hold).
    pub fn input_icon(&self, key: &str) -> Option<&str> {
        self.input_icons.get(key).and_then(|n| self.textures.get(n)).map(TextureInfo::entry)
    }

    /// MPC_CentralPalette's rows as the game sets them from the UI preferences `ui` (the bundle's paletteDefaults,
    /// then the preferences' material colours), in the collection's vector order.
    pub fn central_palette_rows(&self, ui: &mvs_settings::UiSettings) -> Vec<[f32; 4]> {
        let bound: [&str; 5] = std::array::from_fn(|k| crate::menu::palette::palette_row(ui, k as u8).unwrap_or(""));
        mvs_view::skins::central_palette(&self.palette_defaults, &self.palette_game, bound)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A bundle and a skin as the importer writes them: the fields the shop's cells read come back typed, and a
    /// member the importer leaves out takes its default.
    #[test]
    fn the_shop_catalogue_reads_a_bundle_and_a_skin() {
        let json = r#"{
            "items": [
                {"slug": "skin_c004_s02", "path": "/Game/S.S", "class": "SkinData", "kind": 1, "tag": "skin",
                 "name": "Samurai Batman", "rarity": "Legendary", "fighter": "Batman", "universe": "DC",
                 "unlock": "Shop", "purchasable": false, "listed": false, "cell_material": "/Game/C.C",
                 "image": "/Game/C.C"},
                {"slug": "character_arya", "kind": 9, "listed": true, "tag": "character"}
            ],
            "bundles": [
                {"slug": "bundle_arya", "name": {"text": "Arya Bundle", "namespace": "", "key": "D0A9"},
                 "prices": [{"currency": "gems", "amount": 1500}],
                 "items": [{"slug": "character_arya", "quantity": 1}, {"slug": "skin_c004_s02", "quantity": 1}],
                 "fighter": "Arya", "unlock": "None"}
            ],
            "settings": {"DEFAULT": {"image_offsets": [0, 0, 1, 1]}},
            "default_image": "/Game/D.D"
        }"#;
        let shop: ShopCatalogue = serde_json::from_str(json).unwrap();
        let skin = shop.item("skin_c004_s02").unwrap();
        assert_eq!((skin.kind, skin.rarity.as_str(), skin.fighter.as_deref()), (1, "Legendary", Some("Batman")));
        assert!(!skin.purchasable && !skin.listed);
        assert_eq!(skin.image, skin.cell_material);
        let bundle = &shop.bundles[0];
        assert_eq!(bundle.price("gems"), Some(1500.0));
        assert_eq!(bundle.price("gold"), None);
        assert!(bundle.sub_text.is_none() && !bundle.requires_all_owned);
        assert_eq!(shop.bundles_with("skin_c004_s02").count(), 1);
        assert_eq!(shop.listed("character").count(), 1);
        assert_eq!(shop.listed("skin").count(), 0);
        assert!(shop.settings_of("character-harley").is_some_and(|s| s.image_offsets == [0.0, 0.0, 1.0, 1.0]));
    }
}
