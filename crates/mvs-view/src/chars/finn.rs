//! Finn's render module: the entry, the AnimBP's pose hook and the teeth
//!. His coin, gem and apple actors and the BMO are in the sections below; the shockwave decal is his
//! effects' (mvs-render fx/chars/finn.rs).
//!
//! What the pose rule draws (Finn_AnimBP), the graph input first: LinkedAnimLayer -> Navigation -> Slot AirAttack /
//! GroundMovingSlot / GroundStillSlot -> Slot DefaultSlot (the sim's slot graph walk, which the view mixes in) ->
//! WAnimationMirrorNode -> LocalToComponent -> ModifyBone_6 (fml_un_C_neck_a) -> _8 (fml_un_C_face_att) -> _3
//! (fml_un_C_head) -> _5 / _4 (the ears) -> Constraint_1 / Constraint (fml_un_C_prop1 onto the wrists) -> ModifyBone_9 /
//! _2 (prop1) -> ModifyBone_1 / ModifyBone (head, face) -> Trail, AnimDynamics (scarf) -> ComponentToLocal.
//! - The HeadScale nodes ModifyBone_6 / _8 / _3 / _5 / _4 (scale BMM_Replace in BCS_ComponentSpace, alpha
//!   BoolToFloat(HeadScale.X > 1)): the bone's component-space scale becomes HeadScale (neck_a, face_att),
//!   ComponentMax((X, Y, Z x 0.75), (1, 1, 1)) (the head) or the same with 0.9 (the ears); its local scale the value
//!   over its parent's component scale. A UE axis Z is the pose's local y. HeadScale is Pawn_Finn's write @7619-7736
//!   (the entry's head_scale is empty: none of his nodes is the shared step's Additive bone-space kind).
//! - ModifyBone_1 / ModifyBone (head and face attach): translation (0, 0, -10) BMM_Additive in bone space, every
//!   evaluation.
//! - The sword's bone fml_un_C_prop1: cond = (IsHitStunned || (!IsMovingOnGround && !IsPlayingMontage)) &&
//!   !IsBeingCarried (@4369-5451). Constraint (R wrist) and Constraint_1 (L wrist) put the bone's component-space
//!   translation on the wrist's; ModifyBone_9 adds (-10, 25, -5) (facing right, bone space). The port draws a left
//!   facing as the reflection of the right-facing pose, so the right-facing pair is applied at both facings.
//! - The teeth: SetTeethVisibility (@886-1442): ToggleTeethVisibility(curve, tags, LastValue) for showTopLeftTeeth
//!   (TopTooth5..8), showTopRightTeeth (TopTooth4..1), showBotLeftTeeth (BotTooth5..8), showBotRightTeeth (BotTooth4..1):
//!   v = FTrunc(GetCurveValue); when v != LastValue each tooth i is set to v - 1 >= i. The curves of the evaluation
//!   before; the AnimBP's write of a tooth holds until the sim changes it (the later write). Facing left the bones
//!   TopTeethN / BotTeethN take the mirrored pose of their partners (Finn_MirrorTable, N <-> 9 - N): the tooth the
//!   build shows on bone N is drawn by the tooth on bone 9 - N.
//!
//! Deviations and unverified models:
//! - Unverified: a curve's value in a crossfade and the one tick the update lags the evaluation (as Steven's pose).
//! - Unverified: ModifyBone_2 (prop1 roll 180 at a left facing) is not drawn: the reflection of the right-facing sword
//!   is drawn; the HeadScale nodes' axes (UE (X, Y, Z) as the pose's (x, z, y)); IsHitStunned / IsMovingOnGround /
//!   IsPlayingMontage / IsBeingCarried read from the tick's sim state.
//! - Not ported: ModifyBone_7 / _11 / _10 (virtual bones, the view holds none), Trail and AnimDynamics of the scarf (no
//!   such node in the port), OnFacingChanged's TurnAlpha (read by no node), a check of the data's slot graph.
//! - Deviation: the view places no dead fighter: the respawn is a gap in the ticks set() runs at.
//!
//! The teeth: Pawn_Finn's sixteen StaticMeshComponents TopTooth1..8 / BotTooth1..8 (mesh Finn_Tooth) on the sockets
//! fml_un_C_finn_TopTeeth1..8 / BotTeeth1..8, hidden by SetupInitialVisibility and shown by the toggles above.
//! Materials: the actor entry Finn_Tooth (no team set). Unverified: the sockets are the last presented pose's.
//! Each draws custom depth with the fighter's stencil (PawnCx::outline).
//! On a preview (the menus' and the podium's BP_CharacterPreview) the skins' SubMeshes put the teeth on the same sockets
//! (socket_statics: component meshes of the preview's view) and Finn_AnimBP toggles them all the same: with no pawn
//! owner, BlueprintInitializeAnimation @5607 falls through to BindAnimationEvents, which finds the preview actor's
//! Comp_FighterVisibilityManager (@286-471), and BlueprintUpdateAnimation reaches SetTeethVisibility (@1571 to @6055)
//! on the preview's curves (preview_step).
//!
//! Not ported here: the shop and its HUD (the HUD lane); see the track's rows.

use mvs_data::value::Value;
use mvs_sim::World;
use mvs_sim::fighter::FighterId;

use super::{CharRender, PawnActor, PawnCx, PoseHook, PoseOf, PreviewOf, PreviewStep};
use crate::ActorPose;
use crate::actor_anim::{
    ActorMachine, BlendOption, BoolListDef, MachineDef, MachineQuery, PoseDef, SeqDef, StateDef, TransitionDef,
};
use crate::actor_view::{Comp, SkinnedActor, StaticView, Xform, place_under, rel_rows, xyz};
use crate::actors::{ActorComponents, ActorHost, ActorMesh, ActorPlace, MeshRule, Rows};
use crate::anim::{Clips, Pose};
use crate::curves::CurveTable;
use crate::frame::MeshPose;
use crate::view::{CharView, PoseCx, PoseRules};
use mvs_sim::actors::world_actor::{WorldActor, WorldActorState};
use mvs_sim::actors::{ActorRef, ItemExt, ProjectileExt};
use mvs_sim::chars::finn::bmo::BmoActor;
use mvs_sim::fighter::common::{actor_def, actor_def_owner};
use mvs_vfx::math::rotation_rows;

