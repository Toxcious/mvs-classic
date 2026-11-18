//! The `ui` kind: the hud and menu widget bundles and the loading screen as the native UI reads them (`mvs-ui`
//! bundle.rs), JSON. The bundle, fonts,
//! tables, widgets and loading screen are U1's; the menu's data (fighters, roster, maps, perks, mastery, promotions,
//! glossary) is [`menu`]'s (U2's).
//!
//! Reads nothing from the store: the widgets, data assets and tables in-process (`Cx::object`), the English text
//! table through `text::table`, the font faces and Slate images through their kinds' builds.

mod fonts;
mod loading;
pub mod menu;
pub mod shop;
pub mod tables;
pub mod widget;

use indexmap::{IndexMap, IndexSet};
use mvs_uasset::value::{Props, TextValue, Value};
use serde::{Deserialize, Serialize};

use self::fonts::FaceData;
use self::menu::{AccountLevelRow, ArcadeNames, MenuData, PerkInfo, menu_data};
use self::widget::{Obj, WidgetReader, truthy};
use crate::fighter_port::{CHARACTERS, character_data};
use crate::game::{UI_ENTRIES, UI_KIND_VERSION, exported, ui_entry_path};
use crate::json::{Json, props_json, to_vec};
use crate::kind::{Cx, Item, Kind, KindError};
use crate::kinds::cosmetics::Cosmetics;
use crate::kinds::font::Font;
use crate::kinds::image::Image;
use crate::kinds::material::layout::{Layout, LayoutTexture, parameter_texture};
use crate::kinds::material::resolve::{material_shader_map, resolve_material};
use crate::kinds::material::sampling::{load_texture_lod_groups, texture_sampling};
use crate::kinds::text::{self, GAME_CULTURE};
use crate::objview::{ObjView, num, short_name, string};

pub const MATERIAL: &str = "/Script/Engine.Material";
pub const INSTANCE: &str = "/Script/Engine.MaterialInstanceConstant";
pub const TEXTURE: &str = "/Script/Engine.Texture2D";
/// The classes a bundle draws.
pub const DRAWN: [&str; 3] = [MATERIAL, INSTANCE, TEXTURE];
/// The folder the tables name packages below.
pub const PANDA_MAIN: &str = "/Game/Panda_Main/";

// paths

/// A path's package: cut at its first `:`, then at the last `.` (`pkgOf`).
pub fn pkg_of(p: &str) -> &str {
    let s = p.find(':').map_or(p, |c| &p[..c]);
    s.rfind('.').map_or(s, |d| &s[..d])
}

/// The last segment of a path (`baseName`).
pub fn base_name(p: &str) -> &str {
    &p[p.rfind('/').map_or(0, |i| i + 1)..]
}

/// An asset's package name (`pkgName`: what the cosmetics and the literals name it by; a reference a widget holds is
/// named by its object: `short_name`).
pub fn pkg_name(p: &str) -> &str {
    base_name(pkg_of(p))
}

/// The name of a path's folder (`folderName`).
pub fn folder_name(p: &str) -> &str {
    base_name(&p[..p.rfind('/').unwrap_or(0)])
}

/// An enum value's name after its last `::`, or `d` for a value that is no non-empty string (`tail`).
pub fn tail<'v>(e: Option<&'v Value>, d: &'v str) -> &'v str {
    match e.and_then(Value::as_str) {
        Some(s) if !s.is_empty() => s.rfind("::").map_or(s, |i| &s[i + 2..]),
        _ => d,
    }
}

/// A text value's source string, `""` for none (`source`).
pub fn source(t: Option<&Value>) -> String {
    match t {
        Some(Value::Text(TextValue::Base { source, .. })) => source.clone(),
        Some(Value::Text(TextValue::Table { source, .. } | TextValue::Invariant { source })) => {
            source.clone().unwrap_or_default()
        }
        _ => String::new(),
    }
}

/// A DataTable's rows, in order (`rows`: its `native.Rows`); a row's members by stored name (read them with
/// `objview::field`).
pub fn rows(o: &ObjView<'_>) -> Result<Vec<(String, Props)>, KindError> {
    let Some(native) = o.native()? else { return Ok(Vec::new()) };
    let Some(Value::Map(rows)) = native.get("Rows") else { return Ok(Vec::new()) };
    rows.iter()
        .map(|r| match r {
            (Value::Str(n), Value::Struct(p)) => Ok((n.clone(), p.clone())),
            _ => Err(KindError::Failed(format!("ui: {}: a row that is no [name, struct]", o.path()))),
        })
        .collect()
}

/// A material's class by its asset name (a name starting M_ is a Material, else an instance: `materialClass`).
pub fn material_class(path: &str) -> &'static str {
    if short_name(path).starts_with("M_") { MATERIAL } else { INSTANCE }
}

