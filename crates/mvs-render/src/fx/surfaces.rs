//! What the effects' Collision modules meet (handed to the particle
//! player when the scenery begins): the stage's horizontal surfaces, each a plane Z and its world XY
//! rectangle, UE units.

use mvs_data::stage::{SolidKind, StageData};
use mvs_sim::consts::SOLID_FLOOR;
use mvs_vfx::Surface;

use crate::Result;
use crate::stage::{stage_data, stage_key};

/// The top of every walking box with its span: World.ground's box (StageData.floor) and each enabled box solid a
/// fighter stands on (SOLID_FLOOR: the other Floor boxes, Trophy 2's Floor2), then the horizontal floor stripes
/// (engine Plane, 100 x 100 cm x scale, yaw 0 or 90).
// Deviation: only these surfaces; a platform, a polygon solid or any other surface in the box gets nothing, and a solid
// is taken where the stage data has it (a moving one's move is not followed: no built stage has one).
pub fn stage_surfaces(stage: &StageData) -> Vec<Surface> {
    let rect = |c: &[f64], h: &[f64]| Some([c[0] - h[0], c[0] + h[0], c[1] - h[1], c[1] + h[1]]);
    let f = &stage.floor;
    let mut out = vec![Surface { z: f.top, rect: rect(&f.center, &f.half_extent) }];
    for s in stage.solids.iter().flatten() {
        if s.kind != SolidKind::Box || s.flags & SOLID_FLOOR == 0 || s.enabled == Some(false) {
            continue;
        }
        out.push(Surface { z: s.center[2] + s.half_extent[2], rect: rect(&s.center, &s.half_extent) });
    }
    for m in &stage.meshes {
        if m.shape != "Plane" || m.rot[0].abs() > 0.01 || m.rot[2].abs() > 0.01 {
            continue;
        }
        let turned = (m.rot[1] * std::f64::consts::PI / 180.0).sin().abs() > 0.5;
        let h = [50.0 * m.scale[usize::from(turned)], 50.0 * m.scale[usize::from(!turned)]];
        out.push(Surface { z: m.loc[2], rect: rect(&m.loc, &h) });
    }
    out
}

/// The surfaces of the stage `map` (MapData or map name), read without a renderer as `level_emitters` is.
pub fn level_surfaces(store: &mvs_store::Store, map: &str) -> Result<Vec<Surface>> {
    Ok(stage_surfaces(&stage_data(store, &stage_key(map))?))
}
