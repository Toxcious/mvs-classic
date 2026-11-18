//! Arya's render module: the entry, the AnimBP's pose hook and the classes' mesh rules.
//!
//! The pose beside the shared one, her sword and her sheath (Arya_AnimBP, AryaSword_AnimBP, Arya_Skin00_AnimBP;
//! Pawn_Arya's SCS: Sword, Sheath and BodyMesh are skeletal children of CharacterMesh0, tagged VisMesh and
//! PrimaryMesh).
//!
//! Arya_AnimBP's graph: Navigation (the linked layer BaseMovementMachine) -> JumpSlot -> DefaultSlot -> the mirror
//! node (Arya_MirrorTable, enabled by FacingLeft) -> LocalToComponent -> Constraint_3 -> Constraint_1 -> Constraint_2
//! -> Constraint -> ModifyBone x 5 -> Trail x 7 / AnimDynamics x 6 -> ComponentToLocal. No node type and no state
//! machine the port did not have.
//!
//! The mirror node. Arya_MirrorTable (AXIS_Y) has BoneFlipAxis None on 420 of its 446 entries (fml_un_C_pelvis_att,
//! fml_un_C_arya_face_att and all below them; 24 are AXIS_Y, the two wrist markers AXIS_XYZ): the body keeps its pose
//! and Arya_AnimBP's On Facing Changed scales the VisMesh components (1, -1, 1), as C019's (`turns`, KEPT_ROOTS).
//! Mirrored (Y): Root, fml_un_COM, fml_un_C_Root_vfx, the *_vfx bones, fml_un_C_prop1 with fml_un_C_sword_a..e,
//! fml_un_C_prop2 with fml_un_C_dagger_a, the markers. Her three mesh components copy CharacterMesh0's pose under ONE
//! scale (C019's prop meshes take two), so the prop mirror is not hers (`copied_props`) and the mirrored prop bones
//! are placed here:
//!   - a prop bone a Constraint holds on a kept bone (a hand's propOffset) is that bone, as the VisMesh scale shows
//!     it: the view's own bone, the plain reflection.
//!   - a prop bone on its own track facing left: the node gives it F_Y x T(B) x M_Y in component space (row vectors;
//!     the bone flip rows), and the component's scale reflects that once more: the drawn bone is at
//!     T(B) x F_Y on the turned actor, which under the view's reflection is the bone's component transform with its
//!     Y translation negated and its rotation mirrored across Y. The bones under it (the blade's) keep their local
//!     transforms.
//!
//! CharacterMesh0 itself is not scaled, so a socket on one of these bones reads the view's bone reflected in depth in
//! both cases: the bones under the two prop roots are kept (`kept_by`).
//!
//! The curves. The graph reads three float curves of the playing sequences by name (GetCurveValue), two of them to
//! an effect, the clip's own (crate::curves), by the sequence's name and its time:
//!   prop2RProp > 0.9  @2628-2762: Constraint_3's alpha (bool, blend times 0): fml_un_C_prop2 (the dagger, part of
//!                     the body mesh) to fml_un_R_propOffset, translation and rotation, all axes, weight 1,
//!                     OffsetOption None.
//!   prop1LProp > 0.9  @2925-3059: Constraint_2's alpha: fml_un_C_prop1 (the sword) to fml_un_L_propOffset, the same.
//!   Constraint_1 (prop2 to the left hand) and Constraint (prop1 to the right hand) take the AND of the curve and a
//!   literal false (@3088-3193, @2791-2896): never on.
//!   BlueprintUpdateAnimation @5926-6114: VisibilityManager.SetSubMeshVisibility(Sheath, prop1SwordSheath > 0.9 OR
//!   prop1LProp < 0.9, true). The call finds no mesh: Comp_FighterVisibilityManager.SetSubMeshVisibility @58-121
//!   looks the name up with Comp_Pawn_MeshManager.Get Mesh (Map_Find(Meshes, tag)) and pops on a miss, and Meshes
//!   is keyed by AddMesh @294-657 / @1232 with a component's first tag that is not PrimaryMesh, VisMesh,
//!   RecieveArmor or IgnoreArmor: Pawn_Arya's Sheath component has [AryaSheath, PrimaryMesh, VisMesh], so its key
//!   is AryaSheath and no mesh has the key Sheath (the Disguise's copies are DisguiseSheath). The scabbard is drawn
//!   all the time and prop1SwordSheath is not in the curve file.
//!   Unverified: read from the graphs, not seen in the build.
//! The Constraints are the shared node (crate::anim_nodes::constraint, FAnimNode_Constraint: the rotation through the
//! rotator).
//!
//! The Sword component of the pawn has no master pose in any state (its SCS template sets none,
//! Comp_Pawn_MeshManager.SetSkelMesh @320-404 returns for a VisMesh component of a Pawn, and Pawn_Arya UpdateSwordAnim
//! @13616 clears one, Sword.SetMasterPoseComponent(null, true), each time a skin is applied), so its own AnimBP runs.
//! AryaSword_AnimBP: CopyPoseFromMesh(the attach parent) -> LocalToComponent -> ModifyBone x 5, scale BMM_Additive in
//! component space (an additive scale multiplies the bone's own): fml_un_C_sword_a (1, SwordWidth, 1), sword_b and
//! sword_c and sword_d (SwordThickness, SwordWidth, 1), sword_e (SwordThickness, SwordWidth, SwordPoint). Class
//! defaults SwordWidth 2.5, SwordThickness 2.5, SwordPoint 1, with no writer in the decoded set
//! (AnimNotify_Arya_CharSelectSwordScale @42-211 casts to Arya_AnimBP and writes that class's own three variables,
//! defaults 4 / 4 / 2, which no graph reads). The five bones are siblings under fml_un_C_prop1 and the mesh is
//! weighted to prop1, sword_a and sword_e. Drawn as draw bones of the Sword part alone (CharView::part_bone_scale):
//! the blade is 2.5 times as wide, and sockets on the blade's bones (CharacterMesh0's) are not scaled.
//! Not on a slaved sword (a preview actor, a Disguise's copy: PoseOf::slaved), where AryaSword_AnimBP is not
//! instanced.
//! The Sheath and BodyMesh components (Arya_Skin00_AnimBP): CopyPoseFromMesh alone.
//!
//! The glow (the sim's SwordGlow event: AnimNotify_AryaSwordGlowOn / Off, mvs-sim chars/arya): Pawn_Arya @4739-4966
//! makes a dynamic instance on every material slot of Sword and writes Emissive_Mult 80.0, @3959-4399 writes 0.0
//! (not the material's own value).
//!
//! Deviations and unverified models:
//!   - Unverified: the unconstrained prop rule facing left is derived from the node and the scales (the sword on its
//!     own track sits on the other side of the body than the reflected body's sheath); not seen in the build.
//!   - Unverified: the value of a curve in a crossfade: the stock FBlendedCurve rule with the weight of the view's one
//!     crossfade, and the update reads the curves of the evaluation before it (as LeBron's pose: not
//!     read in the exe). The first evaluation of a view takes its own clip's curves (the port's own: the build's
//!     first update reads none, a tick spent under the spawn).
//!   - a Constraint's target is the pose's bone: the turn placeKept gives a kept top bone facing left when Root or
//!     fml_un_COM leave their rest transforms is not in it.
//!   - ModifyBone / ModifyBone_1 (fml_un_C_head, fml_un_C_arya_face_att: scale from HeadScale, the big head
//!     modifier): the shared head-scale step (`head_scale`).
//!   - not ported: ModifyBone_2 / _3 / _4 (VB L_LowLeg, VB R_LowLeg, VB Spine: virtual bones, which the view does not
//!     hold); the seven Trail nodes (the hair side chains, the Waterdancer skin's bow tie and belt, the ugly sweater's
//!     hat) and the six AnimDynamics (the ugly sweater's hat): no vertex of the default skin's meshes is weighted to
//!     those bones.
//!   - not drawn: Pawn_Arya's idle fade (@3406-3719: its CheckHasBuff names no class) and the per-material fade it
//!     would write.
//!   - the reset (a new pawn on one view: the Lab's reset): PoseHook::reset, the glow left on by the pawn that
//!     went turned off.

