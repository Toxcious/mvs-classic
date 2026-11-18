//! Superman's render module: the entry, the AnimBP's pose hook and the classes' mesh
//! rules.
//!
//! The pose beside the shared one (Superman_AnimBP: 46 anim nodes; the update is BlueprintUpdateAnimation).
//! The graph from the slots to the root (LinkID -> ChildProperties[LinkID + 1]):
//!   Navigation (the linked layer BaseMovementMachine of Base_AnimBP_Superman) -> Slot JumpSlot -> Slot DefaultSlot
//!   -> BlendListByBool (HoldingAlly, false) -> Slot OverrideSlot: the shared pose (state machine, montage).
//!   ApplyAdditive: Base = that; Additive = BlendSpacePlayer SupermanFlightAdditive_BlendSpace (1D, X <- FlyLean,
//!     -35..35, samples Superman_Flying_D / _N / _U_Additive at -35 / 0 / 35 on Superman_Flying_Null);
//!     Alpha <- AngleToVelocityAlpha (Float, scale 1, bias 0).
//!   LocalToComponent, then on fml_un_COM, rotation alone, BMM_Replace in BCS_ComponentSpace:
//!   ModifyBone_6: Rotation = MakeRotator(Roll 0, Pitch = fwd ? DirectionSoft + 90 : DirectionSoft - 90, Yaw 0),
//!     fwd = (FacingLeft AND VelocityY >= 0) OR (NOT FacingLeft AND 0 >= VelocityY) (the actor's velocity, the
//!     build's Y: the port's -vx, so fwd is "not moving backward"); Alpha <- AngleToVelocityAlpha (@5549-6102).
//!   ModifyBone_4: Rotation = MakeRotator(0, FindLookAtRotation(AimDashFromLocation, ActorLocation).Pitch, 0);
//!     Alpha <- AimOn (@5060-5342).
//!   ModifyBone_5 / _1 / _2 / _3 / ModifyBone, ComponentToLocal, the Talk layer, the mirror node, the AnimDynamics /
//!     Trail chain: below.
//! The four AnimBP variables are the sim's (SupermanState: angle_to_velocity_alpha, direction_soft, fly_lean written by
//! AnimNotify_SupermanFlight and the AnimBP's update, mvs-sim chars/superman/flight.rs; aim_on, aim_dash_from_x / y by
//! the aim punch): this module reads them and writes nothing.
//!
//! The facing. The mirror node (WAnimationMirrorNode, Superman_MirrorTableV2, bEnabled <- FacingLeft) is after all of
//! these, and the port draws a left facing as the reflection of the right-facing view (CharView::place).
//! Unverified: the nodes are applied on the unmirrored pose at both facings, with the inputs the graph gives them (fwd
//! reads the facing).
//!
//! A rotator (0, P, 0) as a quaternion is the build's (0, -sin P/2, 0, cos P/2), here (0, 0, sin P/2, cos P/2) (the
//! hit rotation on the same bone): forward toward up.
//!
//! Native rules and what is left out:
//!   - FAnimNode_ModifyBone at an alpha between 0 and 1 (LocalBlendCSBoneTransforms 142485bc0): blended in the bone's
//!     local space, the rotation on the shorter arc and normalised; here the same.
//!   - the nodes sit under the mirror node and above the slots, so they turn the crossfaded pose: here too (the rule
//!     runs after the view's crossfade). The hit rotation of Base_AnimBP is inside the linked layer, before.
//!   - ApplyAdditive updates its additive input only while its alpha is above 1e-5 and never reinitialises it (Update
//!     142493470): the player's time and filter stand still at alpha 0 and resume.
//!   - ModifyBone_1 / _2 (the neck and head offsets and the head's scale at HeadScale.X > 1, the big head modifier):
//!     the shared head-scale step.
//!   - not ported: ModifyBone_5 (fml_un_C_prop1 to CarryLocation, alpha HoldingAlly: nothing sets HoldingAlly, the
//!     ally carry is dormant) and the BlendListByBool's HoldingAllyBlen slot with it; ModifyBone / _3 (the VB bones,
//!     not drawn); the LayeredBoneBlend of Superman_Talk from fml_un_C_face_att at TalkAlpha (the lip flap of a voice
//!     line: no character has it here).
//!   - the AnimDynamics and Trail nodes after the mirror (SKIN_NODES: hat1 / hat2, beard1 / 2, bow1 / 2, clark_L_tie1 /
//!     2, clark_R_tie1 / 2 / 3, the bones of the skins that have them), made from the fighter's data at the first set
//!     (crate::anim_nodes::SkinNodes); each alpha float(!Is Respawning && !Is Hit Stunned) (@3102-3235 and each
//!     node's). They run on the unmirrored pose with the component's transform at the actor's facing (mesh_xf), as
//!     Reindog's (chars/creature/skin_controls.rs, its Deviation). The update's DeltaTime carries the mesh's
//!     CustomTimeDilation (0.005 in hitpause). Not run in a preview (no fighter data).
//!     Deviation: the head-scale step (ModifyBone_2 / _1, before the mirror) is the view's after the rule, so the
//!     nodes read the pose before the big head's scale.

