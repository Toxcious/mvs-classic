//! The fighter data's body part: the pawn's and BaseFighter's class defaults and
//! component templates, the anim sets and their blend spaces, the AnimBP's slot graph (`anim_graph.rs`) and the
//! skeleton's sockets, virtual bones and mirror table.
//!
//! The chain: CharacterData -> the pawn class PGI_GlobalData's CharacterDataByFighter maps to it (a data-only key: its
//! port table entry's `pawn`) -> the pawn's CharacterMesh0 AnimClass -> its TargetSkeleton and the MirrorTable of its
//! mirror node. The anim set class (`animbp`), the further sets (`anim_sets`) and the anim folders are the port
//! table's (`fighter_port.rs`).

use std::cell::{OnceCell, RefCell};
use std::collections::HashMap;

use mvs_data::fighter::{AnimAsset, PolymorphAnimSet};
use mvs_data::value::from_value;
use mvs_data::{Props, Value};

use super::Bind;
use super::anim_graph::{
    AnimBp, anim_node_props, anim_nodes, child_props, exposed_inputs, prop_name, slot_holds, state_machines, struct_of,
};
use super::bag::{arr_of, at, b_or, n_or, num, obj, or, s, s_or, some, str_of, tail, truthy};
use crate::fighter_port::{FighterPort, fighter_port};
use crate::kind::{Cx, KindError};
use crate::kinds::gamedata::shape::{
    cdo_path, deep_update, defaults_or_properties, enums_for, fail, native, properties, shape_value, short_name,
};
use crate::objview::ObjView;

// ---- constants

/// PGI_GlobalData's class default object: its CharacterDataByFighter maps each pawn class to its CharacterData (the
/// map Comp_Pawn_TauntManager @521 and AXFunctions @5407 Map_Find read).
const PGI_GLOBAL_DATA: &str = "/Game/Panda_Main/Blueprints/PandaGameInstance/PGI_GlobalData.Default__PGI_GlobalData_C";
/// BaseFighter, every pawn's parent.
pub const BASE_FIGHTER: &str = "/Game/Panda_Main/Blueprints/BaseFighter.BaseFighter_C";
/// Base_AnimBP, the parent of every base anim set class.
const BASE_ANIMBP: &str = "/Game/Panda_Main/Characters/Common/ANim/Base_AnimBP.Base_AnimBP_C";
const BLEND_SPACE_1D: &str = "/Script/Engine.BlendSpace1D";
const ANIM_COMPOSITE: &str = "/Script/Engine.AnimComposite";
const MIRROR_TABLE_CLASS: &str = "/Script/WAnimationMirroringRuntime.WMirrorTable";
const ANIM_INSTANCE: &str = "/Script/Engine.AnimInstance";

/// Native component classes: the members their constructor sets that no template of BaseFighter serialises and the
/// port reads. FighterMovementComponent: CrouchedHalfHeight 40 (UCharacterMovementComponent constructor exe
/// 1429164d0, +0x1d4; the FighterMovementComponent constructor 1410a97a0 does not write it).
const NATIVE_COMPONENT_DEFAULTS: &[(&str, &[(&str, f64)])] =
    &[("FighterMovementComponent", &[("CrouchedHalfHeight", 40.0)])];

/// A polymorph prop's AnimBP class (the class a polymorph buff's skin names) -> (that AnimBP's package, the package
/// of the class it links its base layer to by a class literal). Chicken_AnimBP ubergraph @15
/// LinkAnimClassLayers(Base_Chicken_AnimBP_C) (its AnimGraphNode_LinkedAnimLayer has no InstanceClass).
pub const POLYMORPH_ANIM_LAYERS: &[(&str, (&str, &str))] = &[(
    "Chicken_AnimBP_C",
    (
        "/Game/Panda_Main/Characters/C015/Chicken/Animations/Chicken_AnimBP",
        "/Game/Panda_Main/Characters/C015/Chicken/Animations/Base_Chicken_AnimBP",
    ),
)];

/// The Constraint nodes of an AnimBP that put a virtual bone onto a mesh bone, by AnimBP class and virtual bone: (the
/// node whose alpha is 1 facing right, the nodes of the other facing). The port draws a left facing as the reflection
/// of the right-facing view (Deviation), so the bone is taken at the right-facing node's target at both facings.
/// BugsBunny_AnimBP ("Pickup constraints"): OnFacingChanged (@0-176) sets FacingScreenLeftAlpha = float(FacingLeft) ->
/// Constraint.Alpha (VB PropPickup onto L_PropOffset) and FacingScreenLeftInverseAlpha = float(!FacingLeft) ->
/// Constraint_1.Alpha (onto R_PropOffset), after the mirror node.
/// (by AnimBP class: a virtual bone, its right-facing constraint node and the nodes it stands for)
type VbConstraint = (&'static str, (&'static str, &'static [&'static str]));

const VB_CONSTRAINT_RIGHT: &[(&str, &[VbConstraint])] =
    &[("BugsBunny_AnimBP_C", &[("VB PropPickup", ("AnimGraphNode_Constraint_1", &["AnimGraphNode_Constraint"]))])];

/// What a BlendSpace1D serialises that the sim's player has; anything else stops.
const FALL_SPACE_KEYS: &[&str] = &["SampleData", "GridSamples", "BlendParameters", "Skeleton"];

/// Base_AnimBP's Wall states (the Wall machine of the live layer, and its AnimGraph copy): each state's
/// BlendListByBool on UseWallBlendSpace and the BlendSpacePlayer of its pose 0, which plays the set's blend space at
/// X = WallBlendSpaceValue. WallJump is not read: its state is never shown (WallJump -> Air has a constant true rule).
const WALL_STATES: &[(&str, (&str, &[&str]))] = &[
    ("enter", ("BlendSpace_WallEnter", &["AnimGraphNode_BlendListByBool_21", "AnimGraphNode_BlendListByBool_5"])),
    ("idle", ("BlendSpace_WallIdle", &["AnimGraphNode_BlendListByBool_22", "AnimGraphNode_BlendListByBool_6"])),
];
/// Base_AnimBP's AirState Fall: BlendListByBool_17 (bActiveValue UseFallBlendspace) plays its pose 0, a
/// BlendSpacePlayer over FallBlendSpace, in place of Anim_Fall_01.
const FALL_BLEND_LIST: &str = "AnimGraphNode_BlendListByBool_17";

/// UBlendSpaceBase ctor 1428831f0: each axis Min 0, Max 100, GridNum 4; FBlendSample RateScale 1 (ops vtable
/// 144353a18).
const AXIS_MIN: f64 = 0.0;
const AXIS_MAX: f64 = 100.0;
const AXIS_GRID_NUM: f64 = 4.0;
const SAMPLE_RATE_DEFAULT: f64 = 1.0;

// ---- helpers

/// A Blueprint package's class (`/Game/A/B` -> `/Game/A/B.B_C`).
pub fn class_of_pkg(pkg: &str) -> String {
    format!("{pkg}.{}_C", last_seg(pkg))
}

fn last_seg(pkg: &str) -> &str {
    &pkg[pkg.rfind('/').map_or(0, |i| i + 1)..]
}

