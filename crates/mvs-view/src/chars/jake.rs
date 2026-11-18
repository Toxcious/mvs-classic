//! Jake's render module: the entry, the AnimBP's pose hook, the weapons on
//! R_HoldPoint / L_HoldPoint and the classes' mesh rules (the stretch body's bottom half,
//! the house, the horse). The WeaponMaster perk's hand
//! emitters and the horse's snack systems are mvs-render's (fx/chars/jake.rs).
//!
//! The HUD: nothing his classes show is drawn by the build. JakeAimer's SkillShotIndicator is hidden at BeginPlay
//! (@1243 goto @768 SetHiddenInGame(true, true)), its Arrow is bVisible false and its Sphere (Engine Sphere,
//! AimPunchMat) bHiddenInGame in the SCS template, shown by nothing; JakeEatAlly_Aim's two indicators and
//! ProjectileAimer_SpitJake's are bVisible false and never shown; JakeHorse_Actor3's InputUI is reached only from
//! overlaps that never fire. The eat hold's stun bar is the shared struggle UI.
//! Nothing of its own to draw:
//! - The skate gear (SkateGear, Skateboard: AnimNotify_Equip in Attack_Skate) and TopHalf / Chair: the skin's
//!   sub-meshes, master-posed parts of the shared view, shown by the sim's sub-mesh state.
//! - BP_CharLights_Jake: no match spawns a CharLights rig.
//! - The DBM_Normal flipbooks (MI_2DEmote_*, MI_Flipbook_C005_Dance): the emotes' ToolTipPreviewMaterial in the
//!   Collection, no match effect.
//!
//! Drawn by mvs-render's effects as component meshes (fx/chars/jake.rs): the stretch body's spline meshes
//! (Jake_StretchMesh, S01_Stretch) and the aimers' AimSpline (SpitAimer on ProjectileAimer_SpitJake_C and
//! JakeEatAlly_Aim_C).
//! No first draw of the actors' meshes is made under the loading screen: the match's warm-up replaces it
//! (`ViewPoser::warm_meshes` lists every actor set of the draws, mvs-render's `Renderer::prewarm_match` makes them).
//! A view with no fighter (a preview actor): PoseHook::preview / preview_step, the two toggles on the shown clip's
//! curves (the preview's AnimBP has its owning actor's VisibilityManager), the weapons hidden.

use std::f64::consts::PI;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use indexmap::IndexMap;
use mvs_data::fighter::SlotGraphNode;
use mvs_data::fighter::{FighterData, Montage, PropMesh};
use mvs_sim::World;
use mvs_sim::actors::ActorRef;
use mvs_sim::actors::world_actor::WorldActorState;
use mvs_sim::chars::jake::anim::{EAT_CLIPS, EAT_WALK};
use mvs_sim::chars::jake::horse::JakeHorse;
use mvs_sim::chars::jake::house::JakeHouse;
use mvs_sim::chars::jake::state::jake_of;
use mvs_sim::chars::jake::stretch::Stretch;
use mvs_sim::fighter::anim::grouped::layers_into;
use mvs_sim::fighter::anim::montages::{instance_data, slot_uid};
use mvs_sim::fighter::anim::{AnimKey, MontageSlot};
use mvs_sim::fighter::{FighterId, shown_montage};
use mvs_sim::names::Name;
use mvs_vfx::math::{V3, compose_rows, rotation_rows};

use super::{CharRender, PawnComponents, PoseHook, PoseOf, PreviewOf, PreviewStep};
use crate::actor_anim::{
    ActorMachine, ActorSlot, BlendOption, MachineDef, MachineQuery, PoseDef, SeqDef, SpaceDef, StateDef, TransitionDef,
};
use crate::actor_view::{SkinnedActor, Xform};
use crate::actors::{ActorComponents, ActorHost, ActorMesh, ActorPlace, MeshRule, Rows};
use crate::anim::{Clips, Pose, accumulate_additive, blend_pose};
use crate::blend::BlendSpaceTable;
use crate::curves::CurveTable;
use crate::frame::MeshPose;
use crate::pose::{MeshMap, Rec};
use crate::skin_intro::warn_once;
use crate::skins::{FighterDraws, Variant};
use crate::view::{CharView, PoseCx, PoseRules};

pub(crate) const RENDER: CharRender = CharRender {
    // Jake_AnimBP's HeadScale nodes are gated by StretchOn / EatOverrideBool and two of them replace the component
    // scale, which the shared head scale step does not take: the pose hook applies them all.
    head_scale: &[],
    // the horse's AnimBP's blend space player (an actor's: the pose hook runs none)
    blend_spaces: Some(&HORSE_BLEND_SPACES),
    pose: Some(jake_pose),
    components: Some(PawnComponents { make: weapons_make, place: weapons_place, reset: None }),
    // The meshes his actors' graphs set that no SCS component holds: the house's other MeshSwap sub-meshes
    // (Jake_House is the SCS JakeHouse's). (The stretch body's and the aim arcs' spline meshes are mvs-render's
    // component meshes.)
    actor_meshes: &["Jake_Car", "Jake_Boat"],
    ..CharRender::GENERIC
};

/// The classes' own mesh drawing, by class name. JakeHorsePlatform_C has no mesh
/// component (Capsule, Collision, WallbounceCollision); the aimers' AimSpline is a spline mesh (mvs-render's
/// component meshes).
pub(crate) const RULES: &[(&str, MeshRule)] = &[
    (STRETCH_CLS, MeshRule { rot: None, mesh: Some(stretch_mesh) }),
    (HOUSE_CLS, MeshRule { rot: None, mesh: Some(house_mesh) }),
    (HORSE_CLS, MeshRule { rot: None, mesh: Some(horse_mesh) }),
];

/// The port's own: JakeHorse_Actor3's mesh AnimBP Jake_Horse_Skeleton_AnimBlueprint, its one BlendSpacePlayer: the
/// machine's Run state (X <- Speed): Horse_Run_BlendSpace, bound with Jake's load.
const HORSE_BLEND_SPACES: BlendSpaceTable =
    BlendSpaceTable { nodes: &[("horseRun", "AnimGraphNode_BlendSpacePlayer")] };

