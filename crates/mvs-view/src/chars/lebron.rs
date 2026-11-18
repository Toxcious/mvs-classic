//! LeBron James's render module: the entry, the AnimBP's pose hook and the classes' mesh
//! rules.
//!
//! The pose beside the shared one (c016_AnimBP) and the held ball: the pawn's Ball
//! component (the skin's sub-mesh Ball, on the body's pose: UserConstructionScript @0
//! Ball.SetMasterPoseComponent(Mesh)), shown by the sequences' showBall curve and squashed by their
//! squashStretchAmount / squashMultiplier curves.
//!
//! The graph: BlendListByBool (HasBall OR IsInMenu; 0.1 / 0.1 linear) of the two anim sets -> Navigation -> JumpSlot
//! -> DefaultSlot -> three Constraint nodes -> the mirror node -> ModifyBone nodes -> Trail / AnimDynamics. The blend
//! of the two sets is the sim's (Fighter.locoBlend): the view's crossfade draws it.
//!
//! The curves. The AnimBP reads three float curves of the playing sequences by name (GetCurveValue, crate::curves),
//! by the sequence's name and its time. Its update (BlueprintUpdateAnimation):
//!   @6499 ToggleSubmeshVisibility(showBall, tag Ball, BallVisibile) (function @0-355): when (curve >= 0.5) differs
//!         from BallVisibile, VisibilityManager.SetSubMeshVisibility(Ball, !BallVisibile, true) and the flag flips.
//!         BallVisibile starts false and the component starts hidden (not PrimaryMesh), so the ball is shown while
//!         showBall >= 0.5. A sequence without the curve (every NoBall one; BoxOut, Burst, ChalkToss, Dash) reads 0.
//!   @6657 StretchAmount = squashStretchAmount
//!   @6812 p = squashStretchAmount x squashMultiplier; p > 0.1 -> Squash = (1 / p, 1 / p, 1) @7081, else (1, 1, 1)
//!         @894-937
//! The pawn (Pawn_c016.SetBallSquashAndStretch, from ReceiveTick @2859) writes the ball's materials Direction
//! (fml_un_C_scaler's component-space scale @115), Center (fml_un_C_center's world location @284) and Amount
//! (StretchAmount @338), which c016_Ball_MAT's vertex shader reads (shader map code entry 00032ac2, lines 77-81):
//! P += Direction x dot(P - Center, Direction) x (Amount - 1).
//!
//! The nodes ported:
//!   - Constraint x 3 (fml_un_C_lbrn_neck_a to fml_un_C_neck_a: translation, rotation, scale; OffsetOption None, all
//!     axes, weight 1, alpha 1): the bone's component-space transform becomes the target's, the rotation through the
//!     rotator (the exe's FAnimNode_Constraint). 733 vertices of the head mesh are weighted to the bone.
//!   - ModifyBone (fml_un_C_center: scale BMM_Replace in BCS_BoneSpace = Squash, alpha 1). The ball mesh's 16065
//!     vertices are all weighted to fml_un_C_rotator under it.
//!   - ModifyBone_3 / _5 (head, Face_att: scale Additive from HeadScale): the shared head-scale step.
//!
//! Unverified (stock rules, not read in the exe): the value of a curve in a crossfade (each player's value by its
//! weight, 0 for a player without the curve, with the weight of the view's one crossfade); the update reads the curves
//! of the evaluation before it, so the ball's visibility and Squash follow the pose by one tick, and Amount by one
//! more (the pawn ticks before its mesh); ModifyBone's scale Replace in bone space taken as the bone's local scale
//! times Squash.
//! Deviation: the visibility is kept here: the sim's sub_hidden holds `ball` throughout (nothing in the sim reads the
//! tag).
//! The stretch is the material's own: the renderer runs c016_Ball_MAT's vertex shader, so the view writes the three
//! parameters on the ball part's materials: Amount (`scalars`, each tick), Direction and Center (`vectors`, each
//! frame). Direction is the pose's (x, z, y) scale product (the build's X, Y, Z), Center the bone's UE world place.
//! Unverified: Direction and Center are taken from the pose drawn (Center each frame, between two ticks),
//! where the pawn reads the mesh's sockets at its tick, which hold the tick before's pose; Center is written only
//! while the ball is shown. The custom depth copy takes the stretch where the renderer's depth pass runs the
//! material's own vertex shader (a copy drawn without it would differ).
//! Not ported: ModifyBone_1 / _2 / _4 (virtual bones the view does not hold); the five Trail nodes, the twenty
//! AnimDynamics on the cape bones and the two holster chains (no vertex of the default skin's meshes is weighted to
//! those bones); the mirror node (a left facing is drawn as the reflected view, unverified).
//! A preview actor (BP_CharacterPreview: the podium, the pre-match previews) runs the same toggle (preview_step):
//! SetupInitialVisibility (PRE@8754) hides the Ball, then the AnimBP's update @6499 shows it from the preview
//! montage's showBall (C016_Emote_CharacterSelect_Montage: 1 throughout). The preview runs no pawn tick, so
//! SetBallSquashAndStretch never writes Amount: the ball draws unstretched (unverified: the material's default Amount
//! taken as 1).

