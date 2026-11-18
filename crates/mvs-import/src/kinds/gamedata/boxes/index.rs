//! The boxes bind's index half: the pawn's SCS walk and shape classes over the collision profiles (`profiles.rs`), the
//! sockets, virtual bones, actors index and bone boxes, the COM bone and the clips to sample, from the fighter data,
//! the build's objects and the mesh kind.
//!
//! What it fills: the mesh's scale and place, the shapes (every field but `com` and `axes_const`, which the frame half
//! sets; `axes` is the socket's place in axis_sockets, before the frame half's constant pass), and the actors' and
//! bone boxes' inputs beside them.

use std::collections::HashSet;

use indexmap::IndexMap;
use mvs_data::fighter::{ActorDef, Map, Socket};
use mvs_data::{FighterData, Props, Value};

use super::super::fighter::body::body_sources;
use super::super::profiles::collision_table;
use super::super::reach::GRAPH_MONTAGES;
use super::super::shape::{cdo_path, defaults_or_properties, fail, properties, short_name};
use super::consts::{COM_BONE, skel_spec};
use super::shapes::{Bones, PawnShapeInput, actor_shapes, check_table, pawn_shapes};
use super::sockets::{SocketInput, socket_shapes};
use super::{
    ActorBindInput, BoneBoxInput, BoneBoxes, BoxBindInput, BoxClip, export_name, get, num_or, pkg_of, truthy, vec_of,
};
use crate::fighter_port::{asset_of, fighter_port};
use crate::kind::{Cx, KindError};
use crate::kinds::mesh;

const ANIM_SEQUENCE: &str = "/Script/Engine.AnimSequence";
const SKELETAL_MESH_SOCKET: &str = "/Script/Engine.SkeletalMeshSocket";
/// Base_AnimBP, the parent of every character's anim blueprint.
const BASE_ANIMBP_CDO: &str = "/Game/Panda_Main/Characters/Common/ANim/Base_AnimBP.Default__Base_AnimBP_C";

/// Base_AnimBP AnimGraphNode_ModifyBone_5.BoneToModify: the bone the hit rotation turns. Read from the node and
/// checked equal to COM_BONE: a mismatch stops the bind.
const COM_NODE: &str = "AnimGraphNode_ModifyBone_5";
/// Base_AnimBP's ModifyBone nodes on COM_BONE with its rotation BMM_Replace in BCS_ComponentSpace under the COM
/// Replace states: Fall _8, FastFall _11, Bounce _13, HitStunned's fly branch _12, Jump1 / Jump2 _9 / _10. Checked
/// against the CDO at bind.
const COM_REPLACE_NODES: [&str; 6] = [
    "AnimGraphNode_ModifyBone_8",
    "AnimGraphNode_ModifyBone_9",
    "AnimGraphNode_ModifyBone_10",
    "AnimGraphNode_ModifyBone_11",
    "AnimGraphNode_ModifyBone_12",
    "AnimGraphNode_ModifyBone_13",
];
/// The class-default keys of the clips the COM Replace states play.
const COM_REPLACE_KEYS: [&str; 10] = [
    "Anim_Fall_01",
    "Anim_FastFall", // Fall (UseFallBlendspace false), FastFall: ModifyBone_8 / _11
    "Anim_HitReact_WallBounceBack",
    "Anim_HitReact_WallBounceFront", // Bounce: ModifyBone_13
    "Anim_HitReact_FloorBounceBack",
    "Anim_HitReact_FloorBounceFront",
    "Anim_HitReact_FlyBack",
    "Anim_HitReact_FlyForward", // HitStunned's fly branch: ModifyBone_12
    "Anim_HitReact_FlyBack_Intro",
    "Anim_HitReact_FlyForward_Intro",
];
/// Jump1 / Jump2: ModifyBone_9 / _10, their blend spaces' samples.
const COM_REPLACE_SPACES: [&str; 2] = ["BlendSpace_Jump_01", "BlendSpace_Jump_02"];
/// Fall with UseFallBlendspace true (Batman's set): BlendListByBool_17 under the same ModifyBone_8 plays
/// FallBlendSpace in place of Anim_Fall_01 (Anim spec 01 A1, the Fall row): its samples are the Fall clips.
const COM_REPLACE_FALL_SPACE: &str = "FallBlendSpace";