// ---- the pose ------------------------------------------------------------------------
//
// The graph's bone controls, input first (each node's LinkID is its index among the class's AnimNode_ properties):
// ModifyBone_13 (fml_un_COM, StretchOn: the sim's) -> ModifyBone_7 -> _4 -> _3 -> _1 -> Constraint_1 ->
// ModifyBone_2 -> ModifyBone -> ModifyBone_6 -> _5 -> Constraint -> ComponentToLocal_1 -> the mirror node (FacingLeft)
// -> LocalToComponent -> ModifyBone_12 -> _11 -> _8 -> _9 -> _10 -> ComponentToLocal -> the output.
//
// The head (every node below but _6 / _5 behind the alpha BoolToFloat(HeadScale.X > 1 AND NOT StretchOn AND NOT
// EatOverrideBool), the exposed-input handlers @3818-6531; the alpha is 0 or 1, and at 0 the node is not evaluated):
//   ModifyBone_7 fml_un_C_spine_b: scale BMM_Additive in bone space <- HeadScale x 0.6 (@6238).
//   ModifyBone_4 fml_un_C_spine_d: translation Additive in bone space (-15, 0, 0); scale Additive in component space
//     <- HeadScale x 0.65 (@5751).
//   ModifyBone_3 fml_un_C_neck_a: translation Additive in bone space (-20, 0, 30); scale BMM_Replace in component
//     space <- HeadScale x 0.9 (@5385).
//   ModifyBone_1 fml_un_C_Face_att: translation Additive in bone space (-5, 0, 5), scale ignored (@4342).
//   Constraint_1 / ModifyBone_2 (fml_un_C_Cake_face_att: the head's translation; scale Replace in component space <-
//     HeadScale x 0.9, @4630 / @5019) and ModifyBone (fml_un_origins_C_Face_att, the same scale, @3818): not ported:
//     both bones hang from fml_un_COM and no vertex of the default skin's meshes (body, Brows, Eyelids, TopHalf,
//     SkateGear) is weighted to them or a bone under them (the Cake and Origins skins' face bones).
// Always (no handler, alpha 1): ModifyBone_6 / _5 fml_un_L / R_uparm_att: scale BMM_Replace in component space
//   (1, 1, 1): the arms keep their size under the scaled spine. Constraint (fml_un_origins_C_Face_att to the head,
//   translation and rotation): not ported (the bone carries no vertex of the default skin, above).
// After the mirror node (constants, no handler): ModifyBone_12 / _11 / _8 / _9 (VB L_HoldPoint, VB R_HoldPoint,
//   VB L_LowLeg, VB R_LowLeg: scale Replace in world space (1, 1, 1)) and ModifyBone_10 (VB Spine: rotation and scale
//   Replace in world space): not ported: the view holds no virtual bones; the weapons' socket R_HoldPoint is on
//   fml_un_R_wrist, not on a virtual bone, and the sockets on VB L / R_LowLeg are named by no notify of his montages
//   that the port draws.
//
// The math: in bone space the node's transform is taken relative to the bone's component-space transform before the
// node (FAnimationRuntime::ConvertCSTransformToBoneSpace reads the pose the node was given), so its translation goes
// through the bone's own rotation and scale before the node's scale; the bones under it follow
// (LocalBlendCSBoneTransforms). A component-space Replace of the scale sets the bone's component scale: its local
// scale becomes the value over its parent's component scale (componentwise products down the chain, the FTransform
// rule). A component-space Additive scale of a uniform value is the bone-space one. The nodes sit before the mirror
// node: the port applies them to the right-facing pose that the view reflects.
//
// The sub-mesh toggles: BlueprintUpdateAnimation @7486..@7759 ToggleTeethVisibility(showEyebrows, Brow, BrowsVisible)
// and (showEyelids, Eyelids, EyelidsVisible) (@0-553: behind IsValid(VisibilityManager), threshold 0.9; class defaults
// false): when (curve > 0.9) differs from the stored value, SetSubMeshVisibility(tag, curve > 0.9, true). The port's
// own state of the two parts over the sim's (sub_hidden) until the sim changes the part, the own state dropped at the
// respawn. The curves: the clips' own (crate::curves::CurveTable) by the sequence the tick shows and the view's
// crossfade.
//
// The weapons (JakeWeapon1-3, the skin's sub-meshes): drawn as the pawn's components on R_HoldPoint (below); the
// view's bone-bound parts of them are kept hidden here (they would draw at fml_un_C_prop1 of Jake's own pose, near
// his feet).
//
// ModifyBone_13 (fml_un_COM, the graph's first node over every montage and slot): BMM_Replace in component space,
// Rotation MakeRotator(0, StretchDirection, 0), Alpha BoolToFloat(StretchOn) (@2898-3018): the turn the sim gives the
// shapes (Fighter.turnBoxes by StretchDirection), here the view's turn_com of the same pitch, at the top of the pose
// rule before the head nodes (which hang below it in the chain; their alpha is 0 while StretchOn).
//
// The EatOverride machine (StateMachine_1 under BlendListByBool_1 on EatOverrideBool, the default skin): the sim runs
// it (mvs-sim chars/jake/anim.rs, its fields in JakePawnState) and `sample` here draws it in place of the frozen
// BaseMachine clip, as the sim's shapes put it: with no montage (and as the source under a blending-out one) the
// whole pose; under a montage the clip when it is the locomotion clip (a slot with no track passes its source on).
// The machine's pose: its state's clip at eat_times[eat], Idle's Walk clip mixed in by walk_w, the last transition's
// crossfade from fade_from by fade_t / fade_dur (HermiteCubic). Base_AnimBP's hit pitch is dropped there (the layer
// it turns is off the graph). A part of the view's slot mix that is the locomotion clip at its time is the machine's
// pose as well (`mix`).
//
// On Jake_S02 (IsOriginsSkin: AppliedSkin == SkinData'Jake_S02', @8379-8490) BlendListByBool_4 (bActiveValue <-
// IsOriginsSkin) takes OriginsEatOverride (StateMachine, StateMachineIndexInClass 0) for StateMachine_1. Its states,
// their order, the 11 transitions' rules (TR .. TR_10 @731-2289, TR_3 a copy of !IsFalling, against TR_11 .. TR_21),
// crossfades, HermiteCubic modes, Idle's list on Moving (BlendListByBool, BlendTime 0.1 / 0.1) and loop flags are
// StateMachine_1's; its players (SequencePlayer_2 .. 7) play Jake_S02_Eat_Attack_Idle / Walk / Fall / Land / Jump /
// Jump, each of the same length and RateScale as the default sequence of its state. The sim runs StateMachine_1 on
// every skin (it keeps the default skin), so its state, times and crossfade are the Origins machine's: `sample`
// draws them with the S02 clips (`eat_clips`).
//
// The slot nodes are the sim's (chars/jake/anim.rs): set() checks once that the data's graph has those nodes. Not
// listed: the Origins skin's slots and the shake slots, drawn here. The Origins slots
// on Jake_S02 (`OriginsPlan`): the graph's OriginsSkin Slot node (bAlwaysUpdateSourcePose) sits over DefaultSlot
// (OverrideSlot <- OriginsSkin <- DefaultSlot), and TwoWayBlend_3 / _2 / _1 (alpha IsOriginsSkin, @3744 / @3670 /
// @3596) take OriginsSpitBase / Up / Down for SpitBase / Up / Down, TwoWayBlend (@3229) OriginsSpit under
// ApplyAdditive for SpitAlly under ApplyAdditive_1. Jake_Eat_Attack_Montage's OriginsSkin track (Jake_S02_Eat_Attack_In
// / Hit), Jake_Eat_Attack_Spit_Montage's OriginsSpit* tracks (Spit_F / Up / D) and the two Spit_Ally montages'
// OriginsSpit track (Spit_Ally) lie on the default tracks' segments; the sim draws the default tracks (its stand-in
// for OriginsSkin, chars/jake/anim.rs origins_stand_in), so the view samples each Origins track's sequence it draws
// in place of its default track's, at the same time (`anim` for the fighter's anim, `mix` for the slot mix's parts).
// The shake (Jake_Eat_Attack_Struggle_Montage, group StruggleGroup: the sim's grouped instance, played while the eaten
// fighter struggles): its AdditiveShake track (Jake_Eat_Attack_Struggle) through ApplyAdditive_1 over SpitAlly, on
// Jake_S02 its OriginsAdditiveShake track (Jake_S02_Eat_Attack_Struggle) through ApplyAdditive over OriginsSpit
// (`shake_of`, applied at the top of `rule`). The slot nodes' source (SequencePlayer_1 / SequencePlayer) is taken
// as the additive identity.
// Unverified: SequencePlayer_1's sequence is not read: a non-zero additive there would move every pose.
// OriginsSkin has no IsOriginsSkin gate: Jake_Eat_Attack_Montage's OriginsSkin track (Jake_S02_Eat_Attack_In / Hit)
// is drawn on every skin (`ORIGINS_EVERY_SKIN`), the Spit slots' on Jake_S02 alone.
//
// Read from the sim: HeadScale (JakeState.head_scale, default (1, 1, 1): no node is on until the sim writes it),
// StretchOn and EatOverrideBool (JakeAnim, defaults false).
//
// Unverified: a curve's value in a crossfade and the one tick the update lags the evaluation (as Bugs's / Garnet's);
// the component-space products of a non-uniform parent scale (the FTransform rule).

/// The float curves its AnimBP reads by name (GetCurveValue), in the order the rule indexes them.
const CURVES: CurveTable = CurveTable::new(&["showEyebrows", "showEyelids"]);
/// ToggleTeethVisibility @109 / @462.
const TOGGLE: f64 = 0.9;
/// The sub-mesh keys of the two toggles (their curves: CURVES in the same order).
const PARTS: [&str; 2] = ["Brow", "Eyelids"];
const WEAPON_PARTS: [&str; 3] = ["JakeWeapon1", "JakeWeapon2", "JakeWeapon3"];
/// ZERO_ANIMWEIGHT_THRESH.
const ZERO_W: f64 = 1e-5;
/// Jake_AnimBP @8430: EqualEqual_ObjectObject(AppliedSkin, SkinData'Jake_S02') sets IsOriginsSkin.
const ORIGINS_SKIN: &str = "Jake_S02";
/// OriginsEatOverride's players by state (EAT_CLIPS' order: Idle, Fall, Land, Jump, Jump2): SequencePlayer_2, _4,
/// _5, _6, _7.
const ORIGINS_EAT_CLIPS: [&str; 5] = [
    "Jake_S02_Eat_Attack_Idle",
    "Jake_S02_Eat_Attack_Fall",
    "Jake_S02_Eat_Attack_Land",
    "Jake_S02_Eat_Attack_Jump",
    "Jake_S02_Eat_Attack_Jump",
];
/// Its Idle's Walk player (SequencePlayer_3, BlendListByBool's pose 0 on Moving).
const ORIGINS_EAT_WALK: &str = "Jake_S02_Eat_Attack_Walk";
/// The pairs of ORIGINS_SLOTS the graph takes on every skin (the OriginsSkin Slot node, no TwoWayBlend over it).
const ORIGINS_EVERY_SKIN: usize = 1;
/// The montage slots the graph takes on Jake_S02 in place of the default skin's: (default slot, Origins slot).
const ORIGINS_SLOTS: [(&str, &str); 5] = [
    ("DefaultSlot", "OriginsSkin"),
    ("SpitBase", "OriginsSpitBase"),
    ("SpitUp", "OriginsSpitUp"),
    ("SpitDown", "OriginsSpitDown"),
    ("SpitAlly", "OriginsSpit"),
];

/// A gated node in graph order: bone, translation (UE cm, bone axes), scale factor of HeadScale and its mode
/// (BMM_Replace in component space, else Additive).
struct HeadNode {
    bone: &'static str,
    moves: Option<[f64; 3]>,
    factor: Option<f64>,
    replace: bool,
}

const HEAD_NODES: [HeadNode; 4] = [
    // ModifyBone_7
    HeadNode { bone: "fml_un_C_spine_b", moves: None, factor: Some(0.6), replace: false },
    // ModifyBone_4
    HeadNode { bone: "fml_un_C_spine_d", moves: Some([-15.0, 0.0, 0.0]), factor: Some(0.65), replace: false },
    // ModifyBone_3
    HeadNode { bone: "fml_un_C_neck_a", moves: Some([-20.0, 0.0, 30.0]), factor: Some(0.9), replace: true },
    // ModifyBone_1
    HeadNode { bone: "fml_un_C_Face_att", moves: Some([-5.0, 0.0, 5.0]), factor: None, replace: false },
];
/// ModifyBone_6 / _5.
const ARM_ATT: [&str; 2] = ["fml_un_L_uparm_att", "fml_un_R_uparm_att"];

const SLOT_NODES: [&str; 20] = [
    "DefaultSlot",
    "OverrideSlot",
    "JumpSlot",
    "FallingBlend",
    "ShortAttack",
    "ReachAttack",
    "SpitAlly",
    "SpitBase",
    "SpitUp",
    "SpitDown",
    "AimPunchBase",
    "AimPunchUp",
    "AimPunchUp2",
    "AimPunchDown",
    "AimPunchDown2",
    "AimPunchShortBase",
    "AimPunchShortUp",
    "AimPunchShortUp2",
    "AimPunchShortDown",
    "AimPunchShortDown2",
];
/// The slot nodes above ApplyAdditive_1 / ApplyAdditive (the graph's SpitBase <- ShortAttack / ReachAttack <-
/// TwoWayBlend on IsOriginsSkin chain, and the root's aim blends' Spit slots): a shown montage's track in one of these
/// is drawn over the shake.
const OVER_SHAKE: [&str; 8] = [
    "SpitBase",
    "SpitUp",
    "SpitDown",
    "ShortAttack",
    "ReachAttack",
    "OriginsSpitBase",
    "OriginsSpitUp",
    "OriginsSpitDown",
];
static GRAPH_CHECKED: AtomicBool = AtomicBool::new(false);

#[derive(Debug)]
struct Head {
    i: usize,
    /// The translation in the pose's space (m).
    moves: Option<[f64; 3]>,
    factor: Option<f64>,
    replace: bool,
    /// Its ancestors, nearest first.
    chain: Vec<usize>,
}

/// The EatOverride machine's state of the tick (JakePawnState's eat fields).
#[derive(Clone, Copy, Debug, Default)]
struct Eat {
    eat: i32,
    eat_times: [f64; 5],
    walk_time: f64,
    walk_w: f64,
    fade_from: i32,
    fade_dur: f64,
    fade_t: f64,
}