/// `path.split('.')[0]`.
fn pkg_of(path: &str) -> &str {
    path.split('.').next().unwrap_or(path)
}

fn req<'v>(o: &'v Value, k: &str, what: &str) -> Result<&'v Value, KindError> {
    at(o, k).ok_or_else(|| KindError::Failed(format!("{what}: no {k}")))
}

fn empty() -> Value {
    Value::Obj(Props::new())
}

/// A value shaped as the tables hold it, its enums found as `key`'s bind finds them.
pub fn shape(cx: &Cx<'_>, key: &str, v: &Value) -> Result<Value, KindError> {
    shape_value(v, &enums_for(cx, key, &[v])?)
}

fn shape_props(cx: &Cx<'_>, key: &str, p: Props) -> Result<Value, KindError> {
    shape(cx, key, &Value::Obj(p))
}

/// An anim set: its class default object's own properties, shaped, and the class defaults the sim reads from a set
/// that a set inherits from its Blueprint parents where it stores none (RunBaseSpeed: Base_AnimBP's 1200 for the sets
/// that leave it unset, as Marvin's, Morty's, Black Adam's and Tom and Jerry's do).
fn set_props(cx: &Cx<'_>, key: &str, cls: &str) -> Result<Value, KindError> {
    let mut p = properties(&cx.object(&cdo_path(cls))?)?;
    for name in INHERITED_SET_DEFAULTS {
        if p.contains_key(*name) {
            continue;
        }
        let mut c = str_of(native(&cx.object(cls)?)?.unwrap_or_default().get("SuperStruct")).map(str::to_owned);
        while let Some(cur) = c.filter(|c| c.starts_with("/Game/")) {
            if let Some(v) = properties(&cx.object(&cdo_path(&cur))?)?.get(*name) {
                p.insert((*name).to_owned(), v.clone());
                break;
            }
            c = str_of(native(&cx.object(&cur)?)?.unwrap_or_default().get("SuperStruct")).map(str::to_owned);
        }
    }
    shape_props(cx, key, p)
}

/// The class defaults of an anim set the sim reads from the set itself (`runPlayRate`, the dash's play rate).
const INHERITED_SET_DEFAULTS: &[&str] = &["RunBaseSpeed"];

/// The inner object of chain `chain` exactly (`o.inner[chain]`).
fn inner_exact<'a>(o: &ObjView<'a>, chain: &str) -> Result<Option<ObjView<'a>>, KindError> {
    Ok(o.inner()?.into_iter().find(|x| x.chain() == chain))
}

/// The character's anim packages and their kinds.
struct Anims {
    all: Vec<String>,
    blend_spaces: Vec<String>,
    composites: Vec<String>,
}

/// The objects of one bind and the character's anim folder listing.
struct Body<'b, 'c, 'a> {
    b: &'b Bind<'c, 'a>,
    anims: OnceCell<Anims>,
    comp_defaults: RefCell<HashMap<String, (Props, Props)>>,
}

impl Body<'_, '_, '_> {
    fn cx(&self) -> &Cx<'_> {
        self.b.cx
    }

    fn port(&self) -> &FighterPort {
        self.b.port
    }

    fn shape(&self, v: &Value) -> Result<Value, KindError> {
        shape(self.b.cx, &self.b.key, v)
    }

    /// The character's anim packages (the port table's anim_dirs, default Animations: each folder with everything
    /// under it, '.' the folder's own packages alone; anim_skip left out), in the folders' order.
    fn anims(&self) -> &Anims {
        self.anims.get_or_init(|| {
            let root = self.port().folder;
            let cat = &self.cx().catalog;
            let all = cat.list_packages(root, true, None);
            let bs = cat.list_packages(root, true, Some(BLEND_SPACE_1D));
            let comp = cat.list_packages(root, true, Some(ANIM_COMPOSITE));
            let rel = |p: &str| p.get(root.len() + 1..).unwrap_or_default().to_owned();
            let skip = self.port().anim_skip.unwrap_or_default();
            let mut out: Vec<String> = Vec::new();
            for d in self.port().anim_dirs.unwrap_or(&["Animations"]) {
                let pre = d.to_lowercase() + "/";
                out.extend(
                    all.iter()
                        .filter(
                            |p| if *d == "." { !rel(p).contains('/') } else { rel(p).to_lowercase().starts_with(&pre) },
                        )
                        .cloned(),
                );
            }
            let kept = out
                .into_iter()
                .filter(|p| !skip.iter().any(|s| rel(p) == *s || (s.ends_with('/') && rel(p).starts_with(s))))
                .collect();
            let low = |v: Vec<String>| v.into_iter().map(|p| p.to_lowercase()).collect();
            Anims { all: kept, blend_spaces: low(bs), composites: low(comp) }
        })
    }

    /// The first anim package named `name` (without regard to case), with its kind among the two the anim set reads.
    fn anim_asset(&self, name: &Value) -> Option<(String, Option<AnimAssetKind>)> {
        let name = name.as_str()?.to_lowercase();
        let a = self.anims();
        let path = a.all.iter().find(|p| last_seg(p).to_lowercase() == name)?;
        let low = path.to_lowercase();
        let kind = if a.blend_spaces.contains(&low) {
            Some(AnimAssetKind::BlendSpace)
        } else if a.composites.contains(&low) {
            Some(AnimAssetKind::Composite)
        } else {
            None
        };
        Some((path.clone(), kind))
    }

    /// A component class's defaults, its Blueprint CDO over its Blueprint parents' (the object's merged `defaults`,
    /// without the ubergraph frame), or NATIVE_COMPONENT_DEFAULTS for a native class. A native class's other members
    /// are code, not in the paks, and stay out. Shaped, and as the build stores them (`raw`: dump_form reads it); a
    /// fresh copy per call.
    fn component_class_defaults(&self, cls: &str) -> Result<(Props, Props), KindError> {
        if let Some(d) = self.comp_defaults.borrow().get(cls) {
            return Ok(d.clone());
        }
        let d = if !cls.starts_with("/Game/") {
            let mut p = Props::new();
            if let Some((_, m)) = NATIVE_COMPONENT_DEFAULTS.iter().find(|(n, _)| *n == short_name(cls)) {
                for (k, v) in m.iter() {
                    p.insert((*k).to_owned(), num(*v));
                }
            }
            (p, Props::new())
        } else {
            let cdo = self.cx().object(&cdo_path(cls))?;
            let mut d = defaults_or_properties(&cdo)?;
            d.shift_remove("UberGraphFrame");
            let p = match self.shape(&Value::Obj(d.clone()))? {
                Value::Obj(p) => p,
                _ => Props::new(),
            };
            (p, d)
        };
        self.comp_defaults.borrow_mut().insert(cls.to_owned(), d.clone());
        Ok(d)
    }
}

// ---- the chain

/// The sources a character's body is bound from: its pawn class, the AnimClass of the pawn's CharacterMesh0, that
/// class's TargetSkeleton, and the MirrorTable of its mirror node (`None` for a class with none: `mirror_node` false;
/// `mirror` then UNREAD_MIRROR_TABLES).
pub struct BodySources {
    pub pawn: String,
    pub anim_class: String,
    pub skeleton: String,
    pub mirror: Option<String>,
    pub mirror_node: bool,
}

