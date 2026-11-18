//! Bugs Bunny's render module: the entry, the AnimBP's pose hook, his pawn's props and
//! the classes' mesh rules.
//!
//! Pawn_BugsBunny on the render side. His montages' effects (trails, PlayVFX, the hit effects, AnimNotify_SubMeshSpawn's
//! BugsBunny_Prop_TopHat of his intro emote) are the shared notify path's.
//!
//! The pose beside the shared one (BugsBunny_AnimBP). The graph, input first:
//! BlendListByBool (IsGliding AND VerticalVelocity < 500: child 0 BugsBunny_Nav_PropellerEars, child 1 the linked
//! layer) -> Slot DefaultSlot -> Slot Override -> LocalToComponent -> the physics chain (six tux-tail nodes,
//! AnimDynamics on carrot3 / carrot4, SpringBone on carrot3, four hair and two belt nodes) -> ComponentToLocal -> the
//! mirror node (BugsBunny_MirrorTableV2, bEnabled <- FacingLeft) -> the cached pose -> ModifyBone / _2 (HeadScale: the
//! entry's head_scale) -> Constraint_5, _4, _2, _3 (prop1) -> Constraint_1, Constraint (VB PropPickup) -> ModifyBone_1
//! / _4 / _3 (virtual bones) -> ComponentToLocal_1.
//!
//! Ported here:
//! - The glide BlendListByBool: its update (bActiveValue = IsGliding AND VerticalVelocity < 500, BlendTime [0.2, 0.1],
//!   the FAlphaBlend rule, the PropellerEars player at RateScale 1.5) is the sim's (mvs-sim chars/bugs: BugsState
//!   glide_w / glide_time, which also fire the player's notifies); the view reads its state after the tick and
//!   evaluates it here. Child 0 is the SequencePlayer of BugsBunny_Nav_PropellerEars, child 1 the linked layer (the
//!   shared pose: PoseCx::sample_layer, its slot mix and turn in it); a child at or under 1e-5 is not evaluated. The
//!   list sits under Slot DefaultSlot (bAlwaysUpdateSourcePose true), so it updates under a montage too; the curves of
//!   the evaluation are the children's by their weights (the glide clip carries none of showTeeth / showEyelids: 0).
//! - SpringBone on carrot3 (AnimGraphNode_SpringBone: stiffness 1000, damping 12, ErrorResetThresh 256, max
//!   displacement 10, limited; translate X and Z, rotate X, Y and Z), on the tick's final local pose (the chain is
//!   after the Slot nodes) before the mirror: the unmirrored pose, as the view holds it. Its alpha (@1189-1322) is
//!   float(!IsHitStunned AND !IsRespawning). IsHitStunned is false in frame order (SetHitStunned's binding never fires
//!   for him), so the alpha is !IsRespawning: SetIsRespawning (@6080) sets it, a Delay(0.1) (@6091) clears it (@911).
//!   The carrot mesh (BugsBunny_Carrot_SkelMesh, the Carrot sub-mesh) is weighted to carrot1-4 alone.
//! - The carrot under the mirror: BugsBunny_MirrorTableV2 has BoneFlipAxis None on carrot2, carrot3 and carrot4 alone,
//!   carrot1 AXIS_Y, so the node writes the three back unchanged on a mirrored carrot1. The view draws a left facing as
//!   the reflection of the right-facing pose (Unverified: the mirror node taken as a full reflection), which would
//!   mirror them too. A kept bone K under a mirrored parent P is, in component space (row vectors), K * F_P * T(P) *
//!   M_Y; the view draws P as T(P) reflected, so facing left carrot2's local becomes K * F_P (F_P = Y: the view's
//!   (1, 1, -1)): translation z, the quaternion's x and y and the scale's z negated. carrot3 / carrot4 keep their
//!   locals on carrot2.
//! - Teeth / Eyelids (@3647-3814): behind VisibilityManager.InitialVisibilitySet (true in a match from the pawn's
//!   first SetupInitialVisibility on), the AnimBP's own ToggleSubmeshVisibility (@0-453, threshold 0.9, not
//!   Source_AnimBP's 0.5): when (curve > 0.9) differs from the stored TeethVisible / EyelidsVisible (class defaults
//!   false), SetSubMeshVisibility(tag, curve > 0.9, true) and the stored value follows. The port's own state of the
//!   two parts over the sim's (sub_hidden: SetupInitialVisibility hides both, nothing in a match shows them), as
//!   Garnet's Visor. The respawn: ResetFighterLogic's VisibilityManager.Reset runs SetupInitialVisibility again, which
//!   hides both; ResetSubmeshVisibility (@5888-5957) resets only LastShowTeeth / LastShowEyelids (read by no graph) and
//!   the Prop1_PS-RightHand pair. So the own state is dropped at the respawn and the stored values are kept.
//!   The curves: the clips' own by the sequence the tick shows (the fighter's anim and anim time) and the view's
//!   crossfade, as Garnet's.
//!
//! Deviations and unverified models:
//! - Unverified: the spring's respawn gate: 6 ticks of alpha 0 from the tick the view sees the respawn (Delay(0.1) at
//!   60 Hz; when SetIsRespawning is called against the respawn's tick is not read).
//! - Deviation: the view places no dead fighter, so the respawn is seen as a gap in the ticks set() runs at (the
//!   effects are told of every dead tick).
//! - Unverified: hitpause: the mesh's CustomTimeDilation 0.005 (the anim update's DeltaTime) while the fighter's
//!   hitpause > 0, as Reindog's pose.
//! - Unverified: a curve's value in a crossfade and the one tick the update lags the evaluation: as Garnet's pose.
//! - Deviation: the carrot2 rule facing left gives carrot2 a negative local scale; the view's draw between two ticks
//!   blends a turn's two poses, so for the frames of a turn the carrot passes through a flat scale.
//!
//! Not ported: the AnimDynamics on carrot3 / carrot4 (linear limits 0 on all axes, Limited, angular limits 0, no
//! planes: it holds the body at its reference offset or does nothing, a native question); the tux-tail, hair and belt
//! Trail / SpringBone nodes (bones of the S03 / S04 skins: no vertex of the default skin's meshes is weighted to them);
//! ModifyBone_1 / _3 / _4 (identity and replace-by-1 on virtual bones); Constraint_2 to _5 (their alphas are the
//! curves prop1LPropL / prop1RPropL, which no sequence of his carries). Constraint_1 / Constraint (VB PropPickup to R /
//! L_PropOffset) are not nodes here: the view has no virtual bone, and what hangs on VB PropPickup is placed at
//! R_PropOffset reflected with the view (the data's virtualBones constraint).

