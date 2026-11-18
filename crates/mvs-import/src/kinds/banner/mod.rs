//! The `banner` kind: an entry per released banner (its element class's components, materials, systems and meshes as
//! its user construction script leaves them) and the index entry (slug -> entry), JSON.
//! The context choice is in [`stencil`].
//!
//! Reads nothing from the store: the BannerData, element classes, meshes and materials in-process (`Cx::object`,
//! `mesh::header`, the material kind's resolve).
//!
//! Inputs: the Released DataLookUpTable's `banner_` rows (an asset not in the paks, the eight `_Unreleased/` ones, is
//! left out), each BannerData's one BannerElements entry (its ActorClass, BaseOffset and SlantOffset), and the
//! element class chain up to BP_Banner_Template_C (each class's SimpleConstructionScript, InheritableComponentHandler
//! and class default object; its UCS as [`ucs`] holds it).
//!
//! Components: the StaticMeshComponents and ParticleSystemComponents of the SCS trees on the chain (each class's
//! RootNodes and ChildNodes), each one's template merged over the chain base first: the class that adds it, then each
//! child class's InheritableComponentHandler record (`<name>_GEN_VARIABLE`, delta-serialised). A component's parent is
//! its SCS parent node, or ParentComponentOrVariableName for a root node attached to an inherited one; the only scene
//! components a drawn one may hang from are DefaultSceneRoot and Parallax Pivot (any other fails). Parallax Pivot's
//! merged transform is `pivot`; DefaultSceneRoot may carry none. UCS arrays (masked / unmasked / mirrored): the
//! nearest class's UCS run, its parent call where it stands (a class without its own UCS runs its parent's).
//! Defaults: the class default objects merged over the chain. graphs: the classes on the chain below
//! BP_Banner_Template with a function beyond the UCS.
//!
//! Materials: a drawn mesh's slots (OverrideMaterials, else the mesh's StaticMaterials) in `bannerlocal`, a particle
//! system's sprite emitters' in `bannersprite`, the custom stencil bound; a material whose build there reads no
//! stencil is named in the plain context (`local`, `sprite`: [`stencil`]). Every built one has its scalar parameters
//! in `materials`. A mesh or trail emitter's material is named as the particles kind names it (`mesh`, `trail`); one
//! whose build reads the stencil fails (there is no banner context of those; none does in this build).
//! maskedLayers: per mesh slot material whose base is M_Banner_MaskedLayers_Basic / _Advanced, that base and each
//! "Layer n" texture parameter's texture and AddressX / AddressY (Mirror Banner's plain-layer branch).
//! A component that never draws (bVisible false; or bHiddenInGame and not in UnmaskedComponents, which Show Banner
//! alone unhides) is written with no materials and nothing of it is named.
//! What cannot be built is in the entry's `refused` with the reason: a material the build refuses or that is not in
//! the paks (the component keeps its ref, with no record in `materials`), an emitter whose material is refused (the
//! effects player leaves it out: it is not counted in `systems`).

pub mod stencil;
pub mod ucs;

use std::sync::Arc;

use indexmap::{IndexMap, IndexSet};
use mvs_material::draw::{DrawContextRow, parse_draw_ref};
use mvs_uasset::value::Value;
use serde::Deserialize;
use serde::Serialize;
use serde::de::IgnoredAny;

use self::stencil::{BannerProbe, BannerProbes, BannerScalar, Plain, banner_probe, banner_ref, draw_ref};
use self::ucs::{UcsSet, UcsStep, ucs_of};
use super::material::resolve::resolve_material;
use super::mesh;
use super::particles::system::{BindSystemOpts, bind_system};
use super::scenery::part::JsonProps;
use super::scenery::parts::util::{json_truthy, over_props, props_of};
use crate::game::{banner_dir, banner_index_path, banner_kind_version, banner_path, system_name};
use crate::json::{Json, to_vec, value_json};
use crate::kind::{Cx, Item, Kind, KindError};
use crate::objview::{ObjView, field, short_name, string};

/// The entry layout.
pub const BANNER_FORMAT_VERSION: u32 = 1;

const TABLE: &str = "/Game/Panda_Main/DataLookUpTables/Released/DataLookUpTable.DataLookUpTable";
const TEMPLATE: &str = "BP_Banner_Template_C";
const BANNER_DATA: &str = "/Script/MultiVersus.BannerData";
/// Template keys read and not written: what the menu scene does not draw or never reads (collision, ticking, editor).
const IGNORED: [&str; 9] = [
    "PrimaryComponentTick",
    "bUseAsOccluder",
    "CastShadow",
    "BodyInstance",
    "CollisionProfileName",
    "bReceivesDecals",
    "UCSModifiedProperties",
    "bCastDynamicShadow",
    "bAffectDistanceFieldLighting",
];
const MESH_KEYS: [&str; 9] = [
    "StaticMesh",
    "OverrideMaterials",
    "TranslucencySortPriority",
    "RelativeLocation",
    "RelativeRotation",
    "RelativeScale3D",
    "bHiddenInGame",
    "bVisible",
    "AttachParent",
];
// (a template's WarmupTime is not written: InitParticles 142bf0750 sets the component's from the system's, and the
// system's rides in the particles kind's entry)
const PSC_KEYS: [&str; 10] = [
    "Template",
    "TranslucencySortPriority",
    "RelativeLocation",
    "RelativeRotation",
    "RelativeScale3D",
    "bHiddenInGame",
    "bVisible",
    "WarmupTime",
    "AttachParent",
    "bAutoActivate",
];
/// The MaskedLayers bases (InitializeBanner's GetBaseMaterial test, TPL IB @5-270 / @1664 / @3146).
const MASKED_LAYERS: [&str; 2] = [
    "/Game/Panda_Main/PreMatch/Banners/Materials/GenericBannerElements/M_Banner_MaskedLayers_Basic.M_Banner_MaskedLayers_Basic",
    "/Game/Panda_Main/PreMatch/Banners/Materials/GenericBannerElements/M_Banner_MaskedLayers_Advanced.M_Banner_MaskedLayers_Advanced",
];

