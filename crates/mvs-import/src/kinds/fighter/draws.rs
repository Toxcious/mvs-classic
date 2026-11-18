//! What a fighter's meshes are drawn with (the fighter entry's `skinSlots` and `actors`): per skin and mesh the
//! material of each slot by team variant, and per spawned actor or prop the materials its Blueprint sets, each a
//! DrawRef (the skin context on a skeletal mesh, locallm on a static one).
//!
//! The drawmat kind builds what these name; a ref it refuses has no entry and is not drawn. Read through the object
//! kind (the skins) and the mesh kind (each mesh's slots and colour stream: SkeletalMaterials is native data the
//! object kind does not hold).
//!
//! The skins (Comp_Pawn_SkinManager ApplySkinToPawn / ApplySubMeshes): every skin the picker offers
//! (FighterData.skins, the default first) and the pawn's further skins (the port's extra skins: Steven's
//! WatermelonSteven, on his minion), each by its SkinData asset name: its MeshOverride ('') and its skeletal
//! SubMeshes (a null Mesh clears the component: nothing; a static one is an actor entry below), each slot the mesh's
//! own material (a null one draws with UMaterial::GetDefaultMaterial(MD_Surface), WorldGridMaterial:
//! C015Core_SkelMesh's Taz_Mat, which the skin overrides, C015_Mouth3_SkelMesh's second slot, which it does not)
//! unless the skin's Materials entry of that index sets one for the variant (SelectMaterialColor: Red / Blue /
//! LocalPlayer, and MenuMaterialOverride falling back to LocalPlayer for the menus). An override past the mesh's
//! slots names none (C017_S00's BodyMesh has three overrides named IronGiant_MAT). `skinned`: the slots an override
//! covers (BPI_AddMaterialInstance: the hit flash list).
//!
//! The actors (`table.rs` ACTOR_TABLE, ported from the actors' graphs), then the rules that follow the table:
//! - skin statics: a skin's static SubMeshes as actors keyed `<skin>.<SubMeshes key>` (ApplySubMeshes:
//!   SetStaticMesh, then the entry's Materials, SelectMaterialColor's LocalPlayer / Red / Blue per slot, the mesh's
//!   own where the entry has none; the view picks the set as a team's SetTeamColor does), drawn by that skin's
//!   matches. The default skin's stand where the table names them (Bugs, Rick, Taz, Velma); every pickable skin's
//!   that no table row keys follow.
//! - The pawn template's mesh components a skin keeps because its SubMeshes lack their key (Rick C020_S02's Blaster,
//!   Iron Giant S01 / S02 / S04's scrap and junkyard car, Bugs Bunny's Brunhilde ears on every skin but S03,
//!   Gizmo C023A_S00's FireArrowNullMesh): the mesh's own materials, no team pick, keyed by the mesh's asset name,
//!   the character's. A component with a slot of no material is left out (nothing of it draws).
//! - Jake's stretch on a skin of his other than the default (whose pieces, Jake_BotHalf_SkelMesh and
//!   Jake_StretchMesh, are table rows): the spline body pieces are dynamic instances of the skin's Materials[0]
//!   (JakeStretch @2976-3048), on S01_Stretch for the Cake skins (@2691, IsCake @5198-5616: Jake_S01, C005_S03),
//!   Jake_StretchMesh otherwise; the lower body takes the pawn mesh's slot 0 (@5898-5971), on S01_LowerBody_Split for
//!   the Cake skins (@5635; the mesh has no material of its own) and on Jake_BotHalf_SkelMesh otherwise (the skin's
//!   body). Keyed `<skin>.Stretch` / `<skin>.LowerBody`, drawn by that skin's matches.
//!
//! `whiteVertexColor`: the mesh's LOD 0 has no colour stream.
// Unverified: instances of one name under one component are taken as separate objects (4.26 source:
// UMaterialInstanceDynamic::Create renames on a collision), not read in the exe.
// Unverified: a static mesh without a colour stream draws with the null colour buffer (FLocalVertexFactory's
// GNullColorVertexBuffer, 4.26 source); a skeletal one without bHasVertexColors the same (FGPUBaseSkinVertexFactory,
// from memory of the 4.26 source).

