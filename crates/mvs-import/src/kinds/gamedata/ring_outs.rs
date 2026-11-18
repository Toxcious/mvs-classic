//! The ring-out effects by asset, from the RingOutVfxData assets: every RingOutVfxData
//! asset by asset name, and the RingOutVfxAnchor's MapsToIgnore. The set: the build takes every RingOutVfxData through
//! IAssetRegistry::GetAssetsByClass (UI_Functions LoadAllRingOutDatas @44); here the packages of their folder of that
//! class.

use mvs_data::tables::{RingOutAsset, RingOutEntry};
use mvs_data::{Props, RingOutTable, Value};

use super::shape::{arr, bool, defaults_or_properties, fail, num, package_name, properties, ref_name, vec3};
use crate::kind::{Cx, KindError};

pub const RING_OUT_FOLDER: &str = "/Game/Panda_Main/Blueprints/Rewards/RingOutVfx";
pub const RING_OUT_CLASS: &str = "/Script/MultiVersus.RingOutVfxData";
const ANCHOR: &str = "/Game/Panda_Main/Blueprints/RingOutVfxAnchor.Default__RingOutVfxAnchor_C";

/// The name of a reference member that may be absent ('' for none).
fn ref_of(p: &Props, name: &str) -> Result<String, KindError> {
    p.get(name).map_or(Ok(String::new()), ref_name)
}

fn entry(e: &Value, at: &str) -> Result<RingOutEntry, KindError> {
    let Some(e) = e.as_obj() else { return fail(format!("{at}: not a struct")) };
    Ok(RingOutEntry {
        ps: ref_of(e, "ParticleEffect")?,
        off: vec3(e, "LocationOffset", at)?.to_vec(),
        ceil: if bool(e, "bCeilingZOffsetEnabled", at)? { Some(num(e, "CeilingZOffset", at)?) } else { None },
        lerp: num(e, "LocationLerpAlpha", at)?,
        scale: vec3(e, "Scale", at)?.to_vec(),
        rot: vec3(e, "RotationOffset", at)?.to_vec(),
        rotate: bool(e, "Rotate", at)?,
        perp: num(e, "PerpendicularAlphaValue", at)?,
        flip_l: bool(e, "bLeftBoundaryFlip", at)?.then_some(1.0),
        flip_r: bool(e, "bRightBoundaryFlip", at)?.then_some(1.0),
        ally: ref_of(e, "AllyAudioEvent")?,
        enemy: ref_of(e, "EnemyAudioEvent")?,
    })
}

/// The ring-out table.
pub fn ring_outs(cx: &Cx<'_>) -> Result<RingOutTable, KindError> {
    let anchor = defaults_or_properties(&cx.object(ANCHOR)?)?;
    let maps = match anchor.get("MapsToIgnore") {
        Some(Value::Arr(l)) => l.iter().map(|m| m.as_str().map(str::to_owned)).collect::<Option<Vec<_>>>(),
        _ => None,
    };
    let Some(maps) = maps else { return fail(format!("{ANCHOR}: no MapsToIgnore")) };
    let mut assets = mvs_data::fighter::Map::new();
    for path in cx.catalog.list_packages(RING_OUT_FOLDER, false, None) {
        let o = cx.object(&path)?;
        if o.class() != RING_OUT_CLASS {
            continue;
        }
        let name = package_name(&path).to_owned();
        let p = properties(&o)?;
        let es = if p.contains_key("ParticleEffectData") { arr(&p, "ParticleEffectData", &name)? } else { &[] };
        let entries = es
            .iter()
            .enumerate()
            .map(|(i, e)| entry(e, &format!("{name} entry {i}")))
            .collect::<Result<Vec<_>, _>>()?;
        // (an asset with no entry: nothing plays, the anchor lives its 0.01 s floor)
        let hold = match es.first() {
            Some(e) => num(e.as_obj().unwrap_or(&Props::new()), "CameraHoldTime", &format!("{name} entry 0"))?,
            None => 0.0,
        };
        let random = (p.get("RandomPlayOrder") == Some(&Value::Bool(true))).then_some(1.0);
        assets.insert(name, RingOutAsset { hold, random, entries });
    }
    Ok(RingOutTable { maps, assets })
}
