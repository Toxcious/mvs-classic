//! A ParticleSystem's entry, bound once per run: the system entries and
//! the index both read it.
//!
//! The form: an object reference is its build object path, a reference to an object of the system's own package the
//! object's name; the Required module's Material is a DrawRef (the emitter's context: sprite, mesh or trail); an
//! emitter's mesh is `<mesh path>` or `<mesh path>#<section>`; a mesh emitter's own material is its sections' slot
//! material (the mesh kind's slots). An emitter whose material the material kind cannot build is kept here: the
//! effect player leaves it out when the material kind has no entry for it.

use std::sync::Arc;

use indexmap::{IndexMap, IndexSet};
use mvs_material::draw::{DrawContextRow, context, parse_draw_ref};
use serde::Serialize;

use super::PARTICLES_FORMAT_VERSION;
use super::owners::listed;
use crate::game::{package_of, system_name};
use crate::json::{Json, props_json};
use crate::kind::{Cx, KindError, Memo};
use crate::kinds::banner::stencil::{BannerProbes, Plain, banner_ref, draw_ref};
use crate::kinds::mesh;
use crate::objview::ObjView;

const ENGINE: &str = "/Script/Engine.";

/// A system entry.
#[derive(Clone, Debug, Serialize)]
pub struct SystemValue {
    /// `system`.
    pub kind: &'static str,
    /// [`super::PARTICLES_FORMAT_VERSION`].
    pub version: u32,
    /// The ParticleSystem's build object path (`/Game/...PS_X.PS_X`).
    pub path: String,
    /// The DrawRefs its emitters draw with (sprite, mesh, trail contexts), once each: what the material kind's list
    /// reads and what the effects player loads with the system.
    pub materials: Vec<String>,
    /// The meshes its mesh emitters draw (the `mesh` kind).
    pub meshes: Vec<String>,
    /// `ParticleSystemData`: the emitters with their modules, and the system's own fields (open shape).
    pub system: Json,
}

/// A sprite emitter's context for its material.
pub type SpriteContext<'o> = &'o dyn Fn(&str) -> Result<&'static DrawContextRow, KindError>;
/// Whether an emitter of this material is left out.
pub type LeaveOut<'o> = &'o dyn Fn(&str) -> Result<bool, KindError>;

/// What a banner's system binds differently (`BindSystemOpts`); the default for every other system.
#[derive(Default)]
pub struct BindSystemOpts<'o> {
    /// A sprite emitter's context for its material (`sprite` without it): a banner's system names its materials in
    /// the banners' context where they read the custom stencil (`banner::stencil::banner_ref`).
    pub sprite_context: Option<SpriteContext<'o>>,
    /// An emitter whose material this says true of is left out (a banner's system: a material not in the paks).
    pub leave_out: Option<LeaveOut<'o>>,
}

type Props = IndexMap<String, Json>;

fn failed(what: String) -> KindError {
    KindError::Failed(what)
}

/// An object's properties as the object kind's JSON holds them.
fn props_of(cx: &Cx<'_>, o: &ObjView<'_>) -> Props {
    props_json(cx.cache, o.props())
}

/// JavaScript's truth of a JSON value (`undefined` is `None`).
fn truthy(v: Option<&Json>) -> bool {
    match v {
        None | Some(Json::Null) => false,
        Some(Json::Bool(b)) => *b,
        Some(Json::Num(n)) => *n != 0.0 && !n.is_nan(),
        Some(Json::Str(s)) => !s.is_empty(),
        Some(Json::Arr(_) | Json::Obj(_)) => true,
    }
}

/// `v ?? default` for a string.
fn string_or<'a>(v: Option<&'a Json>, default: &'a str) -> &'a str {
    match v {
        Some(Json::Str(s)) => s,
        _ => default,
    }
}

/// The last name of a path (`/Game/A/B.B:Sub.Inner` -> `Inner`).
fn last_name(p: &str) -> &str {
    let at = [p.rfind('.'), p.rfind(':')].into_iter().flatten().max();
    at.map_or(p, |i| &p[i + 1..])
}

/// Whether a property's string is an object reference (the object kind writes references as paths): `/Name/...`.
fn is_ref(v: &str) -> bool {
    let Some(rest) = v.strip_prefix('/') else { return false };
    let Some(slash) = rest.find('/') else { return false };
    slash > 0 && rest[..slash].bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_')
}

