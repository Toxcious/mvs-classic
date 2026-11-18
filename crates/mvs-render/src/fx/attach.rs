//! Where an attached component sits after each step: on a fighter's mesh component, on a socket or bone of its
//! pose, on its root (socketFollow, rootFollow;
//! boneFollow, boneUE, mirrorPlayVfx).

use std::collections::{HashMap, HashSet};
use std::sync::{Mutex, OnceLock};

use mvs_data::fighter::{Skeleton, VirtualBone};
use mvs_data::value::{Props, Value};
use mvs_vfx::Attachment;
use mvs_vfx::math::{IDENTITY, Rows, V3, compose_rows, rotation_rows};
use mvs_view::MeshPose;
use mvs_view::math::{Mat4, invert, mul};
use mvs_view::mesh::MeshInfo;

pub const D2R: f64 = std::f64::consts::PI / 180.0;

/// An Attachment from rotation rows.
pub fn att(loc: V3, r: &Rows, scale: Option<V3>) -> Attachment {
    Attachment { loc, rot: [[r[0], r[1], r[2]], [r[3], r[4], r[5]], [r[6], r[7], r[8]]], scale }
}

/// `mulRow`: v times the rows.
pub fn mul_row(v: V3, m: &Rows) -> V3 {
    [0, 1, 2].map(|c| v[0] * m[c] + v[1] * m[3 + c] + v[2] * m[6 + c])
}

/// `transposeRows`.
pub fn transpose(m: &Rows) -> Rows {
    [m[0], m[3], m[6], m[1], m[4], m[7], m[2], m[5], m[8]]
}

/// The mesh's rotation rows (`meshRows`): yaw -90 facing right, +90 left.
pub fn mesh_rows(left: bool) -> Rows {
    let h = std::f64::consts::FRAC_PI_2;
    rotation_rows(0.0, if left { h } else { -h }, 0.0)
}

/// A property struct's `X`, `Y`, `Z`, `d` where unset.
pub fn xyz(v: Option<&Value>, d: f64) -> V3 {
    let o = v.and_then(Value::as_obj);
    let g = |k: &str| o.and_then(|o| o.get(k)).and_then(Value::as_f64).unwrap_or(d);
    [g("X"), g("Y"), g("Z")]
}

/// A rotator struct's rows.
pub fn rot_rows(v: Option<&Value>) -> Rows {
    let o = v.and_then(Value::as_obj);
    let g = |k: &str| o.and_then(|o| o.get(k)).and_then(Value::as_f64).unwrap_or(0.0);
    rotation_rows(g("Pitch") * D2R, g("Yaw") * D2R, g("Roll") * D2R)
}

pub fn num(p: &Props, k: &str) -> Option<f64> {
    p.get(k).and_then(Value::as_f64)
}

pub fn text<'a>(p: &'a Props, k: &str) -> Option<&'a str> {
    p.get(k).and_then(Value::as_str)
}

pub fn flag(p: &Props, k: &str) -> bool {
    p.get(k).and_then(Value::as_bool).unwrap_or(false)
}

/// `swapLR`: L_ and R_ exchanged (case-sensitive, as the build's Replace).
pub fn swap_lr(n: &str) -> String {
    n.replace("L_", "SWAP_").replace("R_", "L_").replace("SWAP_", "R_")
}

/// A body mesh's skeleton for the effects: its bones' reference component-space matrices and names.
pub struct Rig {
    ref_cs: Vec<Mat4>,
    index: HashMap<String, usize>,
    /// A socket or bone name in lower case -> the skeleton's own spelling (sockets over bones, as
    /// GetSocketTransform looks them up: FName equality ignores case).
    names: HashMap<String, String>,
    /// Per bone: a bone the view keeps unmirrored (its keptBy, mvs_view::view::kept_bones); empty: none.
    kept: Vec<bool>,
}