pub(crate) const RENDER: CharRender =
    CharRender { pose: Some(finn_pose), pawn_actors: Some(teeth), socket_statics: &TOOTH_TAGS, ..CharRender::GENERIC };

/// The classes' own mesh drawing, by class name.
pub(crate) const RULES: &[(&str, MeshRule)] = &[
    (TREASURE, MeshRule { rot: None, mesh: Some(coin_mesh) }),
    (DASH_TARGET, MeshRule { rot: None, mesh: Some(gem_mesh) }),
    (BMO, MeshRule { rot: None, mesh: Some(bmo_view) }),
];

const CURVES: CurveTable =
    CurveTable::new(&["showTopLeftTeeth", "showTopRightTeeth", "showBotLeftTeeth", "showBotRightTeeth"]);

/// The teeth arrays of the CDO, by curve (ToggleTeethVisibility's SubmeshTags), as indices into the tags below.
const TEETH: [[usize; 4]; 4] = [[4, 5, 6, 7], [3, 2, 1, 0], [12, 13, 14, 15], [11, 10, 9, 8]];
/// Every tooth tag, Top1..8 then Bot1..8.
const TOOTH_TAGS: [&str; 16] = [
    "TopTooth1",
    "TopTooth2",
    "TopTooth3",
    "TopTooth4",
    "TopTooth5",
    "TopTooth6",
    "TopTooth7",
    "TopTooth8",
    "BotTooth1",
    "BotTooth2",
    "BotTooth3",
    "BotTooth4",
    "BotTooth5",
    "BotTooth6",
    "BotTooth7",
    "BotTooth8",
];
/// A tooth's mirror partner (Finn_MirrorTable: N <-> 9 - N).
const fn partner(i: usize) -> usize {
    if i < 8 { 7 - i } else { 8 + (15 - i) }
}

/// The HeadScale nodes in graph order: the bone and the factor on UE Z (0: HeadScale itself).
const HEAD_NODES: [(&str, f64); 5] = [
    ("fml_un_C_neck_a", 0.0),
    ("fml_un_C_face_att", 0.0),
    ("fml_un_C_head", 0.75),
    ("fml_un_L_Ear_att", 0.9),
    ("fml_un_R_Ear_att", 0.9),
];
/// ModifyBone_1 / ModifyBone: (0, 0, -10) UE cm in the bone's frame, in the pose's axes (x, z, y) / 100.
const FACE_MOVE: [f64; 3] = [0.0, -0.1, 0.0];
const FACE_BONES: [&str; 2] = ["fml_un_C_head", "fml_un_C_face_att"];
const SWORD: &str = "fml_un_C_prop1";
const WRIST: &str = "fml_un_R_wrist";
/// ModifyBone_9 facing right: (-10, 25, -5) UE cm -> the pose's (x, z, y) / 100.
const SWORD_MOVE: [f64; 3] = [-0.1, -0.05, 0.25];

#[derive(Debug)]
struct FinnPose {
    head: Vec<(usize, f64)>,
    face: Vec<usize>,
    sword: Option<usize>,
    wrist: Option<usize>,
    head_scale: f64,
    sword_on: bool,
    facing_left: bool,
    last_tick: Option<i32>,
    tgt: [f64; 4],
    cur: [f64; 4],
    from: [f64; 4],
    fades: u32,
    /// TopLeftTeethVal .. BotRightTeethVal (CDO 0).
    last: [i32; 4],
    /// The AnimBP's last write of a tooth (-1 none, 0 hidden, 1 shown) and the sim's last visibility (-1 not seen).
    own: [i8; 16],
    sim_vis: [i8; 16],
    shown: [bool; 16],
    /// PoseOf::slaved: no AnimBP of its own (set does nothing, the sword's nodes do not run).
    slaved: bool,
    /// A fighter's view this tick (set ran, not slaved).
    live: bool,
}

fn finn_pose(view: &mut CharView, _clips: &Clips, of: &PoseOf<'_>) -> Box<dyn PoseHook> {
    let bone = |b: &str| {
        let i = view.rig.index(b);
        if i.is_none() {
            eprintln!("warning: Finn pose: bone missing: {b}");
        }
        i
    };
    Box::new(FinnPose {
        head: HEAD_NODES.iter().filter_map(|&(b, z)| Some((bone(b)?, z))).collect(),
        face: FACE_BONES.iter().filter_map(|b| bone(b)).collect(),
        sword: bone(SWORD),
        wrist: bone(WRIST),
        head_scale: 1.0,
        sword_on: false,
        facing_left: false,
        last_tick: None,
        tgt: [0.0; 4],
        cur: [0.0; 4],
        from: [0.0; 4],
        fades: view.fade_count(),
        last: [0; 4],
        own: [-1; 16],
        sim_vis: [-1; 16],
        shown: [false; 16],
        slaved: of.slaved,
        live: false,
    })
}

type V3 = [f64; 3];

/// A bone's component-space scale in `p` (the FTransform rule's scale: componentwise products from the top).
fn comp_scale(p: &Pose, parents: &[i32], i: usize) -> V3 {
    let mut s = [1.0; 3];
    let mut k = i as i32;
    while k >= 0 {
        for (c, v) in s.iter_mut().enumerate() {
            *v *= f64::from(p.s[k as usize * 3 + c]);
        }
        k = parents[k as usize];
    }
    s
}

