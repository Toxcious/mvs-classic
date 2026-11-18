//! The blast box's drawn edge (BlastBoxPlayerExit_C's border meshes).

use std::sync::Arc;

use mvs_view::MeshPose;
use serde::Deserialize;

use super::{Scenery, SlotLight, material_ref, slot_light};
use crate::Result;
use crate::stage::{bad, to_world, transform};

/// The blast box's drawn edge in the scenery kind: the level's
/// BlastBoxPlayerExit_C's SM_LevelBorder01 and SM_LevelBorder02 (attached under it with no transform of its own),
/// AdjustJumpOutCollision's numbers (world half extent on Y / Z = factor x (box half extent - inset), at loc + (0, 0,
/// box origin Z)) and the first part's mesh half extent on Y / Z.
#[derive(Deserialize)]
pub(super) struct BlastBorderEntry {
    factor: [f64; 2],
    inset: [f64; 2],
    loc: [f64; 3],
    bounds: [f64; 2],
    parts: Vec<BlastBorderPart>,
}

/// A border mesh: its key in the scenery's `meshes`, its material's DrawRef (the dynamic instance's parent),
/// TranslucencySortPriority and the scalars SetSpaceBorderValues sets on its dynamic instance.
#[derive(Deserialize)]
struct BlastBorderPart {
    name: String,
    mesh: String,
    material: String,
    #[serde(default)]
    sort: f64,
    scalars: indexmap::IndexMap<String, f64>,
}

/// The blast box's drawn edge (BlastBoxPlayerExit_C, every game mode: the class
/// has no switch for it): SM_LevelBorder01, a frame whose opening is the box (BlastBoxBorders_01_MAT: translucent,
/// unlit, no depth test, TranslucencySortPriority 101; its white fringe reads the scene colour copy), and under it
/// SM_LevelBorder02, a sheet of stars (BlastBoxBorders_01_InnerDebris_MAT, depth tested, priority 0). Each draws with
/// its dynamic instance's scalars over the parent material. AdjustJumpOutCollision (ExecuteUbergraph @5235..6162;
/// BeginPlay and every update of the shrink timeline) sizes and places the first from CollisionComp's bounds
/// (`place`); the second has no transform of its own and takes the same.
// The stars sort by view Z among the stage's translucent meshes (the build's priority-then-distance order).
// The cooked vertex shader of the stars' material (BlastBoxBorders_01_InnerDebris_MAT, TBasePassVSFNoLightMapPolicy of
// FLocalVertexFactory) adds nothing to the world position: its WorldPositionOffset pin is not in the build's shader.
#[derive(Clone, Debug)]
pub struct BlastBorder {
    factor: [f64; 2],
    inset: [f64; 2],
    loc: [f64; 3],
    bounds: [f64; 2],
    /// Each part's pose and TranslucencySortPriority.
    pub parts: Vec<(MeshPose, f32)>,
    /// The box the parts are placed for (sim left, right, bottom, top); None until placed.
    placed: Option<[f64; 4]>,
}

impl BlastBorder {
    /// The parts placed for the blast box `b` (World.blast: CollisionComp's bounds, sim x = -UE Y, sim y = UE Z):
    /// relative scale (1, factor x (half Y - inset) / bounds Y, factor x (half Z - inset) / bounds Z), so the
    /// frame's world half extent is AdjustJumpOutCollision's, at (loc X, loc Y, the box's origin Z + loc Z), no
    /// rotation. X keeps the root's scale (1 in the maps; the meshes are flat in X).
    pub fn place(&mut self, b: &mvs_sim::world::BlastBounds) {
        let key = [b.left, b.right, b.bottom, b.top];
        if self.placed == Some(key.map(f64::from)) {
            return;
        }
        self.placed = Some(key.map(f64::from));
        let (half_y, half_z) = ((b.right - b.left) / 2.0, (b.top - b.bottom) / 2.0);
        let scale = [
            1.0,
            self.factor[0] * (f64::from(half_y) - self.inset[0]) / self.bounds[0],
            self.factor[1] * (f64::from(half_z) - self.inset[1]) / self.bounds[1],
        ];
        let loc = [self.loc[0], self.loc[1], f64::from(b.top + b.bottom) / 2.0 + self.loc[2]];
        let m = to_world(&transform(loc, [0.0; 3], scale));
        for (pose, _) in &mut self.parts {
            pose.to_world = m;
        }
    }

    /// Placed at all.
    pub fn placed(&self) -> bool {
        self.placed.is_some()
    }
}

/// The scenery's border entry as the renderer draws it: each part's mesh path, material with its scalars, sort.
pub(super) fn blast_border(s: &Scenery, entry: &str) -> Result<Option<BlastBorder>> {
    let Some(e) = &s.blast_border else { return Ok(None) };
    let parts = e
        .parts
        .iter()
        .map(|p| {
            let what = format!("{entry}: {}", p.name);
            let path = &s.meshes.get(&p.mesh).ok_or_else(|| bad(&what, format!("no mesh {}", p.mesh)))?.path;
            if slot_light(&Some(p.material.clone()), &what)? != Some(SlotLight::Movable) {
                return Err(bad(&what, format!("{}: not a movable component's material", p.material)));
            }
            let mut material = material_ref(&Some(p.material.clone()));
            material.scalars = p.scalars.iter().map(|(n, v)| (Arc::from(n.as_str()), *v as f32)).collect();
            let pose = MeshPose { mesh: Arc::from(path.as_str()), materials: vec![material], ..Default::default() };
            Ok((pose, p.sort as f32))
        })
        .collect::<Result<Vec<_>>>()?;
    if e.bounds.contains(&0.0) {
        return Err(bad(entry, "a blast border mesh with no extent"));
    }
    Ok(Some(BlastBorder { factor: e.factor, inset: e.inset, loc: e.loc, bounds: e.bounds, parts, placed: None }))
}
