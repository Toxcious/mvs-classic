//! The boxes bind's sockets: the point shapes `socket:<name>` a spawned actor, a trace or a hold starts from, the
//! virtual bones among them with the AnimBP's ModifyBone / Constraint rule, and the sockets an attached actor takes
//! its whole transform from (AttachToActor: axis_sockets). In UE space.
//!
//! The sources the game reads a socket name in: the Skeleton's sockets, Spawner notifies' Socket, Ak_SurfaceEffect
//! notifies' TraceFrom, the pawn's Comp_ItemManager HoldSlotName and the bones a graph reads (GRAPH_SOCKETS).

use std::collections::BTreeSet;

use indexmap::IndexMap;
use mvs_data::boxes::BoxKind;
use mvs_data::fighter::{Map, Socket, VirtualBone};
use mvs_data::{FighterData, Value};

use super::super::shape::fail;
use super::super::transform::{Vec3, rotator_quat};
use super::shapes::{Bones, new_shape};
use super::{Placed, VbPlace, get, truthy};
use crate::kind::KindError;

/// Bones a graph reads as sockets on any fighter: fml_un_COM, Parent_Car_SpawnedActor UpdateDriverInfo's
/// K2_AttachToComponent(the driver's main mesh, fml_un_COM) @281 (any fighter drives an item car); VB Spine,
/// VelmaChildBeam's BeamAttachSocket, read on GetOwner's mesh (VelmaHealBeam SpawnProjectile GetSocketLocation @221),
/// the ally Buff_VelmaHealOverTimeBlast spawns it on: any fighter. Every fighter's Skeleton has the virtual bone (on
/// its spine's spine_c, Gizmo's on fml_un_C_pelvis_att).
pub const EVERY_FIGHTER_SOCKETS: [&str; 2] = ["fml_un_COM", "VB Spine"];

/// Bones an actor's graph reads as sockets (GetSocketLocation / an attach) where no Spawner notify names them: sockets
/// named by Blueprint graph code, not data, so kept here with their graph offsets.
pub const GRAPH_SOCKETS: &[(&str, &[&str])] = &[
    // the wrists: C017_CannonShotAim reads them for the shot's start (the weapon mode aimers' shots); the spine bone
    // Pawn_C017 EyeLaser attaches C017_AntenaBolts to
    // fml_un_R_middle_base: C017_AnimBP's HoldLocation (the held fighter at HoldAlpha 1, @10652)
    // the tentacle tips: Comp_C017_Ally_Turret PilotFastAttack's GetSocketLocation (@10484-10546, the fast shot's
    // start); giantEmblem_att: its PilotLargeAttackVisuals (@456 the bomb's start, @8841 the charge emitter's attach)
    (
        "irongiant",
        &[
            "fml_un_R_wrist",
            "fml_un_L_wrist",
            "fml_un_vr_spine3",
            "fml_un_R_middle_base",
            "fml_un_R_tentacle8",
            "fml_un_L_tentacle8",
            "fml_un_C_tentacle8",
            "giantEmblem_att",
        ],
    ),
    // LeBron: Buff_C016_Ball_Conditional @919 attaches PS_C016Basketball_Recall to the mesh at fml_un_C_rotator.
    ("lebron", &["fml_un_C_rotator"]),
    // Arya: fml_un_C_head is the VacuumTargetSocket of Arya_Attack_Special_Forward_Montage2's hitframes #10, #13 and
    // #14 (Comp_Pawn_DamageManager CalculateVacuumKnockback @1165 Mesh.GetSocketLocation, @1250 DoesSocketExist: a
    // bone name counts).
    ("arya", &["fml_un_C_head"]),
    // Garnet: Buff_HeldByEnemyGarnet's ReceiveBeginPlay writes AttachSocket VB L_Hand or VB R_Hand by the instigator's
    // facing (@673-754), the socket Buff_HeldByEnemy's grapple holds the fighter at (InitiateGrapple @1229,
    // GetSocketLocation @2461): two virtual bones nothing else names.
    ("garnet", &["VB L_Hand", "VB R_Hand"]),
    // Finn: DropCoins spawns its coins at the backpack (Pawn_Finn GetSocketLocation @6781, SelectVector @7084).
    ("finn", &["fml_un_C_BackPack_b"]),
    // Jake: Buff_JakEat's AttachSocket is 'Root' (the Root bone), the socket Buff_HeldByEnemy's grapple holds the
    // swallowed fighter at (InitiateGrapple @1229, GetSocketLocation @2461). fml_un_C_head: JakeStretch_C's Tick
    // @12687 aims the spline's end tangent at it (JakeMainMesh.GetSocketLocation).
    ("jake", &["Root", "fml_un_C_head"]),
    // Tom and Jerry: Pawn_TomAndJerry's tick reads jerry_un_C_spine_d's Z for the wall trace (@1340 / @2131,
    // GetSocketLocation), JerryCoolDownMulticast jerry_un_C_spine_a for the poof and its sounds (@5579 / @5715 /
    // @5900). TomAndJerry_NoJerry_SlingShot_Aim reads the neck (Mesh.GetSocketLocation @2016, @4776, @5260: the aim's
    // origin and its place without Jerry) and spawns the tennis ball at the left wrist (CompSpawnActor @5763, Socket
    // tom_un_L_wrist): two bare bones no Spawner notify names.
    ("tomandjerry", &["jerry_un_C_spine_d", "jerry_un_C_spine_a", "tom_un_C_neck_a", "tom_un_L_wrist"]),
    // Stripe: Comp_C023B_TargetTracker FireAtTarget's trace start (main mesh GetSocketLocation(fml_un_L_uparm) @2269 /
    // @3093), a bare bone no Spawner notify names.
    ("stripe", &["fml_un_L_uparm"]),
];

