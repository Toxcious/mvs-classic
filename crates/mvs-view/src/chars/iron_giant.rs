//! The Iron Giant's render module: the entry, the AnimBP's pose hook, the weapon
//! mode glow on his weapon meshes and the classes' mesh rules. His effects are mvs-render fx/chars/iron_giant.rs.
//!
//! The entry: the big head is C017_AnimBP's ModifyBone_2 (fml_un_C_head) and ModifyBone_1
//! (fml_un_wp_head), scale Additive in bone space, the handlers @8995-9458: Scale = HeadScale x 0.8, Alpha =
//! HeadScale.X > 1 (Pawn_C017 @21124). The montage meshes (C017_DiscArms_SkelMesh, AntenaMesh) are the shared
//! montage-mesh path's. Not drawn here: the "+1 Bolt" pop text (the HUD's). The pawn props Prop_Scrap /
//! Prop_ScrapThrow and C017_AntenaBolts' meshes are the pawn's actors (CharRender.pawn_actors, below).

#![allow(clippy::too_many_arguments, reason = "the pose graph's helpers take the scratch poses they write")]

use std::collections::{BTreeSet, HashMap};
use std::sync::Mutex;

use mvs_sim::World;
use mvs_sim::actors::ActorRef;
use mvs_sim::actors::projectile::{Projectile, ProjectileExt, class_p};
use mvs_sim::actors::world_actor::{WorldActor, WorldActorState};
use mvs_sim::chars::iron_giant::actor_state::AntenaBolts;
use mvs_sim::chars::iron_giant::split::{Layers, lean_back, lean_forward, nav_pose, tracks_into};
use mvs_sim::chars::iron_giant::state::giant_of;
use mvs_sim::fighter::FighterId;
use mvs_sim::fighter::anim::grouped::layers_into;
use mvs_sim::fighter::anim::montages::instance_data;
use mvs_sim::fighter::common::latent_frames;
use mvs_sim::names::Name;
use mvs_vfx::math::{V3, compose_rows, rotation_rows};

use super::{CharRender, PawnActor, PawnComponents, PawnCx, PoseHook, PoseOf, PreviewOf, PreviewStep};
use crate::actor_view::{Comp, IDENT, StaticView, Xform, place_at, rel_rows, xyz};
use crate::actors::{ActorComponents, ActorHost, ActorMesh, ActorPlace, ComponentKind, MeshRule, Rows};
use crate::anim::{
    Clips, HeadScaleNode, Pose, accumulate_additive, accumulate_held, add_weighted_pose, normalize_rotations,
    sample_clip, weight_pose,
};
use crate::blend::{BlendSpacePlayer, BlendSpaceTable};
use crate::frame::{ActorPose, MeshPose};
use crate::math;
use crate::pose::{MeshMap, Rec};
use crate::skins::FighterDraws;
use crate::view::{CharView, PoseCx, PoseRules};

pub(crate) const RENDER: CharRender = CharRender {
    head_scale: &GIANT_HEAD,
    blend_spaces: Some(&BLEND_SPACES),
    pose: Some(giant_pose),
    components: Some(PawnComponents { make: glow_make, place: glow_place, reset: Some(glow_reset) }),
    pawn_actors: Some(giant_actors),
    // Pawn_C017 BeginPlay @12470: CameraTracker, attached to CollisionCylinder at (0, 0, 282.586).
    camera_tracker: Some("CameraTracker_GEN_VARIABLE"),
    ..CharRender::GENERIC
};

/// The classes' own mesh drawing, by class name.
pub(crate) const RULES: &[(&str, MeshRule)] = &[
    ("WeaponModeMissile_C", MeshRule { rot: None, mesh: Some(missile_view) }),
    ("C017_Spit_Bolt_C", MeshRule { rot: None, mesh: Some(bolt_view) }),
    ("C017_ThrownScrap_C", MeshRule { rot: None, mesh: Some(scrap_view) }),
    ("Item_C017_Scrap_C", MeshRule { rot: None, mesh: Some(scrap_item_view) }),
    ("Item_C017_Art_C", MeshRule { rot: None, mesh: Some(art_item_view) }),
];

const GIANT_HEAD: [HeadScaleNode; 2] = [
    HeadScaleNode {
        node: "ModifyBone_2",
        bone: "fml_un_C_head",
        factor: 0.8,
        gated: true,
        moves: [0.0; 3],
        scale: true,
    },
    HeadScaleNode {
        node: "ModifyBone_1",
        bone: "fml_un_wp_head",
        factor: 0.8,
        gated: true,
        moves: [0.0; 3],
        scale: true,
    },
];

/// C017_AnimBP's BlendSpacePlayer nodes the view runs: the two states of the machine HoverState and
/// the additive input of ApplyAdditive_3. BlendSpacePlayer_2 (C017_LaserEye_Look_BlendSpace) is not drawn.
const BLEND_SPACES: BlendSpaceTable = BlendSpaceTable {
    nodes: &[
        ("hoverIn", "AnimGraphNode_BlendSpacePlayer"),
        ("hoverLoop", "AnimGraphNode_BlendSpacePlayer_1"),
        ("tenticleAim", "AnimGraphNode_BlendSpacePlayer_3"),
    ],
};

const STEP: f64 = 1.0 / 60.0;
const DEG: f64 = std::f64::consts::PI / 180.0;

/// A warning once per text (the module's: an unknown layer, the rules' first sight).
fn warn_once(text: String) {
    static SEEN: Mutex<BTreeSet<String>> = Mutex::new(BTreeSet::new());
    if SEEN.lock().expect("the warnings' set").insert(text.clone()) {
        eprintln!("warning: {text}");
    }
}

// ---- the pose -----------------------------------------------------------------
//
// C017_AnimBP's graph, innermost first, as the view draws it on the whole skeleton (the sim's pose.rs states the same
// graph on the shapes' bones, with the layers the view leaves out):
//   the shared pose: the state machines (HoverState's two blend space players in place of the hover clips, `sample`)
//     or the main montage, crossfaded;
//   the split (under a montage of the split's slots, the sim's nav_pose: the base clip is Navigation's then, the
//     montage's tracks are listed with the grouped instances', and a fading one of those at its fade weight):
//     LayeredBoneBlend_11 at fml_un_C_spine_a over the GroundStillSlot / ModifyBone_6 / Navigation base with the
//     UpperBodyOnly track leaned to SpinLeanB / SpinLeanF (TwoWayBlend_4 / _3), or Slot Evade while Split is off;
//   LayeredBoneBlend_1 (local space, weight AlternateMode): C017_WeaponMode_Override on fml_un_tentacle_att's branch;
//     LayeredBoneBlend_10 (mesh space rotation, weight AlternateMode AND NOT Is Hit Stunned): the same clip on the
//     arms, necks and weapon head; LayeredBoneBlend_9 (local): the two clavicle plates;
//   LayeredBoneBlend_5 (mesh space rotation, weight TenticleOverride): the looping C017_WeaponMode_Override_Tenticle
//     with Slot TenticleAdditive on it, under Slot TenticleDefault, on the tentacle branch;
//   ApplyAdditive_3 (alpha TenticleOverride as 0 / 1): C017_TenticleAim_BlendSpace's additive, whole body;
//   LayeredBoneBlend_6 / _8: Slot WeaponAdditive3 (a full pose) from fml_un_C_spine_a up, the tentacle chain kept in
//     mesh space;
//   LayeredBoneBlend_4 (mesh space rotation, weight HoldAlpha x HoldingBuffValid): C017_Hold_Override on the branches
//     fml_un_R_uparm_att and HitCollision01;
//   LayeredBoneBlend_2 (local, weight AlternateMode): C017_WeaponMode_Override on the neck branches;
//   ApplyAdditive_6: Slot Additive (the additive hit reacts), whole body;
//   the Transform stage: LayeredBoneBlend #9 (spine_a, weight 1) of Slot Transform over this pose; its base the same
//     slot, or this pose while Moving.
// The view's rules:
//   - Unverified: a layered blend's PerBoneBlendWeights at a node weight of 1 are 1 on the branch's bones, 0 elsewhere;
//   - Unverified: the override players' and the hold player's times are view state, advanced by the view's step while
//     their node is drawn; the tentacle loop's too (the sim keeps its own for the shapes);
//   - Unverified: HoverFallLean is updated once a pose (the step of the view) from the sim's vx, facing and the tick's
//     down input; the hover players' time is the clip's normalised time of the shared pose;
//   - Unverified: the tentacle aim's inputs are made from the head socket as last drawn and the reticle (its depth
//     0);
//   - ApplyAdditive_3 accumulates the blend at alpha 1 as one additive, scale too;
//   - not ported: ApplyAdditive_4 (WeaponAdditive2 / AimUp2), ApplyAdditive_5 (the plasma attack's), ApplyAdditive_1
//     (FullBodyAdditive), ApplyAdditive (DiscAdditive), ModifyBone_5 (the torso's half turn), AnimGraphNode_
//     BlendSpacePlayer_2 (C017_LaserEye_Look_BlendSpace): the view leaves them out (the sim's shapes have the
//     first five).

