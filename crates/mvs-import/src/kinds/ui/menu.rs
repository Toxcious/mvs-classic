//! The menu bundle's data: the Lab's
//! fighters with their skins, Move Lists and perk lists, the character select's roster, the class infos, the maps,
//! the perks, the mastery track, the home screen's promotions, the glossary, and the cosmetics' tile pictures, with
//! what they add to the bundle's refs and soft textures. U2's to port; the bundle (`ui::mod`) calls it and writes the
//! fields into the menu entry in its order.

use indexmap::IndexMap;
use mvs_uasset::locres::LocresTable;
use mvs_uasset::value::{Props, TextValue, Value};
use serde::Serialize;

use super::shop::{Shop, read_shop};
use super::tables::{GLOSSARY_TABLES, OFFLINE_MAPS, OFFLINE_MAPS_LOCAL, PROMOTIONS};
use super::{INSTANCE, PANDA_MAIN, Refs, TEXTURE, base_name, material_class, pkg_name, pkg_of, rows, source, tail};
use crate::fighter_port::{CHARACTER_DATA, CHARACTERS, fighter_port, port_default_skin};
use crate::game::exported;
use crate::json::Json;
use crate::kind::{Cx, Item, Kind, KindError};
use crate::kinds::cosmetics::{Cosmetics, mastery_tiers, read_cosmetics, reward_tier, reward_type};
use crate::kinds::literals::Literals;
use crate::objview::{ObjView, field, num, short_name, string, truthy};

/// A perk the menu lists (`hud/slate` PerkInfo). The bundle clears `icon` where the texture is not cooked.
#[derive(Clone, Debug, Serialize)]
pub struct PerkInfo {
    pub slug: String,
    pub name: String,
    pub description: String,
    pub stacked: String,
    pub category: u32,
    pub level: String,
    pub icon: Option<String>,
    pub fighter: Option<String>,
}

/// The menu entry's fields of [`menu_data`], each as the entry writes it.
#[derive(Clone, Debug, Default)]
pub struct MenuData {
    /// key -> FighterInfo with its moves, background colour and perk lists.
    pub fighters: IndexMap<String, Json>,
    /// RosterEntry[], in the character select's order.
    pub roster: Vec<Json>,
    pub class_info: IndexMap<String, Json>,
    /// map asset name -> MapData.
    pub maps: IndexMap<String, Json>,
    pub perks: IndexMap<String, PerkInfo>,
    /// `{ levels, rewards }`.
    pub mastery: Option<Json>,
    /// `{ slug, widget }[]`.
    pub promotions: Vec<Json>,
    pub promo_widgets: IndexMap<String, Json>,
    /// table -> GlossaryRow[].
    pub glossary: IndexMap<String, Json>,
    /// UI_GameplayTipDataTable: each row's Tip as its Game.locres key, in row order.
    pub gameplay_tips: Vec<String>,
    /// The cosmetics catalogue.
    pub cosmetics: Cosmetics,
    /// The shop's catalogue: the single items and the bundles the Shop lists (`shop.rs`).
    pub shop: Shop,
    /// The account level table, one row per level.
    pub account_levels: Vec<AccountLevelRow>,
    /// The arcade bots' name literals and boss skins.
    pub arcade_names: ArcadeNames,
}

/// One row of AccountLevelUpTable.LevelUpData: the XP of the level it stands for (row level - 1) and its gold reward
#[derive(Clone, Debug, Default, Serialize)]
pub struct AccountLevelRow {
    pub xp: f64,
    pub gold: f64,
}

/// GetArcadeBotLoadouts' literals and the CoOpVsAI manager's boss skins.
#[derive(Clone, Debug, Default, Serialize)]
pub struct ArcadeNames {
    /// The rival's name prefix (@5065), used as it is: its trailing spaces belong to it.
    pub nemesis_prefix: String,
    /// The Mini (@5145).
    pub mini: String,
    /// Reign Dog (@5182).
    pub reindog: String,
    /// Giant Gizmo (@5216).
    pub giant_gizmo: String,
    /// The text for an empty DisplayName (@4227).
    pub empty_name: String,
    /// ReindogBossSkin and GizmoBossSkin: SkinData asset names.
    pub reindog_boss_skin: String,
    pub gizmo_boss_skin: String,
}

const MAP_DATA_FOLDER: &str = "/Game/Panda_Main/Maps/MapDataAssets";
const MAP_DATA_CLASS: &str = "/Script/MultiVersus.MapData";
const PERK_DATA_FOLDER: &str = "/Game/Panda_Main/Blueprints/Perks/PerkData";
const PERK_DATA_CLASS: &str = "/Script/MultiVersus.PerkData";
const PGI_GLOBAL_DATA: &str = "/Game/Panda_Main/Blueprints/PandaGameInstance/PGI_GlobalData.PGI_GlobalData_C";

/// A perk category's number (`CATEGORY`).
fn category(name: &str) -> Option<u32> {
    ["Offense", "Defense", "Utility", "CharacterSpecific"].iter().position(|c| *c == name).map(|i| i as u32)
}

fn s(v: &str) -> Json {
    Json::Str(v.to_owned())
}

/// An object of the members that are set: a member left `undefined` is not written.
fn jobj<const N: usize>(members: [(&str, Option<Json>); N]) -> Json {
    Json::object(members.into_iter().filter_map(|(k, v)| v.map(|v| (k, v))))
}

fn failed<T>(msg: String) -> Result<T, KindError> {
    Err(KindError::Failed(format!("ui: {msg}")))
}

/// A property as the string the object kind's JSON holds (a name, a reference's path, a soft path).
pub(super) fn pstr(cx: &Cx<'_>, p: &Props, name: &str) -> Option<String> {
    field(p, name).and_then(|v| string(cx.cache, v))
}

