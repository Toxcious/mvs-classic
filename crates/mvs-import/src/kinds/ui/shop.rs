//! The shop's catalogue: what the Shop's pages list, read from the build's reward assets and ProductData bundles
//! (the decode of the shop widgets). The build's shop answers come from a backend the port does
//! not have: only the 22 bundles carry prices in the paks, and which assets were on sale is the backend's answer. The
//! catalogue stands in for it from the assets themselves: a single item is listed when its own UnlockLocation is Shop
//! and it is not marked IsPurchasable false (ledger S3), a bundle when it is a real ProductData of the Bundles folder
//! (the test products are left out, S8).
//!
//! Not a kind of the store: the `ui` kind's menu bundle holds it (`MenuData::shop`) and the menu's shop reads it. Every
//! picture is a build path whose package the build holds; [`Shop::pictures`] lists them for the bundle's refs.

use std::cell::RefCell;
use std::collections::{HashMap, HashSet};

use indexmap::IndexMap;
use mvs_uasset::locres::LocresTable;
use mvs_uasset::value::{Props, TextValue, Value};
use serde::Serialize;

use super::rows;
use crate::fighter_port::pkg_of;
use crate::kind::{Cx, KindError};
use crate::kinds::cosmetics::{asset_offered, enum_tail, localized, reward_type};
use crate::objview::{ObjView, field, num, short_name, string};

const PANDA_MAIN: &str = "/Game/Panda_Main";
const BUNDLE_FOLDER: &str = "/Game/Panda_Main/UI/MainMenu/WWShop/Products/Bundles";
const ITEM_SETTINGS: &str = "/Game/Panda_Main/UI/MainMenu/WWShop/ItemSettings";
const DEFAULT_SHOP_IMAGE: &str = "/Game/Panda_Main/UI/MainMenu/WWShop/MI_WW_DefaultShopImage";

const SCRIPT: &str = "/Script/MultiVersus.";
const PRODUCT_CLASS: &str = "/Script/MultiVersus.ProductData";
const CHARACTER_CLASS: &str = "/Script/MultiVersus.CharacterData";

/// The classes a tab lists besides the skins (which come from each CharacterData's Skins) and the characters, with
/// the tag the tab table's IncludedTags name for them (ledger S2: the port's stand-in for Hydra's product tags).
const LISTED_CLASSES: &[(&str, &str)] = &[
    ("RingOutVfxData", "ring_out_vfx"),
    ("AnnouncerPackData", "announcer_pack"),
    ("BannerData", "banner"),
    ("ProfileIconData", "profile_icon"),
    ("StatTrackingBundleData", "stat_tracking_bundle"),
    ("EmoteData", "emote"),
    ("TauntData", "taunt"),
];

/// A text of the build: its string in the game's culture and its key, so the shop can show either (a text key is never
/// typed in the code).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
pub struct ShopText {
    /// The localized string (the text's source where the culture's table lacks the key).
    pub text: String,
    pub namespace: String,
    pub key: String,
}

/// One price of a ProductData (`Prices`): the currency and the amount.
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct ShopPrice {
    /// ECurrencyType's tail (`gems`).
    pub currency: String,
    pub amount: f64,
}

