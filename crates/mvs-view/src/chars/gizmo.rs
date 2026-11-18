//! Gizmo's render module: the entry, the AnimBP's pose hook, the arrow-charge
//! components and the classes' mesh rules. His effects are mvs-render fx/chars/gizmo.rs.
//!
//! What the pose hook draws (C023A_AnimBP's update, BlueprintUpdateAnimation):
//! - The sub-mesh toggles: SetSubmeshVisibility (@9627 -> @11506-11979) runs ToggleSubmeshVisibility(curve, tag,
//!   state) for Bow, Popcorn, Keyboard, Box, Umbrella and Lantern (TOGGLES): when GetCurveValue(curve) >= 0.5 differs
//!   from the state the part is set to the new state and the state flips (as Marvin's). The AnimBP's write stands
//!   until the sim changes the part (the later write). The curves: the clips' own by the sequence the tick shows and
//!   the view's crossfade.
//! - SetFaceVisibility (@10348): facing left HeadMesh is hidden and HeadMIMesh shown, facing right the reverse, written
//!   when the facing differs from the last write's (FaceLeftVis) or before the first (InitSubmeshVis).
//! - HeadScale: the entry's head_scale (ModifyBone_3 on fml_un_ear_att, _4 on fml_un_C_gzm_face_att, _5 on
//!   fml_un_C_head).
//! - The slot graph's walk (the aim, bow walk, attack and driving slots) is the sim's, which the view mixes into the
//!   drawn pose.
//!
//! The evaluation (rule): every node sits below WAnimationMirrorNode, so it works on the build's pose before the
//! mirror, which is the pose the view reflects facing left. The nodes, in the graph's evaluation order (input first):
//!   - Under DefaultSlot (only while the montage has no DefaultSlot track; scaled by 1 - its weight, unverified):
//!     IsDriving (BlendListByBool_5): the Driving machine is the sim's walk; above it ModifyBone_13 (carAtt, Additive,
//!     parent-bone space, Pitch CarLean @2707), ModifyBone_10 (fml_un_COM, Replace, parent-bone space, Pitch CarLean
//!     @2519) and CopyBone_4 (COM -> carAtt, translation, world space). CarLean = BPI_DrivingWheelie's RotationAngle
//!     (the car's `wheelie`, mvs-sim chars/gizmo/car.rs). Not driving: LayeredBoneBlend (the bow): base =
//!     BlendListByBool_1 (the bow walk: the sim's timed weight, TwoWayBlend of the BowWalkF / BowWalkB slots; else
//!     IsFalling: Navigation or AfterAim), blend = AfterAim (the aim slots, which the sim takes whole: the view's
//!     pose), per bone over spine_a / gzm_face_att / bowHandle / ear_att (crate::layered). Not done when the
//!     montage's AirAttack (falling) or GroundAttack slot has a track (those slots sit above it).
//!   - ModifyBone_11 (fml_un_COM, Replace, parent-bone space, Pitch FallLean; alpha OverrideFallLean @6536): FallLean
//!     = FInterpTo(FallLean, Lerp(20, -20, clamp(NormalizeToRange(-(sel x VelocityY), -1200, 1200))), dt, 4) with sel
//!     FacingLeft ? 1 : -1 while OverrideFallLean, else 0 (@9642-10324; BaseMaxWalkSpeed 1200, FallLeanAmount 20,
//!     FallLeanSpeed 4: the CDO).
//!   - ModifyBone_12 (fml_un_COM, Replace, parent-bone space): Pitch = FacingLeft ? -COM_Override_Angle :
//!     COM_Override_Angle, alpha min(COM_Override_Alpha, Lerp(1, 0, clamp((GameTime - COM_BlendStartTime) /
//!     COM_BlendTime))) (@5848-6294).
//!   - CopyBone_5 (R_wrist -> umbrellaAtt, translation, component space, alpha 1).
//!   - LayeredBoneBlend_2 (base: the pose so far, blend: Navigation = C023A_Lantern_Idle's player while attached) and
//!     LayeredBoneBlend_1 (blend: AfterDefaultSlot, the pose before ModifyBone_11), both at AttachedToAlly && !Stop
//!     Overrides (@8037-8316).
//!   - CopyBone_3 (head -> gzm_face_att), CopyBone_2 (L_wrist -> bowHandle), ModifyBone_9 (bowHandle +(0, 15, 15),
//!     Pitch 5, bone space): (Attached || BowVisible) && !SO (@7500-8000); ModifyBone_6 (prop1 +Z 60): min(Att && !SO,
//!     KeyboardVisible && !SO) (@4070-4295); CopyBone_1 (L_wrist -> prop1), ModifyBone_8 (prop1 +(40, -30, -50)):
//!     min(Att && !SO, PopcornVisible && !SO) (@6976-7463); CopyBone (L_wrist -> umbrellaAtt), ModifyBone_7
//!     (umbrellaAtt +(-20, 10, 0)): Att && !SO (@6694-6939). (The CopyBones: the shared crate::anim_nodes node.)
//!
//! Deviations and unverified models:
//!   - Unverified: the Lantern slot (above every node here): with a Lantern track at full weight nothing here runs; in
//!     its blend the nodes run whole.
//!   - Unverified: the montage's DefaultSlot weight scales the driving nodes' alphas (the build blends the slot's
//!     source with the montage pose); the bow layer is skipped under any DefaultSlot weight.
//!   - Deviation: IsFalling's 0.1 s list blends (BlendListByBool_3 / _4) are taken as switches; the slot tracks' poses
//!     are sampled at the montage's position (pose_in) and the root lock is the skeleton's top bone's.
//!   - Unverified: FallLean's FInterpTo steps 1/60 a tick (no hitpause dilation).
//!   - Not ported: ModifyBone_1 (VB ArrowAttach, alpha IsRolling) and ModifyBone_2 (VB Spine): virtual bones the view
//!     has no bone for (ArrowCharge hangs on the virtual bone's source); ModifyBone (holiday_hat1, after the skin
//!     nodes: translation (0, 0, -20) and scale (0.2, 0.2, 0.2) Additive in component space, alpha the curve
//!     HolidayHatAdjust: the additive scale's rule is not read).
//!   - The skin nodes above the mirror node (SKIN_NODES: Trail x3, SpringBone x3, AnimDynamics x2, on the holiday hat,
//!     rope and headband bones of the skins that have them), made from the fighter's data at the first set
//!     (crate::anim_nodes::SkinNodes), after every node above; each alpha float(!Is Hit Stunned && !Is Respawning &&
//!     !(HolidayHatAdjust > 0.5)) (@3162-3437 and each node's), the curve the evaluation's before (HAT_CURVE, faded as
//!     the toggles' curves). They run on the unmirrored pose with the component's transform at the actor's facing
//!     (mesh_xf), as Reindog's (chars/creature/skin_controls.rs, its Deviation), with the hitpause dilation 0.005.
//!     Deviation: the head-scale step (ModifyBone_3 / _4 / _5, after them) is the view's after the rule: the same order.
//!   - A preview (BP_CharacterPreview, the podium): SetSubmeshVisibility runs with no Fighter (the IsValid(Fighter)
//!     test @2274 pops to it @9627) and the actor has a Comp_FighterVisibilityManager (set @588-773), so the toggles
//!     and the face write act on it (PoseHook::preview_step).
//!
//! The arrow-charge components (Pawn_C023A's SCS, drawn from the sim's writes): ArrowCharge (StaticMeshComponent
//! C023A_Arrow_Mesh, tag ArrowCharge) on the socket VB ArrowAttach (a virtual bone: ActorHost::socket), relative
//! (-45, 0, 0), rotation (Pitch 0, Yaw 10, Roll 25); shown by the sim's sub-mesh state (Fighter.sub_hidden). Its child
//! ArrowFireCharged and the FireArrowNullMesh's ArrowFire (PS_C023A_LitArrow) are the effects'. FireArrowNullMesh (a
//! Plane with no material, scale 0.0001) is not drawn.
//! Unverified: the sockets are the last presented pose's. Deviation: VB ArrowAttach's ModifyBone_1 (alpha IsRolling) is
//! the shared virtual bone follow's (which does not apply it in full).

use mvs_data::fighter::SlotGraphNode;
use mvs_sim::World;
use mvs_sim::chars::gizmo::car::car_of;
use mvs_sim::chars::gizmo::gizmo_of;
use mvs_sim::fighter::FighterId;
use mvs_sim::fighter::anim::MontageSlot;
use mvs_sim::fighter::anim::montages::{instance_data, slot_uid};
use mvs_sim::fighter::anim::slot_blend::slot_player_pose;
use mvs_sim::fighter::anim::update::loco_clip;
use mvs_sim::fighter::in_hitstun;
use mvs_sim::fighter::moves::is_falling;