fn rotate(q: [f64; 4], v: V3) -> V3 {
    let [x, y, z] = v;
    let [qx, qy, qz, qw] = q;
    let tx = 2.0 * (qy * z - qz * y);
    let ty = 2.0 * (qz * x - qx * z);
    let tz = 2.0 * (qx * y - qy * x);
    [x + qw * tx + qy * tz - qz * ty, y + qw * ty + qz * tx - qx * tz, z + qw * tz + qx * ty - qy * tx]
}

/// A bone's component-space transform in `p`: translation, rotation, scale.
#[allow(clippy::needless_range_loop, reason = "the three axes index parallel arrays")]
fn component(p: &Pose, parents: &[i32], i: usize) -> (V3, [f64; 4], V3) {
    let mut t = [0.0; 3];
    let mut r = [0.0, 0.0, 0.0, 1.0];
    let mut s = [1.0; 3];
    let mut k = i as i32;
    while k >= 0 {
        let ku = k as usize;
        let sc = [f64::from(p.s[ku * 3]), f64::from(p.s[ku * 3 + 1]), f64::from(p.s[ku * 3 + 2])];
        for c in 0..3 {
            t[c] *= sc[c];
        }
        t = rotate(p.quat(ku), t);
        for c in 0..3 {
            t[c] += f64::from(p.t[ku * 3 + c]);
        }
        let [ax, ay, az, aw] = p.quat(ku);
        let [bx, by, bz, bw] = r;
        r = [
            aw * bx + ax * bw + ay * bz - az * by,
            aw * by - ax * bz + ay * bw + az * bx,
            aw * bz + ax * by - ay * bx + az * bw,
            aw * bw - ax * bx - ay * by - az * bz,
        ];
        for c in 0..3 {
            s[c] *= sc[c];
        }
        k = parents[ku];
    }
    (t, r, s)
}

/// t += q (s * m): a bone-space additive translation on the local pose.
fn move_bone(p: &mut Pose, i: usize, m: V3) {
    let v = [f64::from(p.s[i * 3]) * m[0], f64::from(p.s[i * 3 + 1]) * m[1], f64::from(p.s[i * 3 + 2]) * m[2]];
    let v = rotate(p.quat(i), v);
    for (c, d) in v.iter().enumerate() {
        p.t[i * 3 + c] = (f64::from(p.t[i * 3 + c]) + d) as f32;
    }
}

impl PoseRules for FinnPose {
    #[allow(clippy::needless_range_loop, reason = "the three axes index parallel arrays")]
    fn rule(&mut self, cx: &mut PoseCx<'_>, p: &mut Pose, _dt: f64) {
        // ModifyBone_6 / _8 / _3 / _5 / _4: the component-space scale replaced (alpha HeadScale.X > 1)
        let k = self.head_scale;
        if k > 1.0 {
            for &(i, z) in &self.head {
                let ty = if z == 0.0 { k } else { (k * z).max(1.0) };
                let pa = cx.parents[i];
                let cs = if pa >= 0 { comp_scale(p, cx.parents, pa as usize) } else { [1.0; 3] };
                let want = [k, ty, k];
                for c in 0..3 {
                    p.s[i * 3 + c] = if cs[c] != 0.0 { (want[c] / cs[c]) as f32 } else { 0.0 };
                }
            }
        }
        // Constraint (Constraint_1 reflected) and ModifyBone_9: the sword's bone on the right wrist
        if let (true, Some(sword), Some(wrist)) = (self.live && self.sword_on, self.sword, self.wrist) {
            let (mut wp, _, _) = component(p, cx.parents, wrist);
            let pa = cx.parents[sword];
            if pa < 0 {
                for c in 0..3 {
                    p.t[sword * 3 + c] = wp[c] as f32;
                }
            } else {
                let (pt, pr, ps) = component(p, cx.parents, pa as usize);
                for c in 0..3 {
                    wp[c] -= pt[c];
                }
                let w = rotate([-pr[0], -pr[1], -pr[2], pr[3]], wp);
                for c in 0..3 {
                    p.t[sword * 3 + c] = if ps[c] != 0.0 { (w[c] / ps[c]) as f32 } else { 0.0 };
                }
            }
            move_bone(p, sword, SWORD_MOVE);
        }
        // ModifyBone_1 / ModifyBone: the head and the face attach 10 cm down their own Z
        for &i in &self.face {
            move_bone(p, i, FACE_MOVE);
        }
        // this evaluation's curves, for the next update
        if cx.fade_count != self.fades {
            self.fades = cx.fade_count;
            self.from = self.cur;
        }
        let w = cx.fade_weight;
        for k in 0..4 {
            self.cur[k] = if w >= 1.0 { self.tgt[k] } else { self.from[k] + (self.tgt[k] - self.from[k]) * w };
        }
    }
}

impl FinnPose {
    /// SetTeethVisibility: ToggleTeethVisibility per curve on the evaluation before's value (`cur`).
    fn toggle_teeth(&mut self) {
        for (teeth, (cur, last)) in TEETH.iter().zip(self.cur.iter().zip(self.last.iter_mut())) {
            let v = cur.trunc() as i32;
            if v == *last {
                continue;
            }
            for (i, &t) in teeth.iter().enumerate() {
                self.own[t] = i8::from(v > i as i32);
            }
            *last = v;
        }
    }
}

impl PoseHook for FinnPose {
    /// `preview()`: a view with no fighter: no sword (Fighter is None), the teeth hidden by SetupInitialVisibility
    /// until the AnimBP's toggles show them (preview_step).
    fn preview(&mut self, _view: &mut CharView, _of: &PreviewOf<'_>) {
        self.live = false;
        self.own = [-1; 16];
        self.shown = [false; 16];
    }