/// A thing the shop can list or a bundle holds: a skin, a ring-out, an emote, a taunt, a banner, a profile icon, an
/// announcer pack, a badge, a fighter, or what only a bundle holds (a timed boost). The shop's ShopProduct of a
/// single item is built from it.
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct ShopItem {
    /// The asset's Slug: the product slug of the port's stand-in, and what a bundle names it by.
    pub slug: String,
    /// The asset's object path (the InventoryDefinition's ContentAssetPath).
    pub path: String,
    /// The asset's class name (`SkinData`): the cell's `Get Item Type` casts.
    pub class: String,
    /// ERewardType's byte (`Get Item Type`): picks the type text, the cell's height (1, 15, 16 are tall) and the
    /// owned wording.
    pub kind: u32,
    /// The stand-in's product tag (the tab table's IncludedTags name: `skin`, `taunt`, `emote`, `ring_out_vfx`,
    /// `banner`, `profile_icon`, `announcer_pack`, `stat_tracking_bundle`, `character`); empty for what no tab lists.
    pub tag: String,
    /// DisplayName in the game's culture: the cell's MainTitle.
    pub name: String,
    /// ERewardRarity's tail (`None` when unset): the rarity stripe and the "{rarity} {type}" line.
    pub rarity: String,
    /// AssociatedCharacter's EFighter tail: the "{fighter} {type}" line, the Has Character filter and the fighter sort.
    pub fighter: Option<String>,
    /// AssociatedUniverse's tail: the universe dropdown of the filter grid.
    pub universe: Option<String>,
    /// UnlockLocation's tail (`None` when unset).
    pub unlock: String,
    /// IsPurchasable is not false (SkinData only; the class default is true).
    pub purchasable: bool,
    /// The standing rule that puts the item on a tab: UnlockLocation Shop, offered, and purchasable (S3). An item a
    /// bundle holds that is not listed is here for the bundle's details only.
    pub listed: bool,
    /// ShopCellOverrideMaterial: the cell's picture where set.
    pub cell_material: Option<String>,
    /// ShopModalOverrideMaterial: the details modal's art where set (the skins that carry one).
    pub modal_material: Option<String>,
    /// The picture the cell draws, resolved by the cell's rules (s2.4): the cell override, else by type the skin's
    /// LocalPlay portrait or variant icon, the ring-out's, taunt's and emote's thumbnail material, the banner's
    /// thumbnail, the fighter's select portrait or first skin, the announcer's character's first skin, else the
    /// thumbnail (drawn through `MI_WW_DefaultShopImage`: [`Shop::default_image`]). A material or a texture of the
    /// bundle.
    pub image: Option<String>,
    /// SkinData VariantIconMaterial.
    pub variant_icon: Option<String>,
    /// SkinData VariantLocalPlayPortraitMaterial.
    pub local_play_portrait: Option<String>,
    /// RewardThumbnail (a texture).
    pub thumbnail: Option<String>,
    /// RewardThumbnailMaterial.
    pub thumbnail_material: Option<String>,
    /// RingOutVfxData ToolTipPreviewMaterial: the details modal's ring-out art.
    pub tooltip_preview: Option<String>,
}

/// One item of a bundle: its slug in [`Shop::items`] and the quantity.
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct ShopBundleItem {
    pub slug: String,
    pub quantity: f64,
}

/// A ProductData bundle of the build (s3.2).
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct ShopBundle {
    /// Slug: the product slug.
    pub slug: String,
    /// The asset's object path.
    pub path: String,
    /// DisplayName: the cell's MainTitle and the details modal's title.
    pub name: ShopText,
    /// BundleSubText: the cell's description line, DisplayName when unset (no bundle of the build sets it).
    pub sub_text: Option<ShopText>,
    /// ProductDescription (the asset's plain string).
    pub description: String,
    /// Prices: the bundles' gem price (CurrencyType gems).
    pub prices: Vec<ShopPrice>,
    /// Items, resolved to the slugs of [`Shop::items`].
    pub items: Vec<ShopBundleItem>,
    /// AssociatedCharacter's EFighter tail (the fighter bundles).
    pub fighter: Option<String>,
    /// AssociatedUniverse's tail.
    pub universe: Option<String>,
    /// ShopCellOverrideMaterial: the cell's picture.
    pub cell_material: Option<String>,
    /// UnlockLocation's tail (`None` when unset).
    pub unlock: String,
    /// RequiresAllOwned: a bundle is owned when all its items are (else when any is).
    pub requires_all_owned: bool,
    /// PurchaseOnce.
    pub purchase_once: bool,
    /// Visible and AvailableForPurchase, as the asset sets them (`None`: unset, the native class default).
    pub visible: Option<bool>,
    pub available_for_purchase: Option<bool>,
    /// IsEnabledInDevelopmentEnvironment as the asset sets it (`None`: unset).
    pub enabled_in_development: Option<bool>,
}

impl ShopBundle {
    /// The price in `currency` (`gems`), `None` when the bundle has none in it.
    pub fn price(&self, currency: &str) -> Option<f64> {
        self.prices.iter().find(|p| p.currency == currency).map(|p| p.amount)
    }
}