/// The pawn class of a character: the key of PGI_GlobalData's CharacterDataByFighter whose value is the character's
/// CharacterData; a data-only key's from its port table entry.
pub fn pawn_class_of(cx: &Cx<'_>, port: &FighterPort, character_data: Option<&str>) -> Result<String, KindError> {
    if let Some(p) = port.pawn {
        return Ok(p.to_owned());
    }
    let Some(cd) = character_data else { return fail("no CharacterData and no pawn in the port table") };
    let pgi = defaults_or_properties(&cx.object(PGI_GLOBAL_DATA)?)?;
    let hits: Vec<&str> = arr_of(pgi.get("CharacterDataByFighter"))
        .iter()
        .filter_map(|e| {
            let p = e.as_arr()?;
            let data = p.get(1)?.as_str()?;
            (pkg_of(data).to_lowercase() == cd.to_lowercase()).then(|| p.first()?.as_str()).flatten()
        })
        .collect();
    if hits.len() != 1 {
        return fail(format!("PGI_GlobalData CharacterDataByFighter: {} pawn classes map to {cd}", hits.len()));
    }
    Ok(hits[0].to_owned())
}

/// The raw value of a component template member, the pawn's template over BaseFighter's (the CDO's default subobject
/// of that name).
fn template_value(
    pawn_cdo: &ObjView<'_>,
    base_cdo: &ObjView<'_>,
    comp: &str,
    member: &str,
) -> Result<Option<Value>, KindError> {
    for cdo in [pawn_cdo, base_cdo] {
        if let Some(x) = inner_exact(cdo, comp)?
            && let Some(v) = properties(&x)?.get(member)
        {
            return Ok(Some(v.clone()));
        }
    }
    Ok(None)
}

/// The WAnimationMirrorNode values of an AnimBP class's own nodes.
fn mirror_nodes(bp: &AnimBp) -> Vec<Value> {
    child_props(&bp.native)
        .into_iter()
        .filter(|c| struct_of(c).contains("MirrorNode"))
        .map(|c| or(bp.cdo.get(prop_name(c)), empty()))
        .collect()
}

/// A class without a mirror node (Iron Giant's C017_AnimBP, Velma's police car) reads no mirror table. The port still
/// keeps one for Iron Giant (`mirror` C017_MirrorTable: no asset of the build references it, and C017_Head_MirrorTable
/// has the same Skeleton, so no property chain picks it), none for the police car. Kept here, by key, for the data's
/// identity. The match's readers take `mirror_node` false first, but the podium preview strips `mirror_node` and
/// mirrors with this table: dropping it changes that preview. A port-made entry, no build fact.
const UNREAD_MIRROR_TABLES: &[(&str, &str)] = &[("irongiant", "C017_MirrorTable")];

fn mirror_table_unread(cx: &Cx<'_>, key: &str, port: &FighterPort) -> Result<Option<String>, KindError> {
    let Some((_, name)) = UNREAD_MIRROR_TABLES.iter().find(|(k, _)| *k == key) else { return Ok(None) };
    let path = format!("{}/{name}", port.folder);
    let o = cx.object(&path)?;
    if o.class() != MIRROR_TABLE_CLASS {
        return fail(format!("{path}: no WMirrorTable"));
    }
    Ok(Some(o.path().to_owned()))
}

/// The body's sources for a character key.
pub fn body_sources(cx: &Cx<'_>, key: &str) -> Result<BodySources, KindError> {
    let Some(port) = fighter_port(key) else { return fail(format!("fighter data {key}: no entry in the port table")) };
    sources(cx, key, port, super::character_data_of_key(key, port))
}

/// The body's sources of the character a bind is for.
pub fn body_sources_of(b: &Bind<'_, '_>) -> Result<BodySources, KindError> {
    sources(b.cx, &b.key, b.port, b.character_data)
}

fn sources(cx: &Cx<'_>, key: &str, port: &FighterPort, character_data: Option<&str>) -> Result<BodySources, KindError> {
    let pawn = pawn_class_of(cx, port, character_data)?;
    let pawn_cdo = cx.object(&cdo_path(&pawn))?;
    let base_cdo = cx.object(&cdo_path(BASE_FIGHTER))?;
    let Some(Value::Str(anim_class)) = template_value(&pawn_cdo, &base_cdo, "CharacterMesh0", "AnimClass")? else {
        return fail(format!("{pawn}: no CharacterMesh0 AnimClass"));
    };
    let bp = AnimBp::load(cx, &anim_class)?;
    let Some(Value::Str(skeleton)) = bp.cls.get("TargetSkeleton").cloned() else {
        return fail(format!("{anim_class}: no TargetSkeleton"));
    };
    let nodes = mirror_nodes(&bp);
    let mut tables: Vec<Option<Value>> = Vec::new();
    for n in &nodes {
        let t = at(n, "MirrorTable").cloned();
        if !tables.contains(&t) {
            tables.push(t);
        }
    }
    if tables.len() > 1 {
        return fail(format!("{anim_class}: mirror nodes with different tables"));
    }
    if !nodes.is_empty() {
        let Some(Some(Value::Str(t))) = tables.first() else {
            return fail(format!("{anim_class}: a mirror node without a MirrorTable"));
        };
        return Ok(BodySources { pawn, anim_class, skeleton, mirror: Some(t.clone()), mirror_node: true });
    }
    // (a node of a parent class would be the child's too, so only a class straight under AnimInstance is taken to
    // hold none)
    if str_of(bp.native.get("SuperStruct")) != Some(ANIM_INSTANCE) {
        return fail(format!("{anim_class}: no mirror node of its own"));
    }
    let mirror = mirror_table_unread(cx, key, port)?;
    Ok(BodySources { pawn, anim_class, skeleton, mirror, mirror_node: false })
}

// ---- components

/// The component templates of a class (the CDO's default subobjects and the SCS / inherited `_GEN_VARIABLE`
/// templates) in the package's export order, each resolved as the engine resolves a template's archetype chain: the
/// template's own tagged properties over the template of the same name in the parent class (`parent`: BaseFighter's
/// for a pawn), over the component class's defaults. Each component's members as the build stores them come back
/// beside (`raw`, layered as `p` is; dump_form reads them).
fn components(
    body: &Body<'_, '_, '_>,
    cls: &ObjView<'_>,
    cdo: &ObjView<'_>,
    parent: Option<&(Props, HashMap<String, Props>)>,
) -> Result<(Props, HashMap<String, Props>), KindError> {
    let mut picked: Vec<ObjView<'_>> = Vec::new();
    for x in cdo.inner()? {
        if !x.chain().contains('.') || x.chain().ends_with("_GEN_VARIABLE") {
            picked.push(x);
        }
    }
    for x in cls.inner()? {
        if x.chain().ends_with("_GEN_VARIABLE") {
            picked.push(x);
        }
    }
    picked.sort_by_key(|x| x.index());
    let mut res = Props::new();
    let mut raws = HashMap::new();
    for x in &picked {
        let name = short_name(x.path()).to_owned();
        let ty = short_name(x.class()).to_owned();
        let up = parent.and_then(|(p, _)| p.get(&name)).filter(|u| str_of(at(u, "type")) == Some(&ty));
        let (mut p, mut raw) = match up {
            Some(u) => (
                at(u, "p").and_then(Value::as_obj).cloned().unwrap_or_default(),
                parent.and_then(|(_, r)| r.get(&name)).cloned().unwrap_or_default(),
            ),
            None => body.component_class_defaults(x.class())?,
        };
        let own = properties(x)?;
        deep_update(&mut raw, &own);
        if let Value::Obj(sp) = body.shape(&Value::Obj(own))? {
            deep_update(&mut p, &sp);
        }
        dump_form(body, x.class(), &mut p, &raw)?;
        res.insert(name.clone(), obj([("type", s(ty)), ("p", Value::Obj(p))]));
        raws.insert(name, raw);
    }
    Ok((res, raws))
}