/// A spawned actor class whose hit shapes ride its own SkeletalMeshComponent, by class: the mesh component (the
/// classes whose sim reads the track). Rick's Meeseeks_SpawnedActor (Meeseeks_SkelMesh) and MeeSeeks_AirFlap
/// (SkeletalMesh).
const ACTOR_BOX_MESHES: [(&str, &str); 2] =
    [("Meeseeks_SpawnedActor_C", "Meeseeks_SkelMesh"), ("MeeSeeks_AirFlap_C", "SkeletalMesh")];

/// A spawned actor's BoxComponent on a bone of its own SkeletalMesh, by class: its components.
/// ItemHelper_BananaGuard's SpearHitBox on CharacterMesh0's Spear_Tip; the clips are those of the mesh's folder's
/// Animations.
const ACTOR_BONE_BOXES: [(&str, &[&str]); 1] = [("ItemHelper_BananaGuard_C", &["SpearHitBox"])];

/// BaseFighter, the parent of every pawn.
const BASE_FIGHTER: &str = "/Game/Panda_Main/Blueprints/BaseFighter.BaseFighter_C";

fn base_name(p: &str) -> &str {
    &p[p.rfind('/').map_or(0, |i| i + 1)..]
}

/// A mesh's bones from the mesh kind's own reference skeleton: name -> parent.
pub fn mesh_bones(cx: &Cx<'_>, mesh_path: &str) -> Result<Bones, KindError> {
    let m = mesh::header(cx, mesh_path)?;
    let Some(sk) = &m.skeleton else { return fail(format!("{mesh_path}: no skeleton (not a skeletal mesh)")) };
    let mut out = Bones::new();
    for (i, b) in sk.bones.iter().enumerate() {
        let p = sk.parents[i];
        out.insert(b.clone(), (p >= 0).then(|| sk.bones[p as usize].clone()));
    }
    Ok(out)
}

/// A Skeleton's sockets (its SkeletalMeshSocket inner objects), as the fighter data's skeleton.sockets holds them.
fn skeleton_sockets(cx: &Cx<'_>, skeleton: &str) -> Result<Map<Socket>, KindError> {
    let o = cx.object(skeleton)?;
    let mut out = Map::new();
    for e in o.inner()?.into_iter().filter(|x| x.class() == SKELETAL_MESH_SOCKET) {
        let p = Value::Obj(properties(&e)?);
        let (l, r, s) = (get(&p, "RelativeLocation"), get(&p, "RelativeRotation"), get(&p, "RelativeScale"));
        let n = |v: Option<&Value>, k: &str, d: f64| num_or(v.and_then(|v| get(v, k)), d);
        let Some(name) = get(&p, "SocketName").and_then(Value::as_str) else {
            return fail(format!("{}: a socket with no SocketName", o.path()));
        };
        let bone = get(&p, "BoneName").and_then(Value::as_str).unwrap_or("").to_owned();
        out.insert(
            name.to_owned(),
            Socket {
                bone,
                loc: [n(l, "X", 0.0), n(l, "Y", 0.0), n(l, "Z", 0.0)],
                rot: [n(r, "Pitch", 0.0), n(r, "Yaw", 0.0), n(r, "Roll", 0.0)],
                scale: [n(s, "X", 1.0), n(s, "Y", 1.0), n(s, "Z", 1.0)],
            },
        );
    }
    Ok(out)
}

// ---- the pawn's SCS

/// The pawn class's SCS: a component's AttachToName, the component it is attached to (the node holding it in its
/// ChildNodes, else, a root node, its ParentComponentOrVariableName: a native component of the class), and the
/// construction order (the SCS runs its RootNodes in order, each node then its ChildNodes; a component joins its
/// parent's AttachChildren when it is made, and UpdateOverlaps walks that array in order: exe 14297ec80).
struct Scs {
    attach: IndexMap<String, Option<String>>,
    parent: IndexMap<String, Option<String>>,
    order: Vec<String>,
}

fn last_dot(s: &str) -> &str {
    s.rfind('.').map_or(s, |i| &s[i + 1..])
}