use mvs_data::value::{Props, Value};
use mvs_sim::World;
use mvs_sim::actors::{ActorRef, Item, ItemExt, ItemId, Projectile, ProjectileExt, ProjectileId};
use mvs_sim::chars::bugs::state::bugs_of;
use mvs_sim::fighter::FighterId;
use mvs_sim::fighter::common::{actor_def, actor_def_owner, latent_frames};
use mvs_sim::fighter::match_side::hold_socket;
use mvs_sim::names::Name;
use mvs_vfx::math::{V3, rotation_rows};

use super::{CharRender, PawnActor, PawnCx, PoseHook, PoseOf, PreviewOf, PreviewStep};
use crate::ActorPose;
use crate::actor_view::{Comp, IDENT, StaticView, Xform, item_rows, place_at, place_under, rel_rows, xyz};
use crate::actors::{ActorComponents, ActorHost, ActorMesh, ActorPlace, Component, MeshRule};
use crate::anim::{Clips, HeadScaleNode, Pose, add_weighted_pose, normalize_rotations, weight_pose};
use crate::anim_nodes::{AnimNodeChain, AnimNodeSkel, UeXf, mesh_xf};
use crate::curves::{CurveKey, CurveTable, rich_curve};
use crate::frame::MeshPose;
use crate::view::{CharView, PoseCx, PoseRules};

pub(crate) const RENDER: CharRender = CharRender {
    // BugsBunny_AnimBP: ModifyBone_2 and ModifyBone (scale Additive, bone space, <- HeadScale; Pawn_BugsBunny @3996)
    head_scale: &[
        HeadScaleNode {
            node: "ModifyBone_2",
            bone: "fml_un_C_head",
            factor: 1.0,
            gated: false,
            moves: [0.0; 3],
            scale: true,
        },
        HeadScaleNode {
            node: "ModifyBone",
            bone: "fml_un_C_bbny_face_att",
            factor: 1.0,
            gated: false,
            moves: [0.0; 3],
            scale: true,
        },
    ],
    pose: Some(bugs_pose),
    pawn_actors: Some(pawn_props),
    ..CharRender::GENERIC
};

/// The float curves its AnimBP reads by name (GetCurveValue), in the order the rule indexes them.
const CURVES: CurveTable = CurveTable::new(&["showTeeth", "showEyelids", "showBrunhildeEars"]);
/// The sub-mesh keys, in the curves' order.
const PARTS: [&str; 3] = ["Teeth", "Eyelids", "brunhildeEars"];
/// The ears' index in PARTS: a part of the S03 skin alone, so the default skin's sim has no tag for it (hidden at its start
/// state until the AnimBP's first write).
const EARS: usize = 2;
/// Delay(0.1) at the 60 Hz tick.
const RESPAWN_TICKS: i32 = 6;
/// BugsBunny_AnimBP ToggleSubmeshVisibility @80.
const TOGGLE: f64 = 0.9;
/// The glide list's child 0 (mvs-sim chars/bugs glide GLIDE_SEQ).
const GLIDE: &str = "BugsBunny_Nav_PropellerEars";
/// ZERO_ANIMWEIGHT_THRESH.
const ZERO_W: f64 = 1e-5;
const CARROT2: &str = "carrot2";

/// AnimGraphNode_SpringBone of Default__BugsBunny_AnimBP_C, as the data has it.
fn carrot_spring() -> Props {
    let n = Value::Num;
    let b = Value::Bool;
    let o = |kv: Vec<(&str, Value)>| Value::Obj(kv.into_iter().map(|(k, v)| (k.to_owned(), v)).collect());
    let s = |v: &str| Value::Str(v.to_owned());
    let props = [
        ("SpringBone", o(vec![("BoneName", s("carrot3"))])),
        ("MaxDisplacement", n(10.0)),
        ("SpringStiffness", n(1000.0)),
        ("SpringDamping", n(12.0)),
        ("ErrorResetThresh", n(256.0)),
        ("bLimitDisplacement", b(true)),
        ("bTranslateX", b(true)),
        ("bTranslateY", b(false)),
        ("bTranslateZ", b(true)),
        ("bRotateX", b(true)),
        ("bRotateY", b(true)),
        ("bRotateZ", b(true)),
        ("AlphaInputType", s("EAnimAlphaInputType::Float")),
        ("Alpha", n(1.0)),
        ("AlphaScaleBias", o(vec![("Scale", n(1.0)), ("Bias", n(0.0))])),
        ("AlphaCurveName", s("None")),
        (
            "AlphaScaleBiasClamp",
            o(vec![
                ("bMapRange", b(false)),
                ("bClampResult", b(false)),
                ("bInterpResult", b(false)),
                ("Scale", n(1.0)),
                ("Bias", n(0.0)),
            ]),
        ),
        ("LODThreshold", n(-1.0)),
    ];
    props.into_iter().map(|(k, v)| (k.to_owned(), v)).collect()
}

/// `bugsPose`'s state on one view.
#[derive(Debug)]
struct BugsPose {
    carrot2: Option<usize>,
    /// Per part (Teeth, Eyelids) its index in the view's parts and its tag in the sim's sub_hidden.
    part: [Option<usize>; 3],
    tags: [String; 3],
    chain: AnimNodeChain,
    comp: UeXf,
    /// A fighter's view this tick (set ran, not slaved).
    live: bool,
    /// PoseOf::slaved: no AnimBP of its own (set stops after the respawn count).
    slaved: bool,
    left: bool,
    dilation: f64,
    /// Ticks of IsRespawning left.
    respawning: i32,
    /// The last tick set() ran at (a gap: the fighter was dead between).
    last_tick: Option<i32>,
    /// The curves: the sequence's (tgt), the evaluation's (cur) and the pose the running fade started from (from).
    tgt: [f64; 3],
    cur: [f64; 3],
    from: [f64; 3],
    fades: u32,
    /// The stored TeethVisible / EyelidsVisible, the AnimBP's last write of each part (-1 none, 0 hidden, 1 shown)
    /// and the sim's last visibility of it (-1 not seen).
    stored: [bool; 3],
    own: [i8; 3],
    sim_vis: [i8; 3],
    glide: Option<usize>,
    g: Pose,
    l: Pose,
    /// Child 0's weight (child 1's is 1 - gw) and the PropellerEars player's time (the sim's, read at set()).
    gw: f64,
    glide_time: f64,
    /// The pose of this tick is the list's (no montage).
    src_only: bool,
}

