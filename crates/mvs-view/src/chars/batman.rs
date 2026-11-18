//! Batman's render module: the entry, the AnimBP's pose hook with Pawn_BatmanV2's
//! idle fade, and the classes' mesh rules (the grapple hook, the batarang). His effects are mvs-render
//! fx/chars/batman.rs.
//!
//! The graph, input first (LinkID -> ChildProperties):
//!   BlendListByBool (bActiveValue <- IsGliding; BlendTime [0, 0.1], linear, bResetChildOnActivation false):
//!     child 0, the glide stack: ApplyAdditive(Base = ApplyAdditive_1(Base = BlendSpacePlayer_1
//!       Batman_Nav_GlideLoop_BlendSpace (X <- SpringJoint1_offsetY), Additive = the state machine GlideAdditive),
//!       Additive = BlendSpacePlayer Batman_Nav_GlideAdd_BlendSpace (X <- SpringJoint2_offsetY)), both alphas 1;
//!     child 1: the linked layer (Base_AnimBP_Batman's BaseMovementMachine: the shared pose, the sim's clip).
//!   LocalToComponent -> ModifyBone_4 (Root, translation BMM_Additive in BCS_WorldSpace <- DepthVector, alpha <-
//!   IsGlidingAlpha) -> SpringBone_4 (SpringJoint1) -> _3 (SpringJoint2) -> _2 (L foot marker) -> _1 (R foot marker)
//!   -> LegIK (R) -> LegIK_1 (L) (alpha <- IsGlidingAlpha) -> ComponentToLocal -> the cached pose under the aim slots'
//!   TwoWayBlends (their alphas are the sim's slot mix) -> Slot DefaultSlot -> Slot OverrideSlot -> Slot Batarang ->
//!   SpringBone (the samurai skin's bone) -> the mirror node (batman_MirrorTableV2, bEnabled <- FacingLeft) ->
//!   ModifyBone (head, HeadScale) -> Constraint_5, _4, _2, _3, _1, Constraint -> ModifyBone_3, _1, _2 (VB bones) ->
//!   13 AnimDynamics and 4 Trail nodes -> ComponentToLocal.
//!
//! The facing. batman_MirrorTableV2 flips every bone (none kept) and swaps the L / R pairs (fml_un_L_PropOffset <->
//! fml_un_R_PropOffset among them); the port draws a left facing as the reflection of the right-facing view
//! (CharView::place; Unverified: the mirror node taken as a full reflection). The nodes below the mirror run here on
//! the unmirrored pose with the inputs the graph gives them at both facings. The Constraints after the mirror are
//! taken at their right-facing set at both facings: OnFacingChanged (@244-374) gives FacingLeftAlpha / Inverted, and
//! the facing-left set (Constraint_4: prop1 -> L, alpha prop1RPropL; Constraint_3: prop1 -> R, prop1LPropL;
//! Constraint: prop2 -> L, 1) is the right-facing one (Constraint_5: prop1 -> R, prop1RPropL; Constraint_2: prop1 -> L,
//! prop1LPropL; Constraint_1: prop2 -> R, 1) on the swapped bones of the mirrored pose: the reflection of the
//! right-facing result.
//!
//! The glide stack's state is the sim's (mvs-sim chars/batman/glide.rs, one port): the BlendListByBool's weight,
//! IsGlidingAlpha, the GlideAdditive machine's state and time, the two blend space players' inputs and times, and
//! SpringBone_4 / _3 with the read back of SpringJoint1_offsetY / SpringJoint2_offsetY (@1716-2639), the machine's
//! restart at GlideEnter after a hold of the Slot nodes among them. `set` copies them; the view runs neither those
//! springs (their joints have no child and no vertex in the default skin's three meshes, so their drawn place is
//! nothing but that read back) nor the players' update.
//! The update (BlueprintUpdateAnimation, @10853 on), on the curves of the evaluation before:
//!   - the capes (@10902-11229, @3882-4234, @5885-6228): capeActive < 3 after >= 3: CapeDetailedA / B shown, CapeFullA
//!     / B hidden (and CapeFullC hidden, @3822); >= 3 after < 3: the detailed ones hidden, CapeFullA / B / C shown;
//!     PreviousFrameCapeActiveValue = capeActive each update (class default 4). Through
//!     VisibilityManager.SetSubMeshVisibility, which is only set after a Delay(0) (@481): the port's own state of the
//!     two parts the default skin has (CapeDetailedA: Batman_Cape2_SkelMesh, CapeFullA: Batman_Cape3_SkelMesh), over
//!     the sim's (sub_hidden: SetupInitialVisibility hides CapeFullA, which has no PrimaryMesh tag).
//!   - IsGlidingAlpha = float(IsGliding) (@4239); the Constraint alphas from prop1LPropL / prop1RPropL (@11234-11663).
//!     The curves: the clips' own (crate::curves) by sequence and time, as Arya's.
//!
//! Pawn_BatmanV2's IdleFadeTimeline (@7832: BPI_IdleFade(On) plays it on true, reverses it on false; its update
//! @7734 -> @6215): BaseColor_Mult and Roughness_Power = alpha on the Mesh and the capes (BodyB / BodyC have no mesh
//! in the default skin), Team on the Mesh = alpha >= 0.6 ? 0.5 : ColorIndex (@7118-7294). The curve (0.25 s, four
//! cubic keys) is the timeline's IdleFadeTimeline_Alpha track. BPI_IdleFade's callers (SetGlideValueMulticast @5991,
//! AnimNotify_Darkness begin / end) are the sim's: BatmanState.idle_fade holds the last On.
//!
//! Deviations and unverified models:
//!   - Unverified: the glide stack, ModifyBone_4 and the spring / leg IK nodes run on the clip the sim names before the
//!     view's crossfade between locomotion clips (the build's crossfade is inside the linked layer, before them);
//!     under a montage blending out they run on the source track (CharView::slot_under), and a montage blending in
//!     fades from the last shown pose (the view's rule: the source is not evaluated there).
//!   - Child 1, the linked layer, is the clip with the sim's slot mix (the Fall blend space's samples) and Base's
//!     FallLean / hit turn of fml_un_COM, all taken before the BlendListByBool and the chain (PoseRules::takes_layer,
//!     PoseCx::sample_layer); its curves are the clip's alone (the mix's not taken).
//!   - Unverified: the update (the foot marker springs and the leg IKs here; the glide players, the machine, the
//!     BlendListByBool and SpringBone_4 / _3 in the sim, by the same rule) is gated as the build's Slot nodes gate their
//!     source (bAlwaysUpdateSourcePose: true for DefaultSlot and the five aim slots, false for OverrideSlot and
//!     Batarang): a shown montage at full weight that holds the source stops it, by the sim's rule (holds_source: a
//!     non-empty, non-additive track in every slot of a slotHolds.stages set, here [Batarang] and [OverrideSlot]).
//!     The BlendListByBool's weights freeze with it; IsGlidingAlpha (the update's variable) does not.
//!   - Unverified: hitpause: the mesh's CustomTimeDilation 0.005 (the anim update's DeltaTime) while the fighter's
//!     hitpause > 0; the world's dilation is 1. The idle fade timeline takes the same dilation.
//!   - Unverified: a curve's value in a crossfade: as Arya's (the view's fade weight); the glide stack's curves are the
//!     GlideLoop blend space rows' (the additive inputs carry no curve in the data).
//!   - Deviation: the capes' first state: the sim's SetupInitialVisibility (detailed shown, full hidden), not the
//!     components' bVisible false.
//!   - The reset (a Lab reset: the AnimBP's and the timeline's state anew): PoseHook::reset.
//!     Deviation: the reset leaves the fade's materials as the timeline left them (it writes no scalar).
//!   - The menu preview (no fighter, PoseHook::preview / preview_step): facing right, no glide, no fade; the cape swap
//!     runs (its branch tests only the owning actor's VisibilityManager).
//!   - ModifyBone (fml_un_C_head scale from HeadScale, the big head modifier): the shared head-scale step.
//!   - not ported: ModifyBone_1 / _2 / _3 (VB L_LowLeg, VB R_LowLeg, VB Spine: virtual bones the view does not hold).
//!   - The samurai skin's nodes (bones no other skin's meshes have: on another skin none is made), from the fighter's
//!     data at the first set (crate::anim_nodes::SkinNodes): the SpringBone after the slots (SAMURAI_SPRING, before
//!     the Constraints, alpha float(!Is Hit Stunned && !Is Respawning) @8937-9070) and the 13 AnimDynamics and 4
//!     Trail nodes after the VB ModifyBones (SAMURAI_NODES, after the Constraints, each alpha float(!Is Respawning &&
//!     !Is Hit Stunned) x float(DisableS02Dynamics < 0.1) @4573-4864 and each node's, the curve the evaluation's
//!     before). Their update runs above the Slot nodes (not under their gate) with the hitpause dilation. Is Respawning
//!     is crate::anim_nodes::RespawnFlag's. The nodes after the mirror run on the unmirrored pose with the component's
//!     transform at the actor's facing (mesh_xf), as Reindog's (chars/creature/skin_controls.rs, its Deviation).
//!     Deviation: the head-scale step (ModifyBone, before the Constraints) is the view's after the rule, so they read
//!     the pose before the big head's scale. Not run in a preview (no fighter data).