use super::{CharRender, PawnActor, PawnCx, PoseHook, PoseOf, PreviewOf, PreviewStep};
use crate::ActorPose;
use crate::actor_anim::ActorSlot;
use crate::actor_view::{Comp, SkinnedActor, StaticView, Xform, place_under, rel_rows, xyz};
use crate::actors::{ActorComponents, ActorHost, ActorMesh, ActorPlace, MeshRule, Rows, actor_rot, class_def};
use crate::anim::{Clips, HeadScaleNode, Pose, apply_additive, blend_pose, sample_clip};
use crate::anim_nodes::{
    AnimNode, AnimNodeSkel, BoneXfs, CopyBoneNode, CsEval, CsPose, RespawnFlag, SkinNodes, UeXf, hypot, mesh_xf, qmul,
    qrot,
};
use crate::curves::CurveTable;
use crate::frame::MeshPose;
use crate::layered::LayeredBlend;
use crate::pose::set_scalar;
use crate::rotator::euler_quat;
use crate::view::{CharView, PoseCx, PoseRules};
use mvs_sim::actors::actor_montage::montage_of;
use mvs_sim::actors::{ActorRef, ProjectileExt, ProjectileId};
use mvs_vfx::math::compose_rows;

pub(crate) const RENDER: CharRender = CharRender {
    // C023A_AnimBP's head scale nodes.
    head_scale: &[
        HeadScaleNode {
            node: "ModifyBone_3",
            bone: "fml_un_ear_att",
            factor: 1.0,
            gated: false,
            moves: [0.0; 3],
            scale: true,
        },
        HeadScaleNode {
            node: "ModifyBone_4",
            bone: "fml_un_C_gzm_face_att",
            factor: 1.0,
            gated: false,
            moves: [0.0; 3],
            scale: true,
        },
        HeadScaleNode {
            node: "ModifyBone_5",
            bone: "fml_un_C_head",
            factor: 1.0,
            gated: false,
            moves: [0.0; 3],
            scale: true,
        },
    ],
    pose: Some(gizmo_pose),
    pawn_actors: Some(arrow_props),
    ..CharRender::GENERIC
};

/// The classes' own mesh drawing, by class name.
pub(crate) const RULES: &[(&str, MeshRule)] = &[
    (ARROW, MeshRule { rot: None, mesh: Some(arrow_mesh) }),
    ("C023A_PianoNote_C", MeshRule { rot: None, mesh: Some(no_mesh) }),
    ("C023A_Music_Projectile_C", MeshRule { rot: None, mesh: Some(no_mesh) }),
    (CAR, MeshRule { rot: None, mesh: Some(car_view) }),
];

/// The skin nodes above the mirror node, in the graph's evaluation order (the pose links from ComponentToLocalSpace
/// back: input first).
const SKIN_NODES: &[&str] = &[
    "AnimGraphNode_Trail_2",
    "AnimGraphNode_Trail_1",
    "AnimGraphNode_SpringBone_2",
    "AnimGraphNode_SpringBone_1",
    "AnimGraphNode_Trail",
    "AnimGraphNode_SpringBone",
    "AnimGraphNode_AnimDynamics_1",
    "AnimGraphNode_AnimDynamics",
];
/// The curve the skin nodes' alphas read (GetCurveValue @3258), and Greater_FloatFloat's bound on it (@3299).
const HAT_CURVE: CurveTable = CurveTable::new(&["HolidayHatAdjust"]);
const HAT_MAX: f64 = 0.5;

const CURVES: CurveTable = CurveTable::new(&[
    "show_bowAndArrow",
    "show_popcorn",
    "show_gizmo_keyboard",
    "show_gizmo_box",
    "show_gizmo_umbrella",
    "show_lantern",
]);
/// ToggleSubmeshVisibility's GreaterEqual 0.5.
const THRESHOLD: f64 = 0.5;
/// The calls in their order (@11531-11921): the skin sub-mesh key; the curve is the same index of CURVES.
const TOGGLES: [&str; 6] = ["Bow", "Popcorn", "Keyboard", "Box", "Umbrella", "Lantern"];
const FACES: [&str; 2] = ["HeadMesh", "HeadMIMesh"];
const PARTS: usize = TOGGLES.len() + FACES.len();

/// The slot nodes the sim's walk mixes into the drawn pose, checked once in the data's graph.
const SLOT_NODES: [&str; 12] = [
    "Lantern",
    "DefaultSlot",
    "AirAttack",
    "GroundAttack",
    "BowWalkF",
    "BowWalkB",
    "Aim",
    "AimUp",
    "AimDown",
    "AirAim",
    "AirAimUp",
    "AirAimDown",
];
const TICK: f64 = 1.0 / 60.0;
/// The CDO: BaseMaxWalkSpeed, FallLeanAmount, FallLeanSpeed.
const BASE_MAX_WALK: f64 = 1200.0;
const FALL_LEAN_AMOUNT: f64 = 20.0;
const FALL_LEAN_SPEED: f64 = 4.0;
const ZERO_W: f64 = 1e-5;
const FULL_W: f64 = 0.99999;
const BOW: usize = 0;
const POPCORN: usize = 1;
const KEYBOARD: usize = 2;

/// FMath::FInterpTo.
fn f_interp_to(cur: f64, target: f64, dt: f64, speed: f64) -> f64 {
    if speed <= 0.0 {
        return target;
    }
    let dist = target - cur;
    if dist * dist < 1e-8 {
        return target;
    }
    cur + dist * (dt * speed).clamp(0.0, 1.0)
}

fn clamp01(x: f64) -> f64 {
    x.clamp(0.0, 1.0)
}

// ---- FAnimNode_ModifyBone ----

/// The modes C023A_AnimBP's nodes use (FAnimationRuntime ConvertCSTransformToBoneSpace /
/// ConvertBoneSpaceTransformToCS). Not a shared node kind: these three are the only uses read (the head scale nodes are
/// the shared HeadScaleNode's).
///   - Bone: Translation and Rotation BMM_Additive in BCS_BoneSpace (ModifyBone_6 / _7 / _8 / _9): the rotation R * Q,
///     the location R (S . T) + t (FTransform composition). Rotation before translation, as the node orders them.
///   - Replace: Rotation BMM_Replace in BCS_ParentBoneSpace (ModifyBone_10 / _11 / _12 on fml_un_COM): the local
///     rotation becomes Q; the component rotation R_parent * Q, the location and scale kept.
///   - Add: Rotation BMM_Additive in BCS_ParentBoneSpace (ModifyBone_13 on carAtt): the local rotation Q * local; the
///     component rotation R_parent * Q * R_parent^-1 * R.
///
/// Q = FRotator(Pitch, 0, 0).Quaternion() (MakeRotator(0, X, 0) in the graph is Kismet's (Roll, Pitch, Yaw)).
#[derive(Clone, Copy, Debug, PartialEq)]
enum ModifyMode {
    Bone,
    Replace,
    Add,
}

#[derive(Debug)]
struct ModifyBone {
    bone: Option<usize>,
    mode: ModifyMode,
    t: Option<[f64; 3]>,
    q: [f64; 4],
    pitch: f64,
    xf: BoneXfs,
}

impl ModifyBone {
    fn new(node: &str, cs: &CsPose, bone: &str, mode: ModifyMode, t: Option<[f64; 3]>, pitch: f64) -> ModifyBone {
        let b = cs.bone(bone);
        if b.is_none() {
            eprintln!("warning: C023A_AnimBP {node}: bone {bone} is not in the skeleton: the node does nothing");
        }
        assert!(t.is_none() || mode == ModifyMode::Bone, "{node}: a translation in {mode:?} mode is not ported");
        let mut m = ModifyBone { bone: b, mode, t, q: [0.0, 0.0, 0.0, 1.0], pitch: f64::NAN, xf: BoneXfs::default() };
        m.set_pitch(pitch);
        m
    }

    /// The node's Rotation (a bound input: written each update).
    fn set_pitch(&mut self, deg: f64) {
        if deg == self.pitch {
            return;
        }
        self.pitch = deg;
        self.q = euler_quat([0.0, deg, 0.0]);
    }

    fn evaluate(&mut self, cs: &mut CsEval<'_>, alpha: f64) {
        let Some(b) = self.bone else { return };
        if !(alpha > ZERO_W) {
            return;
        }
        let x = cs.get(b);
        let mut nt = x.t;
        let nr = match self.mode {
            ModifyMode::Bone => {
                let nr = qmul(x.q, self.q);
                if let Some(t) = self.t {
                    let a = qrot(x.q, [t[0] * x.s[0], t[1] * x.s[1], t[2] * x.s[2]], false);
                    for k in 0..3 {
                        nt[k] += a[k];
                    }
                }
                nr
            }
            mode => {
                let pa = cs.parents()[b];
                let rp = if pa < 0 { [0.0, 0.0, 0.0, 1.0] } else { cs.get(pa as usize).q };
                if mode == ModifyMode::Replace {
                    qmul(rp, self.q)
                } else {
                    // local = R_p^-1 R; Q * local; R_p * that
                    let inv = [-rp[0], -rp[1], -rp[2], rp[3]];
                    qmul(rp, qmul(self.q, qmul(inv, x.q)))
                }
            }
        };
        self.xf.clear();
        self.xf.push(b, UeXf { t: nt, q: nr, s: x.s });
        cs.local_blend(&self.xf, alpha.min(1.0));
    }
}