use mvs_data::value::Props;
use mvs_sim::World;
use mvs_sim::events::Event;
use mvs_sim::fighter::FighterId;

use mvs_sim::actors::ActorRef;
use mvs_sim::actors::projectile::{Projectile, ProjectileExt};
use mvs_vfx::math::V3;

use super::{CharRender, PoseHook, PoseOf, PreviewOf};
use crate::actor_view::{StaticView, Xform};
use crate::actors::{ActorComponents, ActorHost, ActorMesh, ActorPlace, ComponentKind, MeshRule, Rows};
use crate::anim::{Clips, HeadScaleNode, Pose};
use crate::anim_nodes::{AnimNode, AnimNodeSkel, BoneXfs, ConstraintNode, CsEval, CsPose, UeXf};
use crate::curves::CurveTable;
use crate::frame::MeshPose;
use crate::view::{CharView, PoseCx, PoseRules};

pub(crate) const RENDER: CharRender = CharRender {
    // The big head: Arya_AnimBP's ModifyBone and ModifyBone_1 (scale Additive, bone space, <- HeadScale; Pawn_Arya
    // @11813).
    head_scale: &[
        HeadScaleNode {
            node: "ModifyBone",
            bone: "fml_un_C_head",
            factor: 1.0,
            gated: false,
            moves: [0.0; 3],
            scale: true,
        },
        HeadScaleNode {
            node: "ModifyBone_1",
            bone: "fml_un_C_arya_face_att",
            factor: 1.0,
            gated: false,
            moves: [0.0; 3],
            scale: true,
        },
    ],
    // Arya_MirrorTable keeps the body; Sword, Sheath and BodyMesh copy the pose under the one VisMesh scale.
    turns: true,
    copied_props: true,
    // a socket on a bone of the two props is CharacterMesh0's: the view's bone reflected in depth facing left
    kept_by: &[PROP1, PROP2],
    pose: Some(arya_pose),
    ..CharRender::GENERIC
};

