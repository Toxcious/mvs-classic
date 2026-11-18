//! Stripe's render module: the entry, the AnimBP's pose hook and the classes' mesh
//! rules (the saws, the dropped bomb and the boombox).
//!
//! The pose hook: C023B_AnimBP's own nodes beside the shared pose, made from the CDO's node properties (the
//! fighter data's anim_nodes); set() is the AnimBP's update of the tick.
//! ToggleSubmeshes (@10890, from BlueprintUpdateAnimation's flow @9221; behind VisibilityManager valid and its
//! InitialVisibilitySet, which the pawn's SetupInitialVisibility has set before the first update): the push_flow
//! stack @10961-10971 runs ToggleSubmeshVisibility(show_Skateboard, Skateboard) @10976, then (show_Gun, Gun) @11500,
//! (show_Sawblade, Sawblade) @11413, then the chainsaw @11063: facing left (show_Chainsaw, ChainsawMI,
//! ChainsawVisibilityMI) @11077 and, while ChainsawVisibility, SetSubMeshVisibility(Chainsaw, false) @11173 and
//! ChainsawVisibility false; facing right (show_Chainsaw, Chainsaw, ChainsawVisibility) @11245 and, while
//! ChainsawVisibilityMI, SetSubMeshVisibility(ChainsawMI, false) @11341 and ChainsawVisibilityMI false.
//! ToggleSubmeshVisibility (@0-310): curve >= 0.5 XOR the state: SetSubMeshVisibility(tag, !state) and the state
//! flips. The five state variables start false (no CDO value). The sequences spell the curve show_sawblade (an FName:
//! no case). The pawn (sim): SetupInitialVisibility hides every sub-mesh but BodyMesh, so the toggles are what shows
//! the props.
//! C023B_SawBlade_EdgeTrace's BeginPlay @923 (the saw and its two children): the owner's
//! SetSubMeshVisibilityMulticast(Sawblade, false) (a write of the manager the sim leaves to the view: set() hides the
//! Sawblade on a saw of the fighter's StripeState.saws it has not seen, the projectile rule at the saw's first
//! sight); SawbladeVisibility stays as the toggle left it.
//! The AnimBP's write holds until the sim changes that mesh's visibility (as Taz's and Velma's).
//!
//! The component-space chain after the mirror node (its order, inner to outer):
//!   - AnimGraphNode_BoneDrivenController (fml_un_COM TranslationY x -2 -> fml_un_C_pelvis_att TranslationY,
//!     AddToInput) and AnimGraphNode_Constraint (fml_un_C_Face_att's translation = fml_un_C_head's), both with the
//!     alpha float(ChainsawVisibilityMI AND disableChainsawOffset < 0.1) (@5518, @5705): facing left alone (the MI
//!     chainsaw). The shared nodes (crate::anim_nodes) from the data's properties.
//!     Unverified: the view draws a left facing as the reflection of the right-facing pose (turns), and the mirror
//!     node (C023B_MirrorTable) flips fml_un_COM's local translation Y, so the driver's value on the port's pose is
//!     the build's negated: the node is made with its Multiplier negated. The Constraint's two bones are kept by the
//!     table: as it is.
//!   - ModifyBone_7 (fml_un_COM, parent space, (0, COM_FallOverride, 0), alpha float(COM_Fall_Override_On)),
//!     ModifyBone_6 (skateboard_att under Root, the same rotation and alpha), ModifyBone_9 (fml_un_COM, the
//!     COM_Rotation angle and its blend alpha), ModifyBone_5 (fml_un_COM, world space, ChainSawRot with
//!     ChainSawOverride): the values the sim's update leaves (StripePawnState ev_*, mvs-sim chars/stripe/pawn.rs) and
//!     the same pitch as its pose_shapes (Root and COM do not turn in any clip, so each Replace is the view's
//!     turn_com, the hit turn's Replace). skateboard_att's Replace in Root's space is its local rotation.
//!   - ModifyBone (fml_un_C_prop1, alpha ChainSawOverride AND WallRiding): WallRiding stays false (BPI_ToggleWallRide's
//!     caller is the edge bind that stays unbound): never on.
//!   - ModifyBone_3 / _4 / _2 / _1: HeadScale (RENDER.head_scale).
//!   - Not ported: the 10 AnimDynamics and the Trail / Trail_1 nodes (combat_headband, grenade and rope bones that the
//!     default skin weights no vertex to; Trail_03 is in no mesh), ModifyBone_8 / _10-_13 (virtual bones the view
//!     does not hold).
//!
//! The slot graph's Slot nodes are the sim's (StripePawnState.slot_mix, mixed into the curves here): set() checks
//! once that the data's graph has them (SLOT_NODES).
//! OverrideKB's BlendListByBool (Base_AnimBP_AltKB_C023B): the sim's anim set, drawn as any locomotion clip.
//! Sub AnimBPs: C023B_Skin00_AnimBP is BodyMesh's CopyPoseFromMesh (the shared slaved sub-mesh); C023B_Chainsaw_AnimBP
//! is only C023B_ChainSawPull_EdgeTrace's, a dormant class.
//! Deviation: the AnimBP's writes are the view's: they do not reach the sim's sub_hidden / always_visible.
//! Unverified: a curve's value in a crossfade and under the slot mix, and the one tick the update lags the evaluation:
//! as Taz's.
//! A preview actor (BP_CharacterPreview) runs the toggles from its first posed frame (PoseHook::preview_step), as
//! Taz's, with no node (they are made at a fighter's first set). The reset (a new pawn on one view: the
//! Lab's reset) is PoseHook::reset.

use std::sync::atomic::{AtomicBool, Ordering};

use mvs_data::Value;
use mvs_data::fighter::SlotGraphNode;
use mvs_sim::World;
use mvs_sim::actors::projectile::ProjectileExt;
use mvs_sim::actors::{ActorRef, ProjectileId};
use mvs_sim::chars::stripe::saw::Saw;
use mvs_sim::chars::stripe::stripe_of;
use mvs_sim::fighter::FighterId;
use mvs_vfx::math::{V3, compose_rows, rotation_rows};

