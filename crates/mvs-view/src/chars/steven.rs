//! Steven Universe's render module: the entry, the AnimBP's pose hook, the
//! pawn's Pink* props, the shield actors' meshes and the grenade's rotation, and the
//! Watermelon minion's view. His effects are mvs-render's fx/chars/steven.rs.
//!
//! The pose beside the shared one (Steven_AnimBP). The graph, input first (each node's LinkID is its index among the
//! class's AnimNode_ / MirrorNode properties): BlendListByBool (bActiveValue <- Gliding; BlendTime [0.25, 0.1], linear:
//! pose 0 the SlowFallState machine, pose 1 the linked layer) -> SaveCachedPose -> Slot JumpSlot -> Slot DefaultSlot ->
//! LocalToComponent -> ModifyBone (fml_un_C_spine_d) -> ModifyBone_1 (fml_un_C_spine_a) -> ComponentToLocal -> the
//! mirror node (Steven_MirrorTable, bEnabled <- FacingLeft) -> cached pose -> LocalToComponent_1 -> ModifyBone_19
//! (fml_un_C_stvn_face_att) / _18 (fml_un_C_head) -> _17 / _16 (wrists), _3 / _2 (loarmTwistLwr), _5 / _4 (loarmTwist),
//! _15 .. _6 (the ten finger roots) -> _22 / _21 / _20 (VB Spine, VB L_LowLeg, VB R_LowLeg) -> Trail
//! (fml_un_TigerMillionaire_C_tie_4) -> SpringBone (fml_un_TigerMillionaire_hair_att) -> ComponentToLocal_1.
//!
//! Ported here:
//! - The glide list: its update (FAlphaBlend over [0.25, 0.1], the SlowFallState machine: Into
//!   Steven_Nav_SlowFall_Enter, not looping, -> Loop Steven_Nav_SlowFall, looping, when GetRelevantAnimTimeRemaining
//!   == 0 @1960-2032, crossfade 0) is the sim's (mvs-sim chars/steven glide: his hit shapes are bones of the sim pose);
//!   the view reads its state after the tick (StevenGlideState) and evaluates it here, as Bugs's: child 0 the
//!   machine's state clip at its time, child 1 the linked layer (PoseCx::sample_layer); a child at or under 1e-5 is not
//!   evaluated; the list's curves are its children's by weight.
//! - ModifyBone / _1 (scale BMM_Additive in BCS_BoneSpace, (1, 1.05, 1) and (1, 0.9, 1); translation additive 0):
//!   the bone's local scale times Scale, componentwise (the FTransform rule; the pose's (x, y, z) = UE (X, Z, Y)).
//!   Before the mirror node in the graph: the reflection the view draws a left facing with leaves a local scale as it
//!   is.
//! - ModifyBone_18 / _19 (scale BMM_Additive, bone space, <- HeadScale x HeadScale_Adj, the bound functions @924 /
//!   @1007): HeadScale is the shared step (the entry's head_scale, factor 1); HeadScale_Adj (CDO (0.95, 0.95, 0.9);
//!   nothing writes it: xref) is a constant factor here, every tick (the product of two local scale factors is the
//!   same in either order).
//! - ModifyBone_15 .. _6 (scale BMM_Additive in BCS_ComponentSpace <- Fingers_Adj, CDO (1, 1, 0.75), never written):
//!   the component-space transform's Scale3D times the factor is, under FTransform composition (no shear), the bone's
//!   local scale times it: (1, 0.75, 1) in the pose's axes.
//! - Teeth / eyes / belly (BlueprintUpdateAnimation @2085, skipped while IsWatermelonSteven):
//!   ToggleSubmeshVisibility (curve, tag, state) per sub-mesh, threshold 0.9: when curve > 0.9 and !state,
//!   SetSubMeshVisibility(tag, true); when state and curve < 0.9, SetSubMeshVisibility(tag, false); the state becomes
//!   curve > 0.9. In this order: UpperTeeth (showUpperTeeth), LowerTeeth (showLowerTeeth), EyeSpecs (showEyeSpecs,
//!   state EyeSpecs_OFF, a variable nothing writes, and the output dropped: the call shows EyeSpecs on every update
//!   its curve is over 0.9 and never hides it), EyeStars (showEyeStars), Belly (showBelly), BellyGem (showBellyGem),
//!   then Belly again with the curve showBellyGem and the state BellyVisible (@3103): the last write of the update
//!   stands. The port's own state of a part over the sim's (sub_hidden), as Bugs's: the AnimBP's write until the sim
//!   changes the part. ResetSubmeshVisibility (OnSetInitialVisibility, @3577-3632) clears UpperTeethVisible,
//!   EyeStarsVisible, LowerTeethVisible and BellyGemVisible (and EyeSpecsVisible / FullTeethVisible, which no toggle
//!   reads), not BellyVisible: at a respawn the sim's SetupInitialVisibility hides the parts and these states go.
//!   The curves: the clips' own by the sequence the tick shows (the fighter's anim and anim time) and the view's
//!   crossfade, as Bugs's.
//!
//! Deviations and unverified models:
//! - Unverified: a curve's value in a crossfade and the one tick the update lags the evaluation: as Lebron's / Bugs's
//!   pose.
//! - Unverified: the hit pause: the AnimBP's DeltaTime is not dilated here (no node of this file integrates time; the
//!   glide machine's time is the sim's).
//! - Deviation: the view places no dead fighter, so the respawn is seen as a gap in the ticks set() runs at (the
//!   effects are told of every dead tick), as Bugs's.
//! - Not ported: the Trail on fml_un_TigerMillionaire_C_tie_4 and the SpringBone on fml_un_TigerMillionaire_hair_att
//!   (their alpha float(TigerMillionaireDynamicsDisable < 0.1 && !IsRespawning && !IsHitStunned)): the bones of the
//!   TigerMillionaire skin, held by the default skeleton but carrying no part of the default skin;
//!   ModifyBone_17 / _16 / _5 .. _2 (<- Hands_Adj / Forearm_Adj, CDO (1, 1, 1), never written: identity);
//!   ModifyBone_22 / _21 / _20 (virtual bones: the view holds none).
//! - The mirror node is the view's reflection (no BoneFlipAxis None in Steven_MirrorTable: `turns` false).
//! - Deviation: BP_CharLights_Steven (the pawn's character lights rig) is not drawn: no match spawns a CharLights rig.