// ---- the pose hook ----

/// What `set` reads of the shown montage for the evaluation (the fighter's shown montage, read in the pose rule).
#[derive(Debug, Default)]
struct MontageRead {
    /// A montage is shown.
    on: bool,
    /// Its shown weight.
    w: f64,
    /// The attack slot above the bow layer (AirAttack falling, else GroundAttack) has a track.
    attack: bool,
    /// The bow walk slot's track (BowWalkB walking back, else BowWalkF) and its time, at a weight over 1e-5.
    walk: Option<(usize, f64)>,
}

#[derive(Debug)]
struct GizmoPose {
    /// Per toggle, then per face: its index in the view's parts.
    part: [Option<usize>; PARTS],
    tags: [String; PARTS],
    last_tick: Option<i32>,
    tgt: [f64; 6],
    cur: [f64; 6],
    from: [f64; 6],
    fades: u32,
    /// BowVisible, PopcornVisible, ...
    state: [bool; 6],
    /// The AnimBP's last write (-1 none, 0 hidden, 1 shown) and the sim's visibility at the last update.
    own: [i8; PARTS],
    sim_vis: [i8; PARTS],
    face_init: bool,
    face_left_vis: bool,
    fall_lean: f64,
    // ---- the nodes (made once a view) ----
    root_bone: i32,
    cs: CsPose,
    /// CopyBone .. CopyBone_5, from the data's anim nodes (made at the first set).
    cb: [Option<CopyBoneNode>; 6],
    mb6: ModifyBone,
    mb7: ModifyBone,
    mb8: ModifyBone,
    mb9: ModifyBone,
    mb10: ModifyBone,
    mb11: ModifyBone,
    mb12: ModifyBone,
    mb13: ModifyBone,
    /// LayeredBoneBlend (the bow), _1 and _2, from the data's slot graph.
    lbb: Option<LayeredBlend>,
    lbb1: Option<LayeredBlend>,
    lbb2: Option<LayeredBlend>,
    walk_list: Option<usize>,
    lantern_seq: Option<usize>,
    nodes: usize,
    made: bool,
    b: Pose,
    t: Pose,
    u: Pose,
    ads: Pose,
    nav_p: Pose,
    // ---- this tick's inputs for the evaluation ----
    live: bool,
    /// PoseOf::slaved: no AnimBP of its own (set returns after the respawn's check; the lantern layer does not run).
    slaved: bool,
    alive: bool,
    car_lean: f64,
    is_driving: bool,
    attached_on: bool,
    falling: bool,
    stop_overrides: bool,
    fall_alpha: f64,
    com_alpha: f64,
    com_angle: f64,
    lantern_w: f64,
    default_w: f64,
    over_w: f64,
    /// The bow walk list's weight (the sim's timed weight).
    ww: f64,
    m: MontageRead,
    /// Navigation's pose: C023A_Lantern_Idle's player while attached, else the locomotion (clip, time).
    nav: Option<(usize, f64)>,
    // ---- the skin nodes ----
    skin: SkinNodes,
    skin_alpha: f64,
    /// HolidayHatAdjust: the sequence's (tgt), the evaluation's (cur) and the fade's start (from).
    hat: [f64; 3],
    comp: UeXf,
    dilation: f64,
    respawning: RespawnFlag,
}

fn gizmo_pose(view: &mut CharView, _clips: &Clips, of: &PoseOf<'_>) -> Box<dyn PoseHook> {
    let keys: Vec<&str> = TOGGLES.iter().chain(FACES.iter()).copied().collect();
    let part: [Option<usize>; PARTS] = std::array::from_fn(|k| of.part_keys.iter().position(|p| p == keys[k]));
    if part.iter().any(Option::is_none) {
        eprintln!("warning: Gizmo pose: the skin lacks a toggled sub-mesh ({})", keys.join(", "));
    }
    let n = view.rest.n;
    let cs = CsPose::new(&view.rig.names, &view.parents);
    let mb = |node: &str, bone: &str, mode: ModifyMode, t: Option<[f64; 3]>, pitch: f64| {
        ModifyBone::new(node, &cs, bone, mode, t, pitch)
    };
    let (mb6, mb7, mb8, mb9) = (
        mb("ModifyBone_6", "fml_un_C_prop1", ModifyMode::Bone, Some([0.0, 0.0, 60.0]), 0.0),
        mb("ModifyBone_7", "umbrellaAtt", ModifyMode::Bone, Some([-20.0, 10.0, 0.0]), 0.0),
        mb("ModifyBone_8", "fml_un_C_prop1", ModifyMode::Bone, Some([40.0, -30.0, -50.0]), 0.0),
        mb("ModifyBone_9", "bowHandle", ModifyMode::Bone, Some([0.0, 15.0, 15.0]), 5.0),
    );
    let (mb10, mb11, mb12, mb13) = (
        mb("ModifyBone_10", "fml_un_COM", ModifyMode::Replace, None, 0.0),
        mb("ModifyBone_11", "fml_un_COM", ModifyMode::Replace, None, 0.0),
        mb("ModifyBone_12", "fml_un_COM", ModifyMode::Replace, None, 0.0),
        mb("ModifyBone_13", "carAtt", ModifyMode::Add, None, 0.0),
    );
    let root_bone = view.parents.iter().position(|&p| p < 0).map_or(-1, |i| i as i32);
    let skin = SkinNodes::new(SKIN_NODES, &AnimNodeSkel { names: &view.rig.names, parents: &view.parents, rest: None });
    let mut p = GizmoPose {
        part,
        tags: std::array::from_fn(|k| keys[k].to_lowercase()),
        last_tick: None,
        tgt: [0.0; 6],
        cur: [0.0; 6],
        from: [0.0; 6],
        fades: view.fade_count(),
        state: [false; 6],
        own: [-1; PARTS],
        sim_vis: [-1; PARTS],
        face_init: false,
        face_left_vis: false,
        fall_lean: 0.0,
        root_bone,
        cs,
        cb: Default::default(),
        mb6,
        mb7,
        mb8,
        mb9,
        mb10,
        mb11,
        mb12,
        mb13,
        lbb: None,
        lbb1: None,
        lbb2: None,
        walk_list: None,
        lantern_seq: None,
        nodes: 0,
        made: false,
        b: Pose::new(n),
        t: Pose::new(n),
        u: Pose::new(n),
        ads: Pose::new(n),
        nav_p: Pose::new(n),
        live: false,
        slaved: of.slaved,
        alive: false,
        car_lean: 0.0,
        is_driving: false,
        attached_on: false,
        falling: false,
        stop_overrides: false,
        fall_alpha: 0.0,
        com_alpha: 0.0,
        com_angle: 0.0,
        lantern_w: 0.0,
        default_w: 0.0,
        over_w: 0.0,
        ww: 0.0,
        m: MontageRead::default(),
        nav: None,
        skin,
        skin_alpha: 0.0,
        hat: [0.0; 3],
        comp: UeXf::default(),
        dilation: 1.0,
        respawning: RespawnFlag::default(),
    };
    p.init();
    Box::new(p)
}

impl GizmoPose {
    /// `init`: the AnimBP's state anew.
    fn init(&mut self) {
        self.tgt = [0.0; 6];
        self.cur = [0.0; 6];
        self.from = [0.0; 6];
        self.state = [false; 6];
        self.own = [-1; PARTS];
        self.sim_vis = [-1; PARTS];
        self.face_init = false;
        self.face_left_vis = false;
        self.fall_lean = 0.0;
        for c in self.cb.iter_mut().flatten() {
            c.initialize();
        }
        self.skin.clear();
        self.hat = [0.0; 3];
        self.respawning.reset();
    }

    /// `makeLayers` (with `makeCopies`): the CopyBones from the data's anim nodes, the layered blends and the lists
    /// from its slot graph.
    fn make_layers(&mut self, w: &World, f: FighterId, view: &CharView) {
        self.made = true;
        let d = &w.char_data(f).fighter;
        let skel = AnimNodeSkel { names: &view.rig.names, parents: &view.parents, rest: None };
        for i in 0..self.cb.len() {
            let node =
                if i == 0 { "AnimGraphNode_CopyBone".to_string() } else { format!("AnimGraphNode_CopyBone_{i}") };
            match d.anim_nodes.as_ref().and_then(|m| m.get(&node)) {
                Some(a) if a.ty == "AnimNode_CopyBone" => {
                    let mut c = CopyBoneNode::new(&a.props, &skel);
                    c.initialize();
                    self.cb[i] = Some(c);
                }
                _ => eprintln!("warning: Gizmo pose: the data has no {node} (the fighter data's anim nodes)"),
            }
        }
        let g = &d.slot_holds.graph;
        self.nodes = g.nodes.len();
        let missing: Vec<&str> = SLOT_NODES
            .iter()
            .copied()
            .filter(|s| !g.nodes.iter().any(|x| matches!(x, SlotGraphNode::Slot { slot, .. } if slot == s)))
            .collect();
        if !missing.is_empty() {
            eprintln!("warning: Gizmo pose: the data's C023A_AnimBP slot graph lacks {}", missing.join(", "));
        }
        let layered = |name: &str| {
            let l = g.nodes.iter().find(|x| matches!(x, SlotGraphNode::Layered { node, .. } if node == name));
            if l.is_none() {
                eprintln!("warning: Gizmo pose: the slot graph has no {name}");
            }
            l.map(|l| LayeredBlend::new(l, &view.rig.names, &view.parents))
        };
        self.lbb = layered("AnimGraphNode_LayeredBoneBlend");
        self.lbb1 = layered("AnimGraphNode_LayeredBoneBlend_1");
        self.lbb2 = layered("AnimGraphNode_LayeredBoneBlend_2");
        self.walk_list = g
            .nodes
            .iter()
            .position(|x| matches!(x, SlotGraphNode::Bool { node, .. } if node == "AnimGraphNode_BlendListByBool_1"));
        self.lantern_seq = g
            .nodes
            .iter()
            .position(|x| matches!(x, SlotGraphNode::Seq { anim: Some(a), .. } if a == "C023A_Lantern_Idle"));
        if self.walk_list.is_none() || self.lantern_seq.is_none() {
            eprintln!("warning: Gizmo pose: the slot graph lacks BlendListByBool_1 or the Lantern_Idle player");
        }
    }