use super::{CharRender, PoseHook, PoseOf, PreviewOf, PreviewStep};
use crate::actor_view::{SkinnedActor, StaticView, Xform, xyz};
use crate::actors::{ActorComponents, ActorHost, ActorMesh, ActorPlace, MeshRule, Rows, class_def};
use crate::anim::{Clips, HeadScaleNode, Pose};
use crate::anim_nodes::{AnimNode, AnimNodeSkel, BoneDrivenControllerNode, ConstraintNode, CsPose, UeXf};
use crate::curves::{CurveKey, CurveTable, rich_curve};
use crate::frame::MeshPose;
use crate::view::{CharView, PoseCx, PoseRules};

pub(crate) const RENDER: CharRender = CharRender {
    // C023B_AnimBP's head scale nodes (HeadScale @6157): ModifyBone_3 (fml_un_C_Face_att) and ModifyBone_4
    // (fml_un_C_head) at HeadScale, ModifyBone_2 (fml_un_ear_att) and ModifyBone_1 (fml_un_hair_att) at their factors
    // behind the gate.
    head_scale: &[
        HeadScaleNode {
            node: "ModifyBone_3",
            bone: "fml_un_C_Face_att",
            factor: 1.0,
            gated: false,
            moves: [0.0; 3],
            scale: true,
        },
        HeadScaleNode {
            node: "ModifyBone_4",
            bone: "fml_un_C_head",
            factor: 1.0,
            gated: false,
            moves: [0.0; 3],
            scale: true,
        },
        HeadScaleNode {
            node: "ModifyBone_2",
            bone: "fml_un_ear_att",
            factor: 0.5,
            gated: true,
            moves: [0.0; 3],
            scale: true,
        },
        HeadScaleNode {
            node: "ModifyBone_1",
            bone: "fml_un_hair_att",
            factor: 0.9,
            gated: true,
            moves: [0.0; 3],
            scale: true,
        },
    ],
    turns: true,
    // Unverified: the props (Gun, Skateboard, the chainsaws, ...) posed as copies under the one scale (Velma's Book,
    // Rick's).
    copied_props: true,
    pose: Some(stripe_pose),
    ..CharRender::GENERIC
};

/// The classes' own mesh drawing, by class name.
pub(crate) const RULES: &[(&str, MeshRule)] = &[
    (SAW, MeshRule { rot: None, mesh: Some(saw_mesh) }),
    (SAW_ON_GROUND, MeshRule { rot: None, mesh: Some(saw_mesh) }),
    (SAW_FROM_ALLY, MeshRule { rot: None, mesh: Some(saw_mesh) }),
    (DROPPED_BOMB, MeshRule { rot: None, mesh: Some(bomb_mesh) }),
    (BOOMBOX, MeshRule { rot: None, mesh: Some(boombox_mesh) }),
    (ALLY_SAW, MeshRule { rot: None, mesh: Some(ally_saw_mesh) }),
];

const SAW: &str = "C023B_SawBlade_EdgeTrace_C";
const SAW_ON_GROUND: &str = "C023B_SawBlade_EdgeTrace_StartOnGround_C";
const SAW_FROM_ALLY: &str = "C023B_SawBlade_FromAlly_EdgeTrace_C";
const DROPPED_BOMB: &str = "C023B_DroppedBomb_C";
const BOOMBOX: &str = "BoomBox_SpawnedActor_C";
const ALLY_SAW: &str = "C023B_AllySaw_C";

const STEP: f64 = 1.0 / 60.0;
const DEG: f64 = std::f64::consts::PI / 180.0;
/// Math.fround.
fn f32r(v: f64) -> f64 {
    v as f32 as f64
}

/// The Capsule's RelativeScale3D.X (`capScale`).
fn cap_scale(w: &World, a: ActorRef) -> f64 {
    let c = class_def(w, a).and_then(|d| d.components.get("Capsule")).and_then(Value::as_obj);
    xyz(c.and_then(|c| c.get("RelativeScale3D")), 1.0)[0]
}

/// A component's RelativeRotation (Pitch, Yaw, Roll) in degrees.
fn rel_rot(w: &World, a: ActorRef, comp: &str) -> [f64; 3] {
    let r = class_def(w, a)
        .and_then(|d| d.components.get(comp))
        .and_then(Value::as_obj)
        .and_then(|c| c.get("RelativeRotation"))
        .and_then(Value::as_obj);
    let g = |k: &str| r.and_then(|r| r.get(k)).and_then(Value::as_f64).unwrap_or(0.0);
    [g("Pitch"), g("Yaw"), g("Roll")]
}

/// `relRows`: rotation rows of a rotator in degrees.
fn rows_of(r: [f64; 3]) -> Rows {
    rotation_rows(r[0] * DEG, r[1] * DEG, r[2] * DEG)
}

/// `x` turned by the rows `rot` (UE row vectors), scaled by `s`.
fn turned(x: V3, rot: &Rows, s: f64) -> V3 {
    std::array::from_fn(|j| s * (x[0] * rot[j] + x[1] * rot[3 + j] + x[2] * rot[6 + j]))
}

// ---- the pose hook ----

/// The float curves its AnimBP reads by name (GetCurveValue), in the order the hook indexes them.
const CURVES: CurveTable =
    CurveTable::new(&["show_Skateboard", "show_Gun", "show_sawblade", "show_Chainsaw", "disableChainsawOffset"]);