use std::sync::Arc;

use mvs_data::Value;
use mvs_data::fighter::FighterData;
use mvs_sim::World;
use mvs_sim::actors::world_actor::WorldActorState;
use mvs_sim::actors::{ActorId, ActorRef, Projectile};
use mvs_sim::chars::steven::barrier::StevenShield;
use mvs_sim::chars::steven::state::steven_of;
use mvs_sim::fighter::FighterId;
use mvs_sim::names::Name;
use mvs_vfx::math::{V3, compose_rows, rotation_rows};

use super::{CharRender, MinionView, PawnActor, PawnCx, PoseHook, PoseOf, PreviewOf, PreviewStep};
use crate::ActorPose;
use crate::actor_view::{SkinnedActor, StaticView, Xform, rel_rows, xyz};
use crate::actors::{ActorComponents, ActorHost, ActorMesh, ActorPlace, ComponentKind, MeshRule, Rows, class_def};
use crate::anim::{Clips, HeadScaleNode, Pose, add_weighted_pose, normalize_rotations, weight_pose};
use crate::curves::CurveTable;
use crate::frame::MeshPose;
use crate::skins::{Skin, SkinPart};
use crate::view::{CharView, PoseCx, PoseRules};

pub(crate) const RENDER: CharRender = CharRender {
    // The big head (HeadScaleStep): Steven_AnimBP ModifyBone_18 (fml_un_C_head) and ModifyBone_19
    // (fml_un_C_stvn_face_att): scale Additive, bone space, <- HeadScale x HeadScale_Adj (the bound functions @924 /
    // @1007); the constant HeadScale_Adj is the pose hook's.
    head_scale: &[
        HeadScaleNode {
            node: "ModifyBone_18",
            bone: "fml_un_C_head",
            factor: 1.0,
            gated: false,
            moves: [0.0; 3],
            scale: true,
        },
        HeadScaleNode {
            node: "ModifyBone_19",
            bone: "fml_un_C_stvn_face_att",
            factor: 1.0,
            gated: false,
            moves: [0.0; 3],
            scale: true,
        },
    ],
    pose: Some(steven_pose),
    pawn_actors: Some(pawn_props),
    minions: &[WATERMELON],
    ..CharRender::GENERIC
};

/// The float curves its AnimBP reads by name (GetCurveValue), in the order the rule indexes them.
pub(crate) const CURVES: CurveTable =
    CurveTable::new(&["showUpperTeeth", "showLowerTeeth", "showEyeSpecs", "showEyeStars", "showBelly", "showBellyGem"]);
const NC: usize = 6;

/// SlowFallState's two states.
const INTO: &str = "Steven_Nav_SlowFall_Enter";
const LOOP: &str = "Steven_Nav_SlowFall";
/// ZERO_ANIMWEIGHT_THRESH.
const ZERO_W: f64 = 1e-5;
/// ToggleSubmeshVisibility @109 / @462.
const TOGGLE: f64 = 0.9;

/// The constant scale factors, the pose's axes (x, y, z) = UE (X, Z, Y).
const MODIFY: [(&[&str], [f64; 3]); 4] = [
    // ModifyBone (1, 1.05, 1)
    (&["fml_un_C_spine_d"], [1.0, 1.0, 1.05]),
    // ModifyBone_1 (1, 0.9, 1)
    (&["fml_un_C_spine_a"], [1.0, 1.0, 0.9]),
    // HeadScale_Adj (0.95, 0.95, 0.9)
    (&["fml_un_C_head", "fml_un_C_stvn_face_att"], [0.95, 0.9, 0.95]),
    // ModifyBone_15 .. _6 <- Fingers_Adj (1, 1, 0.75)
    (
        &[
            "fml_un_L_hand_index",
            "fml_un_R_hand_index",
            "fml_un_L_hand_middle",
            "fml_un_R_hand_middle",
            "fml_un_L_hand_ring",
            "fml_un_R_hand_ring",
            "fml_un_L_hand_pinky",
            "fml_un_R_hand_pinky",
            "fml_un_L_hand_thumb",
            "fml_un_R_hand_thumb",
        ],
        [1.0, 0.75, 1.0],
    ),
];