/// Like [`pstr`], as the JSON member: unset leaves it out.
fn jstr(cx: &Cx<'_>, p: &Props, name: &str) -> Option<Json> {
    pstr(cx, p, name).map(Json::Str)
}

fn jnum(p: &Props, name: &str) -> Option<Json> {
    field(p, name).and_then(num).map(Json::Num)
}

/// A property that must be a reference or a name (it is cast `as string` and read on).
fn need(cx: &Cx<'_>, p: &Props, name: &str, what: &str) -> Result<String, KindError> {
    pstr(cx, p, name).map_or_else(|| failed(format!("{what}: no {name}")), Ok)
}

fn strings(cx: &Cx<'_>, p: &Props, name: &str) -> Vec<String> {
    match field(p, name) {
        Some(Value::Array(a)) => a.iter().filter_map(|v| string(cx.cache, v)).collect(),
        _ => Vec::new(),
    }
}

/// `cdoPath`: `/Game/A/B.B_C` -> `/Game/A/B.Default__B_C`.
pub(super) fn cdo_path(cls: &str) -> String {
    let dot = cls.rfind('.').unwrap_or(0);
    format!("{}.Default__{}", &cls[..dot], &cls[dot + 1..])
}

/// The class defaults over the properties, else the properties (`o.defaults ?? o.properties`).
fn defaults_or_props(o: &ObjView<'_>) -> Result<Props, KindError> {
    Ok(match o.defaults()? {
        Some(d) => d,
        None => o.props().clone(),
    })
}

/// Every string of a value that names an object of the game or the engine (`stringsOf`: the references a table row
/// holds).
fn strings_of(cx: &Cx<'_>, v: &Value, out: &mut Vec<String>) {
    match v {
        Value::Str(_) | Value::Ref(Some(_)) | Value::SoftPath { .. } => {
            if let Some(t) = string(cx.cache, v).filter(|t| is_object_path(t)) {
                out.push(t);
            }
        }
        Value::Array(a) => a.iter().for_each(|x| strings_of(cx, x, out)),
        Value::Struct(p) => p.values().for_each(|x| strings_of(cx, x, out)),
        Value::Map(m) => m.iter().for_each(|(k, x)| {
            strings_of(cx, k, out);
            strings_of(cx, x, out);
        }),
        _ => {}
    }
}

/// `^\/(Game|Engine)\/.+\..+`
fn is_object_path(t: &str) -> bool {
    let rest = t.strip_prefix("/Game/").or_else(|| t.strip_prefix("/Engine/"));
    rest.is_some_and(|r| r.char_indices().any(|(i, c)| c == '.' && i >= 1 && i + 1 < r.len()))
}

/// The build's English of a text (`english` of the glossary): the table's string by namespace and key.
fn english(en_text: &LocresTable, v: Option<&Value>, table: &str, what: &str) -> Result<String, KindError> {
    if let Some(Value::Text(TextValue::Base { namespace, key, .. })) = v {
        if let Some(t) = en_text.get(namespace).and_then(|n| n.get(key)) {
            return Ok(t.clone());
        }
        return failed(format!("glossary {table} {what}: no English string for {namespace}/{key}"));
    }
    failed(format!("glossary {table} {what}: no English string for undefined/undefined"))
}

