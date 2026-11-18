//! A socket on a virtual bone: the view has no such bone;
//! it is its target bone (or its AnimBP Constraint's) under the AnimBP's ModifyBone node on it. The views' follow of
//! it, for the items a fighter holds or wears (Bugs Bunny's VB PropPickup, Taz's Head ATT on VB Head) and Harley
//! Quinn's bomb; the effects' own is mvs-render fx/attach.rs VirtualFollow.
// Unverified: the ModifyBone channels are stock 4.26; a Constraint's target is its right-facing node
// reflected with the view; Root's scale is taken as 1.

use mvs_data::fighter::{Skeleton, VirtualBone};
use mvs_sim::World;
use mvs_sim::fighter::FighterId;
use mvs_vfx::math::{V3, compose_rows, rotation_rows};

use crate::actor_view::{IDENT, Xform};
use crate::actors::{ActorHost, FighterBones, Rows};
use crate::math;
use crate::skin_intro::warn_once;

/// The virtual bone socket (or bone) `socket` of `skel` is on, if any.
pub fn virtual_bone_of<'a>(skel: &'a Skeleton, socket: &str) -> Option<(&'a str, &'a VirtualBone)> {
    let name = skel.sockets.get(socket).map_or(socket, |s| s.bone.as_str());
    skel.virtual_bones.as_ref()?.get_key_value(name).map(|(k, v)| (k.as_str(), v))
}

/// The mesh bone a bone named `bone` hangs on: a virtual bone's constraint target, else its target; any other
/// itself.
pub fn real_bone<'a>(skel: &'a Skeleton, bone: &'a str) -> &'a str {
    match skel.virtual_bones.as_ref().and_then(|v| v.get(bone)) {
        Some(vb) => vb.constraint.as_ref().map_or(vb.target.as_str(), |c| c.target.as_str()),
        None => bone,
    }
}

impl ActorHost<'_> {
    /// `socketFollow`: `rel` under fighter `f`'s socket (or bone) `socket`, a virtual
    /// bone's ([`Self::virtual_socket`]) or the view's ([`Self::socket`]); None with no view or no such bone.
    pub fn socket_follow(&self, w: &World, f: FighterId, socket: &str, rel: &Xform) -> Option<Xform> {
        if virtual_bone_of(&w.char_data(f).fighter.skeleton, socket).is_some() {
            return self.virtual_socket(w, f, socket, rel);
        }
        self.socket(w, f, socket, rel)
    }

    /// `virtualSocketFollow`: `rel` (its location and rotation, then its scale on the result's, as socketFollow) on
    /// fighter `f`'s socket `socket` of a virtual bone: the target's place (facing left a Constraint's mirror twin's)
    /// under the bone's one ModifyBone node, the socket's own transform under it. None: not on a virtual bone, no
    /// view, or the view lacks the target.
    pub fn virtual_socket(&self, w: &World, f: FighterId, socket: &str, rel: &Xform) -> Option<Xform> {
        let skel = &w.char_data(f).fighter.skeleton;
        let (name, vb) = virtual_bone_of(skel, socket)?;
        let sk = skel.sockets.get(socket);
        let id = w.fighter(f).id;
        let fb = self.fighters.iter().find(|b| b.id == id)?;
        let left = w.fighter(f).body.facing_dir < 0.0;
        let target = vb.constraint.as_ref().map_or(vb.target.as_str(), |k| k.target.as_str());
        let at = if left && vb.constraint.is_some() {
            skel.mirror.get(target).map_or(target, String::as_str)
        } else {
            target
        };
        let ms = (fb.model[0] * fb.model[0] + fb.model[1] * fb.model[1] + fb.model[2] * fb.model[2]).sqrt();
        let Some((tloc, trot, ts)) = bone_follow(fb, skel, at, left) else {
            warn_once(format!("virtual bone {name}: the view has no bone {at}"));
            return None;
        };
        let nodes = vb.modify.as_deref().unwrap_or(&[]);
        let n = (nodes.len() == 1 && vb.constraint.is_none()).then(|| &nodes[0]);
        if nodes.len() > 1 || (vb.constraint.is_some() && !nodes.is_empty()) {
            warn_once(format!("virtual bone {name}: its ModifyBone nodes not applied"));
        }
        let v3 = |v: &[f64]| [0, 1, 2].map(|k| v.get(k).copied().unwrap_or(0.0));
        let d2r = std::f64::consts::PI / 180.0;
        let world_rot =
            n.filter(|n| n.rotation_mode == "BMM_Replace" && n.rotation_space == "BCS_WorldSpace").map(|n| {
                let r = v3(&n.rotation);
                rotation_rows(r[0] * d2r, r[1] * d2r, r[2] * d2r)
            });
        let scale = n.filter(|n| n.scale_mode == "BMM_Replace").and_then(|n| match n.scale_space.as_str() {
            "BCS_WorldSpace" => Some(v3(&n.scale)),
            "BCS_ComponentSpace" => Some(v3(&n.scale).map(|v| v * ms)),
            _ => None,
        });
        let add = n
            .filter(|n| n.translation_mode == "BMM_Additive" && n.translation_space == "BCS_BoneSpace")
            .map(|n| v3(&n.translation));
        if let Some(n) = n {
            let odd = n.alpha != 1.0
                || (add.is_none() && n.translation_mode != "BMM_Ignore")
                || (world_rot.is_none() && n.rotation_mode != "BMM_Ignore")
                || (scale.is_none() && n.scale_mode != "BMM_Ignore");
            if odd {
                warn_once(format!("virtual bone {name}: {} not applied in full", n.node));
            }
        }
        let s = match scale {
            Some(s) => s,
            None if vb.constraint.is_some() => ts.map(|v| ms * sign1(v)),
            None => ts,
        };
        let bone = scale_rows(world_rot.as_ref().unwrap_or(&trot), s);
        let sock_l =
            sk.map_or(IDENT, |s| scale_rows(&rotation_rows(s.rot[0] * d2r, s.rot[1] * d2r, s.rot[2] * d2r), s.scale));
        let sock_w = compose_rows(&sock_l, &bone);
        let l = compose_rows(&rel.rot, &sock_w);
        let (a, cc) = (mul_row(rel.loc, &sock_w), mul_row(sk.map_or([0.0; 3], |s| s.loc), &bone));
        let dd = add.map_or([0.0; 3], |v| mul_row(v, &scale_rows(&trot, ts)));
        let sc: V3 = std::array::from_fn(|k| {
            let h = (l[3 * k] * l[3 * k] + l[3 * k + 1] * l[3 * k + 1] + l[3 * k + 2] * l[3 * k + 2]).sqrt();
            let h = if h == 0.0 || h.is_nan() { 1.0 } else { h };
            h * sign1(s[k] * sk.map_or(1.0, |q| q.scale[k]))
        });
        Some(Xform {
            loc: [0, 1, 2].map(|k| a[k] + cc[k] + dd[k] + tloc[k]),
            rot: std::array::from_fn(|j| l[j] / sc[j / 3]),
            scale: [0, 1, 2].map(|k| sc[k] * rel.scale[k]),
        })
    }
}