    /// A part's shown state: the AnimBP's write until the sim changes the part's visibility (the later write).
    fn show(&mut self, w: &World, f: FighterId, view: &mut CharView, k: usize) {
        let Some(pt) = self.part[k] else { return };
        let fi = w.fighter(f);
        let listed = w.char_data(f).def.sub_meshes.contains(&self.tags[k]);
        let hidden = w.names.get(&self.tags[k]).is_some_and(|n| fi.pose.sub_hidden.contains(&n));
        let vis = i8::from(listed && !hidden);
        if self.sim_vis[k] >= 0 && self.sim_vis[k] != vis {
            self.own[k] = -1;
        }
        self.sim_vis[k] = vis;
        view.set_part_visible(pt, if self.own[k] >= 0 { self.own[k] == 1 } else { vis == 1 });
    }

    /// `slotOver`: the montage's bow walk track over `src` by the montage's weight into `out`; false when the slot
    /// has no track.
    fn slot_over(&self, cx: &mut PoseCx<'_>, src: &Pose, t: &mut Pose, out: &mut Pose) -> bool {
        let Some((c, time)) = self.m.walk else { return false };
        sample_clip(cx.clips, c, cx.rig, cx.rest, time, t, self.root_bone);
        blend_pose(src, t, self.m.w.min(1.0), out);
        true
    }

    /// `sampleNav`: Navigation (BlendListByBool_2).
    fn sample_nav(&self, cx: &mut PoseCx<'_>, out: &mut Pose) -> bool {
        let Some((c, time)) = self.nav else { return false };
        sample_clip(cx.clips, c, cx.rig, cx.rest, time, out, self.root_bone);
        true
    }

    /// LayeredBoneBlend (the bow): p = layered(node 29's pose, p).
    fn bow_layer(&mut self, cx: &mut PoseCx<'_>, p: &mut Pose) {
        if self.lbb.is_none() || self.walk_list.is_none() {
            return;
        }
        if !self.m.on {
            return; // (no aim slot track: blend and base are both the locomotion)
        }
        if self.m.attack {
            return; // the slot above it plays its own track
        }
        let ww = self.ww;
        // child 1 (BlendListByBool, IsFalling, no blend): Navigation falling, else AfterAim (p)
        let mut u = std::mem::take(&mut self.u);
        let base_u = self.falling && self.sample_nav(cx, &mut u);
        let mut b = std::mem::take(&mut self.b);
        let mut t = std::mem::take(&mut self.t);
        let done = if ww > ZERO_W {
            // child 0: TwoWayBlend (0 / 1) of the BowWalkF / BowWalkB slots over AfterAim
            if !self.slot_over(cx, p, &mut t, &mut b) {
                b.copy(p);
            }
            if ww < FULL_W {
                let base: &Pose = if base_u { &u } else { p };
                blend_pose(base, &b, ww, &mut t);
                b.copy(&t);
            }
            false
        } else if !base_u {
            true
        } else {
            b.copy(&u);
            false
        };
        if !done {
            let lbb = self.lbb.as_mut().expect("checked");
            lbb.evaluate(&mut b, &[Some(&*p)], &[1.0]);
            p.copy(&b);
        }
        self.u = u;
        self.b = b;
        self.t = t;
    }

    fn run_copy(c: &mut Option<CopyBoneNode>, cs: &mut CsEval<'_>, a: f64) {
        let Some(c) = c else { return };
        c.update(TICK, a);
        c.evaluate(cs, &UeXf::default());
    }
}

impl PoseRules for GizmoPose {
    fn rule(&mut self, cx: &mut PoseCx<'_>, p: &mut Pose, dt: f64) {
        if self.live && self.alive && self.lantern_w < FULL_W {
            // under DefaultSlot
            if self.over_w < FULL_W {
                if self.is_driving {
                    let a = 1.0 - self.over_w;
                    let mut cs = self.cs.begin(p);
                    self.mb13.set_pitch(self.car_lean);
                    self.mb13.evaluate(&mut cs, a);
                    self.mb10.set_pitch(self.car_lean);
                    self.mb10.evaluate(&mut cs, a);
                    Self::run_copy(&mut self.cb[4], &mut cs, a);
                } else if self.default_w <= ZERO_W {
                    self.bow_layer(cx, p);
                }
            }
            let att = self.attached_on && !self.stop_overrides;
            if att && (self.lbb1.is_some() || self.lbb2.is_some()) {
                self.ads.copy(p); // AfterDefaultSlot, for LayeredBoneBlend_1
            }
            {
                let mut cs = self.cs.begin(p);
                self.mb11.set_pitch(self.fall_lean);
                self.mb11.evaluate(&mut cs, self.fall_alpha);
                self.mb12.set_pitch(self.com_angle);
                self.mb12.evaluate(&mut cs, self.com_alpha);
                Self::run_copy(&mut self.cb[5], &mut cs, 1.0);
            }
            if att {
                if self.lbb2.is_some() {
                    let mut nav = std::mem::take(&mut self.nav_p);
                    if self.sample_nav(cx, &mut nav)
                        && let Some(l) = &mut self.lbb2
                    {
                        l.evaluate(p, &[Some(&nav)], &[1.0]);
                    }
                    self.nav_p = nav;
                }
                if let Some(l) = &mut self.lbb1 {
                    l.evaluate(p, &[Some(&self.ads)], &[1.0]);
                }
            }
            // the attach chain
            let so = self.stop_overrides;
            let a_att: f64 = if att { 1.0 } else { 0.0 };
            let (bow_visible, keyboard_visible, popcorn_visible) =
                (self.state[BOW], self.state[KEYBOARD], self.state[POPCORN]);
            let a_bow = if (self.attached_on || bow_visible) && !so { 1.0 } else { 0.0 };
            let a_key = a_att.min(if keyboard_visible && !so { 1.0 } else { 0.0 });
            let a_pop = a_att.min(if popcorn_visible && !so { 1.0 } else { 0.0 });
            let mut cs = self.cs.begin(p);
            Self::run_copy(&mut self.cb[3], &mut cs, a_bow);
            Self::run_copy(&mut self.cb[2], &mut cs, a_bow);
            self.mb9.evaluate(&mut cs, a_bow);
            self.mb6.evaluate(&mut cs, a_key);
            Self::run_copy(&mut self.cb[1], &mut cs, a_pop);
            self.mb8.evaluate(&mut cs, a_pop);
            Self::run_copy(&mut self.cb[0], &mut cs, a_att);
            self.mb7.evaluate(&mut cs, a_att);
        }
        if self.live && self.alive {
            // the skin nodes, above the Lantern slot and the mirror node
            let a = self.skin_alpha;
            self.skin.update(dt * self.dilation, |_| a);
            self.skin.evaluate(p, &self.comp);
        }
        // this evaluation's curves, for the next update
        if cx.fade_count != self.fades {
            self.fades = cx.fade_count;
            self.from = self.cur;
            self.hat[2] = self.hat[1];
        }
        let w = cx.fade_weight;
        for k in 0..6 {
            self.cur[k] = if w >= 1.0 { self.tgt[k] } else { self.from[k] + (self.tgt[k] - self.from[k]) * w };
        }
        self.hat[1] = if w >= 1.0 { self.hat[0] } else { self.hat[2] + (self.hat[0] - self.hat[2]) * w };
    }
}

impl PoseHook for GizmoPose {
    /// `preview()`: a view with no fighter: the AnimBP anew, run by preview_step.
    fn preview(&mut self, _view: &mut CharView, _of: &PreviewOf<'_>) {
        self.live = false;
        self.alive = false;
        self.init();
    }

