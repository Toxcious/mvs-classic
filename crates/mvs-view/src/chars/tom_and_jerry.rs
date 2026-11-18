//! Tom and Jerry's render module: the entry, the AnimBP's pose hook,
//! the Jerry actor's view, the trap's view and the classes' mesh rules.
//!
//! The pose beside the shared one (Tom_and_Jerry_AnimBP). set() is the AnimBP's update of the tick (after
//! the parts' visibility from the sim's sub_hidden), the rule runs on the final local pose.
//!
//! The slot nodes are the sim's (mvs-sim chars/tom_and_jerry/aim.rs): the slot graph walk writes the track it weighs
//! most as the fighter's anim and the others as the slot mix the shared view mixes in: the four SlingShot aim slots
//! by AimDirection and the mirror override, GroundMoving / GroundStill by Moving, FallingBlend / OnGround by
//! IsFalling. set() checks once that the data's graph has those nodes. The LayeredBoneBlend's two control slots
//! ControlL / ControlR under BlendListByBool_1 are this module's (the rocket arms below). Not listed: the
//! AdditiveSlot (not mixed: aim.rs).
//!
//! The rocket arms: AnimGraphNode_LayeredBoneBlend under Slot OverrideSlot (the class data: BasePose the aim tree,
//! BlendPoses[0] BlendListByBool_1, LayerSetup[0].BranchFilters tom_un_L_clav / tom_un_R_clav with BlendDepth 0,
//! BlendWeights [1] unbound, bMeshSpaceRotationBlend / bMeshSpaceScaleBlend false): the two clavicles and every bone
//! under them take the layer whole, in local space. The layer is BlendListByBool_1 (bActiveValue <-
//! RocketControlBool, BPI_SetControlBool @16129 from JerryRocket's SetVelocityMulticast; BlendTime [0.1, 0.1]) over
//! Slot ControlL (true) / Slot ControlR (false), each over the aim tree (their Source). The list's FAlphaBlend is the
//! sim's (mvs-sim chars/tom_and_jerry/arms.rs, which lays the same layer over the shapes): its child 0 weight is read
//! here. A slot's pose is its montage track over the aim tree by the montage's weight; a slot with no track passes
//! the aim tree on. Only Tom_and_Jerry_Rocket_Ground_Montage has ControlL / ControlR tracks.
//!
//! The sub-mesh toggles: SetSubmeshVisibility @16149-17232 (after BlueprintUpdateAnimation, each update):
//! - (a preview actor runs the same update: `preview_step`)
//! - ToggleSubmeshVisibility(showTomEyelids, TomEyelids, TomEyelidsVisible); on a change TomEyelashes = NOT the new
//!   state @16416.
//! - WithJerry OR IsCharacterViewer: ToggleSubmeshVisibility(showJerryEyelids, JerryEyelids, JerryEyelidsVisible); on
//!   a change JerryEyelashes = NOT the new state @16815; then JerryEyelashesVisible = NOT JerryEyelidsVisible @943-972
//!   (a variable no node reads). Else SetSubMeshVisibility(JerryEyelids, false) and (JerryEyelashes, false)
//!   @16903-17045, every update, the stored state left as it was.
//! - ToggleSubmeshVisibility(showTomTeeth, TomTeeth, TomTeethVisible) @17102.
//! - ToggleSubmeshVisibility (the class's own, @0-530, behind IsValid(VisibilityManager)): shown when the curve is
//!   above 0.9 and the state false, hidden when the state is true and the curve below 0.9; the state is (curve > 0.9).
//!   WithJerry = BPI_IsJerryVisible @1614 = the pawn's JerryMesh.bVisible (Pawn_TomAndJerry @43): the JerryMesh part as
//!   drawn (below). ResetSubmeshVisibility @17258 (the four states false) at the respawn, with the own state.
//! - The port's own state of a part over the sim's (sub_hidden) until the sim changes the part, as Bugs's. The curves:
//!   the clips' own by the sequence the tick shows and the view's crossfade.
//!
//! The graph's bone controls after the layers (ApplyAdditive, LocalToComponent), in order:
//! - ModifyBone_1 (tom_un_C_Tom_face_att) / ModifyBone_2 (tom_un_C_head): scale BMM_Additive in bone space <-
//!   HeadScale, alpha 1: the entry's head_scale.
//! - ModifyBone_6 (jerry_un_COM): translation BMM_Replace in component space (0, 0, 0), then ModifyBone_7
//!   (jerry_un_COM): translation BMM_Additive in component space (0, 100, 0); both with the alpha SelectFloat(0,
//!   JerryWallOffset, IgnoreJerryWallOffset) (@7809 / @7341), JerryWallOffset = FMax(TurnAlpha, WallAlpha)
//!   @1295-1341 (the sim's TomAndJerryAnim). Applied here (wall_offset) on the right-facing pose the view reflects
//!   (the nodes sit before WAnimationMirrorGraphNode).
//! - ModifyBone_3 / _4 / _5 (VB bones): not ported (the view holds no virtual bones).
//! - The AnimDynamics_1-14, SpringBone 0-13, Trail 0-6 and Constraint / Constraint_1 nodes: not ported: they drive the
//!   vampireCape, WildWest, BakerStreet and Pirates chains, and no vertex of the default skin's meshes is weighted to a
//!   bone of those chains (checked on the glbs).
//! - ModifyBone #22 (jerry_un_COM, translation Additive in component space <- COM_Override_Location from the linked
//!   Base_AnimBP_Tom_and_Jerry @15511): no-op here: only the car actors (BPI_COM_Carried_Override, C023A) write it.
//! - ComponentToLocal, then WAnimationMirrorGraphNode (FacingLeft, Tom_and_Jerry_MirrorTable): the view's reflection.
//!
//! The WithJerry layer and the No_Jerry idles (BlendListByBool_2 on WithJerry) are the sim's (the fighter's anim
//! through the character's anim set): the view draws its sequence.
//!
//! The attached Jerry (the JerryMesh sub-mesh, PrimaryMesh, master-posed by the shared view):
//! SetJerryAttachedMulticast @6465-6484 SetSubMeshVisibility(JerryMesh, JerryAttached, true). The sim's sub-mesh
//! state is taken, and the part is hidden as well while TomAndJerryState::jerry_attached is false, never shown over
//! the sim's.
//!
//! Unverified: a curve's value in a crossfade and the one tick the update lags the evaluation (as Bugs's); the
//! component-space products of a non-uniform parent scale in the wall offset (the FTransform rule); the montage
//! weight standing for the control slots' (GetSlotWeight), as the view's own blend.