const HEAD_SOCKET: &str = "fml_un_C_head";
const TENTACLE_LOOP: &str = "C017_WeaponMode_Override_Tenticle";
const TENTACLE_BRANCH: &str = "fml_un_tentacle_att";
const WEAPON_OVERRIDE: &str = "C017_WeaponMode_Override";
/// SequencePlayer_7.
const HOLD_CLIP: &str = "C017_Hold_Override";
/// LayeredBoneBlend_4.
const HOLD_BRANCHES: [&str; 2] = ["fml_un_R_uparm_att", "HitCollision01"];
/// LayeredBoneBlend_10.
const ARM_BRANCHES: [&str; 7] = [
    "fml_un_L_uparm_att",
    "fml_un_R_uparm_att",
    "fml_un_C_neck_a",
    "fml_un_wp_neck1",
    "fml_un_wp_head",
    "fml_un_wp_neck2",
    "fml_un_wp_neck3",
];
/// LayeredBoneBlend_9.
const CLAV_BRANCHES: [&str; 2] = ["fml_un_wp_L_clav_plate_offset", "fml_un_wp_R_clav_plate_offset"];
/// LayeredBoneBlend_2.
const NECK_BRANCHES: [&str; 2] = ["fml_un_C_neck_a", "fml_un_wp_neck_att"];
const SPINE_BRANCH: &str = "fml_un_C_spine_a";
/// ZERO_ANIMWEIGHT_THRESH (DAT_143b3a298).
const MIN_W: f64 = 1e-5;
/// DAT_143ce1700: a bone of a layered blend takes its blend pose from this weight up.
const FULL_W: f64 = 0.99999;
/// DAT_143b46c24 / 143a4bc30: a quaternion under this squared length becomes the identity.
const TINY_Q: f64 = 1e-8;
/// DAT_144210d9c: SlotEvaluatePose scales the weights past this total.
const OVER_W: f64 = 1.00001;

/// One slot track of the pose (`PoseLayer`): the slot, the clip as a full pose and as an additive (the clips
/// and the additive clips), the time in it, the instance's weight.
#[derive(Debug, Clone)]
struct Layer {
    slot: String,
    anim: String,
    full: Option<usize>,
    add: Option<usize>,
    time: f64,
    w: f64,
}

/// The tick's inputs of the AnimBP's update (`update`), read from the sim at `set`.
#[derive(Debug, Default, Clone, Copy)]
struct UpdateIn {
    /// The lean's walk alpha: clamp01((vx x facing + 1.5 x walk) / (3 x walk)).
    a: f64,
    /// DownPressed: the tick's input (stick at or under -0.4, or the down button).
    down: bool,
    facing_left: bool,
    /// Turret Aim and Torso Flip Alpha flip the aim.
    flip: bool,
    /// The reticle (the sim's aim x, y) and the head socket's place in the build's axes (X, Y, Z), when the turret
    /// has a reticle and the view has been drawn.
    aim: Option<([f64; 2], [f64; 3])>,
}

/// `giantPose`'s state on one view: the masks, the clips, the players, the variables and the scratch poses.
#[derive(Debug)]
struct GiantPose {
    order: Vec<usize>,
    parents: Vec<i32>,
    tent_w: Vec<f32>,
    spine_w: Vec<f32>,
    arm_w: Vec<f32>,
    clav_w: Vec<f32>,
    neck_w: Vec<f32>,
    hold_arm_w: Vec<f32>,
    weapon: Option<usize>,
    hold_clip: Option<usize>,
    looped: Option<usize>,
    root_ix: i32,
    head: Option<usize>,
    hover_in: BlendSpacePlayer,
    hover_loop: BlendSpacePlayer,
    aim: BlendSpacePlayer,
    aim_bones: Vec<usize>,
    s: Pose,
    m: Pose,
    b: Pose,
    ta: Pose,
    w: Pose,
    a: Pose,
    l: Pose,
    ms: Vec<f32>,
    mt: Vec<f32>,
    mb: Vec<f32>,
    layers: Vec<Layer>,
    scratch: Layers,
    nav: bool,
    split: bool,
    moving: bool,
    falling: bool,
    back: bool,
    lean_b: f64,
    lean_f: f64,
    over: bool,
    alt: bool,
    stunned: bool,
    weapon_t: f64,
    weapon_arm_t: f64,
    hold_w: f64,
    hold_t: f64,
    loop_t: f64,
    upd: UpdateIn,
    lean: f64,
    /// The hover player drawn at the last pose: 1 hoverIn, 2 hoverLoop.
    hover: u8,
    aim_vy: f64,
    aim_vz: f64,
    aim_alpha: f64,
    aim_x: f64,
    aim_y: f64,
    posed: bool,
    /// The Superman seal of C017_S01 (ShowSupermanSeal): the view's parts ExtrasMesh and ExtrasMIMesh.
    seal: Option<[usize; 2]>,
}

/// C017_AnimBP @1412: ShowSupermanSeal = AppliedSkin.Slug == this.
const SEAL_SKIN: &str = "skin_c017_s01";
/// The seal's two sub-mesh keys (SetSealVisibility): the plain one right-facing, the mirrored one left-facing.
const SEAL_PARTS: [&str; 2] = ["ExtrasMesh", "ExtrasMIMesh"];

/// The seal's part indices when the skin shows it (C017_AnimBP @1412-1519 sets ShowSupermanSeal and InitSubmeshVis).
fn seal_parts(of: &PoseOf<'_>) -> Option<[usize; 2]> {
    if !of.skin.slug.eq_ignore_ascii_case(SEAL_SKIN) {
        return None;
    }
    let find = |k: &str| of.part_keys.iter().position(|p| p.eq_ignore_ascii_case(k));
    let [a, b] = SEAL_PARTS;
    match (find(a), find(b)) {
        (Some(a), Some(b)) => Some([a, b]),
        _ => {
            eprintln!("warning: C017 pose: the seal skin has no {} / {} sub-meshes", SEAL_PARTS[0], SEAL_PARTS[1]);
            None
        }
    }
}

fn giant_pose(view: &mut CharView, clips: &Clips, of: &PoseOf<'_>) -> Box<dyn PoseHook> {
    let names = &view.rig.names;
    let parents = view.parents.clone();
    let n = names.len();
    let bone = |b: &str| names.iter().position(|x| x == b).unwrap_or_else(|| panic!("C017 pose: bone missing: {b}"));
    let in_branch = |i: usize, top: usize| {
        let mut k = i as i32;
        while k >= 0 {
            if k as usize == top {
                return true;
            }
            k = parents[k as usize];
        }
        false
    };
    let branches = |tops: &[&str]| -> Vec<f32> {
        let ix: Vec<usize> = tops.iter().map(|t| bone(t)).collect();
        (0..n).map(|i| if ix.iter().any(|&t| in_branch(i, t)) { 1.0 } else { 0.0 }).collect()
    };
    let depth = |i: usize| {
        let mut d = 0;
        let mut k = parents[i];
        while k >= 0 {
            d += 1;
            k = parents[k as usize];
        }
        d
    };
    let mut order: Vec<usize> = (0..n).collect();
    order.sort_by_key(|&i| (depth(i), i));
    let full = |name: &str| clips.get(name).filter(|&c| !clips.clips[c].additive);
    let weapon = full(WEAPON_OVERRIDE);
    if weapon.is_none() {
        eprintln!("warning: C017 pose: no such clip: {WEAPON_OVERRIDE}");
    }
    let hover_in = BlendSpacePlayer::new(of.blend.player("hoverIn"), clips);
    let hover_loop = BlendSpacePlayer::new(of.blend.player("hoverLoop"), clips);
    let aim = BlendSpacePlayer::new(of.blend.player("tenticleAim"), clips);
    let aim_bones = aim.tracked_bones(&view.rig);
    let head = names.iter().position(|b| b == HEAD_SOCKET);
    if head.is_none() {
        eprintln!("warning: C017 pose: bone missing: {HEAD_SOCKET}");
    }
    let mut p = GiantPose {
        order,
        tent_w: branches(&[TENTACLE_BRANCH]),
        spine_w: branches(&[SPINE_BRANCH]),
        arm_w: branches(&ARM_BRANCHES),
        clav_w: branches(&CLAV_BRANCHES),
        neck_w: branches(&NECK_BRANCHES),
        hold_arm_w: branches(&HOLD_BRANCHES),
        parents,
        weapon,
        hold_clip: full(HOLD_CLIP),
        looped: full(TENTACLE_LOOP),
        root_ix: names.iter().position(|b| b == "Root").map_or(-1, |i| i as i32),
        head,
        hover_in,
        hover_loop,
        aim,
        aim_bones,
        s: Pose::new(n),
        m: Pose::new(n),
        b: Pose::new(n),
        ta: Pose::new(n),
        w: Pose::new(n),
        a: Pose::new(n),
        l: Pose::new(n),
        ms: vec![0.0; n * 4],
        mt: vec![0.0; n * 4],
        mb: vec![0.0; n * 4],
        layers: Vec::new(),
        scratch: Layers::default(),
        nav: false,
        split: false,
        moving: false,
        falling: false,
        back: false,
        lean_b: 0.0,
        lean_f: 0.0,
        over: false,
        alt: false,
        stunned: false,
        weapon_t: 0.0,
        weapon_arm_t: 0.0,
        hold_w: 0.0,
        hold_t: 0.0,
        loop_t: 0.0,
        upd: UpdateIn::default(),
        lean: 0.0,
        hover: 0,
        aim_vy: 0.0,
        aim_vz: 0.0,
        aim_alpha: 0.0,
        aim_x: 0.0,
        aim_y: 0.0,
        posed: false,
        seal: seal_parts(of),
    };
    // (the aim clips hold one key a track: their deltas made once, at the first pose here, as the clips are mutable
    // there)
    p.aim_bones.sort_unstable();
    Box::new(p)
}