fn failed<T>(why: String) -> Result<T, KindError> {
    Err(KindError::Failed(why))
}

// ---------------------------------------------------------------- the entry

type V3 = [f64; 3];

#[derive(Serialize)]
struct Xform {
    loc: V3,
    quat: [f64; 4],
    scale: V3,
}

#[derive(Serialize)]
struct Pivot {
    loc: V3,
    rot: V3,
    scale: V3,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Defaults {
    fake_ortho: bool,
    fake_ortho_move_up: f64,
    old_initialize: bool,
    hide_default_border: bool,
    strength_h: f64,
    strength_v: f64,
    horizontal_offset: f64,
    mirror_axis: [bool; 3],
    mirror_offset_axis: [bool; 3],
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Component {
    name: String,
    kind: &'static str,
    parent: String,
    loc: V3,
    rot: V3,
    scale: V3,
    materials: Vec<String>,
    visible: bool,
    hidden_in_game: bool,
    sort_priority: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    mesh: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    system: Option<String>,
}

#[derive(Serialize)]
struct MaterialRecord {
    scalars: Vec<BannerScalar>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Layer {
    texture: Option<String>,
    address_x: u8,
    address_y: u8,
}

#[derive(Serialize)]
struct MaskedLayers {
    base: String,
    layers: IndexMap<String, Layer>,
}

#[derive(Serialize)]
struct SystemRecord {
    path: String,
    emitters: usize,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Entry {
    kind: &'static str,
    version: u32,
    slug: String,
    asset: String,
    data: String,
    cls: String,
    parents: Vec<String>,
    defaults: Defaults,
    masked: Vec<String>,
    unmasked: Vec<String>,
    mirrored: Vec<String>,
    pivot: Pivot,
    base_offset: Xform,
    slant_offset: Xform,
    components: Vec<Component>,
    materials: IndexMap<String, MaterialRecord>,
    masked_layers: IndexMap<String, MaskedLayers>,
    systems: IndexMap<String, SystemRecord>,
    meshes: Vec<String>,
    graphs: Vec<String>,
    refused: Vec<String>,
}

#[derive(Serialize)]
struct IndexLine {
    path: String,
    asset: String,
    cls: String,
}

#[derive(Serialize)]
struct Index {
    kind: &'static str,
    version: u32,
    banners: IndexMap<String, IndexLine>,
}

// ---------------------------------------------------------------- the rows

/// One released banner of the table: its slug and BannerData path.
#[derive(Clone, Debug)]
pub struct BannerRow {
    pub slug: String,
    pub data: String,
}

/// The run's rows.
struct Rows(Result<Arc<Vec<BannerRow>>, KindError>);

/// The released banners (the table's `banner_` rows) whose BannerData the build holds, in the table's order
/// (the others are left out).
fn banner_rows(cx: &Cx<'_>) -> Result<Arc<Vec<BannerRow>>, KindError> {
    cx.shared.get_or_init(|| Rows(read_rows(cx).map(Arc::new))).0.clone()
}

fn read_rows(cx: &Cx<'_>) -> Result<Vec<BannerRow>, KindError> {
    let table = cx.object(TABLE)?;
    let native = table.native()?;
    let Some(Value::Map(rows)) = native.as_ref().and_then(|n| n.get("Rows")) else {
        return failed(format!("{TABLE}: no Rows"));
    };
    let mut out = Vec::new();
    for (_, r) in rows {
        let Value::Struct(r) = r else { return failed(format!("{TABLE}: a row that is no struct")) };
        let slug = field(r, "Slug").and_then(|v| string(cx.cache, v)).unwrap_or_default();
        if !slug.starts_with("banner_") {
            continue;
        }
        let object = field(r, "Object").and_then(|v| string(cx.cache, v)).unwrap_or_default();
        if !object.starts_with("/Game/") {
            return failed(format!("banner {slug}: {object}"));
        }
        if cx.exists(&object) {
            out.push(BannerRow { slug, data: object });
        }
    }
    Ok(out)
}

// ---------------------------------------------------------------- the chain

struct ChainClass<'a> {
    path: String,
    name: String,
    cls: ObjView<'a>,
}

/// The chain of an ActorClass, nearest first, ending at BP_Banner_Template_C.
fn chain_of<'a>(cx: &Cx<'a>, cls: &str) -> Result<Vec<ChainClass<'a>>, KindError> {
    let mut out = Vec::new();
    let mut path = cls.to_owned();
    loop {
        let o = cx.object(&path)?;
        let name = short_name(&path).to_owned();
        let sup = o.super_path()?;
        out.push(ChainClass { path: path.clone(), name: name.clone(), cls: o });
        if name == TEMPLATE {
            return Ok(out);
        }
        match sup {
            Some(s) if s.starts_with("/Game/") => path = s,
            _ => return failed(format!("banner: {name}: the chain ends before {TEMPLATE}")),
        }
    }
}

struct Node {
    ty: String,
    parent: Option<String>,
    props: JsonProps,
}

fn load<'a>(cx: &Cx<'a>, c: &ChainClass<'a>, v: Option<&Value>, what: &str) -> Result<ObjView<'a>, KindError> {
    match v.and_then(|v| string(cx.cache, v)) {
        Some(path) => cx.object(&path),
        None => failed(format!("banner: {} {what} is not a reference", c.name)),
    }
}

fn walk<'a>(
    cx: &Cx<'a>,
    c: &ChainClass<'a>,
    nodes: &mut IndexMap<String, Node>,
    r: &Value,
    parent: Option<&str>,
) -> Result<(), KindError> {
    let n = load(cx, c, Some(r), "SCS node")?;
    if short_name(n.class()) != "SCS_Node" {
        return failed(format!("banner: {}: not an SCS node", c.name));
    }
    let Some(name) = n.prop("InternalVariableName").and_then(|v| string(cx.cache, v)) else {
        return failed(format!("banner: {} InternalVariableName is not a reference", c.name));
    };
    if nodes.contains_key(&name) {
        return failed(format!("banner: {}: component {name} twice on the chain", c.name));
    }
    let tpl = load(cx, c, n.prop("ComponentTemplate"), &format!("{name} template"))?;
    let par =
        parent.map(str::to_owned).or_else(|| n.prop("ParentComponentOrVariableName").and_then(|v| string(cx.cache, v)));
    if let Some(p) = &par
        && !nodes.contains_key(p)
        && parent.is_none()
    {
        return failed(format!("banner: {}: {name} hangs from {p}, not on the chain above it", c.name));
    }
    let Some(ty) = n.prop("ComponentClass").and_then(|v| string(cx.cache, v)) else {
        return failed(format!("banner: {} {name} ComponentClass is not a reference", c.name));
    };
    nodes.insert(name.clone(), Node { ty: short_name(&ty).to_owned(), parent: par, props: props_of(&tpl) });
    if let Some(Value::Array(children)) = n.prop("ChildNodes") {
        for ch in children {
            walk(cx, c, nodes, ch, Some(&name))?;
        }
    }
    Ok(())
}