use std::sync::atomic::{AtomicBool, Ordering};

use mvs_data::fighter::SlotGraphNode;
use mvs_sim::World;
use mvs_sim::chars::tom_and_jerry::arms::{ARMS_SLOTS, arms_node};
use mvs_sim::chars::tom_and_jerry::state::tj_of;
use mvs_sim::fighter::FighterId;
use mvs_sim::fighter::anim::MontageSlot;
use mvs_sim::fighter::anim::montages::{instance_data, slot_uid};

use super::{CharRender, PoseHook, PoseOf, PreviewOf, PreviewStep};
use crate::actor_anim::{BranchFilter, branch_mask};
use crate::actors::MeshRule;
use crate::anim::{Clips, HeadScaleNode, Pose, blend_pose};
use crate::curves::CurveTable;
use crate::view::{CharView, PoseCx, PoseRules};

pub(crate) const RENDER: CharRender = CharRender {
    // Tom_and_Jerry_AnimBP: ModifyBone_2 and ModifyBone_1 (scale Additive, bone space, <- HeadScale)
    head_scale: &[
        HeadScaleNode {
            node: "ModifyBone_2",
            bone: "tom_un_C_head",
            factor: 1.0,
            gated: false,
            moves: [0.0; 3],
            scale: true,
        },
        HeadScaleNode {
            node: "ModifyBone_1",
            bone: "tom_un_C_Tom_face_att",
            factor: 1.0,
            gated: false,
            moves: [0.0; 3],
            scale: true,
        },
    ],
    pose: Some(tj_pose),
    ..CharRender::GENERIC
};

// ---- the pose ----