/// A baked lookup table (`table`).
fn table(t: &Props) -> Json {
    let or_zero = |k: &str| if truthy(t.get(k)) { t[k].clone() } else { Json::Num(0.0) };
    let mut o = Json::obj();
    for (out, key) in [("op", "Op"), ("n", "EntryCount"), ("st", "EntryStride")] {
        // (an `undefined` is no key)
        if let Some(v) = t.get(key) {
            o.set(out, v.clone());
        }
    }
    o.set("sub", or_zero("SubEntryStride"));
    o.set("ts", or_zero("TimeScale"));
    o.set("tb", or_zero("TimeBias"));
    if let Some(v) = t.get("Values") {
        o.set("v", v.clone());
    }
    if truthy(t.get("LockFlag")) {
        o.set("lock", t["LockFlag"].clone());
    }
    o
}

/// A distribution object as `conv` loads it: its class and properties.
struct Dist {
    class: String,
    props: Props,
}

fn locked_axes(name: &str) -> Option<u8> {
    Some(match name {
        "EDVLF_None" => 0,
        "EDVLF_XY" => 1,
        "EDVLF_XZ" => 2,
        "EDVLF_YZ" => 3,
        "EDVLF_XYZ" => 4,
        _ => return None,
    })
}

fn interp_mode(name: &str) -> Option<f64> {
    Some(match name {
        "CIM_Linear" => 0.0,
        "CIM_CurveAuto" | "CIM_CurveUser" | "CIM_CurveBreak" | "CIM_CurveAutoClamped" => 1.0,
        "CIM_Constant" => 2.0,
        _ => return None,
    })
}