/// A ShopCellItemSettingsRowStructure's palette, linear RGBA each.
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct ShopPalette {
    pub background: [f64; 4],
    pub foreground: [f64; 4],
    pub button: [f64; 4],
    pub stripe: [f64; 4],
    pub text: [f64; 4],
}

/// A row of the ItemSettings DataTable (`GetItemSettingsFromItem`: the product slug's row, else `DEFAULT`).
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct ShopItemSettings {
    /// UseCustomPalette: the cell's colours come from `palette` (else from the central palette).
    pub use_custom_palette: bool,
    pub palette: ShopPalette,
    /// ImageOffsets (X, Y, Z, W): the picture's UV window in a normal cell.
    pub image_offsets: [f64; 4],
    /// DifferentOffsetsForLarge: a large cell uses `image_offsets_large`.
    pub different_offsets_for_large: bool,
    pub image_offsets_large: [f64; 4],
}

/// The shop catalogue of the menu bundle.
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct Shop {
    /// Every item the shop lists or a bundle holds: the listed ones in tab order (skins by fighter, then each
    /// class's registry order), then the bundles' unlisted ones.
    pub items: Vec<ShopItem>,
    /// The 22 real bundles, in the asset registry's order.
    pub bundles: Vec<ShopBundle>,
    /// The ItemSettings rows by row name.
    pub settings: IndexMap<String, ShopItemSettings>,
    /// `MI_WW_DefaultShopImage`: the material a thumbnail texture is drawn through when a cell has no better picture.
    pub default_image: Option<String>,
}

impl Shop {
    /// Every build path of a picture the catalogue names, for the bundle's refs.
    pub fn pictures(&self) -> Vec<String> {
        let mut out: Vec<String> = Vec::new();
        for it in &self.items {
            out.extend(
                [
                    &it.cell_material,
                    &it.modal_material,
                    &it.image,
                    &it.variant_icon,
                    &it.local_play_portrait,
                    &it.thumbnail,
                    &it.thumbnail_material,
                    &it.tooltip_preview,
                ]
                .into_iter()
                .flatten()
                .cloned(),
            );
        }
        out.extend(self.bundles.iter().filter_map(|b| b.cell_material.clone()));
        out.extend(self.default_image.clone());
        let mut seen = HashSet::new();
        out.retain(|p| seen.insert(p.clone()));
        out
    }

    /// The item of a slug.
    pub fn item(&self, slug: &str) -> Option<&ShopItem> {
        self.items.iter().find(|i| i.slug == slug)
    }

    /// The listed items of a tag, in catalogue order.
    pub fn listed(&self, tag: &str) -> impl Iterator<Item = &ShopItem> {
        self.items.iter().filter(move |i| i.listed && i.tag == tag)
    }
}

/// A path value that names something: not empty, not `None`.
fn path_of(cx: &Cx<'_>, v: Option<&Value>) -> Option<String> {
    v.and_then(|v| string(cx.cache, v)).filter(|s| !s.is_empty() && s != "None")
}

fn tail_of(p: &Props, name: &str) -> Option<String> {
    let t = enum_tail(field(p, name));
    (!t.is_empty() && t != "None").then_some(t)
}

fn flag(p: &Props, name: &str) -> Option<bool> {
    match field(p, name) {
        Some(Value::Bool(b)) => Some(*b),
        _ => None,
    }
}

/// The class defaults over an object's properties, else its properties (`o.defaults ?? o.properties`).
fn props_of(o: &ObjView<'_>) -> Result<Props, KindError> {
    Ok(match o.defaults()? {
        Some(d) => d,
        None => o.props().clone(),
    })
}

fn failed<T>(msg: String) -> Result<T, KindError> {
    Err(KindError::Failed(format!("shop: {msg}")))
}

/// A text with its key: the table's string, else the source.
fn shop_text(table: &LocresTable, v: Option<&Value>) -> Option<ShopText> {
    match v {
        Some(Value::Text(TextValue::Base { namespace, key, .. })) => {
            Some(ShopText { text: localized(table, v), namespace: namespace.clone(), key: key.clone() })
        }
        Some(Value::Text(_)) => Some(ShopText { text: localized(table, v), ..ShopText::default() }),
        _ => None,
    }
}