/// The SCS nodes of the chain, base first: name -> its class, parent and merged template properties.
fn components_of<'a>(cx: &Cx<'a>, chain: &[ChainClass<'a>]) -> Result<IndexMap<String, Node>, KindError> {
    let mut nodes = IndexMap::new();
    for c in chain.iter().rev() {
        if let Some(scs) = c.cls.prop("SimpleConstructionScript") {
            let scs = load(cx, c, Some(scs), "SimpleConstructionScript")?;
            if let Some(Value::Array(roots)) = scs.prop("RootNodes") {
                for r in roots {
                    walk(cx, c, &mut nodes, r, None)?;
                }
            }
        }
        let Some(ich) = c.cls.prop("InheritableComponentHandler") else { continue };
        let ich = load(cx, c, Some(ich), "InheritableComponentHandler")?;
        let Some(Value::Array(records)) = ich.prop("Records") else { continue };
        for rec in records {
            let Value::Struct(rec) = rec else {
                return failed(format!("banner: {}: a record that is no struct", c.name));
            };
            let key = match field(rec, "ComponentKey") {
                Some(Value::Struct(k)) => field(k, "SCSVariableName").and_then(|v| string(cx.cache, v)),
                _ => None,
            };
            let Some(name) = key else {
                return failed(format!("banner: {} SCSVariableName is not a reference", c.name));
            };
            let tpl = load(cx, c, field(rec, "ComponentTemplate"), &format!("{name} record"))?;
            let Some(n) = nodes.get_mut(&name) else {
                return failed(format!(
                    "banner: {}: an inherited-component record of {name}, not on the chain",
                    c.name
                ));
            };
            n.props = over_props(&n.props, Some(&props_of(&tpl)));
        }
    }
    Ok(nodes)
}

/// A class default object's path: `/Game/A/B.B_C` -> `/Game/A/B.Default__B_C`.
fn cdo_path(cls: &str) -> String {
    let dot = cls.rfind('.').unwrap_or(0);
    format!("{}.Default__{}", &cls[..dot], &cls[dot + 1..])
}

fn axis(s: Option<&Json>) -> Result<[bool; 3], KindError> {
    let mut out = [false; 3];
    for (slot, pre) in out.iter_mut().zip(["MirrorX", "MirrorY", "MirrorZ"]) {
        let Some(Json::Obj(o)) = s else { continue };
        // (the axis structs' members by their name: the object kind drops a user struct member's GUID suffix)
        let keys: Vec<&String> = o.keys().filter(|k| *k == pre || k.starts_with(&format!("{pre}_"))).collect();
        if keys.len() > 1 {
            return failed(format!("banner: axis struct: {pre} twice"));
        }
        *slot = keys.first().is_some_and(|k| o[*k] == Json::Bool(true));
    }
    Ok(out)
}