    /// `previewStep(anim, time, facingLeft)`: SetSubmeshVisibility on the curves of the evaluation before
    /// (the AnimBP is the parts' only writer here), SetFaceVisibility, then the curves of the frame's sequence.
    fn preview_step(&mut self, view: &mut CharView, clips: &Clips, s: &PreviewStep<'_>) {
        for t in 0..TOGGLES.len() {
            let on = self.cur[t] >= THRESHOLD;
            if on == self.state[t] {
                continue;
            }
            self.state[t] = on;
            if let Some(p) = self.part[t] {
                view.set_part_visible(p, on);
            }
        }
        if !self.face_init || self.face_left_vis != s.facing_left {
            self.face_left_vis = s.facing_left;
            self.face_init = true;
            let n = TOGGLES.len();
            if let Some(p) = self.part[n] {
                view.set_part_visible(p, !s.facing_left);
            }
            if let Some(p) = self.part[n + 1] {
                view.set_part_visible(p, s.facing_left);
            }
        }
        CURVES.fill(clips, s.anim, s.time, &mut self.tgt);
    }

    fn set(&mut self, w: &World, f: FighterId, view: &mut CharView, clips: &Clips) {
        let fi = w.fighter(f);
        // the respawn: SetupInitialVisibility's write is the later one (the states are kept); the view places no dead
        // fighter, so it is a gap in the ticks
        let tick = w.flow.tick;
        if self.last_tick.is_some_and(|t| tick > t + 1) {
            self.own = [-1; PARTS];
            self.face_init = false;
        }
        self.last_tick = Some(tick);
        self.live = !self.slaved;
        if !self.live {
            return;
        }
        if !self.made {
            self.make_layers(w, f, view);
        }
        self.alive = fi.life.dead == 0.0;
        // the skin nodes' inputs (on the curve of the evaluation before)
        let respawning = self.respawning.step(tick);
        let on = !in_hitstun(w, f) && !respawning && !(self.hat[1] > HAT_MAX);
        self.skin_alpha = f64::from(u8::from(on));
        if !self.skin.is_made() {
            let skel = AnimNodeSkel { names: &view.rig.names, parents: &view.parents, rest: Some(&view.rest) };
            let a = self.skin_alpha;
            self.skin.make(w.char_data(f).fighter.anim_nodes.as_ref(), &skel, "Gizmo pose", |_| a);
        }
        self.dilation = if fi.hit.hitpause > 0.0 { f64::from(mvs_sim::consts::HITPAUSE_DILATION) } else { 1.0 };
        self.comp = mesh_xf(
            f64::from(-mvs_sim::fighter::mesh_wx(w, f)),
            f64::from(mvs_sim::fighter::mesh_wy(w, f)) + view.mesh_z * view.size_scale,
            f64::from(fi.body.facing_dir),
            view.mesh_scale * view.size_scale,
        );
        let gz = gizmo_of(w, f);
        let pz = gz.map(|g| &g.pawn);
        // BlueprintUpdateAnimation's inputs
        let left = fi.body.facing_dir < 0.0;
        let velocity_y = -fi.body.vx;
        self.is_driving = gz.is_some_and(|g| g.driving);
        self.attached_on = gz.is_some_and(|g| g.attach.attached);
        self.falling = is_falling(w, f);
        // C023A_AnimBP's TwoWayBlend input (@6331-6499: BowWalkB when moving away from the facing)
        let back = if left { 0.0 > velocity_y } else { velocity_y > 0.0 };
        self.stop_overrides = pz.is_some_and(|p| p.stop_overrides);
        let car = gz
            .and_then(|g| g.drives)
            .and_then(|id| w.actors.projectiles.get(id))
            .filter(|pr| !pr.dead)
            .and_then(|pr| car_of(&pr.ext));
        if self.is_driving
            && let Some(c) = car
        {
            self.car_lean = f64::from(c.wheelie);
        }
        if pz.is_some_and(|p| p.override_fall_lean) {
            let sel = if left { 1.0 } else { -1.0 };
            let x = clamp01((-f64::from(sel * velocity_y) + BASE_MAX_WALK) / (2.0 * BASE_MAX_WALK));
            self.fall_lean = f_interp_to(
                self.fall_lean,
                FALL_LEAN_AMOUNT + (-FALL_LEAN_AMOUNT - FALL_LEAN_AMOUNT) * x,
                TICK,
                FALL_LEAN_SPEED,
            );
            self.fall_alpha = 1.0;
        } else {
            self.fall_lean = 0.0;
            self.fall_alpha = 0.0;
        }
        let com_override_angle = gz.map_or(0.0, |g| g.com_override);
        self.com_angle = if left { f64::from(-com_override_angle) } else { f64::from(com_override_angle) };
        let bt = pz.map_or(0.0, |p| p.com_blend_time);
        let start = pz.map_or(0.0, |p| p.com_blend_start);
        let gt = fi.clock.time;
        // NormalizeToRange over a 0 range: (x - min) / 0 guarded as 0 / 1
        let k = if bt != 0.0 {
            (gt - start) / bt
        } else if gt - start >= 0.0 {
            1.0
        } else {
            0.0
        };
        self.com_alpha =
            f64::from(gz.map_or(0.0, |g| g.com_override_alpha).min(mvs_sim::math::narrow(1.0 - clamp01(f64::from(k)))));
        // the shown montage's slots
        let sm = mvs_sim::fighter::shown_montage(w, f).and_then(|dash| {
            let st = if dash { fi.montages.dash_montage.as_ref() } else { fi.montages.montage.as_ref() }?;
            let uid = slot_uid(w, f, if dash { MontageSlot::Dash } else { MontageSlot::Main })?;
            Some((st, instance_data(&w.data, w, f, uid)?))
        });
        self.m = MontageRead::default();
        match sm {
            Some((st, m)) => {
                let mw = st.shown_weight(m, &|s| eprintln!("warning: {s}"));
                let has = |slot: &str| st.pose_in(m, slot).is_some();
                self.m.on = true;
                self.m.w = mw;
                self.m.attack = has(if self.falling { "AirAttack" } else { "GroundAttack" });
                if mw > ZERO_W
                    && let Some((si, gi, time)) = st.pose_in(m, if back { "BowWalkB" } else { "BowWalkF" })
                {
                    self.m.walk = clips.get(&m.slots[si].segments[gi].anim).map(|c| (c, time));
                }
                self.lantern_w = if has("Lantern") { mw } else { 0.0 };
                self.default_w = if has("DefaultSlot") { mw } else { 0.0 };
            }
            None => {
                self.lantern_w = 0.0;
                self.default_w = 0.0;
            }
        }
        self.over_w = self.default_w;
        // the bow walk list's weight: the sim's timed weight
        let sb = pz.and_then(|p| p.slot_mix.slot_blend.as_ref());
        self.ww = match (sb, self.walk_list) {
            (Some(sb), Some(wl)) if sb.w.len() == self.nodes => f64::from(sb.w[wl]),
            _ => {
                if !self.falling && fi.body.vx.abs() > 50.0 {
                    1.0
                } else {
                    0.0
                }
            }
        };
        // Navigation: C023A_Lantern_Idle's player while attached, else the linked layer (the locomotion: the loco clip
        // under a montage, the tick's own clip without one)
        let g = &w.char_data(f).fighter.slot_holds.graph;
        let players = pz.and_then(|p| p.slot_mix.slot_players.as_ref());
        let lantern =
            if self.attached_on { self.lantern_seq.and_then(|i| slot_player_pose(g, players, i)) } else { None };
        let (anim, time) = match lantern {
            Some((a, t)) => (a.to_string(), t),
            None if self.m.on => {
                let (la, lt) = loco_clip(w, f);
                (w.names.str(la).to_string(), lt)
            }
            None => (w.names.str(fi.anim.anim).to_string(), fi.anim.anim_time),
        };
        self.nav = clips.get(&anim).map(|c| (c, f64::from(time)));
        // SetSubmeshVisibility on the curves of the evaluation before
        for t in 0..TOGGLES.len() {
            let on = self.cur[t] >= THRESHOLD;
            if on != self.state[t] {
                self.state[t] = !self.state[t];
                self.own[t] = i8::from(self.state[t]);
            }
            self.show(w, f, view, t);
        }
        // SetFaceVisibility
        if !self.face_init || self.face_left_vis != left {
            self.own[TOGGLES.len()] = i8::from(!left);
            self.own[TOGGLES.len() + 1] = i8::from(left);
            self.face_left_vis = left;
            self.face_init = true;
        }
        for k in TOGGLES.len()..PARTS {
            self.show(w, f, view, k);
        }
        // the sequence this tick's pose is sampled from
        CURVES.fill(clips, w.names.str(fi.anim.anim), f64::from(fi.anim.anim_time), &mut self.tgt);
        self.hat[0] = HAT_CURVE.value(clips, w.names.str(fi.anim.anim), 0, f64::from(fi.anim.anim_time));
    }

    /// `reset()`: a new pawn: the AnimBP's state anew (init; wasDead false).
    fn reset(&mut self, _part_scalar: &mut dyn FnMut(usize, &str, f64)) {
        self.init();
        self.last_tick = None;
    }
}

// ---- the arrow-charge component ----

