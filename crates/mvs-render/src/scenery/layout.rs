//! A custom stage's layout (`mvs-stage`) applied to its base stage's scenery entry, before the statics are made:
//! the hidden instances, skeletal meshes and emitters gone, the blocks not kept stripped, the pieces moved or
//! added (a borrowed piece with its source stage's mesh and materials), and the mesh bounds the layout's boxes tied
//! to a piece are made from.

use mvs_material::jsonbin::JsonBin;
use mvs_stage::{Layout, LayoutError, Light, MeshBounds, Piece};
use serde::Deserialize;
use serde_json::Value;

use super::blocks::GENERIC_KEYS;
use super::{Instance, Scenery, SceneryHeader};
use crate::Result;
use crate::stage::bad;
use crate::stagedraw::StageDraw;

/// The scenery entry of the stage `key`, parsed.
fn read_scenery(store: &mvs_store::Store, key: &str) -> Result<Scenery> {
    let entry = StageDraw::load(store, key)?.scenery.entry;
    let bytes = store.kind("scenery")?.bytes(&entry)?;
    Ok(JsonBin::split(b"MVSC", &bytes, &entry)?.parse::<SceneryHeader>(&entry)?.scenery)
}

/// [`apply`] with the borrowed pieces' stages read from `store`.
pub(super) fn apply_from_store(s: &mut Scenery, layout: &Layout, store: &mvs_store::Store) -> Result<()> {
    let mut read = |key: &str| read_scenery(store, key).map_err(|e| LayoutError::Invalid(e.to_string()));
    apply(s, layout, &mut read).map_err(|e| bad(&format!("layout {}", layout.key), e.to_string()))
}