/// The class defaults after the parents.
fn defaults_of(cx: &Cx<'_>, chain: &[ChainClass<'_>]) -> Result<Defaults, KindError> {
    let mut p = JsonProps::new();
    for c in chain.iter().rev() {
        p = over_props(&p, Some(&props_of(&cx.object(&cdo_path(&c.path))?)));
    }
    for k in ["MaskedComponents", "UnmaskedComponents", "MirroredComponents"] {
        if p.get(k).is_some_and(json_truthy) {
            return failed(format!("banner: a class default sets {k}"));
        }
    }
    let num = |k: &str| match p.get(k) {
        Some(Json::Num(n)) => Ok(*n),
        _ => failed(format!("banner: class default {k} missing")),
    };
    let flag = |k: &str| p.get(k) == Some(&Json::Bool(true));
    Ok(Defaults {
        fake_ortho: flag("FakeOrtho"),
        fake_ortho_move_up: num("FakeOrthoMoveUp")?,
        old_initialize: flag("OldInitialize"),
        hide_default_border: flag("HideDefaultBorder"),
        strength_h: num("Parallax Strength Horizontal")?,
        strength_v: num("Parallax Strength Vertical")?,
        horizontal_offset: num("Horizontal Offset")?,
        mirror_axis: axis(p.get("MirrorAxis"))?,
        mirror_offset_axis: axis(p.get("MirrorOffsetAxis"))?,
    })
}

/// The names of a class's own functions (its FuncMap).
fn funcs(c: &ChainClass<'_>) -> Result<Vec<String>, KindError> {
    Ok(match c.cls.native()?.as_ref().and_then(|n| n.get("FuncMap")) {
        Some(Value::Map(m)) => m
            .iter()
            .filter_map(|(k, _)| match k {
                Value::Str(s) => Some(s.clone()),
                _ => None,
            })
            .collect(),
        _ => Vec::new(),
    })
}

#[derive(Default)]
struct Arrays {
    masked: Vec<String>,
    unmasked: Vec<String>,
    mirrored: Vec<String>,
}

