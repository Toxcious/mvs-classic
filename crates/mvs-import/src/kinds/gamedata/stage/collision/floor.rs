//! The stage block (`StageData.floor`, `edgeBox`) and its candidates.
//! Deviation: the pick is the port's (the build has no stage block: every candidate is a solid alike).

use std::collections::HashSet;
use std::rc::Rc;

use mvs_data::stage::{EdgeBox, Floor};
use mvs_data::{Props, Value};
use mvs_math::js;

use super::super::super::shape::{fail, short_name};
use super::super::super::transform::quat_rotator;
use super::super::context::{StageCtx, Vec3, ar, f, js_truthy, rot, s, st, strings, truthy, vec, vec0};
use super::solids::mesh_body_setup;
use crate::kind::KindError;
use crate::objview::ObjView;

/// A floor box as the block holds it.
#[derive(Clone, Debug)]
pub struct FloorBox {
    pub center: Vec3,
    pub half_extent: Vec3,
    pub top: f64,
    pub profile: Option<String>,
    pub tags: Vec<String>,
}

impl FloorBox {
    pub fn floor(&self) -> Floor {
        Floor {
            center: self.center.to_vec(),
            half_extent: self.half_extent.to_vec(),
            top: self.top,
            profile: self.profile.clone(),
            tags: self.tags.clone(),
        }
    }

    pub fn edge_box(&self) -> Result<EdgeBox, KindError> {
        let Some(profile) = self.profile.clone() else { return fail("an edge box with no profile") };
        Ok(EdgeBox {
            center: self.center.to_vec(),
            half_extent: self.half_extent.to_vec(),
            top: self.top,
            profile,
            tags: self.tags.clone(),
        })
    }
}

/// A stage block candidate.
#[derive(Debug)]
pub struct BlockCandidate {
    pub actor: String,
    pub floor: Option<FloorBox>,
    pub edge_box: Option<FloorBox>,
    /// The (actor.component) names the block takes, left out of the solids.
    pub skip: HashSet<String>,
}

fn zero3(r: Vec3) -> bool {
    r[0] == 0.0 && r[1] == 0.0 && r[2] == 0.0
}

/// A component's tags (`ComponentTags ?? []`).
fn tags_of(p: &Props, what: &str) -> Result<Vec<String>, KindError> {
    match p.get("ComponentTags") {
        None | Some(Value::Null) => Ok(Vec::new()),
        v => strings(v).map_or_else(|| fail(format!("{what}: ComponentTags is no list of names")), Ok),
    }
}

fn profile_of(p: &Props) -> Option<String> {
    st(p, "BodyInstance").and_then(|b| s(b, "CollisionProfileName")).map(str::to_owned)
}

/// The collision of a static mesh as a box: (min, max) of its BodySetup's one element, a convex element that is a box
/// (SM_Template_Map_Floor) or a box element with no rotation (the engine's BasicShapes Cube).
fn mesh_box(ctx: &StageCtx<'_>, mesh: &str) -> Result<(Vec3, Vec3), KindError> {
    let bs = mesh_body_setup(ctx, mesh)?;
    let empty = Props::new();
    let agg = bs.as_ref().and_then(|b| st(b, "AggGeom")).unwrap_or(&empty);
    let keys: Vec<&String> = agg.keys().collect();
    if keys.len() == 1 && keys[0] == "BoxElems" && ar(agg, "BoxElems").is_some_and(|l| l.len() == 1) {
        let el = ar(agg, "BoxElems")
            .map(|l| &l[0])
            .and_then(Value::as_obj)
            .ok_or_else(|| KindError::Failed(format!("{mesh}: a box element")))?;
        if rot(el.get("Rotation")).iter().any(|v| *v != 0.0) {
            return fail(format!("{mesh}: a rotated box element"));
        }
        let c = vec0(el.get("Center"));
        let ax = ["X", "Y", "Z"];
        let len = |i: usize| f(el, ax[i]).unwrap_or(f64::NAN);
        return Ok(([0, 1, 2].map(|i| c[i] - len(i) / 2.0), [0, 1, 2].map(|i| c[i] + len(i) / 2.0)));
    }
    if !(keys.len() == 1 && keys[0] == "ConvexElems" && ar(agg, "ConvexElems").is_some_and(|l| l.len() == 1)) {
        return fail(format!("{mesh}: not one box or convex element"));
    }
    let el = ar(agg, "ConvexElems")
        .map(|l| &l[0])
        .and_then(Value::as_obj)
        .ok_or_else(|| KindError::Failed(format!("{mesh}: a convex element")))?;
    let elem_box = st(el, "ElemBox");
    let lo = vec0(elem_box.and_then(|b| b.get("Min")));
    let hi = vec0(elem_box.and_then(|b| b.get("Max")));
    let verts = ar(el, "VertexData").unwrap_or(&[]);
    for v in verts {
        let v = v.as_obj();
        let ok = ["X", "Y", "Z"].iter().enumerate().all(|(i, k)| {
            let x = v.and_then(|v| f(v, k)).unwrap_or(f64::NAN);
            js::min((x - lo[i]).abs(), (x - hi[i]).abs()) < 1e-3
        });
        if !ok {
            return fail(format!("{mesh}: a convex element that is no box"));
        }
    }
    let tr = vec0(st(el, "Transform").and_then(|t| t.get("Translation")));
    if verts.len() != 8 || tr.iter().any(|v| *v != 0.0) {
        return fail(format!("{mesh}: a convex element that is no box"));
    }
    Ok((lo, hi))
}