/// A virtual bone's ModifyBone rule for a socket on it: the scale mode (`Some(true)` world, `Some(false)` local) and
/// Scale (`None`: the target's scale), a bone-space translation BMM_Additive, and whether the node turns the bone.
#[derive(Clone, Copy, Debug)]
struct VbRule {
    mode: Option<bool>,
    scale: Option<Vec3>,
    add: Option<Vec3>,
    turned: bool,
}

fn v3(v: &[f64]) -> Vec3 {
    [
        v.first().copied().unwrap_or(f64::NAN),
        v.get(1).copied().unwrap_or(f64::NAN),
        v.get(2).copied().unwrap_or(f64::NAN),
    ]
}

/// A virtual bone's rule: with scale BMM_Replace in BCS_WorldSpace the bone's world scale is the node's Scale (mode
/// world), in BCS_ComponentSpace its component scale (mode local); a translation BMM_Additive in BCS_BoneSpace
/// (Velma_AnimBP ModifyBone_7 on VB HoldPoint) moves the bone by that offset in its own space (14281bf70 case 3.
/// Unverified: the translation channel of 1424651b0, its order and AddToTranslation, not read). Any other node stops
/// the bind.
fn vb_rule(name: &str, vb: &VirtualBone) -> Result<VbRule, KindError> {
    let nodes = vb.modify.as_deref().unwrap_or(&[]);
    if nodes.is_empty() {
        return Ok(VbRule { mode: None, scale: None, add: None, turned: false });
    }
    if nodes.len() > 1 {
        return fail(format!("virtual bone {name}: {} ModifyBone nodes (not handled)", nodes.len()));
    }
    let n = &nodes[0];
    let bone_add = n.translation_mode == "BMM_Additive" && n.translation_space == "BCS_BoneSpace";
    // (a turn with no translation moves only a socket's offset off the bone, refused by socket_shapes: VB Spine's
    // rotation BMM_Replace in BCS_WorldSpace)
    let moved = n.translation_mode != "BMM_Ignore";
    if n.alpha != 1.0 || (moved && !bone_add) {
        return fail(format!(
            "virtual bone {name}: {} with alpha {}, translation {}, rotation {} (not handled)",
            n.node, n.alpha, n.translation_mode, n.rotation_mode
        ));
    }
    let add = bone_add.then(|| v3(&n.translation));
    let turned = n.rotation_mode != "BMM_Ignore";
    if n.scale_mode == "BMM_Ignore" {
        return Ok(VbRule { mode: None, scale: None, add, turned });
    }
    if n.scale_mode != "BMM_Replace" || (n.scale_space != "BCS_WorldSpace" && n.scale_space != "BCS_ComponentSpace") {
        return fail(format!(
            "virtual bone {name}: {} scale {} in {} (not handled)",
            n.node, n.scale_mode, n.scale_space
        ));
    }
    Ok(VbRule { mode: Some(n.scale_space == "BCS_WorldSpace"), scale: Some(v3(&n.scale)), add, turned })
}