/// A distribution whose lookup table has no entries (FRawDistributionVector::GetValue 142a35f20 uses the table only
/// when EntryCount and Values.Num are not 0; otherwise the Distribution object's own GetValue).
/// DistributionVectorConstant (142a361c0): a one entry table under LockedAxes.
/// DistributionVectorConstantCurve (142a36250): `curve`, 11 numbers a point (InVal, OutVal xyz, ArriveTangent xyz,
/// LeaveTangent xyz, mode 0 linear / 2 constant / 1 cubic, as FInterpCurve<FVector>::Eval 140e69c70 tells them
/// apart). DistributionVectorUniform (Unverified: the stock 4.26 GetValue, written as the one entry table a bake would
/// give, op 3 extremes / op 2 random; MirrorFlags throw). DistributionFloatConstant (Unverified: the object's
/// Constant). Any other object throws.
fn unbaked(d: Option<&Dist>, system: &str, v: &Json) -> Result<Json, KindError> {
    let fail = |why: &str| -> KindError {
        let shown = match d {
            Some(d) => Json::Obj(d.props.clone()),
            None => v.clone(),
        };
        let mut text =
            crate::json::to_vec(&shown).map(|b| String::from_utf8_lossy(&b).into_owned()).unwrap_or_default();
        text.truncate(text.char_indices().nth(300).map_or(text.len(), |(i, _)| i));
        failed(format!("particles {system}: unbaked distribution, {why}: {text}"))
    };
    let Some(d) = d else { return Err(fail("no Distribution object")) };
    let p = &d.props;
    let lock_name = string_or(p.get("LockedAxes"), "EDVLF_None").rsplit("::").next().unwrap_or_default();
    let Some(lock) = locked_axes(lock_name) else { return Err(fail(&format!("LockedAxes {lock_name}"))) };
    let xyz = |c: Option<&Json>| -> Vec<Json> {
        ["X", "Y", "Z"]
            .map(|k| match c.and_then(|c| c.get(k)) {
                None | Some(Json::Null) => Json::Num(0.0),
                Some(v) => v.clone(),
            })
            .to_vec()
    };
    let locked = |mut c: Vec<Json>| -> Vec<Json> {
        if lock == 1 || lock == 4 {
            c[1] = c[0].clone();
        }
        if lock == 2 || lock == 4 {
            c[2] = c[0].clone();
        }
        if lock == 3 {
            c[2] = c[1].clone();
        }
        c
    };
    let num = |n: f64| Json::Num(n);
    let class = d.class.replacen(ENGINE, "", 1);
    match class.as_str() {
        "DistributionVectorConstant" => {
            let t = Json::object([
                ("op", num(1.0)),
                ("n", num(1.0)),
                ("st", num(3.0)),
                ("sub", num(0.0)),
                ("ts", num(0.0)),
                ("tb", num(0.0)),
                ("v", Json::Arr(locked(xyz(p.get("Constant"))))),
            ]);
            Ok(Json::object([("table", t)]))
        }
        "DistributionVectorConstantCurve" => {
            let cc = p.get("ConstantCurve").filter(|c| !matches!(c, Json::Null));
            if truthy(cc.and_then(|c| c.get("bIsLooped"))) {
                return Err(fail("a looped curve"));
            }
            let mut out: Vec<Json> = Vec::new();
            let points = cc.and_then(|c| c.get("Points")).and_then(Json::as_array).unwrap_or_default();
            for pt in points {
                let mode_name = string_or(pt.get("InterpMode"), "CIM_Linear").rsplit("::").next().unwrap_or_default();
                let Some(mode) = interp_mode(mode_name) else { return Err(fail(&format!("InterpMode {mode_name}"))) };
                out.push(match pt.get("InVal") {
                    None | Some(Json::Null) => num(0.0),
                    Some(v) => v.clone(),
                });
                out.extend(xyz(pt.get("OutVal")));
                out.extend(xyz(pt.get("ArriveTangent")));
                out.extend(xyz(pt.get("LeaveTangent")));
                out.push(num(mode));
            }
            let mut o = Json::object([("v", Json::Arr(out))]);
            if lock != 0 {
                o.set("lock", num(f64::from(lock)));
            }
            Ok(Json::object([("curve", o)]))
        }
        "DistributionVectorUniform" => {
            if truthy(p.get("MirrorFlags")) {
                return Err(fail("MirrorFlags"));
            }
            let extremes = truthy(p.get("bUseExtremes"));
            let mut v = locked(xyz(p.get("Min")));
            v.extend(locked(xyz(p.get("Max"))));
            let mut t = Json::object([
                ("op", num(if extremes { 3.0 } else { 2.0 })),
                ("n", num(1.0)),
                ("st", num(6.0)),
                ("sub", num(3.0)),
                ("ts", num(0.0)),
                ("tb", num(0.0)),
                ("v", Json::Arr(v)),
            ]);
            if lock != 0 && !extremes {
                t.set("lock", num(f64::from(lock)));
            }
            Ok(Json::object([("table", t)]))
        }
        "DistributionFloatConstant" => {
            let c = match p.get("Constant") {
                None | Some(Json::Null) => num(0.0),
                Some(v) => v.clone(),
            };
            let t = Json::object([
                ("op", num(1.0)),
                ("n", num(1.0)),
                ("st", num(1.0)),
                ("sub", num(0.0)),
                ("ts", num(0.0)),
                ("tb", num(0.0)),
                ("v", Json::Arr(vec![c])),
            ]);
            Ok(Json::object([("table", t)]))
        }
        _ => Err(fail(&format!("type {class}"))),
    }
}

/// `conv`: a property value as the entry holds it: references to the system's own package as their last name, the
/// distributions as tables (`table`, `unbaked`, a particle parameter's set-up in `param`), `LODValidity` dropped.
fn conv(cx: &Cx<'_>, pkg: &str, system: &str, v: &Json) -> Result<Json, KindError> {
    match v {
        Json::Arr(a) => Ok(Json::Arr(a.iter().map(|x| conv(cx, pkg, system, x)).collect::<Result<_, _>>()?)),
        Json::Str(s) => {
            Ok(Json::Str(if is_ref(s) && package_of(s) == pkg { last_name(s).to_owned() } else { s.clone() }))
        }
        Json::Obj(o) => {
            if let Some(Json::Obj(t)) = o.get("Table")
                && truthy(t.get("Values"))
            {
                let mut out = Json::object([("table", table(t))]);
                let d = match o.get("Distribution") {
                    Some(Json::Str(p)) => {
                        let view = cx.object(p)?;
                        Some(Dist { class: view.class().to_owned(), props: props_of(cx, &view) })
                    }
                    _ => None,
                };
                let entries = match t.get("EntryCount") {
                    Some(Json::Num(n)) => *n,
                    Some(Json::Str(s)) => s.trim().parse().unwrap_or(f64::NAN),
                    _ => f64::NAN,
                };
                if let Some(d) = d.as_ref().filter(|d| d.class.contains("ParticleParameter")) {
                    out.set("param", Json::Obj(d.props.clone()));
                } else if entries.is_nan() || entries <= 0.0 {
                    return unbaked(d.as_ref(), system, v);
                }
                return Ok(out);
            }
            let mut out = Json::obj();
            for (k, x) in o {
                if k != "LODValidity" && k != "Distribution" {
                    out.set(k.clone(), conv(cx, pkg, system, x)?);
                }
            }
            Ok(out)
        }
        other => Ok(other.clone()),
    }
}

