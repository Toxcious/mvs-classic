//! The stage data's pass-through platforms (`StageData.platforms`): BasePlatformBP_C and its children (Scene (root) >
//! Parent > Box, profile Platform), their Box's world box, a moving one's StartPosition and PlatformCurves, the drawn
//! StaticMesh.

use mvs_data::Value;
use mvs_data::stage::{Platform, PlatformCurve, PlatformMesh, PlatformMoving};

use super::super::super::shape::{fail, short_name};
use super::super::super::transform::identity_rot;
use super::super::context::{StageCtx, ar, js_truthy, rot, s, st, vec, vec0};
use super::solids::collision;
use crate::kind::KindError;
use crate::objview::ObjView;

/// A build object path as a content key: its package less `/Game/` (a `MultiVersus/Content/` ObjectPath) or `/Engine/`
/// as `Engine/Content/`.
pub fn asset_key(path: &str) -> Result<String, KindError> {
    let pkg = &path[..path.find('.').unwrap_or(path.len())];
    if let Some(r) = pkg.strip_prefix("/Game/") {
        return Ok(r.to_owned());
    }
    if let Some(r) = pkg.strip_prefix("/Engine/") {
        return Ok(format!("Engine/Content/{r}"));
    }
    fail(format!("stage data: no asset key for {path}"))
}

/// A pass-through platform actor: BasePlatformBP_C or a child of it.
pub fn is_platform(ctx: &StageCtx<'_>, a: &ObjView<'_>) -> Result<bool, KindError> {
    Ok(ctx.class_chain(a)?.iter().any(|c| c == "BasePlatformBP_C"))
}

/// BasePlatformBP's PlatformCurves entry (its member names carry a GUID tail): what Move Platform reads of it,
/// DistanceX / Y / Z (BasePlatformBP @939, @1190) and Duration (@878, @1348); the rest drives nothing.
fn platform_curve(c: &Value) -> PlatformCurve {
    let get = |name: &str| -> f64 {
        c.as_obj()
            .and_then(|o| o.iter().find(|(k, _)| k.split('_').next() == Some(name)))
            .and_then(|(_, v)| v.as_f64())
            .unwrap_or(0.0)
    };
    PlatformCurve { d: vec![get("DistanceX"), get("DistanceY"), get("DistanceZ")], duration: get("Duration") }
}