use mvs_data::value::Props;
use mvs_sim::World;
use mvs_sim::actors::ActorRef;
use mvs_sim::actors::item::ItemExt;
use mvs_sim::actors::world_actor::WorldActorState;
use mvs_sim::chars::batman::grapple::grapple_of;
use mvs_sim::chars::batman::state::batman_of;
use mvs_sim::fighter::anim::MontageSlot;
use mvs_sim::fighter::anim::montages::{instance_data, slot_uid};
use mvs_sim::fighter::{FighterId, in_hitstun};
use mvs_sim::names::Name;
use mvs_vfx::math::{V3, compose_rows, rotation_rows};

use super::{CharRender, PawnComponents, PoseHook, PoseOf, PreviewOf, PreviewStep};
use crate::actor_view::{Comp, StaticView, Xform, comp_of, place_at};
use crate::actors::{ActorComponents, ActorHost, ActorMesh, ActorPlace, ComponentKind, MeshRule, Rows, class_def};
use crate::anim::{
    Clips, HeadScaleNode, Pose, accumulate_held, add_weighted_pose, apply_additive, normalize_rotations, weight_pose,
};
use crate::anim_nodes::{AnimNodeChain, AnimNodeSkel, BoneXfs, CsPose, RespawnFlag, SkinNodes, UeXf, mesh_xf};
use crate::blend::{BlendSpacePlayer, BlendSpaceTable};
use crate::curves::{CurveKey, CurveTable, rich_curve};
use crate::frame::MeshPose;
use crate::pose::{MeshMap, Rec, material, set_scalar};
use crate::rotator::through_rotator;
use crate::skins::FighterDraws;
use crate::view::{CharView, PoseCx, PoseRules};

pub(crate) const RENDER: CharRender = CharRender {
    // The big head: Batman_AnimBP's ModifyBone (scale Additive, bone space, <- HeadScale; Pawn_BatmanV2 @7973).
    head_scale: &[HeadScaleNode {
        node: "ModifyBone",
        bone: "fml_un_C_head",
        factor: 1.0,
        gated: false,
        moves: [0.0; 3],
        scale: true,
    }],
    blend_spaces: Some(&BLEND_SPACES),
    pose: Some(batman_pose),
    components: Some(PawnComponents { make: bomb_make, place: bomb_place, reset: None }),
    ..CharRender::GENERIC
};

/// The classes' own mesh drawing, by class name.
pub(crate) const RULES: &[(&str, MeshRule)] = &[
    (GRAPPLE, MeshRule { rot: None, mesh: Some(hook_mesh) }),
    (PERK_GRAPPLE, MeshRule { rot: None, mesh: Some(hook_mesh) }),
    (BATARANG, MeshRule { rot: None, mesh: Some(batarang_mesh) }),
    (GRENADE, MeshRule { rot: None, mesh: Some(grenade_mesh) }),
];

/// Batman_AnimBP's BlendSpacePlayer nodes the view runs, by the port's names:
/// the glide stack under BlendListByBool(IsGliding), child 0: GlideLoop (the base of ApplyAdditive_1, X <-
/// SpringJoint1_offsetY) and GlideAdd (the additive input of ApplyAdditive, X <- SpringJoint2_offsetY).
const BLEND_SPACES: BlendSpaceTable = BlendSpaceTable {
    nodes: &[("glideLoop", "AnimGraphNode_BlendSpacePlayer_1"), ("glideAdd", "AnimGraphNode_BlendSpacePlayer")],
};

// ---- the pose ----

/// The float curves its AnimBP reads by name (GetCurveValue), in the order the rule indexes them.
const CURVES: CurveTable = CurveTable::new(&["capeActive", "prop1LPropL", "prop1RPropL", "DisableS02Dynamics"]);
const NC: usize = 4;
const CAPE: usize = 0;
const P1L: usize = 1;
const P1R: usize = 2;
const DISABLE_S02: usize = 3;

/// The samurai skin's SpringBone (fml_un_L_samurai_sideLowArmor_1_att), between the slots and the mirror node.
const SAMURAI_SPRING: &[&str] = &["AnimGraphNode_SpringBone"];
/// The samurai skin's Trail and AnimDynamics nodes, after the VB ModifyBones, in the graph's evaluation order (the
/// pose links from ComponentToLocalSpace back: input first).
const SAMURAI_NODES: &[&str] = &[
    "AnimGraphNode_Trail_3",
    "AnimGraphNode_AnimDynamics_11",
    "AnimGraphNode_AnimDynamics_12",
    "AnimGraphNode_AnimDynamics_10",
    "AnimGraphNode_Trail_2",
    "AnimGraphNode_Trail_1",
    "AnimGraphNode_AnimDynamics_9",
    "AnimGraphNode_AnimDynamics_8",
    "AnimGraphNode_AnimDynamics_7",
    "AnimGraphNode_AnimDynamics_6",
    "AnimGraphNode_AnimDynamics_5",
    "AnimGraphNode_Trail",
    "AnimGraphNode_AnimDynamics_4",
    "AnimGraphNode_AnimDynamics_3",
    "AnimGraphNode_AnimDynamics_2",
    "AnimGraphNode_AnimDynamics_1",
    "AnimGraphNode_AnimDynamics",
];
/// Less_FloatFloat's bound on DisableS02Dynamics (@4747 and each node's).
const S02_CURVE_MAX: f64 = 0.1;