fn bugs_pose(view: &mut CharView, clips: &Clips, of: &PoseOf<'_>) -> Box<dyn PoseHook> {
    let carrot2 = view.rig.index(CARROT2);
    if carrot2.is_none() {
        eprintln!("warning: Bugs pose: bone missing: {CARROT2}");
    }
    let part = PARTS.map(|k| {
        let i = of.part_keys.iter().position(|p| p == k);
        if i.is_none() {
            eprintln!("warning: Bugs pose: the skin has no sub-mesh {k}");
        }
        i
    });
    let glide = clips.get(GLIDE);
    if glide.is_none() {
        eprintln!("warning: Bugs pose: clip missing: {GLIDE} (the glide pose is not drawn)");
    }
    let spring = carrot_spring();
    let skel = AnimNodeSkel { names: &view.rig.names, parents: &view.parents, rest: None };
    let mut chain = AnimNodeChain::new(&[("AnimNode_SpringBone", &spring)], &skel);
    // InitAnim's DeltaTime-0 tick
    chain.initialize();
    chain.update(0.0, &[1.0], 1.0);
    let n = view.rest.n;
    Box::new(BugsPose {
        carrot2,
        part,
        tags: PARTS.map(str::to_lowercase),
        chain,
        comp: UeXf::default(),
        live: false,
        slaved: of.slaved,
        left: false,
        dilation: 1.0,
        respawning: 0,
        last_tick: None,
        tgt: [0.0; 3],
        cur: [0.0; 3],
        from: [0.0; 3],
        fades: 0,
        stored: [false; 3],
        own: [-1; 3],
        sim_vis: [-1; 3],
        glide,
        g: Pose::new(n),
        l: Pose::new(n),
        gw: 0.0,
        glide_time: 0.0,
        src_only: false,
    })
}

impl PoseRules for BugsPose {
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
        let w = match self.glide {
            Some(_) if self.live => self.gw,
            _ => 0.0,
        };
        if w <= ZERO_W {
            cx.sample_layer(clip, time, out);
            return true;
        }
        let Some(g) = self.glide else { return false };
        cx.sample(g, self.glide_time, &mut self.g);
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

    fn rule(&mut self, cx: &mut PoseCx<'_>, p: &mut Pose, dt: f64) {
        if self.live {
            // the update of the tick: the chain's alpha and its time (the anim update's DeltaTime under the dilation)
            let a = if self.respawning > 0 { 0.0 } else { 1.0 };
            self.chain.update(dt * self.dilation, &[a], 1.0);
            self.chain.evaluate(p, &self.comp);
        }
        // the mirror node's kept carrot2 on the mirrored carrot1 (the header)
        if self.left
            && let Some(c) = self.carrot2
        {
            let (t, r) = (c * 3, c * 4);
            p.t[t + 2] = -p.t[t + 2];
            p.r[r] = -p.r[r];
            p.r[r + 1] = -p.r[r + 1];
            p.s[t + 2] = -p.s[t + 2];
        }
        // this evaluation's curves, for the next update
        if cx.fade_count != self.fades {
            self.fades = cx.fade_count;
            self.from = self.cur;
        }
        // the list's curves: the children's by their weights
        if self.live
            && self.src_only
            && let Some(g) = self.glide
            && self.gw > ZERO_W
        {
            for k in 0..self.tgt.len() {
                let v = CURVES.value_of(cx.clips, Some(g), k, self.glide_time);
                self.tgt[k] = v * self.gw + self.tgt[k] * (1.0 - self.gw);
            }
        }
        let w = cx.fade_weight;
        for k in 0..self.cur.len() {
            self.cur[k] = if w >= 1.0 { self.tgt[k] } else { self.from[k] + (self.tgt[k] - self.from[k]) * w };
        }
    }
}

impl BugsPose {
    /// The AnimBP's ToggleSubmeshVisibility of part `k` on the curves of the evaluation before: when (curve > 0.9)
    /// differs from the stored value, the part's write and the stored value follow.
    fn toggle(&mut self, k: usize) {
        let on = self.cur[k] > TOGGLE;
        if on != self.stored[k] {
            self.stored[k] = on;
            self.own[k] = i8::from(on);
        }
    }
}

impl PoseHook for BugsPose {
    /// `preview()`: a view with no fighter (no spring, facing right). BP_CharacterPreview has a VisibilityManager
    /// and calls SetupInitialVisibility (@8754), so the toggles run (preview_step).
    fn preview(&mut self, _view: &mut CharView, _of: &PreviewOf<'_>) {
        self.live = false;
        self.left = false;
        self.stored = [false; 3];
        self.own = [-1; 3];
    }

    /// The preview's update: the toggles behind IsValid(VisibilityManager) and InitialVisibilitySet only (no Fighter
    /// read before them), on the curves of the evaluation before; the AnimBP is the parts' only writer here (the
    /// ears included). Then the curves of the frame's sequence.
    fn preview_step(&mut self, view: &mut CharView, clips: &Clips, s: &PreviewStep<'_>) {
        for k in 0..PARTS.len() {
            let Some(pi) = self.part[k] else { continue };
            self.toggle(k);
            if self.own[k] >= 0 {
                view.set_part_visible(pi, self.own[k] == 1);
            }
        }
        CURVES.fill(clips, s.anim, s.time, &mut self.tgt);
    }