/// The classes' own mesh drawing, by class name.
pub(crate) const RULES: &[(&str, MeshRule)] =
    &[(PIE, MeshRule { rot: None, mesh: Some(pie_view) }), (KNIFE, MeshRule { rot: Some(knife_rot), mesh: None })];

// ---- MarkTeleportProjectile (the knife's rule `rot`) ----

const KNIFE: &str = "MarkTeleportProjectile_C";

/// SetVFXRotation(the cast fighter's facing left, kept in the frame) @6134, from ActivateObject alone:
/// ParticleSystem.K2_SetWorldRotation(MakeRotator(v, IsOnline ? v : 0, 0)), v = left ? -90 : 90 @12913-13097 (Roll v,
/// Pitch v online). The component is gone on a knife that is on a fighter or in the stage, so it shows on a knife still
/// in flight alone (the effects' rule: mvs-render fx/chars/arya.rs).
// Deviation: IsOnline (the owner's BPI_GetLocality) is taken as false: the World holds no locality (a match host
// flag, playingOnline, tells it).
fn knife_rot(_w: &World, pr: &Projectile) -> Option<Rows> {
    let ProjectileExt::Knife(k) = &pr.ext else { return None };
    if !k.teleporting || k.stuck || k.ground {
        return None;
    }
    let online = false;
    let v = if k.cast_left { -std::f64::consts::FRAC_PI_2 } else { std::f64::consts::FRAC_PI_2 };
    Some(mvs_vfx::math::rotation_rows(if online { v } else { 0.0 }, 0.0, v))
}