/// StageData.platforms' entry of a BasePlatformBP actor, or none where its Box has no collision.
pub fn platform(ctx: &StageCtx<'_>, a: &ObjView<'_>) -> Result<Option<Platform>, KindError> {
    let n = short_name(a.path()).to_owned();
    let ap = ctx.properties(a)?;
    let one = |cls: &str, name: &str| -> Result<ObjView<'_>, KindError> {
        ctx.comps(a, Some(cls))
            .into_iter()
            .find(|x| short_name(x.path()) == name)
            .ok_or_else(|| KindError::Failed(format!("platform {n}: no {cls} {name}")))
    };
    let box_c = one("BoxComponent", "Box")?;
    let bx = ctx.with_templates(&box_c)?;
    let col = collision(ctx, st(&bx, "BodyInstance"), "OverlapAllDynamic")?;
    if col.en == "NoCollision" {
        return Ok(None);
    }
    let object_type = st(&bx, "BodyInstance").and_then(|b| s(b, "ObjectType"));
    if !(col.prof == "Platform" && object_type == Some("ECC_GameTraceChannel7") && col.en == "QueryAndPhysics") {
        return fail(format!("platform {n}: not the Platform profile"));
    }
    // (a moving one: BeginPlay's Delay(2) -> Move Platform, gated on Moving at BasePlatformBP @186; the class default is
    // empty, @408 reads its length)
    let moving = js_truthy(ap.get("Moving"));
    if moving && !js_truthy(ap.get("PlatformCurves")) {
        return fail(format!("platform {n}: Moving with no PlatformCurves"));
    }
    let parent = one("SceneComponent", "Parent")?;
    let w = ctx.world(&box_c)?;
    if !identity_rot(w.q, 1e-7) {
        return fail(format!("platform {n}: a rotated platform Box"));
    }
    let loc = w.loc;
    let ps = ctx.world(&parent)?.scale;
    let ext = vec(bx.get("BoxExtent"), [32.0; 3]);
    let sv = vec(bx.get("RelativeScale3D"), [1.0; 3]);
    let he = [0, 1, 2].map(|i| (ext[i] * sv[i] * ps[i]).abs());
    let mut plat = Platform {
        name: n.clone(),
        center: loc.to_vec(),
        half_extent: he.to_vec(),
        top: loc[2] + he[2],
        y: vec![loc[1] - he[1], loc[1] + he[1]],
        z: vec![loc[2] - he[2], loc[2] + he[2]],
        moving: None,
        rise: None,
        mesh: None,
    };
    if moving {
        // StartPosition: the construction script's (Parent's location, UserConstructionScript @50)
        let Some(Value::Arr(curves)) = ap.get("PlatformCurves") else {
            return fail(format!("platform {n}: PlatformCurves is no list"));
        };
        plat.moving = Some(PlatformMoving {
            start: vec0(ap.get("StartPosition")).to_vec(),
            curves: curves.iter().map(platform_curve).collect(),
        });
    }
    // The drawn mesh: the StaticMesh component under Parent, nothing where the actor is hidden in game or the component
    // is not visible or hidden in game.
    let smc_c = one("StaticMeshComponent", "StaticMesh")?;
    let smc = ctx.with_templates(&smc_c)?;
    let static_mesh = s(&smc, "StaticMesh").filter(|m| !m.is_empty());
    if !js_truthy(ap.get("bHidden"))
        && !matches!(smc.get("bVisible"), Some(Value::Bool(false)))
        && !js_truthy(smc.get("bHiddenInGame"))
        && let Some(static_mesh) = static_mesh
    {
        let rp = ctx.world(&parent)?;
        let native_lod = crate::kinds::gamedata::shape::native(&smc_c)?.is_some_and(|nv| js_truthy(nv.get("LODData")));
        let attach = s(&smc, "AttachParent");
        if attach.is_none_or(|p| short_name(p) != "Parent") || js_truthy(smc.get("LODData")) || native_lod {
            return fail(format!("platform {n}: its mesh is not under Parent, or has LODData"));
        }
        let root_name = s(&ap, "RootComponent").map(short_name);
        let Some(root_c) =
            ctx.comps(a, Some("SceneComponent")).into_iter().find(|c| Some(short_name(c.path())) == root_name)
        else {
            return fail(format!("platform {n}: no root"));
        };
        let rw = ctx.world(&root_c)?;
        let (rl0, sc) = (rw.loc, rw.scale);
        if !(identity_rot(rp.q, 1e-7) && rp.loc == rl0 && rp.scale == sc) {
            return fail(format!("platform {n}: Parent moved off the root"));
        }
        let rl = vec0(smc.get("RelativeLocation"));
        let rs = vec(smc.get("RelativeScale3D"), [1.0; 3]);
        let empty = mvs_data::Props::new();
        let ch = st(&smc, "LightingChannels").unwrap_or(&empty);
        let chan = |k: &str, d: bool| ch.get(k).and_then(Value::as_bool).unwrap_or(d);
        let mut materials = Vec::new();
        for m in ar(&smc, "OverrideMaterials").unwrap_or(&[]) {
            match m {
                Value::Str(p) if !p.is_empty() => materials.push(asset_key(p)?),
                _ => return fail(format!("platform {n}: an empty override material (the data holds no null here)")),
            }
        }
        plat.mesh = Some(PlatformMesh {
            mesh: asset_key(static_mesh)?,
            materials,
            loc: (0..3).map(|i| rl0[i] + sc[i] * rl[i]).collect(),
            rot: rot(smc.get("RelativeRotation")).to_vec(),
            scale: (0..3).map(|i| sc[i] * rs[i]).collect(),
            cast_shadow: smc.get("CastShadow").and_then(Value::as_bool).unwrap_or(true),
            channels: vec![chan("bChannel0", true), chan("bChannel1", false), chan("bChannel2", false)],
        });
    }
    Ok(Some(plat))
}