/// The modules whose Spawn and Update draw nothing of their own (`SPLIT_SAFE`).
const SPLIT_SAFE: &[&str] = &[
    "Required",
    "Spawn",
    "TypeDataMesh",
    "Lifetime",
    "Size",
    "SizeMultiplyLife",
    "SizeScale",
    "Color",
    "ColorOverLife",
    "ColorScaleOverLife",
    "CameraOffset",
    "ParameterDynamic",
    "MeshRotation",
    "MeshRotationRate",
    "MeshRotationRateOverLife",
    "Location",
    "LocationDirect",
];

/// Whether a random table (op 2 / op 3) is at any depth of a value.
fn has_random_table(v: &Json) -> bool {
    match v {
        Json::Obj(o) => {
            let hit = matches!(o.get("table"), Some(Json::Obj(t)) if t.get("op").and_then(Json::as_f64).is_some_and(|op| op > 1.0));
            hit || o.values().any(has_random_table)
        }
        Json::Arr(a) => a.iter().any(has_random_table),
        _ => false,
    }
}

/// Whether an emitter's particles could differ between two copies of it (a split mesh emitter): the port draws each
/// random from Math.random, so two copies agree only with no random draw: no random table (op 2
/// / op 3) at any depth, no Required delay or duration range, random UV flip or random SubUV mode, no burst with a
/// CountLow, and no module outside [`SPLIT_SAFE`].
fn split_random(mods: &[Json]) -> bool {
    for m in mods {
        let ty = string_or(m.get("type"), "");
        if !SPLIT_SAFE.contains(&ty) || has_random_table(m) {
            return true;
        }
        if ty == "Required" {
            if truthy(m.get("bEmitterDelayUseRange")) || truthy(m.get("bEmitterDurationUseRange")) {
                return true;
            }
            if string_or(m.get("UVFlippingMode"), "").contains("Random")
                || string_or(m.get("InterpolationMethod"), "").contains("Random")
            {
                return true;
            }
        }
        if ty == "Spawn" {
            let bursts = m.get("BurstList").and_then(Json::as_array).unwrap_or_default();
            // (`(b.CountLow ?? -1) > -1`: a missing CountLow is -1, which is not above -1)
            if bursts.iter().any(|b| b.get("CountLow").and_then(Json::as_f64).is_some_and(|c| c > -1.0)) {
                return true;
            }
        }
    }
    false
}

/// A mesh emitter's section slots and the slots' materials (`meshSlots`: the mesh kind's header, LOD 0).
fn mesh_slots(cx: &Cx<'_>, path: &str) -> Result<(Vec<f64>, Vec<Option<String>>), KindError> {
    let h = mesh::header(cx, path)?;
    let Some(lod) = h.lods.first() else { return Err(failed(format!("particles: mesh {path} has no LOD 0"))) };
    Ok((lod.sections.iter().map(|s| s.slot).collect(), h.slots.iter().map(|s| s.material.clone()).collect()))
}

/// A DrawRef of a module's material (`ref`).
fn module_ref(material: Option<&Json>, ctx: &DrawContextRow, what: &str) -> Result<String, KindError> {
    match material {
        Some(Json::Str(m)) => draw_ref(m, ctx),
        other => Err(failed(format!(
            "particles {what}: no material ({})",
            other.map_or("undefined".into(), |v| format!("{v:?}"))
        ))),
    }
}

fn ctx(name: &str) -> &'static DrawContextRow {
    context(name).expect("a draw context")
}

/// The module of type `ty` of an emitter's modules.
fn module_of<'m>(mods: &'m mut [Json], ty: &str) -> Option<&'m mut Json> {
    mods.iter_mut().find(|m| m.get("type").and_then(Json::as_str) == Some(ty))
}