    /// The preview's BlueprintUpdateAnimation: SetTeethVisibility on the curves of the evaluation before, then the
    /// frame's sequence sampled for this evaluation's curves.
    // Unverified: SetupInitialVisibility is taken as run before the first update (preview.rs's own Unverified on the
    // podium's Delay(0.2)), so InitialVisibilitySet holds from the first frame.
    fn preview_step(&mut self, _view: &mut CharView, clips: &Clips, step: &PreviewStep<'_>) {
        self.facing_left = step.facing_left;
        self.toggle_teeth();
        for t in 0..16 {
            self.shown[t] = self.own[t] == 1;
        }
        CURVES.fill(clips, step.anim, step.time, &mut self.tgt);
    }

    #[allow(clippy::needless_range_loop, reason = "the curve and tooth indices address parallel arrays")]
    fn set(&mut self, w: &World, f: FighterId, view: &mut CharView, clips: &Clips) {
        // (the HeadScale nodes read the entry's head scale live or not)
        self.head_scale = view.head_scale;
        self.live = !self.slaved;
        if !self.live {
            return;
        }
        let fi = w.fighter(f);
        // the respawn: SetupInitialVisibility's write of the teeth (the sim's)
        let tick = w.flow.tick;
        if self.last_tick.is_some_and(|t| tick > t + 1) {
            self.own = [-1; 16];
        }
        self.last_tick = Some(tick);
        self.facing_left = fi.body.facing_dir < 0.0;
        // Finn_AnimBP's inputs by their names
        let moving_on_ground = mvs_sim::fighter::grounded(w, f);
        let playing_montage = mvs_sim::fighter::slot_montage(w, f).is_some();
        let carried = fi.shapes.carried.is_some();
        self.sword_on = (mvs_sim::fighter::in_hitstun(w, f) || (!moving_on_ground && !playing_montage)) && !carried;
        // SetTeethVisibility on the curves of the evaluation before
        self.toggle_teeth();
        for t in 0..16 {
            // the AnimBP's write until the sim changes the tooth's visibility (the later write)
            let hidden = w.names.get(&TOOTH_TAGS[t].to_lowercase()).is_some_and(|n| fi.pose.sub_hidden.contains(&n));
            let vis = i8::from(!hidden);
            if self.sim_vis[t] >= 0 && self.sim_vis[t] != vis {
                self.own[t] = -1;
            }
            self.sim_vis[t] = vis;
            self.shown[t] = if self.own[t] >= 0 { self.own[t] == 1 } else { vis == 1 };
        }
        // the sequence this tick's pose is sampled from
        CURVES.fill(clips, w.names.str(fi.anim.anim), f64::from(fi.anim.anim_time), &mut self.tgt);
    }

    /// `reset()`: a new pawn: the AnimBP's state anew.
    fn reset(&mut self, _part_scalar: &mut dyn FnMut(usize, &str, f64)) {
        self.tgt = [0.0; 4];
        self.cur = [0.0; 4];
        self.from = [0.0; 4];
        self.last = [0; 4];
        self.own = [-1; 16];
        self.sim_vis = [-1; 16];
        self.shown = [false; 16];
        self.sword_on = false;
        self.last_tick = None;
    }

    /// FinnPose.toothShown: the AnimBP's write or the sim's, by the facing's partner.
    fn shown(&self, tag: &str, sim: bool) -> bool {
        match TOOTH_TAGS.iter().position(|t| t.eq_ignore_ascii_case(tag)) {
            Some(k) => self.shown[if self.facing_left { partner(k) } else { k }],
            None => sim,
        }
    }
}

// ---- the teeth ----

const TOOTH_MESH: &str = "Finn_Tooth";

#[derive(Debug)]
struct Tooth {
    tag: String,
    v: StaticView,
    socket: String,
    rel: Xform,
}

impl PawnActor for Tooth {
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
        out.push(ActorPose { meshes, ..ActorPose::default() });
    }
}

/// `finnTeeth`: the StaticMeshComponents of the data with a tooth tag, in the data's order.
fn teeth(host: &ActorHost<'_>, w: &World, f: FighterId) -> Vec<Box<dyn PawnActor>> {
    let mut out: Vec<Box<dyn PawnActor>> = Vec::new();
    let d = &w.char_data(f).fighter;
    for c in d.components.pawn.values() {
        if c.ty != "StaticMeshComponent" {
            continue;
        }
        let Some(p) = c.p.as_obj() else { continue };
        let Some(tag) = p
            .get("ComponentTags")
            .and_then(Value::as_arr)
            .and_then(|t| {
                t.iter().filter_map(Value::as_str).find(|t| TOOTH_TAGS.iter().any(|k| k.eq_ignore_ascii_case(t)))
            })
            .map(str::to_lowercase)
        else {
            continue;
        };
        let Some(v) = StaticView::new(host, w, f, TOOTH_MESH, "") else {
            eprintln!("warning: Finn: the teeth are not drawn (Finn_Tooth or its materials are not loaded)");
            break;
        };
        let rel = Xform {
            loc: xyz(p.get("RelativeLocation"), 0.0),
            rot: rel_rows(p.get("RelativeRotation")),
            scale: xyz(p.get("RelativeScale3D"), 1.0),
        };
        out.push(Box::new(Tooth { tag, v, socket: c.socket.clone().unwrap_or_else(|| "None".to_owned()), rel }));
    }
    if out.len() != TOOTH_TAGS.len() {
        eprintln!("warning: Finn: {} teeth in the data, not {}", out.len(), TOOTH_TAGS.len());
    }
    out
}