/// The sub-meshes the update toggles (the skin's keys = their tags).
const PARTS: [&str; 6] = ["UpperTeeth", "LowerTeeth", "EyeSpecs", "EyeStars", "Belly", "BellyGem"];
const ST_UPPER: usize = 0;
const ST_LOWER: usize = 1;
const ST_STARS: usize = 2;
const ST_BELLY: usize = 3;
const ST_GEM: usize = 4;
/// The calls in the update's order: the part, its curve (CURVES' index), and the state variable it reads and writes
/// (None: EyeSpecs_OFF, false, and the output dropped).
const CALLS: [(usize, usize, Option<usize>); 7] = [
    (0, 0, Some(ST_UPPER)),
    (1, 1, Some(ST_LOWER)),
    (2, 2, None),
    (3, 3, Some(ST_STARS)),
    (4, 4, Some(ST_BELLY)),
    (5, 5, Some(ST_GEM)),
    // @3103
    (4, 5, Some(ST_BELLY)),
];

/// `stevenPose`'s state on one view.
#[derive(Debug)]
pub(crate) struct StevenPose {
    /// The view of a Watermelon minion: its parts are the Watermelon skin's, and the update's toggles never run for
    /// it (IsWatermelonSteven).
    watermelon: bool,
    /// A copy's pose (a disguise's second view of the victim: PoseOf.slaved): no update of its own.
    slaved: bool,
    /// A stand-in's view (PoseOf.stand_in): no Steven state read (no glide).
    stand_in: bool,
    /// The constant ModifyBones: bone and factor.
    modify: Vec<(usize, [f64; 3])>,
    /// Per part its index in the view's parts (None: the skin has none) and its tag in the sim's sub_hidden.
    part: [Option<usize>; 6],
    tags: [String; 6],
    /// A fighter's view this tick (set ran).
    live: bool,
    /// The last tick set() ran at (a gap: the fighter was dead between).
    last_tick: Option<i32>,
    /// The curves: the sequence's (tgt), the evaluation's (cur) and the pose the running fade started from (from).
    tgt: [f64; NC],
    cur: [f64; NC],
    from: [f64; NC],
    fades: u32,
    /// The state variables (ST_*), the AnimBP's last write of each part (-1 none, 0 hidden, 1 shown) and the sim's
    /// last visibility of it (-1 not seen).
    stored: [bool; 5],
    own: [i8; 6],
    sim_vis: [i8; 6],
    /// The glide list's two clips.
    into: Option<usize>,
    looped: Option<usize>,
    g: Pose,
    l: Pose,
    /// Child 0's weight (child 1's is 1 - gw), SlowFallState's state (0 Into, 1 Loop) and its player time.
    gw: f64,
    g_state: i32,
    g_time: f64,
    /// The pose of this tick is the list's (no montage).
    src_only: bool,
}

fn steven_pose(view: &mut CharView, clips: &Clips, of: &PoseOf<'_>) -> Box<dyn PoseHook> {
    Box::new(StevenPose::new(view, clips, of.part_keys, of.slaved, of.stand_in, false))
}

impl StevenPose {
    /// `stevenPose`; `slaved`: PoseOf::slaved (a Disguise's copy); `watermelon`: the view of a Watermelon minion.
    pub(crate) fn new(
        view: &CharView,
        clips: &Clips,
        part_keys: &[String],
        slaved: bool,
        stand_in: bool,
        watermelon: bool,
    ) -> StevenPose {
        let mut modify = Vec::new();
        for (bones, s) in MODIFY {
            for b in bones {
                match view.rig.index(b) {
                    Some(i) => modify.push((i, s)),
                    None => eprintln!("warning: Steven pose: bone missing: {b}"),
                }
            }
        }
        let part = PARTS.map(|k| {
            let i = part_keys.iter().position(|p| p == k);
            if i.is_none() && !watermelon {
                eprintln!("warning: Steven pose: the skin has no sub-mesh {k}");
            }
            i
        });
        let (into, looped) = (clips.get(INTO), clips.get(LOOP));
        for (c, n) in [(into, INTO), (looped, LOOP)] {
            if c.is_none() {
                eprintln!("warning: Steven pose: clip missing: {n} (the glide pose is not drawn)");
            }
        }
        let n = view.rest.n;
        StevenPose {
            watermelon,
            slaved,
            stand_in,
            modify,
            part,
            tags: PARTS.map(str::to_lowercase),
            live: false,
            last_tick: None,
            tgt: [0.0; NC],
            cur: [0.0; NC],
            from: [0.0; NC],
            fades: view.fade_count(),
            stored: [false; 5],
            own: [-1; 6],
            sim_vis: [-1; 6],
            into,
            looped,
            g: Pose::new(n),
            l: Pose::new(n),
            gw: 0.0,
            g_state: 0,
            g_time: 0.0,
            src_only: false,
        }
    }

    /// The state's clip (`glideClip`).
    fn glide_clip(&self) -> Option<usize> {
        if self.g_state == 1 { self.looped } else { self.into }
    }

    /// `glideTime`: the state's time on clip `c` (Into held at its end, Loop wrapped).
    fn glide_time(&self, clips: &Clips, c: usize) -> f64 {
        let len = clips.clips[c].anim.length;
        if self.g_state == 1 { if len > 0.0 { self.g_time % len } else { 0.0 } } else { self.g_time.min(len) }
    }