/// JS `String(n)` of an integer-valued number.
fn js_int_string(n: f64) -> String {
    if n.fract() == 0.0 && n.abs() < 1e21 { format!("{}", n as i64) } else { format!("{n}") }
}

/// The data holds two kinds of component member in the form the dump (cue4dump) wrote them, not as shape_value
/// shapes them: a map with integer keys has its keys as strings (Comp_HoldCharge ChargePack), a soft class or object
/// reference is {AssetPathName, SubPathString} (Comp_Pawn_Tether TetherSoftClass; read nowhere). Rewritten so here, by
/// the member's declared type (the component class's ChildProperties up its Blueprint parents), for the data's
/// identity.
fn dump_form(body: &Body<'_, '_, '_>, cls: &str, p: &mut Props, raw: &Props) -> Result<(), KindError> {
    let mut c = Some(cls.to_owned());
    while let Some(cur) = c.filter(|c| c.starts_with("/Game/")) {
        let o = body.cx().object(&cur)?;
        let nat = native(&o)?.unwrap_or_default();
        for d in child_props(&nat) {
            let name = prop_name(d);
            let Some(v) = p.get_mut(name) else { continue };
            let ty = str_of(d.get("Type"));
            let key_ty = str_of(d.get("KeyProp").and_then(|k| at(k, "Type")));
            if ty == Some("MapProperty") && key_ty == Some("IntProperty") && matches!(v, Value::Arr(_)) {
                if let Value::Arr(entries) = v {
                    for e in entries {
                        if let Value::Obj(e) = e
                            && let Some(k) = e.get_mut("Key")
                        {
                            *k = match &*k {
                                Value::Num(n) => s(js_int_string(*n)),
                                Value::Str(t) => s(t.clone()),
                                other => s(super::bag::str_of(Some(other)).unwrap_or("null")),
                            };
                        }
                    }
                }
            } else if matches!(ty, Some("SoftClassProperty" | "SoftObjectProperty"))
                && let Some(Value::Str(r)) = raw.get(name)
            {
                *v = obj([("AssetPathName", s(r.clone())), ("SubPathString", s(""))]);
            }
        }
        c = str_of(nat.get("SuperStruct")).map(str::to_owned);
    }
    Ok(())
}

// ---- anim sets

/// The further base set (the port table's anim_sets: LeBron, Tom and Jerry, Stripe) and the blend between the sets.
/// The one form read: the `graph` AnimBP's one AnimNode_BlendListByBool whose poses are a LinkedAnimLayer (the class
/// LinkAnimClassLayers puts the base set on) and a LinkedAnimGraph with the declared Tag (LinkAnimGraphByTag: the
/// further set), in either order (C023B_AnimBP has them the other way round: then true is the further set). Which
/// class goes on which node is in the ubergraph's bytecode and is declared, not read; the node shapes are checked and
/// anything else stops. BlendTime[i] is the time of a switch to pose i.
fn anim_sets(body: &Body<'_, '_, '_>, decl: &crate::fighter_port::AnimSets) -> Result<(Value, Value), KindError> {
    if decl.sets.len() != 1 {
        return fail("animSets: one further set is the form read (a BlendListByBool)");
    }
    let (name, st) = decl.sets[0];
    let bp = AnimBp::load(body.cx(), &class_of_pkg(decl.graph))?;
    let nodes = anim_node_props(&bp.native);
    let props = &bp.cdo;
    let node_at = |x: &Value| -> Option<&Props> { nodes.get(n_or(at(x, "LinkID"), f64::NAN) as usize).copied() };
    let layer_graph = ["AnimNode_LinkedAnimLayer", "AnimNode_LinkedAnimGraph"];
    let linked = |c: &Props| -> bool {
        let mut k: Vec<&str> = arr_of(props.get(prop_name(c)).and_then(|v| at(v, "BlendPose")))
            .iter()
            .map(|x| node_at(x).map_or("", |n| struct_of(n)))
            .collect();
        k.sort();
        let mut want = layer_graph.to_vec();
        want.sort();
        k == want
    };
    let blends: Vec<&Props> =
        nodes.iter().copied().filter(|c| struct_of(c) == "AnimNode_BlendListByBool" && linked(c)).collect();
    if blends.len() != 1 {
        return fail(format!(
            "animSets: {} has {} BlendListByBool nodes over a LinkedAnimLayer and a LinkedAnimGraph, 1 expected",
            decl.graph,
            blends.len()
        ));
    }
    let bname = prop_name(blends[0]);
    let bl = props.get(bname).cloned().unwrap_or(Value::Null);
    let mut pose: Vec<&Props> = arr_of(at(&bl, "BlendPose")).iter().filter_map(node_at).collect();
    let flip = pose.first().is_some_and(|p| struct_of(p) == "AnimNode_LinkedAnimGraph");
    if flip {
        pose.reverse();
    }
    let p1 = pose.get(1).and_then(|p| props.get(prop_name(p))).cloned().unwrap_or(Value::Null);
    let kinds: Vec<&str> = pose.iter().map(|p| struct_of(p)).collect();
    if kinds != layer_graph || str_of(at(&p1, "Tag")) != Some(st.tag) {
        return fail(format!(
            "animSets: {}: the poses of the blend are not a LinkedAnimLayer and a LinkedAnimGraph tagged {}",
            decl.graph, st.tag
        ));
    }
    if truthy(at(&bl, "CustomBlendCurve")) || truthy(at(&bl, "BlendProfile")) {
        return fail("animSets: the blend has a curve or a profile, not read");
    }
    let time = arr_of(Some(req(&bl, "BlendTime", "animSets blend")?)).to_vec();
    let tail_of = |k: &str| -> Result<Value, KindError> {
        Ok(s(tail(str_of(Some(req(&bl, k, "animSets blend")?)).unwrap_or_default())))
    };
    let t = |i: usize| time.get(i).cloned().unwrap_or(Value::Null);
    let mut bt = Props::new();
    bt.insert("base".into(), t(if flip { 1 } else { 0 }));
    bt.insert(name.to_owned(), t(if flip { 0 } else { 1 }));
    let blend = obj([
        ("node", s(bname)),
        ("onTrue", s(if flip { name } else { "base" })),
        ("onFalse", s(if flip { "base" } else { name })),
        ("blendTime", Value::Obj(bt)),
        ("blendType", tail_of("BlendType")?),
        ("transitionType", tail_of("TransitionType")?),
        ("resetChildOnActivation", req(&bl, "bResetChildOnActivation", "animSets blend")?.clone()),
    ]);
    let mut sets = Props::new();
    sets.insert(name.to_owned(), set_props(body.cx(), &body.b.key, &class_of_pkg(st.animbp))?);
    Ok((Value::Obj(sets), blend))
}