/// The layout over the base's scenery `s`: hides, blocks and pieces. A piece comes from the instance of its stage's
/// entry as the file has it (a hidden base instance may be copied); `source` reads another stage's entry.
pub(super) fn apply(
    s: &mut Scenery,
    layout: &Layout,
    source: &mut dyn FnMut(&str) -> std::result::Result<Scenery, LayoutError>,
) -> std::result::Result<(), LayoutError> {
    let invalid = |why: String| Err(LayoutError::Invalid(why));
    mvs_stage::check(layout)?;
    for h in &layout.hide {
        let known = s.instances.iter().any(|i| &i.name == h)
            || s.skeletal.iter().any(|k| &k.name == h)
            || s.emitters.iter().any(|e| &e.name == h);
        if !known {
            return invalid(format!("hide {h}: no instance, skeletal mesh or emitter of that name"));
        }
    }
    for b in &layout.blocks {
        if GENERIC_KEYS.contains(&b.as_str()) || !s.rest.contains_key(b) {
            return invalid(format!("block {b}: the base's scenery has no such block"));
        }
    }
    s.rest.retain(|k, _| GENERIC_KEYS.contains(&k.as_str()) || layout.blocks.contains(k));
    // A kept block finds its instances by name (or by the actor's name): none of them hidden.
    for (key, data) in s.rest.iter().filter(|(k, _)| !GENERIC_KEYS.contains(&k.as_str())) {
        let mut named = None;
        strings(data, &mut |t| {
            if named.is_none() {
                named = layout.hide.iter().find(|h| *h == t || h.split('.').next() == Some(t)).cloned();
            }
        });
        if let Some(instance) = named {
            return Err(LayoutError::HiddenInBlock { instance, block: key.clone() });
        }
    }
    let hidden = |n: &str| layout.hide.iter().any(|h| h == n);
    for i in s.instances.iter().filter(|i| !hidden(&i.name)) {
        if let Some(parent) = i.attach.as_ref().and_then(|a| a.get(0)).and_then(Value::as_str)
            && hidden(parent)
        {
            return invalid(format!("hide {parent}: the instance {} is attached to it", i.name));
        }
    }
    // (the base's instances as the file has them: the pieces' sources)
    let original = s.instances.clone();
    let receivers: Vec<u32> = std::mem::take(&mut s.decal_receivers);
    let mut kept = Vec::with_capacity(s.instances.len());
    for (k, i) in std::mem::take(&mut s.instances).into_iter().enumerate() {
        if hidden(&i.name) {
            continue;
        }
        if receivers.contains(&(k as u32)) {
            s.decal_receivers.push(kept.len() as u32);
        }
        kept.push(i);
    }
    s.instances = kept;
    s.skeletal.retain(|k| !hidden(&k.name));
    s.emitters.retain(|e| !hidden(&e.name));

    let mut borrowed: Vec<(String, Scenery)> = Vec::new();
    for p in &layout.pieces {
        let stage = layout.piece_stage(p);
        let name = layout.piece_name(p);
        if layout.moved_in_place(p) {
            let Some(i) = s.instances.iter_mut().find(|i| i.name == p.instance) else {
                return invalid(format!("the piece {name} moves {}, which the scenery has not (or hides)", p.instance));
            };
            place(i, p);
            continue;
        }
        let src = if stage == layout.base {
            original.iter().find(|i| i.name == p.instance).cloned()
        } else {
            if !borrowed.iter().any(|(k, _)| k == stage) {
                let sc = source(stage)?;
                borrowed.push((stage.to_owned(), sc));
            }
            let sc = &borrowed.iter().find(|(k, _)| k == stage).expect("read above").1;
            let found = sc.instances.iter().find(|i| i.name == p.instance).cloned();
            if let Some(i) = &found {
                // (the source's mesh key, renamed where the base has the key for another mesh)
                let path = &sc.meshes.get(&i.mesh).ok_or_else(|| missing_mesh(stage, &i.mesh))?.path;
                let key = match s.meshes.get(&i.mesh) {
                    Some(m) if &m.path != path => format!("{stage}:{}", i.mesh),
                    _ => i.mesh.clone(),
                };
                s.meshes.entry(key.clone()).or_insert_with(|| super::SceneryMesh { path: path.clone() });
                let mut i = i.clone();
                i.mesh = key;
                Some(i)
            } else {
                None
            }
        };
        let Some(mut i) = src else {
            return invalid(format!("the piece {name}: no instance {} in the stage {stage}", p.instance));
        };
        if i.attach.is_some() {
            return invalid(format!("the piece {name}: {} is attached to a skeletal mesh", p.instance));
        }
        let receiver = stage == layout.base
            && original.iter().position(|o| o.name == p.instance).is_some_and(|k| receivers.contains(&(k as u32)));
        i.name = name;
        i.hidden = false;
        i.platform = p.platform.as_deref().and_then(|n| layout.platform_index(n));
        // (a copy is no stage part's: only the component's shadow flag of its own fields)
        i.fields.retain(|k, _| k == "castShadow");
        place(&mut i, p);
        if receiver {
            s.decal_receivers.push(s.instances.len() as u32);
        }
        s.instances.push(i);
    }
    if !receivers.is_empty() && s.decal_receivers.is_empty() {
        // (an empty list with decals would make every instance a receiver: scenery's rule)
        return invalid("every decal receiver hidden: hide the decals' instances' stage parts too".to_owned());
    }
    Ok(())
}

fn missing_mesh(stage: &str, mesh: &str) -> LayoutError {
    LayoutError::Invalid(format!("the stage {stage}'s scenery has no mesh {mesh}"))
}

/// A piece's place, light and floating flag on its instance.
fn place(i: &mut Instance, p: &Piece) {
    i.loc = p.loc;
    i.rot = p.rot;
    i.scale = p.scale;
    if p.light == Light::Movable {
        // (lit by the base's lights and volumetric lightmap: no lightmap of its own)
        for m in i.materials.iter_mut().flatten() {
            if let Some((path, ctx)) = m.split_once("?ctx=")
                && (ctx == "static" || ctx == "shadowmap")
            {
                *m = format!("{path}?ctx=locallm");
            }
        }
        i.lm = None;
    }
    if p.floating {
        i.fields.insert("floating".to_owned(), Value::Bool(true));
    } else {
        i.fields.remove("floating");
    }
}