/// GlideAdditive: GlideEnter (Batman_Nav_GlideEnterAdd, not looping) -> GlideLoop (Batman_Nav_GlideLoopAdd, looping)
/// (the machine's state and time: the sim's). Both sequences are AAT_LocalSpaceBase over Batman_Nav_GlideLoopC frame 0.
const ENTER: &str = "Batman_Nav_GlideEnterAdd";
const LOOP: &str = "Batman_Nav_GlideLoopAdd";
/// OnFacingChanged's DepthVector: (FacingLeft ? -210 : 210, 0, 0), world; the component's +Y at both facings.
const DEPTH: f64 = 210.0;
/// BaseFighter's CustomTimeDilation while in hitpause.
const HITPAUSE_DILATION: f64 = 0.005;
/// ZERO_ANIMWEIGHT_THRESH.
const ALPHA_MIN: f64 = 0.00001;
const CAPE_SWITCH: f64 = 3.0;
const DETAILED: &str = "CapeDetailedA";
const FULL: &str = "CapeFullA";
/// IdleFadeTimeline's float track (Pawn_BatmanV2's timeline template): [time, value, arrive, leave, mode 1 = cubic].
const FADE_KEYS: [CurveKey; 4] = [
    [-0.00088699, 1.0, -12.986792, -12.986798, 1.0],
    [0.026018545, 0.6836245, -10.022699, -10.022698, 1.0],
    [0.08374033, 0.2417438, -4.71239, -4.712365, 1.0],
    [0.24990131, 0.01, -0.09243907, -0.09244908, 1.0],
];
const FADE_LEN: f64 = 0.25;
/// The actor's tick (the timeline component's DeltaTime before the dilation).
const TICK: f64 = 1.0 / 60.0;

/// Batman_AnimBP's CDO values of the four nodes the view runs (Default__Batman_AnimBP_C: AnimGraphNode_SpringBone_1 /
/// _2, AnimGraphNode_LegIK / _1), as the data has them, in the graph's order, input first; the springs' alpha is the
/// constant 1, the legs' IsGlidingAlpha. SpringBone_4 / _3 (SpringJoint1 / 2) are the sim's (the header).
fn node_props() -> Vec<(&'static str, Props)> {
    let alpha = |mut v: serde_json::Value| {
        let a = serde_json::json!({
            "AlphaInputType": "EAnimAlphaInputType::Float", "Alpha": 1, "AlphaScaleBias": { "Scale": 1, "Bias": 0 },
            "AlphaCurveName": "None",
            "AlphaScaleBiasClamp": { "bMapRange": false, "bClampResult": false, "bInterpResult": false, "Scale": 1,
                "Bias": 0 },
            "LODThreshold": -1
        });
        for (k, x) in a.as_object().expect("an object") {
            v[k] = x.clone();
        }
        serde_json::from_value::<Props>(v).expect("Batman's anim node properties")
    };
    let spring = |bone: &str, k: f64, d: f64, thresh: f64, max: f64, rot_x: bool| {
        alpha(serde_json::json!({
            "SpringBone": { "BoneName": bone }, "MaxDisplacement": max, "SpringStiffness": k, "SpringDamping": d,
            "ErrorResetThresh": thresh, "bLimitDisplacement": true, "bTranslateX": false, "bTranslateY": true,
            "bTranslateZ": false, "bRotateX": rot_x, "bRotateY": false, "bRotateZ": false
        }))
    };
    let leg_ik = |side: &str| {
        alpha(serde_json::json!({
            "ReachPrecision": 0.01, "MaxIterations": 12,
            "LegsDefinition": [{
                "IKFootBone": { "BoneName": format!("fml_un_{side}_foot_marker") },
                "FKFootBone": { "BoneName": format!("fml_un_{side}_foot") },
                "NumBonesInLimb": 2, "MinRotationAngle": 15, "FootBoneForwardAxis": "EAxis::Y",
                "HingeRotationAxis": "EAxis::None", "bEnableRotationLimit": false, "bEnableKneeTwistCorrection": true
            }]
        }))
    };
    vec![
        ("AnimNode_SpringBone", spring("fml_un_L_foot_marker", 275.0, 16.0, 1000.0, 50000.0, true)), // SpringBone_2
        ("AnimNode_SpringBone", spring("fml_un_R_foot_marker", 250.0, 18.0, 1000.0, 50000.0, true)), // SpringBone_1
        ("AnimNode_LegIK", leg_ik("R")),                                                             // LegIK
        ("AnimNode_LegIK", leg_ik("L")),                                                             // LegIK_1
    ]
}

/// `batmanPose`'s state on one view: made once a view; nothing in the tick allocates.
#[derive(Debug)]
struct BatmanPose {
    root: Option<usize>,
    prop1: Option<usize>,
    prop2: Option<usize>,
    prop_r: Option<usize>,
    prop_l: Option<usize>,
    detailed: Option<usize>,
    full: Option<usize>,
    /// The cape parts' tags as the sim's sub_hidden holds them (lowercase), by part.
    cape_tags: Vec<(usize, String)>,
    chain: AnimNodeChain,
    alphas: [f64; 4],
    comp: UeXf,
    lp: BlendSpacePlayer,
    add: BlendSpacePlayer,
    enter: Option<usize>,
    loop_clip: Option<usize>,
    glide_ok: bool,
    /// The bones the GlideAdd samples have a track for: the others' delta is the identity.
    add_bones: Vec<usize>,
    g: Pose,
    s: Pose,
    b: Pose,
    ta: Pose,
    xf: BoneXfs,
    cs: CsPose,
    /// The samurai skin's nodes (none run on another skin's mesh: their bones are not in it).
    spring: SkinNodes,
    samurai: SkinNodes,
    /// Their alphas: the SpringBone's (@8937-9070) and the others' (@4573-4864 and each node's).
    spring_alpha: f64,
    samurai_alpha: f64,
    respawning: RespawnFlag,
    // ---- the AnimBP's state ----
    /// The update of this tick is still to run (at the first rule call: its dt).
    pending: bool,
    /// A fighter's view this tick (set ran, not slaved).
    live: bool,
    /// PoseOf::slaved: no AnimBP of its own (no update, no glide, no ModifyBone_4 or chain; the prop alphas are the
    /// curves of the evaluation before, as in a preview).
    slaved: bool,
    gliding_alpha: f64,
    glide_w: f64,
    /// The source's nodes update this tick (the Slot gate).
    updated: bool,
    sm_state: f64,
    sm_time: f64,
    dilation: f64,
    mix_w: f64,
    /// Constraint_5 (prop1 -> R) and Constraint_2 (prop1 -> L) alphas; Constraint_1 is 1.
    a5: f64,
    a2: f64,
    /// The curves: the sampled clips' (tgt; src: the source's under a montage blending out), the evaluation's (cur)
    /// and the pose the running fade started from (from).
    tgt: [f64; NC],
    src: [f64; NC],
    cur: [f64; NC],
    from: [f64; NC],
    tmp: [f64; NC],
    fades: u32,
    first_eval: bool,
    /// The capes: the AnimBP's last write of each part (None none) and the sim's last visibility.
    prev_cape: f64,
    cape_own: Vec<Option<bool>>,
    sim_vis: Vec<Option<bool>>,
    // ---- the idle fade timeline ----
    fade_pos: f64,
    fade_dir: f64,
    fade_on: bool,
    /// The timeline wrote this tick: the materials take it (PoseHook::scalars).
    fade_write: bool,
}