/// An asset as the catalogue reads it.
struct Asset {
    path: String,
    pkg: String,
    class: String,
    p: Props,
}

struct Reader<'a, 'c> {
    cx: &'c Cx<'a>,
    text: &'c LocresTable,
    /// Which packages the build holds (lower-cased), asked once each.
    held: RefCell<HashMap<String, bool>>,
    /// EFighter tail -> the VariantIconMaterial of the character's first skin (the announcer's and the fighter's
    /// picture).
    first_skin: HashMap<String, Option<String>>,
}

impl<'a> Reader<'a, '_> {
    fn has(&self, pkg: &str) -> bool {
        if pkg.is_empty() {
            return false;
        }
        *self.held.borrow_mut().entry(pkg.to_lowercase()).or_insert_with(|| self.cx.exists(pkg))
    }

    /// A picture: the reference's object path when its package is in the build.
    fn pic(&self, p: &Props, name: &str) -> Option<String> {
        let s = path_of(self.cx, field(p, name))?;
        self.has(pkg_of(&s)).then_some(s)
    }

    fn at(&self, r: &str) -> Result<Asset, KindError> {
        let pkg = pkg_of(r).to_owned();
        let o = self.cx.object(&pkg).map_err(|e| KindError::Failed(format!("shop: {pkg} was not read: {e}")))?;
        Ok(Asset {
            path: if r == pkg { o.path().to_owned() } else { r.to_owned() },
            pkg,
            class: o.class().to_owned(),
            p: props_of(&o)?,
        })
    }

    fn item(&self, a: &Asset, tag: &str, listed_rule: bool) -> ShopItem {
        let p = &a.p;
        let class = short_name(&a.class).to_owned();
        let slug = path_of(self.cx, field(p, "Slug")).unwrap_or_default();
        let kind = reward_type(&class, field(p, "Slug"));
        let fighter = tail_of(p, "AssociatedCharacter");
        let purchasable = flag(p, "IsPurchasable") != Some(false);
        let unlock = enum_tail(field(p, "UnlockLocation"));
        let listed = listed_rule && asset_offered(p) && unlock == "Shop" && purchasable;
        let cell_material = self.pic(p, "ShopCellOverrideMaterial");
        let variant_icon = self.pic(p, "VariantIconMaterial");
        let local_play_portrait = self.pic(p, "VariantLocalPlayPortraitMaterial");
        let thumbnail = self.pic(p, "RewardThumbnail");
        let thumbnail_material = self.pic(p, "RewardThumbnailMaterial");
        let first_skin = |f: &Option<String>| f.as_ref().and_then(|f| self.first_skin.get(f)).cloned().flatten();
        // The cell's picture (WW_ShopItemCellBase Load Item Image @133, @6409-11300): the override, else by type.
        let by_type = match kind {
            1 => local_play_portrait.clone().or_else(|| variant_icon.clone()),
            6 => thumbnail_material.clone().or_else(|| thumbnail.clone()),
            2 | 8 => thumbnail_material.clone(),
            15 => thumbnail.clone(),
            9 => self.pic(p, "CharacterSelectPortraitTexture").or_else(|| first_skin(&fighter)),
            11 => first_skin(&fighter),
            _ => None,
        };
        let image = cell_material.clone().or(by_type).or_else(|| thumbnail.clone());
        // a cell with nothing to draw is no product (the placeholder CharacterData of the build)
        let listed = listed && image.is_some();
        ShopItem {
            slug,
            path: a.path.clone(),
            class,
            kind,
            tag: tag.to_owned(),
            name: localized(self.text, field(p, "DisplayName")),
            rarity: {
                let r = enum_tail(field(p, "Rarity"));
                if r.is_empty() { "None".into() } else { r }
            },
            fighter,
            universe: tail_of(p, "AssociatedUniverse"),
            unlock: if unlock.is_empty() { "None".into() } else { unlock },
            purchasable,
            listed,
            cell_material,
            modal_material: self.pic(p, "ShopModalOverrideMaterial"),
            image,
            variant_icon,
            local_play_portrait,
            thumbnail,
            thumbnail_material,
            tooltip_preview: self.pic(p, "ToolTipPreviewMaterial"),
        }
    }
}