fn scs_of(cx: &Cx<'_>, cls_path: &str) -> Result<Scs, KindError> {
    let cls = cx.object(cls_path)?;
    let mut nodes: IndexMap<String, Props> = IndexMap::new();
    let mut roots: Vec<String> = Vec::new();
    for x in cls.inner()? {
        let last = last_dot(x.chain()).to_owned();
        if x.class() == "/Script/Engine.SCS_Node" {
            nodes.insert(last, properties(&x)?);
        } else if x.class() == "/Script/Engine.SimpleConstructionScript" {
            let p = Value::Obj(properties(&x)?);
            roots.extend(super::strings(get(&p, "RootNodes")));
        }
    }
    fn node_of<'n>(nodes: &'n IndexMap<String, Props>, cls_path: &str, r: &str) -> Result<&'n Props, KindError> {
        nodes.get(last_dot(r)).map_or_else(|| fail(format!("{cls_path}: no SCS node {r}")), Ok)
    }
    let node = |r: &str| node_of(&nodes, cls_path, r);
    let name = |p: &Props| p.get("InternalVariableName").and_then(Value::as_str).unwrap_or("").to_owned();
    let children = |p: &Props| super::strings(p.get("ChildNodes"));
    let opt_str = |v: Option<&Value>| v.filter(|x| !x.is_null()).and_then(Value::as_str).map(str::to_owned);
    let mut attach = IndexMap::new();
    let mut parent: IndexMap<String, Option<String>> = IndexMap::new();
    for p in nodes.values() {
        attach.insert(name(p), opt_str(p.get("AttachToName")));
    }
    for p in nodes.values() {
        for c in children(p) {
            parent.insert(name(node(&c)?), Some(name(p)));
        }
    }
    for p in nodes.values() {
        if !parent.contains_key(&name(p)) {
            parent.insert(name(p), opt_str(p.get("ParentComponentOrVariableName")));
        }
    }
    let mut order = Vec::new();
    fn walk(
        r: &str,
        nodes: &IndexMap<String, Props>,
        cls_path: &str,
        name: &dyn Fn(&Props) -> String,
        children: &dyn Fn(&Props) -> Vec<String>,
        order: &mut Vec<String>,
    ) -> Result<(), KindError> {
        let p = node_of(nodes, cls_path, r)?;
        order.push(name(p));
        for c in children(p) {
            walk(&c, nodes, cls_path, name, children, order)?;
        }
        Ok(())
    }
    for r in &roots {
        walk(r, &nodes, cls_path, &name, &children, &mut order)?;
    }
    Ok(Scs { attach, parent, order })
}

// ---- COM

/// An enum value's name without its type (`String(s).split('::').pop()`).
fn tail(v: Option<&Value>) -> String {
    let s = match v {
        Some(Value::Str(s)) => s.clone(),
        None => "undefined".to_owned(),
        Some(Value::Null) => "null".to_owned(),
        Some(x) => format!("{x:?}"),
    };
    s.rsplit("::").next().unwrap_or("").to_owned()
}

/// The COM bone, read from Base_AnimBP and checked against the COM Replace nodes.
fn com_bone(cx: &Cx<'_>) -> Result<String, KindError> {
    let cdo = cx.object(BASE_ANIMBP_CDO)?;
    let p = Value::Obj(defaults_or_properties(&cdo)?);
    let bone_of =
        |n: &Value| get(n, "BoneToModify").and_then(|b| get(b, "BoneName")).and_then(Value::as_str).map(str::to_owned);
    let bone = get(&p, COM_NODE).and_then(bone_of);
    if bone.as_deref() != Some(COM_BONE) {
        return fail(format!("Base_AnimBP {COM_NODE}: BoneToModify {bone:?}, the port's {COM_BONE}"));
    }
    for n in COM_REPLACE_NODES {
        let ok = get(&p, n).is_some_and(|v| {
            bone_of(v) == bone
                && tail(get(v, "RotationMode")) == "BMM_Replace"
                && tail(get(v, "RotationSpace")) == "BCS_ComponentSpace"
                && num_or(get(v, "Alpha").filter(|x| !x.is_null()), 1.0) == 1.0
        });
        if !ok {
            return fail(format!(
                "Base_AnimBP {n}: not a rotation Replace in component space on {COM_BONE} (the COM Replace states)"
            ));
        }
    }
    Ok(COM_BONE.to_owned())
}

/// The anim set and the further anim sets of a fighter.
fn anim_sets(fd: &FighterData) -> impl Iterator<Item = &Props> {
    std::iter::once(&fd.anim_set).chain(fd.anim_sets.iter().flat_map(|m| m.values()))
}