// ---- AryaKillPie ----
//
// AryaKillPie (an actor of World.actors): ScoobySnackStaticMesh = BugsBunny_Prop_Pie under the Capsule at
// (0, 0, -125), yaw 90, scale (1.5, 0.35, 3) (the class data), the mesh's own material. No bob, spin or blink. Its
// PS_Arya_Pie and its PopText are the effects' (mvs-render fx/chars/arya.rs).
// Unverified: the actor's rotation is taken as zero (Comp_SpawnArea's spawn transform is not read for it).

const PIE: &str = "AryaKillPie_C";
const PIE_MESH: &str = "ScoobySnackStaticMesh";

/// The pie's mesh: its relative place under the Capsule (the root's scale in it).
#[derive(Debug)]
struct PieView {
    view: StaticView,
    rel: V3,
    rot: Rows,
    scale: V3,
}

fn pie_view(host: &mut ActorHost<'_>, w: &World, a: ActorRef, cs: &ActorComponents) -> Option<Box<dyn ActorMesh>> {
    let ActorRef::Actor(id) = a else { return None };
    let owner = w.actors.actors.get(id)?.owner;
    let c = cs.get(PIE_MESH)?;
    let ComponentKind::StaticMesh { mesh, .. } = &c.kind else { return None };
    let from = mvs_sim::fighter::common::actor_def_owner(w, owner, w.actors.actors.get(id)?.cls);
    // (no variant: the entry's own materials, staticView's default)
    let view = StaticView::new(host, w, from, mesh, "")?;
    let root = c.parent.scale;
    Some(Box::new(PieView {
        view,
        rel: std::array::from_fn(|k| c.loc[k] * f64::from(root[k])),
        rot: c.rot,
        scale: std::array::from_fn(|k| c.scale[k] * f64::from(root[k])),
    }))
}

impl ActorMesh for PieView {
    fn place(
        &mut self,
        _host: &mut ActorHost<'_>,
        _w: &World,
        at: &ActorPlace,
        alpha: f64,
        shown: bool,
        out: &mut Vec<MeshPose>,
    ) {
        let (x, y) = at.at(alpha);
        let loc = [self.rel[0], -x + self.rel[1], y + self.rel[2]];
        self.view.set(&Xform { loc, rot: self.rot, scale: self.scale });
        self.view.visible = shown;
        self.view.draw(out);
    }
}

/// The float curves its AnimBP reads by name (GetCurveValue), in the order the rule indexes them.
const CURVES: CurveTable = CurveTable::new(&["prop1LProp", "prop2RProp"]);
const P1L: usize = 0;
const P2R: usize = 1;

/// The sub-mesh's key.
const SWORD: &str = "AryaSword";
const PROP1: &str = "fml_un_C_prop1";
const PROP1_TO: &str = "fml_un_L_propOffset";
const PROP2: &str = "fml_un_C_prop2";
const PROP2_TO: &str = "fml_un_R_propOffset";
/// AryaSword_AnimBP's class defaults, as scales of the bones' own axes (the view's (x, y, z) = UE (X, Z, Y)).
const WIDTH: f64 = 2.5;
const THICKNESS: f64 = 2.5;
const POINT: f64 = 1.0;
const SWORD_SCALES: &[(&str, [f64; 3])] = &[
    ("fml_un_C_sword_a", [1.0, 1.0, WIDTH]),
    ("fml_un_C_sword_b", [THICKNESS, 1.0, WIDTH]),
    ("fml_un_C_sword_c", [THICKNESS, 1.0, WIDTH]),
    ("fml_un_C_sword_d", [THICKNESS, 1.0, WIDTH]),
    ("fml_un_C_sword_e", [THICKNESS, POINT, WIDTH]),
];
const GLOW: &str = "Emissive_Mult";
const GLOW_ON: f64 = 80.0;
const GLOW_OFF: f64 = 0.0;