    /// `init`: the AnimBP's state anew.
    fn init(&mut self, fades: u32) {
        self.gw = 0.0;
        self.g_state = 0;
        self.g_time = 0.0;
        self.src_only = false;
        self.tgt = [0.0; NC];
        self.cur = [0.0; NC];
        self.from = [0.0; NC];
        self.fades = fades;
        self.stored = [false; 5];
        self.own = [-1; 6];
        self.sim_vis = [-1; 6];
    }
}

impl PoseRules for StevenPose {
    fn takes_layer(&self) -> bool {
        true
    }

    /// The list's evaluation as the source of the Slot nodes (a montage's own sample is the shared one's).
    fn sample(
        &mut self,
        cx: &mut PoseCx<'_>,
        clip: usize,
        time: f64,
        montage: Option<Name>,
        _dt: f64,
        out: &mut Pose,
        _again: bool,
    ) -> bool {
        self.src_only = montage.is_none();
        if montage.is_some() {
            return false;
        }
        let gc = if self.live { self.glide_clip() } else { None };
        let w = if gc.is_some() { self.gw } else { 0.0 };
        let Some(gc) = gc.filter(|_| w > ZERO_W) else {
            cx.sample_layer(clip, time, out);
            return true;
        };
        let t = self.glide_time(cx.clips, gc);
        cx.sample(gc, t, &mut self.g);
        if w >= 1.0 - ZERO_W {
            out.copy(&self.g);
            return true;
        }
        cx.sample_layer(clip, time, &mut self.l);
        weight_pose(&self.g, w, out);
        add_weighted_pose(&self.l, 1.0 - w, out);
        normalize_rotations(out);
        true
    }

    fn rule(&mut self, cx: &mut PoseCx<'_>, p: &mut Pose, _dt: f64) {
        // the constant ModifyBones (the header)
        for &(i, s) in &self.modify {
            let o = i * 3;
            for (k, v) in s.iter().enumerate() {
                p.s[o + k] = (f64::from(p.s[o + k]) * v) as f32;
            }
        }
        // this evaluation's curves, for the next update
        if cx.fade_count != self.fades {
            self.fades = cx.fade_count;
            self.from = self.cur;
        }
        // the list's curves: the children's by their weights
        let gc = if self.live && self.src_only { self.glide_clip() } else { None };
        if let Some(gc) = gc
            && self.gw > ZERO_W
        {
            let t = self.glide_time(cx.clips, gc);
            for k in 0..NC {
                let g = CURVES.value_of(cx.clips, Some(gc), k, t);
                self.tgt[k] = g * self.gw + self.tgt[k] * (1.0 - self.gw);
            }
        }
        let w = cx.fade_weight;
        for k in 0..NC {
            self.cur[k] = if w >= 1.0 { self.tgt[k] } else { self.from[k] + (self.tgt[k] - self.from[k]) * w };
        }
    }
}

impl StevenPose {
    /// `set(f)`: the AnimBP's update of the tick, before the view's pose().
    pub(crate) fn update(&mut self, w: &World, f: FighterId, view: &mut CharView, clips: &Clips) {
        let fi = w.fighter(f);
        self.live = !self.slaved;
        // the respawn: SetupInitialVisibility's hide of the parts and ResetSubmeshVisibility
        let tick = w.flow.tick;
        if self.last_tick.is_some_and(|t| tick > t + 1) {
            self.own = [-1; 6];
            for s in [ST_UPPER, ST_LOWER, ST_STARS, ST_GEM] {
                self.stored[s] = false;
            }
        }
        self.last_tick = Some(tick);
        if !self.live {
            return;
        }
        // the glide list after the tick's update (a stand-in: none, NO_GLIDE)
        let g = if self.stand_in { None } else { steven_of(w, f).map(|s| &s.glide) };
        self.gw = g.map_or(0.0, |g| f64::from(g.w));
        self.g_state = g.map_or(0, |g| g.sm);
        self.g_time = g.map_or(0.0, |g| f64::from(g.sm_time));
        // BlueprintUpdateAnimation on the curves of the evaluation before: the toggles (not for the Watermelon;
        // Steven_AnimBP.IsWatermelonSteven: the port's minion of a Steven is that Watermelon alone)
        if !self.watermelon && !fi.life.is_minion {
            self.toggle_calls();
            for k in 0..PARTS.len() {
                let Some(pi) = self.part[k] else { continue };
                // the AnimBP's write until the sim changes the part's visibility (the later write)
                let hidden = w.names.get(&self.tags[k]).is_some_and(|n| fi.pose.sub_hidden.contains(&n));
                let vis = i8::from(!hidden);
                if self.sim_vis[k] >= 0 && self.sim_vis[k] != vis {
                    self.own[k] = -1;
                }
                self.sim_vis[k] = vis;
                view.set_part_visible(pi, if self.own[k] >= 0 { self.own[k] == 1 } else { vis == 1 });
            }
        }
        // the sequence this tick's pose is sampled from
        CURVES.fill(clips, w.names.str(fi.anim.anim), f64::from(fi.anim.anim_time), &mut self.tgt);
    }

    /// The update's ToggleSubmeshVisibility calls (CALLS) on the curves of the evaluation before: each write into
    /// `own`.
    fn toggle_calls(&mut self) {
        for &(k, c, state) in &CALLS {
            let v = self.cur[c];
            let st = state.is_some_and(|s| self.stored[s]);
            if v > TOGGLE && !st {
                self.own[k] = 1;
            } else if st && v < TOGGLE {
                self.own[k] = 0;
            }
            if let Some(s) = state {
                self.stored[s] = v > TOGGLE;
            }
        }
    }