    fn set(&mut self, w: &World, f: FighterId, view: &mut CharView, clips: &Clips) {
        let fi = w.fighter(f);
        self.live = !self.slaved;
        let facing = fi.body.facing_dir;
        self.left = facing < 0.0;
        self.dilation = if fi.hit.hitpause > 0.0 { f64::from(mvs_sim::consts::HITPAUSE_DILATION) } else { 1.0 };
        self.comp = mesh_xf(
            f64::from(-mvs_sim::fighter::mesh_wx(w, f)),
            f64::from(mvs_sim::fighter::mesh_wy(w, f)) + view.mesh_z * view.size_scale,
            f64::from(facing),
            view.mesh_scale * view.size_scale,
        );
        // the respawn: SetIsRespawning, and SetupInitialVisibility's hide of both parts
        let tick = w.flow.tick;
        if self.last_tick.is_some_and(|t| tick > t + 1) {
            self.respawning = RESPAWN_TICKS;
            self.own = [-1; 3];
        } else if self.respawning > 0 {
            self.respawning -= 1;
        }
        self.last_tick = Some(tick);
        if !self.live {
            return;
        }
        // the glide list after the tick's update
        let s = bugs_of(w, f);
        self.gw = s.map_or(0.0, |s| f64::from(s.glide_w));
        self.glide_time = s.map_or(0.0, |s| f64::from(s.glide_time));
        // BlueprintUpdateAnimation on the curves of the evaluation before: the two toggles
        for k in 0..PARTS.len() {
            let Some(pi) = self.part[k] else { continue };
            self.toggle(k);
            // the AnimBP's write until the sim changes the part's visibility (the later write)
            let hidden = w.names.get(&self.tags[k]).is_some_and(|n| fi.pose.sub_hidden.contains(&n));
            let vis = i8::from(!hidden);
            if self.sim_vis[k] >= 0 && self.sim_vis[k] != vis {
                self.own[k] = -1;
            }
            self.sim_vis[k] = vis;
            if k == EARS && self.own[k] < 0 {
                // the sim has no tag for the ears: the part stays at its start state (hidden) until the AnimBP writes
                continue;
            }
            view.set_part_visible(pi, if self.own[k] >= 0 { self.own[k] == 1 } else { vis == 1 });
        }
        // the sequence this tick's pose is sampled from
        CURVES.fill(clips, w.names.str(fi.anim.anim), f64::from(fi.anim.anim_time), &mut self.tgt);
    }

    fn shown(&self, tag: &str, sim: bool) -> bool {
        match PARTS.iter().position(|p| p.eq_ignore_ascii_case(tag)) {
            Some(k) if self.own[k] >= 0 => self.own[k] == 1,
            _ => sim,
        }
    }
}

// ---- his pawn's props ----
//
// Pawn_BugsBunny's StaticMeshComponents, children of CharacterMesh0 in its SCS (the data's `socket` is the node's
// AttachToName): AcmeBox, Breifcase, Brush and Rocket (yaw -90) on fml_un_C_prop1, PieProp1 and Target (yaw
// -89.99994) on the socket Prop1, Pie (scale 1.25) on L_Holster. Each is in Comp_Pawn_MeshManager.Meshes under its tag
// (the sim's static sub-meshes), hidden by SetupInitialVisibility and shown by the montages' AnimNotify_Equip /
// AnimNotify_SetSubmeshVisibility: the sim keeps the state (sub_hidden). The skeletal props (Mallet, Bat, Carrot,
// Teeth, Eyelids) are skin sub-meshes on the body's pose: the shared view draws them.
// Materials: the default skin's SubMeshes entries (the static Mesh and its Red / Blue / LocalPlayer overrides;
// Comp_Pawn_SkinManager.ApplySubMeshes puts SelectMaterialColor's pick on (@1405-1514): the fighter's skin variant, as
// his body's), keyed 'BugsBunny.<key>' in the fighter's draws. Pie and PieProp1 have the local instance in every
// variant (their entries name no team override).
// The Pie: OnFacingChange attaches it to FindMirrorSocket(HolsterSlotName) (L_Holster; R_Holster facing left) with
// relative scale 1.25 (mvs-sim chars/bugs/pawn.rs: the view's), by the fighter's facing each tick.
// Brush: Pawn_BugsBunny UserConstructionScript @178-269 sets its material scalar Team = ColorIndex.
// Unverified: the construction script's Team write against the skin's material set (ApplySubMeshes runs later and
// makes new instances): taken as the Team of every set the Brush wears.
// Unverified: the sockets are the last presented pose's.
// Another skin: its static entry of the tag, else the template's mesh in its own materials, no team pick
// (ActorHost::skin_static). Deviation: no first draw of the props is made under the
// loading screen: the match warm-up makes their GPU objects instead
// (ViewPoser::warm_meshes, mvs-render Renderer::prewarm_match).

/// The props' keys (the skin's SubMeshes, as the draws key them: 'BugsBunny.<key>').
const PROP_KEYS: [&str; 7] = ["Rocket", "AcmeBox", "Breifcase", "Target", "Pie", "Brush", "PieProp1"];
/// The one the pawn attaches to the mirrored socket by facing.
const MIRRORED: &str = "Pie";
const TEAM_SCALAR: &str = "Brush";
/// OnFacingChange's relative scale @906.
const PIE_SCALE: f64 = 1.25;
const ITEM_MANAGER: &str = "Comp_ItemManager_GEN_VARIABLE";
/// Comp_Pawn_MeshManager's role tags.
const MESH_ROLE_TAGS: [&str; 4] = ["RecieveArmor", "IgnoreArmor", "VisMesh", "PrimaryMesh"];

/// One prop: its tag in sub_hidden (lower case), its view, its socket (None: the Pie's holster) and its relative
/// transform.
#[derive(Debug)]
struct PawnProp {
    tag: String,
    v: StaticView,
    socket: Option<String>,
    rel: Xform,
}

impl PawnActor for PawnProp {
    fn present(&mut self, host: &ActorHost<'_>, w: &World, cx: &PawnCx<'_>, _alpha: f64, out: &mut Vec<ActorPose>) {
        // AddInvisibility hides every mesh of the manager; a dead fighter's view is not drawn
        if !cx.shown(w, &self.tag) {
            return;
        }
        let (socket, rel) = match &self.socket {
            Some(s) => (s.clone(), self.rel),
            None => (pie_holster_socket(w, cx.fid), Xform { scale: [PIE_SCALE; 3], ..self.rel }),
        };
        let Some(at) = host.socket(w, cx.fid, &socket, &rel) else { return };
        self.v.set(&at);
        let mut meshes = Vec::new();
        self.v.draw(&mut meshes);
        cx.outline(true, &mut meshes);
        out.push(ActorPose { meshes, actor: None, bones: Vec::new(), ..ActorPose::default() });
    }
}

/// `socket` with every `from` (any case) replaced by `to`.
fn replace_any_case(socket: &str, from: &str, to: &str) -> String {
    let lower = socket.to_lowercase();
    let mut out = String::new();
    let mut at = 0;
    while let Some(i) = lower[at..].find(from) {
        out.push_str(&socket[at..at + i]);
        out.push_str(to);
        at += i + from.len();
    }
    out.push_str(&socket[at..]);
    out
}

