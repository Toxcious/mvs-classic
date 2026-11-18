//! The stage data's placed actors (`StageData.placed`): each level actor whose class a stage module enters, its root's
//! world transform and its properties over its class defaults.

use mvs_data::stage::StagePlacedData;

use super::super::super::shape::short_name;
use super::super::context::{StageCtx, v3};
use crate::kind::KindError;
use crate::objview::ObjView;

/// The class names (with _C) the stage modules enter (checked against the stage modules' stageActors).
pub const ENTERED_STAGE_ACTORS: [&str; 13] = [
    "TreehouseSidePillar_C",
    "TreehouseSidePillar_Child_C",
    "M009_DestructiblePlatformBP_C",
    "M009_DestructiblePlatformBP_Child_C",
    "ScoobyWall_C",
    "ScoobyElevator_C",
    "ScoobyElevatorSwitch_C",
    "FogOcclusionBP_C",
    "BatcaveWallDestructible_C",
    "BatcaveBatsBP_C",
    "M008_Elevator_BP_C",
    "m010_wall_C",
    "M010_Hoop_A_01_BP_C",
];

/// StageData.placed: each actor of `actors` whose class is entered, in the levels' order: its name and class, the
/// root's world transform (rootXf) and the instance's values over the class defaults, less RootComponent, in the
/// store's form (a reference is its path). The ids of its solids are the caller's.
pub fn placed_actors(ctx: &StageCtx<'_>, actors: &[ObjView<'_>]) -> Result<Vec<StagePlacedData>, KindError> {
    let mut out = Vec::new();
    for a in actors {
        let cls = short_name(a.class());
        if !ENTERED_STAGE_ACTORS.contains(&cls) {
            continue;
        }
        let (loc, rot, scale) = ctx.root_xf(a)?;
        let mut p = (*ctx.with_templates(a)?).clone();
        p.shift_remove("RootComponent");
        out.push(StagePlacedData {
            name: short_name(a.path()).to_owned(),
            cls: cls.to_owned(),
            loc: v3(loc),
            rot: v3(rot),
            scale: v3(scale),
            p,
            solids: None,
        });
    }
    Ok(out)
}