const ARROW_MESH: &str = "C023A_Arrow_Mesh";
const CHARGE_COMPONENT: &str = "ArrowCharge_GEN_VARIABLE";
const CHARGE_TAG: &str = "arrowcharge";

#[derive(Debug)]
struct ArrowCharge {
    v: StaticView,
    socket: String,
    rel: Xform,
}

impl PawnActor for ArrowCharge {
    fn present(&mut self, host: &ActorHost<'_>, w: &World, cx: &PawnCx<'_>, _alpha: f64, out: &mut Vec<ActorPose>) {
        // AddInvisibility hides every mesh of the manager; a dead fighter's view is not drawn
        let listed = w.char_data(cx.fid).def.sub_meshes.iter().any(|t| t == CHARGE_TAG);
        if !listed || !cx.shown(w, CHARGE_TAG) {
            return;
        }
        let Some(at) = host.socket(w, cx.fid, &self.socket, &self.rel) else { return };
        self.v.set(&at);
        let mut meshes = Vec::new();
        self.v.draw(&mut meshes);
        cx.outline(true, &mut meshes);
        out.push(ActorPose { meshes, ..ActorPose::default() });
    }
}

/// `arrowProps`: the ArrowCharge component of the data.
fn arrow_props(host: &ActorHost<'_>, w: &World, f: FighterId) -> Vec<Box<dyn PawnActor>> {
    let d = &w.char_data(f).fighter;
    let Some(c) = d.components.pawn.get(CHARGE_COMPONENT) else {
        eprintln!("warning: Gizmo: the arrow-charge components are not in the data: not drawn");
        return Vec::new();
    };
    let (Some(p), Some(socket)) = (c.p.as_obj(), c.socket.clone()) else {
        eprintln!("warning: Gizmo: the arrow-charge components are not in the data: not drawn");
        return Vec::new();
    };
    let Some(v) = StaticView::new(host, w, f, ARROW_MESH, "") else {
        eprintln!("warning: Gizmo: ArrowCharge is not drawn ({ARROW_MESH} or its materials are not loaded)");
        return Vec::new();
    };
    let rel =
        Xform { loc: xyz(p.get("RelativeLocation"), 0.0), rot: rel_rows(p.get("RelativeRotation")), scale: [1.0; 3] };
    vec![Box::new(ArrowCharge { v, socket, rel })]
}

// ---- the projectile classes' meshes ----
// C023A_Arrow_Projectile_C (sim chars/gizmo/bow.rs Arrow): StaticMesh = C023A_Arrow_Mesh at (-9.27, 0, -2.83), yaw
// -90.00018 under the Capsule root, material override C023A_B_Props_EOG_MIC (the class's actor entry); the actor
// turns with its velocity. C023A_PianoNote_C and C023A_Music_Projectile_C: their StaticMeshes (MusicNote_TrebleClef,
// MusicNote_02) are bVisible false: nothing drawn (the shared draw must not take them).

const ARROW: &str = "C023A_Arrow_Projectile_C";

#[derive(Debug)]
struct ArrowView {
    v: StaticView,
    c: Comp,
}

fn arrow_mesh(host: &mut ActorHost<'_>, w: &World, a: ActorRef, cs: &ActorComponents) -> Option<Box<dyn ActorMesh>> {
    let ActorRef::Projectile(id) = a else { return None };
    let pr = w.actors.projectiles.get(id)?;
    let c = cs.get("StaticMesh")?;
    let from = pr.owner?;
    let v = StaticView::new(host, w, from, ARROW, "")?;
    Some(Box::new(ArrowView { v, c: Comp { par: c.parent.clone(), rl: c.loc, rot: c.rot, rs: c.scale } }))
}

impl ActorMesh for ArrowView {
    fn place(
        &mut self,
        _host: &mut ActorHost<'_>,
        _w: &World,
        at: &ActorPlace,
        alpha: f64,
        shown: bool,
        out: &mut Vec<MeshPose>,
    ) {
        self.v.visible = shown;
        if !shown {
            return;
        }
        let (x, y) = at.at(alpha);
        self.v.set(&place_under(&self.c, &at.rot, at.scale, x, y));
        self.v.draw(out);
    }
}

#[derive(Debug)]
struct NoMesh;