/// The floor box of a root static mesh component whose collision is the floor: its mesh's box under the actor's
/// transform (no rotation), the bounds as floats as the engine holds them.
fn scaled_mesh_floor(
    ctx: &StageCtx<'_>,
    smc: &Props,
    loc: Vec3,
    sc: Vec3,
    tags: Vec<String>,
) -> Result<FloorBox, KindError> {
    let Some(mesh) = s(smc, "StaticMesh") else { return fail("a mesh floor with no StaticMesh") };
    let (lo0, hi0) = mesh_box(ctx, mesh)?;
    let lo = [0, 1, 2].map(|i| js::fround(loc[i] + lo0[i] * sc[i]));
    let hi = [0, 1, 2].map(|i| js::fround(loc[i] + hi0[i] * sc[i]));
    Ok(FloorBox {
        center: [0, 1, 2].map(|i| (hi[i] + lo[i]) / 2.0),
        half_extent: [0, 1, 2].map(|i| (hi[i] - lo[i]) / 2.0),
        top: hi[2],
        profile: Some("Floor".to_owned()),
        tags,
    })
}

/// The Box components of an actor tagged TraceEdges, with their properties over their templates'.
pub fn traced_boxes(ctx: &StageCtx<'_>, a: &ObjView<'_>) -> Result<Vec<(String, Rc<Props>)>, KindError> {
    let mut out = Vec::new();
    for c in ctx.comps(a, Some("BoxComponent")) {
        let p = ctx.with_templates(&c)?;
        if tags_of(&p, c.path())?.iter().any(|t| t == "TraceEdges") {
            out.push((short_name(c.path()).to_owned(), p));
        }
    }
    Ok(out)
}

/// Whether a block candidate can be the block: one tagged Box with no rotation under an unrotated root (`r`, the
/// root's world rotation).
fn one_traced_box(r: Vec3, traced: &[(String, Rc<Props>)]) -> bool {
    zero3(r) && traced.len() == 1 && !truthy(traced[0].1.get("RelativeRotation"))
}

/// Whether a candidate's tagged Box is its edge box and its mesh the collision (the Box NoCollision, the actor's one
/// StaticMeshComponent of profile Floor). The render part does not draw such a mesh: the candidate's own test, not
/// the stage block's pick.
pub fn is_edge_box(
    ctx: &StageCtx<'_>,
    a: &ObjView<'_>,
    r: Vec3,
    traced: &[(String, Rc<Props>)],
) -> Result<bool, KindError> {
    if !one_traced_box(r, traced) {
        return Ok(false);
    }
    let smcs = ctx.comps(a, Some("StaticMeshComponent"));
    if smcs.len() > 1 {
        return Ok(false);
    }
    let smc = match smcs.first() {
        Some(c) => Some(ctx.with_templates(c)?),
        None => None,
    };
    Ok(profile_of(&traced[0].1).as_deref() == Some("NoCollision")
        && smc.as_deref().and_then(profile_of).as_deref() == Some("Floor"))
}