impl Rig {
    pub fn new(info: &MeshInfo, skel: &Skeleton) -> Rig {
        let ref_cs = info.inv_ref.iter().map(invert).collect();
        let bones = info.bones();
        let index = bones.iter().enumerate().map(|(i, b)| (b.clone(), i)).collect();
        let mut names = HashMap::new();
        for b in bones {
            names.entry(b.to_lowercase()).or_insert_with(|| b.clone());
        }
        let mut socks: HashMap<String, String> = HashMap::new();
        for k in skel.sockets.keys() {
            socks.entry(k.to_lowercase()).or_insert_with(|| k.clone());
        }
        names.extend(socks);
        Rig { ref_cs, index, names, kept: Vec::new() }
    }

    /// The bones the view keeps unmirrored (mvs_view::view::kept_bones).
    pub fn with_kept(mut self, kept: Vec<bool>) -> Rig {
        self.kept = kept;
        self
    }

    /// `skelName`: a socket or bone name in the skeleton's spelling; an unknown name stays as given.
    pub fn name(&self, n: &str) -> String {
        self.names.get(&n.to_lowercase()).cloned().unwrap_or_else(|| n.to_string())
    }

    pub fn has_bone(&self, n: &str) -> bool {
        self.index.contains_key(n)
    }
}

/// A fighter's body at the latest tick: the mesh component's place and its bones' world transforms.
#[derive(Clone, Debug, Default)]
pub struct Body {
    /// The mesh component's location (UE cm) and world scale (the view's worldZ and worldScale).
    pub loc: V3,
    pub scale: f64,
    pub left: bool,
    /// Per bone of the mesh: its world rows (UE axes, row vectors: boneUE's L) and location.
    pub bones: Vec<(Rows, V3)>,
}

/// A body's mesh component place: its location, world scale and facing (`Body`'s, without the bones).
#[derive(Clone, Copy, Debug)]
pub struct BodyPlace {
    pub loc: V3,
    pub scale: f64,
    pub left: bool,
}

fn mat_of(a: &[f32; 12]) -> Mat4 {
    let mut m = mvs_view::math::IDENTITY;
    for r in 0..3 {
        for c in 0..4 {
            m[c * 4 + r] = f64::from(a[r * 4 + c]);
        }
    }
    m
}

impl Body {
    /// From the body mesh's pose: bone world = LocalToWorld x ReferenceToLocal x the reference component-space
    /// matrix (the skinning matrix undone).
    pub fn of(mesh: &MeshPose, rig: &Rig, left: bool) -> Body {
        let mut b = Body::default();
        b.set(&mesh.to_world, &mesh.bones, rig, left);
        b
    }

    /// `of` into this body's buffers: from a mesh's LocalToWorld and skinning matrices.
    pub fn set(&mut self, to_world: &[f32; 12], skinning: &[[f32; 12]], rig: &Rig, left: bool) {
        let w = mat_of(to_world);
        self.scale = (w[0] * w[0] + w[1] * w[1] + w[2] * w[2]).sqrt();
        self.loc = [w[12], w[13], w[14]];
        self.left = left;
        self.bones.clear();
        self.bones.extend(skinning.iter().zip(&rig.ref_cs).map(|(s, r)| {
            let m = mul(&mul(&w, &mat_of(s)), r);
            let l = [m[0], m[1], m[2], m[4], m[5], m[6], m[8], m[9], m[10]];
            (l, [m[12], m[13], m[14]])
        }));
    }

    /// Its place alone (no bones): what an effect spawned at the mesh reads.
    pub fn place(&self) -> BodyPlace {
        BodyPlace { loc: self.loc, scale: self.scale, left: self.left }
    }

    /// `meshAttach`: a relative location (and rotation) under the mesh component.
    pub fn mesh_attach(&self, rel: V3, rel_rows: Option<&Rows>) -> (V3, Rows) {
        let rot = mesh_rows(self.left);
        let mut loc = self.loc;
        let d = mul_row(rel, &rot);
        for c in 0..3 {
            loc[c] += self.scale * d[c];
        }
        (loc, rel_rows.map_or(rot, |r| compose_rows(r, &rot)))
    }
}