// ---- FinnTreasure_C, the coin ----
// SkelMesh_FinnCoin (AnimationSingleNode SkelMesh_FinnCoin_Anim, looping; yaw 90, scale 0.8 on the Capsule). Its
// MIC_Coin (CreateDynamicMaterialInstance @2371): Team = SelectFloat(0.6, the colour value, IsLocalOnline) @2265-2316
// (the colour's team), Alpha 1 at the start and at PauseFade, max(Blink, 0.25) in DeathBlinkWarning's update
// @3722-3764. The spin: TargetedMulticast @1591-1628 SetPlayRate(0.1) and SetPosition(0) on both its callers
// (Targeted with gravity 0.5, Untargeted with 4.0), then Untargeted's SetPlayRate(1.0) @1432; InitFromPool @4040
// SetPlayRate(1.0). The sim keeps IsTargeted and the gravity scale, so the spin is told from them: rate 0.1 while
// targeted at gravity 0.5, 1 otherwise, the position from 0 at each change.
// Unverified: the spin's position is the render's from the coin's age (the animation's tick against the actor's not
// read); a SetPosition the view sees late (a rollback) starts the spin at that tick.
// Its particle components (PS_Finn_CoinSparkle, PS_Finn_CoinSparkles) are the effects' (mvs-render fx/chars/finn.rs).

const TREASURE: &str = "FinnTreasure_C";
const COIN_MESH: &str = "SkelMesh_FinnCoin";
const COIN_CLIP: &str = "SkelMesh_FinnCoin_Anim";
const STEP: f64 = 1.0 / 60.0;
/// DeathBlinkWarning's Blink track (FinnTreasure CurveFloat_1, every key RCIM_Constant), time and value.
const BLINK: [(f64, f64); 14] = [
    (0.0, 1.0),
    (0.07063842, 0.009337544),
    (0.24687612, 1.0),
    (0.65320194, 0.0),
    (0.8392309, 1.0),
    (1.2406613, 0.0),
    (1.475645, 1.0),
    (1.8477024, 0.0),
    (2.0092535, 1.0),
    (2.2882967, 0.0),
    (2.4988027, 1.0),
    (2.6848316, 0.0),
    (2.81701, 1.0),
    (2.9736657, 0.0015562773),
];

/// `blinkAt`: the last key at or before `t`.
pub fn blink_at(t: f64) -> f64 {
    let mut v = BLINK[0].1;
    for (k, b) in BLINK {
        if t < k {
            break;
        }
        v = b;
    }
    v
}

#[derive(Debug)]
struct CoinView {
    id: mvs_sim::actors::ProjectileId,
    view: SkinnedActor,
    comp: Comp,
    clip: Option<usize>,
    from: FighterId,
    phase: i32,
    phase_age: i32,
    sampled: i32,
    alpha: f64,
}

fn comp_of(c: &crate::actors::Component) -> Comp {
    Comp { par: c.parent.clone(), rl: c.loc, rot: c.rot, rs: c.scale }
}

fn coin_mesh(host: &mut ActorHost<'_>, w: &World, a: ActorRef, cs: &ActorComponents) -> Option<Box<dyn ActorMesh>> {
    let ActorRef::Projectile(id) = a else { return None };
    let pr = w.actors.projectiles.get(id)?;
    let owner = pr.owner?;
    let c = cs.get(COIN_MESH)?;
    let from = actor_def_owner(w, owner, pr.cls);
    let (path, keys) = host.draws_of(w, from).and_then(|d| d.actor(COIN_MESH, None))?;
    let info = host.mesh(&path)?;
    let mut view = SkinnedActor::new(info, &keys);
    let clip = host.clips_of(w, from).and_then(|c| c.get(COIN_CLIP));
    if clip.is_none() {
        eprintln!("warning: Finn: the coin clip is not loaded: {COIN_CLIP}");
    }
    let team = host.colour(w, owner).team;
    for m in view.mats.iter_mut().flatten() {
        crate::pose::set_scalar(m, "Team", team);
    }
    Some(Box::new(CoinView {
        id,
        view,
        comp: comp_of(c),
        clip,
        from,
        phase: -1,
        phase_age: 0,
        sampled: -1,
        alpha: -1.0,
    }))
}

impl ActorMesh for CoinView {
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
        if !shown {
            return;
        }
        let Some(pr) = w.actors.projectiles.get(self.id) else { return };
        let (targeted, fade) = match &pr.ext {
            ProjectileExt::FinnTreasure(c) => (c.is_targeted, c.fade),
            _ => (false, -1.0),
        };
        let ph = if !targeted {
            0
        } else if mvs_sim::actors::projectile::gravity_scale(pr) == 0.5 {
            1
        } else {
            2
        };
        if ph != self.phase {
            self.phase = ph;
            self.phase_age = if ph == 0 { 0 } else { pr.age };
            self.sampled = -1;
        }
        if let Some(clip) = self.clip
            && pr.age != self.sampled
        {
            self.sampled = pr.age;
            let t = f64::from((pr.age - self.phase_age).max(0)) * STEP * if ph == 1 { 0.1 } else { 1.0 };
            if let Some(clips) = host.clips_of(w, self.from) {
                self.view.play(clips, clip, t, true);
            }
        }
        let a = if fade < 0.0 { 1.0 } else { blink_at(f64::from(fade)).max(0.25) };
        if a != self.alpha {
            self.alpha = a;
            for m in self.view.mats.iter_mut().flatten() {
                crate::pose::set_scalar(m, "Alpha", a);
            }
        }
        let (x, y) = at.at(alpha);
        self.view.set(&place_under(&self.comp, &at.rot, at.scale, x, y));
        self.view.draw(out);
    }

    fn bones(&self, out: &mut Vec<(std::sync::Arc<str>, Xform)>) {
        self.view.bones_world(out);
    }

    fn end(&mut self) {}
}

// ---- FinnDashThrownTarget_C, the gem and the apple ----
// StaticMesh (Gem, scale 8 on the Capsule) under the item's rotation (its facing's yaw and the pitch). SpawnVisuals
// @3740-3968: SetMaterial(0, IsLocalOnline ? Yellow : colour index 0 ? Red : 1 ? Blue) (ActorHost::variant).
// SetAppleVisualsMulticast @3497: SetStaticMesh(Apple), then after its Delay(0.1) @249-477 SetMaterial(0, ...apple).
// The fade's SetActorHiddenInGame(Blink < 0.5) @5527 is the sim's (the view's `shown`).
// Deviation: the apple's first 0.1 s keeps the gem's material on the Apple mesh in the build; drawn with the apple's
// materials from the swap.
// Deviation: held (Comp_ItemManager), the gem is drawn at the place the shared path hands over (the holder's mesh
// origin), not on the hold socket.
// Its PS_Finn_CoinSparkle (on the Capsule) is the effects' (mvs-render fx/chars/finn.rs).