/// The ItemSettings DataTable's rows.
fn settings(cx: &Cx<'_>) -> Result<IndexMap<String, ShopItemSettings>, KindError> {
    let table = cx.object(ITEM_SETTINGS)?;
    let v4 = |s: &Props, n: &str| -> [f64; 4] {
        match field(s, n) {
            Some(Value::Struct(c)) => ["R", "G", "B", "A"].map(|k| field(c, k).and_then(num).unwrap_or(0.0)),
            _ => [0.0; 4],
        }
    };
    let off = |s: &Props, n: &str| -> [f64; 4] {
        match field(s, n) {
            Some(Value::Struct(c)) => ["X", "Y", "Z", "W"].map(|k| field(c, k).and_then(num).unwrap_or(0.0)),
            _ => [0.0; 4],
        }
    };
    let mut out = IndexMap::new();
    for (name, row) in rows(&table)? {
        let Some(Value::Struct(s)) = field(&row, "ItemSettings") else {
            return failed(format!("ItemSettings row {name}: no ItemSettings struct"));
        };
        let palette = match field(s, "CustomPalette") {
            Some(Value::Struct(c)) => ShopPalette {
                background: v4(c, "background"),
                foreground: v4(c, "foreground"),
                button: v4(c, "button"),
                stripe: v4(c, "stripe"),
                text: v4(c, "text"),
            },
            _ => ShopPalette::default(),
        };
        out.insert(
            name,
            ShopItemSettings {
                use_custom_palette: flag(s, "UseCustomPalette") == Some(true),
                palette,
                image_offsets: off(s, "ImageOffsets"),
                different_offsets_for_large: flag(s, "DifferentOffsetsForLarge") == Some(true),
                image_offsets_large: off(s, "ImageOffsetsLarge"),
            },
        );
    }
    Ok(out)
}