/// `boneFlipRows` (`?? FLIP_ROWS.Y`): the rows of a bone's BoneFlipAxis; Y for a bone the table does not list
/// and for None (a bone the node writes back: the view's keptBy bones take neither partner nor flip, the rest are
/// taken as mirrored).
fn flip_rows(skel: &Skeleton, bone: &str) -> Rows {
    let axis = skel.flip.as_ref().and_then(|f| f.get(bone)).map_or("Y", String::as_str);
    match axis {
        "X" => [-1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0],
        "Y" | "None" => [1.0, 0.0, 0.0, 0.0, -1.0, 0.0, 0.0, 0.0, 1.0],
        "Z" => [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, -1.0],
        "XYZ" => [-1.0, 0.0, 0.0, 0.0, -1.0, 0.0, 0.0, 0.0, -1.0],
        a => panic!("mirror table: BoneFlipAxis {a} ({bone})"),
    }
}

/// `warnOnce`.
pub(crate) fn warn_once(what: String) {
    static W: OnceLock<Mutex<HashSet<String>>> = OnceLock::new();
    let mut w = W.get_or_init(|| Mutex::new(HashSet::new())).lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    if !w.contains(&what) {
        eprintln!("warning: {what}");
        w.insert(what);
    }
}

/// `Math.sign(x) || 1`.
fn sign1(x: f64) -> f64 {
    if x < 0.0 { -1.0 } else { 1.0 }
}

/// SpawnEmitterAttached(..., Mesh, Socket, loc, rot, scale, KeepRelativeOffset) on a socket or bone of the body
/// (`socketFollow`): a bone the view has (`boneFollow`), or a virtual bone (`virtualSocketFollow`).
#[derive(Clone, Debug)]
pub struct SocketFollow(Kind);

// (the caller boxes a SocketFollow: the bone case stays inline)
#[allow(clippy::large_enum_variant)]
#[derive(Clone, Debug)]
enum Kind {
    Bone(BoneFollow),
    Virtual(Box<VirtualFollow>),
}

impl SocketFollow {
    /// `socket` in the skeleton's spelling (Rig::name).
    pub fn new(skel: &Skeleton, socket: &str, loc: V3, rot: &Rows, scale: V3) -> SocketFollow {
        // `virtualBoneOf`
        let bone = skel.sockets.get(socket).map_or(socket, |s| s.bone.as_str());
        match skel.virtual_bones.as_ref().and_then(|v| v.get(bone)) {
            Some(vb) => SocketFollow(Kind::Virtual(Box::new(VirtualFollow::new(skel, socket, vb, loc, rot, scale)))),
            None => SocketFollow(Kind::Bone(BoneFollow::new(skel, socket, loc, rot, scale))),
        }
    }

    /// The world transform on `body`; None while the bone is not in the pose (the caller's mesh origin then).
    pub fn eval(&self, body: &Body, rig: &Rig) -> Option<(V3, Rows, V3)> {
        match &self.0 {
            Kind::Bone(b) => b.eval(body, rig),
            Kind::Virtual(v) => Some(v.eval(body, rig)),
        }
    }

    /// boneFollow's 'copy': attached to a pose copy (a tagged sub-mesh) rather than CharacterMesh0. On a pawn without a
    /// mirror node (`asView`) the copy is what the view draws: the view's bone as it is, and the copy's (1, -1, 1)
    /// scale facing left reaches the child (its Scale3D Y negated, the rotation kept proper). On a pawn with a node a
    /// copy is the main mesh, and a virtual bone's socket takes no component.
    // Unverified: how a particle component draws under that scale is not read in the exe; the rule for a
    // copy on a pawn with a node is not derived.
    pub fn copy(mut self) -> SocketFollow {
        if let Kind::Bone(b) = &mut self.0 {
            b.as_view = b.no_node;
        }
        self
    }
}

/// `boneFollow` on the main mesh. The view reflects the right-facing pose; facing left the build's bone B is F_B x
/// the view's mirror partner of B. A bone the view keeps (keptBy) and every bone of a pawn without a mirror node
/// (the skeleton's mirror_node false, C017: CharacterMesh0 holds the right-facing pose turned with the actor) is the
/// view's bone of the same name reflected in depth (UE X) about the view's root, with no partner and no flip.
#[derive(Clone, Debug)]
struct BoneFollow {
    bone: String,
    partner: String,
    flip: Rows,
    no_node: bool,
    /// `asView`: a copy on a pawn without a mirror node (SocketFollow::copy).
    as_view: bool,
    sock_l: Rows,
    sock_loc: V3,
    sign: V3,
    rel_l: Rows,
    loc: V3,
}

