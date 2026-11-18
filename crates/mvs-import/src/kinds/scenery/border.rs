//! The blast box's drawn edge (the scenery entry's `blastBorder`): the two StaticMeshComponents of the gameplay level's
//! BlastBoxPlayerExit_C.
//!
//! SM_LevelBorder01 sits under CollisionComp and SM_LevelBorder02 under it with no transform of its own. Each draws
//! with the dynamic instance the construction script made (SetSpaceBorderValues; cooked in the level as
//! MaterialInstanceDynamic_0): its parent in the `local` context with the instance's scalars (`scalars`: the graph's
//! values, checked against the cooked instance). AdjustJumpOutCollision (BeginPlay and every shrink update) sizes and
//! places SM_LevelBorder01 from CollisionComponent's bounds: `factor`, `inset` and `loc` are that graph's numbers
//! (world half extent on Y / Z = factor x (box half extent - inset), at loc + (0, 0, box origin Z)), `bounds` the
//! mesh's own half extent on Y / Z (GetComponentBounds of the component: the mesh's ExtendedBounds). The renderer
//! does the sum each time the box changes.

use std::cmp::Ordering;
use std::sync::Arc;

use indexmap::IndexMap;

use super::bind::main_path;
use super::materials::SceneryMaterials;
use super::part::JsonProps;
use super::parts::util::{class_of, defaults_of, json_truthy, number, over_props, props_of};
use crate::json::Json;
use crate::kind::{Cx, KindError};
use crate::kinds::mesh::MeshHeader;
use crate::objview::short_name;
use crate::stage_levels::StageLevel;

const BLAST_BOX_CLASS: &str = "/Game/Panda_Main/GameMode/BlastBoxPlayerExit.BlastBoxPlayerExit_C";

// BlastBoxPlayerExit_C's numbers. AdjustJumpOutCollision's border part, ExecuteUbergraph @5235..6162:
// SM_LevelBorder01's world half extent on Y / Z = factor x (box half extent - inset), at loc + (0, 0, box origin Z):
// @5327 Y x 3.875, @5415 (CornerTop + CornerBottom) / 2.0 off Z, @5503 x 3.875, @6073 box Z - 15.0, @6115
// MakeVector(-200.0, 0.0, that).
const BORDER_FACTOR: [f64; 2] = [3.875, 3.875];
const BORDER_HALF: f64 = 2.0;
const BORDER_DOWN: f64 = 15.0;
const BORDER_AT: [f64; 2] = [-200.0, 0.0];
// SetSpaceBorderValues @10..983: per border mesh, its material parameter <- the actor's variable.
const BORDER_SCALARS: [(&str, &[(&str, &str)]); 2] = [
    (
        "SM_LevelBorder01",
        &[
            ("Reveal", "Reveal"),
            ("Dissolve Amount", "Dissolve"),
            ("FringeScaleU", "FringeScaleU"),
            ("FringeScaleV", "FringeScaleV"),
            ("FringeScale", "FringeScale"),
            ("Box_Scale", "Box_Scale"),
            ("Box_ScaleU", "BoxScaleU"),
            ("Box_ScaleV", "BoxScaleV"),
            ("Box_Power", "BoxPower"),
            ("Box_OffsetU", "Box_OffsetU"),
            ("Box_OffsetV", "Box_OffsetV"),
            ("FringeGlow_Mult", "FringeGlow_Mult"),
        ],
    ),
    (
        "SM_LevelBorder02",
        &[
            ("StarRevealU", "StarRevealU"),
            ("StarRevealV", "StarRevealV"),
            ("StarFalloff", "StarFalloff"),
            ("StarScaleU", "StarScaleU"),
            ("StarScaleV", "StarScaleV"),
            ("Box_OffsetU", "StarOffsetU"),
            ("Box_OffsetV", "StarOffsetV"),
        ],
    ),
];

fn fail(msg: String) -> KindError {
    KindError::Failed(msg)
}