/// A stage block candidate, an actor with a Box tagged TraceEdges: its floor and edge box where it can be the block
/// (one tagged Box, no rotation, under the root), else floor none.
pub fn block_candidate(
    ctx: &StageCtx<'_>,
    a: &ObjView<'_>,
    loc: Vec3,
    r: Vec3,
    sc: Vec3,
    floor: &[(String, Rc<Props>)],
) -> Result<BlockCandidate, KindError> {
    let n = short_name(a.path()).to_owned();
    let mut out = BlockCandidate { actor: n.clone(), floor: None, edge_box: None, skip: HashSet::new() };
    let box_ = &floor[0].1;
    if !one_traced_box(r, floor) {
        return Ok(out);
    }
    out.skip.insert(format!("{n}.{}", floor[0].0));
    // UBoxComponent's default BoxExtent is 32 (stock 4.26 constructor)
    let ext = vec(box_.get("BoxExtent"), [32.0; 3]);
    let sv = vec(box_.get("RelativeScale3D"), [1.0; 3]);
    let bl = vec0(box_.get("RelativeLocation"));
    let he = [0, 1, 2].map(|i| ext[i] * sv[i] * sc[i]);
    let ce = [0, 1, 2].map(|i| loc[i] + bl[i] * sc[i]);
    let tagged = FloorBox {
        center: ce,
        half_extent: he,
        top: ce[2] + he[2],
        profile: profile_of(box_),
        tags: tags_of(box_, a.path())?,
    };
    // the actor's mesh, when it has one (the Lab's floor actor is the Box alone)
    let mut smcs = Vec::new();
    for c in ctx.comps(a, Some("StaticMeshComponent")) {
        smcs.push((short_name(c.path()).to_owned(), ctx.with_templates(&c)?));
    }
    if smcs.len() > 1 {
        return Ok(out);
    }
    let smc = smcs.first().map(|x| x.1.clone());
    let mesh_profile = smc.as_deref().and_then(profile_of);
    // (Batcave's BCAVE_ST_main_A_01_BP Box: BlockAll, the ini's EditProfiles giving it the Floor profile's blockers)
    if matches!(tagged.profile.as_deref(), Some("Floor" | "BlockAll")) {
        if mesh_profile.as_deref().is_some_and(|m| m != "NoCollision") {
            return Ok(out);
        }
        out.floor = Some(tagged);
    } else if is_edge_box(ctx, a, r, floor)? {
        // the mesh is the collision (the root component: the actor's transform)
        let Some(smc) = &smc else { return fail(format!("{}: an edge box with no mesh", a.path())) };
        out.floor = Some(scaled_mesh_floor(ctx, smc, loc, sc, Vec::new())?);
        out.edge_box = Some(tagged);
        out.skip.insert(format!("{n}.{}", smcs[0].0));
    }
    Ok(out)
}

/// A stage block candidate of a level with no Box tagged TraceEdges (Classic 3 Platform's TrainingFloor): a
/// StaticMeshActor whose one StaticMeshComponent0, its root, stores the Floor profile (bUseDefaultCollision false) and
/// no rotation. Deviation: the pick.
pub fn mesh_block_candidate(
    ctx: &StageCtx<'_>,
    a: &ObjView<'_>,
    loc: Vec3,
    r: Vec3,
    sc: Vec3,
) -> Result<Option<BlockCandidate>, KindError> {
    if short_name(a.class()) != "StaticMeshActor" || !zero3(r) {
        return Ok(None);
    }
    let cs = ctx.comps(a, Some("StaticMeshComponent"));
    if cs.len() != 1 || short_name(cs[0].path()) != "StaticMeshComponent0" {
        return Ok(None);
    }
    let smc = ctx.with_templates(&cs[0])?;
    let use_default = match smc.get("bUseDefaultCollision") {
        None | Some(Value::Null) => true,
        v => js_truthy(v),
    };
    if use_default || profile_of(&smc).as_deref() != Some("Floor") {
        return Ok(None);
    }
    if smc.get("RelativeRotation").is_some_and(|v| js_truthy(Some(v))) && !zero3(rot(smc.get("RelativeRotation"))) {
        return Ok(None);
    }
    let n = short_name(a.path()).to_owned();
    let tags = tags_of(&smc, a.path())?;
    Ok(Some(BlockCandidate {
        actor: n.clone(),
        floor: Some(scaled_mesh_floor(ctx, &smc, loc, sc, tags)?),
        edge_box: None,
        skip: HashSet::from([format!("{n}.StaticMeshComponent0")]),
    }))
}