fn batman_pose(view: &mut CharView, clips: &Clips, of: &PoseOf<'_>) -> Box<dyn PoseHook> {
    let n = view.rest.n;
    let bone = |b: &str| {
        let i = view.rig.index(b);
        if i.is_none() {
            eprintln!("warning: Batman pose: bone missing: {b}");
        }
        i
    };
    let root = bone("Root");
    let (prop1, prop2) = (bone("fml_un_C_prop1"), bone("fml_un_C_prop2"));
    let (prop_r, prop_l) = (bone("fml_un_R_PropOffset"), bone("fml_un_L_PropOffset"));
    let detailed = of.part_keys.iter().position(|k| k == DETAILED);
    let full = of.part_keys.iter().position(|k| k == FULL);
    if detailed.is_none() || full.is_none() {
        let miss = if detailed.is_none() { DETAILED } else { FULL };
        eprintln!("warning: Batman pose: the skin has no sub-mesh {miss}");
    }
    let cape_tags =
        [detailed, full].into_iter().flatten().map(|k| (k, of.part_keys[k].to_lowercase())).collect::<Vec<_>>();
    let props = node_props();
    let defs: Vec<(&str, &Props)> = props.iter().map(|(t, p)| (*t, p)).collect();
    let skel = AnimNodeSkel { names: &view.rig.names, parents: &view.parents, rest: None };
    let chain = AnimNodeChain::new(&defs, &skel);
    let lp = BlendSpacePlayer::new(of.blend.player("glideLoop"), clips);
    let add = BlendSpacePlayer::new(of.blend.player("glideAdd"), clips);
    let (enter, loop_clip) = (clips.get(ENTER), clips.get(LOOP));
    let glide_ok = lp.ok && add.ok && enter.is_some() && loop_clip.is_some();
    if !glide_ok {
        eprintln!("warning: Batman pose: a glide clip is not loaded: the glide stack is not drawn");
    }
    let add_bones = add.tracked_bones(&view.rig);
    let parts = of.part_keys.len();
    let mut p = BatmanPose {
        root,
        prop1,
        prop2,
        prop_r,
        prop_l,
        detailed,
        full,
        cape_tags,
        cs: CsPose::new(&view.rig.names, &view.parents),
        spring: SkinNodes::new(SAMURAI_SPRING, &skel),
        samurai: SkinNodes::new(SAMURAI_NODES, &skel),
        spring_alpha: 0.0,
        samurai_alpha: 0.0,
        respawning: RespawnFlag::default(),
        chain,
        alphas: [0.0; 4],
        comp: UeXf::default(),
        lp,
        add,
        enter,
        loop_clip,
        glide_ok,
        add_bones,
        g: Pose::new(n),
        s: Pose::new(n),
        b: Pose::new(n),
        ta: Pose::new(n),
        xf: BoneXfs::default(),
        pending: false,
        live: false,
        slaved: of.slaved,
        gliding_alpha: 0.0,
        glide_w: 0.0,
        updated: false,
        sm_state: 0.0,
        sm_time: 0.0,
        dilation: 1.0,
        mix_w: 1.0,
        a5: 0.0,
        a2: 0.0,
        tgt: [0.0; NC],
        src: [0.0; NC],
        cur: [0.0; NC],
        from: [0.0; NC],
        tmp: [0.0; NC],
        fades: 0,
        first_eval: true,
        prev_cape: 4.0,
        cape_own: vec![None; parts],
        sim_vis: vec![None; parts],
        fade_pos: 0.0,
        fade_dir: 0.0,
        fade_on: false,
        fade_write: false,
    };
    p.init();
    Box::new(p)
}

impl BatmanPose {
    fn init(&mut self) {
        self.chain.initialize();
        self.alphas = [1.0, 1.0, 0.0, 0.0];
        self.chain.update(0.0, &self.alphas, 1.0); // InitAnim's DeltaTime-0 tick
        self.lp.init();
        self.add.init();
        self.gliding_alpha = 0.0;
        self.glide_w = 0.0;
        self.sm_state = 0.0;
        self.sm_time = 0.0;
        self.prev_cape = 4.0;
        self.cape_own.fill(None);
        self.sim_vis.fill(None);
        self.first_eval = true;
        self.fade_pos = 0.0;
        self.fade_dir = 0.0;
        self.fade_on = false;
        self.spring.clear();
        self.samurai.clear();
        self.respawning.reset();
    }

    /// The update's capes (@10902-11229) on the curves of the evaluation before: each write into `cape_own`.
    fn capes(&mut self) {
        let c = self.cur[CAPE];
        let mut own = |k: Option<usize>, on: bool| {
            if let Some(k) = k {
                self.cape_own[k] = Some(on);
            }
        };
        if c < CAPE_SWITCH && self.prev_cape >= CAPE_SWITCH {
            own(self.detailed, true);
            own(self.full, false);
        } else if c >= CAPE_SWITCH && self.prev_cape < CAPE_SWITCH {
            own(self.detailed, false);
            own(self.full, true);
        }
        self.prev_cape = c;
    }

    /// The update of the tick: BlueprintUpdateAnimation on the evaluation before, then the graph's update (the glide
    /// stack's part: the sim's, set).
    fn update(&mut self, dt0: f64) {
        self.pending = false;
        let dt = dt0 * self.dilation;
        self.capes();
        self.a5 = self.cur[P1R];
        self.a2 = self.cur[P1L];
        // the samurai nodes sit above the Slot nodes: no gate but their alphas
        let ok = f64::from(u8::from(self.cur[DISABLE_S02] < S02_CURVE_MAX));
        self.samurai_alpha = ok * self.spring_alpha;
        let (a, b) = (self.spring_alpha, self.samurai_alpha);
        self.spring.update(dt, |_| a);
        self.samurai.update(dt, |_| b);
        // The graph's update: the nodes under the Slot nodes, so a montage that holds the source freezes them.
        if !self.updated {
            return;
        }
        self.alphas[2] = self.gliding_alpha;
        self.alphas[3] = self.gliding_alpha;
        self.chain.update(dt, &self.alphas, 1.0);
    }

