//! The index entry: the systems the kind binds and who can draw each system,
//! material, mesh and decal.
//!
//! Who can draw each system comes from the store: the folder of each fighter (`fighter_port`), the fighters' data a
//! fighter names a system in (the gamedata kind's `fighter`), the stickers'
//! EmoteData and the ring-outs' RingOutVfxData assets, and the owner tables ([`super::tables`]). The systems are
//! [`super::systems`] with the stickers'. The materials are DrawRefs (the emitters' contexts; the component materials
//! in their `#local` / `#spline` contexts, the decals in `decal`).
//!
//! - the data a fighter names a system in is its fighter data; the items part's, owner `items`, is the gamedata
//!   kind's items part;
//! - each [`PERK_FOR`] perk is checked against the build's PerkData assets (the perk folder's, an EnabledForDebugging
//!   one left out): a perk id is its PerkData export's name, matched exactly, as the runtime matches a player's perk
//!   to its owner (a plain string compare; package Perk_Batman_HEavyGrapple holds export Perk_Batman_HeavyGrapple);
//! - a material, mesh or decal the material kind could not build keeps its owners here: the effect player leaves out
//!   what has no entry.

use std::sync::Arc;

use indexmap::{IndexMap, IndexSet};
use mvs_material::draw::context;
use mvs_uasset::value::Value;
use serde::{Deserialize, Serialize};

use super::PARTICLES_FORMAT_VERSION;
use super::system::SystemValue;
use super::systems::{EFFECT_SYSTEMS, RINGOUT_SYSTEMS};
use super::tables::{
    BASE_SYSTEMS, COMPONENT_FOR, COMPONENT_MATERIALS, COMPONENT_MESHES, DECALS, NOT_FOLDER, PERK_FOR, PERK_ONLY,
    SCREEN_SYSTEMS, SPAWNED_FOR,
};
use crate::fighter_port::FIGHTER_PORT;
use crate::game::{BANNER, MENUSCENE, SCENERY, chars, system_name};
use crate::json::{Json, props_json};
use crate::kind::{Cx, KindError};
use crate::kinds::banner::{self, stencil::draw_ref};
use crate::kinds::scenery;
use crate::objview::short_name;

const EMOTE_DATA: &str = "/Script/MultiVersus.EmoteData";
const RINGOUT_DATA: &str = "/Script/MultiVersus.RingOutVfxData";
const RINGOUT_DIR: &str = "/Game/Panda_Main/Blueprints/Rewards/RingOutVfx";
const PERK_DATA_FOLDER: &str = "/Game/Panda_Main/Blueprints/Perks/PerkData";
const PERK_DATA_CLASS: &str = "/Script/MultiVersus.PerkData";
/// The menuscene entry's magic: the kind is phase 4's, its header's
/// `systems` is what the particles kind reads.
const MENUSCENE_MAGIC: &[u8; 4] = b"MVMS";

fn failed(what: String) -> KindError {
    KindError::Failed(what)
}

/// A content key (the path below Content, no export: `Panda_Main/VFX/PS_X`, `Engine/Content/BasicShapes/Sphere`) as
/// the build object path of its export named as the package (`bvKeyPath`).
pub fn bv_key_path(key: &str) -> String {
    let name = &key[key.rfind('/').map_or(0, |i| i + 1)..];
    match key.strip_prefix("Engine/Content/") {
        Some(rest) => format!("/Engine/{rest}.{name}"),
        None => format!("/Game/{key}.{name}"),
    }
}

/// A component material's key (`<key>#local`, `<key>#spline`) as its DrawRef (`componentRef`).
fn component_ref(key: &str) -> Result<String, KindError> {
    let (k, tag) = key.split_once('#').unwrap_or((key, ""));
    if tag != "local" && tag != "spline" {
        return Err(failed(format!("particles: component material {key}: not #local or #spline")));
    }
    draw_ref(&bv_key_path(k), context(tag).expect("a draw context"))
}

/// An asset and its properties: its name and the object kind's JSON of its properties.
type Offered = (String, IndexMap<String, Json>);

/// The properties of each asset of class `class` under `folder`, the export named as its package (EnabledForDebugging
/// ones left out: IsShippingBuild is true) (`offered`): the asset's name and its properties.
fn offered(cx: &Cx<'_>, folder: &str, class: &str, recursive: bool) -> Result<Vec<Offered>, KindError> {
    let mut out = Vec::new();
    for pkg in cx.catalog.list_packages(folder, recursive, Some(class)) {
        let name = pkg[pkg.rfind('/').map_or(0, |i| i + 1)..].to_owned();
        let o = cx.object(&format!("{pkg}.{name}"))?;
        let props = props_json(cx.cache, o.props());
        if props.get("EnabledForDebugging") == Some(&Json::Bool(true)) {
            continue;
        }
        out.push((name, props));
    }
    Ok(out)
}