/// FMath::FInterpTo (4.26: speed <= 0 or a squared distance under 1e-8: the target).
fn f_interp_to(cur: f64, target: f64, dt: f64, speed: f64) -> f64 {
    if speed <= 0.0 {
        return target;
    }
    let d = target - cur;
    if d * d < 1e-8 {
        return target;
    }
    cur + d * (dt * speed).clamp(0.0, 1.0)
}

/// a x b, both read before out is written (three.js order: b is the child's), into f32 storage.
fn qmul_into(a: &[f32], ai: usize, b: &[f32], bi: usize, out: &mut [f32], oi: usize) {
    let f = |v: f32| f64::from(v);
    let (ax, ay, az, aw) = (f(a[ai]), f(a[ai + 1]), f(a[ai + 2]), f(a[ai + 3]));
    let (bx, by, bz, bw) = (f(b[bi]), f(b[bi + 1]), f(b[bi + 2]), f(b[bi + 3]));
    out[oi] = (aw * bx + ax * bw + ay * bz - az * by) as f32;
    out[oi + 1] = (aw * by - ax * bz + ay * bw + az * bx) as f32;
    out[oi + 2] = (aw * bz + ax * by - ay * bx + az * bw) as f32;
    out[oi + 3] = (aw * bw - ax * bx - ay * by - az * bz) as f32;
}

/// The masks and chains a layered blend reads.
struct Chains<'a> {
    order: &'a [usize],
    parents: &'a [i32],
}

/// FAnimNode_LayeredBoneBlend with one blend pose and bMeshSpaceRotationBlend (BlendPosesPerBoneFilter
/// FUN_142817220, flags 1), in place on `p`: `blend` laid over it at the per-bone weights w x alpha. Per bone,
/// parents first: S = S[parent] x the base's local rotation, T = T[parent] x the blend pose's; at a weight up to 1e-5
/// the base's translation and scale, B = S; from 0.99999 the blend pose's, B = T; between, translation and scale
/// lerped, B = normalise(S x (1 - x) x sign + T x x) (sign -1 when S . T < 0); the local rotation is then
/// conjugate(B[parent]) x B normalised (a root keeps B). Both normalises give the identity under 1e-8.
fn layered(
    ch: &Chains<'_>,
    p: &mut Pose,
    blend: &Pose,
    w: &[f32],
    alpha: f64,
    ms: &mut [f32],
    mt: &mut [f32],
    mb: &mut [f32],
) {
    let f = |v: f32| f64::from(v);
    for &b in ch.order {
        let pa = ch.parents[b];
        let (b3, b4) = (b * 3, b * 4);
        if pa < 0 {
            ms[b4..b4 + 4].copy_from_slice(&p.r[b4..b4 + 4]);
            mt[b4..b4 + 4].copy_from_slice(&blend.r[b4..b4 + 4]);
        } else {
            let p4 = pa as usize * 4;
            let sa: [f32; 4] = ms[p4..p4 + 4].try_into().expect("four floats");
            let sb: [f32; 4] = mt[p4..p4 + 4].try_into().expect("four floats");
            qmul_into(&sa, 0, &p.r, b4, ms, b4);
            qmul_into(&sb, 0, &blend.r, b4, mt, b4);
        }
        let x = (f(w[b]) * alpha).min(1.0);
        if !(x > MIN_W) {
            mb[b4..b4 + 4].copy_from_slice(&ms[b4..b4 + 4]);
        } else if x >= FULL_W {
            p.t[b3..b3 + 3].copy_from_slice(&blend.t[b3..b3 + 3]);
            p.s[b3..b3 + 3].copy_from_slice(&blend.s[b3..b3 + 3]);
            mb[b4..b4 + 4].copy_from_slice(&mt[b4..b4 + 4]);
        } else {
            for c in 0..3 {
                p.t[b3 + c] = (f(p.t[b3 + c]) + (f(blend.t[b3 + c]) - f(p.t[b3 + c])) * x) as f32;
                p.s[b3 + c] = (f(p.s[b3 + c]) + (f(blend.s[b3 + c]) - f(p.s[b3 + c])) * x) as f32;
            }
            let dot = (0..4).fold(0.0, |s, k| s + f(ms[b4 + k]) * f(mt[b4 + k]));
            let g = if dot < 0.0 { x - 1.0 } else { 1.0 - x };
            let q: [f64; 4] = std::array::from_fn(|k| f(ms[b4 + k]) * g + f(mt[b4 + k]) * x);
            let l2 = q.iter().fold(0.0, |s, v| s + v * v);
            if l2 < TINY_Q {
                mb[b4..b4 + 4].copy_from_slice(&[0.0, 0.0, 0.0, 1.0]);
            } else {
                let l = l2.sqrt();
                for k in 0..4 {
                    mb[b4 + k] = (q[k] / l) as f32;
                }
            }
        }
        if pa < 0 {
            p.r[b4..b4 + 4].copy_from_slice(&mb[b4..b4 + 4]);
            continue;
        }
        // conjugate(B[parent]) x B[b]
        let p4 = pa as usize * 4;
        let (ax, ay, az, aw) = (-f(mb[p4]), -f(mb[p4 + 1]), -f(mb[p4 + 2]), f(mb[p4 + 3]));
        let (bx, by, bz, bw) = (f(mb[b4]), f(mb[b4 + 1]), f(mb[b4 + 2]), f(mb[b4 + 3]));
        let r = [
            aw * bx + ax * bw + ay * bz - az * by,
            aw * by - ax * bz + ay * bw + az * bx,
            aw * bz + ax * by - ay * bx + az * bw,
            aw * bw - ax * bx - ay * by - az * bz,
        ];
        let l2 = r.iter().fold(0.0, |s, v| s + v * v);
        if l2 < TINY_Q {
            p.r[b4..b4 + 4].copy_from_slice(&[0.0, 0.0, 0.0, 1.0]);
            continue;
        }
        let l = l2.sqrt();
        for (o, v) in p.r[b4..b4 + 4].iter_mut().zip(r) {
            *o = (v / l) as f32;
        }
    }
}

/// A local space layered node at weight 1: the masked bones take the blend pose's local transforms.
fn replace(p: &mut Pose, blend: &Pose, w: &[f32]) {
    for (b, &x) in w.iter().enumerate() {
        if x != 0.0 {
            p.copy_bone(blend, b);
        }
    }
}

/// FAnimNode_TwoWayBlend of two full poses into a: b alone from 0.99999, else a x (1 - alpha) with b added x alpha,
/// rotations normalised. The caller tests alpha > 1e-5.
fn two_way(a: &mut Pose, b: &Pose, alpha: f64, ta: &mut Pose) {
    if alpha >= FULL_W {
        a.copy(b);
        return;
    }
    weight_pose(a, 1.0 - alpha, ta);
    add_weighted_pose(b, alpha, ta);
    normalize_rotations(ta);
    a.copy(ta);
}

/// A looping player's time advanced by dt at the clip's rate scale.
fn loop_step(clips: &Clips, c: usize, t: f64, dt: f64) -> f64 {
    let a = &clips.clips[c].anim;
    let mut t = t + dt * a.rate_scale;
    if a.length > 0.0 {
        while t >= a.length {
            t -= a.length;
        }
    }
    t
}

/// The slot nodes over the tick's layers.
struct Slots<'a> {
    layers: &'a [Layer],
}