use mvs_sim::World;
use mvs_sim::chars::superman::state::superman_of;
use mvs_sim::fighter::{FighterId, in_hitstun};

use super::{CharRender, PoseHook, PoseOf, PreviewOf, PreviewStep};
use crate::actors::MeshRule;
use crate::anim::{Clips, HeadScaleNode, Pose, accumulate_held};
use crate::anim_nodes::{AnimNodeSkel, RespawnFlag, SkinNodes, UeXf, mesh_xf};
use crate::blend::{BlendSpacePlayer, BlendSpaceTable};
use crate::view::{CharView, PoseCx, PoseRules};

pub(crate) const RENDER: CharRender = CharRender {
    head_scale: &HEAD,
    pose: Some(superman_pose),
    blend_spaces: Some(&BLEND_SPACES),
    ..CharRender::GENERIC
};

/// Superman_AnimBP's ModifyBone_2 (head: scale Additive = HeadScale @6580, translation Additive (0, 0, 10), bone
/// space) and ModifyBone_1 (neck: translation Additive (10, 0, 0), bone space, scale Ignore), each Alpha = HeadScale.X
/// > 1 (@6309-6616; Pawn_Superman @2060).
const HEAD: [HeadScaleNode; 2] = [
    HeadScaleNode {
        node: "ModifyBone_2",
        bone: "fml_un_C_head",
        factor: 1.0,
        gated: true,
        moves: [0.0, 0.0, 10.0],
        scale: true,
    },
    HeadScaleNode {
        node: "ModifyBone_1",
        bone: "fml_un_C_neck_a",
        factor: 1.0,
        gated: true,
        moves: [10.0, 0.0, 0.0],
        scale: false,
    },
];

/// The classes' own mesh drawing, by class name.
pub(crate) const RULES: &[(&str, MeshRule)] = &[];

/// Superman_AnimBP's BlendSpacePlayer nodes the pose runs.
const BLEND_SPACES: BlendSpaceTable = BlendSpaceTable { nodes: &[("flightLean", "AnimGraphNode_BlendSpacePlayer")] };

const COM: &str = "fml_un_COM";
/// The AnimDynamics and Trail nodes after the mirror node, in the graph's evaluation order (the pose links from
/// ComponentToLocalSpace back: input first).
const SKIN_NODES: &[&str] = &[
    "AnimGraphNode_AnimDynamics_10",
    "AnimGraphNode_AnimDynamics_9",
    "AnimGraphNode_AnimDynamics_8",
    "AnimGraphNode_AnimDynamics_6",
    "AnimGraphNode_AnimDynamics_7",
    "AnimGraphNode_Trail",
    "AnimGraphNode_AnimDynamics_5",
    "AnimGraphNode_AnimDynamics_4",
    "AnimGraphNode_Trail_1",
    "AnimGraphNode_AnimDynamics_2",
    "AnimGraphNode_AnimDynamics_3",
    "AnimGraphNode_Trail_2",
    "AnimGraphNode_AnimDynamics_1",
    "AnimGraphNode_AnimDynamics",
];
/// ZERO_ANIMWEIGHT_THRESH.
const ALPHA_MIN: f64 = 0.00001;

/// `supermanPose`'s state on one view: the bone index, the player and the scratch poses, made once a view.
#[derive(Debug)]
struct SupermanPose {
    com: Option<usize>,
    lean: BlendSpacePlayer,
    /// The bones any of the additive clips has a track for: the others' delta is the identity.
    lean_bones: Vec<usize>,
    b: Pose,
    m: Pose,
    fly_alpha: f64,
    fly_lean: f64,
    fly_pitch: f64,
    aim_on: bool,
    aim_pitch: f64,
    lean_init: bool,
    /// The rings of C003_S02 (ShowRing): the view's parts LeftRing and RightRing.
    rings: Option<[usize; 2]>,
    /// SKIN_NODES on the view's mesh, their alpha, the component's transform, the update's dilation; whether a set
    /// ran since the last pose (a fighter's view).
    nodes: SkinNodes,
    node_alpha: f64,
    comp: UeXf,
    dilation: f64,
    respawning: RespawnFlag,
    live: bool,
}