/// One object a bundle draws.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UiRef {
    pub path: String,
    pub cls: String,
}

/// The objects the bundle draws, by the name the bundle gives each (an asset's name; a second package of a name gets
/// `@<its folder>` after it): `Refs`.
#[derive(Default)]
pub struct Refs {
    pub by_name: IndexMap<String, UiRef>,
    class_of: IndexMap<String, Option<String>>,
}

impl Refs {
    /// The class of an object the build holds; `None` for one it does not (an object inside a package, a widget's or
    /// an animation's, is no material or texture: `None` too).
    pub fn cls(&mut self, cx: &Cx<'_>, path: &str) -> Result<Option<String>, KindError> {
        if let Some(c) = self.class_of.get(path) {
            return Ok(c.clone());
        }
        let c = if path.contains(':') || !cx.exists(path) { None } else { Some(cx.object(path)?.class().to_owned()) };
        self.class_of.insert(path.to_owned(), c.clone());
        Ok(c)
    }

    pub fn set(&mut self, name: &str, path: &str, cls: &str) {
        self.by_name.insert(name.to_owned(), UiRef { path: path.to_owned(), cls: cls.to_owned() });
    }

    /// Every material and texture a list of references names, under its asset's name (`collect`).
    pub fn collect<'p>(&mut self, cx: &Cx<'_>, paths: impl IntoIterator<Item = &'p str>) -> Result<(), KindError> {
        for p in paths {
            if let Some(c) = self.cls(cx, p)?.filter(|c| DRAWN.contains(&c.as_str())) {
                self.set(short_name(p), p, &c);
            }
        }
        Ok(())
    }

    /// A literal of a Blueprint or a table: a texture or a material by its package below [`PANDA_MAIN`] (`literal`).
    pub fn literal(&mut self, rel: &str, cls: &str) {
        let path = exported(&format!("{PANDA_MAIN}{rel}"));
        self.set(base_name(rel), &path, cls);
    }

    /// A material by path under a key of its own (a name another package holds gets `@<folder>`: `keyed`).
    pub fn keyed(&mut self, path: &str, cls: &str) -> String {
        let mut k = pkg_name(path).to_owned();
        if self.by_name.get(&k).is_some_and(|had| pkg_of(&had.path) != pkg_of(path)) {
            k = format!("{k}@{}", folder_name(pkg_of(path)));
        }
        self.set(&k, path, cls);
        k
    }
}

#[derive(Deserialize)]
struct Named {
    entry: String,
    materials: Option<IndexMap<String, String>>,
}

/// The DrawRefs an entry names (`uiDrawRefs`: a bundle's materials; the loading entry names none), what the material
/// kind's list reads.
pub fn draw_refs(bytes: &[u8], out: &mut IndexSet<String>) -> Result<(), KindError> {
    let v: Named = serde_json::from_slice(bytes).map_err(|e| KindError::Failed(format!("an entry: {e}")))?;
    if v.entry != "loading" {
        out.extend(v.materials.into_iter().flat_map(IndexMap::into_values));
    }
    Ok(())
}

/// The three entries, hud, menu and loading.
pub fn items() -> Vec<Item> {
    UI_ENTRIES.iter().map(|e| Item::new(exported(&ui_entry_path(e)))).collect()
}

pub struct Ui;

impl Kind for Ui {
    fn id(&self) -> &'static str {
        "ui"
    }

    fn version(&self) -> u64 {
        UI_KIND_VERSION
    }

    fn content_type(&self, _: &[u8]) -> &'static str {
        "application/json"
    }

    /// [`items`].
    fn list(&self, _: &Cx<'_>) -> Result<Vec<Item>, KindError> {
        Ok(items())
    }

    fn build(&self, cx: &Cx<'_>, item: &Item) -> Result<Vec<u8>, KindError> {
        let entry = UI_ENTRIES
            .iter()
            .find(|e| exported(&ui_entry_path(e)) == item.path)
            .ok_or_else(|| KindError::NotFound(format!("ui-import: no ui entry {}", item.path)))?;
        match *entry {
            "loading" => loading::bind_loading(cx),
            e => bind_bundle(cx, e == "hud"),
        }
    }
}

// ---------------------------------------------------------------------------------------------- the bundles

fn failed<T>(why: String) -> Result<T, KindError> {
    Err(KindError::Failed(why))
}

/// A texture's size, mips and sampling (`hud/material` TextureInfo); the HUD reads them before the entry.
#[derive(Serialize)]
struct TextureInfo {
    #[serde(skip_serializing_if = "Option::is_none")]
    path: Option<String>,
    srgb: bool,
    ax: String,
    ay: String,
    /// The filter the texture is sampled with (`material::sampling` texture_sampling: its Filter, TF_Default its
    /// LOD group's); none for an engine Slate brush file (no texture object: the element batches' bilinear).
    #[serde(skip_serializing_if = "Option::is_none")]
    filter: Option<&'static str>,
    w: i64,
    h: i64,
    mips: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    image: Option<String>,
}