const NC: usize = 5;
const C_SKATE: usize = 0;
const C_GUN: usize = 1;
const C_SAW: usize = 2;
const C_CS: usize = 3;
const C_OFF: usize = 4;
/// The manager's tags (lower case) the update writes; their C023B_AnimBP states are SkateboardVisibility,
/// GunVisibility, SawbladeVisibility, ChainsawVisibility and ChainsawVisibilityMI.
const TAGS: [&str; 5] = ["skateboard", "gun", "sawblade", "chainsaw", "chainsawmi"];
const SKATE: usize = 0;
const GUN: usize = 1;
const SAW_TAG: usize = 2;
const CS: usize = 3;
const MI: usize = 4;
const COM_BONE: &str = "fml_un_COM";
const SKATE_BONE: &str = "skateboard_att";
const BONE_DRIVEN: &str = "AnimGraphNode_BoneDrivenController";
const CONSTRAINT: &str = "AnimGraphNode_Constraint";
/// The slot graph's Slot nodes the sim mixes (pawn.rs slot_input): the gun montages' Aim* / AimBack* by
/// GunAimDirection / AimBack under the IsFalling list, the air saw throw's BaseAim* by AimDirection, DefaultSlot,
/// DashSlot and ChainLeanBase. Not listed: ChainLeanUp / ChainLeanDown (ChainAimDirection has no writer).
const SLOT_NODES: [&str; 18] = [
    "DefaultSlot",
    "DashSlot",
    "ChainLeanBase",
    "AimNutralGround",
    "AimUpGround",
    "AimDownGround",
    "AimNutralAir",
    "AimUpAir",
    "AimDownAir",
    "AimBackNutral",
    "AimBackUp",
    "AimBackDown",
    "AimBackNutralAir",
    "AimBackUpAir",
    "AimBackDownAir",
    "BaseAimNutral",
    "BaseAimUp",
    "BaseAimDown",
];
static GRAPH_CHECKED: AtomicBool = AtomicBool::new(false);

/// `stripePose`'s state on one view.
#[derive(Debug)]
struct StripePose {
    part_of: [Option<usize>; 5],
    skate: Option<usize>,
    /// The BoneDrivenController and the Constraint, made at the first set() (None: the data has not both).
    nodes: Option<(BoneDrivenControllerNode, ConstraintNode)>,
    made: bool,
    cs: CsPose,
    tgt: [f64; NC],
    cur: [f64; NC],
    from: [f64; NC],
    fades: u32,
    /// The *Visibility variables, by tag.
    state: [bool; 5],
    /// Per tag: the AnimBP's last write (None: none), the sim's last visibility of the mesh (None: not seen), whether
    /// the sim has it in AlwaysVisibleMeshes, and whether a hide of the AnimBP took it out of that set since (cut).
    own: [Option<bool>; 5],
    sim_vis: [Option<bool>; 5],
    sim_av: [bool; 5],
    cut: [bool; 5],
    /// The evaluation's inputs of this update: the BoneDriven / Constraint alpha, the COM Replace's pitch
    /// (ModifyBone_7 / _9 / _5 as one) and skateboard_att's (ModifyBone_6), in degrees.
    offset_on: bool,
    com: Option<f64>,
    fall: Option<f64>,
    /// The fighter's saws seen so far (hideSawblade at a new one).
    saws: Vec<ProjectileId>,
    /// PoseOf::slaved: no AnimBP of its own (set returns after the graph check, hideSawblade does nothing).
    slaved: bool,
    /// Per tag: the AnimBP wrote it in this update (a preview's parts).
    wrote: [bool; 5],
    /// A preview's next step is its first (its curves seeded from that frame).
    preview_first: bool,
}

fn stripe_pose(view: &mut CharView, _clips: &Clips, of: &PoseOf<'_>) -> Box<dyn PoseHook> {
    let part_of = TAGS.map(|t| of.part_keys.iter().position(|k| k.eq_ignore_ascii_case(t)));
    for (t, p) in TAGS.iter().zip(&part_of) {
        if p.is_none() {
            eprintln!("warning: Stripe pose: the skin has no sub-mesh {t}");
        }
    }
    let skate = view.rig.index(SKATE_BONE);
    if view.rig.index(COM_BONE).is_none() || skate.is_none() {
        eprintln!("warning: Stripe pose: bone missing: {COM_BONE} or {SKATE_BONE}");
    }
    Box::new(StripePose {
        part_of,
        skate,
        nodes: None,
        made: false,
        cs: CsPose::new(&view.rig.names, &view.parents),
        tgt: [0.0; NC],
        cur: [0.0; NC],
        from: [0.0; NC],
        fades: 0,
        state: [false; 5],
        own: [None; 5],
        sim_vis: [None; 5],
        sim_av: [false; 5],
        cut: [false; 5],
        offset_on: false,
        com: None,
        fall: None,
        saws: Vec::new(),
        slaved: of.slaved,
        wrote: [false; 5],
        preview_first: false,
    })
}

impl StripePose {
    /// SetSubMeshVisibility on tag `t`.
    fn write(&mut self, t: usize, vis: bool) {
        self.own[t] = Some(vis);
        self.wrote[t] = true;
        if !vis {
            self.cut[t] = true;
        }
    }

    /// ToggleSubmeshVisibility(curve `c`, tag `t`, its state).
    fn toggle(&mut self, c: usize, t: usize) {
        let on = self.cur[c] >= 0.5;
        if on != self.state[t] {
            self.write(t, !self.state[t]);
            self.state[t] = !self.state[t];
        }
    }

    /// ToggleSubmeshes on the curves of the evaluation before.
    fn update(&mut self, left: bool) {
        self.toggle(C_SKATE, SKATE);
        self.toggle(C_GUN, GUN);
        self.toggle(C_SAW, SAW_TAG);
        let (on, off) = if left { (MI, CS) } else { (CS, MI) };
        self.toggle(C_CS, on);
        if self.state[off] {
            self.write(off, false);
            self.state[off] = false;
        }
        self.offset_on = self.state[MI] && self.cur[C_OFF] < 0.1;
    }

    /// A preview's parts: those the AnimBP wrote in this update; the others as the preview's manager has them.
    fn preview_parts(&mut self, view: &mut CharView) {
        for t in 0..TAGS.len() {
            if let (true, Some(p)) = (self.wrote[t], self.part_of[t]) {
                view.set_part_visible(p, self.own[t] == Some(true));
            }
        }
        self.wrote.fill(false);
    }