use indexmap::{IndexMap, IndexSet};
use mvs_data::fighter::FighterData;
use mvs_data::value::Value as Data;
use mvs_uasset::value::{Props, Value};
use serde::{Deserialize, Serialize};

use super::table::{ACTOR_TABLE, ActorRow, Row};
use crate::fighter_port::{FighterPort, asset_name, asset_of, character_data_of, pkg_of, port_default_skin};
use crate::kind::{Cx, KindError, Memo};
use crate::kinds::material::resolve::{BaseOverride, conv_string, overridden, resolve_material};
use crate::kinds::mesh;
use crate::objview::{field, string};

/// A list of refs, a slot with none null.
pub type Slots = Vec<Option<String>>;

/// One mesh of a skin: the slots a material override covers and each slot's
/// DrawRef by team variant.
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SkinMesh {
    #[serde(default)]
    pub skinned: Vec<bool>,
    pub red: Slots,
    pub blue: Slots,
    pub local: Slots,
    pub menu: Slots,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub white_vertex_color: Option<bool>,
}

/// An actor or prop.
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Actor {
    #[serde(default)]
    pub mesh: String,
    pub materials: Slots,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub variants: Option<IndexMap<String, Slots>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub white_vertex_color: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub skin: Option<String>,
}

/// A fighter's draws: the entry's `skinSlots` (per skin asset name, per mesh key) and `actors`.
#[derive(Debug, Default)]
pub struct Draws {
    pub skin_slots: Option<IndexMap<String, IndexMap<String, SkinMesh>>>,
    pub actors: Option<IndexMap<String, Actor>>,
}

/// UMaterial::GetDefaultMaterial(MD_Surface): a slot with no material.
pub(crate) const WORLD_GRID: &str = "/Engine/EngineMaterials/WorldGridMaterial.WorldGridMaterial";
const JAKE_CAKE: [&str; 2] = ["Jake_S01", "C005_S03"];

fn failed<T>(why: String) -> Result<T, KindError> {
    Err(KindError::Failed(why))
}

/// A non-empty string value (`str`).
fn str_of(cx: &Cx<'_>, v: Option<&Value>) -> Option<String> {
    v.and_then(|v| string(cx.cache, v)).filter(|s| !s.is_empty())
}

/// An array property's struct elements (`structs`: a non-array is none; a non-struct element reads as no members).
fn structs(v: Option<&Value>) -> Vec<Props> {
    match v {
        Some(Value::Array(a)) => a
            .iter()
            .map(|e| match e {
                Value::Struct(p) => p.clone(),
                _ => Props::default(),
            })
            .collect(),
        _ => Vec::new(),
    }
}

/// A map property's entries (`Array.isArray(sp.SubMeshes) ? sp.SubMeshes : []`): key and struct value.
fn sub_meshes(cx: &Cx<'_>, v: Option<&Value>) -> Vec<(String, Props)> {
    let Some(Value::Map(m)) = v else { return Vec::new() };
    m.iter()
        .filter_map(|(k, v)| {
            let k = string(cx.cache, k)?;
            let Value::Struct(p) = v else { return Some((k, Props::default())) };
            Some((k, p.clone()))
        })
        .collect()
}

/// A package as the table spells it (below /Game/, or Engine/Content/...) as its asset's object path; `<pkg>.<object>`
/// where the object's name is not the package's (C017_SUbmeshes_MIC_Red holds C017_Submeshes_MIC_Red).
fn table_object(pkg: &str) -> String {
    let p = match pkg.strip_prefix("Engine/Content/") {
        Some(rest) => format!("/Engine/{rest}"),
        None => format!("/Game/{pkg}"),
    };
    let slash = p.rfind('/').unwrap_or(0);
    if p[slash..].contains('.') { p } else { asset_of(&p) }
}