use mvs_data::Value;
use mvs_sim::World;
use mvs_sim::fighter::FighterId;

use super::{CharRender, PawnComponents, PoseHook, PoseOf, PreviewOf, PreviewStep, PutVector};
use crate::actors::MeshRule;
use crate::anim::{Clips, HeadScaleNode, Pose};
use crate::curves::CurveTable;
use crate::math::Mat4;
use crate::pose::{MeshMap, Rec, material};
use crate::rotator::through_rotator;
use crate::skins::FighterDraws;
use crate::view::{CharView, PoseCx, PoseRules};

pub(crate) const RENDER: CharRender = CharRender {
    head_scale: &LEBRON_HEAD,
    pose: Some(lebron_pose),
    components: Some(PawnComponents { make: hoop_make, place: hoop_place, reset: None }),
    ..CharRender::GENERIC
};

/// The classes' own mesh drawing, by class name.
pub(crate) const RULES: &[(&str, MeshRule)] = &[];

/// The big head: C016_AnimBP's ModifyBone_3 and ModifyBone_5 (scale Additive, bone space, from HeadScale; Pawn_C016
/// @2794).
const LEBRON_HEAD: [HeadScaleNode; 2] = [
    HeadScaleNode {
        node: "ModifyBone_3",
        bone: "fml_un_C_head",
        factor: 1.0,
        gated: false,
        moves: [0.0; 3],
        scale: true,
    },
    HeadScaleNode {
        node: "ModifyBone_5",
        bone: "fml_un_C_Face_att",
        factor: 1.0,
        gated: false,
        moves: [0.0; 3],
        scale: true,
    },
];

/// The float curves its AnimBP reads by name, in the order the rule indexes them (`LEBRON_CURVES`).
const CURVES: CurveTable = CurveTable::new(&["showBall", "squashStretchAmount", "squashMultiplier"]);
const SHOW: usize = 0;
const AMOUNT: usize = 1;
const MULT: usize = 2;
const N: usize = 3;

/// The sub-mesh's key (and its component's tag).
const BALL: &str = "Ball";
const NECK: &str = "fml_un_C_lbrn_neck_a";
const NECK_TO: &str = "fml_un_C_neck_a";
const CENTER: &str = "fml_un_C_center";
const SCALER: &str = "fml_un_C_scaler";

/// `lebronPose`'s state on one view: the bone indices and the AnimBP's variables, made once a view.
#[derive(Debug)]
struct LebronPose {
    neck: Option<usize>,
    neck_to: Option<usize>,
    center: Option<usize>,
    scaler: Option<usize>,
    ball_part: Option<usize>,
    /// The curves: the playing sequence's (`tgt`), the evaluation's (`cur`: blended as the pose is) and the pose the
    /// running fade started from (`from`).
    tgt: [f64; N],
    cur: [f64; N],
    from: [f64; N],
    fades: u32,
    /// The reset's `fades = view.fadeCount`, taken at the next set (the hook's reset has no view).
    resync_fades: bool,
    ball_visible: bool,
    stretch_amount: f64,
    squash: f64,
    /// The ball materials' Amount and Direction (the build's axes are (x, z, y) of these).
    amount: f64,
    dir: [f64; 3],
    /// A preview's next step is its first (its curves seeded from that frame).
    preview_first: bool,
}

fn lebron_pose(view: &mut CharView, _clips: &Clips, of: &PoseOf<'_>) -> Box<dyn PoseHook> {
    let bone = |b: &str| {
        let i = view.rig.index(b);
        if i.is_none() {
            eprintln!("warning: LeBron pose: bone missing: {b}");
        }
        i
    };
    let ball_part = of.part_keys.iter().position(|k| k == BALL);
    if ball_part.is_none() {
        eprintln!("warning: LeBron pose: the skin has no sub-mesh {BALL}");
    }
    Box::new(LebronPose {
        neck: bone(NECK),
        neck_to: bone(NECK_TO),
        center: bone(CENTER),
        scaler: bone(SCALER),
        ball_part,
        tgt: [0.0; N],
        cur: [0.0; N],
        from: [0.0; N],
        fades: 0,
        resync_fades: false,
        ball_visible: false,
        stretch_amount: 0.0,
        squash: 1.0,
        amount: 1.0,
        dir: [0.0, 1.0, 0.0],
        preview_first: false,
    })
}