impl ActorMesh for NoMesh {
    fn place(&mut self, _: &mut ActorHost<'_>, _: &World, _: &ActorPlace, _: f64, _: bool, _: &mut Vec<MeshPose>) {}
}

fn no_mesh(_: &mut ActorHost<'_>, _: &World, _: ActorRef, _: &ActorComponents) -> Option<Box<dyn ActorMesh>> {
    Some(Box::new(NoMesh))
}

// ---- the car ----
// C023A_Car_SpawnedActor_C as a match draws it (the sim: mvs-sim chars/gizmo/car.rs GizmoCar).
// Components (the class's SCS): the Capsule root; CarMesh (an empty StaticMeshComponent, bVisible false) at
// (38.85, -0.95, -55.10), yaw -90, scale 2; under it SkeletalMesh (C023A_Corvette_SkelMesh, AnimClass
// C023A_Corvette_SkelMesh_Skeleton_AnimBlueprint) at yaw 89.99999, scale 0.3; ParticleSystem (PS_C023A_OverheatCar,
// bVisible false) on its socket corvette_front: the effects'.
// Placement: driven by a BPI_C023A pawn, the actor is attached to his carAtt (AnimNotify_Spawner AttachToActor,
// UpdateDriverInfo @327; the sim's Projectile.follow), so the root is the socket's transform (ActorHost::socket);
// driven by another class (fml_un_COM on his carried pose, the sim's CarActor.ride) and undriven, the actor's place
// and rotation (ProjectileMovement bRotationFollowsVelocity: actor_rot).
// Deviation: under a driver of another class the car is at the sim's place on the carried pose while his own pose is
// drawn (the view has no carried pose), turned by the actor's side, not by the fml_un_COM bone.
// Material (BeginPlay @6141-6557): slot 0 a dynamic instance of C023A_Corvette_Overheat_MIC for the local player, else
// C023A_Props_A_MIC_Red / _Blue by the colour index (the class's materials and red / blue variants); SetTeamColor's
// team (@15846) and BlinkAmount (the sim's blink).
// C023A_Corvette_SkelMesh_Skeleton_AnimBlueprint:
//   Root <- BlendBoneByChannel (bone space, alpha 1, rotation only: rearWheel_L -> rearWheel_R, rearWheel_R ->
//   rearWheel_L, frontWheel_L -> frontWheel_R, frontWheel_R -> frontWheel_L; A = ApplyAdditive, B = C023A_Car_TireSpin
//   looped at PlayRate RPS)
//   ApplyAdditive (alpha 1): base = Slot DefaultSlot (the car's montage, C023A_Car_Damage_Montage: ActorSlot) <-
//   BlendListByBool IsInHit (blend 0): true BlendListByInt HitInt (C023A_Car_HR_N_01 / 02 / 03, looped, blend 0, no
//   reset on activation), false the machine Movement (one state: TwoWayBlend Idle = C023A_Car_Idle, Run =
//   C023A_Car_Run, alpha FClamp(RPS, 0, 1); bReinitializeOnBecomingRelevant); additive = the machine AdditiveState
//   (entry Idle = C023A_Car_Idle_Additive_Idle looped, Run = _Additive_Start, Stop = _Additive_Stop; Idle -> Run Speed
//   >= TransitionSpeed, Run -> Stop Speed < TransitionSpeed, Stop -> Run Speed >= TransitionSpeed (crossfade 0.05),
//   Stop -> Idle Stop's remaining time <= 0; MaxTransitionsPerFrame 3, bSkipFirstUpdateTransition).
//   BlueprintUpdateAnimation (@582-751): Speed = |actor location - StoredLocation| / dt, StoredLocation = the location,
//   RPS = 2600 / Speed (Divide_FloatFloat: 0 for a 0 divisor). TransitionSpeed 1500 (the CDO).
//   The additives' RefPoseSeq: C023A_Car_Idle_Additive_Null frame 0.
// HitShake (C023A_Car_SpawnedActor's at ReceiveAnyDamage @12741, its Update @18047-18136; IsInHit = true with HitInt
// @12741 and false at its Finished @14793): the sim's shake_age / damage.
// Deviation: the AnimBP runs at a drawn frame that sees a new car tick (each tick at 60 drawn frames a
// second or more; a slower rate or a rollback's jump of several ticks runs them with the latest place); Speed is the
// sim place's (the drawn root rides the socket of the last drawn pose).
// Deviation: AdditiveState's 0.05 s crossfade (Stop -> Run) is a cut.
// TurnVisuals (the run trail and tire tracks at a DriverTurnEvent) are the effects' (mvs-render fx/chars/gizmo.rs).

const CAR: &str = "C023A_Car_SpawnedActor_C";
const STEP: f64 = 1.0 / 60.0;
/// The CDO.
const TRANSITION_SPEED: f64 = 1500.0;
const RPS_NUM: f64 = 2600.0;
const WHEELS: [(&str, &str); 4] = [
    ("rearWheel_L", "rearWheel_R"),
    ("rearWheel_R", "rearWheel_L"),
    ("frontWheel_L", "frontWheel_R"),
    ("frontWheel_R", "frontWheel_L"),
];
const ADD_SEQ: [&str; 3] =
    ["C023A_Car_Idle_Additive_Idle", "C023A_Car_Idle_Additive_Start", "C023A_Car_Idle_Additive_Stop"];
const HR_SEQ: [&str; 3] = ["C023A_Car_HR_N_01", "C023A_Car_HR_N_02", "C023A_Car_HR_N_03"];
/// AdditiveState's states.
const AS_IDLE: usize = 0;
const AS_RUN: usize = 1;
const AS_STOP: usize = 2;

// HitShake of the cars (C023A_Car_SpawnedActor and Parent_Car_SpawnedActor, the same timeline in each):
// HitShakeTimeline (0.2 s, its CurveFloat_0 linear) played from the start at
// ReceiveAnyDamage; its Update sets CarMesh's relative location to (21.193443, HitShakeAlpha x 300, -55.10111)
// (Parent_Car_SpawnedActor @17515-17604). After the first shake the X stays 21.19 (the Update's constant against the
// template's 38.85), as the build. The sim's CarActor.shake_age is the car's age at the PlayFromStart.
const SHAKE_LEN: f64 = 0.2;
/// CurveFloat_0 (linear keys: time, value).
const SHAKE: [f64; 14] = [
    0.0,
    1.0014443,
    0.021089733,
    -0.72670245,
    0.06237288,
    0.8865422,
    0.10470116,
    -0.603554,
    0.1386683,
    0.44320774,
    0.17106774,
    -0.18484926,
    0.2,
    0.0,
];
const SHAKE_X: f64 = 21.193443;
const SHAKE_Y: f64 = 300.0;
const SHAKE_Z: f64 = -55.10111;

/// HitShakeAlpha at the timeline's time t (s).
fn shake_at(t: f64) -> f64 {
    if t <= 0.0 {
        return SHAKE[1];
    }
    let mut i = 2;
    while i < SHAKE.len() {
        if t <= SHAKE[i] {
            let (t0, v0) = (SHAKE[i - 2], SHAKE[i - 1]);
            return v0 + ((SHAKE[i + 1] - v0) * (t - t0)) / (SHAKE[i] - t0);
        }
        i += 2;
    }
    SHAKE[SHAKE.len() - 1]
}

/// CarMesh's relative location: the shake's while one has run (shake_age >= 0 once), else the template's.
pub(crate) fn car_mesh_loc(age: i32, shake_age: i32, tmpl: [f64; 3]) -> [f64; 3] {
    if shake_age < 0 {
        return tmpl;
    }
    let t = f64::from(age - shake_age) / 60.0;
    [SHAKE_X, if (0.0..SHAKE_LEN).contains(&t) { shake_at(t) * SHAKE_Y } else { 0.0 }, SHAKE_Z]
}

#[derive(Debug)]
struct CarView {
    id: ProjectileId,
    sk: SkinnedActor,
    slot: ActorSlot,
    base: Pose,
    tmp: Pose,
    spin: Pose,
    ta: Pose,
    /// Per WHEELS row: source, target.
    wheel: [(Option<usize>, Option<usize>); 4],
    idle: Option<usize>,
    run: Option<usize>,
    hr: [Option<usize>; 3],
    add: [Option<usize>; 3],
    tire: Option<usize>,
    // the AnimBP's state
    last_age: i32,
    stored: [f64; 3],
    speed: f64,
    rps: f64,
    tire_t: f64,
    idle_t: f64,
    run_t: f64,
    hr_t: [f64; 3],
    was_in_hit: bool,
    state: usize,
    as_t: f64,
    as_first: bool,
    damage: f64,
    // the placement
    mesh_rot: Rows,
    cm_rot: Rows,
    cm_loc: [f64; 3],
    cm_scale: [f64; 3],
    sk_loc: [f64; 3],
    sk_scale: [f64; 3],
}

fn car_view(host: &mut ActorHost<'_>, w: &World, a: ActorRef, _cs: &ActorComponents) -> Option<Box<dyn ActorMesh>> {
    let ActorRef::Projectile(id) = a else { return None };
    let pr = w.actors.projectiles.get(id)?;
    let owner = pr.owner?;
    let variant = host.variant(w, owner);
    let Some((path, keys)) =
        host.draws_of(w, owner).and_then(|d| d.actor(CAR, (variant != "local").then_some(variant)))
    else {
        eprintln!("warning: Gizmo: the car is not drawn ({CAR}'s mesh or its materials are not loaded)");
        return None;
    };
    let Some(info) = host.mesh(&path) else {
        eprintln!("warning: Gizmo: the car is not drawn ({path} is not loaded)");
        return None;
    };
    let sk = SkinnedActor::new(info, &keys);
    let n = sk.names.len();
    let clips = host.clips_of(w, owner)?;
    let clip = |c: &str| {
        let i = clips.get(c);
        if i.is_none() {
            eprintln!("warning: Gizmo car: the clip is not loaded: {c}");
        }
        i
    };
    let wheel = WHEELS.map(|(s, t)| {
        let (a, b) = (sk.names.iter().position(|x| x == s), sk.names.iter().position(|x| x == t));
        if a.is_none() || b.is_none() {
            eprintln!("warning: Gizmo car: a wheel bone is not in the skeleton: {s} / {t}");
        }
        (a, b)
    });
    // (CarMesh has no mesh: the raw component)
    let raw = |n: &str| {
        class_def(w, a).and_then(|d| d.components.get(n)).and_then(|c| c.as_obj()).map(|c| Xform {
            loc: xyz(c.get("RelativeLocation"), 0.0),
            rot: rel_rows(c.get("RelativeRotation")),
            scale: xyz(c.get("RelativeScale3D"), 1.0),
        })
    };
    let (cm, skc) = (raw("CarMesh"), raw("SkeletalMesh"));
    if cm.is_none() || skc.is_none() {
        eprintln!("warning: Gizmo car: CarMesh or SkeletalMesh missing from the class data");
    }
    let ident = crate::actor_view::IDENT;
    let cm_rot = cm.as_ref().map_or(ident, |c| c.rot);
    let sk_rot = skc.as_ref().map_or(ident, |c| c.rot);
    Some(Box::new(CarView {
        id,
        slot: ActorSlot::new("DefaultSlot", n),
        base: Pose::new(n),
        tmp: Pose::new(n),
        spin: Pose::new(n),
        ta: Pose::new(n),
        wheel,
        idle: clip("C023A_Car_Idle"),
        run: clip("C023A_Car_Run"),
        hr: HR_SEQ.map(clip),
        add: ADD_SEQ.map(clip),
        tire: clip("C023A_Car_TireSpin"),
        last_age: -1,
        stored: [0.0; 3],
        speed: 0.0,
        rps: 0.0,
        tire_t: 0.0,
        idle_t: 0.0,
        run_t: 0.0,
        hr_t: [0.0; 3],
        was_in_hit: false,
        state: AS_IDLE,
        as_t: 0.0,
        as_first: true,
        damage: 0.0,
        mesh_rot: compose_rows(&sk_rot, &cm_rot),
        cm_rot,
        cm_loc: cm.as_ref().map_or([0.0; 3], |c| c.loc),
        cm_scale: cm.as_ref().map_or([1.0; 3], |c| c.scale),
        sk_loc: skc.as_ref().map_or([0.0; 3], |c| c.loc),
        sk_scale: skc.as_ref().map_or([1.0; 3], |c| c.scale),
        sk,
    }))
}

/// A clip's length (0 for none).
fn clip_len(clips: &Clips, c: Option<usize>) -> f64 {
    c.map_or(0.0, |c| clips.clips[c].anim.length)
}

/// a = a + (b - a) w (rotations by the shorter way, normalised), in place, through the pose's float32.
fn blend_in_place(a: &mut Pose, b: &Pose, w: f64) {
    let f = f64::from;
    for i in 0..a.t.len() {
        a.t[i] = (f(a.t[i]) + (f(b.t[i]) - f(a.t[i])) * w) as f32;
        a.s[i] = (f(a.s[i]) + (f(b.s[i]) - f(a.s[i])) * w) as f32;
    }
    for i in 0..a.n {
        let o = i * 4;
        let dot: f64 = (0..4).map(|k| f(a.r[o + k]) * f(b.r[o + k])).sum();
        let sg = if dot < 0.0 { -1.0 } else { 1.0 };
        let mut len = 0.0;
        for k in 0..4 {
            a.r[o + k] = (f(a.r[o + k]) * (1.0 - w) + sg * f(b.r[o + k]) * w) as f32;
            len += f(a.r[o + k]) * f(a.r[o + k]);
        }
        let len = len.sqrt();
        let len = if len == 0.0 || len.is_nan() { 1.0 } else { len };
        for k in 0..4 {
            a.r[o + k] = (f(a.r[o + k]) / len) as f32;
        }
    }
}

impl CarView {
    fn hit_int(&self) -> usize {
        if (0.0..3.0).contains(&self.damage) { self.damage as usize } else { 0 }
    }