impl BoneFollow {
    fn new(skel: &Skeleton, socket: &str, loc: V3, rot: &Rows, scale: V3) -> BoneFollow {
        let sk = skel.sockets.get(socket);
        let bone = sk.map_or(socket, |s| s.bone.as_str()).to_string();
        let sock_l = sk.map_or(IDENTITY, |s| {
            let r = rotation_rows(s.rot[0] * D2R, s.rot[1] * D2R, s.rot[2] * D2R);
            scale_rows(&r, s.scale)
        });
        let sign = [0, 1, 2].map(|k| {
            let v = scale[k] * sk.map_or(1.0, |s| s.scale[k]);
            if v < 0.0 { -1.0 } else { 1.0 }
        });
        BoneFollow {
            partner: skel.mirror.get(&bone).cloned().unwrap_or_else(|| bone.clone()),
            flip: flip_rows(skel, &bone),
            no_node: skel.mirror_node == Some(false),
            as_view: false,
            bone,
            sock_l,
            sock_loc: sk.map_or([0.0; 3], |s| s.loc),
            sign,
            rel_l: scale_rows(rot, scale),
            loc,
        }
    }

    fn eval(&self, body: &Body, rig: &Rig) -> Option<(V3, Rows, V3)> {
        let own = if body.left && !self.as_view { rig.index.get(&self.bone).copied() } else { None };
        let kept = own.filter(|&b| self.no_node || rig.kept.get(b).copied().unwrap_or(false));
        let b = match kept {
            Some(b) => b,
            None => *rig.index.get(if body.left && !self.no_node { &self.partner } else { &self.bone })?,
        };
        let &(mut wl, mut wloc) = body.bones.get(b)?;
        if kept.is_some() {
            mvs_view::actors::reflect_depth(&mut wl, &mut wloc);
        }
        let parent_l = if body.left && kept.is_none() && !self.no_node { compose_rows(&self.flip, &wl) } else { wl };
        let sock_w = compose_rows(&self.sock_l, &parent_l);
        let l = compose_rows(&self.rel_l, &sock_w);
        let (a, c) = (mul_row(self.loc, &sock_w), mul_row(self.sock_loc, &parent_l));
        let mut sc = [0, 1, 2].map(|k| mvs_view::anim_nodes::hypot(&l[3 * k..3 * k + 3]) * self.sign[k]);
        if self.as_view && body.left {
            sc[1] = -sc[1];
        }
        let loc = [0, 1, 2].map(|k| a[k] + c[k] + wloc[k]);
        let mut rot = [0.0; 9];
        for j in 0..9 {
            rot[j] = l[j] / sc[j / 3];
        }
        Some((loc, rot, sc))
    }
}

/// `scaleRows`: each row times its axis' scale.
pub fn scale_rows(r: &Rows, s: V3) -> Rows {
    let mut o = *r;
    for j in 0..9 {
        o[j] *= s[j / 3];
    }
    o
}