    /// The two nodes from the data's properties, the driver's multiplier negated (the module doc).
    fn make_nodes(&mut self, w: &World, f: FighterId, view: &CharView) {
        self.made = true;
        let nodes = w.char_data(f).fighter.anim_nodes.as_ref();
        let (Some(bd), Some(cn)) = (nodes.and_then(|m| m.get(BONE_DRIVEN)), nodes.and_then(|m| m.get(CONSTRAINT)))
        else {
            eprintln!("warning: Stripe pose: the fighter data has no {BONE_DRIVEN} or {CONSTRAINT}");
            return;
        };
        let skel = AnimNodeSkel { names: &view.rig.names, parents: &view.parents, rest: Some(&view.rest) };
        let mut props = bd.props.clone();
        let m = props.get("Multiplier").and_then(Value::as_f64).unwrap_or(1.0);
        props.insert("Multiplier".to_string(), Value::Num(-m));
        self.nodes = Some((BoneDrivenControllerNode::new(&props, &skel), ConstraintNode::new(&cn.props, &skel)));
    }
}

impl PoseRules for StripePose {
    /// The update's nodes on the pose, then this evaluation's curves (the view's crossfade of the targets), for the
    /// next update.
    fn rule(&mut self, cx: &mut PoseCx<'_>, p: &mut Pose, _dt: f64) {
        if self.offset_on
            && let Some((driven, constraint)) = &mut self.nodes
        {
            let comp = UeXf::default();
            let mut cs = self.cs.begin(p);
            driven.update(0.0, 1.0);
            driven.evaluate(&mut cs, &comp);
            constraint.update(0.0, 1.0);
            constraint.evaluate(&mut cs, &comp);
        }
        if let Some(pitch) = self.com {
            let h = pitch * std::f64::consts::PI / 360.0;
            cx.turn_com(p, [0.0, 0.0, h.sin(), h.cos()]);
        }
        if let (Some(pitch), Some(b)) = (self.fall, self.skate) {
            let h = pitch * std::f64::consts::PI / 360.0;
            p.r[b * 4] = 0.0;
            p.r[b * 4 + 1] = 0.0;
            p.r[b * 4 + 2] = h.sin() as f32;
            p.r[b * 4 + 3] = h.cos() as f32;
        }
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

impl PoseHook for StripePose {
    /// `preview()`: a preview actor's view (no fighter): the AnimBP anew, run by preview_step. Its two nodes
    /// are made at a fighter's first set: a preview has none.
    fn preview(&mut self, view: &mut CharView, _of: &PreviewOf<'_>) {
        self.reset(&mut |_, _, _| {});
        self.wrote = [false; 5];
        self.fades = view.fade_count();
        self.preview_first = true;
    }

    /// `previewStep(anim, time, facingLeft)`: ToggleSubmeshes on the curves of the evaluation before, the
    /// parts the update wrote, then the curves of the frame's sequence.
    fn preview_step(&mut self, view: &mut CharView, clips: &Clips, s: &PreviewStep<'_>) {
        if std::mem::take(&mut self.preview_first) {
            CURVES.fill(clips, s.anim, s.time, &mut self.cur);
            self.from = self.cur;
        }
        self.update(s.facing_left);
        self.preview_parts(view);
        CURVES.fill(clips, s.anim, s.time, &mut self.tgt);
    }

    /// init(): the AnimBP's state anew (the crossfade's count needs no reset: cur and from are both 0), the nodes'
    /// alpha, the saws seen.
    fn reset(&mut self, _part_scalar: &mut dyn FnMut(usize, &str, f64)) {
        self.tgt = [0.0; NC];
        self.cur = [0.0; NC];
        self.from = [0.0; NC];
        self.state = [false; 5];
        self.own = [None; 5];
        self.sim_vis = [None; 5];
        self.sim_av = [false; 5];
        self.cut = [false; 5];
        self.offset_on = false;
        self.com = None;
        self.fall = None;
        if let Some((driven, constraint)) = &mut self.nodes {
            driven.initialize();
            constraint.initialize();
        }
        self.saws.clear();
    }

    /// The AnimBP's update of the tick, before the view's pose: the sim's changes of the tick, the update on the
    /// curves of the evaluation before, the parts' visibility, the COM inputs, this tick's curve targets, then the
    /// saw's hide.
    fn set(&mut self, w: &World, f: FighterId, view: &mut CharView, clips: &Clips) {
        let fi = w.fighter(f);
        if !GRAPH_CHECKED.swap(true, Ordering::Relaxed) {
            let g = &w.char_data(f).fighter.slot_holds.graph;
            let has = |s: &str| g.nodes.iter().any(|n| matches!(n, SlotGraphNode::Slot { slot, .. } if slot == s));
            let missing: Vec<&str> = SLOT_NODES.into_iter().filter(|s| !has(s)).collect();
            if !missing.is_empty() {
                eprintln!("warning: Stripe pose: the data's C023B_AnimBP slot graph lacks {}", missing.join(", "));
            }
        }
        if self.slaved {
            return;
        }
        if !self.made {
            self.make_nodes(w, f, view);
        }
        let name = |t: &str| w.names.get(t);
        // the sim's changes of the tick: its write is the later one
        for (t, &tag) in TAGS.iter().enumerate() {
            let n = name(tag);
            let vis = self.part_of[t].is_some() && !n.is_some_and(|n| fi.pose.sub_hidden.contains(&n));
            if self.sim_vis[t].is_some_and(|s| s != vis) {
                self.own[t] = None;
            }
            self.sim_vis[t] = Some(vis);
            let av = fi.vis.always_visible.as_ref().is_some_and(|s| n.is_some_and(|n| s.contains(&n)));
            if !av || !self.sim_av[t] {
                self.cut[t] = false;
            }
            self.sim_av[t] = av;
        }
        self.update(fi.body.facing_dir < 0.0);
        self.wrote = [false; 5];
        // the parts: under an AddInvisibility (the view's parts-only mode) the meshes the count leaves shown and no
        // hide of the AnimBP took out since; else the AnimBP's write until the sim changes the mesh
        let kept = if fi.vis.invisible { fi.vis.invis_shown.as_ref() } else { fi.vis.always_visible.as_ref() };
        for (t, &tag) in TAGS.iter().enumerate() {
            let Some(p) = self.part_of[t] else { continue };
            let on = if view.parts_only {
                kept.is_some_and(|s| name(tag).is_some_and(|n| s.contains(&n))) && !self.cut[t]
            } else {
                self.own[t].unwrap_or(self.sim_vis[t] == Some(true))
            };
            view.part_on[p] = on;
        }
        // the COM nodes: the pitch the sim's pose_shapes turns the shapes by
        let sp = stripe_of(w, f);
        self.com = None;
        self.fall = None;
        if let Some(c) = sp.map(|s| &s.pawn) {
            if c.ev_fall_on {
                self.fall = Some(f64::from(c.ev_fall));
            }
            let a = c.ev_com_alpha;
            if c.ev_fall_on || a > 1e-5 || c.ev_saw_on {
                let mut pitch =
                    if c.ev_fall_on { c.ev_fall } else { mvs_sim::fighter::anim::hit_pitch(w, f).unwrap_or(0.0) };
                if a > 1e-5 {
                    pitch = if a >= 0.99999 {
                        c.ev_com_pitch
                    } else {
                        mvs_sim::math::narrow(nlerp_pitch(f64::from(pitch), f64::from(c.ev_com_pitch), f64::from(a)))
                    };
                }
                if c.ev_saw_on {
                    pitch = c.ev_saw_rot;
                }
                self.com = Some(f64::from(pitch));
            }
        }
        let anim = w.names.str(fi.anim.anim);
        CURVES.fill(clips, anim, f64::from(fi.anim.anim_time), &mut self.tgt);
        if let Some(s) = sp {
            for x in &s.pawn.slot_mix.slot_mix {
                let clip = clips.get(w.names.str(x.anim));
                for k in 0..NC {
                    self.tgt[k] += (CURVES.value_of(clips, clip, k, f64::from(x.time)) - self.tgt[k]) * f64::from(x.w);
                }
            }
        }
        // hideSawblade: a saw's BeginPlay @923, at its first sight
        if let Some(s) = sp {
            self.saws.retain(|id| s.saws.contains(id));
            let mut new = false;
            for &id in &s.saws {
                if !self.saws.contains(&id) {
                    self.saws.push(id);
                    new = true;
                }
            }
            if new {
                self.write(SAW_TAG, false);
                if let Some(p) = self.part_of[SAW_TAG]
                    && !view.parts_only
                {
                    view.part_on[p] = false;
                }
            }
        }
    }
}

/// `nlerpPitch`: two pitches as quaternions about one axis, FQuat's shortest-path nlerp at alpha, normalised; the
/// pitch of the result in degrees.
fn nlerp_pitch(from: f64, to: f64, a: f64) -> f64 {
    let h0 = from * std::f64::consts::PI / 360.0;
    let h1 = to * std::f64::consts::PI / 360.0;
    let (s0, c0) = (h0.sin(), h0.cos());
    let (mut s1, mut c1) = (h1.sin(), h1.cos());
    if s0 * s1 + c0 * c1 < 0.0 {
        s1 = -s1;
        c1 = -c1;
    }
    2.0 * ((1.0 - a) * s0 + a * s1).atan2((1.0 - a) * c0 + a * c1) * 180.0 / std::f64::consts::PI
}

// ---- the saw ----
//
// C023B_SawBlade_EdgeTrace_C and its data children _StartOnGround_C / _FromAlly_EdgeTrace_C (mvs-sim
// chars/stripe/saw.rs): Sawblade (StaticMesh Sawblade, yaw 180, scale 0.8 under the Capsule; its own material
// C023B_Props_A_MIC) spun by ReceiveTick @9455-9703 while SpinOn: spinAmount += DeltaSeconds x 180, the relative
// rotation's pitch = spinAmount (its roll and yaw kept). ToggleVFX(On, Clockwise, OnWall) @8419 sets SpinOn = On.
// The ToggleVFX calls the sim does not record are read off the saw's state each tick ([`SawToggle`], the effects'
// side reads the same: mvs-render fx/chars/stripe.rs):
//   on an edge (Comp_RunAlongEdgeTrace.Active: the attach @7042, the corner @8740, the stationary resume @1623, the
//   reverse @1092): (true, travel X >= 0 in sim axes, true): !(CurrentTravelDirection.Y > 0) as @8711;
//   'Attack ID' changed (HitLaunch @7420 -> @7931): (true, the launch's X < 0: sim vx > 0, false);
//   CanSetTimer false -> true off an edge with the same attack id (HitEnemyPlayer's BounceGate branch @2617 ->
//   @2784): (false, -, false).
// Before the first call the saw is not spinning.
// Unverified: the calls are inferred from the state the sim keeps (a launch and a bounce in one tick read as the
// launch; a detach (the reverse @10123, the stationary pause @1532) calls none, as the build).
// The saw drawn half into the floor is the build's own: every Attach To Edge call (@142, @1196, @1676, @6354) passes a
// zero offset, which puts the Capsule's centre on the edge.

/// Degrees a second (@9465).
const SPIN_RATE: f64 = 180.0;

/// The ToggleVFX values the saw's state gives (`SawFx`): `on` None before the
/// first call; `spin` the spinAmount it counts while on.
#[derive(Clone, Copy, Debug)]
pub struct SawToggle {
    pub on: Option<bool>,
    pub cw: bool,
    pub wall: bool,
    pub spin: f64,
    age: i32,
    attack: i32,
    can_set: bool,
}

/// What one tick's read of the saw changed (`toggleVfx`'s call, None: no call).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SawCall {
    pub on: bool,
    pub cw: bool,
    pub wall: bool,
}

impl SawToggle {
    /// At the saw's first sight.
    pub fn new(s: &Saw, age: i32) -> SawToggle {
        SawToggle {
            on: None,
            cw: false,
            wall: false,
            spin: 0.0,
            age,
            attack: s.saw_attack_id,
            can_set: s.can_set_timer,
        }
    }