impl Slots<'_> {
    fn has(&self, slot: &str) -> bool {
        self.layers.iter().any(|l| l.slot == slot && l.w > MIN_W)
    }

    /// GetSlotWeight and SlotEvaluatePose's weights for a slot: None when the node passes its source through; else
    /// (norm, source_w): the factor on every instance's weight and the source's weight. (Total > 1 strictly, against
    /// 1.0: 142856d44 comiss / jbe; the pose weights' own test is OVER_W.)
    fn weights(&self, slot: &str) -> Option<(f64, f64)> {
        let (mut total, mut full) = (0.0, 0.0);
        for l in self.layers {
            if l.slot != slot || !(l.w > MIN_W) {
                continue;
            }
            total += l.w;
            if l.full.is_some() {
                full += l.w;
            }
        }
        if total > 1.0 {
            full /= total;
        }
        let source = (1.0 - full).clamp(0.0, 1.0);
        let norm = if total > OVER_W { 1.0 / total } else { 1.0 };
        (total > MIN_W).then_some((norm, source))
    }

    /// The additive instances of a slot accumulated on p in play order, each at its weight x norm. scale: p is a full
    /// pose (a full-pose clip of the slot is not one of these then); else p stands for an additive pose and a
    /// full-pose clip here is a mistake of the data.
    fn accumulate(&self, cx: &mut PoseCx<'_>, slot: &str, p: &mut Pose, scale: bool, norm: f64, ta: &mut Pose) {
        for l in self.layers {
            if l.slot != slot || !(l.w > MIN_W) || (scale && l.full.is_some()) {
                continue;
            }
            let Some(c) = l.add else {
                let why = if !scale && l.full.is_some() {
                    format!("a full pose in the additive slot {slot}")
                } else {
                    "no such clip".to_owned()
                };
                warn_once(format!("C017 pose: {}: {why}", l.anim));
                continue;
            };
            accumulate_additive(cx.clips, c, cx.rig, cx.rest, l.time, l.w * norm, scale, p, ta);
        }
    }

    /// A slot node whose source is the additive identity, applied by its ApplyAdditive node (alpha 1) to p: each
    /// additive instance accumulated on p, not the scale.
    fn additive(&self, cx: &mut PoseCx<'_>, slot: &str, p: &mut Pose, ta: &mut Pose) {
        if let Some((norm, _)) = self.weights(slot) {
            self.accumulate(cx, slot, p, false, norm, ta);
        }
    }

    /// A slot node over p, in place: the full-pose instances in play order, each at its weight, then the source p
    /// at its own weight (only above 1e-5), blended (BlendPosesTogetherIndirect's kernels); then the additive
    /// instances. root: the pose root for the clips' root lock (the split's tracks), -1 for the grouped instances'.
    fn pose(
        &self,
        cx: &mut PoseCx<'_>,
        slot: &str,
        p: &mut Pose,
        root: i32,
        m: &mut Pose,
        b: &mut Pose,
        ta: &mut Pose,
    ) {
        let Some((norm, source)) = self.weights(slot) else { return };
        let mut first = true;
        for l in self.layers {
            let Some(c) = l.full.filter(|_| l.slot == slot && l.w > MIN_W) else { continue };
            let w = l.w * norm;
            sample_clip(cx.clips, c, cx.rig, cx.rest, l.time, m, root);
            if first {
                first = false;
                weight_pose(m, w, b);
            } else {
                add_weighted_pose(m, w, b);
            }
        }
        if !first {
            if source > MIN_W {
                add_weighted_pose(p, source, b);
            }
            normalize_rotations(b);
            p.copy(b);
        }
        self.accumulate(cx, slot, p, true, norm, ta);
    }
}

impl GiantPose {
    /// BlueprintUpdateAnimation's part for the blend spaces, once a pose.
    fn update(&mut self, dt: f64) {
        let u = self.upd;
        self.lean = f_interp_to(self.lean, 10.0 - 20.0 * u.a, dt, 2.0);
        self.lean = f_interp_to(self.lean, 10.0 - 20.0 * (if u.down { 1.0 } else { u.a }), dt, 2.0);
        if let Some(([ax, ay], [hx, hy, hz])) = u.aim
            && self.posed
        {
            // the reticle (the sim's x is the build's -Y) from the head socket + (0, 0, 250); depth 0
            let (ty, tz) = (-ax, ay);
            let (dx, dy, dz) = (-hx, ty - hy, tz - (hz + 250.0));
            let l = (dx * dx + dy * dy + dz * dz).sqrt();
            // GetDirectionUnitVector: (To - From).GetSafeNormal(), zero under a squared length of 1e-8
            self.aim_vy = if l * l > 1e-8 { dy / l } else { 0.0 };
            self.aim_vz = if l * l > 1e-8 { dz / l } else { 0.0 };
            self.aim_alpha = (crate::anim_nodes::hypot(&[ty - hy, tz - hz]) / 1200.0).clamp(0.0, 1.0);
        }
        let mut y = if u.facing_left { self.aim_vy } else { -self.aim_vy };
        if u.flip {
            y = -y;
        }
        self.aim_x = y * self.aim_alpha;
        self.aim_y = self.aim_vz * self.aim_alpha;
    }
}

impl PoseRules for GiantPose {
    fn sample(
        &mut self,
        cx: &mut PoseCx<'_>,
        clip: usize,
        time: f64,
        montage: Option<Name>,
        dt: f64,
        out: &mut Pose,
        again: bool,
    ) -> bool {
        // the montage's sample of a tick whose source pose under the slot was sampled first: that call made the
        // tick's update and keeps the hover player
        if again {
            return false;
        }
        self.update(dt);
        let pl = if montage.is_some() {
            0
        } else if self.hover_in.ok && self.hover_in.uses(clip) {
            1
        } else if self.hover_loop.ok && self.hover_loop.uses(clip) {
            2
        } else {
            0
        };
        if pl != self.hover {
            self.hover = pl;
            match pl {
                1 => self.hover_in.init(),
                2 => self.hover_loop.init(),
                _ => {}
            }
        }
        let lean = self.lean;
        let player = match pl {
            1 => &mut self.hover_in,
            2 => &mut self.hover_loop,
            _ => return false,
        };
        if !player.select(lean, 0.0, dt) {
            return false;
        }
        let len = cx.clips.clips[clip].anim.length;
        player.t = if len > 0.0 { (time / len).clamp(0.0, 1.0) } else { 0.0 };
        player.pose(cx.clips, cx.rig, cx.rest, out, &mut self.m, cx.root_index);
        true
    }