    /// The source pose (no montage): the BlendListByBool's children, then ModifyBone_4 and the spring / leg IK nodes.
    fn source(&mut self, cx: &mut PoseCx<'_>, clip: usize, time: f64, out: &mut Pose) {
        let gw = if self.glide_ok { self.glide_w } else { 0.0 };
        if gw > ALPHA_MIN {
            let (enter, lp) = (self.enter.expect("glide_ok"), self.loop_clip.expect("glide_ok"));
            self.lp.pose(cx.clips, cx.rig, cx.rest, &mut self.g, &mut self.s, cx.root_index);
            let c = if self.sm_state == 0.0 { enter } else { lp };
            apply_additive(cx.clips, c, cx.rig, cx.rest, self.sm_time, &mut self.g, &mut self.ta);
            self.add.additive_pose(cx.clips, cx.rig, cx.rest, &mut self.b, &mut self.s);
            accumulate_held(&self.b, &self.add_bones, 1.0, true, &mut self.g);
            if gw >= 1.0 - ALPHA_MIN {
                out.copy(&self.g);
            } else {
                cx.sample_layer(clip, time, &mut self.s); // child 1: the linked layer, its slot mix and lean in it
                weight_pose(&self.g, gw, out);
                add_weighted_pose(&self.s, 1.0 - gw, out);
                normalize_rotations(out);
            }
            // the glide child's curves: the GlideLoop rows' clips
            self.src = [0.0; NC];
            for k in 0..self.lp.count {
                let Some(c) = self.lp.clips[self.lp.row_sample[k] as usize] else { continue };
                let len = cx.clips.clips[c].anim.length;
                CURVES.fill_of(cx.clips, Some(c), self.lp.t.clamp(0.0, 1.0) * len, &mut self.tmp);
                let w = f64::from(self.lp.row_w[k]).clamp(0.0, 1.0) * gw;
                for j in 0..NC {
                    self.src[j] += self.tmp[j] * w;
                }
            }
            if gw < 1.0 - ALPHA_MIN {
                CURVES.fill_of(cx.clips, Some(clip), time, &mut self.tmp);
                for j in 0..NC {
                    self.src[j] += self.tmp[j] * (1.0 - gw);
                }
            }
        } else {
            cx.sample_layer(clip, time, out);
            CURVES.fill_of(cx.clips, Some(clip), time, &mut self.src);
        }
        if !self.live {
            return;
        }
        // ModifyBone_4: the world translation (210, 0, 0) is the component's +Y (the view's z) over its scale
        if self.gliding_alpha > ALPHA_MIN
            && let Some(r) = self.root
        {
            let i = r * 3 + 2;
            out.t[i] = (f64::from(out.t[i]) + DEPTH / 100.0 / self.comp.s[0] * self.gliding_alpha.min(1.0)) as f32;
        }
        self.chain.evaluate(out, &self.comp);
    }
}

/// A Constraint node: the bone's component-space translation and rotation (through the rotator) become the target's,
/// its scale its own, blended in by the alpha (as Arya's; crate::anim_nodes::CsPose).
fn constrain(
    cs: &mut crate::anim_nodes::CsEval<'_>,
    xf: &mut BoneXfs,
    b: Option<usize>,
    to: Option<usize>,
    alpha: f64,
) {
    let (Some(b), Some(to)) = (b, to) else { return };
    if !(alpha > ALPHA_MIN) {
        return;
    }
    let t = cs.get(to);
    let own = cs.get(b);
    let q = t.q;
    let l = (q[0] * q[0] + q[1] * q[1] + q[2] * q[2] + q[3] * q[3]).sqrt();
    let l = if l == 0.0 || l.is_nan() { 1.0 } else { l };
    let rq = through_rotator([q[0] / l, q[1] / l, q[2] / l, q[3] / l]);
    xf.clear();
    xf.push(b, UeXf { t: t.t, q: rq, s: own.s });
    cs.local_blend(xf, alpha.min(1.0));
}

impl PoseRules for BatmanPose {
    /// The linked layer (the Fall blend space's mix, the lean) under the BlendListByBool and the chain.
    fn takes_layer(&self) -> bool {
        true
    }

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
        if self.pending {
            self.update(dt);
        }
        if montage.is_none() {
            self.source(cx, clip, time, out);
            self.tgt = self.src;
            return true;
        }
        // the montage's sample: its curves, over the source's while it blends out
        CURVES.fill_of(cx.clips, Some(clip), time, &mut self.tgt);
        if again && self.mix_w < 1.0 {
            let m = self.mix_w.max(0.0);
            for k in 0..NC {
                self.tgt[k] = self.src[k] + (self.tgt[k] - self.src[k]) * m;
            }
        }
        false
    }

    fn rule(&mut self, cx: &mut PoseCx<'_>, p: &mut Pose, dt: f64) {
        if self.pending {
            self.update(dt);
        }
        if !self.live {
            self.a5 = self.cur[P1R];
            self.a2 = self.cur[P1L];
        }
        if self.first_eval {
            self.first_eval = false;
            self.cur = self.tgt;
            self.from = self.tgt;
            self.fades = cx.fade_count;
        }
        if self.live {
            self.spring.evaluate(p, &self.comp);
        }
        // Constraint_5, Constraint_2, Constraint_1 (the right-facing set: the header)
        {
            let mut cs = self.cs.begin(p);
            constrain(&mut cs, &mut self.xf, self.prop1, self.prop_r, self.a5);
            constrain(&mut cs, &mut self.xf, self.prop1, self.prop_l, self.a2);
            constrain(&mut cs, &mut self.xf, self.prop2, self.prop_r, 1.0);
        }
        if self.live {
            self.samurai.evaluate(p, &self.comp);
        }
        // this evaluation's curves, for the next update
        if cx.fade_count != self.fades {
            self.fades = cx.fade_count;
            self.from = self.cur;
        }
        let w = cx.fade_weight;
        for k in 0..NC {
            self.cur[k] = if w >= 1.0 { self.tgt[k] } else { self.from[k] + (self.tgt[k] - self.from[k]) * w };
        }
    }
}

impl PoseHook for BatmanPose {
    /// `preview()`: a view with no fighter: facing right, no glide, no fade; the cape swap runs in
    /// preview_step.
    fn preview(&mut self, _view: &mut CharView, _of: &PreviewOf<'_>) {
        self.live = false;
        self.pending = false;
        self.gliding_alpha = 0.0;
        self.glide_w = 0.0;
        self.mix_w = 1.0;
        self.prev_cape = 4.0;
        self.cape_own.fill(None);
    }

    /// The preview's update (as gizmo.rs's): on BP_CharacterPreview the update (@10853) pushes the capes' branch
    /// (@10902) apart from the Fighter test's (@11769, its pop ends only that branch), and the cape writes (@3951-4234,
    /// @5885-6228) test only IsValid(VisibilityManager), the owning actor's (@276-461): the swap on the curves of the
    /// evaluation before (the sample's, the preview's montage), each write of this update put on its part (the AnimBP
    /// is the parts' only writer here). The curves of the frame are the view's sample's (PoseRules::sample).
    fn preview_step(&mut self, view: &mut CharView, _clips: &Clips, _s: &PreviewStep<'_>) {
        self.cape_own.fill(None);
        self.capes();
        for k in 0..self.cape_own.len() {
            if let Some(on) = self.cape_own[k] {
                view.set_part_visible(k, on);
            }
        }
    }

    /// The reset (init): the AnimBP's and the timeline's state anew; the materials keep the fade's last write.
    fn reset(&mut self, _part_scalar: &mut dyn FnMut(usize, &str, f64)) {
        self.init();
        self.fade_write = false;
    }