/// The menu's data. `chars` are the roster's keys with their CharacterData objects (`chars`); `en_text` the build's
/// English table; `refs`, `soft_tex` (texture package name -> path) and `cos_materials` (the cosmetics' material
/// names, in order) are the bundle's, added to here. The promotions' widgets are read as objects (`Cx::object`), not
/// through the widget reader.
pub fn menu_data<'a>(
    cx: &Cx<'a>,
    chars: &[(String, ObjView<'a>)],
    en_text: &LocresTable,
    refs: &mut Refs,
    soft_tex: &mut IndexMap<String, String>,
    cos_materials: &mut Vec<String>,
) -> Result<MenuData, KindError> {
    let mut out = MenuData::default();
    maps(cx, refs, soft_tex, &mut out)?;
    fighters(cx, chars, refs, soft_tex, &mut out)?;
    perks(cx, chars, soft_tex, &mut out)?;
    roster(cx, refs, &mut out)?;
    mastery(cx, refs, soft_tex, &mut out)?;
    class_info(cx, refs, &mut out)?;
    promotions(cx, soft_tex, &mut out)?;
    out.shop = read_shop(cx, en_text)?;
    out.cosmetics = cosmetic_pictures(cx, refs, soft_tex, cos_materials, out.shop.pictures())?;
    glossary(cx, en_text, &mut out)?;
    gameplay_tips(cx, en_text, &mut out)?;
    account_levels(cx, &mut out)?;
    arcade_names(cx, en_text, &mut out)?;
    Ok(out)
}

/// The maps by asset name; the Lab's first (the training config's SelectedMap).
fn maps(
    cx: &Cx<'_>,
    refs: &mut Refs,
    soft_tex: &mut IndexMap<String, String>,
    out: &mut MenuData,
) -> Result<(), KindError> {
    let training = cx.object(&exported(&format!("{MAP_DATA_FOLDER}/map_training")))?;
    let ttex = need(cx, training.props(), "Texture", "map_training")?;
    refs.set(pkg_name(&ttex), &ttex, TEXTURE);
    let mut list = cx.catalog.list_packages(MAP_DATA_FOLDER, true, Some(MAP_DATA_CLASS));
    list.sort();
    for p in &list {
        let d = cx.object(&exported(pkg_of(p)))?;
        let d = d.props();
        let t = need(cx, d, "Texture", p)?;
        soft_tex.insert(pkg_name(&t).to_owned(), t.clone());
        let level = pstr(cx, d, "Level").map(|l| short_name(&l).to_owned());
        let levels = strings(cx, d, "LabLevelsToLoad").iter().map(|l| s(short_name(l))).collect();
        let rotations = strings(cx, d, "MapRotations")
            .iter()
            .map(|r| {
                let v = Value::Str(r.clone());
                s(tail(Some(&v), "undefined"))
            })
            .collect();
        out.maps.insert(
            base_name(pkg_of(p)).to_owned(),
            jobj([
                ("displayName", Some(Json::Str(source(field(d, "DisplayName"))))),
                ("property", Some(Json::Str(source(field(d, "Property"))))),
                ("min", Some(jnum(d, "MinSupportedPlayers").unwrap_or(Json::Num(2.0)))),
                ("max", Some(jnum(d, "MaxSupportedPlayers").unwrap_or(Json::Num(4.0)))),
                ("experimental", Some(Json::Bool(field(d, "bIsExperimental").is_some_and(truthy)))),
                ("texture", Some(s(pkg_name(&t)))),
                ("level", level.map(Json::Str)),
                ("levels", Some(Json::Arr(levels))),
                ("rotations", Some(Json::Arr(rotations))),
                ("slug", jstr(cx, d, "Slug")),
            ]),
        );
    }
    for n in OFFLINE_MAPS.iter().chain(OFFLINE_MAPS_LOCAL.iter()) {
        if !out.maps.contains_key(*n) {
            return failed(format!("offline map without a MapData: {n}"));
        }
    }
    Ok(())
}

/// The Lab menu's fighters: the CharacterData's DisplayName and Slug, the port's default skin with its
/// VariantIconMaterial, the skin selector's tiles (the Skins that load, without the IsUnavailableOffline ones), the
/// Move List's rows and the BackgroundColor.
fn fighters<'a>(
    cx: &Cx<'a>,
    chars: &[(String, ObjView<'a>)],
    refs: &mut Refs,
    soft_tex: &mut IndexMap<String, String>,
    out: &mut MenuData,
) -> Result<(), KindError> {
    // RewardInfo DetermineRewardTier of a skin: the tier of the mastery level it is the first reward of
    let tiers = mastery_tiers(cx, &chars.iter().map(|(_, cd)| cd.props()).collect::<Vec<_>>());
    for (key, cd) in chars {
        let c = cd.props();
        let port = fighter_port(key).ok_or_else(|| KindError::Failed(format!("ui: no FIGHTER_PORT for {key}")))?;
        let def = base_name(&port_default_skin(cx, key, port, Some(cd))?).to_lowercase();
        let skin_refs = strings(cx, c, "Skins");
        let Some(s0) = skin_refs.iter().find(|r| base_name(pkg_of(r)).to_lowercase() == def) else {
            return failed(format!(
                "menu fighters: {key}: the port's default skin {def} is not in its CharacterData Skins"
            ));
        };
        let skin_obj = cx.object(s0)?;
        let skin = skin_obj.props();
        let icon = need(cx, skin, "VariantIconMaterial", s0)?;
        refs.set(pkg_name(&icon), &icon, INSTANCE);
        let mut skins = Vec::new();
        for r in &skin_refs {
            // a soft reference to a package the build does not ship loads as null (C019_S03)
            if !cx.exists(r) {
                continue;
            }
            let o = cx.object(r)?;
            let d = o.props();
            if field(d, "IsUnavailableOffline").is_some_and(truthy) {
                continue;
            }
            let vim = need(cx, d, "VariantIconMaterial", r)?;
            let m = refs.keyed(&vim, material_class(&vim));
            // VariantLocalPlayPortraitMaterial: the Local Play slot's thumbnail (UI_MatchSetup_Player @4043, @3878)
            let lp = pstr(cx, d, "VariantLocalPlayPortraitMaterial").filter(|l| !l.is_empty());
            let lp_key = lp.map(|lp| refs.keyed(&lp, material_class(&lp)));
            let is_default = base_name(pkg_of(r)).to_lowercase() == def;
            // RewardInfo CreateFromAsset's Thumbnail (@700), AssociatedCharacter (@588), UnlockLocation and
            // DetermineRewardTier: the skin tile's tooltip (WBP_VariantTooltip)
            let thumb = soft_texture(cx, refs, d, "RewardThumbnail")?;
            if let Some(t) = &thumb {
                soft_tex.insert(pkg_name(t).to_owned(), t.clone());
            }
            let character = tail(field(d, "AssociatedCharacter"), "");
            skins.push(jobj([
                ("name", Some(s(base_name(pkg_of(r))))),
                ("path", Some(s(r))),
                ("slug", jstr(cx, d, "Slug")),
                ("displayName", Some(Json::Str(source(field(d, "DisplayName"))))),
                ("icon", Some(Json::Str(m))),
                ("rarity", Some(s(tail(field(d, "Rarity"), "None")))),
                ("isDefault", Some(Json::Bool(is_default))),
                ("localPlayPortrait", lp_key.map(Json::Str)),
                ("thumb", thumb.as_deref().map(|t| s(pkg_name(t)))),
                ("character", (!character.is_empty()).then(|| s(character))),
                ("unlock", unlock_of(d)),
                ("tier", Some(Json::Num(f64::from(reward_tier(d, pkg_of(r), &tiers))))),
            ]));
        }
        // the Move List's texts (UI_CharacterMoveList UpdateAppearance @2782): the MoveList table's rows in order
        let move_list = need(cx, c, "MoveList", key)?;
        let mut moves = Json::obj();
        for (rn, r) in rows(&cx.object(&move_list)?)? {
            moves.set(
                rn,
                jobj([
                    ("title", Some(Json::Str(source(field(&r, "Title"))))),
                    ("description", Some(Json::Str(source(field(&r, "Description"))))),
                    ("outOfDate", Some(Json::Bool(field(&r, "OutOfDate").is_some_and(truthy)))),
                ]),
            );
        }
        let bg = match field(c, "BackgroundColor") {
            Some(Value::Struct(p)) => p.clone(),
            _ => return failed(format!("{key}: no BackgroundColor")),
        };
        // ArcadeProgressCharacterIconTexture: the rival's icon on the arcade progress pips (UI_AXProgressStage @1929)
        let arcade_icon = pstr(cx, c, "ArcadeProgressCharacterIconTexture").filter(|a| !a.is_empty());
        let portrait = collections_portrait(cx, refs, c)?;
        if let Some(a) = &arcade_icon {
            soft_tex.insert(pkg_name(a).to_owned(), a.clone());
        }
        out.fighters.insert(
            key.clone(),
            jobj([
                ("slug", jstr(cx, c, "Slug")),
                ("displayName", Some(Json::Str(source(field(c, "DisplayName"))))),
                (
                    "skin",
                    Some(jobj([
                        ("name", Some(s(base_name(pkg_of(s0))))),
                        ("slug", jstr(cx, skin, "Slug")),
                        ("displayName", Some(Json::Str(source(field(skin, "DisplayName"))))),
                        ("icon", Some(s(pkg_name(&icon)))),
                    ])),
                ),
                ("skins", Some(Json::Arr(skins))),
                ("moves", Some(moves)),
                (
                    "backgroundColor",
                    Some(jobj([
                        ("R", jnum(&bg, "R")),
                        ("G", jnum(&bg, "G")),
                        ("B", jnum(&bg, "B")),
                        ("A", jnum(&bg, "A")),
                    ])),
                ),
                ("arcadeIcon", arcade_icon.as_deref().map(|a| s(pkg_name(a)))),
                ("bio", Some(Json::Str(source(field(c, "Bio"))))),
                ("collectionsPortrait", portrait.map(Json::Str)),
                // CharacterData ArcadeRivalSlug, ArcadeMapSlug1v1, ArcadeMapSlug2v2 ('' when the asset leaves one
                // empty)
                (
                    "arcade",
                    Some(jobj([
                        ("rival", Some(Json::Str(pstr(cx, c, "ArcadeRivalSlug").unwrap_or_default()))),
                        ("map1v1", Some(Json::Str(pstr(cx, c, "ArcadeMapSlug1v1").unwrap_or_default()))),
                        ("map2v2", Some(Json::Str(pstr(cx, c, "ArcadeMapSlug2v2").unwrap_or_default()))),
                    ])),
                ),
            ]),
        );
    }
    Ok(())
}

/// CharacterData CollectionsPortraitMaterial (the Collection's big portrait): the bundle's name of the material, `None`
/// where it is unset or its package is not in the build.
fn collections_portrait(cx: &Cx<'_>, refs: &mut Refs, c: &Props) -> Result<Option<String>, KindError> {
    let Some(m) = pstr(cx, c, "CollectionsPortraitMaterial").filter(|m| !m.is_empty()) else { return Ok(None) };
    let Some(cls) = refs.cls(cx, &m)? else { return Ok(None) };
    Ok(Some(refs.keyed(&m, &cls)))
}

/// One PerkData: its id (its asset's name) and properties.
struct PerkEntry {
    id: String,
    p: Props,
}

/// The perk page: the lists rule (GetAllPerksForCharacter's forced-offline branch: not EnabledForDebugging, not
/// IsUnavailableOffline, Level Small or Unsized, a CharacterSpecific perk only for its AssociatedCharacter, plus
/// the RecommendedPerkDatas).
// Unverified: the AssetRegistry's order is not read: a list is in package path order.
fn perks<'a>(
    cx: &Cx<'a>,
    chars: &[(String, ObjView<'a>)],
    soft_tex: &mut IndexMap<String, String>,
    out: &mut MenuData,
) -> Result<(), KindError> {
    let mut paths = cx.catalog.list_packages(PERK_DATA_FOLDER, true, Some(PERK_DATA_CLASS));
    paths.sort();
    let mut all = Vec::new();
    for p in &paths {
        let o = cx.object(p)?;
        all.push(PerkEntry { id: short_name(o.path()).to_owned(), p: o.props().clone() });
    }
    let cat = |x: &PerkEntry| -> Result<u32, KindError> {
        match field(&x.p, "Category").filter(|v| truthy(v)) {
            Some(c) => category(tail(Some(c), "undefined"))
                .map_or_else(|| failed(format!("perk {}: category {c:?}", x.id)), Ok),
            None => Ok(0),
        }
    };
    let level = |x: &PerkEntry| tail(field(&x.p, "Level"), "Large").to_owned();
    let listed = |x: &PerkEntry| {
        !field(&x.p, "EnabledForDebugging").is_some_and(truthy)
            && !field(&x.p, "IsUnavailableOffline").is_some_and(truthy)
            && matches!(level(x).as_str(), "Small" | "Unsized")
    };
    let mut cats = Vec::new();
    for x in &all {
        cats.push(cat(x)?);
    }
    let mut used = std::collections::BTreeSet::new();
    for (key, cd) in chars {
        let fighter = pstr(cx, cd.props(), "AssociatedCharacter");
        let mut recommended = Vec::new();
        for r in strings(cx, cd.props(), "RecommendedPerkDatas") {
            let id = short_name(&r).to_owned();
            if !all.iter().any(|x| x.id == id) {
                return failed(format!("{key}: RecommendedPerkDatas {id} is not a PerkData"));
            }
            recommended.push(id);
        }
        let lists: Vec<Vec<String>> = (0..4)
            .map(|k| {
                all.iter()
                    .zip(&cats)
                    .filter(|(x, c)| {
                        **c == k
                            && (listed(x) || recommended.contains(&x.id))
                            && (k != 3 || pstr(cx, &x.p, "AssociatedCharacter") == fighter)
                    })
                    .map(|(x, _)| x.id.clone())
                    .collect()
            })
            .collect();
        for id in &recommended {
            if !lists.iter().any(|l| l.contains(id)) {
                return failed(format!("{key}: recommended {id} is in no list"));
            }
        }
        for l in &lists {
            used.extend(l.iter().cloned());
        }
        let mut info = out.fighters.get(key).cloned().unwrap_or_else(Json::obj);
        info.set(
            "perks",
            Json::object([
                (
                    "lists",
                    Json::Arr(lists.into_iter().map(|l| Json::Arr(l.into_iter().map(Json::Str).collect())).collect()),
                ),
                ("recommended", Json::Arr(recommended.into_iter().map(Json::Str).collect())),
            ]),
        );
        out.fighters.insert(key.clone(), info);
    }
    for id in &used {
        let Some(i) = all.iter().position(|x| &x.id == id) else { continue };
        let x = &all[i];
        let mut icon = None;
        if let Some(path) = field(&x.p, "Icon").and_then(|v| string(cx.cache, v)) {
            let name = pkg_name(&path).to_owned();
            soft_tex.insert(name.clone(), path);
            icon = Some(name);
        }
        out.perks.insert(
            id.clone(),
            PerkInfo {
                slug: pstr(cx, &x.p, "Slug").unwrap_or_default(),
                name: source(field(&x.p, "DisplayName")),
                description: source(field(&x.p, "Description")),
                stacked: source(field(&x.p, "StackedDescription")),
                category: cats[i],
                level: level(x),
                icon,
                fighter: field(&x.p, "AssociatedCharacter")
                    .filter(|v| truthy(v))
                    .map(|v| tail(Some(v), "undefined").to_owned()),
            },
        );
    }
    Ok(())
}

/// The character selector's grid: PGI_GlobalData FightersInCharacterSelect_HardCodedOrder through
/// CharacterDataByFighter.
fn roster(cx: &Cx<'_>, refs: &mut Refs, out: &mut MenuData) -> Result<(), KindError> {
    let pgi = cx.object(&cdo_path(PGI_GLOBAL_DATA))?;
    let gp = defaults_or_props(&pgi)?;
    let by_fighter: Vec<(Option<String>, Option<String>)> = match field(&gp, "CharacterDataByFighter") {
        Some(Value::Map(m)) => m.iter().map(|(k, v)| (string(cx.cache, k), string(cx.cache, v))).collect(),
        _ => Vec::new(),
    };
    for f in strings(cx, &gp, "FightersInCharacterSelect_HardCodedOrder") {
        let Some(at) = by_fighter.iter().find(|(k, _)| k.as_deref() == Some(&f)).and_then(|(_, v)| v.clone()) else {
            return failed(format!("roster: no CharacterData for {f}"));
        };
        let o = cx.object(&at)?;
        let c = o.props();
        let portrait = need(cx, c, "CharacterSelectPortraitMaterial", &at)?;
        refs.set(pkg_name(&portrait), &portrait, material_class(&portrait));
        let pkg = pkg_of(&at).to_lowercase();
        let fighter = CHARACTERS
            .iter()
            .find(|k| CHARACTER_DATA.iter().any(|(ck, p)| ck == *k && p.to_lowercase() == pkg))
            .map(|k| Json::Str((*k).to_owned()));
        // the Collection's data of every selectable fighter, the port's or not
        let collections_portrait = collections_portrait(cx, refs, c)?;
        let bg = match field(c, "BackgroundColor") {
            Some(Value::Struct(p)) => {
                Some(jobj([("R", jnum(p, "R")), ("G", jnum(p, "G")), ("B", jnum(p, "B")), ("A", jnum(p, "A"))]))
            }
            _ => None,
        };
        out.roster.push(jobj([
            ("slug", jstr(cx, c, "Slug")),
            ("bio", Some(Json::Str(source(field(c, "Bio"))))),
            ("collectionsPortrait", collections_portrait.map(Json::Str)),
            ("backgroundColor", bg),
            ("displayName", Some(Json::Str(source(field(c, "DisplayName"))))),
            ("portrait", Some(s(pkg_name(&portrait)))),
            ("class", Some(s(tail(field(c, "Class"), "Support")))),
            ("type", Some(s(tail(field(c, "Type"), "Horizontal")))),
            ("difficulty", Some(s(tail(field(c, "Difficulty"), "Low")))),
            ("property", Some(Json::Str(source(field(c, "Property"))))),
            ("experimental", Some(Json::Bool(field(c, "IsExperimental").is_some_and(truthy)))),
            ("fighter", Some(fighter.unwrap_or(Json::Null))),
        ]));
    }
    Ok(())
}

/// The Collection's mastery track: CharacterMasteryTable's XP per level (GetMasteryDataForLevel), and per
/// GlobalCharacterData slug the first reward of each MasteryRewardTable entry (GetMasteryRewardsForLevel(L): the
/// entry at index L - 1, @336) as RewardInfo AsyncCreateFromLevelUpReward -> CreateFromAsset reads it (@204-1018):
/// DisplayName, the reward's Count, DetermineRewardType, Rarity, RewardThumbnail (soft), a PerkData's Category,
/// AssociatedCharacter (@588), UnlockLocation and DetermineRewardTier.
/// The asset is BPI_AssetFromCharacterMasteryReward's: the AssetReward if valid (@0), else by NonAssetRewardSlug gold
/// / match_toasts (@85-195), else by RewardType 2 / 3 (the perk slot infos, @209-389), else null (@413). The build's
/// tables hold two entries with no AssetReward (Steven's and Velma's level 1: Inventory, no slug; Count 0 and 1), so
/// null; a null asset makes the empty reward (no DisplayName: SetupEmptyRewardVisuals). The slug and RewardType
/// fallbacks, which no table reaches, are an error.
fn mastery(
    cx: &Cx<'_>,
    refs: &mut Refs,
    soft_tex: &mut IndexMap<String, String>,
    out: &mut MenuData,
) -> Result<(), KindError> {
    let pgi = cx.object(&cdo_path(PGI_GLOBAL_DATA))?;
    let gp = defaults_or_props(&pgi)?;
    let levels: Vec<Json> = match field(&gp, "CharacterMasteryTable") {
        Some(Value::Struct(t)) => match field(t, "Levels") {
            Some(Value::Array(l)) => l
                .iter()
                .map(|l| match l {
                    Value::Struct(l) => jnum(l, "XP").unwrap_or(Json::Null),
                    _ => Json::Null,
                })
                .collect(),
            _ => Vec::new(),
        },
        _ => Vec::new(),
    };
    if levels.is_empty() {
        return failed("PGI_GlobalData CharacterMasteryTable has no Levels".into());
    }
    let mut rewards = Json::obj();
    let global: Vec<(Option<String>, Option<String>)> = match field(&gp, "GlobalCharacterData") {
        Some(Value::Map(m)) => m.iter().map(|(k, v)| (string(cx.cache, k), string(cx.cache, v))).collect(),
        _ => Vec::new(),
    };
    let mut chars = Vec::new();
    for (slug, at) in global {
        let (Some(slug), Some(at)) = (slug, at) else { continue };
        chars.push((slug, cx.object(&at)?));
    }
    let tiers = mastery_tiers(cx, &chars.iter().map(|(_, o)| o.props()).collect::<Vec<_>>());
    for (slug, o) in &chars {
        let table: Vec<&Props> = match field(o.props(), "MasteryRewardTable") {
            Some(Value::Struct(t)) => match field(t, "LevelRewardThresholds") {
                Some(Value::Array(a)) => a.iter().filter_map(Value::as_struct).collect(),
                _ => Vec::new(),
            },
            _ => Vec::new(),
        };
        let mut list = Vec::new();
        for (i, level) in table.iter().enumerate() {
            let r = match field(level, "Rewards") {
                Some(Value::Array(a)) => a.first().and_then(Value::as_struct),
                _ => None,
            };
            let asset = r.and_then(|r| field(r, "AssetReward")).and_then(|v| string(cx.cache, v));
            let Some(asset) = asset else {
                if r.is_some_and(|r| {
                    field(r, "NonAssetRewardSlug").is_some_and(truthy)
                        || tail(field(r, "RewardType"), "Inventory") != "Inventory"
                }) {
                    return failed(format!(
                        "mastery {slug} level {}: BPI_AssetFromCharacterMasteryReward's slug / RewardType fallback is not ported",
                        i + 1
                    ));
                }
                list.push(jobj([
                    ("name", Some(s(""))),
                    ("count", Some(r.and_then(|r| jnum(r, "Count")).unwrap_or(Json::Num(0.0)))),
                    ("type", Some(Json::Num(0.0))),
                    ("rarity", Some(s("None"))),
                    ("thumb", Some(Json::Null)),
                    ("category", Some(Json::Num(0.0))),
                    ("asset", Some(s(""))),
                    ("fighter", Some(Json::Null)),
                    ("unlock", Some(Json::Null)),
                    ("tier", Some(Json::Num(0.0))),
                ]));
                continue;
            };
            let a = cx.object(&asset)?;
            let p = a.props();
            let thumb = soft_texture(cx, refs, p, "RewardThumbnail")?;
            if let Some(t) = &thumb {
                soft_tex.insert(pkg_name(t).to_owned(), t.clone());
            }
            let category = if a.class() == PERK_DATA_CLASS && field(p, "Category").is_some_and(truthy) {
                category(tail(field(p, "Category"), "undefined")).unwrap_or(0)
            } else {
                0
            };
            list.push(jobj([
                ("name", Some(Json::Str(source(field(p, "DisplayName"))))),
                ("count", r.and_then(|r| jnum(r, "Count"))),
                ("type", Some(Json::Num(f64::from(reward_type(short_name(a.class()), field(p, "Slug")))))),
                ("rarity", Some(s(tail(field(p, "Rarity"), "None")))),
                ("thumb", Some(thumb.as_deref().map_or(Json::Null, |t| s(pkg_name(t))))),
                ("category", Some(Json::Num(f64::from(category)))),
                ("asset", Some(s(a.path()))),
                ("fighter", Some(fighter_of(p).map_or(Json::Null, |f| s(&f)))),
                ("unlock", Some(unlock_of(p).unwrap_or(Json::Null))),
                ("tier", Some(Json::Num(f64::from(reward_tier(p, pkg_of(&asset), &tiers))))),
            ]));
        }
        rewards.set(slug.clone(), Json::Arr(list));
    }
    out.mastery = Some(Json::object([("levels", Json::Arr(levels)), ("rewards", rewards)]));
    Ok(())
}

/// AssociatedCharacter's EFighter tail (None: unset).
fn fighter_of(p: &Props) -> Option<String> {
    field(p, "AssociatedCharacter").is_some_and(truthy).then(|| tail(field(p, "AssociatedCharacter"), "").to_string())
}

/// UnlockLocation's EUnlockLocation tail (None: unset or None).
fn unlock_of(p: &Props) -> Option<Json> {
    let u = tail(field(p, "UnlockLocation"), "None");
    (u != "None" && !u.is_empty()).then(|| s(u))
}

/// A soft texture property: its path when it is set and names a texture.
fn soft_texture(cx: &Cx<'_>, refs: &mut Refs, p: &Props, name: &str) -> Result<Option<String>, KindError> {
    Ok(match pstr(cx, p, name) {
        Some(t) if field(p, name).is_some_and(|v| !matches!(v, Value::Null)) => {
            (refs.cls(cx, &t)?.as_deref() == Some(TEXTURE)).then_some(t)
        }
        _ => None,
    })
}

fn class_info(cx: &Cx<'_>, refs: &mut Refs, out: &mut MenuData) -> Result<(), KindError> {
    let table =
        cx.object("/Game/Panda_Main/UI/In-Game/Data/UICharacterClassInfo_Datatable.UICharacterClassInfo_Datatable")?;
    for (_, r) in rows(&table)? {
        let mut found = Vec::new();
        r.values().for_each(|v| strings_of(cx, v, &mut found));
        refs.collect(cx, found.iter().map(String::as_str))?;
        let icon = pstr(cx, &r, "Icon").unwrap_or_default();
        out.class_info.insert(
            tail(field(&r, "Class"), "undefined").to_owned(),
            Json::object([
                ("displayName", Json::Str(source(field(&r, "DisplayName")))),
                ("icon", s(short_name(&icon))),
            ]),
        );
    }
    Ok(())
}

/// (Deviation: the port's own) the home screen's promotions.
fn promotions(cx: &Cx<'_>, soft_tex: &mut IndexMap<String, String>, out: &mut MenuData) -> Result<(), KindError> {
    for (slug, widget) in PROMOTIONS {
        let pkg = format!("{PANDA_MAIN}{widget}");
        let name = base_name(&pkg);
        let cls_path = format!("{pkg}.{name}_C");
        let cls = cx.object(&cls_path)?;
        let cdo = cx.object(&cdo_path(&cls_path))?;
        if !cls.super_path()?.is_some_and(|s| s.ends_with("UI_PromoParent_C")) {
            return failed(format!("{widget}: not a UI_PromoParent child"));
        }
        let Some(thumb) = pstr(cx, cdo.props(), "Thumbnail") else {
            return failed(format!("{widget}: no Thumbnail"));
        };
        soft_tex.insert(pkg_name(&thumb).to_owned(), thumb.clone());
        out.promo_widgets.insert(
            name.to_owned(),
            Json::object([
                ("title", Json::Str(source(field(cdo.props(), "Title")))),
                ("thumbnail", s(pkg_name(&thumb))),
            ]),
        );
        out.promotions.push(Json::object([("slug", s(slug)), ("widget", s(name))]));
    }
    Ok(())
}

/// The cosmetics' tile pictures (the Collection's chooser and overview tiles: each by its build path), soft:
/// textures, and the taunts' and emotes' thumbnail material instances, whose textures are soft too. `shop_pictures`
/// are the shop catalogue's (`Shop::pictures`), added the same way.
fn cosmetic_pictures(
    cx: &Cx<'_>,
    refs: &mut Refs,
    soft_tex: &mut IndexMap<String, String>,
    cos_materials: &mut Vec<String>,
    shop_pictures: Vec<String>,
) -> Result<Cosmetics, KindError> {
    let cos = read_cosmetics(cx)?;
    let mut pics: Vec<String> = Vec::new();
    for list in cos.lists() {
        for it in list {
            pics.extend(
                [&it.icon, &it.thumb, &it.preview, &it.tex].into_iter().flatten().filter(|p| !p.is_empty()).cloned(),
            );
            for t in it.tiers.iter().flatten() {
                pics.extend([&t.2, &t.3].into_iter().flatten().filter(|p| !p.is_empty()).cloned());
            }
        }
    }
    pics.extend(shop_pictures);
    // UI_AXProgressStage UpdateCharacterPortrait's soft textures of the Mini (@2085) and the bosses' rounds (@2179,
    // @2273)
    pics.push(exported("/Game/Panda_Main/UI/Badges/UI_Stat_Reindog_Icon"));
    pics.push(exported("/Game/Panda_Main/UI/Assets/Icons/ui_icons_question"));
    // WBP_StatBadge's star brushes (its StarLevel switch @527: UI_Stat_Tier1..5)
    for i in 1..=5 {
        pics.push(exported(&format!("/Game/Panda_Main/UI/Badges/UI_Stat_Tier{i}")));
    }
    for p in &pics {
        let Some(c) = refs.cls(cx, p)? else { continue };
        let name = pkg_name(p).to_owned();
        if c == TEXTURE {
            soft_tex.entry(name).or_insert_with(|| p.clone());
        } else if !refs.by_name.contains_key(&name) {
            refs.set(&name, p, &c);
            cos_materials.push(name);
        }
    }
    Ok(cos)
}

/// The account level table: PGI_GlobalData's AccountLevelUpTable.LevelUpData, each row's XP and RewardData.Gold
fn account_levels(cx: &Cx<'_>, out: &mut MenuData) -> Result<(), KindError> {
    const AT: &str = "PGI_GlobalData AccountLevelUpTable";
    let pgi = cx.object(&cdo_path(PGI_GLOBAL_DATA))?;
    let p = defaults_or_props(&pgi)?;
    let Some(Value::Struct(table)) = field(&p, "AccountLevelUpTable") else {
        return failed(format!("{AT}: no struct"));
    };
    let Some(Value::Array(rows)) = field(table, "LevelUpData") else { return failed(format!("{AT}: no LevelUpData")) };
    for (i, r) in rows.iter().enumerate() {
        let Value::Struct(r) = r else { return failed(format!("{AT} row {i}: not a struct")) };
        let xp = field(r, "XP").and_then(num);
        let gold = match field(r, "RewardData") {
            Some(Value::Struct(d)) => field(d, "Gold").and_then(num),
            _ => None,
        };
        let (Some(xp), Some(gold)) = (xp, gold) else {
            return failed(format!("{AT} row {i}: no XP or RewardData.Gold"));
        };
        out.account_levels.push(AccountLevelRow { xp, gold });
    }
    if out.account_levels.is_empty() {
        return failed(format!("{AT}: no rows"));
    }
    Ok(())
}

/// The arcade bots' names: GetArcadeBotLoadouts' literals (the `literals` kind's reads) and the CoOpVsAI manager's boss
/// skins.
fn arcade_names(cx: &Cx<'_>, en_text: &LocresTable, out: &mut MenuData) -> Result<(), KindError> {
    const FUNC: &str =
        "/Game/Panda_Main/Blueprints/PandaGameInstance/PGI_CoOpVsAIManager.PGI_CoOpVsAIManager_C:GetArcadeBotLoadouts";
    const COOP: &str =
        "/Game/Panda_Main/Blueprints/PandaGameInstance/PGI_CoOpVsAIManager.Default__PGI_CoOpVsAIManager_C";
    let bytes = Literals.build(cx, &Item::new(FUNC))?;
    let lit: serde_json::Value =
        serde_json::from_slice(&bytes).map_err(|e| KindError::Failed(format!("ui: arcade literals: {e}")))?;
    let text = |statement: &str, is_text: bool| -> Result<String, KindError> {
        match lit.get(statement) {
            Some(serde_json::Value::String(v)) if !is_text => Ok(v.clone()),
            Some(serde_json::Value::Object(o)) if is_text => {
                let get = |k: &str| o.get(k).and_then(serde_json::Value::as_str).unwrap_or_default();
                // localized: the table's string, else the text's source
                Ok(en_text
                    .get(get("namespace"))
                    .and_then(|n| n.get(get("key")))
                    .cloned()
                    .unwrap_or_else(|| get("source").to_owned()))
            }
            Some(_) => failed(format!(
                "arcade: GetArcadeBotLoadouts @{statement} is not a {} constant",
                if is_text { "text" } else { "string" }
            )),
            None => failed(format!("arcade: GetArcadeBotLoadouts has no constant @{statement} in the literals kind")),
        }
    };
    let coop = cx.object(COOP)?;
    let coop = defaults_or_props(&coop)?;
    let skin = |name: &str| -> Result<String, KindError> {
        match pstr(cx, &coop, name).filter(|s| !s.is_empty()) {
            Some(v) => Ok(base_name(pkg_of(&v)).to_owned()),
            None => failed(format!("arcade: {COOP} has no {name}")),
        }
    };
    out.arcade_names = ArcadeNames {
        nemesis_prefix: text("5065", false)?,
        mini: text("5145", false)?,
        reindog: text("5182", false)?,
        giant_gizmo: text("5216", false)?,
        empty_name: text("4227", true)?,
        reindog_boss_skin: skin("ReindogBossSkin")?,
        gizmo_boss_skin: skin("GizmoBossSkin")?,
    };
    Ok(())
}

/// The Glossary's three lists: per table its rows "1" .. "N" in that order, each the row's Name and Description in
/// the build's English.
fn glossary(cx: &Cx<'_>, en_text: &LocresTable, out: &mut MenuData) -> Result<(), KindError> {
    for t in GLOSSARY_TABLES {
        let table = cx.object(&exported(&format!("/Game/Panda_Main/UI/Prototype/Foundation/Types/{t}")))?;
        let mut list = Vec::new();
        for (i, (n, r)) in rows(&table)?.into_iter().enumerate() {
            if n != (i + 1).to_string() {
                return failed(format!("glossary: {t} row {i} is named {n}"));
            }
            list.push(Json::object([
                ("name", Json::Str(english(en_text, field(&r, "Name"), t, &format!("{n} Name"))?)),
                ("description", Json::Str(english(en_text, field(&r, "Description"), t, &format!("{n} Description"))?)),
            ]));
        }
        out.glossary.insert((*t).to_owned(), Json::Arr(list));
    }
    Ok(())
}

/// UI_PM_GameplayTips Load Tips (@444-611): every row's Tip of UI_GameplayTipDataTable, kept as its Game.locres key
/// (the game reads the player's words through it); a tip with no string in the build's table fails the import.
fn gameplay_tips(cx: &Cx<'_>, en_text: &LocresTable, out: &mut MenuData) -> Result<(), KindError> {
    let table = cx.object(&exported("/Game/Panda_Main/UI/PreMatch/Data/UI_GameplayTipDataTable"))?;
    for (n, r) in rows(&table)? {
        let Some(Value::Text(TextValue::Base { namespace, key, .. })) = field(&r, "Tip") else {
            return failed(format!("gameplay tips: row {n} has no Tip text"));
        };
        if !namespace.is_empty() || en_text.get("").and_then(|t| t.get(key)).is_none() {
            return failed(format!("gameplay tips: row {n}: no string for {namespace}/{key}"));
        }
        out.gameplay_tips.push(key.clone());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn object_paths_and_categories() {
        assert!(is_object_path("/Game/A/B.B"));
        assert!(is_object_path("/Engine/X/Y.Y_C"));
        assert!(!is_object_path("/Game/A/B"));
        assert!(!is_object_path("/Game/.")); // nothing before the dot
        assert!(!is_object_path("/Game/A."));
        assert!(!is_object_path("/Script/Engine.Material"));
        assert_eq!(category("Utility"), Some(2));
        assert_eq!(category("Other"), None);
        assert_eq!(cdo_path("/Game/A/PGI.PGI_C"), "/Game/A/PGI.Default__PGI_C");
    }
}
