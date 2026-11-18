//! The stage data's collision part: the floor and its pick, platforms, the edge box, edges, solids and the placed
//! actors (whose solids ids it sets). The levels' actors in export order.

pub mod edges;
pub mod floor;
pub mod placed;
pub mod platforms;
pub mod solids;

use std::collections::HashSet;

use mvs_data::stage::{
    ComplexSegment, Edge, EdgeBox, Floor, Platform, StagePlacedData, StageQueryBody, StageSolidData,
};

use self::edges::edge_owners;
use self::floor::{
    BlockCandidate, block_candidate, floor_box_candidate, mesh_block_candidate, pick_block, traced_boxes,
};
use self::placed::placed_actors;
use self::platforms::{is_platform, platform};
use self::solids::solids;
use super::super::shape::short_name;
use super::context::{StageCtx, Vec3, js_truthy, s};
use crate::kind::KindError;
use crate::objview::ObjView;

/// What the collision part writes.
pub struct CollisionOut {
    pub floor: Floor,
    pub platforms: Vec<Platform>,
    pub edge_box: Option<EdgeBox>,
    pub edges: Option<Vec<Edge>>,
    pub solids: Vec<StageSolidData>,
    pub placed: Vec<StagePlacedData>,
    pub query_bodies: Vec<StageQueryBody>,
    pub complex_segments: Vec<ComplexSegment>,
}

// (ctx.actors leaves out a LevelSequenceActor's own subobjects (trophyroom_2platforms, m010's scenery): they are no
// platform, candidate, solid, edge owner or placed actor.)

/// The collision part (COLLISION_PART.bind).
pub fn bind(ctx: &StageCtx<'_>) -> Result<CollisionOut, KindError> {
    let mut platforms: Vec<Platform> = Vec::new();
    let mut platform_actors: HashSet<String> = HashSet::new();
    let mut blocks: Vec<BlockCandidate> = Vec::new();
    let mut mesh_blocks: Vec<BlockCandidate> = Vec::new();
    // (the actors that are none of the above: their Boxes of profile Floor are candidates only where the level has no
    // other)
    let mut rest: Vec<ObjView<'_>> = Vec::new();
    // (the first PlayerStart tagged Player1: StageData.spawns' Player1, which the pick reads; the sim part writes spawns)
    let mut p1: Option<Vec3> = None;
    let actors = ctx.actors();
    for a in &actors {
        let t = short_name(a.class());
        let (loc, r, sc) = ctx.root_xf(a)?;
        if is_platform(ctx, a)? {
            platform_actors.insert(a.path().to_owned());
            if let Some(p) = platform(ctx, a)? {
                platforms.push(p);
            }
            continue;
        }
        // the stage block: every actor with a Box tagged TraceEdges is a candidate; with none, a StaticMeshActor of
        // profile Floor; with neither, an actor's Box of profile Floor
        let floor = traced_boxes(ctx, a)?;
        let ap = ctx.properties(a)?;
        if !floor.is_empty() {
            blocks.push(block_candidate(ctx, a, loc, r, sc, &floor)?);
        } else if js_truthy(ap.get("bHidden")) {
            continue;
        } else if t == "StaticMeshActor" {
            if let Some(mb) = mesh_block_candidate(ctx, a, loc, r, sc)? {
                mesh_blocks.push(mb);
            }
        } else if t == "PlayerStart" {
            if p1.is_none() && s(&ap, "PlayerStartTag") == Some("Player1") {
                p1 = Some(loc);
            }
        } else {
            rest.push(a.clone());
        }
    }
    let scenery = ctx.scenery_actors();
    for a in &scenery {
        if is_platform(ctx, a)? {
            platform_actors.insert(a.path().to_owned());
            if let Some(p) = platform(ctx, a)? {
                platforms.push(p);
            }
        }
    }

    let mut placed = placed_actors(ctx, &actors)?;
    let mut floor_blocks: Vec<BlockCandidate> = Vec::new();
    if blocks.is_empty() && mesh_blocks.is_empty() {
        for a in &rest {
            if let Some(fb) = floor_box_candidate(ctx, a)? {
                floor_blocks.push(fb);
            }
        }
    }
    let pick = pick_block(&ctx.key, &blocks, &mesh_blocks, &floor_blocks, p1)?;
    let all: Vec<ObjView<'_>> = actors.iter().chain(&scenery).cloned().collect();
    let mut query = Vec::new();
    let mut complex = Vec::new();
    let placed_classes: HashSet<String> = placed.iter().map(|pa| pa.cls.clone()).collect();
    let sol = solids(ctx, &all, &pick.skip, &placed_classes, &platform_actors, &mut query, &mut complex)?;
    // a placed actor's solids: those of its components (the gameplay level's: actor names repeat between levels)
    for pa in &mut placed {
        let ids: Vec<i32> = sol
            .iter()
            .filter(|x| x.li == 0 && x.e.name.split('.').next() == Some(pa.name.as_str()))
            .map(|x| x.e.id)
            .collect();
        if !ids.is_empty() {
            pa.solids = Some(ids);
        }
    }
    let edges = edge_owners(ctx, &all, &pick.actor, &platforms, &sol)?;

    let Some(pf) = &pick.floor else { return Err(KindError::Failed("the picked stage block has no floor".into())) };
    Ok(CollisionOut {
        floor: pf.floor(),
        platforms,
        edge_box: pick.edge_box.as_ref().map(floor::FloorBox::edge_box).transpose()?,
        edges,
        solids: sol.into_iter().map(|x| x.e).collect(),
        placed,
        query_bodies: query,
        complex_segments: complex,
    })
}