    /// The AnimBP's update of the tick, before the view's pose.
    fn set(&mut self, w: &World, f: FighterId, view: &mut CharView, _clips: &Clips) {
        self.live = !self.slaved;
        self.pending = self.live;
        let fi = w.fighter(f);
        let b = batman_of(w, f);
        // the glide stack's state: the sim's (glide.rs ran in this tick's anim update)
        match b.filter(|_| self.live) {
            Some(b) => {
                self.gliding_alpha = f64::from(b.glide_alpha);
                self.glide_w = f64::from(b.glide_w);
                self.sm_state = f64::from(b.glide_sm);
                self.sm_time = f64::from(b.glide_sm_time);
                if self.glide_w > ALPHA_MIN && self.glide_ok {
                    // (a step above the input filter's: the input taken as it is)
                    self.lp.select(f64::from(b.glide_loop_x), 0.0, 1.0);
                    self.lp.t = f64::from(b.glide_loop_t);
                    self.add.select(f64::from(b.glide_add_x), 0.0, 1.0);
                    self.add.t = f64::from(b.glide_add_t);
                }
            }
            None => {
                self.gliding_alpha = 0.0;
                self.glide_w = 0.0;
            }
        }
        self.dilation = if fi.hit.hitpause > 0.0 { HITPAUSE_DILATION } else { 1.0 };
        // Is Respawning and Is Hit Stunned: the nodes' alphas, the update's
        let respawning = self.respawning.step(w.flow.tick);
        let on = self.live && !respawning && !in_hitstun(w, f);
        self.spring_alpha = f64::from(u8::from(on));
        if self.live && !self.samurai.is_made() {
            let skel = AnimNodeSkel { names: &view.rig.names, parents: &view.parents, rest: Some(&view.rest) };
            let data = w.char_data(f).fighter.anim_nodes.as_ref();
            let a = self.spring_alpha;
            let ok = f64::from(u8::from(self.cur[DISABLE_S02] < S02_CURVE_MAX));
            self.spring.make(data, &skel, "Batman pose", |_| a);
            self.samurai.make(data, &skel, "Batman pose", |_| a * ok);
        }
        let facing = fi.body.facing_dir;
        self.comp = mesh_xf(
            f64::from(-(fi.body.x + facing * fi.hit.mesh_x)),
            f64::from(fi.body.y) + f64::from(fi.hit.mesh_z) + view.mesh_z * view.size_scale,
            f64::from(facing),
            view.mesh_scale * view.size_scale,
        );
        // the Slot nodes' gate on the source's update (the sim's rule: holds_source)
        let sm = mvs_sim::fighter::shown_montage(w, f).and_then(|dash| {
            let st = if dash { fi.montages.dash_montage.as_ref() } else { fi.montages.montage.as_ref() }?;
            let uid = slot_uid(w, f, if dash { MontageSlot::Dash } else { MontageSlot::Main });
            Some((st, uid.and_then(|u| instance_data(&w.data, w, f, u))))
        });
        self.mix_w = match sm {
            Some((st, Some(m))) => st.shown_weight(m, &|s| eprintln!("warning: {s}")),
            _ => 1.0,
        };
        self.updated = !matches!(sm, Some((st, Some(m))) if m.holds_source == Some(true) && st.full_weight(m));
        // the capes: the AnimBP's write until the sim changes the part's visibility (the later write)
        for (k, tag) in &self.cape_tags {
            let k = *k;
            let vis = !w.names.get(tag).is_some_and(|n| fi.pose.sub_hidden.contains(&n));
            if self.sim_vis[k].is_some_and(|v| v != vis) {
                self.cape_own[k] = None;
            }
            self.sim_vis[k] = Some(vis);
            view.set_part_visible(k, self.cape_own[k].unwrap_or(vis));
        }
        // the idle fade timeline (the actor's tick: before the mesh's)
        let on = b.is_some_and(|b| b.idle_fade);
        if on != self.fade_on {
            self.fade_on = on;
            self.fade_dir = if on { 1.0 } else { -1.0 };
        }
        self.fade_write = false;
        if self.fade_dir != 0.0 {
            self.fade_pos = (self.fade_pos + self.fade_dir * TICK * self.dilation).clamp(0.0, FADE_LEN);
            self.fade_write = true;
            if (self.fade_dir > 0.0 && self.fade_pos >= FADE_LEN) || (self.fade_dir < 0.0 && self.fade_pos <= 0.0) {
                self.fade_dir = 0.0;
            }
        }
    }

    /// The idle fade's write (writeFade): BaseColor_Mult and Roughness_Power on the Mesh and the capes, Team on the
    /// Mesh (the colour index, Comp_Pawn_SkinManager.GetColorIndex @7118).
    fn scalars(&mut self, color_index: u8, put: &mut dyn FnMut(Option<usize>, &str, f64)) {
        if !self.fade_write {
            return;
        }
        let a = rich_curve(&FADE_KEYS, self.fade_pos);
        put(None, "BaseColor_Mult", a);
        put(None, "Roughness_Power", a);
        put(None, "Team", if a >= 0.6 { 0.5 } else { f64::from(color_index) });
        for k in [self.detailed, self.full].into_iter().flatten() {
            put(Some(k), "BaseColor_Mult", a);
            put(Some(k), "Roughness_Power", a);
        }
    }
}

// ---- the classes' meshes ----
//
// GrappleHookProjectileV2_C (its SCS): StaticMesh = SM_BatmanGrappleHook under the Capsule root, scale 3, no relative
// rotation and no material override (the mesh's own); the actor turns with its velocity (ProjectileMovement
// bRotationFollowsVelocity: actor_rot). A non-fighter attach sets the mesh's relative rotation (@5968: MakeRotator(Roll,
// -Pitch, 0) of FindLookAtRotation(the actor, GetGrappleOrigin()), once; the effects' side takes the same for the glow
// under it, mvs-render fx/chars/batman.rs). GrappleHookProjectile_Perk_C: its parent's components, so its parent's
// rule.
// Item_BatarangV2_C: Geo = SM_BatarangV2 at (0, 0, -7), yaw -90, scale 3 under the Capsule root (bAbsoluteScale: the
// actor's scale), bVisible false in the template. MulticastCreateVisuals @9394 hides it, StopBatarang @66 shows it:
// drawn while the sim's trail is off. The actor's rotation is (0, 0, 0) (K2_SetActorRotation at BeginPlay @1775 and
// StopBatarang @385; the ProjectileMovement does not turn it), held: the holder's hold socket (Comp_ItemManager's
// HoldSlotName). BatarangMaterial = Geo.CreateDynamicMaterialInstance(0, BatmanBatarangV1_MESH_MIC) @8866 with Team =
// SelectFloat(0.6, the Instigator's colour value, local) @9163, and the owner's on SetTeamColor @7601.
// Unverified: the Instigator is taken as the owner; the material's Team follows the owner each frame.
// Deviation: the custom depth outline a held item gets from its grabber is not drawn (as Iron Giant's items).
// SmokeGrenade_C (a World actor): the actor at Comp_Spawner CompSpawnActor's transform, yaw FacingLeft ? 90
// : -90 (Comp_Spawner @1385-1479), the Capsule root (bAbsoluteScale: world scale 1) at the sim's place. MineMesh:
// SM_BatmanBomb01 under the Capsule, no relative location or rotation, RelativeScale3D 3, visible, no graph hides it.
// ReceiveBeginPlay's vector Color @1899 names no parameter of C020_Satchel_MAT: not written. K2_DestroyActor @289 takes
// it at once.
// The body turns about the locked X axis (the sim's welded Box angle, counter-clockwise in its plane: the actor's
// world rotation is the spawn's yaw, then that roll about world X); the owner's facing is read when the view first
// sees the actor (the spawn's tick).
// The held bomb: the pawn components below; the particles of all of these: the effects'.

