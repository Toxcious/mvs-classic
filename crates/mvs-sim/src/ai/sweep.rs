//! The bots' ground sweep: SphereTraceSingle / SphereTraceMulti of TraceTypeQuery4
//! (ProjectileGroundTrace), shared by the decorators and tasks of `ai/` and the characters' own bot tasks.

use crate::Real;
use crate::actors::world_actor::{actor_cls, actor_place};
use crate::fighter::FighterId;
use crate::names::Name;
use crate::world::World;
use crate::world::spawned_platform::{BlockKind, BoxMark};
use crate::world::stage_query::StageShape;
use mvs_collision::sweep::sweep_box;

/// `SweepHit`: the nearest box the sweep hit and where. The stage's pass-through platforms (profile Platform) block
/// the trace like the block, and so do the pieces of a C019_Ship with their collision on, the boxes of the other
/// spawned actors that answer it (`World::blockers`) and the polygon solids that block it (`World::blocker_shapes`).
#[derive(Clone, Debug)]
pub struct SweepHit {
    /// The sweep's centre at the hit, world Y / Z.
    pub y: Real,
    pub z: Real,
    /// The shape hit.
    pub shape: StageShape,
    /// The box is a ship's piece.
    pub ship: bool,
    /// The class of the actor the box belongs to (the hit's HitActor): a spawned actor's, a placed stage actor's for
    /// one of its components' solids; `None` for the stage's own boxes, a solid no placed actor owns and the ships.
    pub host: Option<Name>,
    /// That actor's K2_GetActorLocation Z (0 with no host).
    pub host_z: Real,
}

/// `traceSweep(w, y0, z0, y1, z1, r, skip)`: a sphere of radius `r` from (y0, z0) to (y1, z1) in world Y / Z;
/// `skip`: a fighter whose own ships the trace ignores (ActorsToIgnore = its Comp_Spawner.SpawnedActors).
pub fn trace_sweep(
    w: &mut World,
    y0: Real,
    z0: Real,
    y1: Real,
    z1: Real,
    r: Real,
    skip: Option<FighterId>,
) -> Option<SweepHit> {
    let mut ps = Vec::new();
    let mut marks = Vec::new();
    w.blockers_marked_into(BlockKind::Trace, &mut ps, &mut marks);
    let mut polys = Vec::new();
    crate::fighter::traces::solids::blocker_polys(w, BlockKind::Trace, &mut polys);
    let (dy, dz) = (y1 - y0, z1 - z0);
    let mut best: Option<(StageShape, BoxMark)> = None;
    let mut bt = Real::INFINITY;
    // (the stage's block first, then the list, then the polygons)
    let floor = std::iter::once((w.stage.ground, BoxMark::Plain));
    for (g, m) in floor.chain(ps.iter().copied().zip(marks.iter().copied())) {
        if skip.is_some_and(|s| m == BoxMark::Ship(s)) {
            continue;
        }
        let t0 = sweep_box(&g, y0, z0, dy, dz, r);
        if t0 < 0.0 || t0 >= f64::from(bt) {
            continue;
        }
        bt = crate::math::narrow(t0);
        best = Some((StageShape::Box(g), m));
    }
    // a polygon: the sphere swept in the play plane (sim x = -Y)
    for q in polys {
        let Some(h) = q.sweep(-y0, z0, -dy, dz, 0.0, 0.0, r) else { continue };
        let t0 = crate::math::narrow(h.time);
        if t0 >= bt {
            continue;
        }
        bt = t0;
        let id = w.stage_run.as_ref().expect("a polygon solid with the stage's run").solids[q.solid as usize].id;
        best = Some((StageShape::Poly(q), BoxMark::Solid(id)));
    }
    let (shape, m) = best?;
    let (mut host, mut host_z) = (None, 0.0);
    match m {
        BoxMark::Actor(a) => {
            host = actor_cls(w, a);
            host_z = actor_place(w, a).map_or(0.0, |p| p.1);
        }
        BoxMark::Solid(id) => {
            let owner =
                w.data.stage.placed.iter().flatten().rfind(|d| d.solids.as_ref().is_some_and(|s| s.contains(&id)));
            if let Some(d) = owner {
                let run = w.stage_run.as_ref().expect("a stage solid with the stage's run");
                let dy = run.solids.iter().find(|s| s.id == id).map_or(0.0, |s| s.m.dy);
                host = Some(w.lit(&d.cls));
                host_z = crate::math::narrow(d.loc[2]) + dy;
            }
        }
        BoxMark::Plain | BoxMark::Ship(_) => {}
    }
    Some(SweepHit { y: y0 + dy * bt, z: z0 + dz * bt, shape, ship: matches!(m, BoxMark::Ship(_)), host, host_z })
}