/// The kind's own systems: [`EFFECT_SYSTEMS`] with the stickers' and the ring-outs', and the stickers' EmoteData
/// owners by system.
pub struct OwnSystems {
    pub paths: Vec<String>,
    pub stickers: IndexMap<String, Vec<String>>,
}

/// The systems the kind binds (`particleSystems`), as build object paths, once each.
pub fn particle_systems(cx: &Cx<'_>) -> Result<OwnSystems, KindError> {
    let mut paths: IndexSet<String> = IndexSet::new();
    for (folder, names) in EFFECT_SYSTEMS {
        for n in *names {
            paths.insert(bv_key_path(&format!("{folder}/{n}")));
        }
    }
    // (a sticker's system: its owner is the EmoteData asset)
    let mut stickers: IndexMap<String, Vec<String>> = IndexMap::new();
    for (name, props) in offered(cx, "/Game/Panda_Main", EMOTE_DATA, true)? {
        let Some(Json::Str(ps)) = props.get("ParticleEffect") else { continue };
        stickers.entry(ps.clone()).or_default().push(name);
    }
    let mut sticker_systems: Vec<&String> = stickers.keys().collect();
    sticker_systems.sort();
    paths.extend(sticker_systems.into_iter().cloned());
    for k in RINGOUT_SYSTEMS {
        paths.insert(bv_key_path(k));
    }
    // (two systems of one name would be one index key)
    let mut by_name: IndexMap<&str, &str> = IndexMap::new();
    for p in &paths {
        if let Some(had) = by_name.insert(system_name(p), p)
            && had != p
        {
            return Err(failed(format!("particles: two systems named {}: {had}, {p}", system_name(p))));
        }
    }
    Ok(OwnSystems { paths: paths.into_iter().collect(), stickers })
}

/// The systems the kind builds: its own, the namers' it does not hold, and which of them are a banner's.
pub struct Listed {
    pub own: OwnSystems,
    /// The namers' systems the index does not hold, in the order met.
    pub named: Vec<String>,
    /// The banner kind's systems, which no other names.
    pub banner: IndexSet<String>,
}

/// The run's one list.
struct ListedSlot(Result<Arc<Listed>, KindError>);

/// The menuscene entry's header members the kind reads.
#[derive(Deserialize)]
struct MenuSceneNamed {
    systems: Vec<String>,
}

fn menuscene_systems(bytes: &[u8]) -> Result<Vec<String>, KindError> {
    let jb = mvs_material::jsonbin::JsonBin::split(MENUSCENE_MAGIC, bytes, "menuscene")
        .map_err(|e| failed(e.to_string()))?;
    Ok(jb.parse::<MenuSceneNamed>("menuscene").map_err(|e| failed(e.to_string()))?.systems)
}