    /// One BlueprintUpdateAnimation and the graph's update (dt one tick).
    fn update(&mut self, clips: &Clips, in_hit: bool) {
        // RPS from the Speed of the update before (the ubergraph's order: Speed, then StoredLocation and RPS)
        self.rps = if self.speed != 0.0 { RPS_NUM / self.speed } else { 0.0 };
        // AdditiveState
        if self.as_first {
            self.as_first = false;
        } else {
            for _ in 0..3 {
                let s = self.state;
                let to = if s == AS_IDLE && self.speed >= TRANSITION_SPEED {
                    Some(AS_RUN)
                } else if s == AS_RUN && self.speed < TRANSITION_SPEED {
                    Some(AS_STOP)
                } else if s == AS_STOP {
                    if self.speed >= TRANSITION_SPEED {
                        Some(AS_RUN)
                    } else if clip_len(clips, self.add[AS_STOP]) - self.as_t <= 0.0 {
                        Some(AS_IDLE)
                    } else {
                        None
                    }
                } else {
                    None
                };
                let Some(to) = to else { break };
                self.state = to;
                self.as_t = 0.0;
            }
        }
        let len = clip_len(clips, self.add[self.state]);
        self.as_t = if self.state == AS_IDLE {
            if len > 0.0 { (self.as_t + STEP) % len } else { 0.0 }
        } else {
            (self.as_t + STEP).min(len)
        };
        // IsInHit's list (blend 0): the hit players, else Movement (reinitialised when it becomes relevant again)
        if in_hit {
            let i = self.hit_int();
            let l = clip_len(clips, self.hr[i]);
            if self.hr[i].is_some() && l > 0.0 {
                self.hr_t[i] = (self.hr_t[i] + STEP) % l;
            }
        } else {
            if self.was_in_hit {
                self.idle_t = 0.0;
                self.run_t = 0.0;
            }
            let a = self.rps.clamp(0.0, 1.0);
            let il = clip_len(clips, self.idle);
            if a < 1.0 && il > 0.0 {
                self.idle_t = (self.idle_t + STEP) % il;
            }
            let rl = clip_len(clips, self.run);
            if a > 0.0 && rl > 0.0 {
                self.run_t = (self.run_t + STEP) % rl;
            }
        }
        self.was_in_hit = in_hit;
        let l = clip_len(clips, self.tire);
        if l > 0.0 {
            self.tire_t = (((self.tire_t + STEP * self.rps) % l) + l) % l;
        }
    }

    fn evaluate(&mut self, clips: &mut Clips, w: &World, in_hit: bool) {
        let mut base = std::mem::take(&mut self.base);
        let mut tmp = std::mem::take(&mut self.tmp);
        let mut spin = std::mem::take(&mut self.spin);
        let mut ta = std::mem::take(&mut self.ta);
        let i = self.hit_int();
        let sk = self.sk.skeleton(clips);
        if in_hit {
            match self.hr[i] {
                Some(c) => sample_clip(clips, c, sk.rig, sk.rest, self.hr_t[i], &mut base, sk.root),
                None => base.copy(sk.rest),
            }
        } else {
            let a = self.rps.clamp(0.0, 1.0);
            match self.idle {
                Some(c) => sample_clip(clips, c, sk.rig, sk.rest, self.idle_t, &mut base, sk.root),
                None => base.copy(sk.rest),
            }
            if a > 0.0
                && let Some(c) = self.run
            {
                sample_clip(clips, c, sk.rig, sk.rest, self.run_t, &mut tmp, sk.root);
                blend_in_place(&mut base, &tmp, a);
            }
        }
        let anim = w.actors.projectiles.get(self.id).and_then(|pr| car_of(&pr.ext)).map(|c| &c.anim);
        let inst = anim.and_then(|an| an.m.as_ref()).and_then(|st| {
            let m = montage_of(&w.data, w, ActorRef::Projectile(self.id), w.names.str(st.montage.name))?;
            Some((st, m))
        });
        let slotted = self.slot.apply(clips, &sk, inst, &base, &mut tmp);
        let p = if slotted { &mut tmp } else { &mut base };
        if let Some(c) = self.add[self.state] {
            apply_additive(clips, c, sk.rig, sk.rest, self.as_t, p, &mut ta);
        }
        if let Some(c) = self.tire {
            sample_clip(clips, c, sk.rig, sk.rest, self.tire_t, &mut spin, sk.root);
            for &(s, t) in &self.wheel {
                let (Some(s), Some(t)) = (s, t) else { continue };
                p.r[t * 4..t * 4 + 4].copy_from_slice(&spin.r[s * 4..s * 4 + 4]);
            }
        }
        self.sk.show(p);
        self.base = base;
        self.tmp = tmp;
        self.spin = spin;
        self.ta = ta;
    }
}

impl CarView {
    /// The AnimBP, once per tick of the car's age, from place once the car is shown: each tick at a drawn
    /// frame rate of 60 or more.
    fn anim(&mut self, host: &mut ActorHost<'_>, w: &World) {
        let Some(pr) = w.actors.projectiles.get(self.id).filter(|pr| !pr.dead) else { return };
        let ProjectileExt::GizmoCar(gc) = &pr.ext else { return };
        let car = &gc.car;
        let Some(owner) = pr.owner else { return };
        let age = pr.age;
        if age == self.last_age {
            return;
        }
        let n = if self.last_age < 0 || age < self.last_age { 1 } else { (age - self.last_age).min(30) };
        self.last_age = age;
        self.damage = f64::from(car.damage);
        let tl = if car.shake_age >= 0 { f64::from(age - car.shake_age) / 60.0 } else { -1.0 };
        let in_hit = (0.0..SHAKE_LEN).contains(&tl);
        // Speed: the actor's sim place (UE: 0, -x, y), over the ticks since the last update
        let u = [0.0, -pr.x, pr.y];
        let d = hypot(&[
            f64::from(u[0]) - self.stored[0],
            f64::from(u[1]) - self.stored[1],
            f64::from(u[2]) - self.stored[2],
        ]);
        self.stored = u.map(f64::from);
        self.speed = d / (f64::from(n) * STEP);
        let Some(clips) = host.clips_of(w, owner) else { return };
        for _ in 0..n {
            self.update(clips, in_hit);
        }
        self.evaluate(clips, w, in_hit);
    }
}

impl ActorMesh for CarView {
    fn place(
        &mut self,
        host: &mut ActorHost<'_>,
        w: &World,
        at: &ActorPlace,
        alpha: f64,
        shown: bool,
        out: &mut Vec<MeshPose>,
    ) {
        let Some(pr) = w.actors.projectiles.get(self.id).filter(|pr| !pr.dead) else {
            self.sk.visible = false;
            return;
        };
        let ProjectileExt::GizmoCar(gc) = &pr.ext else {
            self.sk.visible = false;
            return;
        };
        let car = &gc.car;
        self.sk.visible = shown;
        if !shown {
            return;
        }
        let Some(owner) = pr.owner else { return };
        // the root: a BPI_C023A driver's carAtt while attached, else the actor's place and rotation
        let driver = car.driver.or(pr.owner).filter(|&d| gizmo_of(w, d).is_some());
        let root = if pr.attached {
            driver.and_then(|d| {
                host.socket(w, d, "carAtt", &Xform { loc: [0.0; 3], rot: crate::actor_view::IDENT, scale: [1.0; 3] })
            })
        } else {
            None
        };
        let root_rot = root.map_or_else(|| actor_rot(w, pr), |r| r.rot);
        let rs = root.map_or([1.0; 3], |r| r.scale);
        let age = pr.age;
        self.anim(host, w);
        let team = host.colour(w, owner).team;
        for m in self.sk.mats.iter_mut().flatten() {
            set_scalar(m, "Team", team);
            set_scalar(m, "BlinkAmount", f64::from(car.blink));
        }
        // CarMesh's relative location (the shake's Update writes it), then SkeletalMesh under it
        let cl = car_mesh_loc(age, car.shake_age, self.cm_loc);
        let (cr, cs, sl) = (&self.cm_rot, self.cm_scale, self.sk_loc);
        // the mesh in the root's frame: CarMesh's location + its rotation of (its scale x SkeletalMesh's location)
        let m: [f64; 3] = std::array::from_fn(|k| {
            cl[k] + cs[0] * sl[0] * cr[k] + cs[1] * sl[1] * cr[3 + k] + cs[2] * sl[2] * cr[6 + k]
        });
        let s = at.scale;
        let (x, y) = at.at(alpha);
        let loc = std::array::from_fn(|k| {
            let o = match root {
                Some(r) => r.loc[k],
                None => [0.0, -x, y][k],
            };
            o + s * (m[0] * rs[0] * root_rot[k] + m[1] * rs[1] * root_rot[3 + k] + m[2] * rs[2] * root_rot[6 + k])
        });
        let rot = compose_rows(&self.mesh_rot, &root_rot);
        let scale = std::array::from_fn(|k| s * rs[k] * cs[k] * self.sk_scale[k]);
        self.sk.set(&Xform { loc, rot, scale });
        self.sk.draw(out);
    }
}