/// The UCS arrays: the nearest class's UCS run, its parent call where it stands.
fn ucs_arrays(chain: &[ChainClass<'_>]) -> Result<Arrays, KindError> {
    fn run(chain: &[ChainClass<'_>], i: usize, out: &mut Arrays) -> Result<(), KindError> {
        let Some(c) = chain.get(i) else { return Ok(()) };
        let has = funcs(c)?.iter().any(|f| f == "UserConstructionScript");
        if c.name == TEMPLATE {
            if has {
                return failed(format!("banner: {TEMPLATE} has a UCS: read it"));
            }
            return Ok(());
        }
        if !has {
            return run(chain, i + 1, out);
        }
        let Some(steps) = ucs_of(&c.path) else {
            return failed(format!("banner: {} has a UCS and no ucs.rs row: read it", c.path));
        };
        for s in steps {
            match s {
                UcsStep::Parent => run(chain, i + 1, out)?,
                UcsStep::Set(which, names) => {
                    let list = names.iter().map(|n| (*n).to_owned()).collect();
                    match which {
                        UcsSet::Masked => out.masked = list,
                        UcsSet::Unmasked => out.unmasked = list,
                        UcsSet::Mirrored => out.mirrored = list,
                    }
                }
            }
        }
        Ok(())
    }
    let mut out = Arrays::default();
    run(chain, 0, &mut out)?;
    Ok(out)
}

/// The byte a texture's address mode is for Mirror Banner's switch (TA_Wrap 0, TA_Clamp 1, TA_Mirror 2).
fn address(cx: &Cx<'_>, t: &ObjView<'_>, key: &str, r: &str) -> Result<u8, KindError> {
    let v = match t.prop(key).and_then(|v| string(cx.cache, v)) {
        Some(s) => s,
        None if t.prop(key).is_none() => "TA_Wrap".to_owned(),
        None => String::new(),
    };
    // (the enum's `Enum::` prefix dropped: `^\w+::`)
    let bare = match v.find("::") {
        Some(at) if v[..at].bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_') && at > 0 => &v[at + 2..],
        _ => &v[..],
    };
    match bare {
        "TA_Wrap" => Ok(0),
        "TA_Clamp" => Ok(1),
        "TA_Mirror" => Ok(2),
        _ => failed(format!("banner: {r}: address mode {v}")),
    }
}

/// A MaskedLayers material (InitializeBanner's GetBaseMaterial test, TPL IB @5-270 / @1664 / @3146): its base and, per
/// layer, what Mirror Banner's plain-layer branch reads (TPL @9893-14941): K2_GetTextureParameterValue("Layer n")
/// (the instance chain's TextureParameterValues, then the base Material's cached texture defaults,
/// CachedExpressionData.Parameters 'RuntimeEntries[2]' / TextureValues) and that Texture2D's AddressX / AddressY as the
/// byte the switch reads. A layer whose parameter the chain lacks is left out (the read is None, the cast fails: the
/// runtime takes byte 0). `None` for any other material.
fn masked_layers_of(cx: &Cx<'_>, material: &str) -> Result<Option<MaskedLayers>, KindError> {
    if !cx.exists(material) {
        return Ok(None);
    }
    let m = resolve_material(cx, material)?;
    if !MASKED_LAYERS.contains(&m.base.as_str()) {
        return Ok(None);
    }
    let params = match m.prop("CachedExpressionData") {
        Some(Value::Struct(c)) => match field(c, "Parameters") {
            Some(Value::Struct(p)) => Some(p),
            _ => None,
        },
        _ => None,
    };
    let infos: Vec<String> = match params.and_then(|p| field(p, "RuntimeEntries[2]")) {
        Some(Value::Struct(e)) => match field(e, "ParameterInfos") {
            Some(Value::Array(a)) => a
                .iter()
                .map(|i| match i {
                    Value::Struct(s) => field(s, "Name").and_then(|n| string(cx.cache, n)).unwrap_or_default(),
                    _ => String::new(),
                })
                .collect(),
            _ => Vec::new(),
        },
        _ => Vec::new(),
    };
    let values: Vec<Option<String>> = match params.and_then(|p| field(p, "TextureValues")) {
        Some(Value::Array(a)) => a.iter().map(|v| string(cx.cache, v)).collect(),
        _ => Vec::new(),
    };
    if infos.len() != values.len() {
        return failed(format!("banner: {}: texture parameter infos and values differ in length", m.base));
    }
    let mut layers = IndexMap::new();
    for n in 0..=9 {
        let name = format!("Layer {n}");
        let r = match m.textures.get(&name) {
            Some(r) => r.clone(),
            None => infos.iter().position(|i| *i == name).and_then(|i| values[i].clone()),
        };
        let Some(r) = r.filter(|r| !r.is_empty()) else { continue };
        // (an import not in the paks, LoveStorm's Layer 0 under _Unreleased: it does not load, the parameter value is
        // None, the cast fails and the AddressX / AddressY read of None gives 0)
        if !cx.exists(&r) {
            layers.insert(name, Layer { texture: None, address_x: 0, address_y: 0 });
            continue;
        }
        let t = cx.object(&r)?;
        if t.class() != "/Script/Engine.Texture2D" {
            return failed(format!(
                "banner: {material}: {name} ({r}) is a {}, not a Texture2D (the cast fails: read it)",
                t.class()
            ));
        }
        let (ax, ay) = (address(cx, &t, "AddressX", &r)?, address(cx, &t, "AddressY", &r)?);
        layers.insert(name, Layer { texture: Some(r), address_x: ax, address_y: ay });
    }
    Ok(Some(MaskedLayers { base: short_name(&m.base).to_owned(), layers }))
}

// ---------------------------------------------------------------- the systems

/// What a banner needs of a system's emitter: its name, the material it draws and the context that material is named
/// in as the particles kind binds it.
struct EmitterRef {
    name: String,
    material: String,
    ctx: &'static DrawContextRow,
}

/// The emitters of a system as the particles kind binds them (`particles::system::bind_system`), its sprite emitters'
/// context the banner's choice (`bannerRef`, `sprite` of `bindBanner`): each one's name and its Required
/// module's DrawRef parsed back to the material and its context.
fn system_emitters(cx: &Cx<'_>, probes: &BannerProbes, path: &str) -> Result<Vec<EmitterRef>, KindError> {
    let sprite = |m: &str| -> Result<&'static DrawContextRow, KindError> {
        let (r, _) = banner_ref(cx, probes, m, Plain::Sprite)?;
        Ok(parse_draw_ref(&r).ok_or_else(|| KindError::Failed(format!("parseDrawRef: {r}")))?.context)
    };
    let s = bind_system(cx, path, &BindSystemOpts { sprite_context: Some(&sprite), leave_out: None })?;
    let emitters = s.system.get("emitters").and_then(Json::as_array).unwrap_or_default();
    let mut out = Vec::with_capacity(emitters.len());
    for em in emitters {
        let name = em.get("name").and_then(Json::as_str).unwrap_or_default().to_owned();
        let required = em
            .get("modules")
            .and_then(Json::as_array)
            .and_then(|m| m.iter().find(|m| m.get("type").and_then(Json::as_str) == Some("Required")));
        let Some(r) = required.and_then(|r| r.get("Material")).and_then(Json::as_str) else {
            return failed(format!("banner system {path} {name}: no Required module's DrawRef"));
        };
        let Some(d) = parse_draw_ref(r) else { return failed(format!("parseDrawRef: {r}")) };
        out.push(EmitterRef { name, material: d.material.to_owned(), ctx: d.context });
    }
    Ok(out)
}

// ---------------------------------------------------------------- the bind

fn v3(v: Option<&Json>, d: f64) -> V3 {
    match v {
        Some(o @ Json::Obj(_)) => ["X", "Y", "Z"].map(|k| o.get(k).and_then(Json::as_f64).unwrap_or(d)),
        _ => [d; 3],
    }
}

fn rot(v: Option<&Json>) -> V3 {
    match v {
        Some(o @ Json::Obj(_)) => ["Pitch", "Yaw", "Roll"].map(|k| o.get(k).and_then(Json::as_f64).unwrap_or(0.0)),
        _ => [0.0; 3],
    }
}

fn xf(t: Option<&Json>) -> Xform {
    let empty = Json::obj();
    let o = match t {
        Some(o @ Json::Obj(_)) => o,
        _ => &empty,
    };
    let q = match o.get("Rotation") {
        Some(r @ Json::Obj(_)) => r,
        _ => &empty,
    };
    let c = |k: &str, d: f64| q.get(k).and_then(Json::as_f64).unwrap_or(d);
    Xform {
        loc: v3(o.get("Translation"), 0.0),
        quat: [c("X", 0.0), c("Y", 0.0), c("Z", 0.0), c("W", 1.0)],
        scale: v3(o.get("Scale3D"), 1.0),
    }
}

/// What the bind collects of materials: the entry's `materials` and `refused`.
struct Named<'e> {
    materials: &'e mut IndexMap<String, MaterialRecord>,
    refused: &'e mut Vec<String>,
}