/// `aryaPose`'s state on one view: the bone indices, the curves and the two Constraint nodes, made once a view.
#[derive(Debug)]
struct AryaPose {
    prop1: Option<usize>,
    prop2: Option<usize>,
    sword_part: Option<usize>,
    /// The curves: the playing sequence's (tgt), the evaluation's (cur: blended as the pose is) and the pose the
    /// running fade started from (from).
    tgt: [f64; 2],
    cur: [f64; 2],
    from: [f64; 2],
    fades: u32,
    first: bool,
    left: bool,
    /// The update's results: the two Constraint alphas.
    hold1: bool,
    hold2: bool,
    cs: CsPose,
    /// Constraint_3 (the dagger) and Constraint_2 (the sword).
    constraint3: ConstraintNode,
    constraint2: ConstraintNode,
    mx: BoneXfs,
    /// The last SwordGlow's state, and whether one came (glowing, glowSet).
    glowing: bool,
    glow_set: bool,
}

/// A Constraint node of Arya_AnimBP's class defaults: BoneToModify `b`, a Translation and a Rotation row on `to`, all
/// axes, weights 1, OffsetOption None; a bool alpha with no blend time.
fn constraint_props(b: &str, to: &str) -> Props {
    let row = |ty: &str| {
        format!(
            r#"{{"TargetBone":{{"BoneName":"{to}"}},"OffsetOption":"EConstraintOffsetOption::None","TransformType":"ETransformConstraintType::{ty}","PerAxis":{{"bX":true,"bY":true,"bZ":true}}}}"#
        )
    };
    let json = format!(
        r#"{{"BoneToModify":{{"BoneName":"{b}"}},"ConstraintSetup":[{},{}],"ConstraintWeights":[1,1],"LODThreshold":-1,"AlphaInputType":"EAnimAlphaInputType::Bool","AlphaBoolBlend":{{"BlendInTime":0,"BlendOutTime":0}}}}"#,
        row("Translation"),
        row("Rotation")
    );
    serde_json::from_str(&json).expect("Arya's constraint properties")
}

fn arya_pose(view: &mut CharView, _clips: &Clips, of: &PoseOf<'_>) -> Box<dyn PoseHook> {
    let bone = |b: &str| {
        let i = view.rig.index(b);
        if i.is_none() {
            eprintln!("warning: Arya pose: bone missing: {b}");
        }
        i
    };
    let (prop1, prop2) = (bone(PROP1), bone(PROP2));
    let sword_part = of.part_keys.iter().position(|k| k == SWORD);
    match sword_part {
        // (not on a slaved sword: no AryaSword_AnimBP)
        Some(k) if !of.slaved => view.part_bone_scale.push((k, SWORD_SCALES)),
        Some(_) => {}
        None => eprintln!("warning: Arya pose: the skin has no sub-mesh {SWORD}"),
    }
    let skel = AnimNodeSkel { names: &view.rig.names, parents: &view.parents, rest: None };
    let constraint3 = ConstraintNode::new(&constraint_props(PROP2, PROP2_TO), &skel);
    let constraint2 = ConstraintNode::new(&constraint_props(PROP1, PROP1_TO), &skel);
    Box::new(AryaPose {
        prop1,
        prop2,
        sword_part,
        tgt: [0.0; 2],
        cur: [0.0; 2],
        from: [0.0; 2],
        fades: 0,
        first: true,
        left: false,
        hold1: false,
        hold2: false,
        cs: CsPose::new(&view.rig.names, &view.parents),
        constraint3,
        constraint2,
        mx: BoneXfs::default(),
        glowing: false,
        glow_set: false,
    })
}

impl AryaPose {
    /// A mirrored prop bone on its own track, facing left: its component transform reflected across the component's
    /// Y, the rotation with it (the quaternion's X and Z negated), its scale its own.
    fn mirror_own(cs: &mut CsEval<'_>, mx: &mut BoneXfs, b: Option<usize>) {
        let Some(b) = b else { return };
        let x = cs.get(b);
        mx.clear();
        mx.push(b, UeXf { t: [x.t[0], -x.t[1], x.t[2]], q: [-x.q[0], x.q[1], -x.q[2], x.q[3]], s: x.s });
        cs.local_blend(mx, 1.0);
    }
}

