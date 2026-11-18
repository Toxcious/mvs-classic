//! The stage data's solids (`StageData.solids`): every blocking Box, static mesh box / convex element and capsule of
//! the level actors besides the stage block and the platforms, a placed class's overlap shapes, each by its world
//! transform and collision profile.

use std::collections::HashSet;

use mvs_data::stage::SolidKind;
use mvs_data::stage::{ComplexSegment, StageQueryBody, StageSolidData};
use mvs_data::{Props, Value};
use mvs_math::js;
use mvs_uasset::natives::static_mesh::read_static_mesh_tail;

use super::super::super::profiles::resolve_body;
use super::super::super::shape::{fail, short_name};
use super::super::super::transform::{
    Quat, Xf, identity_rot, mirrored_quat, qmul, qrot, quat_rotator, rotator_quat, tf_apply,
};
use super::super::context::{StageCtx, Vec3, ar, f, js_truthy, rot, s, st, strings, vec, vec0};
use crate::kind::KindError;
use crate::objview::ObjView;

/// StageSolidData.flags: the channels whose response is Block.
const SOLID_FLAGS: [(&str, i32); 8] = [
    ("Pawn", 1 | 2),
    ("ProjectileGroundTrace", 8),
    ("PhysicsBody", 16),
    ("Item", 32),
    ("WorldStaticDetection", 64),
    ("WallJump", 128),
    ("ProjectileExplode", 256),
    ("ProjectileGroundNoPlatform", 512),
];
/// SOLID_ONE_WAY: object type Platform, a pass-through platform's (BasePlatformBP's rule).
const SOLID_ONE_WAY: i32 = 4;
/// CanCharacterStepUpOn ECB_Yes (UPrimitiveComponent's default).
const SOLID_STEP: i32 = 1024;

/// A BodyInstance's (profile, enabled, flags, object type).
pub struct Col {
    pub prof: String,
    pub en: String,
    pub flags: i32,
    pub obj: String,
}

/// (profile, enabled, flags, object type) of a BodyInstance: a profile the ini lacks stops the bind. An empty object
/// type name (Floor's) is WorldStatic: FCollisionResponseTemplate's ObjectType default. Unverified.
pub fn collision(ctx: &StageCtx<'_>, bi: Option<&Props>, default_profile: &str) -> Result<Col, KindError> {
    let mut body = bi.cloned().unwrap_or_default();
    if !body.contains_key("CollisionProfileName") {
        body.insert("CollisionProfileName".into(), Value::Str(default_profile.to_owned()));
    }
    let table = ctx.collisions()?;
    let r = resolve_body(&body, table)?;
    let Some(resp) = &r.resp else {
        return fail(format!("collision profile {} is not in the ini", r.prof.as_deref().unwrap_or("null")));
    };
    let def = table.profiles.get(default_profile);
    // (a named profile's own CollisionEnabled wins: FBodyInstance's LoadProfileData)
    let en =
        r.enabled.clone().or_else(|| def.map(|d| d.enabled.clone())).unwrap_or_else(|| "QueryAndPhysics".to_owned());
    let mut flags = 0;
    for (ch, bit) in SOLID_FLAGS {
        if resp.get(ch).is_some_and(|v| v == "ECR_Block") {
            flags |= bit;
        }
    }
    let mut obj = r.obj.clone();
    if obj.as_deref() == Some("Platform") && flags != 0 {
        flags |= SOLID_ONE_WAY;
    }
    // (a Custom body that stores no ObjectType keeps the default profile's: a shape's OverlapAllDynamic WorldDynamic)
    if obj.is_none() && (r.prof.is_none() || r.prof.as_deref() == Some("Custom")) {
        obj = def.map(|d| d.obj.clone());
    }
    let obj = obj.filter(|o| !o.is_empty()).unwrap_or_else(|| "WorldStatic".to_owned());
    Ok(Col { prof: r.prof.unwrap_or_else(|| "Custom".to_owned()), en, flags, obj })
}