/// What a draws bind reads of a mesh (its kind, each slot's material object path, whether LOD 0 lacks a colour
/// stream).
pub(crate) struct MeshInfo {
    pub skeletal: bool,
    pub slots: Vec<Option<String>>,
    pub white: bool,
}

pub(crate) fn mesh_info(cx: &Cx<'_>, path: &str) -> Result<MeshInfo, KindError> {
    let h = mesh::header(cx, path)?;
    Ok(MeshInfo {
        skeletal: h.skinned(),
        slots: h.slots.iter().map(|s| s.material.clone()).collect(),
        white: h.lods.first().is_none_or(|l| !l.streams.contains_key("color")),
    })
}

/// The run's verdict per material: whether a slot of it is let through to the drawmat kind.
#[derive(Default)]
struct Drawn(Memo<Result<bool, KindError>>);

/// Whether a material is named (`slotReader`'s `drawn`): lit translucency of another shading model than DefaultLit
/// is not let through. Surface translucency (TLM_Surface*: Superman_Cornea_Mat, the glass of JuiceGlass_MAT,
/// BottleGlass_MAT, Windshield_MAT, ClockGlass_MAT) is: the material kind builds it with its cooked translucent
/// base pass shaders, which the renderer's translucency pass draws.
// The native entry names the TLM_Surface* slots (an earlier slot reader left them empty: its drawmat skin context
// could not be bound for them).
fn drawn(cx: &Cx<'_>, path: &str) -> Result<bool, KindError> {
    cx.shared.get_or_init(Drawn::default).0.get(path, || {
        let m = resolve_material(cx, path)?;
        let (blend, _) = overridden(
            cx,
            &m,
            BaseOverride::BlendMode,
            "BLEND_Opaque".to_owned(),
            "BLEND_Opaque".to_owned(),
            conv_string,
        )?;
        let (shading, _) = overridden(
            cx,
            &m,
            BaseOverride::ShadingModel,
            "MSM_DefaultLit".to_owned(),
            "MSM_DefaultLit".to_owned(),
            conv_string,
        )?;
        let translucent = blend == "BLEND_Translucent" || blend == "BLEND_Additive";
        let base_unlit = str_of(cx, m.prop("ShadingModel")).as_deref() == Some("MSM_Unlit");
        let unlit = base_unlit && (blend == "BLEND_Opaque" || blend == "BLEND_Masked" || translucent);
        if !translucent || unlit {
            return Ok(true);
        }
        Ok(shading == "MSM_DefaultLit")
    })
}

/// A slot's DrawRef, or null where the slot is left empty (not drawn): `<material>?ctx=<ctx>` (`drawRef`).
fn slot_ref(cx: &Cx<'_>, path: &str, ctx: &str) -> Result<Option<String>, KindError> {
    if !drawn(cx, path)? {
        return Ok(None);
    }
    if !path.starts_with('/') || !path.contains('.') || path.contains('?') {
        return failed(format!("drawRef: {path} is not a <package>.<export> path"));
    }
    Ok(Some(format!("{path}?ctx={ctx}")))
}

pub(crate) fn slot_refs(cx: &Cx<'_>, l: &[String], ctx: &str) -> Result<Slots, KindError> {
    l.iter().map(|p| slot_ref(cx, p, ctx)).collect()
}

/// A mesh's object path by its export name.
fn mesh_path<'d>(data: &'d FighterData, name: &str) -> Option<&'d str> {
    let m = &data.paths.meshes;
    if let Some(v) = m.get(name) {
        return Some(v);
    }
    let lower = name.to_lowercase();
    m.iter().find(|(k, _)| k.to_lowercase() == lower).map(|(_, v)| v.as_str())
}