/// What both bundles hold.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Common {
    entry: &'static str,
    widgets: IndexMap<String, Json>,
    typefaces: IndexMap<String, String>,
    faces: IndexMap<String, FaceData>,
    default_typeface: String,
    palette: IndexMap<String, Json>,
    palette_game: IndexMap<String, Json>,
    palette_defaults: IndexMap<String, Vec<Json>>,
    materials: IndexMap<String, String>,
    textures: IndexMap<String, TextureInfo>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct HudBundle {
    #[serde(flatten)]
    common: Common,
    portraits: IndexMap<String, String>,
    char_textures: IndexMap<String, Vec<String>>,
    soft_textures: Vec<String>,
    texts: IndexMap<String, String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct MenuBundle {
    #[serde(flatten)]
    common: Common,
    input_icons: IndexMap<String, String>,
    fighters: IndexMap<String, Json>,
    roster: Vec<Json>,
    class_info: IndexMap<String, Json>,
    lab_map: &'static str,
    maps: IndexMap<String, Json>,
    offline_maps: &'static [&'static str],
    offline_maps_local: &'static [&'static str],
    soft_textures: Vec<String>,
    rich_styles: IndexMap<String, IndexMap<String, Json>>,
    /// The rich text image decorators' rows by decorator class: `{ texture, size: [x, y], tint }`.
    rich_images: IndexMap<String, IndexMap<String, Json>>,
    glossary: IndexMap<String, Json>,
    gameplay_tips: Vec<String>,
    texts: IndexMap<String, String>,
    promotions: Vec<Json>,
    promo_widgets: IndexMap<String, Json>,
    perks: IndexMap<String, PerkInfo>,
    mastery: Option<Json>,
    palette_choices: Vec<String>,
    cosmetics: Cosmetics,
    shop: shop::Shop,
    account_levels: Vec<AccountLevelRow>,
    arcade_names: ArcadeNames,
}

/// MPC_CentralPalette's collection and its vectors in the
/// asset's order.
const CENTRAL_PALETTE: &str = "/Game/Panda_Main/Materials/MPC_CentralPalette.MPC_CentralPalette";
const CENTRAL_PALETTE_VECTORS: [&str; 5] = ["ColorMe", "ColorAlly", "ColorEnemy", "ColorTeam3", "ColorTeam4"];
/// The CentralPalette DataTable.
const CENTRAL_PALETTE_TABLE: &str = "/Game/Panda_Main/UI/Prototype/Foundation/Types/CentralPalette.CentralPalette";
const INPUT_ICONS_TABLE: &str = "/Game/Panda_Main/UI/InputBinding/InputButtonIconsTable.InputButtonIconsTable";
const PFG: &str = "/Game/Panda_Main/UI/Fonts/PFG.PFG";

/// A DataTable's rows as JSON, in order (the object kind's `native.Rows` of a table).
fn rows_json(o: &ObjView<'_>) -> Result<Vec<(String, Json)>, KindError> {
    Ok(rows(o)?.into_iter().map(|(n, p)| (n, Json::Obj(props_json(o.cache(), &p)))).collect())
}

/// A colour's R, G, B, A as an object (the members it has).
fn rgba(c: &Json) -> Json {
    Json::Obj(["R", "G", "B", "A"].iter().filter_map(|k| c.get(k).map(|v| ((*k).to_owned(), v.clone()))).collect())
}

/// The face of each typeface of the PFG composite font: `(name, face asset)` (`CompositeFont.DefaultTypeface.Fonts`).
fn pfg_fonts(pfg: &Obj<'_>) -> Vec<(String, String)> {
    let fonts = pfg
        .props
        .get("CompositeFont")
        .and_then(|c| c.get("DefaultTypeface"))
        .and_then(|d| d.get("Fonts"))
        .and_then(Json::as_array)
        .unwrap_or(&[]);
    fonts
        .iter()
        .map(|f| {
            let s = |v: Option<&Json>| v.and_then(Json::as_str).unwrap_or("undefined").to_owned();
            (s(f.get("Name")), s(f.get("Font").and_then(|x| x.get("LocalFontFaceAsset"))))
        })
        .collect()
}

/// A `DrawRef` of a material in the slate context.
fn draw_ref(material: &str) -> Result<String, KindError> {
    if !material.starts_with('/') || !material.contains('.') || material.contains('?') {
        return failed(format!("drawRef: {material} is not a <package>.<export> path"));
    }
    Ok(format!("{material}?ctx=slate"))
}