impl Named<'_> {
    fn refuse(&mut self, material: &str, why: &str) {
        if !self.refused.iter().any(|r| r.starts_with(&format!("{material}: "))) {
            self.refused.push(format!("{material}: {why}"));
        }
    }

    /// A slot's or a sprite emitter's ref, its record in `materials` when built (false when refused).
    fn named(
        &mut self,
        cx: &Cx<'_>,
        probes: &BannerProbes,
        material: &str,
        plain: Plain,
    ) -> Result<(String, bool), KindError> {
        let (r, probe) = banner_ref(cx, probes, material, plain)?;
        match &*probe {
            BannerProbe::Refused(why) => {
                self.refuse(material, why);
                Ok((r, false))
            }
            BannerProbe::Built { scalars, .. } => {
                self.materials.insert(r.clone(), MaterialRecord { scalars: scalars.clone() });
                Ok((r, true))
            }
        }
    }
}

/// One banner's entry (`bindBanner`).
fn bind_banner(cx: &Cx<'_>, probes: &BannerProbes, row: &BannerRow) -> Result<Entry, KindError> {
    let slug = &row.slug;
    let bd = cx.object(&row.data)?;
    if bd.class() != BANNER_DATA {
        return failed(format!("banner {slug}: {} is a {}", row.data, bd.class()));
    }
    if bd.prop("Slug").and_then(|v| string(cx.cache, v)).as_deref() != Some(slug.as_str()) {
        return failed(format!("banner {slug}: its BannerData has another slug"));
    }
    let els = match bd.prop("BannerElements") {
        Some(Value::Array(a)) => a.as_slice(),
        _ => &[],
    };
    if els.len() != 1 {
        return failed(format!("banner {slug}: {} banner elements", els.len()));
    }
    if bd.prop("ParallaxOriginOffset").is_some_and(crate::objview::truthy) {
        return failed(format!("banner {slug}: ParallaxOriginOffset set (read it)"));
    }
    let Value::Struct(el) = &els[0] else { return failed(format!("banner {slug}: an element that is no struct")) };
    let Some(actor_class) = field(el, "ActorClass").and_then(|v| string(cx.cache, v)) else {
        return failed(format!("banner {slug}: ActorClass is not a reference"));
    };
    let chain = chain_of(cx, &actor_class)?;
    let nodes = components_of(cx, &chain)?;
    let arrays = ucs_arrays(&chain)?;
    for (k, list) in [("masked", &arrays.masked), ("unmasked", &arrays.unmasked), ("mirrored", &arrays.mirrored)] {
        if let Some(n) = list.iter().find(|n| !nodes.contains_key(*n)) {
            return failed(format!("banner {slug}: {k} names {n}, no component"));
        }
    }
    let (Some(root), Some(pivot)) = (nodes.get("DefaultSceneRoot"), nodes.get("Parallax Pivot")) else {
        return failed(format!("banner {slug}: DefaultSceneRoot / Parallax Pivot not as the template has them"));
    };
    if root.parent.is_some() || pivot.parent.as_deref() != Some("DefaultSceneRoot") {
        return failed(format!("banner {slug}: DefaultSceneRoot / Parallax Pivot not as the template has them"));
    }
    for k in ["RelativeLocation", "RelativeRotation", "RelativeScale3D"] {
        if root.props.get(k).is_some_and(json_truthy) {
            return failed(format!("banner {slug}: DefaultSceneRoot {k}"));
        }
    }
    let offset = |k: &str| field(el, k).map(|v| value_json(cx.cache, v));
    let mut refused = Vec::new();
    let mut materials = IndexMap::new();
    let mut masked_layers = IndexMap::new();
    let mut systems: IndexMap<String, SystemRecord> = IndexMap::new();
    let mut components = Vec::new();
    let mut meshes: IndexSet<String> = IndexSet::new();
    for (name, n) in &nodes {
        let kind = match n.ty.as_str() {
            "StaticMeshComponent" => "mesh",
            "ParticleSystemComponent" => "psc",
            _ => continue,
        };
        let Some(parent) = &n.parent else { return failed(format!("banner {slug}: {name} is a root")) };
        let Some(par) = nodes.get(parent) else {
            return failed(format!("banner {slug}: {name} hangs from {parent}, which is no component"));
        };
        if par.ty == "SceneComponent" && parent != "DefaultSceneRoot" && parent != "Parallax Pivot" {
            return failed(format!("banner {slug}: {name} hangs from scene component {parent}"));
        }
        if !matches!(par.ty.as_str(), "SceneComponent" | "StaticMeshComponent" | "ParticleSystemComponent") {
            return failed(format!("banner {slug}: {name} hangs from {}", par.ty));
        }
        let p = &n.props;
        let keys: &[&str] = if kind == "mesh" { &MESH_KEYS } else { &PSC_KEYS };
        if let Some(k) = p.keys().find(|k| !IGNORED.contains(&k.as_str()) && !keys.contains(&k.as_str())) {
            return failed(format!("banner {slug}: {name} sets {k} (read it)"));
        }
        let mut c = Component {
            name: name.clone(),
            kind,
            parent: parent.clone(),
            loc: v3(p.get("RelativeLocation"), 0.0),
            rot: rot(p.get("RelativeRotation")),
            scale: v3(p.get("RelativeScale3D"), 1.0),
            materials: Vec::new(),
            visible: p.get("bVisible") != Some(&Json::Bool(false)),
            hidden_in_game: p.get("bHiddenInGame") == Some(&Json::Bool(true)),
            sort_priority: p.get("TranslucencySortPriority").and_then(Json::as_f64).unwrap_or(0.0),
            mesh: None,
            system: None,
        };
        let draws = c.visible && (!c.hidden_in_game || arrays.unmasked.contains(name));
        let mut named = Named { materials: &mut materials, refused: &mut refused };
        if kind == "mesh" {
            let Some(Json::Str(mesh_path)) = p.get("StaticMesh") else {
                return failed(format!("banner {slug}: {name} has no mesh"));
            };
            c.mesh = Some(mesh_path.clone());
            if draws {
                let slots = &mesh::header(cx, mesh_path)?.slots;
                let over = match p.get("OverrideMaterials") {
                    Some(Json::Arr(a)) => a.as_slice(),
                    _ => &[],
                };
                for (i, s) in slots.iter().enumerate() {
                    let mat = match over.get(i) {
                        Some(Json::Str(m)) => Some(m.as_str()),
                        _ => s.material.as_deref(),
                    };
                    let Some(mat) = mat.filter(|m| !m.is_empty()) else {
                        return failed(format!("banner {slug}: {name} slot {i} has no material"));
                    };
                    let (r, _) = named.named(cx, probes, mat, Plain::Local)?;
                    c.materials.push(r.clone());
                    if !masked_layers.contains_key(&r)
                        && let Some(ml) = masked_layers_of(cx, mat)?
                    {
                        masked_layers.insert(r, ml);
                    }
                }
                meshes.insert(mesh_path.clone());
            }
        } else {
            let Some(Json::Str(path)) = p.get("Template") else {
                return failed(format!("banner {slug}: {name} has no system"));
            };
            let sn = system_name(path).to_owned();
            c.system = Some(sn.clone());
            // (bAutoActivate false would need its own activation rule; none of the released banners sets it)
            if p.get("bAutoActivate") == Some(&Json::Bool(false)) {
                return failed(format!("banner {slug}: {name} bAutoActivate false (read it)"));
            }
            if draws {
                let mut kept: Vec<String> = Vec::new();
                for em in system_emitters(cx, probes, path)? {
                    let built = if matches!(em.ctx.name, "sprite" | "bannersprite") {
                        let (r, built) = named.named(cx, probes, &em.material, Plain::Sprite)?;
                        if built {
                            kept.push(r);
                        }
                        built
                    } else {
                        // (a mesh or trail emitter: no banner context of those; its build in the context it is named in)
                        let probe = banner_probe(cx, probes, &em.material, em.ctx)?;
                        let r = draw_ref(&em.material, em.ctx)?;
                        match &*probe {
                            BannerProbe::Built { reads: true, .. } => {
                                return failed(format!(
                                    "banner {slug}: {sn} {}: {} reads the custom stencil in a {} emitter (no banner \
                                     context of it)",
                                    em.name, em.material, em.ctx.name
                                ));
                            }
                            BannerProbe::Built { scalars, .. } => {
                                named.materials.insert(r.clone(), MaterialRecord { scalars: scalars.clone() });
                                kept.push(r);
                                true
                            }
                            BannerProbe::Refused(why) => {
                                named.refuse(&em.material, why);
                                false
                            }
                        }
                    };
                    if !built {
                        named.refused.push(format!("{sn} / {}: left out (its material is refused)", em.name));
                    }
                }
                if let Some(prev) = named.systems_prev(&systems, &sn)
                    && prev != path
                {
                    return failed(format!("banner {slug}: two systems named {sn}"));
                }
                systems.insert(sn, SystemRecord { path: path.clone(), emitters: kept.len() });
                c.materials = kept.into_iter().collect::<IndexSet<_>>().into_iter().collect();
            }
        }
        components.push(c);
    }
    let parents = chain[1..].iter().map(|c| c.name.clone()).collect();
    let graphs = chain
        .iter()
        .filter(|c| c.name != TEMPLATE)
        .map(|c| Ok((c, funcs(c)?)))
        .collect::<Result<Vec<_>, KindError>>()?
        .into_iter()
        .filter(|(_, f)| f.iter().any(|f| f != "UserConstructionScript"))
        .map(|(c, _)| c.name.clone())
        .collect();
    Ok(Entry {
        kind: "banner",
        version: BANNER_FORMAT_VERSION,
        slug: slug.clone(),
        asset: short_name(&row.data).to_owned(),
        data: row.data.clone(),
        cls: chain[0].name.clone(),
        parents,
        defaults: defaults_of(cx, &chain)?,
        masked: arrays.masked,
        unmasked: arrays.unmasked,
        mirrored: arrays.mirrored,
        pivot: Pivot {
            loc: v3(pivot.props.get("RelativeLocation"), 0.0),
            rot: rot(pivot.props.get("RelativeRotation")),
            scale: v3(pivot.props.get("RelativeScale3D"), 1.0),
        },
        base_offset: xf(offset("BaseOffset").as_ref()),
        slant_offset: xf(offset("SlantOffset").as_ref()),
        components,
        materials,
        masked_layers,
        systems,
        meshes: meshes.into_iter().collect(),
        graphs,
        refused,
    })
}