/// The shop's catalogue from the build's assets.
pub fn read_shop(cx: &Cx<'_>, text: &LocresTable) -> Result<Shop, KindError> {
    let mut r = Reader { cx, text, held: RefCell::new(HashMap::new()), first_skin: HashMap::new() };

    // every CharacterData of the build, the port's fighters or not (U6)
    let char_pkgs = cx.catalog.list_packages(PANDA_MAIN, true, Some(CHARACTER_CLASS));
    let characters: Vec<Asset> = char_pkgs.iter().map(|p| r.at(p)).collect::<Result<_, _>>()?;
    for c in &characters {
        let Some(f) = tail_of(&c.p, "AssociatedCharacter") else { continue };
        let first = match field(&c.p, "Skins") {
            Some(Value::Array(a)) => a.first().and_then(|v| string(cx.cache, v)).filter(|s| !s.is_empty()),
            _ => None,
        };
        let icon = match first.filter(|s| r.has(pkg_of(s))) {
            Some(s) => r.pic(&r.at(&s)?.p, "VariantIconMaterial"),
            None => None,
        };
        r.first_skin.entry(f).or_insert(icon);
    }

    let mut items: Vec<ShopItem> = Vec::new();
    let mut by_slug: HashMap<String, usize> = HashMap::new();
    let mut add = |items: &mut Vec<ShopItem>, it: ShopItem| {
        if it.slug.is_empty() {
            return;
        }
        match by_slug.get(&it.slug) {
            Some(i) => items[*i].listed |= it.listed,
            None => {
                by_slug.insert(it.slug.clone(), items.len());
                items.push(it);
            }
        }
    };

    // skins (the Variants tab), fighter by fighter through the CharacterData's Skins
    for c in &characters {
        if let Some(Value::Array(skins)) = field(&c.p, "Skins") {
            for v in skins {
                let Some(s) = string(cx.cache, v).filter(|s| !s.is_empty() && r.has(pkg_of(s))) else { continue };
                let a = r.at(&s)?;
                let it = r.item(&a, "skin", true);
                if it.listed {
                    add(&mut items, it);
                }
            }
        }
    }
    // the other classes, in the registry's order
    for (class, tag) in LISTED_CLASSES {
        for p in cx.catalog.list_packages(PANDA_MAIN, true, Some(&format!("{SCRIPT}{class}"))) {
            let it = r.item(&r.at(&p)?, tag, true);
            if it.listed {
                add(&mut items, it);
            }
        }
    }
    // the fighters (the Fighters tab)
    for c in &characters {
        let it = r.item(c, "character", true);
        if it.listed {
            add(&mut items, it);
        }
    }

    // the bundles
    let mut bundles = Vec::new();
    for p in cx.catalog.list_packages(BUNDLE_FOLDER, false, Some(PRODUCT_CLASS)) {
        let a = r.at(&p)?;
        let q = &a.p;
        let mut held = Vec::new();
        if let Some(Value::Array(list)) = field(q, "Items") {
            for e in list {
                let Value::Struct(e) = e else { continue };
                let Some(path) = path_of(cx, field(e, "Item")) else { continue };
                // an item whose package the build does not ship is dropped
                if !r.has(pkg_of(&path)) {
                    continue;
                }
                let ia = r.at(&path)?;
                let class = short_name(&ia.class);
                let tag = match class {
                    "SkinData" => "skin",
                    "CharacterData" => "character",
                    c => LISTED_CLASSES.iter().find(|(k, _)| *k == c).map_or("", |(_, t)| t),
                };
                let it = r.item(&ia, tag, false);
                if it.slug.is_empty() {
                    return failed(format!("bundle {}: item {path} has no Slug", a.pkg));
                }
                let slug = it.slug.clone();
                add(&mut items, it);
                held.push(ShopBundleItem { slug, quantity: field(e, "Quantity").and_then(num).unwrap_or(1.0) });
            }
        }
        let prices = match field(q, "Prices") {
            Some(Value::Array(list)) => list
                .iter()
                .filter_map(Value::as_struct)
                .map(|s| ShopPrice {
                    currency: enum_tail(field(s, "CurrencyType")),
                    amount: field(s, "Price").and_then(num).unwrap_or(0.0),
                })
                .collect(),
            _ => Vec::new(),
        };
        let unlock = enum_tail(field(q, "UnlockLocation"));
        bundles.push(ShopBundle {
            slug: path_of(cx, field(q, "Slug")).unwrap_or_default(),
            path: a.path.clone(),
            name: shop_text(text, field(q, "DisplayName")).unwrap_or_default(),
            sub_text: shop_text(text, field(q, "BundleSubText")).filter(|t| !t.text.is_empty()),
            description: path_of(cx, field(q, "ProductDescription")).unwrap_or_default(),
            prices,
            items: held,
            fighter: tail_of(q, "AssociatedCharacter"),
            universe: tail_of(q, "AssociatedUniverse"),
            cell_material: r.pic(q, "ShopCellOverrideMaterial"),
            unlock: if unlock.is_empty() { "None".into() } else { unlock },
            requires_all_owned: flag(q, "RequiresAllOwned") == Some(true),
            purchase_once: flag(q, "PurchaseOnce") == Some(true),
            visible: flag(q, "Visible"),
            available_for_purchase: flag(q, "AvailableForPurchase"),
            enabled_in_development: flag(q, "IsEnabledInDevelopmentEnvironment"),
        });
    }
    if bundles.is_empty() {
        return failed(format!("no ProductData under {BUNDLE_FOLDER}"));
    }
    let default_image = r.has(DEFAULT_SHOP_IMAGE).then(|| format!("{DEFAULT_SHOP_IMAGE}.MI_WW_DefaultShopImage"));
    Ok(Shop { items, bundles, settings: settings(cx)?, default_image })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_text_keeps_its_key() {
        let mut t = LocresTable::new();
        t.entry("".into()).or_default().insert("K".into(), "Hallo".into());
        let v = Value::Text(TextValue::Base { source: "Hello".into(), namespace: "".into(), key: "K".into() });
        assert_eq!(
            shop_text(&t, Some(&v)),
            Some(ShopText { text: "Hallo".into(), namespace: "".into(), key: "K".into() })
        );
        assert_eq!(shop_text(&t, None), None);
    }

    /// The catalogue over the developer's paks (`MVS_PAKS`): one bundle and one skin read to the field, the counts per
    /// tab printed (`--nocapture`).
    #[test]
    #[ignore = "needs the paks"]
    fn the_catalogue_over_the_paks() {
        use std::path::PathBuf;
        use std::sync::Arc;

        use mvs_pak::{BuildPaths, pak_files_in, setup};
        use mvs_uasset::PackageCache;
        use mvs_uasset::package::FileSource;

        use crate::catalog::scan_catalog;
        use crate::kind::Shared;
        use crate::kinds::text;
        use crate::reads::StoreReads;

        let dir = PathBuf::from(std::env::var_os("MVS_PAKS").expect("set MVS_PAKS"));
        let key = setup::key_from_exe(&setup::exe_beside_paks(&dir)).unwrap();
        let set = Arc::new(mvs_pak::PakSet::open(&pak_files_in(&dir).unwrap(), &key).unwrap());
        let paths = Arc::new(BuildPaths::from_paks(&set).unwrap());
        let cache = PackageCache::new(set.clone() as Arc<dyn FileSource>, paths.clone(), 1 << 30);
        let (catalog, _) = scan_catalog(&set, &paths, &|_| {});
        let shared = Shared::default();
        let reads = StoreReads::none();
        let cx = Cx { paks: &set, paths: &paths, cache: &cache, catalog: &catalog, shared: &shared, store: &reads };
        let table = text::table(&cx, text::GAME_CULTURE).unwrap();
        let shop = read_shop(&cx, &table).unwrap();

        assert_eq!(shop.bundles.len(), 22);
        let arya = shop.bundles.iter().find(|b| b.slug == "bundle_arya").expect("Bundle_Arya");
        assert_eq!(arya.name.text, "Arya Bundle");
        assert_eq!(arya.name.key, "D0A946C248AC36C0A2F15BBFBC437B17");
        assert_eq!(arya.prices, [ShopPrice { currency: "gems".into(), amount: 1500.0 }]);
        assert_eq!(arya.fighter.as_deref(), Some("Arya"));
        assert!(
            arya.cell_material.as_deref().is_some_and(|m| m.ends_with("MI_Arya_Store_Banner.MI_Arya_Store_Banner"))
        );
        assert!(arya.sub_text.is_none() && !arya.requires_all_owned && !arya.purchase_once);
        let held: Vec<&str> = arya.items.iter().map(|i| i.slug.as_str()).collect();
        assert_eq!(held[0], "character_arya");
        assert!(shop.item(held[1]).is_some_and(|s| s.kind == 1 && !s.purchasable && !s.listed));

        let s02 = shop.item("skin_c006_s02").expect("Arya_S02");
        assert_eq!((s02.kind, s02.tag.as_str(), s02.rarity.as_str(), s02.unlock.as_str()), (1, "skin", "Rare", "Shop"));
        assert_eq!(s02.fighter.as_deref(), Some("Arya"));
        assert_eq!(s02.universe.as_deref(), Some("GameOfThrones"));
        assert!(!s02.purchasable && !s02.listed && s02.modal_material.is_some());
        assert_eq!(s02.image, s02.cell_material);
        assert!(shop.listed("skin").all(|s| s.purchasable && s.unlock == "Shop" && s.image.is_some()));
        assert!(shop.settings.contains_key("DEFAULT"));

        let mut per_tag: IndexMap<&str, usize> = IndexMap::new();
        for i in shop.items.iter().filter(|i| i.listed) {
            *per_tag.entry(i.tag.as_str()).or_default() += 1;
        }
        eprintln!(
            "listed per tag: {per_tag:?}; unlisted bundle items: {}",
            shop.items.iter().filter(|i| !i.listed).count()
        );
        for b in &shop.bundles {
            eprintln!("bundle {} gems {:?} items {}", b.slug, b.price("gems"), b.items.len());
        }
    }

    #[test]
    fn pictures_are_listed_once() {
        let mut s = Shop::default();
        let it = |p: &str| ShopItem { cell_material: Some(p.into()), image: Some(p.into()), ..ShopItem::default() };
        s.items = vec![it("/Game/A.A"), it("/Game/B.B"), it("/Game/A.A")];
        s.default_image = Some("/Game/D.D".into());
        assert_eq!(s.pictures(), ["/Game/A.A", "/Game/B.B", "/Game/D.D"]);
    }
}
