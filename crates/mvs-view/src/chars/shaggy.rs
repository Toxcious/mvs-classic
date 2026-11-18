//! Shaggy's render module: the sandwich props, the
//! equip layer, the head scale node, and the held Item_Sandwich_C every holder's view draws.

use mvs_data::Value;
use mvs_sim::World;

use super::{CharRender, HeldItem};
use crate::actors::MeshRule;
use crate::anim::{Clips, HeadScaleNode};
use crate::pose::{MeshMap, Rec};
use crate::skins::FighterDraws;
use crate::view::PropAttach;

/// The item class drawn in a holder's hand (Shaggy's module enters Item_Sandwich_C).
const HELD_ITEM: &str = "Item_Sandwich_C";
/// Shaggy's sandwich: RockMesh's and the held item's model.
pub(crate) const SANDWICH: &str = "/Game/Panda_Main/Characters/Shaggy/Props/Sandwich.Sandwich";

pub(crate) const RENDER: CharRender = CharRender {
    head_scale: &SHAGGY_HEAD,
    rage_shirt: Some("Shaggy_RageShirt_Idle"),
    view: Some(shaggy_view),
    held_items: &[HeldItem { cls: HELD_ITEM, prop: held_item }],
    ..CharRender::GENERIC
};

/// His classes' own mesh drawing: none (the sandwich projectile and item are the shared path's).
pub(crate) const RULES: &[(&str, MeshRule)] = &[];

/// Shaggy_AnimBP's ModifyBone_2 (scale Additive, bone space, from HeadScale by property access).
const SHAGGY_HEAD: [HeadScaleNode; 1] = [HeadScaleNode {
    node: "ModifyBone_2",
    bone: "fml_un_C_head",
    factor: 1.0,
    gated: false,
    moves: [0.0; 3],
    scale: true,
}];

/// `shaggyView`: the RockMesh sandwich on the wrist
/// sockets and the SandwichEquipped layer.
fn shaggy_view(r: &mut Rec, clips: &mut Clips, draws: &FighterDraws, meshes: &MeshMap) {
    // Deviation: RockMesh is not drawn with a material of its own on the Sandwitch texture, though the fighter
    // entry's `<skin>.RockMesh` (the pawn's component) has Sandwich_MAT_Inst; here the Sandwich actor's material,
    // as the held item's.
    let drawn = draws.actor("Sandwich", None);
    r.add_prop(
        "RockMesh",
        meshes,
        drawn,
        &PropAttach {
            bone: "fml_un_R_wrist",
            socket_loc: [25.813492, 28.431526, -30.903633],
            socket_rot: Some([73.69679, 98.19702, 43.3367]),
            loc: [12.786961, 41.10062, -19.221163],
            rot: [34.80478, -30.491438, -45.83111],
            scale: [1.0, 1.0, 1.0],
            abs_scale: false,
            left: Some((
                [-11.206309, -19.18257, 36.68803],
                [-9.847083, 151.51851, -107.495544],
                [-25.034094, -18.119926, -38.365807],
                [-22.031128, -6.207018, 42.770645],
                // (fml_un_L_wrist's BoneFlipAxis XYZ)
                [-1.0, -1.0, -1.0],
            )),
        },
    );
    // Shaggy_AnimBP: BlendListByBool_1 (SandwichEquipped, 0.1 s) -> LayeredBoneBlend of Shaggy_SandwichEquipPose on
    // branch fml_un_R_wrist, after all montage slots.
    if let Some(hold) = clips.get("Shaggy_SandwichEquipPose") {
        r.view.set_layer(clips, hold, "fml_un_R_wrist", 0.1);
    }
}