/// The textures a material's slate pixel shader samples (its 2D texture parameters: the drawmat entry's textures,
/// by `parameterTexture`), the ones the build holds and that are Texture2D objects (`sampledTextures`).
fn sampled_textures(cx: &Cx<'_>, path: &str) -> Result<Vec<String>, KindError> {
    let m = resolve_material(cx, path)?;
    let map = material_shader_map(cx, &m.shader_map)?;
    let tps = map.record.material_compilation_output.uniform_expression_set.uniform_texture_parameters.first();
    let layout = Layout {
        size: 0,
        values: Vec::new(),
        scalar_params: Vec::new(),
        vector_params: Vec::new(),
        textures: tps
            .map(Vec::as_slice)
            .unwrap_or(&[])
            .iter()
            .map(|t| LayoutTexture {
                name: t.parameter_info.name.clone(),
                type_: "2d",
                texture_index: t.texture_index,
                sampler_source: t.sampler_source,
            })
            .collect(),
        texture_sizes: None,
    };
    let mut out: Vec<String> = Vec::new();
    for k in 0..layout.textures.len() {
        let Some(r) = parameter_texture(cx, &m, &layout, k)? else { continue };
        if !cx.exists(&r) || cx.object(&r)?.class() != TEXTURE || out.contains(&r) {
            continue;
        }
        out.push(r);
    }
    Ok(out)
}

/// A sampled texture under its asset's name, `@<folder>` after it where another package holds the name (`nameOf`).
fn name_of(tex_paths: &mut IndexMap<String, String>, p: &str) -> String {
    if let Some((k, _)) = tex_paths.iter().find(|(_, v)| *v == p) {
        return k.clone();
    }
    let mut k = short_name(p).to_owned();
    if tex_paths.contains_key(&k) {
        k = format!("{k}@{}", folder_name(pkg_of(p)));
    }
    tex_paths.insert(k.clone(), p.to_owned());
    k
}