fn strings(v: &Value, f: &mut impl FnMut(&str)) {
    match v {
        Value::String(s) => f(s),
        Value::Array(a) => a.iter().for_each(|x| strings(x, f)),
        Value::Object(o) => o.values().for_each(|x| strings(x, f)),
        _ => {}
    }
}

#[derive(Deserialize)]
struct BoundsHeader {
    bounds: BoundsRow,
}

#[derive(Deserialize)]
struct BoundsRow {
    origin: [f64; 3],
    extent: [f64; 3],
}

/// The pieces' mesh bounds (the mesh kind's `bounds`), by the piece's composed name: what the layout's boxes tied to
/// a piece are made from, read with no renderer (the match's data composes with them too).
#[derive(Clone, Debug, Default)]
pub struct PieceBounds {
    by_piece: Vec<(String, MeshBounds)>,
}

impl PieceBounds {
    /// Every piece's mesh bounds, from its stage's scenery entry and the mesh kind of `store`.
    pub fn read(store: &mvs_store::Store, layout: &Layout) -> Result<PieceBounds> {
        let mut entries: Vec<(String, Scenery)> = Vec::new();
        let mut by_piece = Vec::new();
        for p in &layout.pieces {
            let stage = layout.piece_stage(p);
            if !entries.iter().any(|(k, _)| k == stage) {
                entries.push((stage.to_owned(), read_scenery(store, stage)?));
            }
            let sc = &entries.iter().find(|(k, _)| k == stage).expect("read above").1;
            let what = format!("layout {}: piece {}", layout.key, p.instance);
            let i = sc.instances.iter().find(|i| i.name == p.instance).ok_or_else(|| bad(&what, "no such instance"))?;
            let path = &sc.meshes.get(&i.mesh).ok_or_else(|| bad(&what, format!("no mesh {}", i.mesh)))?.path;
            let bytes = store.kind("mesh")?.bytes(path)?;
            let h: BoundsHeader = JsonBin::split(b"MVM1", &bytes, path)?.parse(path)?;
            by_piece.push((layout.piece_name(p), MeshBounds { origin: h.bounds.origin, extent: h.bounds.extent }));
        }
        Ok(PieceBounds { by_piece })
    }

    /// A piece's bounds, for `mvs_stage::compose_stage`.
    pub fn get(&self, layout: &Layout, p: &Piece) -> Option<MeshBounds> {
        let name = layout.piece_name(p);
        self.by_piece.iter().find(|(n, _)| *n == name).map(|x| x.1)
    }
}

/// Whether a material draws as a movable static mesh in a lit level: its shader map holds the local vertex factory's
/// base pass of the volumetric lightmap policy (with the sky light for a lit material), the pass a `locallm` slot
/// takes. A piece made movable whose material lacks it does not draw.
pub fn has_movable_base_pass(m: &mvs_material::Material) -> bool {
    let p = crate::material::LightMapPolicy::PrecomputedIrradianceVolumeIndirectLighting.type_suffix();
    let sky = if m.state.shading_model == "MSM_Unlit" { "" } else { "Skylight" };
    let (vs, ps) = (format!("TBasePassVS{p}"), format!("TBasePassPS{p}{sky}"));
    let pipeline = format!("BasePassPipeline{p}{sky}");
    let vf = crate::material::VertexFactory::Local.type_name();
    m.shaders.pass(Some(vf), Some(&pipeline), &[(&vs, 0), (&ps, 0)]).is_some()
}

/// The base's stage data with the layout composed over it (`mvs_stage::compose_stage`), its spawn areas dropped:
/// what the renderer and the particle surfaces read.
pub fn composed_stage_data(
    store: &mvs_store::Store,
    base: &mvs_data::StageData,
    layout: &Layout,
) -> Result<mvs_data::StageData> {
    let b = PieceBounds::read(store, layout)?;
    let c = mvs_stage::compose_stage(base, &[], layout, &|p| b.get(layout, p))
        .map_err(|e| bad(&format!("layout {}", layout.key), e.to_string()))?;
    Ok(c.data)
}

#[cfg(test)]
mod tests;