/// {clip: the anim set keys that play it} for the COM Replace states of every anim set of the fighter.
pub fn com_replaced(fd: &FighterData) -> Result<IndexMap<String, Vec<String>>, KindError> {
    let mut out: IndexMap<String, Vec<String>> = IndexMap::new();
    let mut add = |c: &str, k: &str| out.entry(c.to_owned()).or_default().push(k.to_owned());
    for st in anim_sets(fd) {
        let fall_space = truthy(st.get("UseFallBlendspace"));
        if fall_space && !truthy(st.get(COM_REPLACE_FALL_SPACE)) {
            return fail("UseFallBlendspace: the anim set names no FallBlendSpace");
        }
        for k in COM_REPLACE_KEYS {
            if k == "Anim_Fall_01" && fall_space {
                continue; // the Fall state plays FallBlendSpace instead
            }
            if truthy(st.get(k))
                && let Some(c) = st.get(k).and_then(Value::as_str)
            {
                add(c, k);
            }
        }
        if fall_space {
            let bs = st.get(COM_REPLACE_FALL_SPACE).and_then(Value::as_str).unwrap_or("");
            let Some(a) = fd.anim_assets.get(bs) else {
                return fail(format!("UseFallBlendspace: {bs} is not a BlendSpace1D of the anim assets (not handled)"));
            };
            for c in a.samples.as_deref().unwrap_or(&[]) {
                add(c, COM_REPLACE_FALL_SPACE);
            }
        }
        for k in COM_REPLACE_SPACES {
            if truthy(st.get(k))
                && let Some(v) = st.get(k).and_then(Value::as_str)
            {
                match fd.anim_assets.get(v).and_then(|a| a.samples.as_ref()) {
                    Some(samples) => samples.iter().for_each(|c| add(c, k)),
                    None => add(v, k),
                }
            }
        }
    }
    Ok(out)
}

/// Every string of a value, in walk order.
fn strings_in(v: &serde_json::Value, acc: &mut HashSet<String>) {
    match v {
        serde_json::Value::Array(a) => a.iter().for_each(|x| strings_in(x, acc)),
        serde_json::Value::Object(o) => o.values().for_each(|x| strings_in(x, acc)),
        serde_json::Value::String(s) => {
            acc.insert(s.clone());
        }
        _ => {}
    }
}

/// Of `clips`, those another state of an anim set (its key) or a montage (its name) plays too. The frame half calls
/// it with the clips whose COM rotation the Replace took off (>= 0.05 deg): a state among the users stops the bind, a
/// montage is a note.
pub fn com_shared(fd: &FighterData, clips: &[String]) -> Result<IndexMap<String, Vec<String>>, KindError> {
    let mut out: IndexMap<String, Vec<String>> = IndexMap::new();
    for st in anim_sets(fd) {
        for (k, v) in st {
            if let Value::Str(v) = v
                && clips.contains(v)
                && !COM_REPLACE_KEYS.contains(&k.as_str())
                && !COM_REPLACE_SPACES.contains(&k.as_str())
                && (k.starts_with("Anim_") || k.starts_with("BlendSpace_"))
            {
                out.entry(v.clone()).or_default().push(k.clone());
            }
        }
    }
    for (name, m) in &fd.montages {
        let slots = serde_json::to_value(&m.slots).map_err(|e| KindError::Failed(format!("{name}: {e}")))?;
        let mut played = HashSet::new();
        strings_in(&slots, &mut played);
        for c in clips {
            if played.contains(c) {
                out.entry(c.clone()).or_default().push(name.clone());
            }
        }
    }
    Ok(out)
}

// ---- clips