/// `rotate`: v turned by the quaternion a.
fn rotate(a: [f64; 4], v: &mut [f64; 3]) {
    let [x, y, z] = *v;
    let [qx, qy, qz, qw] = a;
    let tx = 2.0 * (qy * z - qz * y);
    let ty = 2.0 * (qz * x - qx * z);
    let tz = 2.0 * (qx * y - qy * x);
    v[0] = x + qw * tx + qy * tz - qz * ty;
    v[1] = y + qw * ty + qz * tx - qx * tz;
    v[2] = z + qw * tz + qx * ty - qy * tx;
}

/// The product a x b of two quaternions (x, y, z, w).
fn qmul(a: [f64; 4], b: [f64; 4]) -> [f64; 4] {
    let [ax, ay, az, aw] = a;
    let [bx, by, bz, bw] = b;
    [
        aw * bx + ax * bw + ay * bz - az * by,
        aw * by - ax * bz + ay * bw + az * bx,
        aw * bz + ax * by - ay * bx + az * bw,
        aw * bw - ax * bx - ay * by - az * bz,
    ]
}

/// `component`: bone i's component-space transform (FTransform's product down from the top bone).
fn component(p: &Pose, parents: &[i32], i: i32) -> ([f64; 3], [f64; 4], [f64; 3]) {
    let (mut t, mut r, mut s) = ([0.0; 3], [0.0, 0.0, 0.0, 1.0], [1.0; 3]);
    let mut k = i;
    while k >= 0 {
        let ku = k as usize;
        let (k3, k4) = (ku * 3, ku * 4);
        let sk: [f64; 3] = std::array::from_fn(|j| f64::from(p.s[k3 + j]));
        for j in 0..3 {
            t[j] *= sk[j];
        }
        rotate(p.quat(ku), &mut t);
        for j in 0..3 {
            t[j] += f64::from(p.t[k3 + j]);
            s[j] *= sk[j];
        }
        let a: [f64; 4] = std::array::from_fn(|j| f64::from(p.r[k4 + j]));
        r = qmul(a, r);
        k = parents[ku];
    }
    (t, r, s)
}

impl LebronPose {
    /// The three Constraint nodes: the neck bone's component-space translation, rotation (through the rotator) and
    /// scale become the target's; its children keep their local transforms.
    fn constrain_neck(&self, parents: &[i32], p: &mut Pose) {
        let (Some(neck), Some(neck_to)) = (self.neck, self.neck_to) else { return };
        let (mut ct, cr, cs) = component(p, parents, neck_to as i32);
        // (the build's axes: (x, y, z) here are its (X, Z, Y), a rotation's w negated)
        let mut uq = [cr[0], cr[2], cr[1], -cr[3]];
        let l = crate::anim_nodes::hypot(&uq);
        let l = if l == 0.0 || l.is_nan() { 1.0 } else { l };
        for v in &mut uq {
            *v /= l;
        }
        let uq = through_rotator(uq);
        let cr = [uq[0], uq[2], uq[1], -uq[3]];
        let (pt, pr, ps) = component(p, parents, parents[neck]);
        let pr = [-pr[0], -pr[1], -pr[2], pr[3]];
        for j in 0..3 {
            ct[j] -= pt[j];
        }
        rotate(pr, &mut ct);
        let (b3, b4) = (neck * 3, neck * 4);
        for k in 0..3 {
            p.t[b3 + k] = if ps[k] != 0.0 { (ct[k] / ps[k]) as f32 } else { 0.0 };
            p.s[b3 + k] = if ps[k] != 0.0 { (cs[k] / ps[k]) as f32 } else { 0.0 };
        }
        for (d, v) in p.r[b4..b4 + 4].iter_mut().zip(qmul(pr, cr)) {
            *d = v as f32;
        }
    }

    /// The AnimBP's update, on the curves of the evaluation before.
    fn update(&mut self, view: &mut CharView) {
        self.ball_visible = self.cur[SHOW] >= 0.5;
        self.stretch_amount = self.cur[AMOUNT];
        let sq = self.cur[AMOUNT] * self.cur[MULT];
        self.squash = if sq > 0.1 { 1.0 / sq } else { 1.0 };
        if let Some(b) = self.ball_part {
            view.set_part_visible(b, self.ball_visible);
        }
    }
}