/// A montage's Origins tracks as Jake_S02's view draws them: each default track's sequence -> the Origins track's
/// sequence on the same segment (ORIGINS_SLOTS).
#[derive(Debug)]
struct OriginsPlan {
    anims: Vec<(Name, Name)>,
}

/// The shown montage's data (`f.shownMontage`).
fn shown_data(w: &World, f: FighterId) -> Option<&Montage> {
    let dash = shown_montage(w, f)?;
    let uid = slot_uid(w, f, if dash { MontageSlot::Dash } else { MontageSlot::Main })?;
    instance_data(&w.data, w, f, uid)
}

/// `jakePose`'s state on one view.
#[derive(Debug)]
struct JakePose {
    /// Jake_S02 (IsOriginsSkin).
    origins: bool,
    slaved: bool,
    stand_in: bool,
    head: Vec<Head>,
    arms: Vec<(usize, Vec<usize>)>,
    part: [Option<usize>; 2],
    tags: [String; 2],
    weapon_parts: Vec<usize>,
    live: bool,
    /// The last tick set() ran at (a gap: the fighter was dead between: the respawn).
    last_tick: Option<i32>,
    /// The head nodes' alpha.
    gate: bool,
    /// HeadScale.X (uniform: Conv_FloatToVector).
    k: f64,
    /// ModifyBone_13's pitch (StretchDirection) while StretchOn.
    turn: Option<f64>,
    /// The EatOverride machine while it is the graph's pose.
    eat: Option<Eat>,
    /// No montage's pose this tick: the whole pose is the machine's.
    eat_all: bool,
    loco_clip: Option<usize>,
    loco_time: f64,
    /// The machine's players on the view's clips (resolved at the first set()): EAT_CLIPS and EAT_WALK, on Jake_S02
    /// ORIGINS_EAT_CLIPS and ORIGINS_EAT_WALK.
    eat_clips: Option<([Option<usize>; 5], Option<usize>)>,
    /// Not slaved, per montage name its Origins plan, None where it has no Origins track drawn.
    plans: IndexMap<String, Option<OriginsPlan>>,
    /// The shown montage's plan of the tick: its pairs as names and as clips.
    slot_anims: Vec<(Name, Name)>,
    slot_clips: Vec<(usize, usize)>,
    eat_a: Pose,
    eat_b: Pose,
    eat_c: Pose,
    tgt: [f64; 2],
    cur: [f64; 2],
    from: [f64; 2],
    fades: u32,
    /// BrowsVisible / EyelidsVisible.
    stored: [bool; 2],
    /// The AnimBP's last write (-1 none, 0 hidden, 1 shown).
    own: [i8; 2],
    /// The tick's grouped instances (`layers_into`): slot, anim, time, weight.
    grouped: (Vec<String>, Vec<String>, Vec<mvs_sim::Real>, Vec<mvs_sim::Real>),
    /// The tick's shake: per additive instance of the skin's shake slot, its clip, time and weight.
    shake: Vec<(usize, f64, f64)>,
    shake_ta: Pose,
    sim_vis: [i8; 2],
}

fn jake_pose(view: &mut CharView, _clips: &Clips, of: &PoseOf<'_>) -> Box<dyn PoseHook> {
    let bone = |b: &str| {
        let i = view.rig.index(b);
        if i.is_none() {
            eprintln!("warning: Jake pose: bone missing: {b}");
        }
        i
    };
    let parents = &view.parents;
    let chain_of = |i: usize| {
        let mut out = Vec::new();
        let mut p = parents[i];
        while p >= 0 {
            out.push(p as usize);
            p = parents[p as usize];
        }
        out
    };
    let head = HEAD_NODES
        .iter()
        .filter_map(|n| {
            let i = bone(n.bone)?;
            // UE (X, Y, Z) cm -> the pose's (X, Z, Y) m
            let moves = n.moves.map(|m| [m[0] / 100.0, m[2] / 100.0, m[1] / 100.0]);
            Some(Head { i, moves, factor: n.factor, replace: n.replace, chain: chain_of(i) })
        })
        .collect();
    let arms = ARM_ATT.iter().filter_map(|b| bone(b)).map(|i| (i, chain_of(i))).collect();
    let part = PARTS.map(|k| of.part_keys.iter().position(|p| p == k));
    for (k, p) in PARTS.iter().zip(&part) {
        if p.is_none() {
            eprintln!("warning: Jake pose: the skin has no sub-mesh {k}");
        }
    }
    let weapon_parts = WEAPON_PARTS.iter().filter_map(|k| of.part_keys.iter().position(|p| p == k)).collect();
    let n = view.rig.names.len();
    Box::new(JakePose {
        origins: of.skin.name == ORIGINS_SKIN,
        slaved: of.slaved,
        stand_in: of.stand_in,
        head,
        arms,
        part,
        tags: PARTS.map(str::to_lowercase),
        weapon_parts,
        live: false,
        last_tick: None,
        gate: false,
        k: 1.0,
        turn: None,
        eat: None,
        eat_all: false,
        loco_clip: None,
        loco_time: 0.0,
        eat_clips: None,
        plans: IndexMap::new(),
        slot_anims: Vec::new(),
        slot_clips: Vec::new(),
        eat_a: Pose::new(n),
        eat_b: Pose::new(n),
        eat_c: Pose::new(n),
        tgt: [0.0; 2],
        cur: [0.0; 2],
        from: [0.0; 2],
        fades: 0,
        stored: [false; 2],
        own: [-1; 2],
        sim_vis: [-1; 2],
        grouped: Default::default(),
        shake: Vec::new(),
        shake_ta: Pose::new(n),
    })
}

/// The bone's component-space scale from its ancestors' local scales (the chain nearest first).
fn cs_scale(p: &Pose, chain: &[usize]) -> [f64; 3] {
    let mut s = [1.0f64; 3];
    for &c in chain {
        for (k, v) in s.iter_mut().enumerate() {
            *v *= f64::from(p.s[c * 3 + k]);
        }
    }
    s
}

/// The bone's component scale set to `v` (its local scale the value over its parent's component scale).
fn replace_scale(p: &mut Pose, i: usize, chain: &[usize], v: f64) {
    let ps = cs_scale(p, chain);
    for (k, s) in ps.iter().enumerate() {
        p.s[i * 3 + k] = if *s != 0.0 { (v / s) as f32 } else { 0.0 };
    }
}

impl JakePose {
    /// `init`: the AnimBP's state anew.
    fn init(&mut self) {
        self.gate = false;
        self.k = 1.0;
        self.turn = None;
        self.eat = None;
        self.tgt = [0.0; 2];
        self.cur = [0.0; 2];
        self.from = [0.0; 2];
        self.fades = 0;
        self.stored = [false; 2];
        self.own = [-1; 2];
        self.sim_vis = [-1; 2];
    }

    /// `eatState`: state `st`'s clip at its time into `out`, Idle's Walk clip mixed in by walk_w.
    fn eat_state(&mut self, cx: &mut PoseCx<'_>, e: &Eat, st: i32, out: &mut Pose) -> bool {
        let Some((clips, walk)) = self.eat_clips else { return false };
        let Some(s) = usize::try_from(st).ok().filter(|&s| s < 5) else { return false };
        let Some(c) = clips[s] else { return false };
        cx.sample(c, e.eat_times[s], out);
        if s == 0 && e.walk_w > ZERO_W {
            let Some(wc) = walk else { return false };
            cx.sample(wc, e.walk_time, &mut self.eat_c);
            blend_pose(out, &self.eat_c, e.walk_w.min(1.0), &mut self.eat_b);
            out.copy(&self.eat_b);
        }
        true
    }

    /// `eatPose`: the machine's pose, with the last transition's crossfade.
    fn eat_pose(&mut self, cx: &mut PoseCx<'_>, e: &Eat, out: &mut Pose) -> bool {
        if e.fade_from >= 0 && e.fade_dur > 0.0 && e.fade_t < e.fade_dur {
            let mut a = std::mem::replace(&mut self.eat_a, Pose::new(0));
            let ok = self.eat_state(cx, e, e.fade_from, &mut a) && self.eat_state(cx, e, e.eat, out);
            if ok {
                let t = (e.fade_t / e.fade_dur).clamp(0.0, 1.0);
                blend_pose(&a, out, t * t * (3.0 - 2.0 * t), &mut self.eat_b);
                out.copy(&self.eat_b);
            }
            self.eat_a = a;
            return ok;
        }
        self.eat_state(cx, e, e.eat, out)
    }

    fn hide_weapons(&self, view: &mut CharView) {
        for &k in &self.weapon_parts {
            view.set_part_visible(k, false);
        }
    }

    /// The machine's players on the view's clips: on Jake_S02 OriginsEatOverride's, else (and where one of those is
    /// not loaded) StateMachine_1's.
    fn eat_clips_of(&self, w: &World, clips: &Clips) -> ([Option<usize>; 5], Option<usize>) {
        if self.origins {
            let st = ORIGINS_EAT_CLIPS.map(|n| clips.get(n));
            let walk = clips.get(ORIGINS_EAT_WALK);
            if st.iter().all(Option::is_some) && walk.is_some() {
                return (st, walk);
            }
            // Deviation: a store without the Jake_S02 sequences among Jake's clips (an import before they were added):
            // the default machine's clips stand in.
            warn_once(
                "Jake pose: the OriginsEatOverride clips (Jake_S02_Eat_Attack_*) are not loaded: the EatOverride \
                 machine's clips stand in"
                    .to_owned(),
            );
        }
        let get = |n: Name| {
            let c = clips.get(w.names.str(n));
            if c.is_none() {
                eprintln!(
                    "warning: Jake pose: the EatOverride clip is not loaded: {} (the frozen locomotion clip is drawn)",
                    w.names.str(n)
                );
            }
            c
        };
        (EAT_CLIPS.map(get), get(EAT_WALK))
    }