/// Superman_AnimBP @1284: ShowRing = AppliedSkin == SkinData C003_S02 (Black Lantern), by its slug.
const RING_SKIN: &str = "skin_c003_s02";
/// The rings' sub-mesh keys (SetRingVisibility @7922 / @8344, `## SetRingVisibility`): facing left shows RightRing and
/// hides LeftRing, facing right the reverse.
const RING_PARTS: [&str; 2] = ["LeftRing", "RightRing"];

fn ring_parts(of: &PoseOf<'_>) -> Option<[usize; 2]> {
    if !of.skin.slug.eq_ignore_ascii_case(RING_SKIN) {
        return None;
    }
    let find = |k: &str| of.part_keys.iter().position(|p| p.eq_ignore_ascii_case(k));
    let [l, r] = RING_PARTS;
    match (find(l), find(r)) {
        (Some(l), Some(r)) => Some([l, r]),
        _ => {
            eprintln!("warning: Superman pose: the ring skin has no {} / {} sub-meshes", RING_PARTS[0], RING_PARTS[1]);
            None
        }
    }
}

fn superman_pose(view: &mut CharView, clips: &Clips, of: &PoseOf<'_>) -> Box<dyn PoseHook> {
    let n = view.rest.n;
    let com = view.rig.index(COM);
    if com.is_none() {
        eprintln!("warning: Superman pose: bone missing: {COM}");
    }
    let lean = BlendSpacePlayer::new(of.blend.player("flightLean"), clips);
    if !lean.ok {
        eprintln!("warning: Superman pose: the flight blend space has a clip that is not loaded");
    }
    let lean_bones = lean.tracked_bones(&view.rig);
    let skel = AnimNodeSkel { names: &view.rig.names, parents: &view.parents, rest: None };
    let nodes = SkinNodes::new(SKIN_NODES, &skel);
    Box::new(SupermanPose {
        com,
        lean,
        lean_bones,
        b: Pose::new(n),
        m: Pose::new(n),
        fly_alpha: 0.0,
        fly_lean: 0.0,
        fly_pitch: 0.0,
        aim_on: false,
        aim_pitch: 0.0,
        lean_init: false,
        rings: ring_parts(of),
        nodes,
        node_alpha: 0.0,
        comp: UeXf::default(),
        dilation: 1.0,
        respawning: RespawnFlag::default(),
        live: false,
    })
}

impl SupermanPose {
    /// SetRingVisibility: the ring on the side the actor faces away from is the one shown (facing left: RightRing).
    fn set_rings(&self, view: &mut CharView, left: bool) {
        if let Some([l, r]) = self.rings {
            view.set_part_visible(l, !left);
            view.set_part_visible(r, left);
        }
    }

    /// The bone's component-space rotation toward the rotator (0, pitch, 0) by alpha, translation and scale kept.
    fn turn(cx: &PoseCx<'_>, com: usize, p: &mut Pose, pitch: f64, alpha: f64) {
        let a = cx.above_rot(p, com);
        let h = pitch * std::f64::consts::PI / 360.0;
        let to = crate::math::qmul([-a[0], -a[1], -a[2], a[3]], [0.0, 0.0, h.sin(), h.cos()]);
        // (the scratch quaternion is a three.js one: f64 values, stored float32 below)
        let o = com * 4;
        if alpha >= 1.0 - ALPHA_MIN {
            for (d, v) in p.r[o..o + 4].iter_mut().zip(to) {
                *d = v as f32;
            }
            return;
        }
        let r = p.quat(com);
        let dot = r[0] * to[0] + r[1] * to[1] + r[2] * to[2] + r[3] * to[3];
        let s = if dot < 0.0 { -alpha } else { alpha };
        let v: [f64; 4] = std::array::from_fn(|k| r[k] * (1.0 - alpha) + to[k] * s);
        let l = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2] + v[3] * v[3]).sqrt();
        let l = if l == 0.0 || l.is_nan() { 1.0 } else { l };
        for (d, v) in p.r[o..o + 4].iter_mut().zip(v) {
            *d = (v / l) as f32;
        }
    }
}