    fn rule(&mut self, cx: &mut PoseCx<'_>, p: &mut Pose, dt: f64) {
        if !self.posed && self.aim.ok {
            self.aim.cache_deltas(cx.clips, cx.rig, cx.rest);
        }
        self.posed = true;
        let wm = self.alt && self.weapon.is_some();
        *cx.fade_from_shown = self.nav;
        let held = self.hold_w > MIN_W && self.hold_clip.is_some();
        if !self.over && self.layers.is_empty() && !wm && !held {
            return; // the shared pose as it is
        }
        let GiantPose { order, parents, s, m, b, ta, w, a, l, ms, mt, mb, layers, .. } = self;
        let ch = Chains { order, parents };
        let sl = Slots { layers };
        let root = self.root_ix;
        // BeforeUperbodySplit and the Evade bypass (p is Navigation)
        if self.nav {
            if self.split {
                a.copy(p);
                sl.pose(cx, "UpperBodyOnly", a, root, m, b, ta);
                if self.lean_b > MIN_W {
                    l.copy(p);
                    sl.pose(cx, "SpinLeanB", l, root, m, b, ta);
                    two_way(a, l, self.lean_b, ta); // TwoWayBlend_4
                }
                if self.lean_f > MIN_W {
                    l.copy(p);
                    sl.pose(cx, "SpinLeanF", l, root, m, b, ta);
                    two_way(a, l, self.lean_f, ta); // TwoWayBlend_3
                }
                if !self.falling {
                    if !self.moving {
                        sl.pose(cx, "GroundStillSlot", p, root, m, b, ta); // BlendListByBool_3's true side
                    } else {
                        // ModifyBone_6
                        let q = if self.back { [0.0, 1.0, 0.0, 0.0] } else { [0.0, 0.0, 0.0, 1.0] };
                        cx.turn_com(p, q);
                    }
                }
                layered(&ch, p, a, &self.spine_w, 1.0, ms, mt, mb); // LayeredBoneBlend_11
            } else {
                sl.pose(cx, "Evade", p, root, m, b, ta); // BlendListByBool_2's false side
            }
        }
        // The weapon mode override on the tentacle chain, then (not hit stunned) the arms, the necks and the weapon
        // head in mesh space and the two clavicle plates
        if let Some(weapon) = self.weapon.filter(|_| wm) {
            self.weapon_t = loop_step(cx.clips, weapon, self.weapon_t, dt);
            sample_clip(cx.clips, weapon, cx.rig, cx.rest, self.weapon_t, w, -1);
            replace(p, w, &self.tent_w); // LayeredBoneBlend_1
            if !self.stunned {
                self.weapon_arm_t = loop_step(cx.clips, weapon, self.weapon_arm_t, dt);
                sample_clip(cx.clips, weapon, cx.rig, cx.rest, self.weapon_arm_t, m, -1);
                layered(&ch, p, m, &self.arm_w, 1.0, ms, mt, mb); // LayeredBoneBlend_10
                replace(p, m, &self.clav_w); // LayeredBoneBlend_9
            }
        }
        // The tentacle chain, shown only while TenticleOverride
        if self.over
            && let Some(lp) = self.looped
        {
            self.loop_t = loop_step(cx.clips, lp, self.loop_t, dt);
            sample_clip(cx.clips, lp, cx.rig, cx.rest, self.loop_t, s, -1);
            sl.additive(cx, "TenticleAdditive", s, ta); // ApplyAdditive_2
            sl.pose(cx, "TenticleDefault", s, -1, m, b, ta); // In / Out_Tenticle over it
            layered(&ch, p, s, &self.tent_w, 1.0, ms, mt, mb); // LayeredBoneBlend_5 at TenticleOverride = 1
        }
        // ApplyAdditive_3 (alpha TenticleOverride as 0 / 1): the aim blend space's additive on the pose so far, whole
        // body. The player is updated while the node's alpha is relevant.
        if self.over && self.aim.ok {
            if self.aim.select(self.aim_x, self.aim_y, dt) {
                self.aim.advance(cx.clips, dt);
            }
            self.aim.additive_pose(cx.clips, cx.rig, cx.rest, b, m);
            accumulate_held(b, &self.aim_bones, 1.0, true, p);
        }
        // (ApplyAdditive_4 with WeaponAdditive2 / AimUp2 is not drawn.)
        // The turn montage from spine_a up, the tentacle chain kept in component space
        if sl.has("WeaponAdditive3") {
            s.copy(p);
            sl.pose(cx, "WeaponAdditive3", s, -1, m, b, ta); // over Slot WeaponTurn (no montage has that track)
            layered(&ch, s, p, &self.tent_w, 1.0, ms, mt, mb); // LayeredBoneBlend_6
            layered(&ch, p, s, &self.spine_w, 1.0, ms, mt, mb); // LayeredBoneBlend_8
        }
        // The hold arm over that (AfterHold)
        if let Some(hc) = self.hold_clip.filter(|_| held) {
            self.hold_t = loop_step(cx.clips, hc, self.hold_t, dt);
            sample_clip(cx.clips, hc, cx.rig, cx.rest, self.hold_t, s, -1);
            layered(&ch, p, s, &self.hold_arm_w, self.hold_w, ms, mt, mb); // LayeredBoneBlend_4
        }
        // The weapon mode override on the neck branches (w: SequencePlayer_2 runs with SequencePlayer_15)
        if wm {
            replace(p, w, &self.neck_w); // LayeredBoneBlend_2
        }
        // The additive hit reacts, the outermost additive, whole body
        sl.additive(cx, "Additive", p, ta);
        // The Transform stage: LayeredBoneBlend #9 (spine_a, weight 1) of Slot Transform over this pose; its base the
        // same slot, or this pose while Moving.
        if self.nav && sl.has("Transform") {
            a.copy(p);
            sl.pose(cx, "Transform", a, root, m, b, ta);
            if self.moving {
                layered(&ch, p, a, &self.spine_w, 1.0, ms, mt, mb);
            } else {
                p.copy(a);
            }
        }
    }
}

impl PoseHook for GiantPose {
    /// `set(layers, tenticleOverride, alternateMode, inHitstun, f)`: the sim's grouped instances, the split's tracks
    /// (the default group's montage when it is one of the split's, and under no shown montage the fading ones of
    /// those at their fade weight), the AnimBP's variables, and the update's inputs.
    fn set(&mut self, w: &World, f: FighterId, view: &mut CharView, clips: &Clips) {
        let l = &mut self.scratch;
        layers_into(w, f, &mut l.slot, &mut l.anim, &mut l.time, &mut l.w);
        let grouped = l.len();
        let gs = giant_of(w, f);
        let fi = w.fighter(f);
        if let Some(ga) = gs.and_then(|g| g.giant_anim.as_ref()) {
            mvs_sim::chars::iron_giant::split::nav_tracks_into(w, f, l);
            if mvs_sim::fighter::shown_montage(w, f).is_none() {
                for fd in &fi.montages.fading {
                    let Some(m) = instance_data(&w.data, w, f, fd.m.montage.uid) else { continue };
                    if !nav_pose(m) {
                        continue;
                    }
                    let ticks = mvs_anim::montage::blend_ticks(fd.time);
                    tracks_into(
                        &fd.m,
                        m,
                        if ticks > 0.0 { mvs_sim::math::narrow(f64::from(fd.left) / ticks) } else { 0.0 },
                        l,
                    );
                }
            }
            self.split = ga.split;
            self.moving = ga.moving;
            self.falling = ga.falling;
            self.back = ga.back;
            self.lean_b = f64::from(lean_back(ga));
            self.lean_f = f64::from(lean_forward(ga));
        }
        self.nav = l.len() > grouped;
        self.layers.clear();
        for k in 0..l.len() {
            let anim = &l.anim[k];
            let c = clips.get(anim);
            let is_add = |c: usize| clips.clips[c].additive;
            self.layers.push(Layer {
                slot: l.slot[k].clone(),
                anim: anim.clone(),
                full: c.filter(|&c| !is_add(c)),
                add: c.filter(|&c| is_add(c)),
                time: f64::from(l.time[k]),
                w: f64::from(l.w[k]),
            });
        }
        self.over = mvs_sim::fighter::tenticle_override(w, f);
        self.alt = gs.is_some_and(|g| g.alternate_mode);
        self.stunned = mvs_sim::fighter::in_hitstun(w, f);
        self.hold_w = gs.map_or(0.0, |g| f64::from(g.hold_alpha * if g.hold_valid { 1.0 } else { 0.0 }));
        // the update's inputs (BaseMaxWalkSpeed: the character's MaxWalkSpeed; Velocity: the sim's vx)
        let walk = mvs_sim::fighter::common::data_of(w, f).def.max_walk;
        let (vx, facing) = (fi.body.vx, fi.body.facing_dir);
        let left = facing < 0.0;
        // SetSealVisibility (@12954, each update; the function is at the AnimBP's `## SetSealVisibility`): facing left
        // shows ExtrasMIMesh and hides ExtrasMesh, facing right the reverse
        if let Some([plain, mirrored]) = self.seal {
            view.set_part_visible(plain, !left);
            view.set_part_visible(mirrored, left);
        }
        let aim = gs.and_then(|g| g.ally_turret.as_ref()).filter(|t| t.reticle).and_then(|t| {
            let h = self.head?;
            // Unverified: the head bone as last drawn, under the view's model as it is now.
            let mut cs = Vec::new();
            view.bones_cs(&mut cs);
            let m = math::mul(&view.model_world(), cs.get(h)?);
            // three (x, y, z) = the build's (X, Z, Y) / 100
            Some(([f64::from(t.aim_x), f64::from(t.aim_y)], [m[12] * 100.0, m[14] * 100.0, m[13] * 100.0]))
        });
        self.upd = UpdateIn {
            a: ((f64::from(vx) * f64::from(facing) + 1.5 * walk) / (3.0 * walk)).clamp(0.0, 1.0),
            down: fi.controls.inp.y <= -0.4 || fi.controls.inp.down_btn,
            facing_left: left,
            flip: gs.is_some_and(|g| g.turret_aim_on && left != g.turret_aim_start_left),
            aim,
        };
    }

    /// `reset()`: the lean, the aim's inputs, the hover player drawn and the three blend space players anew.
    fn reset(&mut self, _part_scalar: &mut dyn FnMut(usize, &str, f64)) {
        self.lean = 0.0;
        self.aim_vy = 0.0;
        self.aim_vz = 0.0;
        self.aim_alpha = 0.0;
        self.aim_x = 0.0;
        self.aim_y = 0.0;
        self.hover = 0;
        self.hover_in.init();
        self.hover_loop.init();
        self.aim.init();
    }

    /// A preview (no fighter): the seal at the actor's FacingLeft of the frame (right until the first step).
    fn preview(&mut self, view: &mut CharView, _of: &PreviewOf<'_>) {
        self.set_seal(view, false);
    }

    fn preview_step(&mut self, view: &mut CharView, _clips: &Clips, step: &PreviewStep<'_>) {
        self.set_seal(view, step.facing_left);
    }
}

impl GiantPose {
    /// SetSealVisibility: facing left shows ExtrasMIMesh and hides ExtrasMesh, facing right the reverse.
    fn set_seal(&self, view: &mut CharView, left: bool) {
        if let Some([plain, mirrored]) = self.seal {
            view.set_part_visible(plain, !left);
            view.set_part_visible(mirrored, left);
        }
    }
}

// ---- the weapon mode glow --------------------------------------------------------------------------
//
// Pawn_C017's WeaponGlow timeline (1.5 s; GlowAlpha: a cubic key 0 at 0 to 1 at 1.5 with flat tangents):
// WeaponModeGlowVisuals(true) (AlternateMode on, @8375) plays it from the start, (false) (@1147 at BeginPlay, @9353 when
// the mode ends) reverses it from where it is; its update (@21978) sets Emissive_Mult = Lerp(0, 50, GlowAlpha) on the
// dynamic instances of material 0 of WeaponMesh, WeaponMeshTop and WeaponMeshReverse (@718..1128).
// Unverified: advanced by the world's step (the pawn's hit pause dilation is left out).