    /// The montage's Origins plan, made at its first sight: per ORIGINS_SLOTS pair the skin takes (every pair on
    /// Jake_S02, the first ORIGINS_EVERY_SKIN on the others) the montage has a track of, the
    /// default track's sequences to the Origins track's, when both tracks lie on the same segments and the Origins
    /// sequences are loaded (else a warning and the default track is drawn).
    fn plan_of(&mut self, w: &World, m: &Montage, clips: &Clips) {
        if self.plans.contains_key(&m.name) {
            return;
        }
        let mut anims: Vec<(Name, Name)> = Vec::new();
        let pairs = if self.origins { &ORIGINS_SLOTS[..] } else { &ORIGINS_SLOTS[..ORIGINS_EVERY_SKIN] };
        for &(def_slot, slot) in pairs {
            let Some(o) = m.slots.iter().find(|s| s.slot == slot) else { continue };
            let d = m.slots.iter().find(|s| s.slot == def_slot);
            let same = d.is_some_and(|d| {
                d.segments.len() == o.segments.len()
                    && d.segments.iter().zip(&o.segments).all(|(a, b)| {
                        a.start == b.start && a.a0 == b.a0 && a.a1 == b.a1 && a.rate == b.rate && a.loops == b.loops
                    })
            });
            let (true, Some(d)) = (same, d) else {
                warn_once(format!(
                    "Jake pose: {}'s {slot} track is not on the {def_slot} track's segments: the {def_slot} pose is drawn",
                    m.name
                ));
                continue;
            };
            if let Some(b) = o.segments.iter().find(|b| clips.get(&b.anim).is_none()) {
                warn_once(format!("Jake pose: clip {} is not loaded: the {def_slot} pose is drawn", b.anim));
                continue;
            }
            for (a, b) in d.segments.iter().zip(&o.segments) {
                let (Some(a), Some(b)) = (w.names.get(&a.anim), w.names.get(&b.anim)) else { continue };
                // (a sequence on two segments takes the last one's)
                match anims.iter_mut().find(|(x, _)| *x == a) {
                    Some(e) => e.1 = b,
                    None => anims.push((a, b)),
                }
            }
        }
        let plan = (!anims.is_empty()).then_some(OriginsPlan { anims });
        self.plans.insert(m.name.clone(), plan);
    }

    /// The tick's Origins pairs: the shown montage's plan (not slaved), as names and as clips.
    fn origins_tracks(&mut self, w: &World, f: FighterId, clips: &Clips) {
        self.slot_anims.clear();
        self.slot_clips.clear();
        if self.slaved {
            return;
        }
        let Some(m) = shown_data(w, f) else { return };
        self.plan_of(w, m, clips);
        let Some(Some(p)) = self.plans.get(&m.name) else { return };
        self.slot_anims.extend_from_slice(&p.anims);
        for &(a, b) in &p.anims {
            if let (Some(a), Some(b)) = (clips.get(w.names.str(a)), clips.get(w.names.str(b))) {
                self.slot_clips.push((a, b));
            }
        }
    }

    /// The tick's shake (ApplyAdditive_1, on Jake_S02 ApplyAdditive): the grouped instances in the skin's shake slot
    /// (AdditiveShake, on Jake_S02 OriginsAdditiveShake: TwoWayBlend's B side on IsOriginsSkin), none while a shown
    /// montage has a track in a slot over it (OVER_SHAKE). A stand-in reads no Jake graph.
    fn shake_of(&mut self, w: &World, f: FighterId, clips: &Clips) {
        self.shake.clear();
        if self.stand_in {
            return;
        }
        let (slot, anim, time, wt) = &mut self.grouped;
        if layers_into(w, f, slot, anim, time, wt) == 0 {
            return;
        }
        // Unverified: a slot over the shake covers it whole at any weight (the build blends the two by that slot's
        // weight, its blend-in and -out).
        if shown_data(w, f).is_some_and(|m| m.slots.iter().any(|s| OVER_SHAKE.contains(&s.slot.as_str()))) {
            return;
        }
        let want = if self.origins { "OriginsAdditiveShake" } else { "AdditiveShake" };
        let mut total = 0.0;
        for k in 0..slot.len() {
            if slot[k] != want || !(wt[k] > 1e-5) {
                continue;
            }
            let Some(c) = clips.get(&anim[k]).filter(|&c| clips.clips[c].additive) else {
                warn_once(format!("Jake pose: {}: no additive clip of that name (the shake is not drawn)", anim[k]));
                continue;
            };
            total += f64::from(wt[k]);
            self.shake.push((c, f64::from(time[k]), f64::from(wt[k])));
        }
        // the slot's weights over 1 normalised (SlotEvaluatePose)
        if total > 1.0 {
            for e in &mut self.shake {
                e.2 /= total;
            }
        }
    }
}

impl PoseRules for JakePose {
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
        let Some(e) = self.eat else { return false };
        // with no montage the clip is the locomotion one (the slot's source too), unless a stopped montage's pose is
        // the tick's; under a montage only the clip that is the locomotion clip (the source call under a shown
        // montage names the locomotion clip at its clamped time: by the clip alone)
        let loco = Some(clip) == self.loco_clip && (montage.is_none() || time == self.loco_time);
        if !(montage.is_none() && self.eat_all) && !loco {
            return false;
        }
        self.eat_pose(cx, &e, out)
    }

    fn mix(&mut self, cx: &mut PoseCx<'_>, clip: usize, time: f64, out: &mut Pose) -> bool {
        if let Some(e) = self.eat
            && Some(clip) == self.loco_clip
            && time == self.loco_time
        {
            return self.eat_pose(cx, &e, out);
        }
        // a part of the slot mix that is a default track's sequence: the Origins track's at its time (Jake_S02)
        let Some(&(_, o)) = self.slot_clips.iter().find(|(d, _)| *d == clip) else { return false };
        cx.sample(o, time, out);
        true
    }

    fn rule(&mut self, cx: &mut PoseCx<'_>, p: &mut Pose, _dt: f64) {
        // ApplyAdditive_1 (alpha 1): the shake's instances accumulated on the slot mix in play order, not the scale
        // (iron_giant.rs Slots::additive's rule)
        // Deviation: applied on the view's whole slot mix, over the montage tracks of the slots between the shake and
        // the graph's root that OVER_SHAKE does not list (none of Jake's montages has one while the shake plays).
        for &(c, time, wt) in &self.shake {
            accumulate_additive(cx.clips, c, cx.rig, cx.rest, time, wt, false, p, &mut self.shake_ta);
        }
        // ModifyBone_13: fml_un_COM turned by StretchDirection in component space (Replace)
        if let Some(turn) = self.turn {
            let h = turn * PI / 360.0;
            cx.turn_com(p, [0.0, 0.0, h.sin(), h.cos()]);
        }
        if self.gate {
            for n in &self.head {
                let i3 = n.i * 3;
                let [sx, sy, sz] = [0, 1, 2].map(|k| f64::from(p.s[i3 + k]));
                if let Some(m) = n.moves {
                    // t += q (s * m): through the bone's rotation and scale before the node
                    let (vx, vy, vz) = (sx * m[0], sy * m[1], sz * m[2]);
                    let [qx, qy, qz, qw] = p.quat(n.i);
                    let (cx_, cy, cz) = (qy * vz - qz * vy, qz * vx - qx * vz, qx * vy - qy * vx);
                    let d = [
                        vx + 2.0 * (qw * cx_ + qy * cz - qz * cy),
                        vy + 2.0 * (qw * cy + qz * cx_ - qx * cz),
                        vz + 2.0 * (qw * cz + qx * cy - qy * cx_),
                    ];
                    for (k, dv) in d.iter().enumerate() {
                        p.t[i3 + k] = (f64::from(p.t[i3 + k]) + dv) as f32;
                    }
                }
                let Some(f) = n.factor else { continue };
                let v = self.k * f;
                if n.replace {
                    replace_scale(p, n.i, &n.chain, v);
                } else {
                    p.s[i3] = (sx * v) as f32;
                    p.s[i3 + 1] = (sy * v) as f32;
                    p.s[i3 + 2] = (sz * v) as f32;
                }
            }
        }
        // ModifyBone_6 / _5: the upper arms' attach bones at component scale 1
        for (i, chain) in &self.arms {
            let ps = cs_scale(p, chain);
            let o = i * 3;
            if (0..3).all(|k| (ps[k] * f64::from(p.s[o + k]) - 1.0).abs() < 1e-6) {
                continue;
            }
            replace_scale(p, *i, chain, 1.0);
        }
        // this evaluation's curves, for the next update
        if cx.fade_count != self.fades {
            self.fades = cx.fade_count;
            self.from = self.cur;
        }
        let w = cx.fade_weight;
        for c in 0..self.cur.len() {
            self.cur[c] = if w >= 1.0 { self.tgt[c] } else { self.from[c] + (self.tgt[c] - self.from[c]) * w };
        }
    }
}

impl PoseHook for JakePose {
    /// `preview()`: a view with no fighter: the AnimBP anew, HeadScale the class default, the weapons
    /// hidden; its toggles run in preview_step.
    fn preview(&mut self, view: &mut CharView, _of: &PreviewOf<'_>) {
        self.init();
        self.live = false;
        self.hide_weapons(view);
    }

    /// The preview's update (as gizmo.rs's): on BP_CharacterPreview TryGetPawnOwner fails, but the update's
    /// toggle branch (@7486) tests only IsValid(VisibilityManager), the owning actor's (@346-531), and the Fighter
    /// test (@2707) ends only its own branch: the two toggles on the curves of the evaluation before (the AnimBP is the
    /// parts' only writer here), then the curves of the frame's sequence.
    fn preview_step(&mut self, view: &mut CharView, clips: &Clips, s: &PreviewStep<'_>) {
        for n in 0..PARTS.len() {
            let on = self.cur[n] > TOGGLE;
            let Some(k) = self.part[n] else { continue };
            if on != self.stored[n] {
                self.stored[n] = on;
                view.set_part_visible(k, on);
            }
        }
        CURVES.fill(clips, s.anim, s.time, &mut self.tgt);
    }