const DASH_TARGET: &str = "FinnDashThrownTarget_C";
const GEM_MESH: &str = "Gem";
const APPLE_MESH: &str = "Apple";

#[derive(Debug)]
struct GemView {
    id: mvs_sim::actors::ItemId,
    gem: Option<StaticView>,
    apple: Option<StaticView>,
    comp: Comp,
    from: FighterId,
}

fn static_of(host: &ActorHost<'_>, w: &World, from: FighterId, mesh: &str, variant: &str) -> Option<StaticView> {
    let v = StaticView::new(host, w, from, mesh, variant);
    if v.is_none() {
        eprintln!("warning: Finn: {mesh} is not drawn (its mesh or materials are not loaded)");
    }
    v
}

fn gem_mesh(host: &mut ActorHost<'_>, w: &World, a: ActorRef, cs: &ActorComponents) -> Option<Box<dyn ActorMesh>> {
    let ActorRef::Item(id) = a else { return None };
    let it = w.actors.items.get(id)?;
    let c = cs.get("StaticMesh")?;
    let from = it.spawner.or(it.owner)?;
    let variant = host.variant(w, it.owner.unwrap_or(from));
    let gem = static_of(host, w, from, GEM_MESH, variant);
    Some(Box::new(GemView { id, gem, apple: None, comp: comp_of(c), from }))
}

impl ActorMesh for GemView {
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
        let is_apple = matches!(&it.ext, ItemExt::Gem(g) if g.weak_throw);
        let variant = host.variant(w, it.owner.unwrap_or(self.from));
        if is_apple && self.apple.is_none() {
            self.apple = static_of(host, w, self.from, APPLE_MESH, variant);
        }
        let (v, other) = if is_apple { (&mut self.apple, &mut self.gem) } else { (&mut self.gem, &mut self.apple) };
        if let Some(o) = other {
            o.visible = false;
        }
        let Some(v) = v else { return };
        v.visible = shown;
        if !shown {
            return;
        }
        v.dress(host, variant);
        let (x, y) = at.at(alpha);
        v.set(&place_under(&self.comp, &at.rot, at.scale, x, y));
        v.draw(out);
    }
}

// ---- BMO ----
//
// BMO_FallowActor (the sim: mvs-sim chars/finn/bmo.rs BmoActor), a World actor: BMOMesh and its BMOAnimBlueprint.
// Offsets are each class's ubergraph's.
// The mesh: BMOMesh (Bmo_SkeletalMesh, one slot, Bmo_Material; CastShadow false) under the Capsule root at (0, 0, -80),
// scale 0.8. Its place: the sim's x / y (the actor's location, attached to Finn) and the depth UE X =
// Follow_Y_Offset = Finn's scaled capsule radius - 100 (@2712-2804), -100 while wall sliding (OnWallSlide @5296-5398:
// SelectFloat(100, -100, FacingLeft) against the pawn's Y axis, which turns with him: the same world X both ways). Its
// rotation: the actor's relative yaw -90 facing left / +90 facing right (@1912, @3727) under Finn's own +90 / -90
// keeps the actor's world yaw 0; BMOMesh's relative yaw +90 facing left / -90 facing right (@4270); the AnimBP's
// FacingLeft = NOT FacingLeft (@4414) turns its WAnimationMirror (BMO_MirrorTable) on when he faces right.
// Unverified (the mirror node taken as a full reflection): the mirrored pose is drawn as the reflection of the
// left-facing one across the play plane (world UE Y), not through BMO_MirrorTable: the view's rows for yaw +90 with
// their Y components negated.
// Visibility: AnimNotify_Finn_BMO_Visibility (the sim's `visible`) and AdjustVisibility @4875 (`mesh_visible`), both
// on BMOMesh: shown when both are.
// Custom depth (UpdateLocality @1172-1515) is written only when IsOnline: offline nothing. Deviation: online not
// drawn.
//
// BMOAnimBlueprint (ActorMachine, once a World step): Root <- WAnimationMirror (#6, bEnabled = FacingLeft: the
// reflection above) <- Slot DefaultSlot (#49: no graph plays a montage on BMO, it passes its source) <- BlendListByBool
// #5 (HitReact: no graph writes it, so false: child 1) <- Movement (#7). The HitReact branch (BMO_N_Hit_01..04 by
// HitReactInt) is never reached.
// BlueprintUpdateAnimation @660-1463 (with the follower set by UpdateFollower @1792, which BMO calls @3946 after its
// Delay(0.1)): IsFalling, IsCrouching, IsMovingOnGround and HorizontalSpeed = |Velocity.XY| of the follower's CMC.
// UpdateFollower binds OnIsWallSliding, OnJumpNumberChange, OnIsJumping and OnIsFastFalling (@1811-2026): the
// SetWallSliding / SetJumpNumberChange / SetOnIsJumping / SetFastFall events keep the last broadcast value.
// Unverified: the four event inputs are read each tick from the fighter's current values (Jumping, CurrentJumpNumber,
// IsFastFalling; IsWallSliding as the sim's BMO kept its OnWallSlide), not from their broadcasts; the 0.1 s before the
// follower is set is counted from the view's first tick (the AnimBP's inputs stay at their defaults until then).
// Deviation: the view's AnimBP runs once a World step, not once a presented tick; a rollback's jump of several
// ticks advances it one tick.