/// `boneUE` of fighter bone `name` as last drawn: its world rows (with scale) and location, UE cm.
fn bone_ue(b: &FighterBones, name: &str) -> Option<(Rows, V3)> {
    let i = b.names.iter().position(|n| n == name)?;
    let m = math::to_ue(&math::mul(&b.model, b.cs.get(i)?));
    Some(([m[0], m[1], m[2], m[4], m[5], m[6], m[8], m[9], m[10]], [m[12], m[13], m[14]]))
}

/// `boneFlipRows` (`?? FLIP_ROWS.Y`).
fn flip_rows(skel: &Skeleton, bone: &str) -> Rows {
    match skel.flip.as_ref().and_then(|f| f.get(bone)).map_or("Y", String::as_str) {
        "X" => [-1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0],
        "Z" => [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, -1.0],
        "XYZ" => [-1.0, 0.0, 0.0, 0.0, -1.0, 0.0, 0.0, 0.0, -1.0],
        _ => [1.0, 0.0, 0.0, 0.0, -1.0, 0.0, 0.0, 0.0, 1.0],
    }
}

fn mul_row(v: V3, m: &Rows) -> V3 {
    [0, 1, 2].map(|c| v[0] * m[c] + v[1] * m[3 + c] + v[2] * m[6 + c])
}

fn scale_rows(r: &Rows, s: V3) -> Rows {
    std::array::from_fn(|j| r[j] * s[j / 3])
}

fn sign1(x: f64) -> f64 {
    if x < 0.0 { -1.0 } else { 1.0 }
}

/// `boneFollow(skel, bones, bone, 0, IDENT, 1, left)`: the build's bone `bone` (facing left the view's mirror partner
/// with the bone's flip in front): location, rotation rows, scale.
// Unverified: a bone the view keeps unmirrored (keptBy) is not known here (FighterBones): taken as mirrored.
fn bone_follow(b: &FighterBones, skel: &Skeleton, bone: &str, left: bool) -> Option<(V3, Rows, V3)> {
    let no_node = skel.mirror_node == Some(false);
    let name = if left && !no_node { skel.mirror.get(bone).map_or(bone, String::as_str) } else { bone };
    let (wl, wloc) = bone_ue(b, name)?;
    let parent = if left && !no_node { compose_rows(&flip_rows(skel, bone), &wl) } else { wl };
    let sk = skel.sockets.get(bone);
    let sock_l = sk.map_or(IDENT, |s| {
        let d = std::f64::consts::PI / 180.0;
        scale_rows(&rotation_rows(s.rot[0] * d, s.rot[1] * d, s.rot[2] * d), s.scale)
    });
    let l = compose_rows(&sock_l, &parent);
    let c = mul_row(sk.map_or([0.0; 3], |s| s.loc), &parent);
    let sc: V3 = std::array::from_fn(|k| {
        let h = (l[3 * k] * l[3 * k] + l[3 * k + 1] * l[3 * k + 1] + l[3 * k + 2] * l[3 * k + 2]).sqrt();
        h * sk.map_or(1.0, |s| sign1(s.scale[k]))
    });
    Some(([0, 1, 2].map(|k| c[k] + wloc[k]), std::array::from_fn(|j| l[j] / sc[j / 3]), sc))
}