/// What the emitters collect: the system's emitters, materials and meshes.
#[derive(Default)]
struct Emitters {
    list: Vec<Json>,
    materials: IndexSet<String>,
    meshes: IndexSet<String>,
}

impl Emitters {
    /// An emitter kept (`keep`): its two flags from the emitter's properties `em`; left out when `leave_out` says so of
    /// its material.
    fn keep(&mut self, mut e: Json, em: &Props, opts: &BindSystemOpts<'_>) -> Result<(), KindError> {
        for k in ["bUseLegacySpawningBehavior", "DetailModeBitmask"] {
            if let Some(v) = em.get(k) {
                e.set(k, v.clone());
            }
        }
        let material = match e
            .get("modules")
            .and_then(Json::as_array)
            .and_then(|m| m.iter().find(|m| m.get("type").and_then(Json::as_str) == Some("Required")))
        {
            Some(r) => r.get("Material").cloned(),
            None => return Err(failed("particles: an emitter without a Required module".into())),
        };
        let Some(Json::Str(material)) = material else {
            return Err(failed("parseDrawRef: the Required module's Material is no DrawRef".into()));
        };
        let parsed = parse_draw_ref(&material)
            .ok_or_else(|| failed(format!("parseDrawRef: {material} is not <package>.<export>?ctx=<context>")))?;
        if let Some(f) = opts.leave_out
            && f(parsed.material)?
        {
            return Ok(());
        }
        self.materials.insert(material);
        if let Some(Json::Str(mesh)) = e.get("mesh") {
            self.meshes.insert(mesh.split('#').next().unwrap_or_default().to_owned());
        }
        self.list.push(e);
        Ok(())
    }
}