const BMO: &str = "BMO_FallowActor_C";
const BMO_COMP: &str = "BMOMesh";
/// Delay(0.1) before UpdateFollower (@3946).
const FOLLOWER_TICKS: i32 = 6;
const BMO_DEG: f64 = std::f64::consts::PI / 180.0;

/// The AnimBP's variables the rules and blend lists read.
#[derive(Debug, Default)]
struct BmoInputs {
    is_falling: bool,
    is_crouching: bool,
    /// IsMovingOnGround.
    on_ground: bool,
    /// HorizontalSpeed.
    h_speed: f64,
    is_jumping: bool,
    jump_number: i32,
    wall_sliding: bool,
    fast_fall: bool,
}

fn seq(seq: &'static str, looped: bool) -> PoseDef<BmoInputs> {
    PoseDef::Seq(SeqDef::new(seq, looped))
}

fn bool_list(test: fn(&BmoInputs) -> bool, t: PoseDef<BmoInputs>, f: PoseDef<BmoInputs>) -> PoseDef<BmoInputs> {
    PoseDef::BoolList(BoolListDef {
        test,
        t: Box::new(t),
        f: Box::new(f),
        blend_time: [0.1, 0.1],
        option: BlendOption::Linear,
    })
}

fn state(name: &'static str, pose: PoseDef<BmoInputs>) -> StateDef<BmoInputs> {
    StateDef { name, pose: Some(pose), entry_rule: None, always_reset: false }
}

fn tr(
    from: &'static str,
    to: &'static str,
    xfade: f64,
    rule: fn(&BmoInputs, &dyn MachineQuery) -> bool,
) -> TransitionDef<BmoInputs> {
    TransitionDef { from, to, xfade, option: BlendOption::HermiteCubic, rule: Some(rule), auto: false }
}

/// RunAndIdle (#9, the Ground state's machine): entry Idle, MaxTransitionsPerFrame 3, bSkipFirstUpdateTransition and
/// bReinitializeOnBecomingRelevant; every crossfade HermiteCubic. Rule offsets in BMOAnimBlueprint.txt.
fn run_and_idle() -> MachineDef<BmoInputs> {
    MachineDef {
        entry: "Idle",
        reinit_on_becoming_relevant: true,
        max_transitions_per_frame: 3,
        skip_first_update_transition: true,
        states: vec![
            // BlendListByBool(IsCrouching): true BMO_Crouch, false BMO_Idle, 0.1 s each
            state("Idle", bool_list(|i| i.is_crouching, seq("BMO_Crouch", true), seq("BMO_Idle", true))),
            state("Run", seq("BMO_Run", true)),
            state("IdleIn", seq("BMO_Idle_In", false)),
        ],
        transitions: vec![
            tr("Idle", "Run", 0.2, |i, _| i.h_speed >= 10.0),                 // @588
            tr("Run", "IdleIn", 0.0, |i, _| i.h_speed < 10.0),                // @521
            tr("IdleIn", "Run", 0.0, |i, _| i.h_speed >= 10.0),               // @349
            tr("IdleIn", "Idle", 0.05, |_, q| q.remaining("IdleIn") <= 0.05), // @244 (9, 2)
        ],
    }
}

/// Movement (#7): entry Ground, MaxTransitionsPerFrame 3, bSkipFirstUpdateTransition, bReinitializeOnBecomingRelevant;
/// every crossfade HermiteCubic; each state's transitions in their baked order.
fn bmo_movement() -> MachineDef<BmoInputs> {
    MachineDef {
        entry: "Ground",
        reinit_on_becoming_relevant: true,
        max_transitions_per_frame: 3,
        skip_first_update_transition: true,
        states: vec![
            state("Ground", PoseDef::Machine(run_and_idle())),
            // BlendListByBool(FastFall): BMO_Fall both ways, 0.1 s each
            state("Fall", bool_list(|i| i.fast_fall, seq("BMO_Fall", true), seq("BMO_Fall", true))),
            state("Jump", seq("BMO_Jump", false)),
            state("WallSlide", seq("BMO_Wall", true)),
            // BlendListByBool(HorizontalSpeed >= 10 @1510): true BMO_Land_Run, false BMO_Land, 0.1 s each
            state("Land", bool_list(|i| i.h_speed >= 10.0, seq("BMO_Land_Run", false), seq("BMO_Land", false))),
            state("Jump2", seq("BMO_Jump", false)),
        ],
        transitions: vec![
            tr("Ground", "Jump", 0.0, |i, _| i.is_jumping),
            tr("Ground", "Fall", 0.2, |i, _| i.is_falling),
            tr("Fall", "Jump2", 0.0, |i, _| i.is_jumping && i.jump_number == 2), // @1577
            tr("Fall", "WallSlide", 0.05, |i, _| i.wall_sliding),
            tr("Fall", "Land", 0.0, |i, _| i.on_ground),
            tr("Jump", "Land", 0.0, |i, _| i.on_ground),
            tr("Jump", "Jump2", 0.0, |i, _| i.is_jumping && i.jump_number == 2), // @1682
            tr("Jump", "Fall", 0.2, |_, q| q.remaining("Jump") == 0.0),          // @34 (7, 2)
            tr("WallSlide", "Fall", 0.05, |i, _| !i.wall_sliding),
            tr("Land", "Jump", 0.0, |i, _| i.is_jumping),
            tr("Land", "Ground", 0.1, |_, q| q.remaining("Land") <= 0.1), // @416 (7, 4)
            tr("Jump2", "Land", 0.0, |i, _| i.on_ground),
            // @139 reads GetRelevantAnimTimeRemaining(7, 2): the Jump state's, not Jump2's (kept as the build has it)
            tr("Jump2", "Fall", 0.2, |_, q| q.remaining("Jump") == 0.0),
        ],
    }
}