    /// The tick's read (`SAW_FX.tick`): the saw's state at the projectile's `age`, `vx` its velocity's sim x.
    pub fn tick(&mut self, s: &Saw, vx: f64, age: i32) -> Option<SawCall> {
        let mut call = None;
        if s.rider.active {
            // dirX: the travel direction's sim x (-CurrentTravelDirection.Y)
            let cw = -s.rider.ctd[1] >= 0.0;
            if self.on != Some(true) || self.cw != cw || !self.wall {
                call = Some(SawCall { on: true, cw, wall: true });
            }
        } else if s.saw_attack_id != self.attack {
            call = Some(SawCall { on: true, cw: vx > 0.0, wall: false });
        } else if s.can_set_timer && !self.can_set {
            call = Some(SawCall { on: false, cw: self.cw, wall: false });
        }
        if let Some(c) = call {
            self.on = Some(c.on);
            self.cw = c.cw;
            self.wall = c.wall;
        }
        self.attack = s.saw_attack_id;
        self.can_set = s.can_set_timer;
        let ticks = age - self.age;
        self.age = age;
        if self.on == Some(true) && ticks > 0 {
            self.spin += f64::from(ticks) * STEP * SPIN_RATE;
        }
        call
    }
}

/// A projectile's saw state (`sawView`).
pub fn saw_of(w: &World, id: ProjectileId) -> Option<(&Saw, f64, i32)> {
    let pr = w.actors.projectiles.get(id)?;
    match &pr.ext {
        ProjectileExt::Saw(s) => Some((s, f64::from(pr.vx), pr.age)),
        _ => None,
    }
}

#[derive(Debug)]
struct SawMesh {
    id: ProjectileId,
    view: StaticView,
    cap: f64,
    rs: V3,
    yaw: f64,
    roll: f64,
    toggle: Option<SawToggle>,
}

fn saw_mesh(host: &mut ActorHost<'_>, w: &World, a: ActorRef, cs: &ActorComponents) -> Option<Box<dyn ActorMesh>> {
    let ActorRef::Projectile(id) = a else { return None };
    let pr = w.actors.projectiles.get(id)?;
    let c = cs.get("Sawblade")?;
    let crate::actors::ComponentKind::StaticMesh { mesh, .. } = &c.kind else { return None };
    let from = mvs_sim::fighter::common::actor_def_owner(w, pr.owner?, pr.cls);
    // (no variant: the entry's own material, staticView's default)
    let view = StaticView::new(host, w, from, mesh, "")?;
    let r = rel_rot(w, a, "Sawblade");
    Some(Box::new(SawMesh { id, view, cap: cap_scale(w, a), rs: c.scale, yaw: r[1], roll: r[2], toggle: None }))
}

impl ActorMesh for SawMesh {
    /// The spin counted once a tick, as the effects' tick.
    fn tick(&mut self, _host: &mut ActorHost<'_>, w: &World) {
        let Some((s, vx, age)) = saw_of(w, self.id) else { return };
        self.toggle.get_or_insert_with(|| SawToggle::new(s, age)).tick(s, vx, age);
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
        let spin = self.toggle.map_or(0.0, |t| t.spin);
        let rel = rows_of([spin, self.yaw, self.roll]);
        let (x, y) = at.at(alpha);
        let s = self.cap * at.scale;
        let scale = std::array::from_fn(|k| s * self.rs[k]);
        self.view.set(&Xform { loc: [0.0, -x, y], rot: compose_rows(&rel, &at.rot), scale });
        let first = out.len();
        self.view.draw(out);
        // (the class's Sawblade sets bReceivesDecals false: the saw's own ground decal does not stamp the blade)
        for p in &mut out[first..] {
            p.no_decals = true;
        }
    }
}

// ---- the dropped bomb ----
//
// C023B_DroppedBomb_C (an item, mvs-sim chars/stripe/bomb.rs): SkeletalMesh (its mesh set at SetTeamColor @2723 to the
// instigator's skin sub-mesh Bomb: C023B_DynamiteAttached_SkelMesh, no AnimClass: the reference pose; slot 0 the
// sub-mesh's Red / Blue / LocalPlayer override @3465) at (-56.21, -120.54, -62), yaw -25 under the Capsule. Its
// TimerEffect is the effects' (mvs-render fx/chars/stripe.rs).

/// The skin's sub-mesh (SetTeamColor's Map_Find(AppliedSkin.SubMeshes, Bomb)).
const BOMB_KEY: &str = "Bomb";

#[derive(Debug)]
struct BombMesh {
    view: SkinnedActor,
    rel: V3,
    rel_rot: Rows,
    rs: V3,
}

/// The dropped bomb's instigator (the character whose data has the class) and that fighter's applied skin.
fn bomb_skin(w: &World, it: &mvs_sim::actors::Item) -> Option<(FighterId, crate::skins::Skin)> {
    let owner = it.owner.or(it.spawner)?;
    let from = mvs_sim::fighter::common::actor_def_owner(w, owner, it.cls);
    let pick =
        usize::try_from(w.fighter(from).id).ok().and_then(|i| w.setup.players.get(i)).and_then(|p| p.skin.as_deref());
    Some((from, crate::skins::skin_of(&w.char_data(from).fighter, pick).ok()?))
}

/// Whether the dropped bomb item `it` is drawn (bombMesh): its instigator's applied skin has a Bomb entry. Its
/// TimerEffect (the effects') is made with the mesh alone.
pub fn bomb_drawn(w: &World, it: &mvs_sim::actors::Item) -> bool {
    bomb_skin(w, it).is_some_and(|(_, s)| s.parts.iter().any(|p| p.key.eq_ignore_ascii_case(BOMB_KEY)))
}

fn bomb_mesh(host: &mut ActorHost<'_>, w: &World, a: ActorRef, cs: &ActorComponents) -> Option<Box<dyn ActorMesh>> {
    let ActorRef::Item(id) = a else { return None };
    let it = w.actors.items.get(id)?;
    let owner = it.owner.or(it.spawner)?;
    let from = mvs_sim::fighter::common::actor_def_owner(w, owner, it.cls);
    let skin = bomb_skin(w, it).map(|(_, s)| s);
    // (the instigator's applied skin's Bomb entry)
    let part = skin.as_ref().and_then(|s| s.parts.iter().find(|p| p.key.eq_ignore_ascii_case(BOMB_KEY)));
    let c = cs.get("SkeletalMesh");
    let (Some(skin), Some(part), Some(c)) = (skin.as_ref(), part, c) else {
        eprintln!("warning: Stripe: the dropped bomb is not drawn (no Bomb sub-mesh or SkeletalMesh component)");
        return None;
    };
    let info = host.mesh(part.path.as_deref()?)?;
    let variant = host.colour(w, owner).variant;
    let d = host.draws_of(w, from)?;
    let keys =
        if part.own { d.actor(&part.mesh, None).map(|a| a.1)? } else { d.set(&skin.name, &part.key, variant).ok()? };
    let r = rel_rot(w, a, "SkeletalMesh");
    Some(Box::new(BombMesh { view: SkinnedActor::new(info, &keys), rel: c.loc, rel_rot: rows_of(r), rs: c.scale }))
}

impl ActorMesh for BombMesh {
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
        let r = turned(self.rel, &at.rot, at.scale);
        let loc = [r[0], -x + r[1], y + r[2]];
        let scale = std::array::from_fn(|k| at.scale * self.rs[k]);
        self.view.set(&Xform { loc, rot: compose_rows(&self.rel_rot, &at.rot), scale });
        self.view.draw(out);
    }
}