    fn set(&mut self, w: &World, f: FighterId, view: &mut CharView, clips: &Clips) {
        if !GRAPH_CHECKED.swap(true, Ordering::Relaxed) {
            let g = &w.char_data(f).fighter.slot_holds.graph;
            let has = |s: &str| g.nodes.iter().any(|n| matches!(n, SlotGraphNode::Slot { slot, .. } if slot == s));
            let missing: Vec<&str> = SLOT_NODES.into_iter().filter(|s| !has(s)).collect();
            if !missing.is_empty() {
                eprintln!("warning: Jake pose: the data's Jake_AnimBP slot graph lacks {}", missing.join(", "));
            }
        }
        if self.eat_clips.is_none() {
            self.eat_clips = Some(self.eat_clips_of(w, clips));
        }
        self.origins_tracks(w, f, clips);
        self.shake_of(w, f, clips);
        self.hide_weapons(view);
        // the respawn (the view places no dead fighter: a gap in the ticks is a death between)
        // Deviation: the respawn is seen as a gap in the ticks the view runs at (the effects are told of
        // every dead tick); a rollback's jump of more than one tick is taken as one and drops the toggle state.
        let tick = w.flow.tick;
        if self.last_tick.is_some_and(|t| tick > t + 1) {
            self.own = [-1; 2];
        }
        self.last_tick = Some(tick);
        self.live = !self.slaved;
        let fi = w.fighter(f);
        // the head nodes' alpha (the exposed-input handlers read the AnimBP's variables of this update); a stand-in
        // reads no Jake state (the class defaults)
        let j = if self.stand_in { None } else { jake_of(w, f) };
        let a = j.map(|j| &j.anim);
        self.k = j.map_or(1.0, |j| f64::from(j.head_scale[0]));
        let stretch_on = a.is_some_and(|a| a.stretch_on);
        let eat_override = a.is_some_and(|a| a.eat_override);
        self.gate = self.k > 1.0 && !stretch_on && !eat_override;
        self.turn = a.filter(|a| a.stretch_on).map(|a| a.stretch_direction).map(f64::from);
        // (on Jake_S02 BlendListByBool_4's OriginsEatOverride: StateMachine_1's state over its own clips, eat_clips)
        self.eat = match j {
            Some(j) if eat_override => {
                let p = &j.pawn;
                Some(Eat {
                    eat: p.eat,
                    eat_times: p.eat_times.map(f64::from),
                    walk_time: f64::from(p.walk_time),
                    walk_w: f64::from(p.walk_w),
                    fade_from: p.fade_from,
                    fade_dur: f64::from(p.fade_dur),
                    fade_t: f64::from(p.fade_t),
                })
            }
            _ => None,
        };
        if self.eat.is_some() {
            self.eat_all =
                mvs_sim::fighter::shown_montage(w, f).is_none() && !matches!(fi.anim.anim_key, AnimKey::Montage(_));
            self.loco_clip = clips.get(w.names.str(fi.anim.loco_anim));
            self.loco_time = f64::from(fi.anim.loco_time);
            // the layer Base_AnimBP's pitch turns is off the graph (no hit pitch while the machine is on)
            view.hit_pitch = None;
            view.layer_pitch = None;
        }
        if !self.live {
            return;
        }
        // BlueprintUpdateAnimation on the curves of the evaluation before: the two toggles
        for n in 0..PARTS.len() {
            let on = self.cur[n] > TOGGLE;
            if on != self.stored[n] && self.part[n].is_some() {
                self.stored[n] = on;
                self.own[n] = i8::from(on);
            }
            let Some(k) = self.part[n] else { continue };
            // the AnimBP's write until the sim changes the part's visibility (the later write)
            let hidden = w.names.get(&self.tags[n]).is_some_and(|t| fi.pose.sub_hidden.contains(&t));
            let vis = i8::from(!hidden);
            if self.sim_vis[n] >= 0 && self.sim_vis[n] != vis {
                self.own[n] = -1;
            }
            self.sim_vis[n] = vis;
            view.set_part_visible(k, if self.own[n] >= 0 { self.own[n] == 1 } else { vis == 1 });
        }
        // the sequence this tick's pose is sampled from
        let anim = self.anim(w, f).unwrap_or(fi.anim.anim);
        CURVES.fill(clips, w.names.str(anim), f64::from(fi.anim.anim_time), &mut self.tgt);
    }

    fn reset(&mut self, _part_scalar: &mut dyn FnMut(usize, &str, f64)) {
        self.init();
        self.last_tick = None;
    }

    fn shown(&self, tag: &str, sim: bool) -> bool {
        match self.tags.iter().position(|t| t.eq_ignore_ascii_case(tag)) {
            Some(n) if self.own[n] >= 0 => self.own[n] == 1,
            _ => sim,
        }
    }

    /// On Jake_S02 the Origins track's sequence in place of the default track's the fighter's anim is (set()'s pairs
    /// of the shown montage).
    fn anim(&self, w: &World, f: FighterId) -> Option<Name> {
        let a = w.fighter(f).anim.anim;
        self.slot_anims.iter().find(|(d, _)| *d == a).map(|(_, o)| *o)
    }
}

// ---- the weapons -------------------------------------------------------------------
//
// The components: Weapon1 / Weapon2 / Weapon3, skeletal mesh components with no mesh of their own, tags
// [JakeWeaponN, JakeWeapon, IgnoreArmor], bVisible false; the skin's SubMeshes give them Jake_Axe_SkelMesh,
// Jake_Hammer_SkelMesh and Jake_Spikeball_SkelMesh (keys JakeWeapon1-3), and Comp_Pawn_MeshManager.SetSkelMesh puts
// the main mesh as their master pose (no VisMesh tag). Comp_WeaponSwap BeginPlay @15-592 attaches every skeletal
// mesh component tagged its MeshTag (JakeWeapon) to the main mesh at its HoldSlotName R_HoldPoint
// (fml_un_R_wrist, identity) with SnapToTarget for location, rotation and scale: the template's RelativeLocation Z
// 202.5 and scale 1.25 are replaced by the socket's. BeginPlay @955 / @978 binds MirrorWeaponSide to the owner's
// FacingChanged: each AddedWeapons entry K2_AttachToComponent(Mesh, FindMirrorSocket(HoldSlotName), SnapToTarget x 3)
// @394 (L_HoldPoint facing left, R_HoldPoint facing right: both exist), then K2_SetRelativeRotation by the facing
// (ROT_LEFT / ROT_RIGHT). The build's socket under the mirrored pose is the view's mirror partner under the
// reflection: Rec::component_attach, given HoldSlotName, takes its L_ twin facing left as FindMirrorSocket does
// (L_HoldPoint, the view's R_HoldPoint reflected).
// Unverified: FacingChanged is taken to have run for the facing he has (SetRotationLogic broadcasts it at every facing
// set, the spawn's too, as Harley's), so the BeginPlay's identity on R_HoldPoint is never drawn.
//
// The meshes share Jake's skeleton and are weighted to fml_un_C_prop1 alone. A master-posed component reads the main
// mesh's component-space prop1, which no clip of his tracks (Jake_Air_HR_Sleep aside): its reference pose, mirrored
// facing left. The port draws each weapon as a component mesh of the view at the socket, its own skeleton at the
// reference pose, and keeps the view's bone-bound copy hidden (the pose hook: the shared parts would draw it at
// Jake's own prop1, at his feet).
//
// Who shows them: Comp_WeaponSwap's SetSubMeshVisibility(JakeWeapon1 / 2 / 3) (@1302-1648, @3035-3381, @3912-4174,
// @4632-4894: mvs-sim chars/jake/weapons.rs) and AnimNotify_SetSubmeshVisibility of the hammer / spike ball / axe
// montages, through the sim's sub-mesh state (sub_hidden, tags of the character's sub_meshes). JakeState.weapon /
// weapon_equipped / weapons_hidden are the sim's mirror of the same calls and are not read here.
//
// Unverified: prop1's component-space transform taken as its reference pose (no clip tracks it; the master pose's
// mirror of it facing left as the reflection's); the socket is the last rendered pose's.

/// Comp_WeaponSwap HoldSlotName (CDO).
const SOCKET: &str = "R_HoldPoint";
/// FindMirrorSocket(R_HoldPoint): the socket facing left.
const MIRROR_SOCKET: &str = "L_HoldPoint";
/// MirrorWeaponSide's K2_SetRelativeRotation: rot(0, 0, 180) facing right @746, rot(0, 180, 0) facing left @556, as
/// FRotator::Quaternion gives them (UE's X, Y, Z, W).
const ROT_RIGHT: [f64; 4] = [-1.0, 0.0, 0.0, 0.0];
const ROT_LEFT: [f64; 4] = [0.0, 0.0, 1.0, 0.0];

/// `jakeWeapons`: the weapons on the view when it is made.
fn weapons_make(r: &mut Rec, w: &World, clips: &Clips, _draws: &FighterDraws, _meshes: &MeshMap) {
    let cd = w.char_data(r.fighter());
    let sockets = &cd.fighter.skeleton.sockets;
    if !sockets.contains_key(SOCKET) || !sockets.contains_key(MIRROR_SOCKET) {
        eprintln!("warning: Jake: the skeleton lacks {SOCKET} or {MIRROR_SOCKET}: the weapons are not drawn");
        return;
    }
    for key in WEAPON_PARTS {
        let tag = key.to_lowercase();
        let at = [false, true].map(|left| {
            r.component_attach(w, SOCKET, [0.0; 3], if left { ROT_LEFT } else { ROT_RIGHT }, [1.0; 3], left)
        });
        // (the part's own material instances, host.partMaterials: the fighter's scalars set on them show here too)
        if !r.add_part_component_mesh(clips, &tag, at, key, false) {
            eprintln!("warning: Jake: weapon not drawn: {key} (the skin's sub-mesh or its materials are not loaded)");
            continue;
        }
        if !cd.def.sub_meshes.contains(&tag) {
            eprintln!("warning: Jake: the sim's mesh list has no {tag}: the weapon stays hidden");
        }
    }
}

/// `placeJakeWeapons`: once a presentation tick, each shown while the sim's mesh list has its tag and sub_hidden
/// does not (the facing's socket is the view's pick of the two made).
fn weapons_place(r: &mut Rec, w: &World) {
    for key in WEAPON_PARTS {
        let tag = key.to_lowercase();
        let on = r.sub_mesh_shown(w, &tag);
        r.set_component_mesh(&tag, on);
    }
}

// ---- the actors ---------------------------------------------------------------------

const STEP: f64 = 1.0 / 60.0;
const DEG: f64 = PI / 180.0;
const STRETCH_CLS: &str = "JakeStretch_C";
const HOUSE_CLS: &str = "JakeHouse_Actor_C";
const HORSE_CLS: &str = "JakeHorse_Actor3_C";

fn right_rows() -> Rows {
    rotation_rows(0.0, -PI / 2.0, 0.0)
}

fn left_rows() -> Rows {
    rotation_rows(0.0, PI / 2.0, 0.0)
}

/// The ticks a view's AnimBP runs since its last presented tick (`n`): 1 at the first and after a
/// rollback back in time, the gap else (at most 30), 0 on the same tick.
fn ticks_since(last: Option<i32>, tick: i32) -> i32 {
    match last {
        None => 1,
        Some(l) if tick < l => 1,
        Some(l) => (tick - l).min(30),
    }
}