const GRAPPLE: &str = "GrappleHookProjectileV2_C";
const PERK_GRAPPLE: &str = "GrappleHookProjectile_Perk_C";
const BATARANG: &str = "Item_BatarangV2_C";
const GRENADE: &str = "SmokeGrenade_C";
const GRENADE_MESH: &str = "SM_BatmanBomb01";
/// MineMesh RelativeScale3D under the Capsule's world scale 1.
const GRENADE_SCALE: f64 = 3.0;
const IDENT: Rows = [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0];
const ORIGIN_MONTAGES: [&str; 2] = ["Batman_Special_F_Montage", "Batman_Special_Air_F_Montage"];

/// GetGrappleOrigin: the owner's socket (IsReeling ? Prop1 : the owner's current montage is Batman_Special_F_Montage
/// or Batman_Special_Air_F_Montage ? GrappleOrigin : FacingLeft ? R_Hand : L_Hand), as last drawn.
pub(crate) fn grapple_origin(host: &ActorHost<'_>, w: &World, owner: FighterId, reeling: bool) -> Option<V3> {
    let fi = w.fighter(owner);
    let montage = fi.montages.montage.as_ref().map(|m| w.names.str(m.montage.name));
    let socket = if reeling {
        "Prop1"
    } else if montage.is_some_and(|m| ORIGIN_MONTAGES.contains(&m)) {
        "GrappleOrigin"
    } else if fi.body.facing_dir < 0.0 {
        "R_Hand"
    } else {
        "L_Hand"
    };
    let x = host.socket(w, owner, socket, &Xform { loc: [0.0; 3], rot: IDENT, scale: [1.0; 3] })?;
    Some(x.loc)
}

/// The hook's static mesh: the StaticMesh component under the root; its look-at rotation once set.
#[derive(Debug)]
struct HookMesh {
    id: mvs_sim::actors::ProjectileId,
    view: StaticView,
    comp: Comp,
    /// The look-at rotation (a non-fighter attach), and the grapple's Attached seen.
    look: Option<Rows>,
    stuck: bool,
}

fn hook_mesh(host: &mut ActorHost<'_>, w: &World, a: ActorRef, cs: &ActorComponents) -> Option<Box<dyn ActorMesh>> {
    let ActorRef::Projectile(id) = a else { return None };
    let pr = w.actors.projectiles.get(id)?;
    let c = cs.get("StaticMesh")?;
    let ComponentKind::StaticMesh { mesh, .. } = &c.kind else { return None };
    let from = mvs_sim::fighter::common::actor_def_owner(w, pr.owner?, pr.cls);
    // (the mesh's own materials: no SetTeamColor writes them)
    let view = StaticView::new(host, w, from, mesh, "")?;
    let comp = comp_of(&class_def(w, a)?.components, "StaticMesh")?;
    Some(Box::new(HookMesh { id, view, comp, look: None, stuck: false }))
}

impl ActorMesh for HookMesh {
    /// The attach of the grapple (the effects' rule's tick): the look-at rotation from the origin, once.
    // Unverified: the socket is read from the last drawn pose (as the notify VFX), a frame behind the build's.
    fn tick(&mut self, host: &mut ActorHost<'_>, w: &World) {
        let Some(pr) = w.actors.projectiles.get(self.id) else { return };
        let Some(g) = grapple_of(pr) else { return };
        if !g.stuck || self.stuck {
            return;
        }
        self.stuck = true;
        if g.target.is_some() {
            return;
        }
        let Some(owner) = pr.owner else { return };
        let Some(o) = grapple_origin(host, w, owner, g.reeling) else { return };
        // FindLookAtRotation(the actor, the origin): the pitch of (origin - actor); MakeRotator(Roll 0, -Pitch, 0)
        let (dx, dy, dz) = (o[0], o[1] + f64::from(pr.x), o[2] - f64::from(pr.y));
        let pitch = dz.atan2(dx.hypot(dy));
        self.look = Some(rotation_rows(-pitch, 0.0, 0.0));
    }

    fn place(
        &mut self,
        _host: &mut ActorHost<'_>,
        _w: &World,
        at: &ActorPlace,
        alpha: f64,
        shown: bool,
        out: &mut Vec<MeshPose>,
    ) {
        self.view.visible = shown;
        if !shown {
            return;
        }
        let (x, y) = at.at(alpha);
        let own = self.look.unwrap_or(self.comp.rot);
        self.view.set(&place_at(&self.comp, &own, self.comp.rs, &at.rot, at.scale, [0.0, -x, y]));
        self.view.draw(out);
    }
}

/// The batarang's Geo.
#[derive(Debug)]
struct BatarangMesh {
    id: mvs_sim::actors::ItemId,
    view: StaticView,
    comp: Comp,
}

fn batarang_mesh(
    host: &mut ActorHost<'_>,
    w: &World,
    a: ActorRef,
    _cs: &ActorComponents,
) -> Option<Box<dyn ActorMesh>> {
    let ActorRef::Item(id) = a else { return None };
    let it = w.actors.items.get(id)?;
    let comp = comp_of(&class_def(w, a)?.components, "Geo")?;
    let mesh = comp_mesh(class_def(w, a)?.components.get("Geo")?.as_obj()?)?;
    let view = StaticView::new(host, w, it.spawner?, &mesh, "")?;
    Some(Box::new(BatarangMesh { id, view, comp }))
}

/// A static mesh component's StaticMesh.
fn comp_mesh(c: &Props) -> Option<String> {
    c.get("StaticMesh").and_then(mvs_data::Value::as_str).map(str::to_owned)
}

impl ActorMesh for BatarangMesh {
    fn place(
        &mut self,
        host: &mut ActorHost<'_>,
        w: &World,
        at: &ActorPlace,
        alpha: f64,
        shown: bool,
        out: &mut Vec<MeshPose>,
    ) {
        let Some(it) = w.actors.items.get(self.id) else { return };
        let trail = matches!(&it.ext, ItemExt::Batarang(b) if b.trail);
        self.view.visible = shown && !trail;
        if !self.view.visible {
            return;
        }
        if let Some(o) = it.owner {
            self.view.set_scalar("Team", host.colour(w, o).team);
        }
        let s = it.scale;
        let hold = at.holder.and_then(|h| {
            let sock = mvs_sim::fighter::match_side::hold_socket(w, h);
            host.socket(w, h, &sock, &Xform { loc: [0.0; 3], rot: IDENT, scale: [1.0; 3] })
        });
        let x = match hold {
            Some(h) => place_at(&self.comp, &self.comp.rot, self.comp.rs, &h.rot, f64::from(s), h.loc),
            None => {
                let (x, y) = at.at(alpha);
                place_at(&self.comp, &self.comp.rot, self.comp.rs, &IDENT, f64::from(s), [0.0, -x, y])
            }
        };
        self.view.set(&x);
        self.view.draw(out);
    }
}