/// The systems the namers' entries name (`namedSystems`: `GameKind.particleSystems` over every entry of the namer's
/// kind the store holds, in path order), each once, in the order met, with the kinds that name it.
fn named_systems(cx: &Cx<'_>) -> Result<IndexMap<String, IndexSet<&'static str>>, KindError> {
    type Reader = fn(&[u8]) -> Result<Vec<String>, KindError>;
    let namers: [(&'static str, Reader); 3] = [
        (SCENERY.0, scenery::particle_systems),
        (BANNER.0, banner::particle_systems),
        (MENUSCENE.0, menuscene_systems),
    ];
    let mut out: IndexMap<String, IndexSet<&'static str>> = IndexMap::new();
    for (kind, reader) in namers {
        for path in cx.store.paths(kind)? {
            let systems =
                reader(&cx.store.entry(kind, &path)?).map_err(|e| failed(format!("particles: {kind} {path}: {e}")))?;
            for p in systems {
                out.entry(p).or_default().insert(kind);
            }
        }
    }
    Ok(out)
}

/// The kind's list, made once a run (`theList`).
pub fn listed(cx: &Cx<'_>) -> Result<Arc<Listed>, KindError> {
    cx.shared.get_or_init(|| ListedSlot(make_listed(cx).map(Arc::new))).0.clone()
}

fn make_listed(cx: &Cx<'_>) -> Result<Listed, KindError> {
    let own = particle_systems(cx)?;
    let have: IndexSet<&String> = own.paths.iter().collect();
    let named = named_systems(cx)?;
    let mut banner = IndexSet::new();
    for (p, by) in &named {
        if !by.contains(BANNER.0) {
            continue;
        }
        if by.len() > 1 || have.contains(p) {
            let other = if have.contains(p) {
                "the index".to_owned()
            } else {
                by.iter().copied().collect::<Vec<_>>().join(", ")
            };
            return Err(failed(format!(
                "particles-import: {p} is a banner's system and {other}'s: one entry cannot hold both sprite contexts"
            )));
        }
        banner.insert(p.clone());
    }
    let named = named.into_keys().filter(|p| !have.contains(p)).collect();
    Ok(Listed { own, named, banner })
}

/// Every string value of a JSON document, not a key (each text between two quotes of the fighter data's
/// JSON that no colon follows).
fn collect_values(v: &serde_json::Value, out: &mut IndexSet<String>) {
    match v {
        serde_json::Value::String(s) => {
            out.insert(s.clone());
        }
        serde_json::Value::Array(a) => a.iter().for_each(|x| collect_values(x, out)),
        serde_json::Value::Object(o) => o.values().for_each(|x| collect_values(x, out)),
        _ => {}
    }
}

fn values_of<T: Serialize>(data: &T, without: Option<&str>) -> Result<IndexSet<String>, KindError> {
    let mut v = serde_json::to_value(data).map_err(|e| failed(format!("particles: fighter data: {e}")))?;
    if let (Some(k), Some(o)) = (without, v.as_object_mut()) {
        o.remove(k);
    }
    let mut out = IndexSet::new();
    collect_values(&v, &mut out);
    Ok(out)
}

fn lookup<'t>(table: &'t [(&str, &[&str])], key: &str) -> &'t [&'t str] {
    table.iter().find(|(k, _)| *k == key).map_or(&[], |(_, v)| v)
}

fn strings(l: &[&str]) -> Vec<String> {
    l.iter().map(|s| (*s).to_owned()).collect()
}

/// `[...new Set(l)].sort()`.
fn sorted(l: impl IntoIterator<Item = String>) -> Vec<String> {
    let mut v: Vec<String> = l.into_iter().collect::<IndexSet<_>>().into_iter().collect();
    v.sort();
    v
}

/// A mesh's owners in the index.
#[derive(Serialize)]
struct MeshOwners {
    path: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    section: Option<f64>,
    owners: Vec<String>,
}

#[derive(Serialize)]
struct SystemOwners {
    path: String,
    owners: Vec<String>,
}

#[derive(Serialize)]
struct DecalOwners {
    #[serde(rename = "ref")]
    ref_: String,
    owners: Vec<String>,
}

/// The index entry (`ParticlesIndex`).
#[derive(Serialize)]
pub struct Index {
    kind: &'static str,
    version: u32,
    systems: IndexMap<String, SystemOwners>,
    materials: IndexMap<String, Vec<String>>,
    meshes: IndexMap<String, MeshOwners>,
    decals: IndexMap<String, DecalOwners>,
    components: IndexMap<String, String>,
}

/// `table[key] = [...new Set([...(table[key] ?? []), ...list])]`.
fn own(table: &mut IndexMap<String, Vec<String>>, key: &str, list: &[String]) {
    let mut set: IndexSet<String> = table.get(key).into_iter().flatten().cloned().collect();
    set.extend(list.iter().cloned());
    table.insert(key.to_owned(), set.into_iter().collect());
}

/// An emitter's Required module's Material and its mesh, what the index reads of a system entry.
fn indexed(s: &SystemValue) -> Result<Vec<(String, Option<String>)>, KindError> {
    let emitters = s.system.get("emitters").and_then(Json::as_array).unwrap_or_default();
    emitters
        .iter()
        .map(|e| {
            let material = e
                .get("modules")
                .and_then(Json::as_array)
                .and_then(|m| m.iter().find(|m| m.get("type").and_then(Json::as_str) == Some("Required")))
                .and_then(|r| r.get("Material"))
                .and_then(Json::as_str)
                .ok_or_else(|| failed(format!("particles: {}: an emitter without a Required material", s.path)))?;
            Ok((material.to_owned(), e.get("mesh").and_then(Json::as_str).map(str::to_owned)))
        })
        .collect()
}