/// What the owner's skin draws an actor's meshes with: the fighter whose data has the class, its skin, the skin
/// pick's variant.
struct Look<'w> {
    from: FighterId,
    name: String,
    /// The look's prop meshes (SkinLook.propMeshes; the default skin's where the look has none).
    props: &'w [PropMesh],
    variant: Variant,
    default: bool,
}

fn look_of<'w>(host: &ActorHost<'_>, w: &'w World, owner: FighterId, cls: Name) -> Option<Look<'w>> {
    let from = mvs_sim::fighter::common::actor_def_owner(w, owner, cls);
    let pick =
        usize::try_from(w.fighter(from).id).ok().and_then(|i| w.setup.players.get(i)).and_then(|p| p.skin.as_deref());
    let fd: &'w FighterData = &w.char_data(from).fighter;
    let skin = crate::skins::skin_of(fd, pick).ok()?;
    let look = fd.skins.as_ref().and_then(|m| m.get(&skin.name));
    let props = look.and_then(|l| l.prop_meshes.as_deref()).or(fd.skin.prop_meshes.as_deref()).unwrap_or(&[]);
    // (the instigator's pick of the owner's applied skin's material sets)
    let variant = match host.colour(w, owner).variant {
        Variant::Menu => Variant::Local,
        v => v,
    };
    Some(Look { from, default: skin.name == fd.skin.name, name: skin.name, props, variant })
}

/// A prop mesh's model (its path, else the character's path of its mesh).
fn prop_info(host: &ActorHost<'_>, w: &World, from: FighterId, p: &PropMesh) -> Option<Arc<crate::mesh::MeshInfo>> {
    let path = p.path.clone().or_else(|| w.char_data(from).fighter.paths.meshes.get(&p.mesh).cloned())?;
    host.mesh(&path)
}

// ---- JakeStretch_C: the bottom half ----
//
// - The body (one USplineMeshComponent per added mesh, AddSplineMesh @2632): mvs-render's component meshes
//   (fx/chars/jake.rs).
// - Jake_BotHalf_SkelMesh (under the Capsule at (0, 0, -143), AnimClass JakeBottomHalfBlueprint_C), S01_LowerBody_Split
//   on a cake skin (@5635): scaled by MeshScaleAmount (the owner's Mesh.RelativeScale3D.Y, the view's mesh scale), its
//   material the owner mesh's slot 0 (@5898-5971: the skin's body slot 0 in the same pick; '<skin>.LowerBody' for a
//   cake skin), placed and pitched by the sim (`leg_x` / `leg_y` / `leg_pitch`, the Return's @20173).
//   JakeBottomHalfBlueprint: Root <- Mirror (no table) <- Mirror_1 (Generic_MirrorTable), both enabled by FacingLeft,
//   <- BlendListByBool(Retracting; BlendTime 0, 0): true Jake_StretchLoop, false Jake_StretchIdle (both looped). The
//   inactive player is not updated (a blend list ticks only its weighted children), so each clip's time runs only while
//   it shows.
// Deviation: the bottom half's mirror is drawn as the reflection of the right-facing view across UE Y (as the
//   fighters'), not as Generic_MirrorTable's bone mirror.
// Deviation: the bottom half's clip time runs once a presented tick of the world (a rollback's jump of several ticks runs
//   the ticks between with the latest state).
// The skin is the render's: IsCake is read here from the drawn skin's name (CAKE_SKINS).
// Not drawn: the armor mesh (AddArmorMesh @30201 adds none).

const BOTTOM_MESH: &str = "Jake_BotHalf_SkelMesh";
const STRETCH_IDLE: &str = "Jake_StretchIdle";
const STRETCH_LOOP: &str = "Jake_StretchLoop";
/// @5198-5362.
const CAKE_SKINS: [&str; 2] = ["C005_S03", "Jake_S01"];

#[derive(Debug)]
struct StretchMesh {
    id: mvs_sim::actors::ActorId,
    from: FighterId,
    legs: SkinnedActor,
    mesh_scale: f64,
    idle: Option<usize>,
    looped: Option<usize>,
    idle_t: f64,
    loop_t: f64,
    last_tick: Option<i32>,
}

fn stretch_mesh(host: &mut ActorHost<'_>, w: &World, a: ActorRef, cs: &ActorComponents) -> Option<Box<dyn ActorMesh>> {
    let ActorRef::Actor(id) = a else { return None };
    let x = w.actors.actors.get(id)?;
    let lk = look_of(host, w, x.owner, x.cls)?;
    let d = host.draws_of(w, lk.from)?;
    let vk = (lk.variant != Variant::Local).then(|| lk.variant.key());
    let cake = CAKE_SKINS.contains(&lk.name.as_str());
    let bot_half = cs.get(BOTTOM_MESH).and_then(|c| match &c.kind {
        crate::actors::ComponentKind::SkeletalMesh { mesh, .. } => Some(mesh.clone()),
        _ => None,
    });
    // (stretchLook: the skin's bottom half mesh and slot materials; without the skin's entry not drawn)
    let (legs_mesh, legs_keys) = if lk.default {
        (
            bot_half.and_then(|m| w.char_data(lk.from).fighter.paths.meshes.get(&m).cloned()),
            d.actor(BOTTOM_MESH, vk).map(|a| a.1),
        )
    } else if cake {
        match d.actor_key(&format!("{}.LowerBody", lk.name)).and_then(|k| d.actor(&k, vk)) {
            Some((m, keys)) => (Some(m), Some(keys)),
            None => (None, None),
        }
    } else {
        let slot0 = d.set(&lk.name, "", lk.variant).ok().map(|s| vec![s.first().cloned().flatten()]);
        (bot_half.and_then(|m| w.char_data(lk.from).fighter.paths.meshes.get(&m).cloned()), slot0)
    };
    let info = legs_mesh.as_deref().and_then(|m| host.mesh(m));
    let (Some(info), Some(keys)) = (info, legs_keys) else {
        eprintln!("warning: Jake: the stretch bottom half is not drawn (its mesh or materials are missing)");
        return None;
    };
    let clips = host.clips_of(w, lk.from)?;
    let clip = |n: &str| {
        let c = clips.get(n);
        if c.is_none() {
            eprintln!("warning: Jake: the stretch clip is not loaded: {n}");
        }
        c
    };
    let (idle, looped) = (clip(STRETCH_IDLE), clip(STRETCH_LOOP));
    Some(Box::new(StretchMesh {
        id,
        from: lk.from,
        legs: SkinnedActor::new(info, &keys),
        mesh_scale: w.char_data(lk.from).boxes.mesh_scale,
        idle,
        looped,
        idle_t: 0.0,
        loop_t: 0.0,
        last_tick: None,
    }))
}

impl ActorMesh for StretchMesh {
    fn place(
        &mut self,
        host: &mut ActorHost<'_>,
        w: &World,
        _at: &ActorPlace,
        alpha: f64,
        shown: bool,
        out: &mut Vec<MeshPose>,
    ) {
        let Some(x) = w.actors.actors.get(self.id) else { return };
        let WorldActorState::Stretch(s) = &x.state else { return };
        self.legs.ticked(w.flow.tick);
        self.place_legs(host, w, s, f64::from(x.size_scale.unwrap_or(1.0)));
        self.legs.present(alpha);
        self.legs.visible = shown;
        self.legs.draw(out);
    }

    fn bones(&self, out: &mut Vec<(Arc<str>, Xform)>) {
        self.legs.bones_world(out);
    }
}

impl StretchMesh {
    fn place_legs(&mut self, host: &mut ActorHost<'_>, w: &World, s: &Stretch, size: f64) {
        // the clip time, once a tick of the world since the last presented one (a rollback back in time: one)
        let tick = w.flow.tick;
        let n = match self.last_tick {
            None => 1,
            Some(l) if tick > l => (tick - l).min(30),
            Some(l) if tick < l => 1,
            _ => 0,
        };
        self.last_tick = Some(tick);
        let retracting = s.retracting;
        if retracting {
            self.loop_t += f64::from(n) * STEP;
        } else {
            self.idle_t += f64::from(n) * STEP;
        }
        let (clip, t) = if retracting { (self.looped, self.loop_t) } else { (self.idle, self.idle_t) };
        if let Some(c) = clip
            && let Some(clips) = host.clips_of(w, self.from)
        {
            self.legs.play(clips, c, t, true);
        }
        // The actor's rows (facing left: the right-facing ones reflected across UE Y), the relative pitch under them.
        let right = right_rows();
        let base: Rows = std::array::from_fn(|j| right[j] * if s.facing_left && j % 3 == 1 { -1.0 } else { 1.0 });
        let rot = compose_rows(&rotation_rows(f64::from(s.leg_pitch) * DEG, 0.0, 0.0), &base);
        let k = self.mesh_scale * size;
        self.legs.set(&Xform { loc: [0.0, f64::from(-s.leg_x), f64::from(s.leg_y)], rot, scale: [k; 3] });
    }
}

// ---- JakeHouse_Actor_C ----
//
// - JakeHouse (a SkeletalMeshComponent under the Capsule, scale 0.8, AnimClass Jake_House_AnimBlueprint_C): its mesh
//   is the skin's MeshSwap sub-mesh of the sim's mesh_index (GetMeshSwapSubmeshes: the skin's propMeshes tagged
//   MeshSwap in order, House / Car / Boat; SetSkeletalMesh @693), each slot SelectMaterialColor of that sub-mesh's
//   material (the skin's set of the prop key, the skin pick's variant); scale (0.8, -0.8, 0.8) when the owner faced
//   left at BeginPlay (@4020: `mirrored`); hidden by the exit's SetVisibility(false).
// - The actor's yaw is the owner's (attached, KeepRelative): -90 facing right, 90 facing left.
// - Jake_House_AnimBlueprint: Root <- DefaultSlot (the sim's montage instance, mvs-sim actors/actor_montage.rs) <- the
//   machine BaseState (entry Intro, MaxTransitionsPerFrame 3, bSkipFirstUpdateTransition): Intro = JakeHouse_Intro (no
//   loop), Idle = JakeHouse_Idle (looped); Intro -> Idle when Intro's remaining time is 0 (crossfade 0.05,
//   HermiteCubic).
// Deviation: the AnimBP runs once a presented tick of the world (a rollback's jump of several ticks runs the ticks
//   between with the latest state).
// - The Car and Boat meshes on the House's sequences: Jake_House_Skeleton gives every bone but Root (Animation) the
//   TranslationRetargetingMode AnimationRelative, and the JakeHouse_* sequences (authored on the skeleton, whose
//   reference pose is Jake_House's rest: Face (182.2, 0.19, 322.5) cm; AnimRetargetSources empty) carry the House's
//   translations. A key's translation plays as key + (the mesh's rest - the skeleton's reference): retarget_delta
//   adds that delta to the shown pose of the Car / Boat (Face (263, 130) / (285, 158) cm). Every JakeHouse_* clip
//   tracks every bone of the skeleton, so the delta is the same for each part of a blend; the rests' rotations and
//   scales are identity in all three meshes (a mesh whose differ is warned and drawn on translations).
//   Unverified: the rule is stock UE 4.26's FAnimationRuntime::RetargetBoneTransform as read from memory (translation
//   += playing ref - authored ref), not a native read of this build.
// Not drawn: the armor mesh (AddedArmorMesh: Jake has no armor packs in this build). The fart's PS_JakeGroundImpact
// is the sim's projectileDeath event (FartCloudVisuals @15311).