// ---- the boombox ----
//
// BoomBox_SpawnedActor_C (mvs-sim chars/stripe/dash.rs BoomBox: a Projectile kind): Mesh (C023B_Boombox_SkelMesh at
// (0, 0, -39.03), yaw 50, scale 1.1 under the Capsule; PlayAnimation with no asset: the reference pose) under the
// actor's rotation. Each Detonate (BoomBox.detonations):
// - ShakeAnimEvent @3735 -> @1010 ShakeAnim.PlayFromStart: its update @2870 sets the Mesh's relative rotation to
//   MakeRotator(Roll ShakeAlpha * 40, Pitch Rot2 * 25, Yaw 50) @3005 (the curves below, 0.5 s); then @29 BoomBox_Mat
//   (slot 0's dynamic instance @1564) BaseColor = (0.1875, 0.1875, 0.1875, 1), which stays.
// Its explosion and its blob shadow Decal (Character_BlobShadow_MAT under the Mesh, turned with it) are the effects'
// (mvs-render fx/chars/stripe.rs), placed by boom_place.
// Unverified: the slot material's drawn copy takes a BaseColor vector (a name it lacks changes nothing).
// Deviation: not drawn: MulticastDamageTaken's Mesh.PlayAnimation(MouseTrap_SnapFast) @3092 (ReceiveAnyDamage's).