impl PoseRules for AryaPose {
    /// The clip pose() samples: the playing sequence's curves, after the update on the evaluation before.
    fn sample(
        &mut self,
        cx: &mut PoseCx<'_>,
        clip: usize,
        time: f64,
        _montage: Option<mvs_sim::names::Name>,
        _dt: f64,
        _out: &mut Pose,
        _again: bool,
    ) -> bool {
        CURVES.fill_of(cx.clips, Some(clip), time, &mut self.tgt);
        if self.first {
            self.first = false;
            self.cur = self.tgt;
            self.from = self.tgt;
            self.fades = cx.fade_count;
        }
        // the graph's update, on the curves of the evaluation before
        self.hold1 = self.cur[P1L] > 0.9;
        self.hold2 = self.cur[P2R] > 0.9;
        false // the shared sampling
    }

    fn rule(&mut self, cx: &mut PoseCx<'_>, p: &mut Pose, _dt: f64) {
        let comp = UeXf::default(); // (component space: the node does not read the component's world transform)
        {
            // Constraint_3 (the dagger), Constraint_2 (the sword); the graph's order
            let mut cs = self.cs.begin(p);
            self.constraint3.update(0.0, if self.hold2 { 1.0 } else { 0.0 });
            if self.hold2 {
                self.constraint3.evaluate(&mut cs, &comp);
            } else if self.left {
                Self::mirror_own(&mut cs, &mut self.mx, self.prop2);
            }
            self.constraint2.update(0.0, if self.hold1 { 1.0 } else { 0.0 });
            if self.hold1 {
                self.constraint2.evaluate(&mut cs, &comp);
            } else if self.left {
                Self::mirror_own(&mut cs, &mut self.mx, self.prop1);
            }
        }
        // this evaluation's curves, for the next update
        if cx.fade_count != self.fades {
            self.fades = cx.fade_count;
            self.from = self.cur;
        }
        let w = cx.fade_weight;
        for k in 0..self.cur.len() {
            self.cur[k] = if w >= 1.0 { self.tgt[k] } else { self.from[k] + (self.tgt[k] - self.from[k]) * w };
        }
    }
}

impl PoseHook for AryaPose {
    /// `preview()`: a view with no fighter faces right.
    fn preview(&mut self, _view: &mut CharView, _of: &PreviewOf<'_>) {
        self.left = false;
    }

    /// Before the view's pose of the tick: the fighter's facing.
    fn set(&mut self, w: &World, f: FighterId, _view: &mut CharView, _clips: &Clips) {
        self.left = w.fighter(f).body.facing_dir < 0.0;
    }

    /// AnimNotify_AryaSwordGlowOn / Off (Pawn_Arya @4739 / @3959): Emissive_Mult on every material of the Sword.
    fn event(&mut self, _w: &World, id: i32, e: &Event, part_scalar: &mut dyn FnMut(usize, &str, f64)) {
        if let Event::SwordGlow { fighter, on } = e
            && *fighter == id
        {
            self.glowing = *on;
            self.glow_set = true;
            if let Some(k) = self.sword_part {
                part_scalar(k, GLOW, if *on { GLOW_ON } else { GLOW_OFF });
            }
        }
    }

    /// A new pawn: the AnimBP's state anew, the sword's materials as the last Off left them (a glow left on by the
    /// pawn that went: its montage's Off never ran here).
    fn reset(&mut self, part_scalar: &mut dyn FnMut(usize, &str, f64)) {
        self.first = true;
        if self.glow_set && self.glowing {
            self.glowing = false;
            if let Some(k) = self.sword_part {
                part_scalar(k, GLOW, GLOW_OFF);
            }
        }
    }
}