/// The clips to sample, in this order: the AnimSequences of the character's anim_dirs (default Animations, each with
/// its subfolders; '.' the folder's own), anim_skip left out, sorted as the script's glob paths sort (a Windows path
/// with mixed separators: `<folder>/<dir>\<sub>\<name>`, `<folder>\<name>` for '.'; an artefact kept for now), then
/// those of box_dirs a montage's slot track names, sorted the same. A sequence on another Skeleton (a prop's: Shaggy's
/// taunt nunchucks) is left out; a second package of one name with the same properties (Taz's
/// Attack_Special_Up_Grab_Cooldown in Specials/Air and Specials/Ground) is the first's.
fn clip_list(cx: &Cx<'_>, key: &str, fd: &FighterData, skeleton: &str) -> Result<Vec<BoxClip>, KindError> {
    let Some(port) = fighter_port(key) else { return fail(format!("{key}: no FIGHTER_PORT entry")) };
    let root = port.folder;
    let all = cx.catalog.list_packages(root, true, Some(ANIM_SEQUENCE));
    let rel = |p: &str| p.get(root.len() + 1..).unwrap_or("").to_owned();
    let skip = port.anim_skip.unwrap_or(&[]);
    let skipped = |p: &str| {
        let r = rel(p);
        skip.iter().any(|s| r == *s || (s.ends_with('/') && r.starts_with(s)))
    };
    // The packages of the folders `dirs`, each with its sort key.
    let files = |dirs: &[&str]| -> Vec<(String, String)> {
        let mut out: Vec<(String, String)> = Vec::new();
        for d in dirs {
            let pre = d.to_lowercase() + "/";
            for p in &all {
                let r = rel(p);
                if if *d == "." { r.contains('/') } else { !r.to_lowercase().starts_with(&pre) } {
                    continue;
                }
                if skipped(p) {
                    continue;
                }
                let k =
                    if *d == "." { format!("\\{r}") } else { format!("/{d}\\{}", r[pre.len()..].replace('/', "\\")) };
                out.push((k + ".mva", p.clone()));
            }
        }
        out.sort_by(|a, b| a.0.cmp(&b.0));
        out
    };
    let mut slot_named: HashSet<&str> = HashSet::new();
    for m in fd.montages.values() {
        for s in &m.slots {
            for g in &s.segments {
                slot_named.insert(&g.anim);
            }
        }
    }
    let main = files(port.anim_dirs.unwrap_or(&["Animations"]));
    let extra = match port.box_dirs {
        Some(d) if !d.is_empty() => files(d),
        _ => Vec::new(),
    };
    let skel = pkg_of(skeleton).to_lowercase();
    let mut clips = Vec::new();
    let mut seen: IndexMap<String, (String, String, String)> = IndexMap::new();
    for (i, (_, p)) in main.iter().chain(extra.iter()).enumerate() {
        let o = cx.object(&asset_of(p))?;
        let name = export_name(o.path()).to_owned();
        if i >= main.len() && !slot_named.contains(name.as_str()) {
            continue;
        }
        let props = properties(&o)?;
        match props.get("Skeleton") {
            Some(Value::Str(s)) if pkg_of(s).to_lowercase() == skel => {}
            _ => continue, // a prop skeleton
        }
        let shown = format!("{props:?}");
        if let Some((cls, path, had)) = seen.get(&name) {
            // one clip in two packages: the first is sampled
            if cls == o.class() && *had == shown {
                continue;
            }
            return fail(format!("two sequences named {name} ({path}, {})", o.path()));
        }
        seen.insert(name.clone(), (o.class().to_owned(), o.path().to_owned(), shown));
        clips.push(BoxClip { name, path: o.path().to_owned() });
    }
    Ok(clips)
}

// ---- actors

/// A path lookup: exact, else the first of the same lower-case name.
fn lookup<'m>(m: &'m Map<String>, name: &str) -> Option<&'m str> {
    if let Some(v) = m.get(name) {
        return Some(v);
    }
    let l = name.to_lowercase();
    m.iter().find(|(k, _)| k.to_lowercase() == l).map(|(_, v)| v.as_str())
}

/// The mesh object path a fighter data component names (its SkeletalMesh, a short name), from FighterData.paths.
fn mesh_of(fd: &FighterData, name: &str, what: &str) -> Result<String, KindError> {
    lookup(&fd.paths.meshes, name)
        .map(str::to_owned)
        .map_or_else(|| fail(format!("{what}: the mesh {name} is not in the fighter data's paths")), Ok)
}

fn skeleton_of_mesh(cx: &Cx<'_>, mesh: &str) -> Result<String, KindError> {
    let o = cx.object(mesh)?;
    match properties(&o)?.get("Skeleton") {
        Some(Value::Str(s)) => Ok(s.clone()),
        _ => fail(format!("{mesh}: no Skeleton")),
    }
}