impl PoseRules for LebronPose {
    fn rule(&mut self, cx: &mut PoseCx<'_>, p: &mut Pose, _dt: f64) {
        self.constrain_neck(cx.parents, p);
        if let Some(c) = self.center
            && self.squash != 1.0
        {
            // Squash (1 / p, 1 / p, 1) in the build's axes: (x, y, z) here are its (X, Z, Y)
            p.s[c * 3] = (f64::from(p.s[c * 3]) * self.squash) as f32;
            p.s[c * 3 + 2] = (f64::from(p.s[c * 3 + 2]) * self.squash) as f32;
        }
        if let Some(sc) = self.scaler {
            // the socket's component-space scale: the product of the local scales down to the bone
            let mut d = [1.0f64; 3];
            let mut k = sc as i32;
            while k >= 0 {
                let ku = k as usize;
                for (j, v) in d.iter_mut().enumerate() {
                    *v *= f64::from(p.s[ku * 3 + j]);
                }
                k = cx.parents[ku];
            }
            self.dir = d;
        }
        // this evaluation's curves, for the next update
        if cx.fade_count != self.fades {
            self.fades = cx.fade_count;
            self.from = self.cur;
        }
        let w = cx.fade_weight;
        for k in 0..N {
            self.cur[k] = if w >= 1.0 { self.tgt[k] } else { self.from[k] + (self.tgt[k] - self.from[k]) * w };
        }
    }
}

impl PoseHook for LebronPose {
    /// `preview()`: a preview actor's view (no fighter): the AnimBP anew, run by preview_step. The preview
    /// runs no pawn tick, so Amount stays 1 (the ball unstretched).
    fn preview(&mut self, view: &mut CharView, _of: &PreviewOf<'_>) {
        self.reset(&mut |_, _, _| {});
        self.fades = view.fade_count();
        self.resync_fades = false;
        self.preview_first = true;
    }

    /// `previewStep(anim, time)`: the AnimBP's update on the curves of the evaluation before, then the
    /// curves of the frame's sequence.
    /// Unverified: the update reads the evaluation before; the first step is seeded with this frame's curves.
    fn preview_step(&mut self, view: &mut CharView, clips: &Clips, s: &PreviewStep<'_>) {
        if std::mem::take(&mut self.preview_first) {
            CURVES.fill(clips, s.anim, s.time, &mut self.cur);
            self.from = self.cur;
        }
        self.update(view);
        CURVES.fill(clips, s.anim, s.time, &mut self.tgt);
    }

    /// Before the view's pose of the tick: the pawn's tick (Amount = the update before's StretchAmount), the AnimBP's
    /// update, and the curves of the sequence the tick shows.
    fn set(&mut self, w: &World, f: FighterId, view: &mut CharView, clips: &Clips) {
        if std::mem::take(&mut self.resync_fades) {
            self.fades = view.fade_count();
        }
        self.amount = self.stretch_amount;
        self.update(view);
        let a = &w.fighter(f).anim;
        CURVES.fill(clips, w.names.str(a.anim), f64::from(a.anim_time), &mut self.tgt);
    }

    /// Pawn_c016.SetBallSquashAndStretch @338: Amount on the ball's materials.
    fn scalars(&mut self, _color_index: u8, put: &mut dyn FnMut(Option<usize>, &str, f64)) {
        if let Some(b) = self.ball_part {
            put(Some(b), "Amount", self.amount);
        }
    }

    /// Pawn_c016.SetBallSquashAndStretch @115 / @284: Direction (fml_un_C_scaler's component-space scale) and Center
    /// (fml_un_C_center's world location) on the ball's materials; Center while the ball is shown.
    fn vectors(&self, world: &dyn Fn(usize) -> Mat4, put: &mut PutVector<'_>) {
        let Some(b) = self.ball_part else { return };
        let d = self.dir;
        put(Some(b), "Direction", [d[0], d[2], d[1], 0.0]);
        if let Some(c) = self.center
            && self.ball_visible
        {
            let m = world(c);
            put(Some(b), "Center", [m[12], m[13], m[14], 0.0]);
        }
    }

    /// `reset()`: the curves, the fade seen, the ball's visibility, the stretch and the squash anew, and
    /// Amount 1.
    fn reset(&mut self, _part_scalar: &mut dyn FnMut(usize, &str, f64)) {
        self.tgt = [0.0; N];
        self.cur = [0.0; N];
        self.from = [0.0; N];
        self.resync_fades = true;
        self.ball_visible = false;
        self.stretch_amount = 0.0;
        self.squash = 1.0;
        self.amount = 1.0;
    }
}