    /// `reset()`: a new pawn: the AnimBP's state anew.
    pub(crate) fn renew(&mut self) {
        self.init(self.fades);
        self.live = false;
        self.last_tick = None;
    }
}

impl PoseHook for StevenPose {
    /// `preview()`: a view with no fighter: the AnimBP anew; its toggles run in preview_step.
    fn preview(&mut self, _view: &mut CharView, _of: &PreviewOf<'_>) {
        self.init(self.fades);
        self.live = false;
    }

    /// The preview's update (as gizmo.rs's): on BP_CharacterPreview IsWatermelonSteven stays false (set only for a
    /// Pawn_StevenUniverse owner, @3485) and the toggles (@2119-3153) test only the owning actor's VisibilityManager
    /// (@35-220; the Fighter test @1120 ends only its own branch): the calls on the curves of the evaluation before,
    /// each write of this update put on its part (the AnimBP is the parts' only writer here), then the curves of the
    /// frame's sequence.
    fn preview_step(&mut self, view: &mut CharView, clips: &Clips, s: &PreviewStep<'_>) {
        self.own = [-1; 6];
        self.toggle_calls();
        for k in 0..PARTS.len() {
            if let Some(pi) = self.part[k]
                && self.own[k] >= 0
            {
                view.set_part_visible(pi, self.own[k] == 1);
            }
        }
        CURVES.fill(clips, s.anim, s.time, &mut self.tgt);
    }

    fn set(&mut self, w: &World, f: FighterId, view: &mut CharView, clips: &Clips) {
        self.update(w, f, view, clips);
    }

    fn reset(&mut self, _part_scalar: &mut dyn FnMut(usize, &str, f64)) {
        self.renew();
    }

    fn shown(&self, tag: &str, sim: bool) -> bool {
        match PARTS.iter().position(|p| p.eq_ignore_ascii_case(tag)) {
            Some(k) if self.part[k].is_some() && self.own[k] >= 0 => self.own[k] == 1,
            _ => sim,
        }
    }
}

// ---- his pawn's props ----
//
// Pawn_StevenUniverse's StaticMeshComponents (PinkShield, PinkShield2, PinkBubble, PinkBarrier), children of
// CharacterMesh0 in its SCS. The data's `socket` is the node's AttachToName: PinkShield and PinkBarrier on
// fml_un_C_prop1, PinkShield2 and PinkBubble on fml_un_C_prop2, no relative transform. Each is in
// Comp_Pawn_MeshManager.Meshes under its tag (the sim's static sub-meshes), hidden by SetupInitialVisibility (bVisible
// false in the template) and shown by the montages' AnimNotify_Equip: PinkShield by Air_BellyAttack, Combo3,
// DoubleSideShield, ShieldSlide, Special_(Air_)UppercutNoPlatform; PinkShield2 by DoubleSideShield; PinkBubble by
// Combo1, Combo2, BubbleEnemy_Throw, Special_BubbleStun; PinkBarrier by Special_SpikeShield. The sim keeps the state
// (sub_hidden).
// Materials: the components' meshes keep their own (Steven_PropBase_MIC: the skin's SubMeshes name none of the four,
// and no graph of the pawn sets one), keyed by the mesh in the fighter's draws.
// Unverified: the sockets are the last presented pose's.
// Deviation: no first draw of the props is made under the loading screen:
// the match warm-up makes their GPU objects instead (ViewPoser::warm_meshes, mvs-render Renderer::prewarm_match).

/// Comp_Pawn_MeshManager's role tags.
const MESH_ROLE_TAGS: [&str; 4] = ["RecieveArmor", "IgnoreArmor", "VisMesh", "PrimaryMesh"];

/// One prop: its tag in sub_hidden (lower case), its view, its socket and its relative transform.
#[derive(Debug)]
struct PawnProp {
    tag: String,
    v: StaticView,
    socket: String,
    rel: Xform,
}

impl PawnActor for PawnProp {
    fn present(&mut self, host: &ActorHost<'_>, w: &World, cx: &PawnCx<'_>, _alpha: f64, out: &mut Vec<ActorPose>) {
        // AddInvisibility hides every mesh of the manager; a dead fighter's view is not drawn
        if !cx.shown(w, &self.tag) {
            return;
        }
        let Some(at) = host.socket(w, cx.fid, &self.socket, &self.rel) else { return };
        self.v.set(&at);
        let mut meshes = Vec::new();
        self.v.draw(&mut meshes);
        cx.outline(true, &mut meshes);
        out.push(ActorPose { meshes, actor: None, ..ActorPose::default() });
    }
}