/// A solid as `solids` leaves it: its level beside it until the edges and placed actors have read it.
pub struct LevelSolid {
    pub e: StageSolidData,
    pub li: i32,
}

/// The BodySetup of a static mesh (the first BodySetup of its package), or none.
pub fn mesh_body_setup(ctx: &StageCtx<'_>, mesh: &str) -> Result<Option<Props>, KindError> {
    let m = ctx.load(mesh)?;
    // (sorted by `index`: inner() is in export order)
    let Some(bs) = m.inner()?.into_iter().find(|o| short_name(o.class()) == "BodySetup") else { return Ok(None) };
    Ok(Some((*ctx.properties(&bs)?).clone()))
}

/// Math.round on values within 1e-5 of 0 or +-1.
fn round(x: f64) -> f64 {
    js::round(x)
}

/// What an `add` call knows.
struct Shape {
    name: String,
    kind: &'static str,
    center: Vec3,
    he: Vec3,
    q: Option<Quat>,
    points: Option<Vec<f64>>,
}

struct Common {
    prof: String,
    obj: String,
    en: String,
    flags: i32,
    tags: Vec<String>,
    step: bool,
}

fn add(sub: &mut Vec<StageSolidData>, sh: Shape, c: &Common) {
    let mut kind = sh.kind;
    let mut he = sh.he;
    if kind == "obox" {
        let q = sh.q.expect("an obox has a rotation");
        // axes on the world's axes (a yaw of 180, a quarter turn): the box it is, the extents in world order
        let m: Vec<Vec3> = (0..3).map(|j| qrot(q, [0, 1, 2].map(|i| if i == j { 1.0 } else { 0.0 }))).collect();
        if m.iter().all(|col| col.iter().all(|v| js::min(v.abs(), (v.abs() - 1.0).abs()) < 1e-5)) {
            kind = "box";
            he = [0, 1, 2].map(|i| (0..3).fold(0.0, |acc, j| acc + round(m[j][i]).abs() * sh.he[j]));
        }
    }
    let kind_e = match kind {
        "box" => SolidKind::Box,
        "obox" => SolidKind::Obox,
        "convex" => SolidKind::Convex,
        "capsule" => SolidKind::Capsule,
        _ => SolidKind::Sphere,
    };
    let rot_v = (kind == "obox").then(|| quat_rotator(sh.q.expect("an obox has a rotation")).to_vec());
    sub.push(StageSolidData {
        id: 0,
        name: sh.name,
        kind: kind_e,
        center: sh.center.to_vec(),
        half_extent: he.to_vec(),
        rot: rot_v,
        axis: None,
        radius: None,
        half_height: None,
        points: sh.points,
        flags: c.flags | if c.step { SOLID_STEP } else { 0 },
        profile: c.prof.clone(),
        object_type: Some(c.obj.clone()),
        tags: if c.tags.is_empty() { None } else { Some(c.tags.clone()) },
        // (a CollisionEnabled other than QueryAndPhysics / QueryOnly is taken as none)
        enabled: (c.en != "QueryAndPhysics" && c.en != "QueryOnly").then_some(false),
    });
}

fn tags_of(p: &Props, what: &str) -> Result<Vec<String>, KindError> {
    match p.get("ComponentTags") {
        None | Some(Value::Null) => Ok(Vec::new()),
        v => strings(v).map_or_else(|| fail(format!("{what}: ComponentTags is no list of names")), Ok),
    }
}

/// A body element's number member, NaN where it has none.
fn n(p: &Props, k: &str) -> f64 {
    f(p, k).unwrap_or(f64::NAN)
}