/// The index entry over the system entries the kind wrote (`bindIndex`): the owners of each system (`base`; a
/// screen's; its stickers' and ring-outs' assets, folder, data, spawn and perk owners; a [`PERK_ONLY`] system's perks;
/// else its folder's fighter, the fighters whose data names it as a value, [`SPAWNED_FOR`] and [`PERK_FOR`]), a
/// material or mesh the owners of the systems that draw it, a component material, component mesh or decal its folder's
/// fighter and [`COMPONENT_FOR`]'s; a list with `base` in it is `["base"]`, any other sorted. A system with no owner is
/// an error.
pub fn bind_index(
    cx: &Cx<'_>,
    systems: &[Arc<SystemValue>],
    stickers: &IndexMap<String, Vec<String>>,
) -> Result<Index, KindError> {
    let mut perks: IndexSet<String> = IndexSet::new();
    for p in cx.catalog.list_packages(PERK_DATA_FOLDER, true, Some(PERK_DATA_CLASS)) {
        let o = cx.object(&p)?;
        if !matches!(o.prop("EnabledForDebugging"), Some(Value::Bool(true))) {
            perks.insert(short_name(o.path()).to_owned());
        }
    }
    for (ps, list) in PERK_FOR {
        for id in *list {
            if !perks.contains(*id) {
                return Err(failed(format!("particles: PERK_FOR {ps}: {id} is not a PerkData of the build")));
            }
        }
    }
    let keys: Vec<&'static str> = FIGHTER_PORT.iter().map(|(k, _)| *k).collect();
    let companions: IndexSet<&str> =
        FIGHTER_PORT.iter().filter(|(_, p)| p.companion_of.is_some()).map(|(k, _)| *k).collect();
    let folders: IndexMap<&str, String> = FIGHTER_PORT.iter().map(|(k, p)| (*k, format!("{}/", p.folder))).collect();
    // (each RingOutVfxData's ParticleEffectData systems, by system name)
    let mut ring_outs: IndexMap<String, IndexSet<String>> = IndexMap::new();
    for (name, props) in offered(cx, RINGOUT_DIR, RINGOUT_DATA, false)? {
        for e in props.get("ParticleEffectData").and_then(Json::as_array).unwrap_or_default() {
            let Some(Json::Str(effect)) = e.get("ParticleEffect") else { continue };
            ring_outs.entry(system_name(effect).to_owned()).or_default().insert(name.clone());
        }
    }
    let sticker_owners = |name: &str| -> Vec<String> {
        stickers.iter().filter(|(p, _)| system_name(p) == name).flat_map(|(_, v)| v.clone()).collect()
    };
    // (the strings of the fighters' data without its tauntTable, which names every sticker's system for every fighter:
    // each value, not a key)
    let all = chars(cx)?;
    let mut values: IndexMap<&str, IndexSet<String>> = IndexMap::new();
    for k in &keys {
        values.insert(k, values_of(&all.get(k)?.fighter, Some("tauntTable"))?);
    }
    // (the same for the items part's data, owner 'items': what a match with random items can show)
    let item_values = values_of(&all.items, None)?;
    let folder_owners = |p: &str| -> Vec<String> {
        if NOT_FOLDER.contains(&system_name(p)) {
            return Vec::new();
        }
        keys.iter()
            .filter(|f| !companions.contains(*f) && p.starts_with(folders[*f].as_str()))
            .map(|f| (*f).to_owned())
            .collect()
    };
    let system_owners = |p: &str| -> Result<Vec<String>, KindError> {
        let name = system_name(p);
        if BASE_SYSTEMS.contains(&name) {
            return Ok(vec!["base".into()]);
        }
        let screen = lookup(SCREEN_SYSTEMS, name).first();
        let items: Vec<String> = if item_values.contains(name) { vec!["items".into()] } else { Vec::new() };
        let spawned = strings(lookup(SPAWNED_FOR, name));
        let stickers_of = sticker_owners(name);
        if let Some(screen) = screen
            && stickers_of.is_empty()
        {
            return Ok(sorted([(*screen).to_owned()].into_iter().chain(items).chain(spawned)));
        }
        // the name as a value, not as a key: a component may be called after a system and hold another one
        let named: Vec<String> =
            keys.iter().filter(|f| values[**f].contains(name)).map(|f| (*f).to_owned()).chain(items).collect();
        let perk_for = strings(lookup(PERK_FOR, name));
        let assets: Vec<String> =
            stickers_of.into_iter().chain(ring_outs.get(name).into_iter().flatten().cloned()).collect();
        if !assets.is_empty() {
            return Ok(sorted(assets.into_iter().chain(folder_owners(p)).chain(named).chain(spawned).chain(perk_for)));
        }
        let owners = if PERK_ONLY.contains(&name) {
            sorted(perk_for)
        } else {
            sorted(folder_owners(p).into_iter().chain(named).chain(spawned).chain(perk_for))
        };
        if owners.is_empty() {
            return Err(failed(format!(
                "particles: {p}: no owner (BASE_SYSTEMS, SCREEN_SYSTEMS, a fighter's or the items part's data, SPAWNED_FOR or PERK_FOR)"
            )));
        }
        Ok(owners)
    };
    let component_owners = |key: &str| -> Result<Vec<String>, KindError> {
        let owners: Vec<String> = folder_owners(&bv_key_path(key.split('#').next().unwrap_or(key)))
            .into_iter()
            .chain(strings(lookup(COMPONENT_FOR, key)))
            .collect();
        if owners.is_empty() {
            return Err(failed(format!("particles: {key}: no owner (not in a fighter's folder or COMPONENT_FOR)")));
        }
        Ok(owners)
    };
    let mut sys: IndexMap<String, Vec<String>> = IndexMap::new();
    let mut paths: IndexMap<String, String> = IndexMap::new();
    let mut mats: IndexMap<String, Vec<String>> = IndexMap::new();
    let mut meshes: IndexMap<String, MeshOwners> = IndexMap::new();
    let mut decals: IndexMap<String, DecalOwners> = IndexMap::new();
    let mut components: IndexMap<String, String> = IndexMap::new();
    let own_mesh =
        |meshes: &mut IndexMap<String, MeshOwners>, key: &str, path: &str, section: Option<f64>, list: &[String]| {
            let m = meshes.entry(key.to_owned()).or_insert_with(|| MeshOwners {
                path: path.to_owned(),
                section,
                owners: Vec::new(),
            });
            let set: IndexSet<String> = m.owners.iter().chain(list).cloned().collect();
            m.owners = set.into_iter().collect();
        };
    for s in systems {
        let name = system_name(&s.path);
        paths.insert(name.to_owned(), s.path.clone());
        own(&mut sys, name, &system_owners(&s.path)?);
        let owners = sys[name].clone();
        for (material, mesh) in indexed(s)? {
            own(&mut mats, &material, &owners);
            if let Some(mesh) = mesh {
                let (p, section) = match mesh.split_once('#') {
                    Some((p, n)) => (
                        p,
                        Some(
                            n.parse::<f64>()
                                .map_err(|_| failed(format!("particles: mesh {mesh}: no section number")))?,
                        ),
                    ),
                    None => (mesh.as_str(), None),
                };
                own_mesh(&mut meshes, &mesh, p, section, &owners);
            }
        }
    }
    for k in COMPONENT_MATERIALS {
        let r = component_ref(k)?;
        components.insert((*k).to_owned(), r.clone());
        own(&mut mats, &r, &component_owners(k)?);
    }
    for k in COMPONENT_MESHES {
        own_mesh(&mut meshes, k, &bv_key_path(k), None, &component_owners(k)?);
    }
    for k in DECALS {
        let name = &k[k.rfind('/').map_or(0, |i| i + 1)..];
        let mut owners: IndexSet<String> = decals.get(name).into_iter().flat_map(|d| d.owners.clone()).collect();
        owners.extend(component_owners(k)?);
        let r = draw_ref(&bv_key_path(k), context("decal").expect("a draw context"))?;
        decals.insert(name.to_owned(), DecalOwners { ref_: r, owners: owners.into_iter().collect() });
    }
    let norm = |mut l: Vec<String>| -> Vec<String> {
        if l.iter().any(|o| o == "base") {
            vec!["base".into()]
        } else {
            l.sort();
            l
        }
    };
    Ok(Index {
        kind: "index",
        version: PARTICLES_FORMAT_VERSION,
        systems: sys
            .into_iter()
            .map(|(n, l)| {
                let path = paths[&n].clone();
                (n, SystemOwners { path, owners: norm(l) })
            })
            .collect(),
        materials: mats.into_iter().map(|(k, l)| (k, norm(l))).collect(),
        meshes: meshes.into_iter().map(|(k, m)| (k, MeshOwners { owners: norm(m.owners), ..m })).collect(),
        decals: decals.into_iter().map(|(k, d)| (k, DecalOwners { owners: norm(d.owners), ..d })).collect(),
        components,
    })
}

/// The index entry's system list for the build: each of the kind's own systems, bound (the entry of a system that
/// did not build fails the index).
pub fn index_systems(cx: &Cx<'_>, own: &OwnSystems) -> Result<Vec<Arc<SystemValue>>, KindError> {
    own.paths
        .iter()
        .map(|p| {
            super::system::system(cx, p)
                .map_err(|e| failed(format!("particles-import: the index's system {p} did not build: {e}")))
        })
        .collect()
}