/// A BlendSpace1D of the character's anim assets as the sim's 1D player reads it: its axis, samples and cooked
/// GridSamples; anything else it serialises stops.
fn blend_space_1d(body: &Body<'_, '_, '_>, name: &Value, what: &str) -> Result<Props, KindError> {
    let shown = str_of(Some(name)).unwrap_or("undefined").to_owned();
    let hit = body.anim_asset(name);
    let Some((path, Some(AnimAssetKind::BlendSpace))) = hit else {
        return fail(format!("{what} {shown} is no BlendSpace1D of the anim assets"));
    };
    let q = properties(&body.cx().object(&path)?)?;
    let mut extra: Vec<&String> = q.keys().filter(|k| !FALL_SPACE_KEYS.contains(&k.as_str())).collect();
    if !extra.is_empty() {
        extra.sort();
        return fail(format!(
            "{what} {shown}: {} not read",
            extra.iter().map(|s| s.as_str()).collect::<Vec<_>>().join(", ")
        ));
    }
    let qv = Value::Obj(q);
    let ax = req(&qv, "BlendParameters", &format!("{what} {shown}"))?;
    if ax.as_obj().is_some_and(|o| o.keys().any(|k| !["DisplayName", "Min", "Max", "GridNum"].contains(&k.as_str()))) {
        return fail(format!("{what} {shown}: axis values not read"));
    }
    let grid = or(at(ax, "GridNum"), num(AXIS_GRID_NUM));
    let samples: Vec<Value> = arr_of(at(&qv, "SampleData"))
        .iter()
        .map(|x| {
            obj([
                ("anim", s(short_name(str_of(at(x, "Animation")).unwrap_or_default()))),
                ("x", or(at(x, "SampleValue").and_then(|v| at(v, "X")), num(0.0))),
                ("rate", or(at(x, "RateScale"), num(SAMPLE_RATE_DEFAULT))),
            ])
        })
        .collect();
    let mut cells = Vec::new();
    let gs = arr_of(at(&qv, "GridSamples"));
    for g in gs {
        let ix = [or(at(g, "Indices"), num(0.0)), or(at(g, "Indices[1]"), num(0.0)), or(at(g, "Indices[2]"), num(0.0))];
        if ix.iter().any(|i| {
            let i = n_or(Some(i), f64::NAN);
            i < -1.0 || i >= samples.len() as f64
        }) {
            return fail(format!("{what} {shown}: GridSamples index out of range"));
        }
        cells.extend(ix);
        cells.push(or(at(g, "Weights"), num(0.0)));
        cells.push(or(at(g, "Weights[1]"), num(0.0)));
        cells.push(or(at(g, "Weights[2]"), num(0.0)));
    }
    if gs.len() as f64 != n_or(Some(&grid), f64::NAN) + 1.0 {
        return fail(format!(
            "{what} {shown}: {} GridSamples for {} grid points",
            gs.len(),
            n_or(Some(&grid), 0.0) + 1.0
        ));
    }
    let mut out = Props::new();
    out.insert("name".into(), name.clone());
    out.insert("min".into(), or(at(ax, "Min"), num(AXIS_MIN)));
    out.insert("max".into(), or(at(ax, "Max"), num(AXIS_MAX)));
    out.insert("grid".into(), grid);
    out.insert("samples".into(), Value::Arr(samples));
    out.insert("cells".into(), Value::Arr(cells));
    Ok(out)
}

fn space_out(sp: &Props, head: Vec<(&str, Value)>) -> Value {
    let mut o = Props::new();
    o.insert("name".into(), sp["name"].clone());
    for (k, v) in head {
        o.insert(k.into(), v);
    }
    for k in ["min", "max", "grid", "samples", "cells"] {
        o.insert(k.into(), sp[k].clone());
    }
    Value::Obj(o)
}

/// For a set with UseFallBlendspace, Base_AnimBP's Fall player's values (BlendListByBool_17's pose 0) and the set's
/// FallBlendSpace, as the sim reads them. Written only for such a set (Batman).
fn fall_space(body: &Body<'_, '_, '_>, base: &AnimBp, st: &Value, more: &Value) -> Result<Option<Value>, KindError> {
    if more.as_obj().is_some_and(|m| m.values().any(|x| truthy(at(x, "UseFallBlendspace")))) {
        return fail("UseFallBlendspace on a further anim set: not ported");
    }
    if !truthy(at(st, "UseFallBlendspace")) {
        return Ok(None);
    }
    let p = &base.cdo;
    let nodes = anim_node_props(&base.native);
    let bl = p.get(FALL_BLEND_LIST).cloned().unwrap_or(Value::Null);
    let zero_zero = Value::Arr(vec![num(0.0), num(0.0)]);
    if at(&bl, "BlendTime") != Some(&zero_zero)
        || truthy(at(&bl, "bResetChildOnActivation"))
        || truthy(at(&bl, "CustomBlendCurve"))
        || truthy(at(&bl, "BlendProfile"))
    {
        return fail(format!("Base_AnimBP {FALL_BLEND_LIST}: blend values the sim does not have"));
    }
    let l0 = arr_of(at(&bl, "BlendPose")).first().map_or(f64::NAN, |x| n_or(at(x, "LinkID"), f64::NAN));
    let Some(node) = nodes.get(l0 as usize).copied().filter(|_| l0 >= 0.0) else {
        return fail(format!("Base_AnimBP {FALL_BLEND_LIST}: no pose 0"));
    };
    if struct_of(node) != "AnimNode_BlendSpacePlayer" {
        return fail(format!("Base_AnimBP {FALL_BLEND_LIST}: pose 0 is no BlendSpacePlayer ({})", prop_name(node)));
    }
    let n = p.get(prop_name(node)).cloned().unwrap_or(Value::Null);
    if s_or(at(&n, "GroupName"), "None") != "None"
        || n_or(at(&n, "StartPosition"), 0.0) != 0.0
        || !b_or(at(&n, "bLoop"), true)
    {
        return fail(format!(
            "Base_AnimBP {}: a sync group, a start position or no loop (not ported)",
            prop_name(node)
        ));
    }
    let sp =
        blend_space_1d(body, at(st, "FallBlendSpace").unwrap_or(&Value::Null), "UseFallBlendspace: FallBlendSpace")?;
    Ok(Some(space_out(&sp, vec![("node", s(prop_name(node))), ("playRate", or(at(&n, "PlayRate"), num(1.0)))])))
}