impl Named<'_> {
    /// The path an earlier component's system of that short name has.
    fn systems_prev<'s>(&self, systems: &'s IndexMap<String, SystemRecord>, sn: &str) -> Option<&'s str> {
        systems.get(sn).map(|s| s.path.as_str())
    }
}

/// The index entry over the rows (`bindBannerIndex`).
fn bind_index(cx: &Cx<'_>, rows: &[BannerRow]) -> Result<Index, KindError> {
    let mut banners = IndexMap::new();
    for r in rows {
        let bd = cx.object(&r.data)?;
        let cls = match bd.prop("BannerElements") {
            Some(Value::Array(a)) => match a.first() {
                Some(Value::Struct(e)) => field(e, "ActorClass").and_then(|v| string(cx.cache, v)),
                _ => None,
            },
            _ => None,
        };
        let Some(cls) = cls else { return failed(format!("banner: {} ActorClass is not a reference", r.slug)) };
        if banners.contains_key(&r.slug) {
            return failed(format!("banner: slug {} twice", r.slug));
        }
        banners.insert(
            r.slug.clone(),
            IndexLine {
                path: banner_path(&r.data),
                asset: short_name(&r.data).to_owned(),
                cls: short_name(&cls).to_owned(),
            },
        );
    }
    Ok(Index { kind: "index", version: BANNER_FORMAT_VERSION, banners })
}

