//! A stage's entry: its data bound from its levels, its manifest entry and the spawn areas of its map.
//!
//! Four parts, each writing its keys into the data: collision (floor, platforms, edges, solids, placed actors), sim
//! (spawns, waypoints, camera, blast boxes, the placed actors' buffs and spawned actors), render (meshes, materials,
//! lights, fog, atmosphere) and audio. They run in that order, the sim part reading the collision part's placed actors.

pub mod audio;
pub mod collision;
pub mod context;
pub mod manifest;
pub mod reach;
pub mod render;
pub mod sim;

use std::sync::Arc;

use indexmap::IndexMap;
use mvs_data::stage::StageData;
use mvs_data::value::{Value as Tree, decode_entry, encode, from_value};
use mvs_data::{StageEntry, StageManifest};

use self::context::StageCtx;
pub use self::manifest::stage_manifest;
use super::shape::fail;
use crate::kind::{Cx, KindError};

/// The stage `key`'s data, bound from its manifest's levels (loadStageData).
fn stage_data(cx: &Cx<'_>, key: &str, mf: &StageManifest) -> Result<StageData, KindError> {
    let ctx = StageCtx::new(cx, key, &mf.levels)?;
    let part = |name: &str, e: KindError| KindError::Failed(format!("stage data {key}, part {name}: {e}"));
    let col = collision::bind(&ctx).map_err(|e| part("collision", e))?;
    let sim = sim::bind(&ctx, &col.placed).map_err(|e| part("sim", e))?;
    let rd = render::bind(&ctx).map_err(|e| part("render", e))?;
    let audio = audio::bind(&ctx).map_err(|e| part("audio", e))?;
    let (Some(post_process), Some(sky_light)) = (rd.post_process, rd.sky_light) else {
        return fail(format!("stage data {key}: no PostProcessVolume or no SkyLight"));
    };
    Ok(StageData {
        name: sim.name,
        units: sim.units,
        meshes: rd.meshes,
        materials: rd.materials,
        spawns: sim.spawns,
        respawns: sim.respawns,
        waypoints: sim.waypoints,
        bot_zones: sim.bot_zones,
        lights: rd.lights,
        radial_lights: rd.radial_lights,
        tlv_shadow: rd.tlv_shadow,
        floor: col.floor,
        platforms: non_empty(col.platforms),
        edge_box: col.edge_box,
        edges: col.edges,
        center: sim.center,
        post_process,
        bloom_intensity: rd.bloom_intensity,
        audio,
        fog: rd.fog,
        atmosphere: rd.atmosphere,
        sky_light,
        blast_boxes: sim.blast_boxes,
        camera: sim.camera,
        camera_center: sim.camera_center,
        blast_box_corners: sim.blast_box_corners,
        blast_shrink: Some(sim.blast_shrink),
        camera_shrunk_angle: sim.camera_shrunk_angle,
        solids: non_empty(col.solids),
        query_bodies: non_empty(col.query_bodies),
        complex_segments: non_empty(col.complex_segments),
        placed: non_empty(col.placed),
        actor_montages: None,
        buffs: sim.buffs,
        actors: sim.actors,
        tagged: sim.tagged,
    })
}

/// A value tree with every `-0` a `0` (`JSON.parse(JSON.stringify(data))`, which layOutStage ends with, reads a `-0` back
/// as `0`).
fn without_negative_zero(v: &mut Tree) {
    match v {
        Tree::Num(n) if *n == 0.0 => *n = 0.0,
        Tree::Arr(a) => a.iter_mut().for_each(without_negative_zero),
        Tree::Obj(o) => o.values_mut().for_each(without_negative_zero),
        _ => {}
    }
}

/// The record as the stored data reads back (layOutStage: `JSON.parse(JSON.stringify(data))`): no `-0`.
fn as_stored(e: &StageEntry) -> Result<StageEntry, KindError> {
    let failed =
        |what: &str, e: &dyn std::fmt::Display| KindError::Failed(format!("stage entry {}: {what}: {e}", "layout"));
    let bytes = encode(e).map_err(|x| failed("encode", &x))?;
    let mut tree = decode_entry(&bytes).map_err(|x| failed("decode", &x))?;
    without_negative_zero(&mut tree);
    from_value(tree).map_err(|x| failed("record", &x))
}

/// The stage `key`'s entry: its data, its manifest entry and the spawn areas the map's
/// spawn area actors read, those of the manifest entry of the stage whose gameplay level is the data's `name`
/// (stageOfLevel: the stage table by the gameplay level's package name, a later stage of the same name over an
/// earlier).
pub fn stage_entry(cx: &Cx<'_>, key: &str) -> Result<StageEntry, KindError> {
    let all: Arc<IndexMap<String, StageManifest>> = stage_manifest(cx)?;
    let Some(mf) = all.get(key) else { return fail(format!("stage entry: no stage {key}")) };
    let data = stage_data(cx, key, mf)?;
    let at = super::STAGES.iter().rev().find(|k| {
        all.get(**k)
            .and_then(|m| m.levels.iter().find(|l| l.1 == "gameplay"))
            .is_some_and(|l| l.0.rsplit('/').next() == Some(&data.name))
    });
    let spawn_areas = at.and_then(|k| all.get(*k)).map(|m| m.spawn_areas.clone()).unwrap_or_default();
    as_stored(&StageEntry { key: key.to_owned(), data, manifest: mf.clone(), spawn_areas })
}

/// A key the data leaves out where it has no entry.
fn non_empty<T>(v: Vec<T>) -> Option<Vec<T>> {
    (!v.is_empty()).then_some(v)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_negative_zero_reads_back_as_zero() {
        let mut v = Tree::Arr(vec![Tree::Num(-0.0), Tree::Num(1.5), Tree::Arr(vec![Tree::Num(-0.0)])]);
        without_negative_zero(&mut v);
        let Tree::Arr(a) = &v else { panic!("array") };
        assert!(a[0].as_f64().is_some_and(|x| x.to_bits() == 0));
        assert_eq!(a[1].as_f64(), Some(1.5));
        let Tree::Arr(b) = &a[2] else { panic!("array") };
        assert!(b[0].as_f64().is_some_and(|x| x.to_bits() == 0));
    }

    #[test]
    fn an_empty_list_is_left_out() {
        let n = non_empty(Vec::<u8>::new());
        assert!(n.is_none());
        assert_eq!(non_empty(vec![1]), Some(vec![1]));
    }
}