/// `pawnProps`: the pawn's static sub-meshes (the static sub-meshes: a StaticMeshComponent with a
/// Meshes tag and no UI tag), in the data's order.
fn pawn_props(host: &ActorHost<'_>, w: &World, f: FighterId) -> Vec<Box<dyn PawnActor>> {
    let mut out: Vec<Box<dyn PawnActor>> = Vec::new();
    let d = &w.char_data(f).fighter;
    let variant = host.selected_variant(w, f);
    for c in d.components.pawn.values() {
        if c.ty != "StaticMeshComponent" {
            continue;
        }
        let Some(p) = c.p.as_obj() else { continue };
        let tags: Vec<&str> = p
            .get("ComponentTags")
            .and_then(Value::as_arr)
            .map_or(Vec::new(), |t| t.iter().filter_map(Value::as_str).collect());
        let Some(tag) = tags.iter().find(|t| !MESH_ROLE_TAGS.contains(t)).map(|t| t.to_lowercase()) else {
            continue;
        };
        if tags.contains(&"UI") {
            continue;
        }
        // bHiddenInGame (PinkShield, PinkShield2, PinkBubble): Equip's SetVisibility does not show it; the one
        // SetHiddenInGame(false) is SkinManager's mobile-only path (@1808 behind @1089), so the build never draws it.
        if p.get("bHiddenInGame").and_then(Value::as_bool) == Some(true) {
            continue;
        }
        let mesh = p.get("StaticMesh").and_then(Value::as_str).filter(|m| !m.is_empty() && *m != "None");
        let Some(v) = mesh.and_then(|m| StaticView::new(host, w, f, m, variant)) else {
            eprintln!("warning: Steven: the prop {tag} is not drawn (its mesh or materials are not loaded)");
            continue;
        };
        let rel = Xform {
            loc: xyz(p.get("RelativeLocation"), 0.0),
            rot: rel_rows(p.get("RelativeRotation")),
            scale: xyz(p.get("RelativeScale3D"), 1.0),
        };
        let socket = c.socket.clone().unwrap_or_else(|| "None".to_owned());
        out.push(Box::new(PawnProp { tag, v, socket, rel }));
    }
    out
}

// ---- his classes' meshes ----
//
// Steven_ShieldActor_C / Steven_ShieldActor_Up_C (World actors): SkeletalMesh (Steven_SeparateShield_SkelMesh, scale
// 2, on the Capsule root; yaw 30, the Up child pitch 90), AnimationSingleNode. BeginPlay @316
// PlayAnimation(Steven_Shield_Enter, no loop); with PlayIdleAnim (CDO true, the Up child false) a 0.35 s timer @395 ->
// PlayAnimation(Steven_Shield_Idle, loop) @4004. SetTeamColor @2484: SetMaterial(1, IsLocalOnline ?
// StevenShield_MESH_MAT_Inst_Local : ColorValue 0 Red / 1 Blue): the owner's skin variant (selected_variant) on the
// mesh's entry of the fighter's draws, whose variants are that slot's. The actor's yaw: FacingLeft ? 90 : -90.
// Unverified: Comp_Spawner's spawn rotation is taken as Harley's box takes it: the shield's own facing.
// The hit reactions: OnHitVisuals @3181-3238 PlayAnimation(FighterHit ? Steven_Shield_HR_Heavy : _Light, no loop),
// with PlayIdleAnim the AfterHitAnim 0.53 s timer @526 -> Idle @4051 (the sim's hit_tick / hit_heavy). Not drawn
// here: ReceiveEndPlay's Exit animation @3781 (the actor is gone). PS_Impact and the outro are the effects'.
// Steven_ShieldActor_U_SpecialPlatform_C, StevenCollisionBlock_C: no visual component (nothing to draw).
// HealGrenade_C (a projectile): its ParticleSystem is the effects' shared component; its rotation is the rule's: the
// Capsule's bLockX/Y/ZRotation keep the spawn's (the side it was thrown to), where the shared rotation would turn
// with the body's velocity (yaw -90 facing +x). Unverified: Comp_Spawner's spawn rotation is taken by the facing, as
// the shield actor's.
// Deviation: the shield's first draw under the loading screen (stevenActorWarm) is not drawn:
// the match warm-up makes their GPU objects instead (ViewPoser::warm_meshes, mvs-render Renderer::prewarm_match).

const SHIELD_MESH: &str = "Steven_SeparateShield_SkelMesh";
const ENTER: &str = "Steven_Shield_Enter";
const IDLE: &str = "Steven_Shield_Idle";
const HR_LIGHT: &str = "Steven_Shield_HR_Light";
const HR_HEAVY: &str = "Steven_Shield_HR_Heavy";
const STEP: f64 = 1.0 / 60.0;
/// AfterEnterAnim's P: the shield steps in its spawn frame (age 1), where BeginPlay sets the timer (0.35 s @395);
/// it fires 21 frames later (timerTicks(0.35), after the tick groups) and the Idle is first drawn the frame after,
/// at 1/60.
const IDLE_ENTER_AGE: i32 = 1 + 21;
/// AfterHitAnim's 0.53 s timer @526: it fires 32 frames after the hit's frame (strict >, counted from the next
/// frame), the Idle at 0 there and first drawn the frame after at 1/60.
const IDLE_AFTER_HIT: i32 = 32;

/// The classes' own mesh drawing, by class name.
pub(crate) const RULES: &[(&str, MeshRule)] = &[
    ("Steven_ShieldActor_C", MeshRule { rot: None, mesh: Some(shield_mesh) }),
    ("Steven_ShieldActor_Up_C", MeshRule { rot: None, mesh: Some(shield_mesh) }),
    ("HealGrenade_C", MeshRule { rot: Some(grenade_rot), mesh: None }),
];

/// HealGrenade_C's rotation by the side it was thrown to (yaw -90 facing +x, as actorRot).
fn grenade_rot(_w: &World, pr: &Projectile) -> Option<Rows> {
    let h = std::f64::consts::FRAC_PI_2;
    Some(rotation_rows(0.0, if pr.facing > 0.0 { -h } else { h }, 0.0))
}