#[derive(Debug)]
struct BmoView {
    id: mvs_sim::actors::ActorId,
    /// The fighter whose clips it plays.
    from: FighterId,
    sk: SkinnedActor,
    machine: ActorMachine<BmoInputs>,
    pose: Pose,
    inp: BmoInputs,
    /// The view's ticks (the follower's Delay).
    age: i32,
    /// BMOMesh's relative location and scale.
    rel: [f64; 3],
    scale: [f64; 3],
    /// The mesh's world rows facing left (yaw 0 + 90), and facing right (their reflection across world Y).
    left: Rows,
    right: Rows,
    /// The actor's place at the last two ticks (UE cm), drawn between them each frame.
    prev: [f64; 3],
    cur: [f64; 3],
    facing_left: bool,
    stepped: bool,
}

fn bmo_of(w: &World, id: mvs_sim::actors::ActorId) -> Option<(&WorldActor, &BmoActor)> {
    let a = w.actors.actors.get(id).filter(|a| !a.dead)?;
    match &a.state {
        WorldActorState::BmoActor(b) => Some((a, b)),
        _ => None,
    }
}

fn bmo_view(host: &mut ActorHost<'_>, w: &World, a: ActorRef, _cs: &ActorComponents) -> Option<Box<dyn ActorMesh>> {
    let ActorRef::Actor(id) = a else { return None };
    let (x, _) = bmo_of(w, id)?;
    // the fighter whose character data has the class (its mesh, clips and materials come with it)
    let from = actor_def_owner(w, x.owner, x.cls);
    let c =
        actor_def(w, ActorRef::Fighter(from), x.cls).and_then(|d| d.components.get(BMO_COMP)).and_then(Value::as_obj);
    let mesh = c.and_then(|c| c.get("SkeletalMesh")).and_then(Value::as_str);
    // (the draws keyed by the mesh)
    let drawn = mesh.and_then(|m| host.draws_of(w, from)?.actor(m, None)).and_then(|(p, k)| Some((host.mesh(&p)?, k)));
    let (Some(c), Some((info, keys))) = (c, drawn) else {
        eprintln!("warning: Finn BMO: not drawn (its mesh or materials are not loaded)");
        return None;
    };
    let sk = SkinnedActor::new(info, &keys);
    let clips = host.clips_of(w, from)?;
    let machine = ActorMachine::new(bmo_movement(), sk.names.len(), clips);
    let left = rotation_rows(0.0, 90.0 * BMO_DEG, 0.0);
    let right: Rows = std::array::from_fn(|k| if k % 3 == 1 { -left[k] } else { left[k] });
    Some(Box::new(BmoView {
        id,
        from,
        pose: Pose::new(sk.names.len()),
        sk,
        machine,
        inp: BmoInputs::default(),
        age: 0,
        rel: xyz(c.get("RelativeLocation"), 0.0),
        scale: xyz(c.get("RelativeScale3D"), 1.0),
        left,
        right,
        prev: [0.0; 3],
        cur: [0.0; 3],
        facing_left: false,
        stepped: false,
    }))
}

impl BmoView {
    /// The follower's inputs (BlueprintUpdateAnimation and the four bound events).
    fn inputs(&mut self, w: &World, owner: FighterId, b: &BmoActor) {
        if self.age <= FOLLOWER_TICKS {
            return; // IsValid(Fighter) @689: none until UpdateFollower
        }
        let f = w.fighter(owner);
        let i = &mut self.inp;
        i.is_falling = !f.body.on_floor;
        i.is_crouching = f.capsule.cmc_crouched;
        i.on_ground = f.body.on_floor;
        i.h_speed = f64::from(f.body.vx.abs());
        i.is_jumping = f.moves.cmd_jumped;
        i.jump_number = f.jump.jumps;
        i.wall_sliding = b.wall_sliding;
        i.fast_fall = f.fast_fall.fast_falling;
    }
}

impl ActorMesh for BmoView {
    /// One sim tick: the anim instance's update and pose, the actor's place at this tick.
    fn tick(&mut self, host: &mut ActorHost<'_>, w: &World) {
        let Some((x, b)) = bmo_of(w, self.id) else { return };
        let first = !self.stepped;
        self.stepped = true;
        self.age += 1;
        self.inputs(w, x.owner, b);
        let Some(clips) = host.clips_of(w, self.from) else { return };
        self.machine.update(&self.inp, STEP, clips);
        let sk = self.sk.skeleton(clips);
        self.machine.evaluate(clips, &sk, &mut self.pose);
        self.sk.show(&self.pose);
        // Follow_Y_Offset (@2712-2804), -100 while wall sliding (@5296)
        let depth = if b.wall_sliding { -100.0 } else { mvs_sim::fighter::cap_radius(w, x.owner) - 100.0 };
        self.prev = self.cur;
        self.cur = [f64::from(depth), f64::from(-b.x), f64::from(b.y)];
        if first {
            self.prev = self.cur;
        }
        self.facing_left = b.facing_left;
        self.sk.visible = b.visible && b.mesh_visible;
    }

    /// Each drawn frame: the mesh between the last two ticks' places.
    fn place(
        &mut self,
        _host: &mut ActorHost<'_>,
        w: &World,
        _at: &ActorPlace,
        alpha: f64,
        _shown: bool,
        out: &mut Vec<MeshPose>,
    ) {
        let Some((x, _)) = bmo_of(w, self.id) else { return };
        if !self.stepped {
            return;
        }
        // the root's scale over BMOMesh's relative place and scale
        let g = x.size_scale.unwrap_or(1.0);
        let loc =
            std::array::from_fn(|k| self.prev[k] + (self.cur[k] - self.prev[k]) * alpha + self.rel[k] * f64::from(g));
        let scale = std::array::from_fn(|k| self.scale[k] * f64::from(g));
        let rot = if self.facing_left { self.left } else { self.right };
        self.sk.set(&Xform { loc, rot, scale });
        self.sk.draw(out);
    }

    fn end(&mut self) {}
}