/// A socket on a virtual bone of the fighter's Skeleton (with
/// socketFollow's scale on the result). USkeleton.VirtualBones: a bone under SourceBoneName with TargetBoneName's
/// component-space transform; the view has no such bone, so it is the target's (boneFollow), under the AnimBP's one
/// ModifyBone node on it as FAnimNode_ModifyBone gives it:
///   scale BMM_Replace: BCS_ComponentSpace -> Scale x the mesh component's world scale; BCS_WorldSpace -> Scale;
///   rotation BMM_Replace in BCS_WorldSpace -> the node's Rotation as the world rotation;
///   translation BMM_Additive in BCS_BoneSpace -> the node's Translation under the target's rotation and scale;
///   BMM_Ignore: the target's.
/// Anything else is not applied and warns. A Constraint node (Bugs's VB PropPickup onto fml_un_R_PropOffset) puts
/// the bone on its target's place, facing left the target's mirror twin; the bone's scale stays the component's with
/// the target's signs.
// Unverified: the ModifyBone channels are stock 4.26 (not read in the exe), the component-space Replace excepted;
// the translation channel's order and AddToTranslation are not read; a Constraint's target is its right-facing node
// reflected with the view; Root's scale is taken as 1; the mirror node is taken to leave the virtual bone on its
// target.
#[derive(Clone, Debug)]
struct VirtualFollow {
    at_right: BoneFollow,
    at_left: Option<BoneFollow>,
    constraint: bool,
    world_rot: Option<Rows>,
    /// The node's Scale and whether it is in world space.
    scale: Option<(V3, bool)>,
    add: Option<V3>,
    sock_l: Rows,
    sock_loc: V3,
    sock_scale: Option<V3>,
    rot: Rows,
    loc: V3,
    /// socketFollow's scale, on the result.
    extra: V3,
}

impl VirtualFollow {
    fn new(skel: &Skeleton, socket: &str, vb: &VirtualBone, loc: V3, rot: &Rows, extra: V3) -> VirtualFollow {
        let sk = skel.sockets.get(socket);
        let name = sk.map_or(socket, |s| s.bone.as_str());
        let at = vb.constraint.as_ref().map_or(vb.target.as_str(), |c| c.target.as_str());
        const IDENT: Rows = [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0];
        let at_right = BoneFollow::new(skel, at, [0.0; 3], &IDENT, [1.0; 3]);
        let at_left = vb.constraint.as_ref().map(|_| {
            let twin = skel.mirror.get(at).map_or(at, String::as_str);
            BoneFollow::new(skel, twin, [0.0; 3], &IDENT, [1.0; 3])
        });
        let nodes = vb.modify.as_deref().unwrap_or(&[]);
        if vb.constraint.is_some() && !nodes.is_empty() {
            warn_once(format!(
                "virtual bone {name}: a Constraint and ModifyBone nodes, the ModifyBone nodes not applied"
            ));
        }
        if nodes.len() > 1 {
            warn_once(format!("virtual bone {name}: {} ModifyBone nodes, none applied", nodes.len()));
        }
        let (mut world_rot, mut scale, mut add) = (None, None, None);
        if nodes.len() == 1 && vb.constraint.is_none() {
            let n = &nodes[0];
            let v3 = |v: &[f64]| [0, 1, 2].map(|k| v.get(k).copied().unwrap_or(0.0));
            let mut odd = Vec::new();
            if n.alpha != 1.0 {
                odd.push(format!("alpha {}", n.alpha));
            }
            if n.translation_mode == "BMM_Additive" && n.translation_space == "BCS_BoneSpace" {
                add = Some(v3(&n.translation));
            } else if n.translation_mode != "BMM_Ignore" {
                odd.push(format!("translation {} {}", n.translation_mode, n.translation_space));
            }
            if n.rotation_mode == "BMM_Replace" && n.rotation_space == "BCS_WorldSpace" {
                let r = v3(&n.rotation);
                world_rot = Some(rotation_rows(r[0] * D2R, r[1] * D2R, r[2] * D2R));
            } else if n.rotation_mode != "BMM_Ignore" {
                odd.push(format!("rotation {} {}", n.rotation_mode, n.rotation_space));
            }
            if n.scale_mode == "BMM_Replace"
                && (n.scale_space == "BCS_WorldSpace" || n.scale_space == "BCS_ComponentSpace")
            {
                scale = Some((v3(&n.scale), n.scale_space == "BCS_WorldSpace"));
            } else if n.scale_mode != "BMM_Ignore" {
                odd.push(format!("scale {} {}", n.scale_mode, n.scale_space));
            }
            if !odd.is_empty() {
                warn_once(format!("virtual bone {name}: {} not applied in full ({})", n.node, odd.join(", ")));
            }
        }
        VirtualFollow {
            at_right,
            at_left,
            constraint: vb.constraint.is_some(),
            world_rot,
            scale,
            add,
            sock_l: sk
                .map_or(IDENT, |s| scale_rows(&rotation_rows(s.rot[0] * D2R, s.rot[1] * D2R, s.rot[2] * D2R), s.scale)),
            sock_loc: sk.map_or([0.0; 3], |s| s.loc),
            sock_scale: sk.map(|s| s.scale),
            rot: *rot,
            loc,
            extra,
        }
    }