impl PoseRules for SupermanPose {
    fn rule(&mut self, cx: &mut PoseCx<'_>, p: &mut Pose, dt: f64) {
        if self.fly_alpha > ALPHA_MIN {
            // ApplyAdditive
            if self.lean.ok {
                // the player is initialised once: at alpha 0 its time and filter stand still and resume (ApplyAdditive
                // Update 142493470)
                if !self.lean_init {
                    self.lean.init();
                    self.lean_init = true;
                }
                if self.lean.select(self.fly_lean, 0.0, dt) {
                    self.lean.advance(cx.clips, dt);
                }
                self.lean.additive_pose(cx.clips, cx.rig, cx.rest, &mut self.b, &mut self.m);
                accumulate_held(&self.b, &self.lean_bones, self.fly_alpha.min(1.0), true, p);
            }
            if let Some(com) = self.com {
                Self::turn(cx, com, p, self.fly_pitch, self.fly_alpha.min(1.0)); // ModifyBone_6
            }
        }
        if self.aim_on
            && let Some(com) = self.com
        {
            Self::turn(cx, com, p, self.aim_pitch, 1.0); // ModifyBone_4
        }
        if self.live {
            let a = self.node_alpha;
            self.nodes.update(dt * self.dilation, |_| a);
            self.nodes.evaluate(p, &self.comp);
        }
    }
}

impl PoseHook for SupermanPose {
    /// Before the view's pose of the tick: the fighter's flight and aim values.
    fn set(&mut self, w: &World, f: FighterId, view: &mut CharView, _clips: &Clips) {
        self.set_rings(view, w.fighter(f).body.facing_dir < 0.0);
        let fi = w.fighter(f);
        let respawning = self.respawning.step(w.flow.tick);
        self.node_alpha = f64::from(u8::from(!respawning && !in_hitstun(w, f)));
        if !self.nodes.is_made() {
            let skel = AnimNodeSkel { names: &view.rig.names, parents: &view.parents, rest: Some(&view.rest) };
            let a = self.node_alpha;
            self.nodes.make(w.char_data(f).fighter.anim_nodes.as_ref(), &skel, "Superman pose", |_| a);
        }
        self.live = self.nodes.any();
        self.dilation = if fi.hit.hitpause > 0.0 { f64::from(mvs_sim::consts::HITPAUSE_DILATION) } else { 1.0 };
        self.comp = mesh_xf(
            f64::from(-mvs_sim::fighter::mesh_wx(w, f)),
            f64::from(mvs_sim::fighter::mesh_wy(w, f)) + view.mesh_z * view.size_scale,
            f64::from(fi.body.facing_dir),
            view.mesh_scale * view.size_scale,
        );
        let Some(s) = superman_of(w, f) else {
            self.fly_alpha = 0.0;
            self.aim_on = false;
            return;
        };
        let b = &w.fighter(f).body;
        self.fly_alpha = f64::from(s.angle_to_velocity_alpha);
        self.fly_lean = f64::from(s.fly_lean);
        // the build's VelocityY is the port's -vx; FacingLeft is facing < 0
        let fwd = if b.facing_dir < 0.0 { -b.vx >= 0.0 } else { 0.0 >= -b.vx };
        self.fly_pitch = if fwd { f64::from(s.direction_soft + 90.0) } else { f64::from(s.direction_soft - 90.0) };
        self.aim_on = s.aim_on;
        // FindLookAtRotation(from, to).Pitch: the pitch of the direction from the dash's origin to the actor
        self.aim_pitch = (f64::from(b.y) - f64::from(s.aim_dash_from_y))
            .atan2((f64::from(b.x) - f64::from(s.aim_dash_from_x)).abs())
            * 180.0
            / std::f64::consts::PI;
    }

    /// A new pawn: the nodes made anew at the next set.
    fn reset(&mut self, _part_scalar: &mut dyn FnMut(usize, &str, f64)) {
        self.nodes.clear();
        self.respawning.reset();
    }

    /// A preview (no fighter): the rings at the actor's FacingLeft of the frame (right until the first step).
    fn preview(&mut self, view: &mut CharView, _of: &PreviewOf<'_>) {
        self.live = false;
        self.set_rings(view, false);
    }

    fn preview_step(&mut self, view: &mut CharView, _clips: &Clips, step: &PreviewStep<'_>) {
        self.set_rings(view, step.facing_left);
    }
}