/// `findMirrorSocket` (Pawn_BugsBunny): the L_ / R_ twin of a socket facing left where it exists, else the socket
/// itself; None (no socket) where neither exists.
fn find_mirror_socket(socket: &str, facing_left: bool, exists: impl Fn(&str) -> bool) -> String {
    let lower = socket.to_lowercase();
    let (has_l, has_r) = (lower.contains("l_"), lower.contains("r_"));
    let cand = if !facing_left {
        socket.to_owned()
    } else if has_l {
        replace_any_case(socket, "l_", "R_")
    } else {
        replace_any_case(socket, "r_", "L_")
    };
    let (cand_ok, orig_ok) = (exists(&cand), exists(socket));
    if !(cand_ok || orig_ok) {
        return "None".to_owned();
    }
    if (has_l || has_r) && cand_ok { cand } else { socket.to_owned() }
}

/// `pieHolsterSocket`: FindMirrorSocket(Comp_ItemManager.HolsterSlotName) by the fighter's facing.
fn pie_holster_socket(w: &World, f: FighterId) -> String {
    let d = &w.char_data(f).fighter;
    let sk = &d.skeleton;
    let exists = |n: &str| sk.sockets.contains_key(n) || sk.mirror.contains_key(n);
    let comps = &d.components;
    let cm = comps.pawn.get(ITEM_MANAGER).or_else(|| comps.base.get(ITEM_MANAGER));
    let slot = cm.and_then(|c| c.p.as_obj()).and_then(|p| p.get("HolsterSlotName")).and_then(Value::as_str);
    find_mirror_socket(slot.unwrap_or("None"), w.fighter(f).body.facing_dir < 0.0, exists)
}

/// `pawnProps`: Pawn_BugsBunny's static sub-meshes (the static sub-meshes: a StaticMeshComponent with
/// a Meshes tag and no UI tag), in the data's order.
fn pawn_props(host: &ActorHost<'_>, w: &World, f: FighterId) -> Vec<Box<dyn PawnActor>> {
    let mut out: Vec<Box<dyn PawnActor>> = Vec::new();
    let d = &w.char_data(f).fighter;
    let colour = host.colour(w, f);
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
        let Some(key) = PROP_KEYS.iter().find(|k| k.to_lowercase() == tag) else {
            eprintln!("warning: Bugs: a static sub-mesh with no prop entry: {tag}");
            continue;
        };
        // (propLook: the default skin's own entries, another skin's static entry of the tag, else the template's)
        let st = if host.on_default_skin(w, f) {
            Some((format!("BugsBunny.{key}"), colour.variant.key()))
        } else {
            host.skin_static(w, f, p)
        };
        let Some(mut v) = st.and_then(|(k, variant)| StaticView::new(host, w, f, &k, variant)) else {
            eprintln!("warning: Bugs: the prop {key} is not drawn (its mesh or materials are not loaded)");
            continue;
        };
        if *key == TEAM_SCALAR {
            v.set_scalar("Team", f64::from(colour.color_index));
        }
        let rel = Xform {
            loc: xyz(p.get("RelativeLocation"), 0.0),
            rot: rel_rows(p.get("RelativeRotation")),
            scale: xyz(p.get("RelativeScale3D"), 1.0),
        };
        let socket = (*key != MIRRORED).then(|| c.socket.clone().unwrap_or_else(|| "None".to_owned()));
        out.push(Box::new(PawnProp { tag, v, socket, rel }));
    }
    out
}

// ---- his classes' meshes ----
//
// The classes:
//   PieProjectile_C (ParentProjectilePhysics): StaticMesh = BugsBunny_Prop_Pie under the Capsule, pitch -90, scale
//     1.5.
//   BugsBunnySafe_C (SpawnedActorParent): Visual = BugsBunny_Prop_Safe at (-100, 0, -170), yaw -90, scale
//     (1, 0.35, 1) under the Capsule.
//   BugsBunny_SafeDrop_C (SpawnedActorParent): Visual = BugsBunny_Prop_Target under the Capsule, no relative
//     transform.
//   BugsBunnyRocketFProjectile_C / BugsBunnyRocketUpProjectile_C: StaticMesh = BugsBunny_Prop_Rocket_02 (F: at
//     (-10, 0, 0), yaw -90; Up: at (-230, 0, 0), pitch 90, yaw -90; both scale 0.925, bVisible false).
//   Those five classes' material: SpawnedActorParent SetTeamColor (BeginPlay; the overrides call it first) puts the
//     instigator's pick of the class default's Materials set on slot 0 of the StaticMesh, over the template's
//     OverrideMaterials (ActorHost::selected_variant). The view takes it once, when it is made, from the owner, which
//     is the instigator then (the safe's knock moves the owner later, the sim's instigator override keeping the
//     instigator). A reflect calls SetOwner alone before SetTeamColor again, so its colour stays the instigator's, as
//     this view's. BugsBunnySafe's ApplyLocalUserColor (a dynamic instance with User Color = the Me palette colour on a
//     locally controlled instigator, @21590-21824): ActorHost::apply_local_user_colour, once with the view.
//   Item_Dynamite_C (ItemActorParent): Geo = BugsBunny_Prop_Dynamite under the Capsule, roll -90, scale 0.8.
//   BugsBunnyNeutralSafe_C (BugsBunnySafe's child, FFAGameMode's centre actor): the same Visual and graph, drawn from
//     the items part's draw. SetTeamColor's material needs an Instigator with a Comp_Pawn_SkinManager
//     (SpawnedActorParent @22391-22784); the area spawns it with none, and a hit's knock moves only its owner, so it
//     keeps the template's OverrideMaterials (BugsBunnyNeutral_Damaged_MAT_Inst) for its life, whoever hit it.
// Unverified: a bot's safe stays untinted (ActorHost::apply_local_user_colour).
// The neutral safe, and a safe, pie or dynamite whose class no fighter of the match has, are drawn from the items
// part's draws (StaticView::item). Deviation: no first draw of the meshes is made under the loading screen:
// the match warm-up makes their GPU objects instead (ViewPoser::warm_meshes, mvs-render Renderer::prewarm_match).

const PIE_MESH: &str = "BugsBunny_Prop_Pie";
const SAFE_MESH: &str = "BugsBunny_Prop_Safe";
const DROP_MESH: &str = "BugsBunny_Prop_Target";
const ROCKET_MESH: &str = "BugsBunny_Prop_Rocket_02";
const DYNAMITE_MESH: &str = "BugsBunny_Prop_Dynamite";
/// PieProjectile's material entry (its set), drawn with PIE_MESH; the Nepter's pie keeps PIE_MESH's entry.
const PIE_KEY: &str = "PieProjectile_C.StaticMesh";
const PIE: &str = "PieProjectile_C";
const NEPTR_PIE: &str = "Projectile_Neptr_Pie_C";
const SAFE: &str = "BugsBunnySafe_C";
const NEUTRAL_SAFE: &str = "BugsBunnyNeutralSafe_C";
const SAFE_DROP: &str = "BugsBunny_SafeDrop_C";
const ROCKET_F: &str = "BugsBunnyRocketFProjectile_C";
const ROCKET_UP: &str = "BugsBunnyRocketUpProjectile_C";
const DYNAMITE: &str = "Item_Dynamite_C";