const HOUSE_COMP: &str = "JakeHouse";

fn house_machine() -> MachineDef<()> {
    let state = |name, seq, looped| StateDef {
        name,
        pose: Some(PoseDef::Seq(SeqDef::new(seq, looped))),
        entry_rule: None,
        always_reset: false,
    };
    MachineDef {
        entry: "Intro",
        reinit_on_becoming_relevant: false,
        max_transitions_per_frame: 3,
        skip_first_update_transition: true,
        states: vec![state("Intro", "JakeHouse_Intro", false), state("Idle", "JakeHouse_Idle", true)],
        transitions: vec![TransitionDef {
            from: "Intro",
            to: "Idle",
            xfade: 0.05,
            option: BlendOption::HermiteCubic,
            rule: Some(|_, q: &dyn MachineQuery| q.remaining("Intro") <= 0.0),
            auto: false,
        }],
    }
}

/// The skin's MeshSwap sub-meshes in order: JakeHouse_Actor @13200 GetMeshSwapSubmeshes of the owner's applied skin.
fn mesh_swaps(props: &[PropMesh]) -> Vec<&PropMesh> {
    props.iter().filter(|p| p.tags.iter().any(|t| t == "MeshSwap")).collect()
}

/// The AnimationRelative delta of a MeshSwap mesh: per bone of its skeleton (pose order) the translation to add, 0 for
/// the root and for a bone the House's rest lacks; None when every delta is 0 (the House itself).
fn retarget_delta(sk: &SkinnedActor, house: &crate::mesh::MeshInfo, mesh: &str) -> Option<Vec<f32>> {
    let names = house.bones();
    let rest = &house.rest;
    let mut d = vec![0.0f32; sk.names.len() * 3];
    let mut any = false;
    for (i, n) in sk.names.iter().enumerate() {
        let Some(h) = names.iter().position(|b| b == n) else { continue };
        if i as i32 == sk.root {
            continue;
        }
        let (o, q) = (i * 3, i * 4);
        for k in 0..3 {
            d[o + k] = sk.rest.t[o + k] - rest.t[h * 3 + k];
            if d[o + k].abs() > 1e-7 {
                any = true;
            }
            if (sk.rest.s[o + k] - rest.s[h * 3 + k]).abs() > 1e-4 {
                eprintln!(
                    "warning: Jake: {mesh} {n} rest scale differs from the House rest: the retarget takes the \
                     translation alone"
                );
            }
        }
        let dot: f32 = (0..4).map(|k| sk.rest.r[q + k] * rest.r[h * 4 + k]).sum();
        if dot.abs() < 1.0 - 1e-6 {
            eprintln!(
                "warning: Jake: {mesh} {n} rest rotation differs from the House rest: the retarget takes the \
                 translation alone"
            );
        }
    }
    any.then_some(d)
}

#[derive(Debug)]
struct HouseShown {
    sk: SkinnedActor,
    machine: ActorMachine<()>,
    slot: ActorSlot,
    base: Pose,
    out: Pose,
    delta: Option<Vec<f32>>,
}

#[derive(Debug)]
struct HouseMesh {
    id: mvs_sim::actors::ActorId,
    from: FighterId,
    shown: Option<HouseShown>,
    mesh_index: i32,
    last_tick: Option<i32>,
    scale: V3,
}

fn house_mesh(_host: &mut ActorHost<'_>, w: &World, a: ActorRef, cs: &ActorComponents) -> Option<Box<dyn ActorMesh>> {
    let ActorRef::Actor(id) = a else { return None };
    let x = w.actors.actors.get(id)?;
    let from = mvs_sim::fighter::common::actor_def_owner(w, x.owner, x.cls);
    let scale = cs.get(HOUSE_COMP).map_or([1.0; 3], |c| c.scale);
    Some(Box::new(HouseMesh { id, from, shown: None, mesh_index: -1, last_tick: None, scale }))
}

impl HouseMesh {
    /// The mesh of a MeshSwap index, with a new anim instance (SetSkeletalMesh reinitialises it).
    fn make(&mut self, host: &mut ActorHost<'_>, w: &World, owner: FighterId, cls: Name, index: i32) {
        self.shown = None;
        self.mesh_index = index;
        self.last_tick = None;
        let Some(lk) = look_of(host, w, owner, cls) else { return };
        let swaps = mesh_swaps(lk.props);
        let swap = usize::try_from(index).ok().and_then(|i| swaps.get(i)).copied();
        let info = swap.and_then(|s| prop_info(host, w, lk.from, s));
        let keys = swap.and_then(|s| host.draws_of(w, lk.from)?.set(&lk.name, &s.key, lk.variant).ok());
        let (Some(swap), Some(info), Some(keys)) = (swap, info, keys) else {
            eprintln!(
                "warning: Jake: the house mesh {index} is not drawn (no MeshSwap sub-mesh, its mesh or materials)"
            );
            return;
        };
        let mut sk = SkinnedActor::new(info, &keys);
        let house = (index > 0).then(|| swaps.first().copied()).flatten();
        let house_info = house.and_then(|h| prop_info(host, w, lk.from, h));
        if let Some(h) = house
            && house_info.is_none()
        {
            eprintln!("warning: Jake: {} not loaded: {} plays the House sequences unretargeted", h.mesh, swap.mesh);
        }
        let delta = house_info.and_then(|h| retarget_delta(&sk, &h, &swap.mesh));
        let Some(clips) = host.clips_of(w, lk.from) else { return };
        for n in ["JakeHouse_Intro", "JakeHouse_Idle"] {
            if clips.get(n).is_none() {
                eprintln!("warning: Jake: the house clip is not loaded: {n}");
            }
        }
        let n = sk.names.len();
        let _ = sk.skeleton(clips);
        let machine = ActorMachine::new(house_machine(), n, clips);
        self.shown = Some(HouseShown {
            sk,
            machine,
            slot: ActorSlot::new("DefaultSlot", n),
            base: Pose::new(n),
            out: Pose::new(n),
            delta,
        });
    }

    fn pose(&mut self, host: &mut ActorHost<'_>, w: &World, h: &JakeHouse, n: i32) {
        let Some(sh) = &mut self.shown else { return };
        let Some(clips) = host.clips_of(w, self.from) else { return };
        // (the Slot's source is updated while it passes it on: bAlwaysUpdateSourcePose false)
        let m = h.anim.m.as_ref().and_then(|st| {
            let m = mvs_sim::actors::actor_montage::montage_of(
                &w.data,
                w,
                ActorRef::Actor(self.id),
                w.names.str(st.montage.name),
            )?;
            Some((st, m))
        });
        let wt = m.map_or(0.0, |(st, m)| {
            let sw = st.shown_weight(m, &|s| eprintln!("warning: {s}"));
            if sw > ZERO_W && st.pose_in(m, "DefaultSlot").is_some() { sw } else { 0.0 }
        });
        if 1.0 - wt > ZERO_W {
            for _ in 0..n {
                sh.machine.update(&(), STEP, clips);
            }
        }
        let sks = sh.sk.skeleton(clips);
        sh.machine.evaluate(clips, &sks, &mut sh.base);
        let slotted = sh.slot.apply(clips, &sks, m, &sh.base, &mut sh.out);
        let shown = if slotted { &mut sh.out } else { &mut sh.base };
        if let Some(d) = &sh.delta {
            for (t, dv) in shown.t.iter_mut().zip(d) {
                *t += dv;
            }
        }
        sh.sk.show(shown);
    }
}

impl ActorMesh for HouseMesh {
    fn place(
        &mut self,
        host: &mut ActorHost<'_>,
        w: &World,
        _at: &ActorPlace,
        alpha: f64,
        shown: bool,
        out: &mut Vec<MeshPose>,
    ) {
        let Some(x) = w.actors.actors.get(self.id) else { return };
        let WorldActorState::JakeHouse(h) = &x.state else { return };
        if h.mesh_index != self.mesh_index {
            self.make(host, w, x.owner, x.cls, h.mesh_index);
        }
        if self.shown.is_none() || h.hidden {
            return;
        }
        let tick = w.flow.tick;
        if let Some(sh) = &mut self.shown {
            sh.sk.ticked(tick);
        }
        if self.last_tick != Some(tick) {
            let n = ticks_since(self.last_tick, tick);
            self.last_tick = Some(tick);
            self.pose(host, w, h, n);
        }
        let rot = if h.facing > 0.0 { right_rows() } else { left_rows() };
        // the root's scale (the sim's size_scale) over JakeHouse's relative one
        let r = x.size_scale.unwrap_or(1.0);
        let s = self.scale;
        let scale = [s[0] * f64::from(r), if h.mirrored { -s[1] } else { s[1] } * f64::from(r), s[2] * f64::from(r)];
        let Some(sh) = &mut self.shown else { return };
        sh.sk.set(&Xform { loc: [0.0, f64::from(-h.x), f64::from(h.y)], rot, scale });
        sh.sk.present(alpha);
        sh.sk.visible = shown;
        sh.sk.draw(out);
    }

    fn bones(&self, out: &mut Vec<(Arc<str>, Xform)>) {
        if let Some(sh) = &self.shown {
            sh.sk.bones_world(out);
        }
    }
}