/// The slot nodes of the sim's graph walk, checked once in the data's graph.
const SLOT_NODES: [&str; 11] = [
    "OverrideSlot",
    "SlingShotAim",
    "SlingShotAimBack",
    "SlingShotAimUp",
    "SlingShotAimDown",
    "GroundMovingSlot",
    "GroundStillSlot",
    "FallingBlend",
    "OnGround",
    "DefaultSlot",
    "JumpSlot",
];
static GRAPH_CHECKED: AtomicBool = AtomicBool::new(false);
/// A control slot's clip not loaded: warned once.
static CLIP_WARNED: AtomicBool = AtomicBool::new(false);

/// AnimGraphNode_LayeredBoneBlend's LayerSetup[0] (the class data).
const ARMS: [BranchFilter; 2] =
    [BranchFilter { bone: "tom_un_L_clav", depth: 0 }, BranchFilter { bone: "tom_un_R_clav", depth: 0 }];
/// ZERO_ANIMWEIGHT_THRESH, and the weight a blend takes whole.
const ZERO_W: f64 = 1e-5;
const FULL_W: f64 = 0.99999;

/// The float curves its AnimBP reads by name (GetCurveValue), in the order the rule indexes them.
const CURVES: CurveTable = CurveTable::new(&["showTomEyelids", "showJerryEyelids", "showTomTeeth"]);
const C_TOM_LIDS: usize = 0;
const C_JERRY_LIDS: usize = 1;
const C_TEETH: usize = 2;
/// ToggleSubmeshVisibility @109 / @383.
const TOGGLE: f64 = 0.9;

const COM: &str = "jerry_un_COM";
/// ModifyBone_7's Translation Y (UE cm): the pose's z, in metres / 100.
const WALL_PUSH: f64 = 100.0;
/// The parts the AnimBP writes, by sub-mesh key (the tags are the keys).
const PARTS: [&str; 5] = ["TomEyelids", "TomEyelashes", "JerryEyelids", "JerryEyelashes", "TomTeeth"];
const P_TOM_LIDS: usize = 0;
const P_TOM_LASH: usize = 1;
const P_JERRY_LIDS: usize = 2;
const P_JERRY_LASH: usize = 3;
const P_TEETH: usize = 4;
/// The attached Jerry's part.
const JERRY_PART: &str = "JerryMesh";
/// [`JERRY_PART`] as the sim's sub-mesh tag (lower case).
const JERRY_PART_LC: &str = "jerrymesh";

/// `tjPose`'s state on one view.
#[derive(Debug)]
struct TjPose {
    /// PoseOf::slaved: no AnimBP of its own (set stops after the arms).
    slaved: bool,
    /// PoseOf::slaved or stand_in: no Tom and Jerry state read.
    no_state: bool,
    com: Option<usize>,
    /// The COM's ancestors, the top first.
    chain: Vec<usize>,
    part: [Option<usize>; 5],
    tags: [String; 5],
    jerry_part: Option<usize>,
    /// The last tick set() ran at (a gap: the fighter was dead between).
    last_tick: Option<i32>,
    /// ModifyBone_6 / _7's alpha of this update.
    alpha: f64,
    tgt: [f64; 3],
    cur: [f64; 3],
    from: [f64; 3],
    fades: u32,
    /// TomEyelidsVisible, JerryEyelidsVisible, TomTeethVisible.
    tom_lids: bool,
    jerry_lids: bool,
    teeth: bool,
    /// The AnimBP's last write (-1 none, 0 hidden, 1 shown) and the sim's last visibility (-1 not seen).
    own: [i8; 5],
    sim_vis: [i8; 5],
    /// The arms: the mask, the slots' poses, this update's tracks (clip, time), the montage's weight, the list's
    /// child 0 weight, and whether either slot has a track.
    mask: Vec<f32>,
    lp: Pose,
    rp: Pose,
    tmp: Pose,
    arm_clip: [Option<usize>; 2],
    arm_time: [f64; 2],
    arm_w: f64,
    arm_l: f64,
    arms_on: bool,
    /// The data's graph has BlendListByBool_1 over the two control slots (sim/chars arms.rs arms_list).
    has_list: Option<bool>,
    /// The preview's first step is seeded with its own frame's curves (`preview_step`).
    preview_first: bool,
}