/// The system at build object path `path`, bound (`bindSystem`): LOD 0 of each enabled emitter with its enabled
/// modules (the LOD's bEnabled gates the emitter, InitParticles 142bf0750 reads LODLevel +0x2c; a module's own
/// bEnabled false drops it, the Required module's flag is never read at run time); a sprite emitter with no type data
/// draws its Required material as a sprite, a ribbon or beam (TypeDataRibbon, TypeDataBeam2: `index`, its place in the
/// system's Emitters, for SetBeamSourcePoint and kin) as a trail, a mesh (TypeDataMesh) the Required material with
/// bOverrideMaterial, else the mesh's own; any other emitter or type data is left out, as are a mesh emitter whose
/// mesh the build does not hold (Unverified: a null mesh draws nothing) and a mesh of several material sections drawn
/// with its own materials whose emitter draws at random ([`split_random`]; without a random draw it is split into one
/// emitter per section, in section order, mesh `<mesh>#<section>` and the slot's material. Unverified: the build's two
/// mesh batches' order). A MeshMaterial module is an error.
pub fn bind_system(cx: &Cx<'_>, path: &str, opts: &BindSystemOpts<'_>) -> Result<SystemValue, KindError> {
    let name = system_name(path);
    let pkg = package_of(path);
    let ps = cx.object(path)?;
    if ps.class() != format!("{ENGINE}ParticleSystem") {
        return Err(failed(format!("particles {path}: a {}, not a ParticleSystem", ps.class())));
    }
    let p = props_of(cx, &ps);
    let all = match p.get("Emitters") {
        Some(Json::Arr(a)) => a.as_slice(),
        _ => &[],
    };
    let mut out = Emitters::default();
    for (at, er) in all.iter().enumerate() {
        let er = match er {
            Json::Str(s) if !s.is_empty() => s.as_str(),
            Json::Null => continue,
            Json::Str(_) => continue,
            other => return Err(failed(format!("particles {name}: emitter {at} is {other:?}"))),
        };
        let em = cx.object(er)?;
        let ep = props_of(cx, &em);
        let Some(Json::Str(lod_ref)) =
            ep.get("LODLevels").and_then(Json::as_array).and_then(<[Json]>::first).filter(|r| truthy(Some(r)))
        else {
            continue;
        };
        let lp = props_of(cx, &cx.object(lod_ref)?);
        if matches!(lp.get("bEnabled"), Some(Json::Bool(false))) {
            continue;
        }
        let mut mods: Vec<Json> = Vec::new();
        let required = lp.get("RequiredModule");
        let refs = [required, lp.get("SpawnModule"), lp.get("TypeDataModule")]
            .into_iter()
            .chain(lp.get("Modules").and_then(Json::as_array).unwrap_or_default().iter().map(Some));
        for r in refs {
            let Some(Json::Str(r)) = r else { continue };
            let m = cx.object(r)?;
            let mp = props_of(cx, &m);
            if Some(&Json::Str(r.clone())) != required && matches!(mp.get("bEnabled"), Some(Json::Bool(false))) {
                continue;
            }
            let mut module =
                Json::object([("type", Json::Str(m.class().replacen(&format!("{ENGINE}ParticleModule"), "", 1)))]);
            let Json::Obj(body) = conv(cx, pkg, name, &Json::Obj(mp))? else {
                unreachable!("an object converts to one")
            };
            for (k, v) in body {
                module.set(k, v);
            }
            mods.push(module);
        }
        let ename = match ep.get("EmitterName") {
            Some(Json::Str(s)) if !s.is_empty() => s.clone(),
            _ => last_name(er).to_owned(),
        };
        let ty_of = |m: &Json| string_or(m.get("type"), "").to_owned();
        let td = mods.iter().position(|m| ty_of(m).starts_with("TypeData"));
        let td_type = td.map(|i| ty_of(&mods[i]));
        if em.class() != format!("{ENGINE}ParticleSpriteEmitter")
            || td_type.as_deref().is_some_and(|t| !matches!(t, "TypeDataMesh" | "TypeDataRibbon" | "TypeDataBeam2"))
        {
            continue;
        }
        if mods.iter().any(|m| ty_of(m) == "MeshMaterial") {
            return Err(failed(format!("particles {name} {ename}: MeshMaterial module")));
        }
        if !mods.iter().any(|m| ty_of(m) == "Required") {
            return Err(failed(format!("particles {name} {ename}: no Required module")));
        }
        let what = format!("{name} {ename}");
        if td_type.as_deref() == Some("TypeDataMesh") {
            let td = &mods[td.expect("a type data module")];
            let mesh = match td.get("Mesh") {
                Some(Json::Str(m)) if cx.exists(m) => m.clone(),
                _ => continue,
            };
            let override_material = truthy(td.get("bOverrideMaterial"));
            if !override_material {
                let (sections, slot_mats) = mesh_slots(cx, &mesh)?;
                let slot_material = |slot: f64| -> Option<Json> {
                    // (an index past the slots, or an empty slot, is `undefined` / null: no material)
                    let i = usize::try_from(slot as i64).ok()?;
                    Some(slot_mats.get(i)?.clone().map_or(Json::Null, Json::Str))
                };
                let distinct: IndexSet<u64> = sections.iter().map(|s| s.to_bits()).collect();
                if distinct.len() > 1 {
                    if split_random(&mods) {
                        continue;
                    }
                    for (i, slot) in sections.iter().enumerate() {
                        let mut copy = mods.clone();
                        let material = slot_material(*slot);
                        let r = module_ref(material.as_ref(), ctx("mesh"), &format!("{what}: {mesh} section {i}"))?;
                        module_of(&mut copy, "Required").expect("a Required module").set("Material", Json::Str(r));
                        let e = Json::object([
                            ("name", Json::Str(ename.clone())),
                            ("modules", Json::Arr(copy)),
                            ("mesh", Json::Str(format!("{mesh}#{i}"))),
                        ]);
                        out.keep(e, &ep, opts)?;
                    }
                    continue;
                }
                let mut own: IndexSet<Option<String>> = IndexSet::new();
                for s in &sections {
                    own.insert(match slot_material(*s) {
                        Some(Json::Str(m)) => Some(m),
                        _ => None,
                    });
                }
                if own.len() != 1 {
                    let names: Vec<String> =
                        own.iter().map(|m| m.clone().unwrap_or_else(|| "undefined".into())).collect();
                    return Err(failed(format!("particles {what}: {mesh} draws with {}", names.join(", "))));
                }
                let material = own[0].clone().map_or(Json::Null, Json::Str);
                let r = module_ref(Some(&material), ctx("mesh"), &what)?;
                module_of(&mut mods, "Required").expect("a Required module").set("Material", Json::Str(r));
            } else {
                let material = module_of(&mut mods, "Required").expect("a Required module").get("Material").cloned();
                let r = module_ref(material.as_ref(), ctx("mesh"), &what)?;
                module_of(&mut mods, "Required").expect("a Required module").set("Material", Json::Str(r));
            }
            let e = Json::object([
                ("name", Json::Str(ename.clone())),
                ("modules", Json::Arr(mods)),
                ("mesh", Json::Str(mesh)),
            ]);
            out.keep(e, &ep, opts)?;
            continue;
        }
        let material = module_of(&mut mods, "Required").expect("a Required module").get("Material").cloned();
        let r = if td_type.is_some() {
            module_ref(material.as_ref(), ctx("trail"), &what)?
        } else {
            let c = match (&opts.sprite_context, &material) {
                (Some(f), Some(Json::Str(m))) => f(m)?,
                _ => ctx("sprite"),
            };
            module_ref(material.as_ref(), c, &what)?
        };
        module_of(&mut mods, "Required").expect("a Required module").set("Material", Json::Str(r));
        let mut e = Json::object([("name", Json::Str(ename.clone())), ("modules", Json::Arr(mods))]);
        if td_type.as_deref() == Some("TypeDataBeam2") {
            e.set("index", Json::Num(at as f64));
        }
        out.keep(e, &ep, opts)?;
    }
    let mut system = Json::object([("emitters", Json::Arr(out.list))]);
    for k in ["Delay", "DelayLow", "bUseDelayRange", "WarmupTime", "SystemUpdateMode", "bOrientZAxisTowardCamera"] {
        if let Some(v) = p.get(k) {
            system.set(k, v.clone());
        }
    }
    // The component bounds (UParticleSystemComponent::CalcBounds 142be3ba0): the fixed relative box when set.
    if truthy(p.get("bUseFixedRelativeBoundingBox")) {
        let b = p.get("FixedRelativeBoundingBox").filter(|b| matches!(b, Json::Obj(_))).ok_or_else(|| {
            failed(format!("particles {path}: bUseFixedRelativeBoundingBox without a FixedRelativeBoundingBox"))
        })?;
        let corner = |c: &str| -> Result<Vec<Json>, KindError> {
            let m = b
                .get(c)
                .filter(|m| matches!(m, Json::Obj(_)))
                .ok_or_else(|| failed(format!("particles {path}: FixedRelativeBoundingBox has no {c}")))?;
            // (an `undefined` member of the array is written `null`)
            Ok(["X", "Y", "Z"].map(|k| m.get(k).cloned().unwrap_or(Json::Null)).to_vec())
        };
        let mut v = corner("Min")?;
        v.extend(corner("Max")?);
        system.set("fixedBox", Json::Arr(v));
    }
    Ok(SystemValue {
        kind: "system",
        version: PARTICLES_FORMAT_VERSION,
        path: path.to_owned(),
        materials: out.materials.into_iter().collect(),
        meshes: out.meshes.into_iter().collect(),
        system,
    })
}