const DEG: f64 = std::f64::consts::PI / 180.0;

/// The classes' own mesh drawing, by class name.
pub(crate) const RULES: &[(&str, MeshRule)] = &[
    (PIE, MeshRule { rot: None, mesh: Some(pie_mesh) }),
    (NEPTR_PIE, MeshRule { rot: None, mesh: Some(pie_mesh) }),
    (SAFE, MeshRule { rot: None, mesh: Some(safe_mesh) }),
    (NEUTRAL_SAFE, MeshRule { rot: None, mesh: Some(safe_mesh) }),
    (SAFE_DROP, MeshRule { rot: None, mesh: Some(drop_mesh) }),
    (ROCKET_F, MeshRule { rot: None, mesh: Some(rocket_mesh) }),
    (ROCKET_UP, MeshRule { rot: None, mesh: Some(rocket_mesh) }),
    (DYNAMITE, MeshRule { rot: None, mesh: Some(dynamite_mesh) }),
];

/// Math.fround.
fn f32r(v: f64) -> f64 {
    v as f32 as f64
}

/// A component of the class, as a placed mesh component.
fn comp(c: &Component) -> Comp {
    Comp { par: c.parent.clone(), rl: c.loc, rot: c.rot, rs: c.scale }
}

/// A timeline's position k frames after its PlayFromStart: the update at 0 inside the call, then position + delta in
/// float32 from the next frame on, held at the length (as the dropped item's DeathBlinkWarning timeline).
fn timeline_at(k: i32, length: f64) -> f64 {
    let mut t = 0.0;
    let mut i = 0;
    while i < k && t < length {
        t = f32r(t + f64::from(mvs_sim::consts::DELTA_SECONDS));
        i += 1;
    }
    t.min(length)
}

/// `teamView`: a SetTeamColor actor's StaticMesh view: the entry `key` of the class data's owner, in the instigator's
/// variant, with the class's ApplyLocalUserColor.
fn team_view(host: &ActorHost<'_>, w: &World, pr: &Projectile, key: &str) -> Option<StaticView> {
    let owner = pr.owner?;
    let from = actor_def_owner(w, owner, pr.cls);
    let mut v = StaticView::new(host, w, from, key, host.selected_variant(w, owner))?;
    if let Some(def) = actor_def(w, ActorRef::Fighter(owner), pr.cls) {
        host.apply_local_user_colour(w, &mut v, &def.p, owner);
    }
    Some(v)
}

/// One StaticMesh component under the actor's root, placed by its template (the pie, the drop target).
#[derive(Debug)]
struct PlainMesh {
    view: StaticView,
    c: Comp,
    /// The actor's rotation (false: rotation 0, the drop target's).
    turns: bool,
}

impl ActorMesh for PlainMesh {
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
        let rot = if self.turns { at.rot } else { IDENT };
        self.view.set(&place_under(&self.c, &rot, at.scale, x, y));
        self.view.draw(out);
    }
}

/// `pieMesh`: PieProjectile_C in its team set; Projectile_Neptr_Pie_C (the Nepter's pie, its own transform from the
/// class data: scale .75) with the mesh's own material (its Materials set is ParentProjectilePhysics' empty one).
fn pie_mesh(host: &mut ActorHost<'_>, w: &World, a: ActorRef, cs: &ActorComponents) -> Option<Box<dyn ActorMesh>> {
    let ActorRef::Projectile(id) = a else { return None };
    let pr = w.actors.projectiles.get(id)?;
    let c = comp(cs.get("StaticMesh")?);
    let view = if w.names.str(pr.cls) == PIE {
        team_view(host, w, pr, PIE_KEY)
    } else {
        // (a pie whose owner's data lacks the class: the items part's draw)
        pr.owner
            .and_then(|o| StaticView::new(host, w, actor_def_owner(w, o, pr.cls), PIE_MESH, ""))
            .or_else(|| StaticView::item(host, &format!("{}.StaticMesh", w.names.str(pr.cls)), "template"))
    };
    let Some(view) = view else {
        eprintln!("warning: Bugs: {} is not drawn (its mesh or materials are not loaded)", w.names.str(pr.cls));
        return None;
    };
    Some(Box::new(PlainMesh { view, c, turns: true }))
}

/// The safe drop's target: placed on the ground at BeginPlay and still (mvs-sim chars/bugs/safe.rs SafeDrop),
/// rotation 0.
fn drop_mesh(host: &mut ActorHost<'_>, w: &World, a: ActorRef, cs: &ActorComponents) -> Option<Box<dyn ActorMesh>> {
    let ActorRef::Projectile(id) = a else { return None };
    let pr = w.actors.projectiles.get(id)?;
    let c = comp(cs.get("Visual")?);
    let Some(view) = team_view(host, w, pr, DROP_MESH) else {
        eprintln!("warning: Bugs: the safe drop's target is not drawn (its mesh or materials are not loaded)");
        return None;
    };
    Some(Box::new(PlainMesh { view, c, turns: false }))
}