    fn eval(&self, body: &Body, rig: &Rig) -> (V3, Rows, V3) {
        let at = if body.left { self.at_left.as_ref().unwrap_or(&self.at_right) } else { &self.at_right };
        // (the target bone not in the pose: socketFollow's missing, meshAttach(i, [0, 0, 0]) with no scale)
        let (tloc, trot, ts) = at.eval(body, rig).unwrap_or_else(|| {
            warn_once(format!("virtual bone: the view has no bone {}", at.bone));
            let (l, r) = body.mesh_attach([0.0; 3], None);
            (l, r, [1.0; 3])
        });
        let ms = body.scale;
        let s = match self.scale {
            Some((s, true)) => s,
            Some((s, false)) => s.map(|v| v * ms),
            None if self.constraint => ts.map(|v| ms * sign1(v)),
            None => ts,
        };
        let bone = scale_rows(self.world_rot.as_ref().unwrap_or(&trot), s);
        let sock_w = compose_rows(&self.sock_l, &bone);
        let l = compose_rows(&self.rot, &sock_w);
        let (a, c) = (mul_row(self.loc, &sock_w), mul_row(self.sock_loc, &bone));
        // the node's bone-space translation under the target's own rotation and scale (the input bone's)
        let d = self.add.map_or([0.0; 3], |v| mul_row(v, &scale_rows(&trot, ts)));
        let sc = [0, 1, 2].map(|k| {
            let h = mvs_view::anim_nodes::hypot(&l[3 * k..3 * k + 3]);
            let h = if h == 0.0 || h.is_nan() { 1.0 } else { h };
            h * sign1(s[k] * self.sock_scale.map_or(1.0, |q| q[k]))
        });
        let loc = [0, 1, 2].map(|k| a[k] + c[k] + d[k] + tloc[k]);
        let mut rot = [0.0; 9];
        for j in 0..9 {
            rot[j] = l[j] / sc[j / 3];
        }
        (loc, rot, [0, 1, 2].map(|k| sc[k] * self.extra[k]))
    }
}

/// AnimNotify_PlayVFX's Mirror(FacingLeft) (`mirrorPlayVfx`): the socket, location, rotation and scale.
pub fn mirror_play_vfx(p: &Props, left: bool) -> (Option<String>, V3, Rows, V3) {
    let mirror = left && flag(p, "ApplyMirroring");
    let obj = |k: &str| p.get(k).and_then(Value::as_obj);
    let on = |o: Option<&Props>, k: &str| o.is_some_and(|o| flag(o, k));
    let (ma, lm, rm) = (obj("MirrorAxis"), obj("LocationMirrorAxisOverride"), obj("RotationMirrorAxisOverride"));
    let any_axis = on(ma, "MirrorX") || on(ma, "MirrorY") || on(ma, "MirrorZ");
    let mut loc = xyz(p.get("Location"), 0.0);
    let mut rot = rot_rows(p.get("Rotation"));
    let mut scale = xyz(p.get("VFX Scale"), 1.0);
    let socket = text(p, "SocketName")
        .map(|s| if mirror && !flag(p, "BlockAttachSocketMirror") { swap_lr(s) } else { s.to_string() });
    let pi = std::f64::consts::PI;
    let sgn = |o: Option<&Props>, k: &str| if on(o, k) { -1.0 } else { 1.0 };
    let half = |o: Option<&Props>, k: &str| if on(o, k) { pi } else { 0.0 };
    if mirror {
        if flag(p, "LocationMirroringVersionB") {
            loc = [loc[0] * sgn(lm, "MirrorX"), loc[1] * sgn(lm, "MirrorY"), loc[2] * sgn(lm, "MirrorZ")];
        } else {
            loc = mul_row(loc, &rotation_rows(half(lm, "MirrorY"), half(lm, "MirrorZ"), half(lm, "MirrorX")));
        }
        if any_axis {
            scale = [scale[0] * sgn(ma, "MirrorX"), scale[1] * sgn(ma, "MirrorY"), scale[2] * sgn(ma, "MirrorZ")];
        } else {
            rot = compose_rows(&rot, &rotation_rows(half(rm, "MirrorY"), half(rm, "MirrorZ"), half(rm, "MirrorX")));
        }
    }
    if flag(p, "ApplyMirrorOffsets") {
        let o = xyz(p.get(if left { "FaceLeftOffset" } else { "FaceRightOffset" }), 0.0);
        loc = [loc[0] + o[0], loc[1] + o[1], loc[2] + o[2]];
    }
    (socket, loc, rot, scale)
}