const GLOW_PARTS: [&str; 3] = ["WeaponMesh", "WeaponMeshTop", "WeaponMeshReverse"];
const GLOW_LENGTH: f64 = 1.5;

/// The glow's state on the view (Rec::own): the timeline's position, AlternateMode at the last tick, the value last
/// written.
fn glow_make(r: &mut Rec, _w: &World, _clips: &Clips, _draws: &FighterDraws, _meshes: &MeshMap) {
    r.own = vec![0.0, 0.0, -1.0];
}

/// The Lab's reset: the timeline at 0 and AlternateMode off; the value last written stays, so the next
/// place writes 0 where it was not.
fn glow_reset(r: &mut Rec) {
    r.own[0] = 0.0;
    r.own[1] = 0.0;
}

fn glow_place(r: &mut Rec, w: &World) {
    let Some(gs) = giant_of(w, r.fighter()) else { return };
    let on = gs.alternate_mode;
    if on && r.own[1] == 0.0 {
        r.own[0] = 0.0; // PlayFromStart
    }
    r.own[1] = if on { 1.0 } else { 0.0 };
    r.own[0] = (r.own[0] + if on { STEP } else { -STEP }).clamp(0.0, GLOW_LENGTH);
    let a = r.own[0] / GLOW_LENGTH;
    let v = 50.0 * (3.0 * a * a - 2.0 * a * a * a);
    if v != r.own[2] {
        r.own[2] = v;
        for k in GLOW_PARTS {
            r.set_part_scalar(k, "Emissive_Mult", v);
        }
    }
}

// ---- his actors' and items' meshes ------------------------------------------------------------------

/// FRichCurve::Eval between RCIM_Cubic keys (t, v, arrive, leave): the Bezier of v1, v1 + leave x dt / 3, v2 - arrive x
/// dt / 3, v2 (his effects' plasma growth too).
pub fn cubic(keys: &[[f64; 4]], t: f64) -> f64 {
    if t <= keys[0][0] {
        return keys[0][1];
    }
    for i in 1..keys.len() {
        let (a, b) = (keys[i - 1], keys[i]);
        if t > b[0] {
            continue;
        }
        let dt = b[0] - a[0];
        let u = (t - a[0]) / dt;
        let w = 1.0 - u;
        let p1 = a[1] + a[3] * dt / 3.0;
        let p2 = b[1] - b[2] * dt / 3.0;
        return w * w * w * a[1] + 3.0 * w * w * u * p1 + 3.0 * w * u * u * p2 + u * u * u * b[1];
    }
    keys[keys.len() - 1][1]
}

/// WeaponModeMissile's Rotation timeline (0.75 s, CurveFloat_0): StaticMesh.K2_SetRelativeRotation(RLerp((-90, 0, 0),
/// FaceAtStart ? (-90, 0, -110) : (-90, 0, 110), RotationAlpha, not the shortest path)) (@2328..2451), and (-90, 0, 0)
/// again with the launch (@212). Keys (t, v, arrive, leave).
const MISSILE_TURN: [[f64; 4]; 3] =
    [[0.0, 0.0, 0.0, 0.0], [0.496991, 0.28559732, 1.3333334, 1.3333334], [0.75, 1.0, 3.7787719, 3.7787712]];

/// `missileRoll`: the mesh's roll in degrees. Delay(DurationToGravity) -> the velocity, Delay(0.033)
/// -> the launch (the rotation back).
/// Unverified: FaceAtStart is the sim's; the timeline's alpha is taken at the actor's age.
pub fn missile_roll(w: &World, pr: &Projectile) -> f64 {
    let left = match &pr.ext {
        ProjectileExt::Missile(m) => m.face_at_start,
        _ => pr.facing < 0.0,
    };
    if missile_launched(w, pr) {
        return 0.0;
    }
    (if left { -110.0 } else { 110.0 }) * cubic(&MISSILE_TURN, (f64::from(pr.age) * STEP).min(0.75))
}

/// `missileLaunch`: Delay(DurationToGravity) -> the velocity, Delay(0.033) ->
/// ParticleSystem.SetVisibility(true) (@178), the mesh's rotation back (@212): whether the missile's age is past it.
pub fn missile_launched(w: &World, pr: &Projectile) -> bool {
    let dur = mvs_sim::actors::item::num(class_p(w, pr), "DurationToGravity").unwrap_or(0.0);
    f64::from(pr.age) >= f64::from(latent_frames(dur) + latent_frames(0.033))
}

/// Sphere / ScrapMesh.K2_SetWorldRotation((Pitch -50, Yaw -50, Roll 0)) at BeginPlay (C017_ThrownScrap @15,
/// Item_C017_Scrap @10): the component's relative rotation is that under the inverse of the actor's rotation then.
fn scrap_world() -> Rows {
    rotation_rows(-50.0 * DEG, -50.0 * DEG, 0.0)
}

fn facing_rows(facing: f64, pitch: f64) -> Rows {
    let h = std::f64::consts::FRAC_PI_2;
    rotation_rows(pitch, if facing > 0.0 { -h } else { h }, 0.0)
}

fn transpose(r: &Rows) -> Rows {
    [r[0], r[3], r[6], r[1], r[4], r[7], r[2], r[5], r[8]]
}

/// A component as comp-place takes it.
fn comp(c: &crate::actors::Component) -> Comp {
    Comp { par: c.parent.clone(), rl: c.loc, rot: c.rot, rs: c.scale }
}

/// How a projectile mesh's relative rotation is had.
#[derive(Debug, Clone, Copy)]
enum Own {
    /// The template's.
    Template,
    /// The missile's turn: (Pitch -90, 0, Roll missileRoll).
    Missile,
    /// A fixed one (the thrown scrap's world rotation under its spawn facing).
    Fixed(Rows),
}

/// `projectileMesh`: a projectile's mesh component under its attach parents, under the actor (its rotation and
/// scale).
#[derive(Debug)]
struct ProjMeshView {
    id: mvs_sim::actors::ProjectileId,
    view: StaticView,
    comp: Comp,
    own: Own,
}

fn proj_mesh(
    host: &mut ActorHost<'_>,
    w: &World,
    a: ActorRef,
    cs: &ActorComponents,
    comp_name: &str,
    mesh: impl FnOnce(&Projectile) -> String,
    team: bool,
    own: impl FnOnce(&Projectile) -> Own,
) -> Option<Box<dyn ActorMesh>> {
    let ActorRef::Projectile(id) = a else { return None };
    let pr = w.actors.projectiles.get(id)?;
    let owner = pr.owner?;
    let c = cs.get(comp_name)?;
    let from = mvs_sim::fighter::common::actor_def_owner(w, owner, pr.cls);
    let variant = if team { host.variant(w, owner) } else { "" };
    let view = StaticView::new(host, w, from, &mesh(pr), variant)?;
    Some(Box::new(ProjMeshView { id, view, comp: comp(c), own: own(pr) }))
}

impl ActorMesh for ProjMeshView {
    fn place(
        &mut self,
        _host: &mut ActorHost<'_>,
        w: &World,
        at: &ActorPlace,
        alpha: f64,
        shown: bool,
        out: &mut Vec<MeshPose>,
    ) {
        self.view.visible = shown;
        let own = match self.own {
            Own::Template => self.comp.rot,
            Own::Fixed(r) => r,
            Own::Missile => {
                let roll = w.actors.projectiles.get(self.id).map_or(0.0, |pr| missile_roll(w, pr));
                rotation_rows(-90.0 * DEG, 0.0, roll * DEG)
            }
        };
        let (x, y) = at.at(alpha);
        self.view.set(&place_at(&self.comp, &own, self.comp.rs, &at.rot, at.scale, [0.0, -x, y]));
        self.view.draw(out);
    }
}

/// StaticMesh = C017_Rocket at (5.87, 0, -3.16), scale (4, 4, 6), turned by the Rotation timeline (missile_roll).
/// SetTeamColor @1839..2067: SetMaterial(0, local ? C017_Rocket_Blue_Mat_Mat : index 0 ? ..._Red_... : ..._Blue_...).
fn missile_view(host: &mut ActorHost<'_>, w: &World, a: ActorRef, cs: &ActorComponents) -> Option<Box<dyn ActorMesh>> {
    proj_mesh(host, w, a, cs, "StaticMesh", |_| "C017_Rocket".to_owned(), true, |_| Own::Missile)
}