/// The actor tracks' inputs, without the frames: per class of ACTOR_BOX_MESHES the fighter's data holds, the mesh,
/// its shapes and the sequences it plays.
fn actor_inputs(cx: &Cx<'_>, fd: &FighterData) -> Result<Option<IndexMap<String, ActorBindInput>>, KindError> {
    let mut out = IndexMap::new();
    for (cls, mesh_comp) in ACTOR_BOX_MESHES {
        let Some(a) = fd.actors.as_ref().and_then(|m| m.get(cls)) else { continue };
        let comps = &a.components;
        let mesh = comps.get(mesh_comp);
        let Some(mesh) = mesh.filter(|m| truthy(get(m, "SkeletalMesh"))) else {
            return fail(format!("{cls}: no SkeletalMeshComponent {mesh_comp}"));
        };
        if truthy(get(mesh, "RelativeRotation")) {
            return fail(format!("{cls}: {mesh_comp} has a RelativeRotation (not handled)"));
        }
        let msc = get(mesh, "RelativeScale3D").filter(|x| !x.is_null());
        let c = |k: &str| num_or(msc.and_then(|s| get(s, k)).filter(|x| !x.is_null()), 1.0);
        if !(c("X") == c("Y") && c("Y") == c("Z")) {
            return fail(format!("{cls}: {mesh_comp} has a non-uniform scale (not handled)"));
        }
        let ml = get(mesh, "RelativeLocation").filter(|x| !x.is_null());
        if truthy(ml.and_then(|l| get(l, "X"))) || truthy(ml.and_then(|l| get(l, "Y"))) {
            return fail(format!("{cls}: {mesh_comp} is off the actor's axis (not handled)"));
        }
        let mesh_scale = c("X");
        let mesh_z = num_or(ml.and_then(|l| get(l, "Z")).filter(|x| !x.is_null()), 0.0);
        let sm = get(mesh, "SkeletalMesh").and_then(Value::as_str).unwrap_or("");
        let mesh_asset = mesh_of(fd, sm, cls)?;
        let skeleton = skeleton_of_mesh(cx, &mesh_asset)?;
        let bones = mesh_bones(cx, &mesh_asset)?;
        let sockets = skeleton_sockets(cx, &skeleton)?;
        let table = collision_table(cx)?;
        let placed = actor_shapes(cls, comps, mesh_comp, mesh_scale, &bones, &sockets, &table)?;
        // The sequences the mesh plays: AnimToPlay of a single-node mesh, else every sequence of the montages the
        // class's own graph plays (actor-reach GRAPH_MONTAGES; the data holds them as `actorMontages`), all slots
        let mut names: Vec<String> = Vec::new();
        if get(mesh, "AnimationMode").and_then(Value::as_str) == Some("EAnimationMode::AnimationSingleNode") {
            let anim = get(mesh, "AnimationData").and_then(|d| get(d, "AnimToPlay")).and_then(Value::as_str);
            names = vec![anim.unwrap_or("").to_owned()];
        } else {
            let supers = a.supers.as_deref().unwrap_or("");
            for k in std::iter::once(cls).chain(supers.split_whitespace()) {
                for mp in GRAPH_MONTAGES.get(k)? {
                    let mn = short_name(mp);
                    let Some(m) = fd.actor_montages.as_ref().and_then(|x| x.get(mn)) else {
                        return fail(format!("{cls}: the montage {mn} is not in the data's actorMontages"));
                    };
                    for sl in &m.slots {
                        for sg in &sl.segments {
                            if sg.anim_type != "AnimSequence" {
                                return fail(format!("{cls}: {mn} plays a {} (not handled)", sg.anim_type));
                            }
                            if !names.contains(&sg.anim) {
                                names.push(sg.anim.clone());
                            }
                        }
                    }
                }
            }
        }
        names.sort();
        let mut clips = Vec::new();
        for n in &names {
            let Some(path) = lookup(&fd.paths.clips, n) else {
                return fail(format!("{cls}: the sequence {n} is not in the fighter data's paths"));
            };
            let o = cx.object(path)?;
            let ok = matches!(properties(&o)?.get("Skeleton"), Some(Value::Str(s)) if pkg_of(s).to_lowercase() == pkg_of(&skeleton).to_lowercase());
            if export_name(o.path()) != n || !ok {
                return fail(format!("{cls}: {} is not {n} on {skeleton}", o.path()));
            }
            clips.push(BoxClip { name: n.clone(), path: o.path().to_owned() });
        }
        out.insert(
            cls.to_owned(),
            ActorBindInput {
                mesh: mesh_asset,
                skeleton,
                mesh_comp: mesh_comp.to_owned(),
                mesh_scale,
                mesh_z,
                shapes: placed,
                clips,
            },
        );
    }
    Ok((!out.is_empty()).then_some(out))
}