fn tj_pose(view: &mut CharView, _clips: &Clips, of: &PoseOf<'_>) -> Box<dyn PoseHook> {
    let com = view.rig.index(COM);
    if com.is_none() {
        eprintln!("warning: Tom and Jerry pose: bone missing: {COM}");
    }
    let mut chain = Vec::new();
    if let Some(c) = com {
        let mut p = view.parents[c];
        while p >= 0 {
            chain.insert(0, p as usize);
            p = view.parents[p as usize];
        }
    }
    let part = PARTS.map(|k| {
        let i = of.part_keys.iter().position(|p| p == k);
        if i.is_none() {
            eprintln!("warning: Tom and Jerry pose: the skin has no sub-mesh {k}");
        }
        i
    });
    let jerry_part = of.part_keys.iter().position(|p| p == JERRY_PART);
    if jerry_part.is_none() {
        eprintln!("warning: Tom and Jerry pose: the skin has no sub-mesh {JERRY_PART}");
    }
    let n = view.rest.n;
    Box::new(TjPose {
        slaved: of.slaved,
        no_state: of.slaved || of.stand_in,
        com,
        chain,
        part,
        tags: PARTS.map(str::to_lowercase),
        jerry_part,
        last_tick: None,
        alpha: 0.0,
        tgt: [0.0; 3],
        cur: [0.0; 3],
        from: [0.0; 3],
        fades: 0,
        tom_lids: false,
        jerry_lids: false,
        teeth: false,
        own: [-1; 5],
        sim_vis: [-1; 5],
        mask: branch_mask(&view.rig.names, &view.parents, &ARMS),
        lp: Pose::new(n),
        rp: Pose::new(n),
        tmp: Pose::new(n),
        arm_clip: [None; 2],
        arm_time: [0.0; 2],
        arm_w: 0.0,
        arm_l: 0.0,
        arms_on: false,
        has_list: None,
        preview_first: true,
    })
}

/// `rotate`: v by the unit quaternion q.
fn rotate(q: [f64; 4], x: f64, y: f64, z: f64) -> [f64; 3] {
    let [qx, qy, qz, qw] = q;
    let (cx, cy, cz) = (qy * z - qz * y, qz * x - qx * z, qx * y - qy * x);
    [
        x + 2.0 * (qw * cx + qy * cz - qz * cy),
        y + 2.0 * (qw * cy + qz * cx - qx * cz),
        z + 2.0 * (qw * cz + qx * cy - qy * cx),
    ]
}

impl TjPose {
    /// ModifyBone_6 / _7 on jerry_un_COM: the parent's component-space transform (FTransform composition down the
    /// chain: t = tp + Rp (Sp t), R = Rp R, S = Sp S), the bone's place to (0, 0, 0) by alpha, then (0, 100, 0) UE
    /// (the pose's z) by alpha, back to the bone's local space.
    fn wall_offset(&self, com: usize, p: &mut Pose) {
        let g = |v: f32| f64::from(v);
        let mut pt = [0.0f64; 3];
        let mut pq = [0.0, 0.0, 0.0, 1.0f64];
        let mut ps = [1.0f64; 3];
        for &b in &self.chain {
            let (o3, o4) = (b * 3, b * 4);
            let v = rotate(pq, ps[0] * g(p.t[o3]), ps[1] * g(p.t[o3 + 1]), ps[2] * g(p.t[o3 + 2]));
            for k in 0..3 {
                pt[k] += v[k];
            }
            let [bx, by, bz, bw] = [g(p.r[o4]), g(p.r[o4 + 1]), g(p.r[o4 + 2]), g(p.r[o4 + 3])];
            let [ax, ay, az, aw] = pq;
            pq = [
                aw * bx + ax * bw + ay * bz - az * by,
                aw * by - ax * bz + ay * bw + az * bx,
                aw * bz + ax * by - ay * bx + az * bw,
                aw * bw - ax * bx - ay * by - az * bz,
            ];
            for (k, v) in ps.iter_mut().enumerate() {
                *v *= g(p.s[o3 + k]);
            }
        }
        let o3 = com * 3;
        let v = rotate(pq, ps[0] * g(p.t[o3]), ps[1] * g(p.t[o3 + 1]), ps[2] * g(p.t[o3 + 2]));
        let k = 1.0 - self.alpha;
        let c = [(pt[0] + v[0]) * k, (pt[1] + v[1]) * k, (pt[2] + v[2]) * k + (self.alpha * WALL_PUSH) / 100.0];
        // Sp^-1 Rp^-1 (c - tp)
        let inv = [-pq[0], -pq[1], -pq[2], pq[3]];
        let v = rotate(inv, c[0] - pt[0], c[1] - pt[1], c[2] - pt[2]);
        for k in 0..3 {
            p.t[o3 + k] = if ps[k] != 0.0 { (v[k] / ps[k]) as f32 } else { 0.0 };
        }
    }