/// The shield's state and its root's scale (the sim's size_scale).
fn shield_of(w: &World, id: ActorId) -> Option<(&StevenShield, f64)> {
    let a = w.actors.actors.get(id)?;
    match &a.state {
        WorldActorState::StevenShield(s) if !a.dead => Some((s, f64::from(a.size_scale.unwrap_or(1.0)))),
        _ => None,
    }
}

/// `ShieldView`: the mesh and its clips (`idle` None: the class plays no idle, PlayIdleAnim false).
#[derive(Debug)]
struct ShieldView {
    id: ActorId,
    v: SkinnedActor,
    from: FighterId,
    enter: Option<usize>,
    idle: Option<usize>,
    hr_light: Option<usize>,
    hr_heavy: Option<usize>,
    /// The mesh's world rotation (the facing's, set once) and its RelativeScale3D.
    rot: Rows,
    base: V3,
}

fn shield_mesh(host: &mut ActorHost<'_>, w: &World, a: ActorRef, cs: &ActorComponents) -> Option<Box<dyn ActorMesh>> {
    let ActorRef::Actor(id) = a else { return None };
    let x = w.actors.actors.get(id)?;
    let (s, _) = shield_of(w, id)?;
    let from = mvs_sim::fighter::common::actor_def_owner(w, x.owner, x.cls);
    let c = cs.get("SkeletalMesh");
    let variant = host.selected_variant(w, x.owner);
    let d = host.draws_of(w, from)?;
    let (path, keys) = d.actor(SHIELD_MESH, (variant != "local").then_some(variant))?;
    let info = host.mesh(&path)?;
    let v = SkinnedActor::new(info, &keys);
    let play_idle = class_def(w, a).and_then(|d| d.p.get("PlayIdleAnim")).and_then(Value::as_bool) == Some(true);
    let clips = host.clips_of(w, from)?;
    let enter = clips.get(ENTER);
    if enter.is_none() {
        eprintln!("warning: Steven: clip missing: {ENTER}");
    }
    let idle = if play_idle { clips.get(IDLE) } else { None };
    let (hr_light, hr_heavy) = (clips.get(HR_LIGHT), clips.get(HR_HEAVY));
    if hr_light.is_none() || hr_heavy.is_none() {
        eprintln!("warning: Steven: clip missing: {}", if hr_light.is_some() { HR_HEAVY } else { HR_LIGHT });
    }
    let h = std::f64::consts::FRAC_PI_2;
    let yaw = rotation_rows(0.0, if s.facing < 0.0 { h } else { -h }, 0.0);
    let (rel, base) = match c {
        Some(c) if matches!(c.kind, ComponentKind::SkeletalMesh { .. }) => (c.rot, c.scale),
        _ => (crate::actor_view::IDENT, [1.0; 3]),
    };
    Some(Box::new(ShieldView { id, v, from, enter, idle, hr_light, hr_heavy, rot: compose_rows(&rel, &yaw), base }))
}

impl ActorMesh for ShieldView {
    /// `placeShield`: the clip the mesh's AnimationSingleNode plays: whichever PlayAnimation came last, at (F - P) /
    /// 60 with F the drawn frame and P the frame its clip sits at time 0 (every call restarts at 0 and snaps, a
    /// non-looping clip holds its last key). Enter at the spawn; Idle after AfterEnterAnim (IDLE_ENTER_AGE); the
    /// latest hit's HR, then with PlayIdleAnim Idle again at that hit's frame + 32 (the _Up child: its HR holds). A
    /// hit's frame in the shield's age as the view reads it there: age - (tick - hit_tick).
    // Unverified: the hit's PlayAnimation runs in tick group 0 in the attacker's tick or the projectile's move,
    // unordered against the shield mesh's own tick; the port takes the mesh as ticking after it (P = the hit's frame -
    // 1: HR at 1/60 in the hit's own frame), so a hit in the same frame as AfterEnterAnim's timer (which fires after
    // the tick groups) is cut to Idle by it, and a hit the frame after wins.
    fn place(
        &mut self,
        host: &mut ActorHost<'_>,
        w: &World,
        _at: &ActorPlace,
        alpha: f64,
        shown: bool,
        out: &mut Vec<MeshPose>,
    ) {
        let Some((s, k)) = shield_of(w, self.id) else { return };
        self.v.ticked(w.flow.tick);
        let scale = self.base.map(|v| v * k);
        self.v.set(&Xform { loc: [0.0, f64::from(-s.x), f64::from(s.y)], rot: self.rot, scale });
        if let Some(clips) = host.clips_of(w, self.from) {
            let age = s.age;
            let idle_enter = self.idle.is_some() && age > IDLE_ENTER_AGE;
            let mut play = None;
            if let Some(ht) = s.hit_tick {
                // the hit's frame
                let h = age - (w.flow.tick - ht);
                let hr = if s.hit_heavy == Some(true) { self.hr_heavy } else { self.hr_light };
                if let Some(idle) = self.idle.filter(|_| age > h + IDLE_AFTER_HIT) {
                    play = Some((idle, f64::from(age - h - IDLE_AFTER_HIT) * STEP, true));
                } else if let Some(hr) = hr.filter(|_| !(idle_enter && h - 1 < IDLE_ENTER_AGE)) {
                    // HR's P (h - 1) against AfterEnterAnim's (IDLE_ENTER_AGE): the later one plays; a hit in the
                    // timer's own frame is cut to Idle
                    play = Some((hr, f64::from(age - h + 1) * STEP, false));
                }
            }
            if play.is_none() {
                play = match (idle_enter, self.idle, self.enter) {
                    (true, Some(i), _) => Some((i, f64::from(age - IDLE_ENTER_AGE) * STEP, true)),
                    (_, _, Some(e)) => Some((e, f64::from(age) * STEP, false)),
                    _ => None,
                };
            }
            if let Some((c, t, looped)) = play {
                self.v.play(clips, c, t, looped);
            }
        }
        self.v.present(alpha);
        self.v.visible = shown;
        self.v.draw(out);
    }