/// The pawn's Hoop component: its SCS template, the data file's
/// components.pawn.Hoop_GEN_VARIABLE.
const HOOP_COMPONENT: &str = "Hoop_GEN_VARIABLE";

/// `hoopComp`: Pawn_C016's Hoop, a SkeletalMeshComponent under CharacterMesh0 on the socket HoopSocket (the skeleton's
/// socket: bone fml_un_C_prop1, no offset), RelativeScale3D 0.6, CastShadow false, ComponentTags [Hoop], SkeletalMesh
/// C016_Hoop_SkelMesh (its own skeleton: Root, Hoop, Rim, Net1..3). The template has no AnimClass and no animation,
/// and no graph of his reads the component: it is drawn in its reference pose.
///
/// Who shows it: Comp_FighterVisibilityManager.SetupInitialVisibility hides it (no PrimaryMesh tag), and
/// AnimNotify_SetSubmeshVisibility (SubmeshTag Hoop) of his Wall_Enter / Wall_Idle sequences shows it and of Wall_Jump,
/// Jump, DoubleJump, Fall and FastFall hides it: the sim runs them and keeps the state in sub_hidden under `hoop`.
///
/// Material: the mesh's own slot, C016_Hoop_MIC (0_Character_MAT with BlendMode BLEND_Additive). No SubMeshes entry of
/// the skin names the component.
/// Unverified: whether Comp_Pawn_SkinManager recolours a component without a skin entry (as Harley's props): the
/// instance keeps the material's own Team 0.5.
///
/// The facing: the component rides the mirrored pose, so facing left the build's bone is the view's mirror partner
/// (fml_un_C_prop1 is its own) under the view's reflection with the bone's flip in front (Rec::component_attach, both
/// facings made here).
fn hoop_make(r: &mut Rec, w: &World, clips: &Clips, draws: &FighterDraws, meshes: &MeshMap) {
    let cd = w.char_data(r.fighter());
    let c = cd.fighter.components.pawn.get(HOOP_COMPONENT);
    let p = c.and_then(|c| c.p.as_obj());
    let socket = c.and_then(|c| c.socket.as_deref());
    let mesh = p.and_then(|p| p.get("SkeletalMesh")).and_then(Value::as_str);
    let tag = p
        .and_then(|p| p.get("ComponentTags"))
        .and_then(Value::as_arr)
        .and_then(|t| t.first())
        .and_then(Value::as_str)
        .map(str::to_lowercase);
    let actor = mesh.and_then(|m| draws.actor(m, None));
    let info = actor.as_ref().and_then(|(path, _)| meshes.get(path)).cloned();
    let (Some(p), Some(socket), Some(tag), Some((_, keys)), Some(info)) = (p, socket, tag, actor, info) else {
        eprintln!(
            "warning: LeBron: the Hoop component is not drawn (its template, socket, mesh or materials are not loaded)"
        );
        return;
    };
    let bone = cd.fighter.skeleton.sockets.get(socket).map_or(socket, |s| s.bone.as_str());
    if r.view.rig.index(bone).is_none() {
        eprintln!("warning: LeBron: the Hoop component is not drawn (the view has no bone for {socket})");
        return;
    }
    if p.contains_key("RelativeRotation") || p.contains_key("RelativeLocation") {
        eprintln!("warning: LeBron: the Hoop component has a rotation or a location (not read)");
    }
    let sc = p.get("RelativeScale3D").and_then(Value::as_obj);
    let scale = ["X", "Y", "Z"].map(|k| sc.and_then(|o| o.get(k)).and_then(Value::as_f64).unwrap_or(1.0));
    let at = [false, true].map(|left| r.component_attach(w, socket, [0.0; 3], [0.0, 0.0, 0.0, 1.0], scale, left));
    let mats = keys.iter().map(|m| m.as_deref().map(material)).collect();
    r.add_component_mesh(clips, &tag, at, info, mats, false);
    if !cd.def.sub_meshes.contains(&tag) {
        eprintln!("warning: LeBron: the sim's mesh list has no {tag}: the Hoop stays hidden");
    }
}

/// `placeHoopComp`: once a presentation tick, shown while the sim's mesh list has the tag and sub_hidden does not.
fn hoop_place(r: &mut Rec, w: &World) {
    let tag = "hoop";
    let on = r.sub_mesh_shown(w, tag);
    r.set_component_mesh(tag, on);
}