/// AnimNotify_PlayVFX's particle (`playVfxParticle`): VFX_Skin's value for the skin (the map's first when it has
/// none of it), or the colour select.
pub fn play_vfx_particle(p: &Props, skin: &str, color_index: u8) -> Option<String> {
    if flag(p, "UseSkinVFX") {
        let map = p.get("VFX_Skin").and_then(Value::as_arr).unwrap_or(&[]);
        let key_of = |e: &Value| {
            let k = e.as_obj().and_then(|o| text(o, "Key"))?;
            Some(k.trim_end_matches('\'').rsplit('.').next().unwrap_or(k).to_string())
        };
        let named = map.iter().find(|e| key_of(e).as_deref() == Some(skin));
        return named.or(map.first()).and_then(|e| e.as_obj()).and_then(|o| text(o, "Value")).map(str::to_string);
    }
    let team = match color_index {
        0 => "ParticleTeam0",
        1 if flag(p, "TeamVFX?") => "ParticleTeam1",
        1 => "ParticleTeam0",
        _ => return None,
    };
    text(p, team).map(str::to_string)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn swaps_sides() {
        assert_eq!(swap_lr("L_Hand"), "R_Hand");
        assert_eq!(swap_lr("fml_R_x_L_y"), "fml_L_x_R_y");
    }

    #[test]
    fn mesh_attach_turns_with_the_facing() {
        let b = Body { loc: [0.0, -100.0, 50.0], scale: 2.0, left: false, bones: vec![] };
        let (loc, _) = b.mesh_attach([10.0, 0.0, 5.0], None);
        // facing right the mesh yaws -90: its X is the world -Y
        assert!((loc[0]).abs() < 1e-9 && (loc[1] + 120.0).abs() < 1e-9 && (loc[2] - 60.0).abs() < 1e-9);
    }

    const ID: Rows = [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0];

    fn skel(json: &str) -> Skeleton {
        serde_json::from_str(json).expect("a skeleton")
    }

    /// Bones B (0) and its mirror partner C (1); B at (10, 20, 30), C at (1, 2, 3), both unturned.
    fn rig_body(kept: Vec<bool>, left: bool) -> (Rig, Body) {
        let index = [("B".to_string(), 0), ("C".to_string(), 1)].into_iter().collect();
        let rig = Rig { ref_cs: vec![], index, names: HashMap::new(), kept };
        let bones = vec![(ID, [10.0, 20.0, 30.0]), (ID, [1.0, 2.0, 3.0])];
        (rig, Body { loc: [0.0; 3], scale: 1.0, left, bones })
    }

    fn close(a: &[f64], b: &[f64]) -> bool {
        a.iter().zip(b).all(|(x, y)| (x - y).abs() < 1e-9)
    }

    #[test]
    fn a_pawn_without_a_mirror_node_reflects_its_own_bone() {
        let sk = skel(r#"{"sockets":{},"mirrorAxis":null,"mirror":{"B":"C"},"mirrorNode":false}"#);
        let f = SocketFollow::new(&sk, "B", [0.0; 3], &ID, [1.0; 3]);
        let (rig, body) = rig_body(vec![], true);
        let (loc, rot, _) = f.eval(&body, &rig).expect("the bone");
        assert!(close(&loc, &[-10.0, 20.0, 30.0]));
        assert!(close(&rot, &[-1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0]));
        // facing right: the bone as it is
        let (rig, body) = rig_body(vec![], false);
        assert!(close(&f.eval(&body, &rig).expect("the bone").0, &[10.0, 20.0, 30.0]));
    }

    #[test]
    fn a_copy_on_a_pawn_without_a_mirror_node_is_the_view_bone() {
        let sk = skel(r#"{"sockets":{},"mirrorAxis":null,"mirror":{"B":"C"},"mirrorNode":false}"#);
        let f = SocketFollow::new(&sk, "B", [0.0; 3], &ID, [1.0; 3]).copy();
        // facing left: the view's bone as it is, the copy's Y scale negated and the rows kept proper
        let (rig, mut body) = rig_body(vec![], true);
        body.bones[0].0 = [1.0, 0.0, 0.0, 0.0, -1.0, 0.0, 0.0, 0.0, 1.0];
        let (loc, rot, sc) = f.eval(&body, &rig).expect("the bone");
        assert!(close(&loc, &[10.0, 20.0, 30.0]));
        assert!(close(&sc, &[1.0, -1.0, 1.0]));
        assert!(close(&rot, &ID));
        // facing right: the bone as it is, no sign
        let (rig, body) = rig_body(vec![], false);
        let (loc, _, sc) = f.eval(&body, &rig).expect("the bone");
        assert!(close(&loc, &[10.0, 20.0, 30.0]) && close(&sc, &[1.0; 3]));
    }

    #[test]
    fn a_kept_bone_takes_no_partner_and_no_flip() {
        let sk = skel(r#"{"sockets":{},"mirrorAxis":null,"mirror":{"B":"C"},"flip":{"B":"None"}}"#);
        let f = SocketFollow::new(&sk, "B", [0.0; 3], &ID, [1.0; 3]);
        let (rig, body) = rig_body(vec![true, false], true);
        let (loc, rot, _) = f.eval(&body, &rig).expect("the bone");
        assert!(close(&loc, &[-10.0, 20.0, 30.0]));
        assert!(close(&rot, &[-1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0]));
        // not kept: the partner under the Y flip (BoneFlipAxis None taken as Y)
        let (rig, body) = rig_body(vec![false, false], true);
        let (loc, rot, _) = f.eval(&body, &rig).expect("the bone");
        assert!(close(&loc, &[1.0, 2.0, 3.0]));
        assert!(close(&rot, &[1.0, 0.0, 0.0, 0.0, -1.0, 0.0, 0.0, 0.0, 1.0]));
    }

    #[test]
    fn a_virtual_bone_follows_its_target_under_its_modify_node() {
        let sk = skel(
            r#"{"sockets":{"S":{"bone":"VB B","loc":[0,0,0],"rot":[0,0,0],"scale":[1,1,1]}},"mirrorAxis":null,
            "mirror":{"B":"C"},"virtualBones":{"VB B":{"source":"B","target":"B","modify":[{"node":"ModifyBone",
            "alpha":1,"translation":[0,0,0],"translationMode":"BMM_Ignore","translationSpace":"BCS_ComponentSpace",
            "rotation":[0,0,0],"rotationMode":"BMM_Replace","rotationSpace":"BCS_WorldSpace","scale":[2,2,2],
            "scaleMode":"BMM_Replace","scaleSpace":"BCS_ComponentSpace"}]}}}"#,
        );
        let f = SocketFollow::new(&sk, "S", [1.0, 0.0, 0.0], &ID, [1.0; 3]);
        let (rig, mut body) = rig_body(vec![], false);
        body.bones[0].0 = [0.0, 1.0, 0.0, -1.0, 0.0, 0.0, 0.0, 0.0, 1.0];
        body.scale = 1.5;
        let (loc, rot, sc) = f.eval(&body, &rig).expect("the bone");
        // the world rotation replaced (identity), the scale 2 x the component's 1.5: the offset is 3 along world X
        assert!(close(&rot, &ID));
        assert!(close(&sc, &[3.0, 3.0, 3.0]));
        assert!(close(&loc, &[13.0, 20.0, 30.0]));
    }
}