// ---- the safe ----
// Damage: BeginPlay @3427 Visual.SetScalarParameterValueOnMaterials(Damage, 0); HandleDamageTaken @6690-6866
// SetSafeScalarParm(1 - FMin(CurrentHP, MaxHP) / MaxHP) -> @8678-8705 the same call with that value. CurrentHP
// changes at those calls alone (the Hitbox's @4660 and ReceiveAnyDamage's @9682 / @6603), so the value is the sim's
// hp read each frame (0 at MaxHP, as BeginPlay's).
// SafeDamageShake (@6942, every damage that leaves HP > 0: the sim's shake_age) -> Timeline_0.PlayFromStart @8760
// (length 0.25; tracks Shake CurveFloat_1_2 and Scale CurveFloat_2_3 of the class export). Update @8793-9055:
// Visual.K2_SetRelativeRotation(MakeRotator(Roll 0, Pitch Shake x 10, Yaw -90)), Visual.SetRelativeScale3D(
// (1 + Scale x 0.1) on all three axes); Finished @9097-9313: rotation (0, 0, -90), scale 1 + Scale(0.25) x 0.1 = 1.
// So from its first shake on the Visual's scale is uniform (the template's (1, 0.35, 1) is gone).
// The actor's rotation: none to draw. It spawns at rotation 0 (CompSpawnActor @2786), its KnockbackSafe and landing
// set rotation 0, and the simulated body has its X rotation (the play plane's) locked (mvs-sim chars/bugs/safe.rs).
// Team: SetTeamColor @10702 calls SpawnedActorParent's (the material, header); its own part (@10712-10988) writes
// ColorIndex alone, which no node of the class reads.
// Unverified: carried through a tunnel the safe is hidden by Buff_ProjectileThroughTunnel as every projectile (the
// shared path's shown).
const SHAKE_KEYS: &[CurveKey] = &[
    [0.0054850057, 1.0, 0.0, 0.0, 1.0],
    [0.04346025, -0.74610233, -0.00907064, -0.00907064, 1.0],
    [0.08437598, 0.4906066, -0.095027536, -0.095027536, 1.0],
    [0.17529964, -0.22967446, -0.002945157, -0.0029449067, 1.0],
    [0.24803853, 0.0, 0.0, 0.0, 0.0],
];
const SHAKE_SCALE_KEYS: &[CurveKey] = &[
    [0.0, 0.69790447, 0.0, 0.0, 0.0],
    [0.05651951, 1.0066482, 0.004252692, 0.004252694, 1.0],
    [0.18622077, -0.30012763, 0.117506444, 0.11750604, 1.0],
    [0.25, 0.0, 0.0, 0.0, 0.0],
];
const SHAKE_LENGTH: f64 = 0.25;

#[derive(Debug)]
struct SafeMesh {
    id: ProjectileId,
    view: StaticView,
    c: Comp,
    dmg: f64,
}

fn safe_mesh(host: &mut ActorHost<'_>, w: &World, a: ActorRef, cs: &ActorComponents) -> Option<Box<dyn ActorMesh>> {
    let ActorRef::Projectile(id) = a else { return None };
    let pr = w.actors.projectiles.get(id)?;
    let c = comp(cs.get("Visual")?);
    // (the neutral safe, and a safe whose class no character of the match has, a Mystery box's: the items part's
    // draw, in the thrower's variant)
    let cls = w.names.str(pr.cls);
    let view = if cls == NEUTRAL_SAFE {
        StaticView::item(host, &format!("{NEUTRAL_SAFE}.Visual"), "template")
    } else {
        team_view(host, w, pr, SAFE_MESH).or_else(|| safe_item_view(host, w, pr))
    };
    let Some(mut view) = view else {
        eprintln!("warning: Bugs: {cls} is not drawn (its mesh or materials are not loaded)");
        return None;
    };
    view.set_scalar("Damage", 0.0); // BeginPlay @3427
    Some(Box::new(SafeMesh { id, view, c, dmg: 0.0 }))
}

/// `safeItemView`: the items part's safe in its thrower's variant and local user colour.
fn safe_item_view(host: &ActorHost<'_>, w: &World, pr: &Projectile) -> Option<StaticView> {
    let owner = pr.owner?;
    let key = format!("{}.Visual", w.names.str(pr.cls));
    let mut v = StaticView::item(host, &key, host.selected_variant(w, owner))?;
    if let Some(def) = actor_def(w, ActorRef::Fighter(owner), pr.cls) {
        host.apply_local_user_colour(w, &mut v, &def.p, owner);
    }
    Some(v)
}

impl ActorMesh for SafeMesh {
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
        let Some(pr) = w.actors.projectiles.get(self.id) else { return };
        let ProjectileExt::BugsBunnySafe(s) = &pr.ext else { return };
        if !shown {
            return;
        }
        let d = f32r(-f32r(f64::from(s.hp.min(s.max_hp) / s.max_hp)) + 1.0);
        if d != self.dmg {
            self.dmg = d;
            self.view.set_scalar("Damage", d);
        }
        let (x, y) = at.at(alpha);
        let xf = if s.shake_age < 0 {
            place_under(&self.c, &IDENT, f64::from(pr.scale), x, y)
        } else {
            let t = timeline_at(pr.age - s.shake_age, SHAKE_LENGTH);
            let k = f32r(1.0 + f32r(rich_curve(SHAKE_SCALE_KEYS, t) * 0.1));
            let own = rotation_rows(f32r(rich_curve(SHAKE_KEYS, t) * 10.0) * DEG, -90.0 * DEG, 0.0);
            place_at(&self.c, &own, [k; 3], &IDENT, f64::from(pr.scale), [0.0, -x, y])
        };
        self.view.set(&xf);
        self.view.draw(out);
    }
}

// ---- the rockets ----
// ReceiveBeginPlay's Delay(0.067) -> RocketF @125 / RocketUp @130 StaticMesh.SetVisibility(true): the sim's step of
// age AT_ON (mvs-sim chars/bugs/rockets.rs), so it is drawn from the frame of that age on. The actor turns with its
// velocity (bRotationFollowsVelocity: actor_rot).
// RocketF alone: BeginPlay @2012-2108 StartingScale = StaticMesh's relative scale (0.925); @554 RocketSquash.
// PlayFromStart in the same frame as @125 (length 0.75; tracks Squash CurveFloat_0 and Stretch CurveFloat_1 of the
// class export); its update @1415-1654 StaticMesh.SetRelativeScale3D((Squash x X, Stretch x Y, Squash x Z) of
// StartingScale); both curves end at 1 (0.75), the template's scale again. Its particle children follow the same
// scale (mvs-render fx/chars/bugs.rs takes rocket_squash_scale through the crate root).
const SQUASH_KEYS: &[CurveKey] = &[
    [0.0, 1.2810228, 0.0, 0.0, 0.0],
    [0.07346034, 1.5215311, -0.032049257, -0.03204902, 1.0],
    [0.23436952, 0.7746897, 0.060918227, 0.060918476, 1.0],
    [0.39508104, 1.1045985, 0.0, 0.0, 0.0],
    [0.5571071, 0.94987404, 0.10968197, 0.10968158, 1.0],
    [0.75, 1.0, 0.0, 0.0, 0.0],
];
const STRETCH_KEYS: &[CurveKey] = &[
    [0.0, 0.90127313, 0.0, 0.0, 0.0],
    [0.09039807, 0.6860815, -0.039961684, -0.039961703, 1.0],
    [0.20896268, 1.2050731, 0.007847665, 0.007847665, 1.0],
    [0.3995818, 0.9229655, 0.0, 0.0, 0.0],
    [0.5391042, 1.0440543, -0.036932856, -0.03693419, 1.0],
    [0.75, 1.0, 0.0, 0.0, 0.0],
];
const SQUASH_LENGTH: f64 = 0.75;