/// The solid colliders of `actors` (the gameplay and lighting levels', then the scenery levels'), less the (actor,
/// component) names in `skip` (the stage block's, gameplay level) and the platforms' Boxes; of a placed class
/// (`placed`, gameplay level only) a shape that blocks nothing but is on. A sub-level's shape off the play plane X = 0
/// is left out. Into `query`: the Boxes of the other actors that block nothing, with query collision and an object
/// type other than WorldDynamic (the overlap triggers' OverlapAllDynamic, which no port query takes from the level).
pub fn solids(
    ctx: &StageCtx<'_>,
    actors: &[ObjView<'_>],
    skip: &HashSet<String>,
    placed: &HashSet<String>,
    platforms: &HashSet<String>,
    query: &mut Vec<StageQueryBody>,
    complex: &mut Vec<ComplexSegment>,
) -> Result<Vec<LevelSolid>, KindError> {
    let mut out: Vec<LevelSolid> = Vec::new();
    for a in actors {
        let ty = short_name(a.class()).to_owned();
        if ty == "BlastBoxPlayerExit_C" {
            continue;
        }
        let li = ctx.li_of(a)?;
        let mine = li == 0 && placed.contains(&ty);
        let mut sub: Vec<StageSolidData> = Vec::new();
        let an = short_name(a.path()).to_owned();
        let ap = ctx.properties(a)?;
        let plat = platforms.contains(a.path());
        if matches!(ap.get("bActorEnableCollision"), Some(Value::Bool(false))) {
            continue;
        }
        for c in ctx.comps(a, None) {
            let t = short_name(c.class()).to_owned();
            let cn = short_name(c.path()).to_owned();
            if (li == 0 && skip.contains(&format!("{an}.{cn}"))) || (plat && cn == "Box" && t == "BoxComponent") {
                continue;
            }
            let nm = format!("{an}.{cn}");
            let p = ctx.with_templates(&c)?;
            let step = s(&p, "CanCharacterStepUpOn").unwrap_or("ECB_Yes") == "ECB_Yes";
            let tags = tags_of(&p, &nm)?;
            let body = st(&p, "BodyInstance");
            match t.as_str() {
                "BoxComponent" => {
                    let col = collision(ctx, body, "OverlapAllDynamic")?;
                    let queried = col.flags == 0 && !mine && col.en != "NoCollision" && col.obj != "WorldDynamic";
                    if col.flags == 0 && !(mine && col.en != "NoCollision") && !queried {
                        continue;
                    }
                    let w = ctx.world(&c)?;
                    let ext = vec(p.get("BoxExtent"), [32.0; 3]);
                    let he = [0, 1, 2].map(|i| js::max((ext[i] * w.scale[i]).abs(), 1e-4));
                    if queried {
                        // a box that blocks nothing, of an object type of its own: met by the object queries alone
                        // (StageData.queryBodies). BlockPortalCollision_C's Box (profile Custom, ECC_GameTraceChannel13
                        // Portal, every channel Ignore) is the one the maps place. Deviation: Box components alone, on
                        // the gameplay level, unrotated; a query-only box of type WorldDynamic (the overlap triggers)
                        // is left out, and an object query for WorldDynamic does not meet it.
                        if li != 0 || !identity_rot(w.q, 1e-7) {
                            return fail(format!("{nm}: a query-only box off the gameplay level or turned"));
                        }
                        query.push(StageQueryBody {
                            name: nm,
                            center: w.loc.to_vec(),
                            half_extent: he.to_vec(),
                            profile: col.prof,
                            object_type: col.obj,
                        });
                        continue;
                    }
                    let kind = if identity_rot(w.q, 1e-7) { "box" } else { "obox" };
                    let cm = Common { prof: col.prof, obj: col.obj, en: col.en, flags: col.flags, tags, step };
                    add(&mut sub, Shape { name: nm, kind, center: w.loc, he, q: Some(w.q), points: None }, &cm);
                }
                "StaticMeshComponent" => {
                    let Some(mesh) = s(&p, "StaticMesh").filter(|m| !m.is_empty()) else { continue };
                    let own = ty == "StaticMeshActor" && cn == "StaticMeshComponent0";
                    let Some(bs) = mesh_body_setup(ctx, mesh)? else { continue };
                    let use_default = match p.get("bUseDefaultCollision") {
                        None | Some(Value::Null) => own,
                        v => js_truthy(v),
                    };
                    let col = if use_default {
                        collision(ctx, st(&bs, "DefaultInstance"), "BlockAll")?
                    } else {
                        collision(ctx, body, if own { "BlockAll" } else { "BlockAllDynamic" })?
                    };
                    let empty = Props::new();
                    let agg = st(&bs, "AggGeom").unwrap_or(&empty);
                    let queried = col.en == "QueryOnly" || col.en == "QueryAndPhysics";
                    let complex_on = s(&bs, "CollisionTraceFlag") != Some("CTF_UseSimpleAsComplex");
                    if col.flags != 0 && agg.is_empty() && queried && complex_on {
                        complex_segments(ctx, mesh, &ctx.world(&c)?, &nm, col.flags, complex)?;
                    }
                    if col.flags == 0 || agg.is_empty() {
                        continue;
                    }
                    let tf: Xf = ctx.world(&c)?;
                    let (q, sc) = (tf.q, tf.scale);
                    let cm = Common { prof: col.prof, obj: col.obj, en: col.en, flags: col.flags, tags, step };
                    for (i, el) in ar(agg, "BoxElems").unwrap_or(&[]).iter().enumerate() {
                        let el = el.as_obj().cloned().unwrap_or_default();
                        // FKBoxElem: the centre and the full lengths scaled by the component's scale (EF 845: at
                        // least 1e-4); its own rotation through the component's mirror, then the component's rotation
                        let [rp, ry, rr] = rot(el.get("Rotation"));
                        let eq = rotator_quat(rp, ry, rr);
                        let ec = vec0(el.get("Center"));
                        let wq = qmul(q, mirrored_quat(eq, sc));
                        let he = [0, 1, 2].map(|j| js::max((n(&el, ["X", "Y", "Z"][j]) * 0.5 * sc[j]).abs(), 1e-4));
                        let kind = if identity_rot(wq, 1e-7) { "box" } else { "obox" };
                        add(
                            &mut sub,
                            Shape {
                                name: format!("{nm}.box{i}"),
                                kind,
                                center: tf_apply(&tf, ec),
                                he,
                                q: Some(wq),
                                points: None,
                            },
                            &cm,
                        );
                    }
                    for (i, el) in ar(agg, "ConvexElems").unwrap_or(&[]).iter().enumerate() {
                        let el = el.as_obj().cloned().unwrap_or_default();
                        let empty = Props::new();
                        let et = st(&el, "Transform").unwrap_or(&empty);
                        let etf = Xf {
                            loc: vec0(et.get("Translation")),
                            q: match et.get("Rotation") {
                                Some(Value::Obj(r)) => [n(r, "X"), n(r, "Y"), n(r, "Z"), n(r, "W")],
                                v if js_truthy(v) => [f64::NAN; 4],
                                _ => [0.0, 0.0, 0.0, 1.0],
                            },
                            scale: vec(et.get("Scale3D"), [1.0; 3]),
                        };
                        // (each point through the element's transform, then the component's)
                        let pts: Vec<Vec3> = ar(&el, "VertexData")
                            .unwrap_or(&[])
                            .iter()
                            .map(|v| {
                                let v = v.as_obj().cloned().unwrap_or_default();
                                tf_apply(&tf, tf_apply(&etf, [n(&v, "X"), n(&v, "Y"), n(&v, "Z")]))
                            })
                            .collect();
                        let lo = [0, 1, 2].map(|j| pts.iter().fold(f64::INFINITY, |m, pt| js::min(m, pt[j])));
                        let hi = [0, 1, 2].map(|j| pts.iter().fold(f64::NEG_INFINITY, |m, pt| js::max(m, pt[j])));
                        add(
                            &mut sub,
                            Shape {
                                name: format!("{nm}.convex{i}"),
                                kind: "convex",
                                center: [0, 1, 2].map(|j| (lo[j] + hi[j]) / 2.0),
                                he: [0, 1, 2].map(|j| (hi[j] - lo[j]) / 2.0),
                                q: None,
                                points: Some(pts.iter().flatten().copied().collect()),
                            },
                            &cm,
                        );
                    }
                }
                "CapsuleComponent" => {
                    // A capsule that blocks a channel: its world centre, axis (the component's Z), radius and half
                    // height scaled as UCapsuleComponent's GetScaledCapsuleRadius / HalfHeight (min(|X|, |Y|), |Z|),
                    // halfExtent its world bounds
                    let col = collision(ctx, body, "OverlapAllDynamic")?;
                    if col.flags == 0 {
                        continue;
                    }
                    if !p.contains_key("CapsuleRadius") || !p.contains_key("CapsuleHalfHeight") {
                        return fail(format!(
                            "{nm}: a capsule with no stored CapsuleRadius / CapsuleHalfHeight (its native default is not read)"
                        ));
                    }
                    let w = ctx.world(&c)?;
                    let sc = w.scale;
                    let r = n(&p, "CapsuleRadius") * js::min(sc[0].abs(), sc[1].abs());
                    let hh = n(&p, "CapsuleHalfHeight") * sc[2].abs();
                    let ax = qrot(w.q, [0.0, 0.0, 1.0]);
                    let he = [0, 1, 2].map(|i| ax[i].abs() * js::max(hh - r, 0.0) + r);
                    let cm = Common { prof: col.prof, obj: col.obj, en: col.en, flags: col.flags, tags, step };
                    add(
                        &mut sub,
                        Shape { name: nm, kind: "capsule", center: w.loc, he, q: Some(w.q), points: None },
                        &cm,
                    );
                    let e = sub.last_mut().expect("just added");
                    e.rot = Some(quat_rotator(w.q).to_vec());
                    e.axis = Some(ax.to_vec());
                    e.radius = Some(r);
                    e.half_height = Some(hh);
                }
                "SphereComponent" => {
                    let col = collision(ctx, body, "OverlapAllDynamic")?;
                    if mine && col.flags == 0 && col.en != "NoCollision" {
                        // a placed class's overlap-only sphere (Batcave's FogOcclusionBP PlayerAttachCollision):
                        // SphereRadius 32 unless stored (USphereComponent ctor 1429a60f0), scaled by the least axis of
                        // the world scale
                        let w = ctx.world(&c)?;
                        let least = w.scale.iter().fold(f64::INFINITY, |m, v| js::min(m, v.abs()));
                        let r = f(&p, "SphereRadius").unwrap_or(32.0) * least;
                        let cm = Common { prof: col.prof, obj: col.obj, en: col.en, flags: col.flags, tags, step };
                        add(
                            &mut sub,
                            Shape {
                                name: nm,
                                kind: "sphere",
                                center: w.loc,
                                he: [r, r, r],
                                q: Some(w.q),
                                points: None,
                            },
                            &cm,
                        );
                        sub.last_mut().expect("just added").radius = Some(r);
                    }
                }
                "BrushComponent" => {
                    // (a volume's brush: BlockAll for a BlockingVolume, else NoCollision unless stored; one that
                    // blocks Pawn is not taken)
                    collision(ctx, body, if ty == "BlockingVolume" { "BlockAll" } else { "NoCollision" })?;
                }
                _ => {}
            }
        }
        for mut e in sub {
            // (a sub-level's solid off the play plane: the sim's section would drop it)
            if li != 0 && e.center[0].abs() > e.half_extent[0] {
                continue;
            }
            e.id = i32::try_from(out.len()).expect("a few solids");
            out.push(LevelSolid { e, li });
        }
    }
    Ok(out)
}

/// The collision triangles of static mesh `mesh` placed by `tf`, cut at the play plane (UE X = 0), into `out`. The
/// cooked PhysX triangle mesh is the render LOD[min(LODForCollision, last)]'s sections with bEnableCollision, in index
/// order, less the triangles cooking welds away (two corners within 0.1, PhysX meshWeldTolerance) and exact
/// duplicates (which change no hit): read against BCAVE_ST_main_A_01 and four midground meshes' BodySetup PhysXPC
/// payloads. Its winding is not kept: a segment has no side.
// Unverified: a mesh setting ComplexCollisionMesh (none of the read ones does) is refused, not followed.
fn complex_segments(
    ctx: &StageCtx<'_>,
    mesh: &str,
    tf: &Xf,
    name: &str,
    flags: i32,
    out: &mut Vec<ComplexSegment>,
) -> Result<(), KindError> {
    let m = ctx.load(mesh)?;
    let tail = read_static_mesh_tail(m.cache(), m.package(), m.index())?;
    if matches!(tail.props.get("ComplexCollisionMesh"), Some(mvs_uasset::value::Value::Ref(Some(_)))) {
        return fail(format!("{name}: {mesh} sets ComplexCollisionMesh (not ported)"));
    }
    let Some(rd) = &tail.render_data else { return fail(format!("{name}: {mesh} holds no cooked render data")) };
    let for_collision = match tail.props.get("LODForCollision") {
        Some(mvs_uasset::value::Value::Int(i)) => usize::try_from(*i).unwrap_or(0),
        _ => 0,
    };
    let Some(lod) = rd.lods.get(for_collision.min(rd.lods.len().saturating_sub(1))) else {
        return fail(format!("{name}: {mesh} has no LOD"));
    };
    let (Some(pos), Some(ix)) = (&lod.position, &lod.index) else {
        return fail(format!("{name}: {mesh}'s collision LOD has no position or index buffer"));
    };
    let stride = usize::try_from(pos.stride).unwrap_or(12);
    let f32_at = |o: usize| f64::from(f32::from_le_bytes(pos.data[o..o + 4].try_into().expect("4 bytes")));
    let vertex = |k: usize| -> Vec3 { [f32_at(k * stride), f32_at(k * stride + 4), f32_at(k * stride + 8)] };
    let index = |k: usize| -> usize {
        if ix.is32 {
            u32::from_le_bytes(ix.data[k * 4..k * 4 + 4].try_into().expect("4 bytes")) as usize
        } else {
            usize::from(u16::from_le_bytes(ix.data[k * 2..k * 2 + 2].try_into().expect("2 bytes")))
        }
    };
    let near = |a: Vec3, b: Vec3| (0..3).map(|i| (a[i] - b[i]) * (a[i] - b[i])).sum::<f64>() < 0.1 * 0.1;
    for sec in lod.sections.iter().filter(|s| s.enable_collision) {
        let first = usize::try_from(sec.first_index).unwrap_or(0);
        for t in 0..usize::try_from(sec.num_triangles).unwrap_or(0) {
            let l = [0, 1, 2].map(|j| vertex(index(first + 3 * t + j)));
            if near(l[0], l[1]) || near(l[1], l[2]) || near(l[0], l[2]) {
                continue;
            }
            let p = l.map(|v| tf_apply(tf, v));
            if let Some((a, b)) = cut(&p) {
                out.push(ComplexSegment { name: name.to_owned(), a, b, flags });
            }
        }
    }
    Ok(())
}

/// Where triangle `p` crosses the plane X = 0, as (Y, Z) end points; none when it does not, touches it at one point
/// or lies in it.
fn cut(p: &[Vec3; 3]) -> Option<([f64; 2], [f64; 2])> {
    let mut pts: Vec<[f64; 2]> = Vec::with_capacity(3);
    for i in 0..3 {
        let (a, b) = (p[i], p[(i + 1) % 3]);
        if a[0] == 0.0 {
            pts.push([a[1], a[2]]);
        }
        if (a[0] < 0.0 && b[0] > 0.0) || (a[0] > 0.0 && b[0] < 0.0) {
            let t = a[0] / (a[0] - b[0]);
            pts.push([a[1] + (b[1] - a[1]) * t, a[2] + (b[2] - a[2]) * t]);
        }
    }
    if p.iter().all(|v| v[0] == 0.0) {
        return None;
    }
    pts.dedup();
    let (a, b) = (*pts.first()?, *pts.last()?);
    (a != b).then_some((a, b))
}