const BOOMBOX_MESH: &str = "C023B_Boombox_SkelMesh";
const DARK: [f64; 4] = [0.1875, 0.1875, 0.1875, 1.0];
const SHAKE_LENGTH: f64 = 0.5;
/// ShakeAnim's CurveFloat_0 (ShakeAlpha) and CurveFloat_2 (Rot2).
const SHAKE_ALPHA: &[CurveKey] = &[
    [0.0, 1.0, 0.0, 0.0, 1.0],
    [0.10978439, -0.8, 0.045947492, 0.045947876, 1.0],
    [0.2530359, 0.6, -0.008768775, -0.008768965, 1.0],
    [0.35586274, -0.4, 0.08703519, 0.0870355, 1.0],
    [0.49742946, 0.0, 0.0, 0.0, 0.0],
];
const SHAKE_ROT2: &[CurveKey] = &[
    [0.0, 1.0, 0.0, 0.0, 1.0],
    [0.049602672, -0.89090264, -0.89411926, -0.89411926, 1.0],
    [0.13019931, 0.88358635, 1.158929, 1.158929, 1.0],
    [0.17088144, -0.75034916, -1.6598793, -1.6598793, 1.0],
    [0.2307532, 0.716679, 1.6227177, 1.6227177, 1.0],
    [0.2737381, -0.58344185, -2.6208627, -2.6208627, 1.0],
    [0.3313071, 0.45314097, 1.4405593, 1.4405593, 1.0],
    [0.38350302, -0.42531896, -1.4927524, -1.4927524, 1.0],
    [0.4372341, 0.2950182, 3.650901, 3.650901, 1.0],
    [0.5, 0.0, 0.0, 0.0, 1.0],
];

/// The timeline's position k ticks after PlayFromStart (a fixed step's float32 sum, clamped to the length).
fn timeline_at(k: i32, length: f64) -> f64 {
    let mut t = 0.0;
    let mut i = 0;
    while i < k && t < length {
        t = f32r(t + f64::from(mvs_sim::consts::DELTA_SECONDS));
        i += 1;
    }
    t.min(length)
}

/// The box's Mesh place (`BoomPlace`): its data transform under the actor's.
#[derive(Clone, Copy, Debug)]
pub struct BoomPlace {
    pub rel: V3,
    pub rel_rot: Rows,
    pub rs: V3,
}

/// `boomPlaceOf`: the class's Mesh component.
pub fn boom_place_of(w: &World, a: ActorRef) -> BoomPlace {
    let c = class_def(w, a).and_then(|d| d.components.get("Mesh")).and_then(Value::as_obj);
    BoomPlace {
        rel: xyz(c.and_then(|c| c.get("RelativeLocation")), 0.0),
        rel_rot: rows_of(rel_rot(w, a, "Mesh")),
        rs: xyz(c.and_then(|c| c.get("RelativeScale3D")), 1.0),
    }
}