/// The run's bound systems.
#[derive(Default)]
struct Systems(Memo<Result<Arc<SystemValue>, KindError>>);

/// The run's banner contexts' verdicts (`probes`).
#[derive(Default)]
struct Probes(BannerProbes);

/// The system at `path`, bound once per run with the options its namers give it (a banner's system the banner
/// context's), whichever item asks first.
pub fn system(cx: &Cx<'_>, path: &str) -> Result<Arc<SystemValue>, KindError> {
    let memo = cx.shared.get_or_init(Systems::default);
    memo.0.get(path, || bind(cx, path).map(Arc::new))
}

/// One bind of `path` with its namers' options: a system the banner kind names takes its sprite emitters' context
/// from the banner's choice (`bannerRef`) and leaves out an emitter whose material the paks do not hold.
fn bind(cx: &Cx<'_>, path: &str) -> Result<SystemValue, KindError> {
    if !listed(cx)?.banner.contains(path) {
        return bind_system(cx, path, &BindSystemOpts::default());
    }
    let probes = cx.shared.get_or_init(Probes::default);
    let sprite = |m: &str| -> Result<&'static DrawContextRow, KindError> {
        let (r, _) = banner_ref(cx, &probes.0, m, Plain::Sprite)?;
        Ok(parse_draw_ref(&r).ok_or_else(|| failed(format!("parseDrawRef: {r}")))?.context)
    };
    let leave_out = |m: &str| -> Result<bool, KindError> { Ok(!cx.exists(m)) };
    bind_system(cx, path, &BindSystemOpts { sprite_context: Some(&sprite), leave_out: Some(&leave_out) })
}