/// The hud's or the menu's bundle (`bindBundle`).
fn bind_bundle(cx: &Cx<'_>, hud: bool) -> Result<Vec<u8>, KindError> {
    let bundle = if hud { "hud" } else { "menu" };
    let reader = WidgetReader::new(cx);
    let mut refs = Refs::default();
    let rel_pkgs: Vec<String> = if hud {
        tables::HUD_WIDGETS.iter().map(|s| (*s).to_owned()).collect()
    } else {
        let roots: Vec<String> = tables::MENU_ROOTS.iter().map(|r| format!("{PANDA_MAIN}{r}")).collect();
        reader.closure(&roots, tables::MENU_SKIP)?
    };
    let pkgs: Vec<String> =
        rel_pkgs.into_iter().map(|r| if r.starts_with('/') { r } else { format!("{PANDA_MAIN}{r}") }).collect();

    // the widgets, and what they name (a child Blueprint: its own package's, then its parent's)
    let mut widgets: IndexMap<String, Json> = IndexMap::new();
    for pkg in &pkgs {
        let mut at = Some(pkg.clone());
        while let Some(cur) = at {
            let w = reader.load(&cur)?;
            let found = reader.refs(&w);
            refs.collect(cx, found.iter().map(String::as_str))?;
            at = reader.parent(&w)?;
        }
        widgets.insert(base_name(pkg).to_owned(), reader.build(pkg)?);
    }

    // PFG composite: typeface name -> the font face's package
    let pfg = Obj::load(cx, PFG)?;
    let fonts = pfg_fonts(&pfg);
    let mut typefaces: IndexMap<String, String> = IndexMap::new();
    for (name, face) in &fonts {
        if tables::FONTS.iter().any(|(k, _)| *k == short_name(face)) {
            typefaces.insert(name.clone(), pkg_of(face).to_owned());
        }
    }
    let mut faces: IndexMap<String, FaceData> = IndexMap::new();
    for (_, rel) in tables::FONTS {
        let pkg = format!("{PANDA_MAIN}{}", rel.strip_suffix(".ufont").unwrap_or(rel));
        let o = Obj::load(cx, &exported(&pkg))?;
        if let Some(h) = o.props.get("Hinting").filter(|h| truthy(Some(h))) {
            return failed(format!("ui: font {pkg}: Hinting {} not handled", h.as_str().unwrap_or("?")));
        }
        let bytes = Font.build(cx, &Item::new(exported(&pkg)))?;
        let bounding_box = o.props.get("LayoutMethod").and_then(Json::as_str) == Some("EFontLayoutMethod::BoundingBox");
        faces.insert(pkg.clone(), fonts::face_metrics(&bytes, bounding_box, &pkg)?);
    }
    for (t, p) in &typefaces {
        if !faces.contains_key(p) {
            return failed(format!("ui: typeface {t}: face {p} has no metrics"));
        }
    }

    // CentralPalette rows by row name, linear: the UI colour (the widgets') and the Game colour (MPC_CentralPalette's,
    // PreferencesManager Apply UI Material Colors); paletteChoices: the rows that are not InternalOnly, in row order
    // (UI_Functions GetPaletteChoices Fn@371)
    let mut palette: IndexMap<String, Json> = IndexMap::new();
    let mut palette_game: IndexMap<String, Json> = IndexMap::new();
    let mut palette_choices: Vec<String> = Vec::new();
    for (k, r) in rows_json(&cx.object(CENTRAL_PALETTE_TABLE)?)? {
        let (Some(ui), Some(game)) = (r.get("UI"), r.get("Game")) else {
            return failed(format!("ui: CentralPalette row {k}: no UI or Game colour"));
        };
        palette.insert(k.clone(), rgba(ui));
        palette_game.insert(k.clone(), rgba(game));
        if !truthy(r.get("InternalOnly")) {
            palette_choices.push(k);
        }
    }
    // MPC_CentralPalette's own defaults (its VectorParameters' DefaultValue): what a vector keeps while no preference
    // binding names a row for it
    let mut palette_defaults: IndexMap<String, Vec<Json>> = IndexMap::new();
    let mpc = Obj::load(cx, CENTRAL_PALETTE)?;
    for v in mpc.props.get("VectorParameters").and_then(Json::as_array).unwrap_or(&[]) {
        let c = v.get("DefaultValue").unwrap_or(&Json::Null);
        let n = v.get("ParameterName").and_then(Json::as_str).unwrap_or("undefined");
        let channel = |k: &str| c.get(k).cloned().unwrap_or(Json::Null);
        palette_defaults.insert(n.to_owned(), vec![channel("R"), channel("G"), channel("B"), channel("A")]);
    }
    for n in CENTRAL_PALETTE_VECTORS {
        if !palette_defaults.contains_key(n) {
            return failed(format!("ui: MPC_CentralPalette has no vector {n}"));
        }
    }

    // the Blueprints' literals
    for (rel, cls) in if hud { tables::HUD_MATERIALS } else { tables::MENU_MATERIALS } {
        refs.literal(rel, cls);
    }
    let mut input_icons: IndexMap<String, String> = IndexMap::new();
    if !hud {
        // DataTable InputButtonIconsTable (UI/InputBinding): key name -> the icon-default texture (the Xbox glyphs; the
        // port shows gamepad type 0). PreferencesManager IconForInputButton takes the first row with the key.
        for (_, r) in rows_json(&cx.object(INPUT_ICONS_TABLE)?)? {
            let key = r.get("key").and_then(|k| k.get("KeyName")).and_then(Json::as_str).unwrap_or("undefined");
            let Some(icon) = r.get("icon-default").and_then(Json::as_str).filter(|i| !i.is_empty()) else { continue };
            if input_icons.contains_key(key) {
                continue;
            }
            input_icons.insert(key.to_owned(), short_name(icon).to_owned());
            refs.set(short_name(icon), icon, TEXTURE);
        }
    }
    for rel in if hud { tables::HUD_LITERAL_TEXTURES } else { tables::MENU_LITERAL_TEXTURES } {
        refs.literal(rel, TEXTURE);
    }
    if hud {
        for t in tables::ABILITY_ICONS {
            refs.literal(t, TEXTURE);
        }
        for (_, c) in tables::CHAR_TEXTURES {
            for t in c.textures {
                refs.literal(t, TEXTURE);
            }
        }
    }

    // the ported characters (the Lab's fighters, their portraits)
    let mut chars: Vec<(String, ObjView<'_>)> = Vec::new();
    for key in CHARACTERS {
        let Some(cd) = character_data(key) else {
            return failed(format!("ui: {key}: no CharacterData in the port's table"));
        };
        chars.push(((*key).to_owned(), cx.object(&exported(cd))?));
    }
    let mut portraits: IndexMap<String, String> = IndexMap::new();
    if hud {
        for (key, cd) in &chars {
            // UI_IGv3_CharacterPortrait OnCharacter: each ported character's CharacterSelectPortraitMaterial
            let m = cd.prop("CharacterSelectPortraitMaterial").and_then(|v| string(cx.cache, v));
            let Some(m) = m else { return failed(format!("ui: {key}: no CharacterSelectPortraitMaterial")) };
            portraits.insert(key.clone(), pkg_name(&m).to_owned());
            refs.set(pkg_name(&m), &m, material_class(&m));
        }
    }
    // the strings the graphs set at run time, in the build's English (the menu's: MENU_TEXTS, the hud's: HUD_TEXTS)
    let en_text = text::table(cx, GAME_CULTURE)?;
    let mut texts: IndexMap<String, String> = IndexMap::new();
    for key in if hud { tables::HUD_TEXTS } else { tables::MENU_TEXTS } {
        let Some(s) = en_text.get("").and_then(|t| t.get(*key)) else {
            return failed(format!("ui: {bundle} text {key}: no English string"));
        };
        texts.insert((*key).to_owned(), s.clone());
    }

    let mut soft_tex: IndexMap<String, String> = IndexMap::new();
    let mut cos_materials: Vec<String> = Vec::new();
    let menu = if hud {
        MenuData::default()
    } else {
        menu_data(cx, &chars, &en_text, &mut refs, &mut soft_tex, &mut cos_materials)?
    };

    // the hud's: a character's own textures are soft (CHAR_TEXTURES); charTextures = per character what its HUD holds
    let mut char_textures: IndexMap<String, Vec<String>> = IndexMap::new();
    let mut char_soft: IndexSet<String> = IndexSet::new();
    if hud {
        let mut named = |base: &str| -> Result<IndexMap<String, String>, KindError> {
            let mut out = IndexMap::new();
            let Some(rel) = tables::HUD_WIDGETS.iter().find(|x| base_name(x) == base) else {
                return failed(format!("ui: {base} is no HUD widget"));
            };
            let w = reader.load(&format!("{PANDA_MAIN}{rel}"))?;
            for p in reader.refs(&w) {
                if let Some(c) = refs.cls(cx, &p)?.filter(|c| DRAWN.contains(&c.as_str())) {
                    out.insert(short_name(&p).to_owned(), c);
                }
            }
            Ok(out)
        };
        let owned: IndexSet<&str> = tables::CHAR_TEXTURES.iter().flat_map(|(_, c)| c.widgets.iter().copied()).collect();
        let mut shared: IndexSet<String> = IndexSet::new();
        for w in tables::HUD_WIDGETS.iter().map(|w| base_name(w)).filter(|w| !owned.contains(w)) {
            shared.extend(named(w)?.into_keys());
        }
        for (name, c) in tables::CHAR_TEXTURES {
            let mut list: Vec<String> = c.textures.iter().map(|t| base_name(t).to_owned()).collect();
            char_soft.extend(list.iter().cloned());
            for w in c.widgets {
                for (n, cls) in named(w)? {
                    if shared.contains(&n) {
                        continue;
                    }
                    if cls == TEXTURE {
                        char_soft.insert(n.clone());
                    }
                    if !c.undrawn.contains(&n.as_str()) {
                        list.push(n);
                    }
                }
            }
            let mut set: Vec<String> = list.into_iter().collect::<IndexSet<_>>().into_iter().collect();
            set.sort();
            char_textures.insert((*name).to_owned(), set);
        }
    }

    let mut soft_textures: Vec<String> = if hud {
        char_soft.iter().cloned().collect()
    } else {
        soft_tex.keys().filter(|n| !refs.by_name.contains_key(*n)).cloned().collect()
    };
    soft_textures.sort();
    if !hud {
        for n in &soft_textures {
            refs.set(n, &soft_tex[n], TEXTURE);
        }
    }

    // the RichTextBlocks' decorator classes (RichTextBlockImageDecorator children): each class's ImageSet table
    // (RichImageRow) by row name, its Brush's texture, image size and tint; the textures go with the rest
    let mut rich_images: IndexMap<String, IndexMap<String, Json>> = IndexMap::new();
    if !hud {
        for pkg in &pkgs {
            let w = reader.load(pkg)?;
            for o in &w.objects {
                if o.class() != "/Script/UMG.RichTextBlock" {
                    continue;
                }
                let classes = o.props.get("DecoratorClasses").and_then(Json::as_array).unwrap_or(&[]);
                for c in classes.iter().filter_map(Json::as_str) {
                    if rich_images.contains_key(short_name(c)) {
                        continue;
                    }
                    let cls = cx.object(c)?;
                    if !cls.super_path()?.is_some_and(|s| s.ends_with(".RichTextBlockImageDecorator")) {
                        return failed(format!("ui: rich text decorator {c}: not a RichTextBlockImageDecorator"));
                    }
                    let cdo = cx.object(&menu::cdo_path(c))?;
                    let Some(set) = menu::pstr(cx, cdo.props(), "ImageSet") else {
                        return failed(format!("ui: rich text decorator {c}: no ImageSet"));
                    };
                    let mut table: IndexMap<String, Json> = IndexMap::new();
                    for (rn, r) in rows_json(&cx.object(&set)?)? {
                        let b = r.get("Brush").unwrap_or(&Json::Null);
                        let Some(tex) = b.get("ResourceObject").and_then(Json::as_str) else {
                            return failed(format!("ui: rich image {}.{rn}: no texture", short_name(&set)));
                        };
                        refs.collect(cx, [tex])?;
                        let size = b.get("ImageSize").unwrap_or(&Json::Null);
                        let tint = b.get("TintColor").and_then(|t| t.get("SpecifiedColor")).unwrap_or(&Json::Null);
                        let or_null = |v: Option<&Json>| v.cloned().unwrap_or(Json::Null);
                        table.insert(
                            rn,
                            Json::object([
                                ("texture", Json::Str(short_name(tex).to_owned())),
                                ("size", Json::Arr(vec![or_null(size.get("X")), or_null(size.get("Y"))])),
                                ("tint", rgba(tint)),
                            ]),
                        );
                    }
                    rich_images.insert(short_name(c).to_owned(), table);
                }
            }
        }
    }

    // the materials as DrawRefs, the textures they sample beside the rest
    let mut materials: IndexMap<String, String> = IndexMap::new();
    let mut tex_paths: IndexMap<String, String> = IndexMap::new();
    let mut sampled: IndexMap<String, Vec<String>> = IndexMap::new();
    for (n, r) in &refs.by_name {
        if r.cls == TEXTURE {
            tex_paths.insert(n.clone(), r.path.clone());
            continue;
        }
        materials.insert(n.clone(), draw_ref(&r.path)?);
        sampled.insert(n.clone(), sampled_textures(cx, &r.path)?);
    }
    let mut samples: IndexMap<String, Vec<String>> = IndexMap::new();
    for (m, ps) in &sampled {
        let names = ps.iter().map(|p| name_of(&mut tex_paths, p)).collect();
        samples.insert(m.clone(), names);
    }
    if !cos_materials.is_empty() {
        let mut all: IndexSet<String> = soft_textures.iter().cloned().collect();
        for n in &cos_materials {
            let Some(s) = samples.get(n) else { return failed(format!("ui: cosmetic material {n} was not sampled")) };
            all.extend(s.iter().cloned());
        }
        soft_textures = all.into_iter().collect();
        soft_textures.sort();
    }
    for rel in if hud { tables::HUD_TEXTURES } else { tables::MENU_TEXTURES } {
        if !tex_paths.contains_key(base_name(rel)) {
            tex_paths.insert(base_name(rel).to_owned(), exported(&format!("{PANDA_MAIN}{rel}")));
        }
    }

    // their size, mips and sampling (the HUD reads them before the entry); a texture whose package is not cooked (an
    // editor-time brush the widgets replace at run time) loads as null in the build: left out
    let mut textures: IndexMap<String, TextureInfo> = IndexMap::new();
    let groups = load_texture_lod_groups(cx)?;
    for (n, p) in &tex_paths {
        if refs.cls(cx, p)?.as_deref() != Some(TEXTURE) {
            continue;
        }
        let o = cx.object(p)?;
        let nat = o.native()?.unwrap_or_default();
        let size = |k: &str| nat.get(k).and_then(num).unwrap_or(0.0);
        let (sx, sy) = (size("SizeX"), size("SizeY"));
        let mips = match nat.get("Mips") {
            Some(Value::Array(m)) => m.len(),
            _ => 0,
        };
        if !(sx > 0.0 && sy > 0.0 && mips > 0) {
            return failed(format!("ui: texture {p}: no size or mips"));
        }
        let addr = |k: &str| tail(o.prop(k), "TA_Wrap").to_owned();
        let sampling = texture_sampling(&o, &groups)?;
        textures.insert(
            n.clone(),
            TextureInfo {
                path: Some(o.path().to_owned()),
                srgb: !matches!(o.prop("SRGB"), Some(Value::Bool(false))),
                ax: addr("AddressX"),
                ay: addr("AddressY"),
                filter: Some(sampling.filter.name()),
                w: sx as i64,
                h: sy as i64,
                mips,
                image: None,
            },
        );
    }
    // the engine's Slate brush files (.png files of the paks: the image kind), menu only
    if !hud {
        for (n, rel) in tables::SLATE_IMAGES {
            let path = exported(&format!("{}{rel}", tables::SLATE_FOLDER));
            let b = Image.build(cx, &Item::new(path.clone()))?;
            let dim = |at: usize| b.get(at..at + 4).map(|v| i64::from(u32::from_be_bytes([v[0], v[1], v[2], v[3]])));
            let (Some(w), Some(h)) = (dim(16), dim(20)) else {
                return failed(format!("ui: {path}: a PNG without a header"));
            };
            textures.insert(
                (*n).to_owned(),
                TextureInfo {
                    path: None,
                    srgb: true,
                    ax: "TA_Clamp".into(),
                    ay: "TA_Clamp".into(),
                    filter: None,
                    w,
                    h,
                    mips: 1,
                    image: Some(path),
                },
            );
        }
    }

    // the RichTextBlocks' TextStyleSet tables (RichTextStyleRow) by table name
    let mut rich_styles: IndexMap<String, IndexMap<String, Json>> = IndexMap::new();
    for pkg in &pkgs {
        let w = reader.load(pkg)?;
        for o in &w.objects {
            if o.class() != "/Script/UMG.RichTextBlock" {
                continue;
            }
            let Some(t) = o.props.get("TextStyleSet").and_then(Json::as_str).filter(|t| !t.is_empty()) else {
                continue;
            };
            if rich_styles.contains_key(short_name(t)) {
                continue;
            }
            let mut table: IndexMap<String, Json> = IndexMap::new();
            for (rn, r) in rows_json(&cx.object(t)?)? {
                let s = r.get("TextStyle").unwrap_or(&Json::Null);
                let f = s.get("Font").unwrap_or(&Json::Null);
                let c = s.get("ColorAndOpacity").and_then(|c| c.get("SpecifiedColor")).unwrap_or(&Json::Null);
                let face = f.get("TypefaceFontName").and_then(Json::as_str);
                if !face.is_some_and(|n| typefaces.contains_key(n)) {
                    return failed(format!(
                        "ui: rich text style {}.{rn}: typeface {} is not in the bundle",
                        short_name(t),
                        face.unwrap_or("undefined")
                    ));
                }
                let so = s.get("ShadowOffset").unwrap_or(&Json::Null);
                let or_null = |v: Option<&Json>| v.cloned().unwrap_or(Json::Null);
                let outline = match f.get("OutlineSettings").and_then(|o| o.get("OutlineSize")) {
                    Some(v) if truthy(Some(v)) => v.clone(),
                    _ => Json::Num(0.0),
                };
                let letter_spacing = match f.get("LetterSpacing") {
                    Some(Json::Null) | None => Json::Num(0.0),
                    Some(v) => v.clone(),
                };
                table.insert(
                    rn,
                    Json::object([
                        ("typeface", or_null(f.get("TypefaceFontName"))),
                        ("size", or_null(f.get("Size"))),
                        ("letterSpacing", letter_spacing),
                        ("outline", outline),
                        ("color", rgba(c)),
                        ("shadow", Json::Arr(vec![or_null(so.get("X")), or_null(so.get("Y"))])),
                    ]),
                );
            }
            rich_styles.insert(short_name(t).to_owned(), table);
        }
    }

    // a perk Icon whose package is not cooked loads as null: the item draws its category's ui_perkbg_* (UI_Perks_Item
    // @1670)
    let mut perks = menu.perks;
    let mut no_icon: IndexSet<String> = IndexSet::new();
    for p in perks.values_mut() {
        if let Some(icon) = p.icon.take() {
            if textures.contains_key(&icon) {
                p.icon = Some(icon);
            } else {
                no_icon.insert(icon);
            }
        }
    }

    let common = Common {
        entry: if hud { "hud" } else { "menu" },
        widgets,
        typefaces,
        faces,
        default_typeface: fonts
            .first()
            .map(|f| f.0.clone())
            .ok_or_else(|| KindError::Failed("ui: the PFG composite font holds no typeface".into()))?,
        palette,
        palette_game,
        palette_defaults,
        materials,
        textures,
    };
    if hud {
        return Ok(to_vec(&HudBundle { common, portraits, char_textures, soft_textures, texts })?);
    }
    soft_textures.retain(|n| !no_icon.contains(n));
    Ok(to_vec(&MenuBundle {
        common,
        input_icons,
        fighters: menu.fighters,
        roster: menu.roster,
        class_info: menu.class_info,
        lab_map: "map_training",
        maps: menu.maps,
        offline_maps: tables::OFFLINE_MAPS,
        offline_maps_local: tables::OFFLINE_MAPS_LOCAL,
        soft_textures,
        rich_styles,
        rich_images,
        glossary: menu.glossary,
        gameplay_tips: menu.gameplay_tips,
        texts,
        promotions: menu.promotions,
        promo_widgets: menu.promo_widgets,
        perks,
        mastery: menu.mastery,
        palette_choices,
        cosmetics: menu.cosmetics,
        shop: menu.shop,
        account_levels: menu.account_levels,
        arcade_names: menu.arcade_names,
    })?)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paths() {
        assert_eq!(pkg_of("/Game/A/B.B:Sub.X"), "/Game/A/B");
        assert_eq!(pkg_of("/Game/A/B"), "/Game/A/B");
        assert_eq!(pkg_name("/Game/A/B.B"), "B");
        assert_eq!(folder_name("/Game/A/B"), "A");
        let s = Value::Str("ERarity::Epic".into());
        assert_eq!(tail(Some(&s), "None"), "Epic");
        assert_eq!(tail(None, "None"), "None");
        let mut r = Refs::default();
        assert_eq!(r.keyed("/Game/A/M_X.M_X", MATERIAL), "M_X");
        assert_eq!(r.keyed("/Game/B/M_X.M_X", MATERIAL), "M_X@B");
        assert_eq!(material_class("/Game/B/MI_X.MI_X"), INSTANCE);
    }
}