    /// A control slot's pose: its track over the aim tree (`p`) by the montage's weight, or `p`.
    fn arm_pose(&mut self, cx: &mut PoseCx<'_>, k: usize, p: &Pose, left: bool) {
        let Some(c) = self.arm_clip[k] else {
            if left {
                self.lp.copy(p)
            } else {
                self.rp.copy(p)
            }
            return;
        };
        cx.sample(c, self.arm_time[k], &mut self.tmp);
        let out = if left { &mut self.lp } else { &mut self.rp };
        if self.arm_w < FULL_W {
            blend_pose(p, &self.tmp, self.arm_w, out);
        } else {
            out.copy(&self.tmp);
        }
    }

    /// The LayeredBoneBlend over BlendListByBool_1 (ControlL / ControlR), in local space.
    fn rocket_arms(&mut self, cx: &mut PoseCx<'_>, p: &mut Pose) {
        let wl = self.arm_l;
        if wl > ZERO_W {
            self.arm_pose(cx, 0, p, true);
        }
        if wl < FULL_W {
            self.arm_pose(cx, 1, p, false);
            if wl > ZERO_W {
                self.tmp.copy(&self.lp);
                blend_pose(&self.rp, &self.tmp, wl, &mut self.lp);
            } else {
                self.lp.copy(&self.rp);
            }
        }
        // the layer at BlendWeights [1]: x = the bone's mask weight (LayeredBlend.local)
        for (b, &m) in self.mask.iter().enumerate() {
            let x = f64::from(m).min(1.0);
            if !(x > ZERO_W) {
                continue;
            }
            if x >= FULL_W {
                p.copy_bone(&self.lp, b);
            } else {
                self.tmp.copy(p);
                blend_bone(&self.tmp, &self.lp, b, x, p);
            }
        }
    }
}

/// LayeredBlend.local's blend of one bone between base and blend at x (the mask of this layer is 0 or 1: unused).
fn blend_bone(base: &Pose, blend: &Pose, b: usize, x: f64, out: &mut Pose) {
    let g = |v: f32| f64::from(v);
    for j in b * 3..b * 3 + 3 {
        out.t[j] = (g(base.t[j]) + (g(blend.t[j]) - g(base.t[j])) * x) as f32;
        out.s[j] = (g(base.s[j]) + (g(blend.s[j]) - g(base.s[j])) * x) as f32;
    }
    let r = b * 4;
    let dot = (0..4).map(|c| g(base.r[r + c]) * g(blend.r[r + c])).sum::<f64>();
    let s = if dot < 0.0 { -x } else { x };
    let q: Vec<f64> = (0..4).map(|c| g(base.r[r + c]) * (1.0 - x) + g(blend.r[r + c]) * s).collect();
    let l = q.iter().map(|v| v * v).sum::<f64>().sqrt();
    let l = if l == 0.0 || l.is_nan() { 1.0 } else { l };
    for (o, v) in out.r[r..r + 4].iter_mut().zip(q) {
        *o = (v / l) as f32;
    }
}