// ---------------------------------------------------------------- the namers' readers

#[derive(Deserialize)]
struct NamedEntry {
    kind: String,
    materials: Option<IndexMap<String, IgnoredAny>>,
    systems: Option<IndexMap<String, NamedSystem>>,
}

#[derive(Deserialize)]
struct NamedSystem {
    path: String,
}

fn named(bytes: &[u8]) -> Result<NamedEntry, KindError> {
    serde_json::from_slice(bytes).map_err(|e| KindError::Failed(format!("an entry: {e}")))
}

/// The DrawRefs an entry names (`bannerDrawRefs`: a banner entry's materials; the index names none), what the
/// material kind's list reads.
pub fn draw_refs(bytes: &[u8], out: &mut IndexSet<String>) -> Result<(), KindError> {
    let v = named(bytes)?;
    if v.kind == "banner" {
        out.extend(v.materials.into_iter().flat_map(IndexMap::into_keys));
    }
    Ok(())
}

/// The particle systems an entry's components spawn (`bannerParticleSystems`): the particles kind binds them.
pub fn particle_systems(bytes: &[u8]) -> Result<Vec<String>, KindError> {
    let v = named(bytes)?;
    Ok(if v.kind == "banner" {
        v.systems.into_iter().flat_map(IndexMap::into_values).map(|s| s.path).collect()
    } else {
        Vec::new()
    })
}

pub struct Banner;

impl Kind for Banner {
    fn id(&self) -> &'static str {
        "banner"
    }

    fn version(&self) -> u64 {
        banner_kind_version()
    }

    fn content_type(&self, _: &[u8]) -> &'static str {
        "application/json"
    }

    /// Each released banner's entry (`bannerRows`, by its BannerData), then the index.
    fn list(&self, cx: &Cx<'_>) -> Result<Vec<Item>, KindError> {
        let rows = banner_rows(cx)?;
        let mut out: Vec<Item> = rows.iter().map(|r| Item::new(banner_path(&r.data))).collect();
        out.push(Item::new(banner_index_path()));
        Ok(out)
    }

    fn build(&self, cx: &Cx<'_>, item: &Item) -> Result<Vec<u8>, KindError> {
        let rows = banner_rows(cx)?;
        if item.path == banner_index_path() {
            return Ok(to_vec(&bind_index(cx, &rows)?)?);
        }
        // (the BannerData of an entry path of this bind's version: `bannerData`)
        let dir = banner_dir();
        let data = item.path.strip_prefix(&format!("{dir}/")).map(|d| format!("/{d}"));
        let Some(row) = data.and_then(|d| rows.iter().find(|r| r.data == d)) else {
            return failed(format!("banner-import: no released banner for {}", item.path));
        };
        let probes = cx.shared.get_or_init(BannerProbes::default);
        Ok(to_vec(&bind_banner(cx, &probes, row)?)?)
    }
}