/// A member set, as `??` takes it (not null, not absent).
fn set<'p>(p: &'p JsonProps, k: &str) -> Option<&'p Json> {
    p.get(k).filter(|v| !matches!(v, Json::Null))
}

/// The gameplay level's blast border.
pub fn blast_border_of<'a>(
    cx: &Cx<'a>,
    lv: &StageLevel<'a>,
    mats: &SceneryMaterials<'_, 'a>,
    add_mesh: &dyn Fn(&str) -> Result<String, KindError>,
    mesh_header: &dyn Fn(&str) -> Result<Arc<MeshHeader>, KindError>,
) -> Result<Option<Json>, KindError> {
    let lvl = lv.level.as_str();
    let boxes: Vec<_> = lv.actors.iter().map(|&i| &lv.objects[i]).filter(|a| a.class() == BLAST_BOX_CLASS).collect();
    if boxes.len() != 1 {
        return Err(fail(format!("{lvl}: {} blast boxes", boxes.len())));
    }
    let bx = boxes[0];
    let box_props = props_of(bx);
    let dot = BLAST_BOX_CLASS.rfind('.').unwrap_or(0);
    let cdo_obj = cx.object(&format!("{}.Default__{}", &BLAST_BOX_CLASS[..dot], &BLAST_BOX_CLASS[dot + 1..]))?;
    let mut cdo = defaults_of(&cdo_obj)?.unwrap_or_default();
    for (k, v) in props_of(&cdo_obj) {
        cdo.insert(k, v);
    }
    // (a float variable neither the instance nor the class serialises is 0)
    let value = |n: &str| set(&box_props, n).or_else(|| set(&cdo, n)).map_or(0.0, |v| number(Some(v)));
    let comp = |n: &str| lv.objects.iter().find(|o| o.path() == format!("{}.{n}", bx.path()));
    let Some(root) = comp("CollisionComp") else { return Err(fail(format!("{lvl}: blast box with no CollisionComp"))) };
    if props_of(root).get("RelativeRotation").is_some_and(json_truthy) {
        return Err(fail(format!("{lvl}: rotated blast box")));
    }
    let mut parts = Vec::new();
    let mut bounds: Vec<f64> = Vec::new();
    for (nm, parent_name) in [("SM_LevelBorder01", "CollisionComp"), ("SM_LevelBorder02", "SM_LevelBorder01")] {
        let Some(e) = comp(nm).filter(|e| class_of(e) == "StaticMeshComponent") else {
            return Err(fail(format!("{lvl}: no {nm}")));
        };
        let tpl = props_of(&cx.object(&format!("{BLAST_BOX_CLASS}:{nm}_GEN_VARIABLE"))?);
        let p = over_props(&tpl, Some(&props_of(e)));
        let over: &[Json] = p.get("OverrideMaterials").and_then(Json::as_array).unwrap_or(&[]);
        let t = |k: &str| p.get(k).is_some_and(json_truthy);
        if t("RelativeLocation")
            || t("RelativeRotation")
            || t("RelativeScale3D")
            || matches!(p.get("bVisible"), Some(Json::Bool(false)))
            || t("bHiddenInGame")
            || box_props.get("bHidden").is_some_and(json_truthy)
            || p.get("AttachParent").and_then(Json::as_str).is_none_or(|a| short_name(a) != parent_name)
            || over.len() != 1
        {
            return Err(fail(format!("{lvl}: {nm}")));
        }
        let Some(sm) = p.get("StaticMesh").and_then(Json::as_str) else {
            return Err(fail(format!("{lvl}: {nm}: no mesh")));
        };
        let mesh = add_mesh(sm)?;
        let h = mesh_header(sm)?;
        if h.lods.first().map(|l| l.sections.len()) != Some(1) {
            return Err(fail(format!("{nm}: mesh sections")));
        }
        // The component's material: the dynamic instance SetSpaceBorderValues made of the template's, with the values
        // that function sets (a variable of the actor per parameter).
        let mid = over[0].as_str().and_then(|o| lv.objects.iter().find(|x| x.path() == o));
        let e_path = e.path();
        let Some(mid) = mid.filter(|m| {
            class_of(m) == "MaterialInstanceDynamic" && m.path().rfind('.').is_some_and(|d| &m.path()[..d] == e_path)
        }) else {
            return Err(fail(format!("{nm}: dynamic instance {:?}", over[0])));
        };
        let mp = props_of(mid);
        let template =
            tpl.get("OverrideMaterials").and_then(Json::as_array).and_then(|a| a.first()).and_then(Json::as_str);
        let nonempty = |k: &str| mp.get(k).and_then(Json::as_array).is_some_and(|a| !a.is_empty());
        let parent = mp.get("Parent").and_then(Json::as_str);
        let (Some(parent), Some(template)) = (parent, template) else {
            return Err(fail(format!("{nm}: dynamic instance")));
        };
        if main_path(parent) != main_path(template)
            || nonempty("VectorParameterValues")
            || nonempty("TextureParameterValues")
        {
            return Err(fail(format!("{nm}: dynamic instance")));
        }
        let table = BORDER_SCALARS.iter().find(|(m, _)| *m == nm).map_or(&[][..], |(_, t)| *t);
        let scalars: IndexMap<String, f64> =
            table.iter().map(|(param, var)| ((*param).to_owned(), value(var))).collect();
        let mut cooked: IndexMap<String, f64> = IndexMap::new();
        for x in mp.get("ScalarParameterValues").and_then(Json::as_array).unwrap_or(&[]) {
            let n = x.get("ParameterInfo").and_then(|i| i.get("Name")).and_then(Json::as_str).unwrap_or_default();
            cooked.insert(n.to_owned(), number(x.get("ParameterValue")));
        }
        if scalars.is_empty()
            || scalars.len() != cooked.len()
            || scalars.iter().any(|(n, v)| {
                (cooked.get(n).copied().unwrap_or(f64::NAN) - v).abs().partial_cmp(&1e-6) != Some(Ordering::Less)
            })
        {
            return Err(fail(format!("{nm}: the cooked instance and SetSpaceBorderValues disagree")));
        }
        let material = mats.local(&main_path(parent))?;
        // (the render data's bounds: the mesh's ExtendedBounds are the same without a bounds extension)
        let smp = props_of(&cx.object(&main_path(sm))?);
        let b = &h.bounds;
        if smp.get("PositiveBoundsExtension").is_some_and(json_truthy)
            || smp.get("NegativeBoundsExtension").is_some_and(json_truthy)
            || b.origin[1].abs() > 1e-4
            || b.origin[2].abs() > 1e-4
        {
            return Err(fail(format!("{nm}: mesh bounds")));
        }
        if bounds.is_empty() {
            bounds = vec![b.extent[1], b.extent[2]];
        }
        let mut part = JsonProps::new();
        part.insert("name".into(), nm.into());
        part.insert("mesh".into(), mesh.into());
        part.insert("material".into(), material.into());
        if let Some(s) = p.get("TranslucencySortPriority").filter(|v| json_truthy(v)) {
            part.insert("sort".into(), s.clone());
        }
        part.insert("scalars".into(), Json::Obj(scalars.into_iter().map(|(k, v)| (k, Json::Num(v))).collect()));
        parts.push(Json::Obj(part));
    }
    let nums = |v: &[f64]| Json::Arr(v.iter().map(|x| Json::Num(*x)).collect());
    Ok(Some(Json::object([
        ("factor", nums(&BORDER_FACTOR)),
        ("inset", nums(&[0.0, (value("BlastBoxCornerTopBuffer") + value("BlastBoxCornerBottomBuffer")) / BORDER_HALF])),
        ("loc", nums(&[BORDER_AT[0], BORDER_AT[1], -BORDER_DOWN])),
        ("bounds", nums(&bounds)),
        ("parts", Json::Arr(parts)),
    ])))
}