/// Base_AnimBP UseWallBlendSpace = IsValid(BlendSpace_WallIdle) (@7471-7500). With it the Wall machine's Wall Enter
/// and Wall Idle play BlendSpacePlayers over BlendSpace_WallEnter / BlendSpace_WallIdle at X = WallBlendSpaceValue (the
/// exposed inputs, checked here). Per state the player's values and the BlendSpace1D, as the sim reads them. Written
/// only for such a set (C018).
fn wall_spaces(body: &Body<'_, '_, '_>, base: &AnimBp, st: &Value, more: &Value) -> Result<Option<Value>, KindError> {
    if more.as_obj().is_some_and(|m| m.values().any(|x| truthy(at(x, "BlendSpace_WallIdle")))) {
        return fail("BlendSpace_WallIdle on a further anim set: not ported");
    }
    if !truthy(at(st, "BlendSpace_WallIdle")) {
        return Ok(None);
    }
    let p = &base.cdo;
    let nodes = anim_node_props(&base.native);
    let pair = |a: &str, b: &str| [a.to_owned(), b.to_owned()];
    let mut out = Props::new();
    for (key, (vr, lists)) in WALL_STATES {
        let mut got: Vec<(String, Value, Value)> = Vec::new();
        for bl in lists.iter() {
            if exposed_inputs(&base.cls, bl)? != Some(vec![pair("UseWallBlendSpace", "bActiveValue")]) {
                return fail(format!("Base_AnimBP {bl}: its bActiveValue is not UseWallBlendSpace"));
            }
            let pose: Vec<&Props> = arr_of(p.get(*bl).and_then(|v| at(v, "BlendPose")))
                .iter()
                .filter_map(|x| nodes.get(n_or(at(x, "LinkID"), f64::NAN) as usize).copied())
                .collect();
            if pose.len() != 2
                || struct_of(pose[0]) != "AnimNode_BlendSpacePlayer"
                || struct_of(pose[1]) != "AnimNode_SequencePlayer"
            {
                return fail(format!("Base_AnimBP {bl}: poses are not (BlendSpacePlayer, SequencePlayer)"));
            }
            let mut want = vec![pair("WallBlendSpaceValue", "X"), pair(vr, "BlendSpace")];
            want.sort_by_key(|x| x.join("\0"));
            let pn = prop_name(pose[0]);
            if exposed_inputs(&base.cls, pn)? != Some(want) {
                return fail(format!("Base_AnimBP {pn}: inputs are not X = WallBlendSpaceValue, BlendSpace = {vr}"));
            }
            let n = p.get(pn).cloned().unwrap_or(Value::Null);
            if s_or(at(&n, "GroupName"), "None") != "None" || n_or(at(&n, "StartPosition"), 0.0) != 0.0 {
                return fail(format!("Base_AnimBP {pn}: a sync group or a start position (not ported)"));
            }
            got.push((pn.to_owned(), or(at(&n, "PlayRate"), num(1.0)), or(at(&n, "bLoop"), Value::Bool(true))));
        }
        if got.iter().any(|g| g.1 != got[0].1 || g.2 != got[0].2) {
            return fail(format!("Base_AnimBP {key}: the two Wall machines differ"));
        }
        let sp = blend_space_1d(body, at(st, vr).unwrap_or(&Value::Null), vr)?;
        let (node, rate, looping) = got.swap_remove(0);
        out.insert((*key).to_owned(), space_out(&sp, vec![("node", s(node)), ("playRate", rate), ("loop", looping)]));
    }
    Ok(Some(Value::Obj(out)))
}

// ---- skeleton

fn axis3(v: Option<&Value>, keys: [&str; 3], d: f64) -> Value {
    let e = empty();
    let v = some(v).unwrap_or(&e);
    Value::Arr(keys.iter().map(|k| or(at(v, k), num(d))).collect())
}