/// `mesh_of(cls, mesh_comp, what)`: the skeletal mesh of an actor's mesh component, as an object path.
pub type MeshOf<'f> = dyn Fn(&str, &str, &str) -> Result<String, KindError> + 'f;

/// The bone boxes' inputs, without the frames: per class of ACTOR_BONE_BOXES `actors` holds (a fighter data's or the
/// items part's actor defs), each component's mesh (`mesh_of`), bone, place, extent and the clips of its mesh's
/// folder's Animations.
pub fn bone_box_inputs(
    cx: &Cx<'_>,
    actors: Option<&Map<ActorDef>>,
    mesh_of: &MeshOf<'_>,
) -> Result<Option<BoneBoxes>, KindError> {
    let mut out = IndexMap::new();
    for (cls, list) in ACTOR_BONE_BOXES {
        let Some(a) = actors.and_then(|m| m.get(cls)) else { continue };
        let comps = &a.components;
        let mut by_comp = IndexMap::new();
        for comp in list {
            let Some(c) = comps.get(*comp).filter(|c| !c.is_null()) else {
                return fail(format!("{cls}: no component {comp}"));
            };
            let parent = get(c, "AttachParent").and_then(Value::as_str).unwrap_or("");
            if !truthy(comps.get(parent)) {
                return fail(format!("{cls}.{comp}: no parent {parent}"));
            }
            if ["RelativeRotation", "RelativeScale3D", "bAbsoluteRotation", "bAbsoluteScale"]
                .iter()
                .any(|k| truthy(get(c, k)))
            {
                return fail(format!("{comp}: a rotation or scale of its own (not handled)"));
            }
            let mesh = mesh_of(cls, parent, &format!("{cls}.{comp}"))?;
            let skeleton = skeleton_of_mesh(cx, &mesh)?;
            let bones = mesh_bones(cx, &mesh)?;
            let bone = get(c, "socket").and_then(Value::as_str).unwrap_or("").to_owned();
            if !bones.contains_key(&bone) {
                return fail(format!("{comp}: {bone} is no bone of {mesh} (a socket is not handled)"));
            }
            // (the mesh's own folder, and its Animations beside it)
            let mp = pkg_of(&mesh);
            let dir = format!("{}/Animations", &mp[..mp.rfind('/').unwrap_or(0)]);
            // (by the script's file names, `.mva` and all: a name with a character below '.' sorts as it did)
            let mva = |p: &str| format!("{}.mva", base_name(p));
            let mut pkgs = cx.catalog.list_packages(&dir, false, Some(ANIM_SEQUENCE));
            pkgs.sort_by_key(|a| mva(a));
            let mut clips = Vec::new();
            for p in &pkgs {
                let o = cx.object(&asset_of(p))?;
                if matches!(properties(&o)?.get("Skeleton"), Some(Value::Str(s)) if pkg_of(s).to_lowercase() == pkg_of(&skeleton).to_lowercase())
                {
                    clips.push(BoxClip { name: export_name(o.path()).to_owned(), path: o.path().to_owned() });
                }
            }
            if clips.is_empty() {
                return fail(format!("{comp}: no clip of {mesh} under {dir}"));
            }
            let e = get(c, "BoxExtent");
            let ex = |k: &str| e.and_then(|e| get(e, k)).and_then(Value::as_f64).unwrap_or(f64::NAN);
            by_comp.insert(
                (*comp).to_owned(),
                BoneBoxInput {
                    mesh,
                    skeleton,
                    bone,
                    loc: vec_of(get(c, "RelativeLocation")),
                    extent: [ex("X"), ex("Y"), ex("Z")],
                    clips,
                },
            );
        }
        out.insert(cls.to_owned(), by_comp);
    }
    Ok((!out.is_empty()).then_some(out))
}

/// The raw value of a component template member, the pawn's template over BaseFighter's (the CDO's default subobject
/// of that name), a null passing to the next (`??`).
fn template_value(cx: &Cx<'_>, pawn: &str, comp: &str, member: &str) -> Result<Option<Value>, KindError> {
    for cls in [pawn, BASE_FIGHTER] {
        let cdo = cx.object(&cdo_path(cls))?;
        if let Some(c) = cdo.inner_named(comp)?
            && let Some(v) = properties(&c)?.get(member).filter(|v| !v.is_null())
        {
            return Ok(Some(v.clone()));
        }
    }
    Ok(None)
}