/// No graph of its own (ParentProjectilePhysics): StaticMesh = BoltMesh, yaw -90, scale 8. The mesh's slot has no
/// material: the engine's default (WorldGridMaterial).
fn bolt_view(host: &mut ActorHost<'_>, w: &World, a: ActorRef, cs: &ActorComponents) -> Option<Box<dyn ActorMesh>> {
    proj_mesh(host, w, a, cs, "StaticMesh", |_| "BoltMesh".to_owned(), false, |_| Own::Template)
}

/// Sphere: SetMesh @1616..1681 (index 0 C017_Scrap_Base, 1 C017_Scrap_Switch; the sim's mesh), the component's
/// OverrideMaterials (C017_Submeshes_MIC), scale 1.2.
/// Unverified: the actor's rotation at BeginPlay is taken as its facing's yaw (the spawn transform's pitch is not
/// read).
fn scrap_view(host: &mut ActorHost<'_>, w: &World, a: ActorRef, cs: &ActorComponents) -> Option<Box<dyn ActorMesh>> {
    let mesh = |pr: &Projectile| {
        let k = match &pr.ext {
            ProjectileExt::ThrownScrap(s) => s.mesh,
            _ => 0,
        };
        (if k == 1 { "C017_Scrap_Switch" } else { "C017_Scrap_Base" }).to_owned()
    };
    let own =
        |pr: &Projectile| Own::Fixed(compose_rows(&scrap_world(), &transpose(&facing_rows(f64::from(pr.facing), 0.0))));
    proj_mesh(host, w, a, cs, "Sphere", mesh, false, own)
}

/// `itemMesh`: Item_C017_Scrap / Item_C017_Art's mesh under the item's Capsule (the item's rotation: its facing's yaw
/// and pitch). SetTeamColor (Scrap @1055..1283, Art @1180..1408): SetMaterial(0, IsLocalOnline ? C017_Submeshes_MIC :
/// GetColorIndex of GetOwner() 0 ? ..._Red : ..._Blue).
/// Unverified: when an item's SetTeamColor runs is not read: the material follows the item's owner each frame.
/// Held: the actor rides the holder's hold socket with the socket's rotation; the Capsule root has bAbsoluteScale, so
/// its scale stays the item's own.
/// Deviation: the custom depth outline a held item gets from its grabber is not drawn; the socket is the view's bone
/// as last drawn, mirrored facing left (ActorHost::socket), not the pose copy's (unverified). Item_C017_Scrap's
/// PS_ScrapGlow is not drawn (mvs-render's effects have no item rule for it).
#[derive(Debug)]
struct ItemView {
    id: mvs_sim::actors::ItemId,
    view: StaticView,
    comp: Comp,
    own: Rows,
}

fn item_view(
    host: &mut ActorHost<'_>,
    w: &World,
    a: ActorRef,
    cs: &ActorComponents,
    comp_name: &str,
    world: Option<Rows>,
) -> Option<Box<dyn ActorMesh>> {
    let ActorRef::Item(id) = a else { return None };
    let it = w.actors.items.get(id)?;
    let c = cs.get(comp_name)?;
    let ComponentKind::StaticMesh { mesh, .. } = &c.kind else { return None };
    let (spawner, owner) = (it.spawner?, it.owner?);
    let variant = host.variant(w, owner);
    let view = StaticView::new(host, w, spawner, mesh, variant)?;
    // Unverified: the actor's rotation at BeginPlay is taken as the item's at its first sight.
    let own = match world {
        Some(r) => compose_rows(&r, &transpose(&facing_rows(f64::from(it.facing), f64::from(it.pitch).to_radians()))),
        None => c.rot,
    };
    Some(Box::new(ItemView { id, view, comp: comp(c), own }))
}

impl ActorMesh for ItemView {
    fn place(
        &mut self,
        host: &mut ActorHost<'_>,
        w: &World,
        at: &ActorPlace,
        alpha: f64,
        shown: bool,
        out: &mut Vec<MeshPose>,
    ) {
        self.view.visible = shown;
        let Some(it) = w.actors.items.get(self.id) else { return };
        if !shown {
            return;
        }
        if let Some(o) = it.owner {
            let v = host.variant(w, o);
            self.view.dress(host, v);
        }
        let hold = at.holder.and_then(|h| {
            let socket = mvs_sim::fighter::match_side::hold_socket(w, h);
            host.socket(w, h, &socket, &Xform { loc: [0.0; 3], rot: crate::actor_view::IDENT, scale: [1.0; 3] })
        });
        let x = match hold {
            Some(s) => place_at(&self.comp, &self.own, self.comp.rs, &s.rot, f64::from(it.scale), s.loc),
            None => {
                let (x, y) = at.at(alpha);
                place_at(&self.comp, &self.own, self.comp.rs, &at.rot, f64::from(it.scale), [0.0, -x, y])
            }
        };
        self.view.set(&x);
        self.view.draw(out);
    }
}

fn scrap_item_view(
    host: &mut ActorHost<'_>,
    w: &World,
    a: ActorRef,
    cs: &ActorComponents,
) -> Option<Box<dyn ActorMesh>> {
    item_view(host, w, a, cs, "ScrapMesh", Some(scrap_world()))
}

/// Tree = C017_Base_Props_Low at (0, 0, -118.6), yaw -50 (no world rotation call, no glow component).
fn art_item_view(host: &mut ActorHost<'_>, w: &World, a: ActorRef, cs: &ActorComponents) -> Option<Box<dyn ActorMesh>> {
    item_view(host, w, a, cs, "Tree", None)
}

// ---- the pawn props and C017_AntenaBolts' meshes: the pawn's actors ----------
//
// Prop_Scrap (tag prop_scrap, socket HitCollision01) and Prop_ScrapThrow (prop_scrapthrow, fml_un_R_wrist): SCS
// children of BodyMesh, in Comp_Pawn_MeshManager.Meshes under their tag, shown by the sim's sub-mesh state. The
// default skin's entry is filed under the mesh's own name with its Red / Blue sets: the fighter's selected variant,
// set once. Each at the socket with the component's relative transform under it.
// Unverified: the socket is the last drawn pose's; a 'copy' (the view's bone as it is, the
// copy's scale on the mesh) is taken as ActorHost::socket's bone as drawn.
// Another skin: its static entry of the tag where it has one, else the template's mesh in its own material, no team
// pick (ActorHost::skin_static).
//
// C017_AntenaBolts: the antenna (its StaticMesh component's mesh, AntenaMesh, its own WorldGridMaterial slot) at
// Antena_Socket + (0, 0, 20) at the component's RelativeScale3D (@7096 / @7167); the bolts (@3427..4781): for k = 1 ..
// StartingBoltNumber a BoltMesh (the default material) under the root at VLerp(0, its start, ExpandAlpha), turned by
// the root's roll (the sim's spin), scale 10 x the root's (the sim's sizeScale); bolt k shown while k < the sim's
// bolts. Each is an actor of its own (a StaticView).

const PAWN_PROP_SOCKETS: [(&str, &str); 2] = [("prop_scrap", "HitCollision01"), ("prop_scrapthrow", "fml_un_R_wrist")];
const ANTENA_SOCKET: &str = "Antena_Socket";
const BOLT_MESH: &str = "BoltMesh";
const BOLT_SCALE: f64 = 10.0;

/// An asset reference's object name (`/Game/../AntenaMesh.AntenaMesh` or `AntenaMesh`): the draws' actors key.
fn asset_name(s: &str) -> &str {
    let s = s.rsplit('/').next().unwrap_or(s);
    s.rsplit('.').next().unwrap_or(s)
}

/// A prop on a socket of the Giant's view.
#[derive(Debug)]
struct PawnProp {
    tag: &'static str,
    view: StaticView,
    socket: &'static str,
    rel: Xform,
}

/// The views of one C017_AntenaBolts actor: the antenna with its component's scale, the bolts.
#[derive(Debug)]
struct BoltViews {
    seq: i32,
    antenna: Option<(StaticView, V3)>,
    bolts: Vec<StaticView>,
}

/// The Giant's C017_AntenaBolts actors, by actor.
#[derive(Debug, Default)]
struct GiantBolts {
    live: HashMap<ActorRef, BoltViews>,
}