/// `boomPlace`: the Mesh's world place of box `id` drawn at (x, y) under the actor's rows `root` and scale `s`, with
/// the relative rotation the shake's update wrote after a Detonate.
pub fn boom_place(w: &World, id: ProjectileId, q: &BoomPlace, x: f64, y: f64, root: &Rows, s: f64) -> Xform {
    let mut rel_rot = q.rel_rot;
    if let Some(pr) = w.actors.projectiles.get(id)
        && let ProjectileExt::BoomBox(b) = &pr.ext
        && b.detonations > 0
    {
        let t = timeline_at(pr.age - b.detonate_age, SHAKE_LENGTH);
        rel_rot = rotation_rows(
            f32r(rich_curve(SHAKE_ROT2, t) * 25.0) * DEG,
            50.0 * DEG,
            f32r(rich_curve(SHAKE_ALPHA, t) * 40.0) * DEG,
        );
    }
    let r = turned(q.rel, root, s);
    Xform {
        loc: [r[0], r[1] - x, r[2] + y],
        rot: compose_rows(&rel_rot, root),
        scale: std::array::from_fn(|k| s * q.rs[k]),
    }
}

#[derive(Debug)]
struct BoomboxMesh {
    id: ProjectileId,
    view: SkinnedActor,
    q: BoomPlace,
    dark: bool,
}

fn boombox_mesh(host: &mut ActorHost<'_>, w: &World, a: ActorRef, _cs: &ActorComponents) -> Option<Box<dyn ActorMesh>> {
    let ActorRef::Projectile(id) = a else { return None };
    let pr = w.actors.projectiles.get(id)?;
    let from = mvs_sim::fighter::common::actor_def_owner(w, pr.owner?, pr.cls);
    let Some((path, keys)) = host.draws_of(w, from).and_then(|d| d.actor(BOOMBOX_MESH, None)) else {
        eprintln!("warning: Stripe: the boombox is not drawn (its model or materials are not loaded)");
        return None;
    };
    let info = host.mesh(&path)?;
    Some(Box::new(BoomboxMesh { id, view: SkinnedActor::new(info, &keys), q: boom_place_of(w, a), dark: false }))
}

impl ActorMesh for BoomboxMesh {
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
        if !shown {
            return;
        }
        let (x, y) = at.at(alpha);
        let xf = boom_place(w, self.id, &self.q, x, y, &at.rot, at.scale);
        let detonated = w
            .actors
            .projectiles
            .get(self.id)
            .is_some_and(|p| matches!(&p.ext, ProjectileExt::BoomBox(b) if b.detonations > 0));
        if !self.dark && detonated {
            self.dark = true;
            if let Some(Some(m)) = self.view.mats.first_mut() {
                crate::pose::set_vector(m, "BaseColor", DARK);
            }
        }
        self.view.set(&xf);
        self.view.draw(out);
    }
}

// ---- the ally saw ----
//
// C023B_AllySaw_C (mvs-sim chars/stripe/saw.rs AllySaw, a World.actors kind): SawBlade (StaticMesh Sawblade at
// (217.06, 0, 0), scale 0.6 under the Capsule). RotatingMovement (Roll 260 a second, world space) turns the actor
// about the world X axis: the sim keeps the blade's place on the orbit (ox, oy), so the actor's rows are read off it:
// X toward the blade, Y the spawn yaw's (+-90 by the ally's facing: (+-1, 0, 0)), Z = X x Y. Its ParticleSystem is
// the effects' (mvs-render fx/chars/stripe.rs).
// Deviation: drawn at the sim's place of the latest tick (no in-between frame).

/// The ally saw's rows and the blade's world place (`allySawMesh`'s place): (loc, rows, the root's scale).
pub fn ally_saw_place(w: &World, a: ActorRef) -> Option<(V3, Rows, f64)> {
    let ActorRef::Actor(id) = a else { return None };
    let x = w.actors.actors.get(id).filter(|x| !x.dead)?;
    let mvs_sim::actors::world_actor::WorldActorState::AllySaw(s) = &x.state else { return None };
    let r = s.ox.hypot(s.oy);
    let r = if r == 0.0 { 1.0 } else { r };
    // X: toward the blade (UE (0, -x, y)); Y: (+-1, 0, 0); Z = X x Y
    let (xy, xz) = (-s.ox / r, s.oy / r);
    let yx = if s.last_facing > 0.0 { 1.0 } else { -1.0 };
    let rows = [0.0, xy, xz, yx, 0.0, 0.0, 0.0, xz * yx, -xy * yx];
    // the root's scale (the sim's size scale) over the blade's place and scale
    let g = x.size_scale.unwrap_or(1.0);
    Some(([0.0, f64::from(-(s.x + s.ox * g)), f64::from(s.y + s.oy * g)], rows.map(f64::from), f64::from(g)))
}

#[derive(Debug)]
struct AllySawMesh {
    a: ActorRef,
    view: StaticView,
    bs: V3,
}

fn ally_saw_mesh(host: &mut ActorHost<'_>, w: &World, a: ActorRef, cs: &ActorComponents) -> Option<Box<dyn ActorMesh>> {
    let ActorRef::Actor(id) = a else { return None };
    let x = w.actors.actors.get(id)?;
    let c = cs.get("SawBlade");
    let mesh = c.and_then(|c| match &c.kind {
        crate::actors::ComponentKind::StaticMesh { mesh, .. } => Some(mesh),
        _ => None,
    });
    let from = mvs_sim::fighter::common::actor_def_owner(w, x.owner, x.cls);
    let view = mesh.and_then(|m| StaticView::new(host, w, from, m, ""));
    let (Some(c), Some(view)) = (c, view) else {
        eprintln!("warning: Stripe: the ally saw blade is not drawn (Sawblade is not loaded)");
        return None;
    };
    Some(Box::new(AllySawMesh { a, view, bs: c.scale }))
}

impl ActorMesh for AllySawMesh {
    fn place(
        &mut self,
        _host: &mut ActorHost<'_>,
        w: &World,
        _at: &ActorPlace,
        alpha: f64,
        shown: bool,
        out: &mut Vec<MeshPose>,
    ) {
        let Some((loc, rot, g)) = ally_saw_place(w, self.a) else { return };
        self.view.visible = shown;
        self.view.ticked(w.flow.tick);
        self.view.set(&Xform { loc, rot, scale: self.bs.map(|v| v * g) });
        self.view.present(alpha);
        self.view.draw(out);
    }
}