// ---- the bind

/// The index half's input of `key` over its bound fighter data.
pub fn bind_box_input(cx: &Cx<'_>, key: &str, fd: &FighterData) -> Result<BoxBindInput, KindError> {
    let src = body_sources(cx, key)?;
    let table = collision_table(cx)?;
    com_bone(cx)?;
    check_table(&table)?;
    // CharacterMesh0: its SkeletalMesh (the pawn's template over BaseFighter's), its place and scale (the fighter
    // data's resolved template). Only its Z is in the tracks (meshZ): BaseFighter writes the mesh's relative location
    // whole at run time (HandleHitShake, BFHitPauseEnd), so its X is the sim's (Fighter.meshX).
    let mesh = match template_value(cx, &src.pawn, "CharacterMesh0", "SkeletalMesh")? {
        Some(Value::Str(s)) => s,
        _ => return fail(format!("{}: no CharacterMesh0 SkeletalMesh", src.pawn)),
    };
    let comps = &fd.components.pawn;
    let Some(m) = comps.get("CharacterMesh0").map(|c| &c.p) else { return fail("no CharacterMesh0 component") };
    if truthy(get(m, "RelativeRotation")) {
        return fail("CharacterMesh0 has a RelativeRotation (not handled)");
    }
    fn nn(v: Option<&Value>) -> Option<&Value> {
        v.filter(|x| !x.is_null())
    }
    let mesh_scale = num_or(nn(nn(get(m, "RelativeScale3D")).and_then(|s| get(s, "X"))), 1.0);
    let mesh_z = num_or(nn(nn(get(m, "RelativeLocation")).and_then(|s| get(s, "Z"))), 0.0);
    let bones = mesh_bones(cx, &mesh)?;
    let scs = scs_of(cx, &src.pawn)?;
    let sockets = &fd.skeleton.sockets;
    // The component tags a notify's HitBoxTags reaches (with the pawn's Use 2D Hitboxes a tag other than FullBody
    // finds tag + "2D"): a detector is written only when one does.
    let use2d = truthy(nn(fd.pawn.get("Use 2D Hitboxes")).or_else(|| fd.base.get("Use 2D Hitboxes")));
    let mut hit_reach: Vec<String> = Vec::new();
    let notifies = fd
        .montages
        .values()
        .flat_map(|m| m.notifies.iter())
        .chain(fd.seq_notifies.iter().flat_map(|s| s.values()).flatten());
    for n in notifies {
        for t in super::strings(n.p.get("HitBoxTags")) {
            let t = if use2d && t != "FullBody" { t + "2D" } else { t };
            if !hit_reach.contains(&t) {
                hit_reach.push(t);
            }
        }
    }
    let mut placed = pawn_shapes(&PawnShapeInput {
        comps,
        base: &fd.components.base,
        attach: &scs.attach,
        scs_parent: &scs.parent,
        scs_order: &scs.order,
        mesh_scale,
        hit_reach: &hit_reach,
        bones: &bones,
        sockets,
        table: &table,
    })?;
    let (sock, axis_sockets) =
        socket_shapes(&SocketInput { key, fd, bones: &bones, sockets, skel: skel_spec(key).is_some() })?;
    placed.extend(sock);
    // (the shapes that ride the COM bone: the frame half marks them, `com`, from the mesh it samples)
    let clips = clip_list(cx, key, fd, &src.skeleton)?;
    let actors = actor_inputs(cx, fd)?;
    let mesh_of_comp = |cls: &str, m: &str, what: &str| -> Result<String, KindError> {
        let sm = fd
            .actors
            .as_ref()
            .and_then(|a| a.get(cls))
            .and_then(|a| a.components.get(m))
            .and_then(|c| get(c, "SkeletalMesh"))
            .and_then(Value::as_str)
            .unwrap_or("");
        mesh_of(fd, sm, what)
    };
    let bone_boxes = bone_box_inputs(cx, fd.actors.as_ref(), &mesh_of_comp)?;
    Ok(BoxBindInput {
        key: key.to_owned(),
        mesh,
        skeleton: src.skeleton,
        mesh_scale,
        mesh_z,
        shapes: placed,
        clips,
        com_replaced: com_replaced(fd)?.into_keys().collect(),
        axis_sockets,
        actors,
        bone_boxes,
    })
}