// ---- the held bomb ----
//
// SCS_Node_17: SM_DebrisBallProjectile (StaticMeshComponent, SM_Bombarang_Bomb, tag SatchelCharge, bVisible false, no
// relative transform), a child of CharacterMesh0 on the socket Prop2 (the bone fml_un_C_prop2); its two aura children
// are the effects' (mvs-render fx/chars/batman.rs). Who shows it: Buff_BatmanOnHitApplyBuff BeginPlay
// EquipObject(SatchelCharge), ReceiveDestroyed UnequipObject (Comp_Spawner @5971 / @6574: SetSubMeshVisibility(tag,
// on, true)); the sim keeps the state (sub_hidden; the tag is in the character's sub mesh list). No graph re-attaches
// the component on a turn (Pawn_BatmanV2 has no K2_AttachToComponent): it rides Prop2 of the mirrored pose, the
// view's mirror partner of the bone with its flip (Rec::component_attach, both facings made here).
// Material: OnMeshActivated (and SpawnInvulnerability, @5916 -> @5388) makes a dynamic instance of
// BombarangMesh_MIC on slot 0 @5388 and sets its Team to SelectFloat(0.6, GetColorIndex, IsLocalOnlinePlayer) @5603.
// Deviation: the smoke's invisibility for a viewer is not ported (as the view's).

const BOMB_COMP: &str = "SM_DebrisBallProjectile_GEN_VARIABLE";

fn bomb_make(r: &mut Rec, w: &World, clips: &Clips, draws: &FighterDraws, meshes: &MeshMap) {
    let cd = w.char_data(r.fighter());
    let c = cd.fighter.components.pawn.get(BOMB_COMP);
    let p = c.and_then(|c| c.p.as_obj());
    let socket = c.and_then(|c| c.socket.as_deref());
    let mesh = p.and_then(|p| p.get("StaticMesh")).and_then(mvs_data::Value::as_str);
    let tag = p
        .and_then(|p| p.get("ComponentTags"))
        .and_then(mvs_data::Value::as_arr)
        .and_then(|t| t.first())
        .and_then(mvs_data::Value::as_str)
        .map(str::to_lowercase);
    let actor = mesh.and_then(|m| draws.actor(m, None));
    let info = actor.as_ref().and_then(|(path, _)| meshes.get(path)).cloned();
    let (Some(p), Some(socket), Some(tag), Some((_, keys)), Some(info)) = (p, socket, tag, actor, info) else {
        eprintln!(
            "warning: Batman: the held bomb is not drawn (its template, socket, mesh or materials are not loaded)"
        );
        return;
    };
    if p.contains_key("RelativeLocation") || p.contains_key("RelativeRotation") || p.contains_key("RelativeScale3D") {
        eprintln!("warning: Batman: the held bomb has a relative transform (not read)");
    }
    if !cd.def.sub_meshes.contains(&tag) {
        eprintln!("warning: Batman: the sim's mesh list has no {tag}: the bomb stays hidden");
    }
    let at = [false, true].map(|left| r.component_attach(w, socket, [0.0; 3], [0.0, 0.0, 0.0, 1.0], [1.0; 3], left));
    let team = r.colour.team;
    let mats = keys
        .iter()
        .map(|m| {
            m.as_deref().map(|k| {
                let mut m = material(k);
                set_scalar(&mut m, "Team", team);
                m
            })
        })
        .collect();
    r.add_component_mesh(clips, &tag, at, info, mats, false);
}

/// `placeHeldBomb`: once a presentation tick, shown while the fighter lives and the sim shows the tag.
fn bomb_place(r: &mut Rec, w: &World) {
    let tag = "satchelcharge";
    let on = w.fighter(r.fighter()).life.dead == 0.0 && r.sub_mesh_shown(w, tag);
    r.set_component_mesh(tag, on);
}

/// The smoke grenade's MineMesh, between the last two ticks' places (the view's own: a World actor has no previous
/// place in the sim).
#[derive(Debug)]
struct GrenadeMesh {
    id: mvs_sim::actors::ActorId,
    view: StaticView,
    rot: Rows,
    /// x, y and the body's angle (radians) of the last two ticks.
    prev: (f64, f64, f64),
    cur: (f64, f64, f64),
}

/// The grenade's body angle (the sim's SmokeGrenade.angle).
fn grenade_angle(w: &World, id: mvs_sim::actors::ActorId) -> f64 {
    match w.actors.actors.get(id).map(|a| &a.state) {
        Some(WorldActorState::SmokeGrenade(g)) => f64::from(g.angle),
        _ => 0.0,
    }
}

/// The roll about UE X of a turn by `a` counter-clockwise in the sim's plane (x = -UE Y, y = UE Z), as rows.
fn plane_roll(a: f64) -> Rows {
    let (s, c) = a.sin_cos();
    [1.0, 0.0, 0.0, 0.0, c, -s, 0.0, s, c]
}

fn grenade_mesh(host: &mut ActorHost<'_>, w: &World, a: ActorRef, _cs: &ActorComponents) -> Option<Box<dyn ActorMesh>> {
    let ActorRef::Actor(id) = a else { return None };
    let act = w.actors.actors.get(id)?;
    let view = StaticView::new(host, w, act.owner, GRENADE_MESH, "")?;
    let h = std::f64::consts::FRAC_PI_2;
    let yaw = if w.fighter(act.owner).body.facing_dir < 0.0 { h } else { -h };
    let (x, y, _) = mvs_sim::actors::world_actor::actor_place(w, a)?;
    let at = (f64::from(x), f64::from(y), grenade_angle(w, id));
    Some(Box::new(GrenadeMesh { id, view, rot: rotation_rows(0.0, yaw, 0.0), prev: at, cur: at }))
}

impl ActorMesh for GrenadeMesh {
    fn tick(&mut self, _host: &mut ActorHost<'_>, w: &World) {
        let Some((x, y, _)) = mvs_sim::actors::world_actor::actor_place(w, ActorRef::Actor(self.id)) else { return };
        self.prev = self.cur;
        self.cur = (f64::from(x), f64::from(y), grenade_angle(w, self.id));
    }

    fn place(
        &mut self,
        _host: &mut ActorHost<'_>,
        w: &World,
        _at: &ActorPlace,
        alpha: f64,
        shown: bool,
        out: &mut Vec<MeshPose>,
    ) {
        self.view.visible = shown;
        if !shown {
            return;
        }
        let x = self.prev.0 + (self.cur.0 - self.prev.0) * alpha;
        let y = self.prev.1 + (self.cur.1 - self.prev.1) * alpha;
        let rot = compose_rows(&self.rot, &plane_roll(self.prev.2 + (self.cur.2 - self.prev.2) * alpha));
        // the root's world scale (SetActorScale3D: the sim's size scale)
        let k = w.actors.actors.get(self.id).and_then(|a| a.size_scale).unwrap_or(1.0);
        self.view.set(&Xform { loc: [0.0, -x, y], rot, scale: [GRENADE_SCALE * f64::from(k); 3] });
        self.view.draw(out);
    }
}