/// CharRender.pawn_actors: his antenna bolts and the two props.
fn giant_actors(host: &ActorHost<'_>, w: &World, f: FighterId) -> Vec<Box<dyn PawnActor>> {
    let d = &w.char_data(f).fighter;
    let mut out: Vec<Box<dyn PawnActor>> = vec![Box::new(GiantBolts::default())];
    for (tag, socket) in PAWN_PROP_SOCKETS {
        let c = d.components.pawn.values().find(|c| {
            c.ty == "StaticMeshComponent"
                && c.p.as_obj().and_then(|p| p.get("ComponentTags")).and_then(|t| t.as_arr()).is_some_and(|t| {
                    let tags: Vec<&str> = t.iter().filter_map(|v| v.as_str()).collect();
                    !tags.contains(&"UI") && sub_mesh_tag(&tags).is_some_and(|t| t.eq_ignore_ascii_case(tag))
                })
        });
        let p = c.and_then(|c| c.p.as_obj());
        let st = if host.on_default_skin(w, f) {
            p.and_then(|p| p.get("StaticMesh")?.as_str())
                .and_then(|m| host.draws_of(w, f)?.actor_key(asset_name(m)))
                .map(|k| (k, host.selected_variant(w, f)))
        } else {
            p.and_then(|p| host.skin_static(w, f, p))
        };
        let view = st.and_then(|(k, variant)| StaticView::new(host, w, f, &k, variant));
        let Some(view) = view else {
            warn_once(format!(
                "Iron Giant: the prop {tag} is not drawn (its component, mesh or materials are not loaded)"
            ));
            continue;
        };
        let rel = Xform {
            loc: xyz(p.and_then(|p| p.get("RelativeLocation")), 0.0),
            rot: rel_rows(p.and_then(|p| p.get("RelativeRotation"))),
            scale: xyz(p.and_then(|p| p.get("RelativeScale3D")), 1.0),
        };
        out.push(Box::new(PawnProp { tag, view, socket, rel }));
    }
    out
}

impl PawnActor for PawnProp {
    /// `placePawnProps`: shown while the fighter is drawn and the sim has the sub-mesh shown.
    fn present(&mut self, host: &ActorHost<'_>, w: &World, cx: &PawnCx<'_>, _alpha: f64, out: &mut Vec<ActorPose>) {
        if !cx.shown(w, self.tag) {
            return;
        }
        let Some(at) = host.socket(w, cx.fid, self.socket, &self.rel) else { return };
        self.view.set(&at);
        let mut meshes = Vec::new();
        self.view.draw(&mut meshes);
        cx.outline(true, &mut meshes);
        out.push(ActorPose { meshes, actor: None, bones: Vec::new(), ..ActorPose::default() });
    }
}

impl BoltViews {
    fn new(host: &ActorHost<'_>, w: &World, x: &WorldActor, a: &AntenaBolts) -> BoltViews {
        let from = mvs_sim::fighter::common::actor_def_owner(w, x.owner, x.cls);
        let def = mvs_sim::fighter::common::actor_def(w, ActorRef::Fighter(from), x.cls);
        let sm = def.and_then(|d| d.components.get("StaticMesh")).and_then(|c| c.as_obj());
        let antenna = sm.and_then(|sm| {
            let key = host.draws_of(w, x.owner)?.actor_key(asset_name(sm.get("StaticMesh")?.as_str()?))?;
            let v = StaticView::new(host, w, x.owner, &key, "")?;
            Some((v, xyz(sm.get("RelativeScale3D"), 1.0)))
        });
        let n = bolt_count(a);
        let mut bolts = Vec::with_capacity(n);
        for _ in 0..n {
            let Some(v) = StaticView::new(host, w, x.owner, BOLT_MESH, "") else { break };
            bolts.push(v);
        }
        if antenna.is_none() || bolts.len() < n {
            warn_once("Iron Giant: C017_AntenaBolts' antenna or bolts are not all drawn (mesh not loaded)".to_owned());
        }
        BoltViews { seq: x.seq, antenna, bolts }
    }
}

/// StartingBoltNumber as a count (the class's whole number).
pub fn bolt_count(a: &AntenaBolts) -> usize {
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss, reason = "a small whole count")]
    let n = a.start.max(0.0) as usize;
    n
}

/// ExpandAlpha (Timeline_0, linear 0..1 over 0.25 s, reversed from the end at Death), from the actor's age.
pub fn expand(a: &AntenaBolts) -> f64 {
    let age = f64::from(a.age);
    let t =
        if a.dying { 1.0 - ((age - f64::from(AntenaBolts::death_tick())) * STEP) / 0.25 } else { age * STEP / 0.25 };
    t.clamp(0.0, 1.0)
}

/// `boltAt`: bolt `j` of `n`'s place and rows in the world, the root at scale `size`.
pub fn bolt_at(a: &AntenaBolts, j: usize, n: usize, alpha: f64, size: f64) -> (V3, Rows) {
    let left = a.facing < 0.0;
    let angle = ((j + 1) as f64 * 360.0) / n as f64;
    let side = if left { 1.0 } else { -1.0 };
    let start = [0.0, side * f64::from(a.radius) * (angle * DEG).cos(), f64::from(a.radius) * (angle * DEG).sin()];
    let root = rotation_rows(0.0, 0.0, f64::from(a.spin) * DEG);
    let mut loc = [0.0, -a.x, a.y];
    let r = alpha * size;
    for (k, l) in loc.iter_mut().enumerate() {
        *l = mvs_sim::math::narrow(f64::from(*l) + r * (start[1] * root[3 + k] + start[2] * root[6 + k]));
    }
    let own = rotation_rows(0.0, 0.0, -(if left { angle + 180.0 } else { -angle }) * DEG);
    (loc.map(f64::from), compose_rows(&own, &root))
}

impl PawnActor for GiantBolts {
    fn present(&mut self, host: &ActorHost<'_>, w: &World, cx: &PawnCx<'_>, _alpha: f64, out: &mut Vec<ActorPose>) {
        let mut seen = Vec::new();
        for (id, x) in w.actors.actors.iter() {
            let WorldActorState::AntenaBolts(a) = &x.state else { continue };
            if x.dead || x.owner != cx.fid {
                continue;
            }
            let r = ActorRef::Actor(id);
            seen.push(r);
            // (a rollback's actor in the same slot is another: its views anew)
            if self.live.get(&r).is_none_or(|v| v.seq != x.seq) {
                self.live.insert(r, BoltViews::new(host, w, x, a));
            }
            let Some(v) = self.live.get_mut(&r) else { continue };
            let size = x.size_scale.unwrap_or(1.0);
            if let Some((view, scale)) = &mut v.antenna {
                let rel = Xform { loc: [0.0, 0.0, 20.0], rot: IDENT, scale: *scale };
                if let Some(at) = socket_main(host, w, x.owner, ANTENA_SOCKET, &rel) {
                    view.set(&at);
                    let mut meshes = Vec::new();
                    view.draw(&mut meshes);
                    out.push(ActorPose { meshes, actor: None, bones: Vec::new(), ..ActorPose::default() });
                }
            }
            let n = bolt_count(a);
            let e = expand(a);
            for (j, view) in v.bolts.iter_mut().enumerate() {
                if !a.bolt_on(mvs_sim::math::narrow(j as f64)) {
                    continue;
                }
                let (loc, rot) = bolt_at(a, j, n, e, f64::from(size));
                let s = BOLT_SCALE * f64::from(size);
                view.set(&Xform { loc, rot, scale: [s; 3] });
                let mut meshes = Vec::new();
                view.draw(&mut meshes);
                out.push(ActorPose { meshes, actor: None, bones: Vec::new(), ..ActorPose::default() });
            }
        }
        self.live.retain(|r, _| seen.contains(r));
    }
}

/// `subMeshTag`: the first tag that is not one of Comp_Pawn_MeshManager's role tags.
fn sub_mesh_tag<'a>(tags: &[&'a str]) -> Option<&'a str> {
    const ROLE: [&str; 4] = ["RecieveArmor", "IgnoreArmor", "VisMesh", "PrimaryMesh"];
    tags.iter().find(|t| !ROLE.contains(t)).copied()
}

/// `socketFollow` on CharacterMesh0 (boneFollow's 'main') of a pawn without a mirror node (C017): facing left the
/// main mesh holds the right-facing pose turned with the actor, so its bone is the view's bone of the same name
/// reflected in depth (UE X) about the view's root; facing right the view's bone. Then the socket and `rel` under it.
/// (ActorHost::socket is the view's bone as drawn: a 'copy'.)
fn socket_main(host: &ActorHost<'_>, w: &World, f: FighterId, socket: &str, rel: &Xform) -> Option<Xform> {
    let fi = w.fighter(f);
    if fi.body.facing_dir >= 0.0 {
        return host.socket(w, f, socket, rel);
    }
    let b = host.fighters.iter().find(|b| b.id == fi.id)?;
    let skel = &w.char_data(f).fighter.skeleton;
    let sk = skel.sockets.iter().find(|(k, _)| k.eq_ignore_ascii_case(socket)).map(|(_, v)| v);
    let bone = sk.map_or(socket, |s| s.bone.as_str());
    let i = b.names.iter().position(|n| n.eq_ignore_ascii_case(bone))?;
    let root_x = math::to_ue(&b.model)[12];
    let mut m = math::to_ue(&math::mul(&b.model, b.cs.get(i)?));
    for c in 0..3 {
        m[c * 4] = -m[c * 4];
    }
    m[12] = 2.0 * root_x - m[12];
    if let Some(s) = sk {
        let sx = Xform { loc: s.loc, rot: mvs_vfx::math::rotator_rows(s.rot, None), scale: s.scale };
        m = math::mul(&m, &crate::actors::xform_mat(&sx));
    }
    Some(crate::actors::mat_xform(&math::mul(&m, &crate::actors::xform_mat(rel))))
}