/// What socket_shapes reads of the fighter.
pub struct SocketInput<'a> {
    pub key: &'a str,
    /// fighter data: montages, seqNotifies, components.pawn, skeleton.virtualBones
    pub fd: &'a FighterData,
    pub bones: &'a Bones,
    pub sockets: &'a Map<Socket>,
    /// a character whose AnimBP layers poses under its shapes (the frame half's skel block, consts.rs SKEL): a socket
    /// on a virtual bone is handled there only at the bone itself, under a node that moves no translation
    pub skel: bool,
}

fn zero(v: &[f64]) -> bool {
    !v.iter().any(|x| *x != 0.0)
}

struct OnVb {
    vname: String,
    target: String,
    rule: VbRule,
    at_bone: Option<String>,
}

/// The socket shapes (`socket:<name>`, in name order), each on its bone (a virtual bone's target, or its constraint's
/// bone), and the names of those an attached actor takes its whole transform from, in name order (`axis_sockets`;
/// each such shape's `axes` is its place there).
pub fn socket_shapes(k: &SocketInput<'_>) -> Result<(Vec<Placed>, Vec<String>), KindError> {
    let fd = k.fd;
    let bones = k.bones;
    // name -> (bone, place on it; bone None: the mesh component itself), the Skeleton's own with their rotation and
    // scale
    let mut sockets: IndexMap<String, (Option<String>, Vec3)> = IndexMap::new();
    let mut socket_rot: IndexMap<String, Vec<f64>> = IndexMap::new();
    let mut socket_scale: IndexMap<String, Vec<f64>> = IndexMap::new();
    for (name, s) in k.sockets {
        sockets.insert(name.clone(), (Some(s.bone.clone()), s.loc));
        if !zero(&s.rot) {
            socket_rot.insert(name.clone(), s.rot.to_vec());
        }
        if s.scale.iter().any(|v| *v != 1.0) {
            socket_scale.insert(name.clone(), s.scale.to_vec());
        }
    }
    fn bare(sockets: &mut IndexMap<String, (Option<String>, Vec3)>, sk: &str) {
        sockets.insert(sk.to_owned(), (Some(sk.to_owned()), [0.0; 3]));
    }
    // the names the sim reads a socket by (for the virtual bones below)
    let mut graph: Vec<&str> = GRAPH_SOCKETS.iter().find(|(g, _)| *g == k.key).map_or(&[][..], |(_, l)| *l).to_vec();
    graph.extend(EVERY_FIGHTER_SOCKETS);
    let mut named: BTreeSet<String> = graph.iter().map(|s| (*s).to_owned()).collect();
    // The hold socket (the pawn's Comp_ItemManager HoldSlotName): a held item is there and is released from there.
    // LeBron's L_Hand (on VB L_Hand), Wonder Woman's VB R_HoldPoint and Bugs's VB PropPickup are on virtual bones
    // nothing else names.
    let hold = fd.components.pawn.get("Comp_ItemManager_GEN_VARIABLE").and_then(|c| get(&c.p, "HoldSlotName"));
    if truthy(hold)
        && let Some(h) = hold.and_then(Value::as_str)
        && h != "None"
    {
        named.insert(h.to_owned());
    }
    let mut axis_names: Vec<String> = Vec::new();
    for m in fd.montages.values() {
        for n in &m.notifies {
            let sk = if n.cls.contains("Spawner") { n.p.get("Socket") } else { None };
            if !truthy(sk) {
                continue;
            }
            let Some(sk) = sk.and_then(Value::as_str) else { continue };
            named.insert(sk.to_owned());
            if !sockets.contains_key(sk) && bones.contains_key(sk) {
                bare(&mut sockets, sk);
            }
            // An actor a Spawner notify attaches to a socket takes the socket's rotation and scale too
            // (WW_ShieldActor_U_Special on HitCollision01Socket): those sockets get axis tracks.
            if sockets.contains_key(sk) && truthy(n.p.get("AttachToActor")) && !axis_names.iter().any(|x| x == sk) {
                axis_names.push(sk.to_owned());
            }
        }
    }
    axis_names.sort();
    for sk in &graph {
        if !sockets.contains_key(*sk) && bones.contains_key(*sk) {
            bare(&mut sockets, sk);
        }
    }
    // The name an Ak_SurfaceEffect notify traces from (TraceFrom, read with GetSocketLocation), in a montage or in a
    // sequence of the anim graph. FNames, so found without case: a socket (Shaggy's L_Foot_End), else a bare bone
    // (C016_Nav_Run's fml_un_R_foot, 'root' the Root bone). A name that is neither (Garnet's L_Foot / R_Foot, Finn's
    // fml_un_RL_foot, C017's foot) gives the mesh component's own location: USkinnedMeshComponent::GetSocketTransform
    // (1429bc000) starts from ComponentToWorld and keeps it when no socket or bone has the name, so the shape is on no
    // bone, marked missing (DoesSocketExist 1429b12c0 is false). Unverified: 1429bc000 was read for None; that a name
    // FindSocket (142d7c120) and GetBoneIndex (1429b39f0) both miss keeps ComponentToWorld too is taken from that
    // read.
    let no_vbones = Map::new();
    let vbones = fd.skeleton.virtual_bones.as_ref().unwrap_or(&no_vbones);
    let traced = fd
        .montages
        .values()
        .flat_map(|m| m.notifies.iter())
        .chain(fd.seq_notifies.iter().flat_map(|s| s.values()).flatten());
    let mut bone_by_lower: IndexMap<String, &String> = IndexMap::new();
    for b in bones.keys() {
        // (a Map built from pairs: a later bone of the same lower name wins)
        bone_by_lower.insert(b.to_lowercase(), b);
    }
    for n in traced {
        let sk = if n.cls.starts_with("Ak_SurfaceEffect") { n.p.get("TraceFrom") } else { None };
        if !truthy(sk) {
            continue;
        }
        let Some(sk) = sk.and_then(Value::as_str) else { continue };
        named.insert(sk.to_owned());
        let lo = sk.to_lowercase();
        if sockets.keys().any(|x| x.to_lowercase() == lo) {
            continue;
        }
        let bone =
            if bones.contains_key(sk) { Some(sk.to_owned()) } else { bone_by_lower.get(&lo).map(|b| (*b).clone()) };
        if let Some(b) = bone {
            bare(&mut sockets, &b);
        } else if !vbones.contains_key(sk) {
            sockets.insert(sk.to_owned(), (None, [0.0; 3]));
        }
    }
    // Virtual bones: a named socket on one, or one named as a socket.
    for sk in &named {
        if !sockets.contains_key(sk) && !bones.contains_key(sk) && vbones.contains_key(sk) {
            bare(&mut sockets, sk);
        }
    }
    let sorted_keys = |s: &IndexMap<String, (Option<String>, Vec3)>| {
        let mut v: Vec<String> = s.keys().cloned().collect();
        v.sort();
        v
    };
    let mut on_vb: IndexMap<String, OnVb> = IndexMap::new();
    for sk in sorted_keys(&sockets) {
        let (vname, loc) = sockets[&sk].clone();
        let Some(vname) = vname else { continue };
        if bones.contains_key(&vname) || !named.contains(&sk) {
            continue;
        }
        let Some(vb0) = vbones.get(&vname) else { continue };
        let mut target = vb0.target.clone();
        if let Some(c) = &vb0.constraint {
            // The AnimBP's Constraint node puts the bone on a mesh bone (translation and rotation, the right-facing
            // node): the bone's place is that bone's.
            if vb0.modify.is_some() || !zero(&loc) {
                return fail(format!(
                    "socket {sk} on the constrained virtual bone {vname}: a ModifyBone node or an offset (not handled)"
                ));
            }
            target = c.target.clone();
        }
        if !bones.contains_key(&target) || !bones.contains_key(&vb0.source) {
            return fail(format!("virtual bone {vname}: {} -> {target} is not on the mesh's bones", vb0.source));
        }
        // (an attached actor on it gets no axes)
        axis_names.retain(|x| *x != sk);
        let mut rule = vb_rule(&vname, vb0)?;
        if k.skel {
            // The skel block takes no ModifyBone rule. A socket at the virtual bone itself, whose node moves no
            // translation (VB Spine), is at its target's place whatever the node does to its rotation and scale: on
            // the target with no rule.
            if !zero(&loc) || rule.add.is_some() {
                return fail(format!(
                    "socket {sk} on the virtual bone {vname}: an offset or a translation, not handled for a character with skel tracks"
                ));
            }
            rule = VbRule { mode: None, scale: None, add: None, turned: rule.turned };
        }
        if rule.turned && !zero(&loc) {
            return fail(format!(
                "socket {sk} on the virtual bone {vname}: an offset under the node's rotation (not handled)"
            ));
        }
        if rule.mode.is_none() && rule.add.is_some() {
            return fail(format!(
                "socket {sk} on the virtual bone {vname}: a bone-space offset with no scale rule (the seam's vb has no form for it)"
            ));
        }
        let mut at_bone: Option<String> = None;
        if rule.mode == Some(true) && !zero(&loc) {
            // the bone's own place, for the runtime's actor scale (the offset does not take it)
            at_bone = sorted_keys(&sockets)
                .into_iter()
                .find(|x| sockets[x].0.as_deref() == Some(target.as_str()) && zero(&sockets[x].1));
            if at_bone.is_none() {
                if sockets.contains_key(&target) {
                    return fail(format!("socket {sk}: a socket named as its bone {target} has an offset"));
                }
                bare(&mut sockets, &target);
                at_bone = Some(target.clone());
            }
        }
        on_vb.insert(sk.clone(), OnVb { vname, target, rule, at_bone });
    }
    let mut shapes: Vec<Placed> = Vec::new();
    for sk in sorted_keys(&sockets) {
        let (mut bone, loc) = sockets[&sk].clone();
        let vb = on_vb.get(&sk);
        if let Some(vb) = vb {
            bone = Some(vb.target.clone());
        } else if let Some(b) = &bone
            && !bones.contains_key(b)
        {
            continue; // a socket on a virtual bone nothing names
        }
        let mut shape = new_shape(format!("socket:{sk}"), BoxKind::Socket, bone.clone(), Vec::new());
        // (no socket or bone has the name: DoesSocketExist is false for it, BoxShape.missing)
        if bone.is_none() {
            shape.missing = Some(true);
        }
        let mut sh = Placed { shape, loc, rot: [0.0, 0.0, 0.0, 1.0], vb: None, srot: None };
        if let Some(vb) = vb {
            sh.shape.vbone = Some(vb.vname.clone());
            if let (Some(world), Some(scale)) = (vb.rule.mode, vb.rule.scale) {
                sh.vb = Some(VbPlace { world, scale, add: vb.rule.add });
            }
            if let Some(a) = &vb.at_bone {
                sh.shape.unscaled = Some(format!("socket:{a}"));
            }
        }
        // the socket's own RelativeRotation (Pitch, Yaw, Roll) and RelativeScale, where the skeleton sets one
        if let Some(r) = socket_rot.get(&sk) {
            sh.shape.rot = Some(r.clone());
            sh.srot = Some(rotator_quat(r[0], r[1], r[2]));
        }
        if let Some(s) = socket_scale.get(&sk) {
            sh.shape.scale = Some(s.clone());
        }
        shapes.push(sh);
    }
    let axis_sockets: Vec<String> =
        axis_names.into_iter().filter(|x| shapes.iter().any(|s| s.shape.name == format!("socket:{x}"))).collect();
    for sh in &mut shapes {
        let n = &sh.shape.name["socket:".len().min(sh.shape.name.len())..];
        if let Some(i) = axis_sockets.iter().position(|x| x == n) {
            sh.shape.axes = Some(i as u32);
        }
    }
    Ok((shapes, axis_sockets))
}