/// A row of the actors: the table's (its packages as object paths) or a skin's.
struct DrawRow {
    cls: String,
    mesh: String,
    materials: Vec<String>,
    variants: Option<Vec<(String, Vec<String>)>>,
    is_static: bool,
    skin: Option<String>,
}

fn table_row(r: &ActorRow) -> DrawRow {
    DrawRow {
        cls: r.cls.to_owned(),
        mesh: table_object(r.mesh),
        materials: r.materials.iter().map(|m| table_object(m)).collect(),
        variants: r
            .variants
            .map(|v| v.iter().map(|(k, l)| ((*k).to_owned(), l.iter().map(|m| table_object(m)).collect())).collect()),
        is_static: r.is_static,
        skin: None,
    }
}

const VARIANTS: [(&str, &str); 4] = [
    ("red", "RedMaterialOverride"),
    ("blue", "BlueMaterialOverride"),
    ("local", "LocalPlayerMaterialOverride"),
    ("menu", "MenuMaterialOverride"),
];

/// The fighter's draws, bound from its fighter data (`bindFighterDraws`).
pub fn bind_fighter_draws(
    cx: &Cx<'_>,
    key: &str,
    port: &FighterPort,
    character_data: Option<&str>,
    data: &FighterData,
) -> Result<Draws, KindError> {
    let picks: Vec<String> = data.skins.iter().flat_map(IndexMap::keys).cloned().collect();
    let extra: &[&str] = port.extra_skins.unwrap_or(&[]);
    // (a key with no skin draws nothing of its own: a table row naming it is an error, not a row dropped)
    if picks.is_empty() && extra.is_empty() {
        let rows: Vec<&str> = ACTOR_TABLE
            .iter()
            .filter_map(|r| match r {
                Row::SkinStatics(k) => (*k == key).then_some("skinStatics"),
                Row::Actor(a) => a.owner.contains(&key).then_some(a.cls),
            })
            .collect();
        if !rows.is_empty() {
            return failed(format!("fighter draws {key}: no skins, and ACTOR_TABLE names it ({})", rows.join(", ")));
        }
        return Ok(Draws::default());
    }
    let skin_props = |pkg: &str| -> Result<Props, KindError> { Ok(cx.object(&asset_of(pkg))?.props().clone()) };

    // the picker's skins by asset name (FighterData.skins' keys: the default, then the CharacterData's Skins)
    let cd = character_data_of(cx, character_data)?;
    let mut pkgs: IndexMap<String, String> = IndexMap::new();
    let mut candidates = vec![port_default_skin(cx, key, port, cd.as_ref())?];
    if let Some(Value::Array(skins)) = cd.as_ref().and_then(|c| c.prop("Skins")) {
        candidates.extend(
            skins.iter().filter_map(|v| string(cx.cache, v)).filter(|x| !x.is_empty()).map(|x| pkg_of(&x).to_owned()),
        );
    }
    for p in candidates {
        pkgs.entry(asset_name(&p).to_owned()).or_insert(p);
    }
    let pick_pkg = |n: &str| -> Result<String, KindError> {
        match pkgs.get(n) {
            Some(p) => Ok(p.clone()),
            None => {
                failed(format!("fighter draws {key}: skin {n} is neither the default nor in the CharacterData's Skins"))
            }
        }
    };

    // ---- the skins
    let skin_mesh = |path: &str, overrides: &[Props]| -> Result<SkinMesh, KindError> {
        let m = mesh_info(cx, path)?;
        if !m.skeletal {
            return failed(format!("fighter draws {key}: skin mesh {path} is not a SkeletalMesh"));
        }
        let own: Vec<String> = m.slots.iter().map(|x| x.clone().unwrap_or_else(|| WORLD_GRID.to_owned())).collect();
        let mut out = SkinMesh {
            skinned: (0..own.len()).map(|i| i < overrides.len()).collect(),
            red: Vec::new(),
            blue: Vec::new(),
            local: Vec::new(),
            menu: Vec::new(),
            white_vertex_color: None,
        };
        for (variant, prop) in VARIANTS {
            let mut set = own.clone();
            for (k, o) in overrides.iter().enumerate() {
                let mat = str_of(cx, field(o, prop)).or_else(|| {
                    (variant == "menu").then(|| str_of(cx, field(o, "LocalPlayerMaterialOverride"))).flatten()
                });
                if let (Some(slot), Some(mat)) = (set.get_mut(k), mat) {
                    *slot = mat;
                }
            }
            let refs = slot_refs(cx, &set, "skin")?;
            match variant {
                "red" => out.red = refs,
                "blue" => out.blue = refs,
                "local" => out.local = refs,
                _ => out.menu = refs,
            }
        }
        if m.white {
            out.white_vertex_color = Some(true);
        }
        Ok(out)
    };
    let mut skin_slots: IndexMap<String, IndexMap<String, SkinMesh>> = IndexMap::new();
    let skin_pkgs: Vec<String> =
        picks.iter().map(|p| pick_pkg(p)).chain(extra.iter().map(|e| Ok((*e).to_owned()))).collect::<Result<_, _>>()?;
    for pkg in &skin_pkgs {
        let sp = skin_props(pkg)?;
        let Some(main) = str_of(cx, field(&sp, "MeshOverride")) else {
            return failed(format!("fighter draws {key}: skin {pkg} has no MeshOverride"));
        };
        let mut out: IndexMap<String, SkinMesh> = IndexMap::new();
        out.insert(String::new(), skin_mesh(&main, &structs(field(&sp, "Materials")))?);
        for (k, v) in sub_meshes(cx, field(&sp, "SubMeshes")) {
            let Some(mp) = str_of(cx, field(&v, "Mesh")) else { continue };
            if !mesh_info(cx, &mp)?.skeletal {
                continue;
            }
            out.insert(k, skin_mesh(&mp, &structs(field(&v, "Materials")))?);
        }
        skin_slots.insert(asset_name(pkg).to_owned(), out);
    }

    // ---- the actors
    let mut rows: Vec<DrawRow> = Vec::new();
    let skin_statics = |skin: &str| -> Result<Vec<DrawRow>, KindError> {
        let sp = skin_props(&pick_pkg(skin)?)?;
        let mut out = Vec::new();
        for (k, v) in sub_meshes(cx, field(&sp, "SubMeshes")) {
            let Some(mp) = str_of(cx, field(&v, "Mesh")) else { continue };
            let m = mesh_info(cx, &mp)?;
            if m.skeletal {
                continue;
            }
            let entry = structs(field(&v, "Materials"));
            let set = |prop: &str| -> Result<Vec<String>, KindError> {
                m.slots
                    .iter()
                    .enumerate()
                    .map(|(i, sl)| {
                        let own = entry.get(i).and_then(|e| str_of(cx, field(e, prop)));
                        match own.or_else(|| sl.clone().filter(|s| !s.is_empty())) {
                            Some(p) => Ok(p),
                            None => failed(format!("fighter draws {key}: {skin}.{k} slot {i} has no material")),
                        }
                    })
                    .collect()
            };
            out.push(DrawRow {
                cls: format!("{skin}.{k}"),
                mesh: asset_of(&mp),
                materials: set("LocalPlayerMaterialOverride")?,
                variants: Some(vec![
                    ("red".to_owned(), set("RedMaterialOverride")?),
                    ("blue".to_owned(), set("BlueMaterialOverride")?),
                ]),
                is_static: true,
                skin: Some(skin.to_owned()),
            });
        }
        Ok(out)
    };
    let hand_keys: IndexSet<&str> = ACTOR_TABLE
        .iter()
        .filter_map(|r| match r {
            Row::Actor(a) => Some(a.cls),
            Row::SkinStatics(_) => None,
        })
        .collect();
    for r in ACTOR_TABLE {
        match r {
            Row::SkinStatics(k) => {
                if *k == key {
                    let Some(first) = picks.first() else {
                        return failed(format!("fighter draws {key}: skinStatics with no skin"));
                    };
                    rows.extend(skin_statics(first)?);
                }
            }
            Row::Actor(a) => {
                if a.owner.contains(&key) {
                    rows.push(table_row(a));
                }
            }
        }
    }
    let have = |rows: &[DrawRow], cls: &str| hand_keys.contains(cls) || rows.iter().any(|r| r.cls == cls);
    for skin in &picks {
        for r in skin_statics(skin)? {
            if !have(&rows, &r.cls) {
                rows.push(r);
            }
        }
    }
    if !picks.is_empty() {
        let pick_pkgs: Vec<String> = picks.iter().map(|p| pick_pkg(p)).collect::<Result<_, _>>()?;
        for t in template_meshes(cx, key, data, &pick_pkgs)? {
            if have(&rows, &t.name) {
                continue;
            }
            let m = mesh_info(cx, &t.mesh)?;
            // (a slot with no material: nothing of it is drawn)
            if m.slots.iter().any(Option::is_none) {
                continue;
            }
            rows.push(DrawRow {
                cls: t.name,
                mesh: asset_of(&t.mesh),
                materials: m.slots.into_iter().flatten().collect(),
                variants: None,
                is_static: t.is_static,
                skin: None,
            });
        }
    }
    if key == "jake" {
        for skin in picks.iter().skip(1) {
            let sp = skin_props(&pick_pkg(skin)?)?;
            let m0 = structs(field(&sp, "Materials")).into_iter().next().unwrap_or_default();
            let (local, red, blue) = (
                str_of(cx, field(&m0, "LocalPlayerMaterialOverride")),
                str_of(cx, field(&m0, "RedMaterialOverride")),
                str_of(cx, field(&m0, "BlueMaterialOverride")),
            );
            let (Some(local), Some(red), Some(blue)) = (local, red, blue) else {
                return failed(format!("fighter draws jake: skin {skin} has no Materials[0] overrides"));
            };
            let cake = JAKE_CAKE.contains(&skin.as_str());
            let variants = vec![("red".to_owned(), vec![red]), ("blue".to_owned(), vec![blue])];
            rows.push(DrawRow {
                cls: format!("{skin}.Stretch"),
                mesh: table_object(&format!(
                    "Panda_Main/Characters/Jake/Mesh/{}",
                    if cake { "S01_Stretch" } else { "Jake_StretchMesh" }
                )),
                materials: vec![local.clone()],
                variants: Some(variants.clone()),
                is_static: true,
                skin: Some(skin.clone()),
            });
            if cake {
                rows.push(DrawRow {
                    cls: format!("{skin}.LowerBody"),
                    mesh: table_object("Panda_Main/Characters/Jake/Mesh/S01_LowerBody_Split"),
                    materials: vec![local],
                    variants: Some(variants),
                    is_static: false,
                    skin: Some(skin.clone()),
                });
            }
        }
    }

    let mut actors: IndexMap<String, Actor> = IndexMap::new();
    for r in &rows {
        let m = mesh_info(cx, &r.mesh)?;
        if m.skeletal == r.is_static {
            return failed(format!(
                "fighter draws {key}: {}: {} is {}",
                r.cls,
                r.mesh,
                if m.skeletal { "skeletal" } else { "static" }
            ));
        }
        if m.slots.len() != r.materials.len() {
            return failed(format!(
                "fighter draws {key}: {}: {} slots vs {} materials",
                r.cls,
                m.slots.len(),
                r.materials.len()
            ));
        }
        for (_, v) in r.variants.iter().flatten() {
            if v.len() != m.slots.len() {
                return failed(format!("fighter draws {key}: {}: a variant with {} materials", r.cls, v.len()));
            }
        }
        let ctx = if r.is_static { "locallm" } else { "skin" };
        let variants = match &r.variants {
            Some(v) => Some(
                v.iter()
                    .map(|(name, l)| Ok((name.clone(), slot_refs(cx, l, ctx)?)))
                    .collect::<Result<IndexMap<_, _>, KindError>>()?,
            ),
            None => None,
        };
        let a = Actor {
            mesh: r.mesh.clone(),
            materials: slot_refs(cx, &r.materials, ctx)?,
            variants,
            white_vertex_color: m.white.then_some(true),
            skin: r.skin.clone(),
        };
        if actors.insert(r.cls.clone(), a).is_some() {
            return failed(format!("fighter draws {key}: two actors {}", r.cls));
        }
    }
    Ok(Draws {
        skin_slots: (!skin_slots.is_empty()).then_some(skin_slots),
        actors: (!actors.is_empty()).then_some(actors),
    })
}