// ---- JakeHorse_Actor3_C ----
//
// - JakeHouse (a SkeletalMeshComponent under the Capsule, scale 0.8, AnimClass Jake_Horse_Skeleton_AnimBlueprint_C):
//   the skin's Horse sub-mesh (@8062-8312), each slot SelectMaterialColor of that sub-mesh's material (@6147-6458: the
//   skin's set of the prop key Horse, the skin pick's variant). Scale (0.8, -0.8, 0.8) when the owner faced left at
//   BeginPlay (@6553-6589), kept after: SetMeshScale (@14704 / @15001, the IsLocallyControlled yaw branches) runs only
//   from the FacingChange event, which no class of the port's set calls.
// - Its world rotation: BeginPlay @367-430 yaw 90 facing left, -90 right; each tick @11504-11598 yaw by OldFacing (90 /
//   -90) before @11650 sets OldFacing to FacingLeft: a turn shows from the tick after the sim's FacingLeft changes.
// - Jake_Horse_Skeleton_AnimBlueprint, its MovementStateMachine (entry Intro, MaxTransitionsPerFrame 3,
//   bSkipFirstUpdateTransition; transitions in their baked order, crossfades HermiteCubic): Intro =
//   Jake_Horse_Intro_Short at rate 0.8, Run = DefaultSlot over the BlendSpacePlayer Horse_Run_BlendSpace (X Speed:
//   HORSE_BLEND_SPACES, bound with Jake's load; no montage plays on the horse), Fall = Jake_Horse_Leap1, Jump =
//   Jake_Horse_Jump, Turn = Jake_Horse_Turn (none looped). The inputs, from the horse's tick (@12306): Speed |velocity
//   Y| (the sim's |vx|), Walking = not falling, IsJumping = falling and velocity Z > 0; IsTurnning from TurnEvent
//   (@11727: true, RetriggerableDelay 0.2 -> @112 false). Turn_2 and TurnNumber are left out: the Turn notifies
//   (TurnStart / Turn1end / Turn2end) are in none of the sequences, so TurnNumber stays 0 and Turn -> Turn_2 never
//   passes.
// - ScoobySnackBuff: mvs-render fx/chars/jake.rs (the systems on the component).
// Deviation: the AnimBP and the turn delay run once a presented tick of the world (a rollback's jump runs the ticks
//   between with the latest state); the BeginPlay facing is the one the view first sees.
// Not drawn: the armor mesh (AddedArmorMesh: Jake has no armor packs in this build); the rider and the JumpOnUi
// prompt (the sim's rider is None and jump_on_ui false).

const HORSE_COMP: &str = "JakeHouse";

#[derive(Debug, Default)]
struct HorseIn {
    speed: f64,
    walking: bool,
    jumping: bool,
    turning: bool,
}

/// MovementStateMachine; `run`: the Run state's blend space player (Jake's bound horseRun).
fn horse_machine(run: crate::blend::BlendSpacePlayerData) -> MachineDef<HorseIn> {
    let seq = |seq, rate| {
        let mut d = SeqDef::new(seq, false);
        d.rate = rate;
        Some(PoseDef::Seq(d))
    };
    let state = |name, pose| StateDef { name, pose, entry_rule: None, always_reset: false };
    type Rule = fn(&HorseIn, &dyn MachineQuery) -> bool;
    let tr = |from, to, xfade, rule: Rule| TransitionDef {
        from,
        to,
        xfade,
        option: BlendOption::HermiteCubic,
        rule: Some(rule),
        auto: false,
    };
    MachineDef {
        entry: "Intro",
        reinit_on_becoming_relevant: false,
        max_transitions_per_frame: 3,
        skip_first_update_transition: true,
        states: vec![
            state("Intro", seq("Jake_Horse_Intro_Short", 0.8)),
            state("Run", Some(PoseDef::Space(SpaceDef { space: run, x: |i: &HorseIn| i.speed, y: None }))),
            state("Fall", seq("Jake_Horse_Leap1", 1.0)),
            state("Jump", seq("Jake_Horse_Jump", 1.0)),
            state("Turn", seq("Jake_Horse_Turn", 1.0)),
        ],
        transitions: vec![
            tr("Intro", "Fall", 0.0, |i, q| q.remaining("Intro") <= 0.0 && !i.walking),
            tr("Intro", "Run", 0.2, |i, q| q.remaining("Intro") < 0.2 && i.walking),
            tr("Run", "Jump", 0.0, |i, _| !i.walking && i.jumping),
            tr("Run", "Turn", 0.0, |i, _| i.turning && i.walking),
            tr("Run", "Fall", 0.2, |i, _| !i.walking && !i.jumping),
            tr("Fall", "Run", 0.1, |i, _| i.walking),
            tr("Jump", "Run", 0.1, |i, _| i.walking),
            tr("Turn", "Fall", 0.2, |i, _| !i.walking && !i.jumping),
            tr("Turn", "Run", 0.0, |i, q| q.remaining("Turn") == 0.0 && i.walking && !i.turning),
        ],
    }
}

#[derive(Debug)]
struct HorseMesh {
    id: mvs_sim::actors::ActorId,
    from: FighterId,
    sk: SkinnedActor,
    machine: ActorMachine<HorseIn>,
    pose: Pose,
    inp: HorseIn,
    scale: V3,
    last_tick: Option<i32>,
    /// FacingLeft at the last tick run.
    seen_left: Option<bool>,
    /// OldFacing.
    old_left: bool,
    /// The yaw drawn this tick: OldFacing before the tick's update (taken once a tick, as the place is).
    yaw_left: bool,
    /// BeginPlay's scale.
    mirrored: bool,
    /// IsTurnning until this tick.
    turn_until: f64,
}

fn horse_mesh(host: &mut ActorHost<'_>, w: &World, a: ActorRef, cs: &ActorComponents) -> Option<Box<dyn ActorMesh>> {
    let ActorRef::Actor(id) = a else { return None };
    let x = w.actors.actors.get(id)?;
    let scale = cs.get(HORSE_COMP).map_or([1.0; 3], |c| c.scale);
    // (JakeHorse_Actor @77-357, @645, @1211: the owner's applied skin's Horse entry, its mesh and materials)
    let lk = look_of(host, w, x.owner, x.cls)?;
    let prop = lk.props.iter().find(|p| p.key == "Horse");
    let info = prop.and_then(|p| prop_info(host, w, lk.from, p));
    let keys = prop.and_then(|p| host.draws_of(w, lk.from)?.set(&lk.name, &p.key, lk.variant).ok());
    let (Some(info), Some(keys)) = (info, keys) else {
        eprintln!("warning: Jake: the horse is not drawn (no Horse sub-mesh or its mesh is not loaded)");
        return None;
    };
    let run = host.blend_of(w, lk.from).map(|b| b.player("horseRun").clone())?;
    let mut sk = SkinnedActor::new(info, &keys);
    let clips = host.clips_of(w, lk.from)?;
    for n in ["Jake_Horse_Intro_Short", "Jake_Horse_Leap1", "Jake_Horse_Jump", "Jake_Horse_Turn"] {
        if clips.get(n).is_none() {
            eprintln!("warning: Jake: the horse clip is not loaded: {n}");
        }
    }
    let n = sk.names.len();
    let _ = sk.skeleton(clips);
    let machine = ActorMachine::new(horse_machine(run), n, clips);
    Some(Box::new(HorseMesh {
        id,
        from: lk.from,
        sk,
        machine,
        pose: Pose::new(n),
        inp: HorseIn { walking: true, ..HorseIn::default() },
        scale,
        last_tick: None,
        seen_left: None,
        old_left: false,
        yaw_left: false,
        mirrored: false,
        turn_until: -1.0,
    }))
}

impl HorseMesh {
    /// One tick of the actor's tick and its anim instance.
    fn tick_once(&mut self, h: &JakeHorse, tick: i32, clips: &Clips) {
        if self.seen_left.is_none() {
            self.seen_left = Some(h.facing_left);
            self.old_left = h.facing_left;
            self.mirrored = h.facing_left;
        }
        // @11504: the yaw by OldFacing, then @11650 OldFacing = FacingLeft and the mesh's TurnEvent
        if h.facing_left != self.old_left {
            self.old_left = h.facing_left;
            self.turn_until = f64::from(tick) + f64::from(mvs_sim::fighter::common::latent_frames(0.2));
        }
        self.inp.speed = f64::from(h.vx.abs());
        self.inp.walking = !h.falling;
        self.inp.jumping = h.falling && h.vy > 0.0;
        self.inp.turning = f64::from(tick) < self.turn_until;
        self.machine.update(&self.inp, STEP, clips);
    }
}

impl ActorMesh for HorseMesh {
    fn place(
        &mut self,
        host: &mut ActorHost<'_>,
        w: &World,
        _at: &ActorPlace,
        alpha: f64,
        shown: bool,
        out: &mut Vec<MeshPose>,
    ) {
        let Some(x) = w.actors.actors.get(self.id) else { return };
        let WorldActorState::JakeHorse(h) = &x.state else { return };
        let tick = w.flow.tick;
        self.sk.ticked(tick);
        if self.last_tick != Some(tick) {
            // (the yaw of the tick before this one's OldFacing update; every frame of the tick draws it)
            self.yaw_left = if self.seen_left.is_none() { h.facing_left } else { self.old_left };
            let n = ticks_since(self.last_tick, tick);
            self.last_tick = Some(tick);
            if let Some(clips) = host.clips_of(w, self.from) {
                for k in (0..n).rev() {
                    self.tick_once(h, tick - k, clips);
                }
                let sks = self.sk.skeleton(clips);
                self.machine.evaluate(clips, &sks, &mut self.pose);
                self.sk.show(&self.pose);
            }
        }
        let rot = if self.yaw_left { left_rows() } else { right_rows() };
        // the root's scale (the sim's size_scale) over JakeHouse's relative one
        let r = x.size_scale.unwrap_or(1.0);
        let s = self.scale;
        let scale = [s[0] * f64::from(r), if self.mirrored { -s[1] } else { s[1] } * f64::from(r), s[2] * f64::from(r)];
        self.sk.set(&Xform { loc: [0.0, f64::from(-h.x), f64::from(h.y)], rot, scale });
        self.sk.present(alpha);
        self.sk.visible = shown;
        self.sk.draw(out);
    }

    fn bones(&self, out: &mut Vec<(Arc<str>, Xform)>) {
        self.sk.bones_world(out);
    }
}