/// The skeleton's sockets (its SkeletalMeshSocket inner objects in export order), its virtual bones with the AnimBP's
/// ModifyBone / Constraint nodes on them, and the mirror table's pairs and flip axes.
fn skeleton_of(cx: &Cx<'_>, src: &BodySources, anim: &AnimBp) -> Result<Value, KindError> {
    let skel = cx.object(&src.skeleton)?;
    let mut sockets = Props::new();
    for e in skel.inner()? {
        if short_name(e.class()) != "SkeletalMeshSocket" {
            continue;
        }
        let p = Value::Obj(properties(&e)?);
        let mut so = Props::new();
        if let Some(b) = at(&p, "BoneName") {
            so.insert("bone".into(), b.clone());
        }
        so.insert("loc".into(), axis3(at(&p, "RelativeLocation"), ["X", "Y", "Z"], 0.0));
        so.insert("rot".into(), axis3(at(&p, "RelativeRotation"), ["Pitch", "Yaw", "Roll"], 0.0));
        so.insert("scale".into(), axis3(at(&p, "RelativeScale"), ["X", "Y", "Z"], 1.0));
        let name = str_of(at(&p, "SocketName")).unwrap_or("undefined").to_owned();
        sockets.insert(name, Value::Obj(so));
    }
    // flip: each bone's BoneFlipAxis of the mirror table (X, Y, Z, XYZ or None); none: empty tables
    let mt = match &src.mirror {
        Some(m) => Some(Value::Obj(properties(&cx.object(m)?)?)),
        None => None,
    };
    // The Skeleton's virtual bones (USkeleton.VirtualBones): a bone under SourceBoneName that has TargetBoneName's
    // component-space transform; 'modify': the AnimBP's ModifyBone nodes that name the bone, as its class defaults
    // give them.
    let skel_p = properties(&skel)?;
    let mut vbones = Props::new();
    for v in arr_of(skel_p.get("VirtualBones")) {
        let name = str_of(at(v, "VirtualBoneName")).unwrap_or("undefined").to_owned();
        let mut b = Props::new();
        if let Some(x) = at(v, "SourceBoneName") {
            b.insert("source".into(), x.clone());
        }
        if let Some(x) = at(v, "TargetBoneName") {
            b.insert("target".into(), x.clone());
        }
        vbones.insert(name, Value::Obj(b));
    }
    if !vbones.is_empty() {
        let bone_of = |node: &Value| -> Option<String> {
            str_of(at(node, "BoneToModify").and_then(|b| at(b, "BoneName"))).map(str::to_owned)
        };
        for (k, node) in &anim.cdo {
            let Some(bone) = bone_of(node) else { continue };
            if !k.starts_with("AnimGraphNode_ModifyBone") || !vbones.contains_key(&bone) {
                continue;
            }
            let en = |f: &str| s(tail(str_of(at(node, f)).unwrap_or_default()));
            let comp = |f: &str, keys: [&str; 3]| -> Value {
                Value::Arr(
                    keys.iter().map(|a| at(node, f).and_then(|v| at(v, a)).cloned().unwrap_or(Value::Null)).collect(),
                )
            };
            // (a list: C017_AnimBP has two nodes on VB Fighter_Hold_Bone; their order in the graph is not read here)
            let m = obj([
                ("node", s(k.clone())),
                ("alpha", or(at(node, "Alpha"), num(1.0))),
                ("translation", comp("Translation", ["X", "Y", "Z"])),
                ("translationMode", en("TranslationMode")),
                ("translationSpace", en("TranslationSpace")),
                ("rotation", comp("Rotation", ["Pitch", "Yaw", "Roll"])),
                ("rotationMode", en("RotationMode")),
                ("rotationSpace", en("RotationSpace")),
                ("scale", comp("Scale", ["X", "Y", "Z"])),
                ("scaleMode", en("ScaleMode")),
                ("scaleSpace", en("ScaleSpace")),
            ]);
            if let Some(Value::Obj(vb)) = vbones.get_mut(&bone) {
                match vb.entry("modify".to_owned()).or_insert_with(|| Value::Arr(Vec::new())) {
                    Value::Arr(a) => a.push(m),
                    _ => unreachable!("modify is a list"),
                }
            }
        }
        // 'constraint': the Constraint node that puts a virtual bone on a mesh bone at the right-facing set
        // (VB_CONSTRAINT_RIGHT; every Constraint node on a virtual bone has to be listed there)
        let anim_cls = anim.name();
        let picks = VB_CONSTRAINT_RIGHT.iter().find(|(c, _)| *c == anim_cls).map_or(&[][..], |(_, p)| p);
        for (k, node) in &anim.cdo {
            let Some(bone) = bone_of(node) else { continue };
            if !k.starts_with("AnimGraphNode_Constraint") || !vbones.contains_key(&bone) {
                continue;
            }
            let (right, left) =
                picks.iter().find(|(b, _)| *b == bone).map_or((None, &[][..]), |(_, (r, l))| (Some(*r), *l));
            if right != Some(k.as_str()) && !left.contains(&k.as_str()) {
                return fail(format!("virtual bone {bone}: {k} of {anim_cls} is not in VB_CONSTRAINT_RIGHT"));
            }
            if right != Some(k.as_str()) {
                continue;
            }
            let setup = arr_of(at(node, "ConstraintSetup"));
            let mut targets: Vec<String> = Vec::new();
            for x in setup {
                let t = str_of(at(x, "TargetBone").and_then(|t| at(t, "BoneName"))).unwrap_or_default().to_owned();
                if !targets.contains(&t) {
                    targets.push(t);
                }
            }
            let mut types: Vec<&str> =
                setup.iter().map(|x| tail(str_of(at(x, "TransformType")).unwrap_or_default())).collect();
            types.sort();
            let axes = setup
                .iter()
                .all(|x| ["bX", "bY", "bZ"].iter().all(|a| truthy(at(x, "PerAxis").and_then(|p| at(p, a)))));
            let mut offs: Vec<&str> = Vec::new();
            for x in setup {
                let o = tail(str_of(at(x, "OffsetOption")).unwrap_or_default());
                if !offs.contains(&o) {
                    offs.push(o);
                }
            }
            if targets.len() != 1
                || types.join(",") != "Rotation,Translation"
                || !axes
                || offs != ["None"]
                || n_or(at(node, "Alpha"), 1.0) != 1.0
                || arr_of(at(node, "ConstraintWeights")).iter().any(|w| n_or(Some(w), f64::NAN) != 1.0)
            {
                return fail(format!("virtual bone {bone}: {k} is not a whole translation and rotation onto one bone"));
            }
            if let Some(Value::Obj(vb)) = vbones.get_mut(&bone) {
                vb.insert("constraint".into(), obj([("node", s(k.clone())), ("target", s(targets[0].clone()))]));
            }
        }
    }
    let mut mirror = Props::new();
    for e in arr_of(mt.as_ref().and_then(|m| at(m, "MirrorMapping"))) {
        if let Some([k, v]) = e.as_arr()
            && k != v
        {
            mirror.insert(str_of(Some(k)).unwrap_or("null").to_owned(), v.clone());
        }
    }
    let mut flip = Props::new();
    if let Some(m) = &mt {
        let smd = at(m, "SkeletonMirrorData");
        let table = arr_of(smd.and_then(|d| at(d, "SkelMirrorTable")));
        for (i, n) in arr_of(smd.and_then(|d| at(d, "BoneNames"))).iter().enumerate() {
            let axis = str_of(table.get(i).and_then(|t| at(t, "BoneFlipAxis"))).unwrap_or_default();
            let v = axis.split("AXIS_").nth(1).map_or(Value::Null, s);
            flip.insert(str_of(Some(n)).unwrap_or("null").to_owned(), v);
        }
    }
    let mut out = Props::new();
    out.insert("sockets".into(), Value::Obj(sockets));
    out.insert("virtualBones".into(), Value::Obj(vbones));
    out.insert(
        "mirrorAxis".into(),
        mt.as_ref().map_or(Value::Null, |m| at(m, "MirrorAxis").cloned().unwrap_or(Value::Null)),
    );
    out.insert("mirror".into(), Value::Obj(mirror));
    out.insert("flip".into(), Value::Obj(flip));
    // mirrorNode false: the AnimBP that runs on CharacterMesh0 holds no WAnimationMirrorNode, so facing left nothing
    // exchanges its bones
    if !src.mirror_node {
        out.insert("mirrorNode".into(), Value::Bool(false));
    }
    Ok(Value::Obj(out))
}

// ---- the anim assets

/// What an anim set entry that is not a plain sequence is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AnimAssetKind {
    BlendSpace,
    Composite,
}

fn anim_asset_value(p: &Props, kind: AnimAssetKind) -> Value {
    match kind {
        AnimAssetKind::BlendSpace => obj([(
            "samples",
            Value::Arr(
                arr_of(p.get("SampleData"))
                    .iter()
                    .map(|x| s(short_name(str_of(at(x, "Animation")).unwrap_or_default())))
                    .collect(),
            ),
        )]),
        AnimAssetKind::Composite => obj([(
            "segments",
            Value::Arr(
                arr_of(p.get("AnimationTrack").and_then(|t| at(t, "AnimSegments")))
                    .iter()
                    .map(|x| {
                        obj([
                            ("anim", s(short_name(str_of(at(x, "AnimReference")).unwrap_or_default()))),
                            ("start", or(at(x, "StartPos"), num(0.0))),
                            ("len", num(n_or(at(x, "AnimEndTime"), 0.0) - n_or(at(x, "AnimStartTime"), 0.0))),
                        ])
                    })
                    .collect(),
            ),
        )]),
    }
}

/// An anim set entry that is not a plain sequence (FighterData.animAssets' value), from the asset's properties (`p`,
/// the bag): a BlendSpace1D's samples in order, an AnimComposite's segments (AnimReference, StartPos, AnimEndTime -
/// AnimStartTime).
pub fn anim_asset_entry(p: &Props, kind: AnimAssetKind) -> Result<AnimAsset, KindError> {
    from_value(anim_asset_value(p, kind)).map_err(|e| KindError::Failed(format!("anim asset: {e}")))
}