/// The age RocketF / RocketUp shows its StaticMesh at (Delay(0.067)).
pub fn at_on() -> i32 {
    latent_frames(0.067) as i32
}

/// `rocketSquashScale`: RocketF's StaticMesh relative scale at an age at or past AT_ON (`rs`: the template's).
pub fn rocket_squash_scale(age: i32, rs: V3) -> V3 {
    let t = timeline_at(age - at_on(), SQUASH_LENGTH);
    let (sq, st) = (rich_curve(SQUASH_KEYS, t), rich_curve(STRETCH_KEYS, t));
    [f32r(sq * rs[0]), f32r(st * rs[1]), f32r(sq * rs[2])]
}

#[derive(Debug)]
struct RocketMesh {
    id: ProjectileId,
    view: StaticView,
    c: Comp,
    squash: bool,
}

fn rocket_mesh(host: &mut ActorHost<'_>, w: &World, a: ActorRef, cs: &ActorComponents) -> Option<Box<dyn ActorMesh>> {
    let ActorRef::Projectile(id) = a else { return None };
    let pr = w.actors.projectiles.get(id)?;
    let c = comp(cs.get("StaticMesh")?);
    let Some(view) = team_view(host, w, pr, ROCKET_MESH) else {
        eprintln!("warning: Bugs: the rocket is not drawn (its mesh or materials are not loaded)");
        return None;
    };
    Some(Box::new(RocketMesh { id, view, c, squash: w.names.str(pr.cls) == ROCKET_F }))
}

impl ActorMesh for RocketMesh {
    fn place(
        &mut self,
        _host: &mut ActorHost<'_>,
        w: &World,
        at: &ActorPlace,
        alpha: f64,
        shown: bool,
        out: &mut Vec<MeshPose>,
    ) {
        let age = w.actors.projectiles.get(self.id).map_or(0, |p| p.age);
        let on = shown && age >= at_on();
        self.view.visible = on;
        if !on {
            return;
        }
        let (x, y) = at.at(alpha);
        let xf = if self.squash {
            let rs = rocket_squash_scale(age, self.c.rs);
            place_at(&self.c, &self.c.rot, rs, &at.rot, at.scale, [0.0, -x, y])
        } else {
            place_under(&self.c, &at.rot, at.scale, x, y)
        };
        self.view.set(&xf);
        self.view.draw(out);
    }
}

// ---- Item_Dynamite_C ----
// The Geo under the item's Capsule with the item's rotation (its facing's yaw and the pitch); held, on the holder's
// hold socket (as Batman's batarang). SetTeamColor @2669-2809: Geo.SetMaterial(0, IsLocalOnline ?
// BugsBunny_Props_Local_MIC : Round(colour value) 0 ? ..._Red_MIC : 1 ? ..._Blue_MIC) (ActorHost::variant).
// BoomMulticast @2232 Geo.SetVisibility(false, true): hidden from the detonation (the sim's detonated) to the release.
// Unverified: when the item's SetTeamColor runs is not read: the material follows the item's owner each frame.
// FFAGameMode's area spawns it with no owner and no spawner (its mesh from the items part's draw); with a spawner and
// no owner the colour value is 0 and not the local player's, so the Red MIC until a pickup gives it an owner.
// Pawn_BugsBunny's HoldSlotName is VB PropPickup, a virtual bone (source and target Root) that his AnimBP's
// Constraint_1 / Constraint nodes put on R_PropOffset / L_PropOffset: ActorHost::hold_place follows it there
// (crate::virtual_bone). A holder whose hold socket is None hides it.
// Deviation: the custom depth outline a held item gets from its grabber is not drawn.

#[derive(Debug)]
struct DynamiteMesh {
    id: ItemId,
    view: StaticView,
    c: Comp,
}

/// The item's material variant: its owner's, else Red.
fn dynamite_variant(host: &ActorHost<'_>, w: &World, it: &Item) -> &'static str {
    it.owner.map_or("red", |o| host.variant(w, o))
}

fn dynamite_mesh(host: &mut ActorHost<'_>, w: &World, a: ActorRef, cs: &ActorComponents) -> Option<Box<dyn ActorMesh>> {
    let ActorRef::Item(id) = a else { return None };
    let it = w.actors.items.get(id)?;
    let c = comp(cs.get("Geo")?);
    // (a random item's, with no spawner, a Mystery box's, or one whose spawner's data lacks the class: the items
    // part's)
    let variant = dynamite_variant(host, w, it);
    let view = it
        .spawner
        .and_then(|s| StaticView::new(host, w, actor_def_owner(w, s, it.cls), DYNAMITE_MESH, variant))
        .or_else(|| StaticView::item(host, "Item_Dynamite_C.Geo", variant));
    let Some(view) = view else {
        eprintln!("warning: Bugs: Item_Dynamite_C is not drawn (its mesh or materials are not loaded)");
        return None;
    };
    Some(Box::new(DynamiteMesh { id, view, c }))
}

impl ActorMesh for DynamiteMesh {
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
        let ItemExt::Dynamite(d) = &it.ext else { return };
        let hold = at.holder.map(|h| (h, hold_socket(w, h)));
        let no_socket = hold.as_ref().is_some_and(|(_, s)| s == "None");
        if no_socket {
            warn_no_socket();
        }
        let on = shown && !d.detonated && !no_socket;
        self.view.visible = on;
        if !on {
            return;
        }
        self.view.dress(host, dynamite_variant(host, w, it));
        // (socketFollow's `missing`, a bone the view lacks: the holder's mesh origin)
        let held = hold.and_then(|(h, s)| host.hold_place(w, h, &s));
        let xf = match held {
            Some(s) => place_at(&self.c, &self.c.rot, self.c.rs, &s.rot, f64::from(it.scale), s.loc),
            None => {
                let (x, y) = at.at(alpha);
                place_under(&self.c, &item_rows(it), f64::from(it.scale), x, y)
            }
        };
        self.view.set(&xf);
        self.view.draw(out);
    }
}

/// The warning of a held dynamite with no hold socket, once a run.
fn warn_no_socket() {
    static WARNED: std::sync::Once = std::sync::Once::new();
    WARNED.call_once(|| {
        eprintln!("warning: Item_Dynamite_C: a holder with no hold socket: the held item is hidden");
    });
}