impl PoseRules for TjPose {
    fn rule(&mut self, cx: &mut PoseCx<'_>, p: &mut Pose, _dt: f64) {
        if self.arms_on {
            self.rocket_arms(cx, p);
        }
        if let Some(com) = self.com
            && self.alpha > 0.0
        {
            self.wall_offset(com, p);
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

impl TjPose {
    /// SetSubMeshVisibility(part n, on) of the AnimBP.
    fn write(&mut self, n: usize, on: bool) {
        if self.part[n].is_some() {
            self.own[n] = i8::from(on);
        }
    }

    /// ToggleSubmeshVisibility: the new state (curve > 0.9), and the part's write on a change.
    fn toggle(&mut self, n: usize, curve: usize, state: bool) -> bool {
        let val = self.cur[curve];
        if val > TOGGLE && !state {
            self.write(n, true);
        } else if state && val < TOGGLE {
            self.write(n, false);
        }
        val > TOGGLE
    }

    /// SetSubmeshVisibility on the curves of the evaluation before (`with_jerry`: WithJerry OR IsCharacterViewer).
    fn toggles(&mut self, with_jerry: bool) {
        let t0 = self.toggle(P_TOM_LIDS, C_TOM_LIDS, self.tom_lids);
        if t0 != self.tom_lids {
            self.write(P_TOM_LASH, !t0);
        }
        self.tom_lids = t0;
        if with_jerry {
            let j0 = self.toggle(P_JERRY_LIDS, C_JERRY_LIDS, self.jerry_lids);
            if j0 != self.jerry_lids {
                self.write(P_JERRY_LASH, !j0);
            }
            self.jerry_lids = j0;
        } else {
            self.write(P_JERRY_LIDS, false);
            self.write(P_JERRY_LASH, false);
        }
        self.teeth = self.toggle(P_TEETH, C_TEETH, self.teeth);
    }

    /// The AnimBP's state anew (`init`).
    fn init(&mut self) {
        self.alpha = 0.0;
        self.tgt = [0.0; 3];
        self.cur = [0.0; 3];
        self.from = [0.0; 3];
        self.tom_lids = false;
        self.jerry_lids = false;
        self.teeth = false;
        self.own = [-1; 5];
        self.sim_vis = [-1; 5];
        self.arms_on = false;
        self.last_tick = None;
    }
}

/// `armsList`'s graph test (mvs-sim arms.rs arms_node).
fn arms_list_in(w: &World, f: FighterId) -> bool {
    arms_node(&w.char_data(f).fighter.slot_holds.graph).is_some()
}

impl PoseHook for TjPose {
    /// `preview()`: a view with no fighter (BP_CharacterPreview): the AnimBP's state anew, run by `preview_step`.
    fn preview(&mut self, _view: &mut CharView, _of: &PreviewOf<'_>) {
        self.init();
        self.arms_on = false;
        self.preview_first = true;
    }

    /// The preview's AnimBP update (Tom_and_Jerry_AnimBP @15798 SetSubmeshVisibility): BP_CharacterPreview has a
    /// Comp_FighterVisibilityManager (its SetupInitialVisibility and AnimNotify_SetSubmeshVisibility use it), so
    /// BlueprintInitializeAnimation @362 finds it and ToggleSubmeshVisibility writes the eyelids, the lashes and the
    /// teeth from the preview montage's curves; @15932 sets IsCharacterViewer, so Jerry's lids toggle as if he were
    /// attached. The update reads the curves of the evaluation before, then the frame's sequence is sampled.
    /// Deviation: a preview runs the toggles (a view with no VisibilityManager would run none, so the eyes never
    /// blinked and a blink's frames showed the body's eye squashed with no lid mesh over it).
    /// Unverified: the first step is seeded with its own frame's curves (as LeBron's).
    fn preview_step(&mut self, view: &mut CharView, clips: &Clips, s: &PreviewStep<'_>) {
        if std::mem::take(&mut self.preview_first) {
            CURVES.fill(clips, s.anim, s.time, &mut self.cur);
            self.from = self.cur;
        }
        self.toggles(true);
        for n in 0..PARTS.len() {
            if let (Some(pi), true) = (self.part[n], self.own[n] >= 0) {
                view.set_part_visible(pi, self.own[n] == 1);
            }
        }
        CURVES.fill(clips, s.anim, s.time, &mut self.tgt);
    }

    fn set(&mut self, w: &World, f: FighterId, view: &mut CharView, clips: &Clips) {
        let fi = w.fighter(f);
        // (not on a Disguise copy: its fighter's graph is another character's)
        if !self.no_state && !GRAPH_CHECKED.swap(true, Ordering::Relaxed) {
            let g = &w.char_data(f).fighter.slot_holds.graph;
            let has = |s: &str| g.nodes.iter().any(|n| matches!(n, SlotGraphNode::Slot { slot, .. } if slot == s));
            let missing: Vec<&str> = SLOT_NODES.into_iter().filter(|s| !has(s)).collect();
            if !missing.is_empty() {
                eprintln!(
                    "warning: Tom and Jerry pose: the data's Tom_and_Jerry_AnimBP slot graph lacks {}",
                    missing.join(", ")
                );
            }
        }
        // the respawn (no set() while dead: a gap in the ticks): ResetSubmeshVisibility (the four states) and the
        // AnimBP's own writes
        // Deviation: the respawn is found by a gap in the ticks set() sees (rather than a dead() call each placed frame
        // while the fighter is dead): a frame that steps several ticks (online catch-up) makes a gap too and resets
        // the four states.
        let tick = w.flow.tick;
        if self.last_tick.is_some_and(|t| tick > t + 1) {
            self.tom_lids = false;
            self.jerry_lids = false;
            self.teeth = false;
            self.own = [-1; 5];
        }
        self.last_tick = Some(tick);
        let s = if self.no_state { None } else { tj_of(w, f) };
        // the attached Jerry: the sim's sub-mesh state, hidden while not attached
        let jerry_hidden = w.names.get(JERRY_PART_LC).is_some_and(|n| fi.pose.sub_hidden.contains(&n));
        let with_jerry = !jerry_hidden && s.is_some_and(|s| s.jerry_attached);
        if let Some(jp) = self.jerry_part
            && !with_jerry
        {
            view.set_part_visible(jp, false);
        }
        // ModifyBone_6 / _7's alpha (JerryWallOffset, the AnimBP's own TurnAlpha: the sim's)
        self.alpha = match s {
            Some(s) if !s.anim.ignore_wall_offset => {
                f64::from(s.anim.turn_alpha.max(s.anim.wall_alpha).clamp(0.0, 1.0))
            }
            _ => 0.0,
        };
        // BlendListByBool_1's weight (the sim's) and the control slots' tracks
        self.arms_on = false;
        let list = *self.has_list.get_or_insert_with(|| arms_list_in(w, f));
        if let Some(s) = s
            && list
        {
            self.arm_l = f64::from(s.anim.arms.w.first().copied().unwrap_or(0.0));
            let sm = mvs_sim::fighter::shown_montage(w, f).and_then(|dash| {
                let st = if dash { fi.montages.dash_montage.as_ref() } else { fi.montages.montage.as_ref() }?;
                let uid = slot_uid(w, f, if dash { MontageSlot::Dash } else { MontageSlot::Main })?;
                Some((st, instance_data(&w.data, w, f, uid)?))
            });
            self.arm_w = sm.map_or(0.0, |(st, m)| st.shown_weight(m, &|s| eprintln!("warning: {s}")).min(1.0));
            if let Some((st, m)) = sm
                && self.arm_w > ZERO_W
            {
                for (k, slot) in ARMS_SLOTS.iter().enumerate() {
                    let at = st.pose_in(m, slot);
                    self.arm_clip[k] = None;
                    self.arm_time[k] = 0.0;
                    if let Some((si, gi, time)) = at {
                        let anim = &m.slots[si].segments[gi].anim;
                        self.arm_clip[k] = clips.get(anim);
                        if self.arm_clip[k].is_none() && !CLIP_WARNED.swap(true, Ordering::Relaxed) {
                            eprintln!("warning: Tom and Jerry pose: clip not loaded: {anim}");
                        }
                        self.arm_time[k] = time;
                        self.arms_on = true;
                    }
                }
            }
        }
        if self.slaved {
            return;
        }
        self.toggles(with_jerry);
        for n in 0..PARTS.len() {
            let Some(pi) = self.part[n] else { continue };
            // the AnimBP's write until the sim changes the part's visibility (the later write)
            let hidden = w.names.get(&self.tags[n]).is_some_and(|t| fi.pose.sub_hidden.contains(&t));
            let vis = i8::from(!hidden);
            if self.sim_vis[n] >= 0 && self.sim_vis[n] != vis {
                self.own[n] = -1;
            }
            self.sim_vis[n] = vis;
            view.set_part_visible(pi, if self.own[n] >= 0 { self.own[n] == 1 } else { vis == 1 });
        }
        // the sequence this tick's pose is sampled from
        CURVES.fill(clips, w.names.str(fi.anim.anim), f64::from(fi.anim.anim_time), &mut self.tgt);
    }

    fn reset(&mut self, _part_scalar: &mut dyn FnMut(usize, &str, f64)) {
        self.init();
    }
}

mod actors;
mod jerry;

/// The classes' own mesh drawing, by class name.
pub(crate) const RULES: &[(&str, MeshRule)] = actors::RULES;