/// A stage block candidate of a level with neither a Box tagged TraceEdges nor a StaticMeshActor candidate
/// (Map_TrophyRoom_Tutorial's TROPHY_TUTORIAL_PlatformCenter_Blueprint_C, its Box tagged IgnoreHitStun alone): an
/// actor's one Box of profile Floor (its values over its templates'), with no rotation in its world transform; the
/// box is its world centre and BoxExtent times its world scale. Deviation: the pick, as the other candidates'.
pub fn floor_box_candidate(ctx: &StageCtx<'_>, a: &ObjView<'_>) -> Result<Option<BlockCandidate>, KindError> {
    let mut boxes = Vec::new();
    for c in ctx.comps(a, Some("BoxComponent")) {
        let p = ctx.with_templates(&c)?;
        if profile_of(&p).as_deref() == Some("Floor") {
            boxes.push((short_name(c.path()).to_owned(), p, c));
        }
    }
    if boxes.len() != 1 {
        return Ok(None);
    }
    let (name, p, c) = &boxes[0];
    let w = ctx.world(c)?;
    let r = quat_rotator(w.q).map(|v| js::round(v * 1e6) / 1e6);
    if !zero3(r) {
        return Ok(None);
    }
    // UBoxComponent's default BoxExtent is 32 (stock 4.26 constructor)
    let ext = vec(p.get("BoxExtent"), [32.0; 3]);
    let he = [0, 1, 2].map(|i| (ext[i] * w.scale[i]).abs());
    let ce = w.loc;
    let n = short_name(a.path()).to_owned();
    Ok(Some(BlockCandidate {
        actor: n.clone(),
        floor: Some(FloorBox {
            center: ce,
            half_extent: he,
            top: ce[2] + he[2],
            profile: Some("Floor".to_owned()),
            tags: tags_of(p, a.path())?,
        }),
        edge_box: None,
        skip: HashSet::from([format!("{n}.{name}")]),
    }))
}

/// The stage block: of the candidates that can be one, the one under Player1's start with the highest top (the first
/// of equal tops); else the first. The candidates: the actors with a Box tagged TraceEdges, else the StaticMeshActors
/// of profile Floor (`mesh_blocks`), else the actors with a Box of profile Floor (`floor_blocks`). `p1`: the first
/// PlayerStart tagged Player1's location.
pub fn pick_block<'c>(
    key: &str,
    blocks: &'c [BlockCandidate],
    mesh_blocks: &'c [BlockCandidate],
    floor_blocks: &'c [BlockCandidate],
    p1: Option<Vec3>,
) -> Result<&'c BlockCandidate, KindError> {
    let able: Vec<&BlockCandidate> = if !blocks.is_empty() {
        blocks.iter().filter(|b| b.floor.is_some()).collect()
    } else if !mesh_blocks.is_empty() {
        mesh_blocks.iter().collect()
    } else {
        floor_blocks.iter().collect()
    };
    let Some(first) = able.first() else {
        return fail(format!(
            "stage data {key}: no stage block (a Box tagged TraceEdges, profile Floor, or a NoCollision one over a Floor mesh, or with no tagged Box a StaticMeshActor of profile Floor, or with neither a Box of profile Floor)"
        ));
    };
    let floor = |b: &BlockCandidate| b.floor.clone().expect("a candidate that can be the block");
    let under: Vec<&BlockCandidate> = match p1 {
        Some(p1) => able
            .iter()
            .copied()
            .filter(|b| {
                let fl = floor(b);
                (p1[1] - fl.center[1]).abs() <= fl.half_extent[1] && fl.top <= p1[2]
            })
            .collect(),
        None => Vec::new(),
    };
    if under.is_empty() {
        return Ok(first);
    }
    let mut pick = under[0];
    for b in &under {
        if floor(b).top > floor(pick).top {
            pick = b;
        }
    }
    Ok(pick)
}