/// The held Item_Sandwich_C's prop on any fighter's view: the holder's hold
/// sockets (Comp_ItemManager HoldSlotName and its mirror), the item's Geo offset and scale, the Capsule's
/// bAbsoluteScale; the Sandwich actor entry's mesh and materials of `draws`.
fn held_item(r: &mut Rec, w: &World, draws: Option<&FighterDraws>, meshes: &MeshMap) {
    let sandwich = draws.and_then(|d| d.actor("Sandwich", None));
    let Some(item) = w.data.chars.values().find_map(|d| d.fighter.actors.as_ref()?.get(HELD_ITEM)) else { return };
    let cd = &w.data.chars[&r.key];
    let hold = mvs_sim::fighter::match_side::hold_slots(cd);
    if hold.right == "None" {
        return; // (no hold socket: the item's own actor at the mesh, not this prop)
    }
    let comp = |c: &str, k: &str| item.components.get(c)?.as_obj()?.get(k).cloned();
    let num = |v: &Option<Value>, k: &str| v.as_ref().and_then(|v| v.as_obj()?.get(k)?.as_f64());
    let gl = comp("Geo", "RelativeLocation");
    // Deviation: the prop is made once per view at the item's spawn scale 1; an item spawned at another scale (a death
    // item of an enlarged sandwich projectile) would be held at its own.
    let gs = num(&comp("Geo", "RelativeScale3D"), "X").unwrap_or(1.0);
    let hand = [num(&gl, "X").unwrap_or(0.0), num(&gl, "Y").unwrap_or(0.0), num(&gl, "Z").unwrap_or(0.0)];
    let abs = comp("Capsule", "bAbsoluteScale").and_then(|v| v.as_bool()).unwrap_or(false);
    let sk = &cd.fighter.skeleton;
    // a socket on a virtual bone, or a virtual bone named as the socket: the prop hangs on the bone's constraint
    // target, else its target
    let real = |b: &str| crate::virtual_bone::real_bone(sk, b).to_owned();
    let sock = |n: &str| match sk.sockets.get(n) {
        Some(s) => (real(&s.bone), s.loc, s.rot),
        None => (real(n), [0.0; 3], [0.0; 3]),
    };
    let (rb, rl, rr) = sock(&hold.right);
    let (lb, ll, lr) = sock(&hold.left);
    // Facing left the build's bone is the left socket's (a virtual bone's Constraint: its target's mirror twin, the
    // node the mirrored pose drives, as virtual_bone.rs virtual_socket): the view's mirror partner of it under the
    // reflection, with its own BoneFlipAxis in front (Bugs's fml_un_L_PropOffset flips Y, a wrist XYZ).
    let left_bone = match sk.sockets.get(&hold.left).map_or(hold.left.as_str(), |s| s.bone.as_str()) {
        b if sk.virtual_bones.as_ref().and_then(|v| v.get(b)).is_some_and(|v| v.constraint.is_some()) => {
            sk.mirror.get(&lb).cloned().unwrap_or(lb)
        }
        _ => lb,
    };
    // A BoneFlipAxis None bone (Black Adam's wrists) takes no flip: his item hangs on HeadMesh's ItemHold
    // (Comp_ItemManager OverrideAttachParentTag), and HeadMesh copies the main mesh's pose (C021_Skin00_AnimBP: one
    // AnimNode_CopyPoseFromMesh, bUseAttachedParent) under its VisMesh scale (1, -1, 1) facing left (C021_AnimBP
    // On Facing Changed @466, no mirror node): the right-facing ItemHold on fml_un_R_wrist, reflected.
    let flip = crate::pose::bone_flip_rows(sk, &left_bone).unwrap_or([1.0, 1.0, 1.0]);
    if !left_bone.eq_ignore_ascii_case(&rb) && sk.mirror.get(&left_bone).is_some_and(|p| !p.eq_ignore_ascii_case(&rb)) {
        eprintln!(
            "warning: held item: the left hold bone {left_bone} is neither {rb} nor its mirror partner (drawn on {rb})"
        );
    }
    r.add_prop(
        "HeldItem",
        meshes,
        sandwich,
        &PropAttach {
            bone: &rb,
            socket_loc: rl,
            socket_rot: Some(rr),
            loc: hand,
            rot: [0.0; 3],
            scale: [gs; 3],
            abs_scale: abs,
            left: Some((ll, lr, hand, [0.0; 3], flip)),
        },
    );
}