/// A pawn template's mesh component a skin keeps.
struct Template {
    name: String,
    mesh: String,
    is_static: bool,
}

/// The pawn template's mesh components a skin keeps (skins_triage.md 2: Comp_Pawn_SkinManager.ApplySubMeshes
/// @43-2205 visits the skin's SubMeshes keys alone, matched to a component by its ComponentTags, so a component none
/// of whose tags the skin names keeps its template mesh, drawn in the mesh's own material with no team pick), from the
/// fighter data's `components.pawn`: a component that no skin of the character names is not a skin's part at all (the
/// main mesh, an actor's look), nor one every skin names. Each mesh by its name among the character's meshes
/// (FighterData.paths), an engine shape under /Engine/BasicShapes.
fn template_meshes(
    cx: &Cx<'_>,
    key: &str,
    data: &FighterData,
    skin_pkgs: &[String],
) -> Result<Vec<Template>, KindError> {
    let mut sets: Vec<IndexSet<String>> = Vec::new();
    for p in skin_pkgs {
        let o = cx.object(&asset_of(p))?;
        sets.push(sub_meshes(cx, o.prop("SubMeshes")).into_iter().map(|(k, _)| k.to_lowercase()).collect());
    }
    let mut out = Vec::new();
    for c in data.components.pawn.values() {
        let is_static = c.ty == "StaticMeshComponent";
        let member = |name: &str| c.p.as_obj().and_then(|o| o.get(name));
        let name = if is_static {
            member("StaticMesh")
        } else if c.ty == "SkeletalMeshComponent" {
            member("SkeletalMesh")
        } else {
            None
        };
        let Some(name) = name.and_then(Data::as_str).filter(|n| !n.is_empty()) else { continue };
        let tags: Vec<String> = match member("ComponentTags") {
            Some(Data::Arr(a)) => a.iter().filter_map(Data::as_str).map(str::to_lowercase).collect(),
            _ => Vec::new(),
        };
        if !sets.iter().any(|k| tags.iter().any(|t| k.contains(t))) {
            continue;
        }
        if !sets.iter().any(|k| !tags.iter().any(|t| k.contains(t))) {
            continue;
        }
        if let Some(Data::Arr(a)) = member("OverrideMaterials")
            && a.iter().any(data_truthy)
        {
            return failed(format!("fighter draws {key}: template mesh {name} has OverrideMaterials: not read here"));
        }
        let path = mesh_path(data, name).map_or_else(|| format!("/Engine/BasicShapes/{name}"), str::to_owned);
        out.push(Template { name: name.to_owned(), mesh: path, is_static });
    }
    Ok(out)
}

/// A JSON value's JavaScript truth (`.some(Boolean)`).
fn data_truthy(v: &Data) -> bool {
    match v {
        Data::Null => false,
        Data::Bool(b) => *b,
        Data::Num(n) => *n != 0.0 && !n.is_nan(),
        Data::Str(s) => !s.is_empty(),
        Data::Arr(_) | Data::Obj(_) | Data::F32s(_) => true,
    }
}