fn polymorph_value(cx: &Cx<'_>, key: &str, anim_cls: &str) -> Result<Value, KindError> {
    let Some((_, (bp_pkg, layer_pkg))) = POLYMORPH_ANIM_LAYERS.iter().find(|(c, _)| *c == anim_cls) else {
        return fail(format!("polymorph anim set: {anim_cls} is not in POLYMORPH_ANIM_LAYERS"));
    };
    let lp = properties(&cx.object(&cdo_path(&class_of_pkg(layer_pkg)))?)?;
    let bpp = properties(&cx.object(&cdo_path(&class_of_pkg(bp_pkg)))?)?;
    let hs = or(bpp.get("HeadScale"), obj([("X", num(1.0)), ("Y", num(1.0)), ("Z", num(1.0))]));
    let head = Value::Arr(["X", "Y", "Z"].iter().map(|k| at(&hs, k).cloned().unwrap_or(Value::Null)).collect());
    Ok(obj([("set", shape_props(cx, key, lp)?), ("headScale", head)]))
}

/// A polymorph prop's anim set (FighterData.polymorphAnimSets' entry) by its AnimBP class (POLYMORPH_ANIM_LAYERS):
/// the linked layer class's defaults, shaped (its Anim_* clips), and the AnimBP's HeadScale.
pub fn polymorph_anim_set(cx: &Cx<'_>, key: &str, anim_cls: &str) -> Result<PolymorphAnimSet, KindError> {
    from_value(polymorph_value(cx, key, anim_cls)?).map_err(|e| KindError::Failed(format!("polymorph anim set: {e}")))
}

// ---- the part

/// The body part: writes pawn, base, components, animSet, animSets, animSetBlend, polymorphAnimSets, animAssets,
/// fallSpace, wallSpaces, slotHolds, animNodes and skeleton.
pub fn bind_body(b: &mut Bind<'_, '_>) -> Result<(), KindError> {
    let cx = b.cx;
    let port = b.port;
    let src = sources(cx, &b.key, port, b.character_data)?;
    let body = Body { b: &*b, anims: OnceCell::new(), comp_defaults: RefCell::new(HashMap::new()) };
    let pawn_cls = cx.object(&src.pawn)?;
    let pawn_cdo = cx.object(&cdo_path(&src.pawn))?;
    let base_cls = cx.object(BASE_FIGHTER)?;
    let base_cdo = cx.object(&cdo_path(BASE_FIGHTER))?;
    let anim = AnimBp::load(cx, &src.anim_class)?;
    let base = AnimBp::load(cx, BASE_ANIMBP)?;
    let key = body.b.key.clone();
    let mut out = Props::new();
    // 1. pawn, base: the CDOs' own properties
    out.insert("pawn".into(), shape_props(cx, &key, properties(&pawn_cdo)?)?);
    out.insert("base".into(), shape_props(cx, &key, properties(&base_cdo)?)?);
    // 2. components
    let base_comps = components(&body, &base_cls, &base_cdo, None)?;
    let (mut pawn_comps, _) = components(&body, &pawn_cls, &pawn_cdo, Some(&base_comps))?;
    // (`EvadeGroundToAir?` is not resolved over the CDOs: the sim reads pawn['EvadeGroundToAir?'], absent where the
    // pawn keeps BaseFighter's false)
    // 3. the anim set and the further ones
    let anim_set = set_props(cx, &key, &class_of_pkg(port.animbp))?;
    out.insert("animSet".into(), anim_set.clone());
    let mut more = Value::Obj(Props::new());
    if let Some(decl) = &port.anim_sets {
        let (sets, blend) = anim_sets(&body, decl)?;
        out.insert("animSets".into(), sets.clone());
        out.insert("animSetBlend".into(), blend);
        more = sets;
    }
    // The anim sets of the kit's own polymorph props: per PolymorphAnimClass, the linked layer class's defaults (its
    // Anim_* clips) and the AnimBP's HeadScale.
    let mut psets = Props::new();
    for (anim_cls, (bp_pkg, _)) in POLYMORPH_ANIM_LAYERS {
        if bp_pkg.starts_with(&format!("{}/", port.folder)) {
            psets.insert((*anim_cls).to_owned(), polymorph_value(cx, &key, anim_cls)?);
        }
    }
    if !psets.is_empty() {
        out.insert("polymorphAnimSets".into(), Value::Obj(psets.clone()));
    }
    // Anim set entries that are not plain sequences: a BlendSpace1D's samples in order, an AnimComposite's segments
    let mut assets = Props::new();
    let mut sets: Vec<&Value> = vec![&anim_set];
    if let Some(m) = more.as_obj() {
        sets.extend(m.values());
    }
    sets.extend(psets.values().filter_map(|x| at(x, "set")));
    for v in sets.iter().filter_map(|x| x.as_obj()).flat_map(|o| o.values()) {
        let Some((path, Some(kind))) = body.anim_asset(v) else { continue };
        let p = properties(&cx.object(&path)?)?;
        assets.insert(str_of(Some(v)).unwrap_or_default().to_owned(), anim_asset_value(&p, kind));
    }
    out.insert("animAssets".into(), Value::Obj(assets));
    if let Some(f) = fall_space(&body, &base, &anim_set, &more)? {
        out.insert("fallSpace".into(), f);
    }
    if let Some(w) = wall_spaces(&body, &base, &anim_set, &more)? {
        out.insert("wallSpaces".into(), w);
    }
    // 4. the Slot nodes of the mesh's AnimBP and the slot sets that hold the locomotion under a full-weight montage
    // (anim_graph.rs); the state machines of Base_AnimBP, the parent of every base layer class, under `layer`
    // Unverified: a Base_AnimBP_<character> is taken to state no state machine node value of its own.
    let mut holds = slot_holds(&anim, port.anim_node_kinds)?;
    holds.insert("layer".into(), state_machines(&base)?);
    out.insert("slotHolds".into(), Value::Obj(holds));
    if port.anim_node_kinds {
        out.insert("animNodes".into(), anim_nodes(&anim)?);
    }
    // The pawn's SCS nodes: the socket (or bone) of its parent a node's component hangs on (AttachToName), as 'socket'
    // beside the template (Pawn_Harley's Pistol on fml_un_C_prop2, BatBomb on fml_un_R_PropOffset)
    for e in pawn_cls.inner()? {
        if short_name(e.class()) != "SCS_Node" {
            continue;
        }
        let p = Value::Obj(properties(&e)?);
        let var = match at(&p, "InternalVariableName") {
            Some(Value::Str(v)) => v.clone(),
            Some(Value::Null) => "null".to_owned(),
            None => "undefined".to_owned(),
            Some(v) => super::bag::str_of(Some(v)).unwrap_or_default().to_owned(),
        };
        let name = format!("{var}_GEN_VARIABLE");
        if truthy(at(&p, "AttachToName"))
            && let Some(Value::Obj(c)) = pawn_comps.get_mut(&name)
        {
            c.insert("socket".into(), at(&p, "AttachToName").cloned().unwrap_or(Value::Null));
        }
    }
    let mut comps = Props::new();
    comps.insert("base".into(), Value::Obj(base_comps.0));
    comps.insert("pawn".into(), Value::Obj(pawn_comps));
    out.insert("components".into(), Value::Obj(comps));
    // 7. skeleton
    out.insert("skeleton".into(), skeleton_of(cx, &src, &anim)?);
    drop(body);
    for (k, v) in out {
        b.out.insert(k, v);
    }
    Ok(())
}