    fn bones(&self, out: &mut Vec<(Arc<str>, Xform)>) {
        self.v.bones_world(out);
    }
}

// ---- the Watermelon minion's view ----
//
// A Fighter of World.minions whose minion_creator is a Steven (Pawn_StevenUniverse's Watermelon set-up, behind
// isWatermelonSteven), drawn by its Steven's module: one view per Steven, made with the match, shown while that
// Steven's minion lives (pose.rs place_minion).
//   - @5714 Comp_Pawn_SkinManager.ForcedColorIndex = the creator's GetColorIndex; GetColorIndex returns it with
//     IsLocalOnlinePlayer false: the skin's Red set for 0, Blue for 1, never the local one.
//   - @5909 ApplySkinToPawn(WatermelonSteven, true): the main mesh WatermelonSteven_SkelMesh with the skin's materials.
//     Its SubMeshes Mouth1 / Mouth2 are keyed by those names, and ApplySubMeshes looks a key up in
//     Comp_Pawn_MeshManager.Meshes, keyed by the component's tag (Mouth): both lookups fail, and the Mouth component
//     keeps its SCS mesh WatermelonSteven_Mouth1_SkelMesh, whose slot material is the Mouth1 entry's.
//   - @6018 SetStencilValue(switch ForcedColorIndex: 0 EnemyOutline, 1 Ally Outline).
//   - @6115 Mouth.SetVisibility(true, false): the Mouth shown, by the component.
//   - Mesh.SetWorldScale3D(0.85) @4987: the view's mesh scale; the mesh's Z is Steven's.
// Pose: Steven_AnimBP on the Watermelon's main mesh (StevenPose with watermelon: no toggles), the montage blend as a
// pawn's view has it.
// Unverified: the view shows the Mouth for the minion's life (no SetSubMeshVisibility on the tag Mouth is read); the
// Steven skin's own sub-mesh components (teeth, eyes, belly) are not drawn on it.
// Deviation: AddInvisibility hides the whole view (the AlwaysVisibleMeshes split is not taken).
// Not ported: the first draw under the loading screen (drawEmptyOnce).

const WATERMELON_SKIN: &str = "WatermelonSteven";
const WATERMELON_MESH: &str = "WatermelonSteven_SkelMesh";
const MOUTH_MESH: &str = "WatermelonSteven_Mouth1_SkelMesh";
/// The skin's SubMeshes entry of the Mouth component's mesh (its materials).
const MOUTH_KEY: &str = "Mouth1";

const WATERMELON: MinionView = MinionView {
    key: mvs_sim::chars::steven::KEY,
    skin: watermelon_skin,
    // WATERMELON_MESH_SCALE: F(0.85)
    mesh_scale: Some(0.85_f32 as f64),
    local_set: false,
    renew: false,
    // Pawn_StevenUniverse's HeadScale event (@8477-8594): the Watermelon pawn's Steven_AnimBP.HeadScale = the event's,
    // which BaseFighter raises for the minion as for any fighter (ModifyBone_18 / _19 then scale its head)
    head_scale: true,
    pose: Some(watermelon_pose),
    nav: None,
    place: None,
};

/// A mesh's path among the character's (any case).
fn mesh_path(d: &FighterData, name: &str) -> crate::Result<String> {
    d.paths
        .meshes
        .get(name)
        .or_else(|| d.paths.meshes.iter().find(|(k, _)| k.eq_ignore_ascii_case(name)).map(|(_, v)| v))
        .cloned()
        .ok_or_else(|| crate::Error::Format {
            what: "Steven's Watermelon".to_owned(),
            why: format!("no mesh {name} among the character's paths"),
        })
}

/// The Watermelon's look: its main mesh and the Mouth component (shown, no sim tag) with the Mouth1 materials.
fn watermelon_skin(d: &FighterData) -> crate::Result<Skin> {
    let mouth = mesh_path(d, MOUTH_MESH)?;
    Ok(Skin {
        name: WATERMELON_SKIN.to_owned(),
        slug: String::new(),
        mesh: mesh_path(d, WATERMELON_MESH)?,
        parts: vec![SkinPart { key: MOUTH_KEY.to_owned(), mesh: MOUTH_MESH.to_owned(), path: Some(mouth), own: false }],
        part_tags: vec![None],
        part_init: vec![true],
        part_armor: vec![None],
        intro: None,
    })
}

fn watermelon_pose(view: &mut CharView, clips: &Clips, of: &PoseOf<'_>) -> Box<dyn PoseHook> {
    Box::new(StevenPose::new(view, clips, of.part_keys, of.slaved, of.stand_in, true))
}
