//! Rick's render module: the entry, the AnimBP's pose hook, the pawn props
//! and the classes' mesh rules (the portals, the aimer, the projectiles, the Meeseeks).
//!
//! The skin's skeletal meshes and materials (C020_S00: BodyMesh, Head1 / Head2, SmallArm, ArmCannon, Whip, WhipArm,
//! CoiledWhip, Boots, PlasmaBlades) are drawn by the shared path; the slot graph is the sim's (RickState.slot_mix).
//! Facing (turns): C020_MirrorTableV2 has BoneFlipAxis None on fml_un_C_pelvis_att and every bone below it (503 of
//! 569 bones; Root, fml_un_COM and the prop, plasma blade, rope, marker and vfx bones under them are mirrored), as
//! C019's, and C020_AnimBPV2's OnFacingChanged (@0..1134, from BPI_SetIsFacingLeft @3209 and the init @1482) sets
//! RelativeScale3D (1, -1, 1) facing left, else (1, 1, 1), on every SkeletalMeshComponent tagged VisMesh (Body,
//! SkeletalMesh_Head1 / _Head2, SmallArm, MachineGun): the view's turns rule.
//! The skin's other skeletal components (ArmCannon, Whip, WhipArm, CoiledWhip, Boots, PlasmaBlades, Rope) are children
//! of Body with no VisMesh tag in Pawn_C020's SCS (SCS_Node_36's ChildNodes): one scale, Body's, as the body's own
//! meshes. So copied_props: every part is drawn as the view's plain reflection, which is the build's picture for the
//! kept bones.
//! Deviation: a part's vertices on a mirrored bone (PlasmaBlades' blade bones under Root) are, in the build, the node's
//! mirrored bone under that one scale: component-space F_B x T(partner(B)), the plain reflection reflected once more
//! in depth. Drawn here as the plain reflection.
//! The big head: C020_AnimBPV2's ModifyBone (fml_un_C_head, scale Additive, bone space, <- HeadScale; Pawn_C020
//! @1499).
//! The Polymorph creature on a victim is the shared second views' (crate::second).
//!
//! The pose hook: C020_AnimBPV2's sub-mesh toggles (BlueprintUpdateAnimation @1126 -> SetPropVisibility @3750 ->
//! @1867, when not IsPickleRick, behind VisibilityManager valid and InitialVisibilitySet, both true in a match).
//! Toggle Submesh Visibility once per state, in the push_flow order @1938..1998 (STATES), on the curve of the
//! evaluation:
//!   - GetCurveValue(CurveName) NearlyEqual (0.001) to the state's PreviousFrameCurveValue: nothing (@46..150).
//!   - else c = ClampAngle(curve, 0, 1); (c > prev) XOR Visible: Visible flips, prev = c and
//!     VisibilityManager.SetSubMeshVisibility(MeshTagName, Visible) (@342..640; MirrorMeshTag is None in every state
//!     of the CDO, so the tag is MeshTagName facing either way); otherwise prev = c alone (@871..964).
//!     So any rise shows a hidden mesh and any fall hides a shown one: an edge rule, not a threshold.
//!   - FaceVisibility switched: SetSubMeshVisibility(Head1, !FaceVisibility.Visibility) (@2618..2666).
//!   - @2735: none of ArmCannon, WhipArm, SmallArm Visible: SetSubMeshVisibility(SmallArm, true), Visible = true,
//!     prev = 1 (@2877..2962).
//!
//! Every state starts Visible false, prev 0 (the CDO). A curve no sequence carries (show_curled_whip) reads 0, so
//! CoiledWhip is never shown; MachineGun and Rope have no mesh in C020_S00 (the pawn's templates are empty), so their
//! calls find nothing. Blaster is the pawn's StaticMeshComponent (SM_C020_LaserGun on socket Prop): the pawn props
//! draw it by the hook's `shown`.
//! The AnimBP's write holds until the sim changes that mesh's visibility (a notify, an Equip, SetupInitialVisibility
//! at the respawn): the later write, as Bugs'. The respawn keeps the states (C020_AnimBPV2 binds no
//! OnSetInitialVisibility: its delegates are OnFacingChanged, SetHitStunned, SetIsRespawning and OnMontageStarted) and
//! drops the AnimBP's writes.
//! The curves: the clips' own (crate::curves) by the sequence the tick shows (the fighter's anim and anim time), the
//! slot mix's tracks (RickState.slot_mix) lerped in by their weights, and the view's crossfade, as LeBron's.
//!
//! Deviations and unverified models:
//!   - ClampAngle(curve, 0, 1) is the plain clamp to [0, 1] for |curve| < 180 (FMath::ClampAngle, exe 141354780,
//!     float32), so FaceModelVis's keys -0.052 and 1.168 (C020_Attack_Air_BlasterChargedFire) clamp to 0 and 1.
//!   - Unverified: a curve's value in a crossfade and under the slot mix's TwoWayBlends (each track's value by its
//!     weight) and the one tick the update lags the evaluation: as LeBron's pose. A montage's own blend-out weight is
//!     not in it.
//!   - Deviation: the AnimBP's writes are the view's: they do not reach the sim's sub_hidden.
//!   - a respawn is seen as a set() after ticks without one (the view is not posed while the fighter is dead), where
//!     dead() would be called each dead tick.
//!   - the headband (Headband): AnimDynamics (bound fml_un_sealTeam_headband1 .. 4, the C020_S02 headband) on a seal
//!     view, made from the fighter's data; alpha @3812..4196 = int(IsSealTeamRick) x (SealTeamSixDynamicsDisable <
//!     0.1 ? 0.9 : 0) x !Is Hit Stunned x !Is Respawning (0 for S00, whose mesh has no headband). It sits between
//!     the skin slots and the mirror node (LocalToComponent_1 .. ComponentToLocal), so it runs on the unmirrored
//!     local pose under the component's own transform (mesh_xf), before the head scale. Is Respawning is true from
//!     the respawn (the set() after a gap, above) for SetIsRespawning's Delay(0.1); Is Hit Stunned is the fighter's
//!     in_hitstun (Unverified: the same reading as Reindog's). Not run in a preview (no fighter data).
//!   - not ported: ModifyBone_1 (fml_un_sealTeam_headband1, scale Replace by (1, 1, 1): the clips' scale of that bone
//!     is within 0.03 of 1, at one frame of C020_Attack_Air_Forcefield, and within 0.002 elsewhere); ModifyBone_2 (VB
//!     Spine, Replace by identity in world space: a virtual bone the view does not hold); the skin slot PickleSkin
//!     (IsPickleRick: no writer, CDO false).
//!   - not ported: the check of the data's slot graph against the AnimBP's Slot nodes.
//!   - a preview actor (the podium, the menus) runs the toggles on the shown clip's curves (PoseHook::preview_step): it
//!     has no fighter, but the toggles test only the owning actor's VisibilityManager.
//!
//! The skin slot SealTeamSkin (IsSealTeamRick = AppliedSkin == C020_S02, @420..499): the client applies the player's
//! pick, so a C020_S02 view draws that track. In a match off a dedicated server the sim walks it already (Fighter.anim
//! is the S02 sequence, the hurt shapes ride it). In a server's match the sim walks NoSkin (the server's
//! skin_c020_server); the view then draws the SealTeamSkin segment at the montage position of the NoSkin one
//! ([`seal_of`]; the ground and air BlasterChargeShot montages hold the only tracks of that slot), the pose (the hook's
//! `sample`) and the curves of the sub-mesh toggles (show_machineGun is 1 on those sequences: the MachineGun shows;
//! show_blaster is absent there, 0).
//!
//! The pawn props: Pawn_C020's StaticMeshComponents, children of CharacterMesh0 on its socket Prop (fml_un_C_prop1).
//! In the SCS (SCS_Node_10, 17, 18, 19, 29, 31): Blaster (SM_C020_LaserGun), MeeseeksBox (SM_MeeseeksBox, tag
//! Meseeks), PortalGun (SM_C020PortalGun), PolymorphGun, Mic (C020_MicProp, Z -2.5) and Megaseed (C020_Megaseed), each
//! yaw -90 but the box (Mic -89.999985), bVisible false. Each is in Comp_Pawn_MeshManager.Meshes under its tag (the
//! sim's static sub-meshes), hidden by SetupInitialVisibility and shown by the montages' AnimNotify_Equip (MeshTag
//! Blaster, Meseeks, PortalGun, PolymorphGun, Megaseed): the sim keeps that state (sub_hidden). The Blaster is also
//! C020_AnimBPV2's BlasterVisibility (show_blaster): the hook's `shown`.
//! Materials: the skin's static SubMeshes (C020_S00: Blaster, PortalGun, PolymorphGun, Mic, Megaseed; the draws'
//! `C020_S00.<key>` entries), with SelectMaterialColor's pick (Comp_Pawn_SkinManager.ApplySubMeshes @1405-1514): the
//! fighter's skin variant (selected_variant), as his body's. The skin's Meeseeks entry names a key no component is
//! filed under (the box's tag is Meseeks), so the box keeps its template's mesh and its OverrideMaterials [null]: the
//! mesh's own slot (the actors' SM_MeeseeksBox entry).
//! AnimNotify_ToggleRickBlaster (Pawn_C020.Blaster.SetVisibility(Equip), not through the mesh manager) is in the
//! dormant C020_BlasterEquip_Montage / C020_BlasterUnequip_Montage alone; the sim does not enter it.
//!   - Unverified: the socket is the last rendered pose's.
//!   - Deviation: a prop is placed each frame at the frame's alpha, on the socket of the pose just presented, not
//!     once a tick on the tick's pose.
//!   - CharacterMesh0 is not scaled by OnFacingChanged (no VisMesh tag): the props are on the view's bone, the plain
//!     reflection facing left (as Shaggy's whole mesh, unverified).
//!   - another skin's props (propLook's skinStatic path): the static entry `<skin>.<tag>` of the draws, else
//!     the template's mesh in its own materials (ActorHost::skin_static).

use std::collections::BTreeSet;
use std::sync::Arc;

use indexmap::IndexMap;

use mvs_anim::montage::Montage;
use mvs_data::Value;
use mvs_data::fighter::Segment;
use mvs_data::value::Props;
use mvs_sim::World;
use mvs_sim::actors::actor_montage::montage_of;
use mvs_sim::actors::projectile::{Projectile, ProjectileExt};
use mvs_sim::actors::{ActorId, ActorRef, ProjectileId, WorldActor, WorldActorState};
use mvs_sim::chars::rick::meeseeks::MeeseeksSummon;
use mvs_sim::chars::rick::portal::PortalActor;
use mvs_sim::chars::rick::state::rick_of;
use mvs_sim::fighter::MontageState;
use mvs_sim::fighter::common::actor_def_owner;
use mvs_sim::fighter::{FighterId, in_hitstun, shown_montage};
use mvs_sim::names::Name;
use mvs_vfx::math::{V3, compose_rows, rotation_rows};

use super::{CharRender, PawnActor, PawnCx, PoseHook, PoseOf, PreviewOf, PreviewStep};
use crate::actor_anim::{
    ActorMachine, ActorSlot, BlendOption, BoolListDef, MachineDef, MachineQuery, PoseDef, SeqDef, StateDef,
    TransitionDef,
};
use crate::actor_view::{Comp, SkinnedActor, StaticView, Xform, comp_of, place_at, rel_rows, xyz};
use crate::actors::{ActorComponents, ActorHost, ActorMesh, ActorPlace, ComponentKind, MeshRule, Rows, class_def};
use crate::anim::{Clips, HeadScaleNode, Pose, blend_pose};
use crate::anim_nodes::{AnimNode, AnimNodeSkel, CsPose, RespawnFlag, UeXf, mesh_xf, nodes_of_data};
use crate::curves::{CurveKey, CurveTable, rich_curve};
use crate::frame::{ActorPose, MeshPose};
use crate::view::{CharView, PoseCx, PoseRules};

pub(crate) const RENDER: CharRender = CharRender {
    head_scale: &[HeadScaleNode {
        node: "ModifyBone",
        bone: "fml_un_C_head",
        factor: 1.0,
        gated: false,
        moves: [0.0; 3],
        scale: true,
    }],
    turns: true,
    copied_props: true,
    pose: Some(rick_pose),
    pawn_actors: Some(pawn_props),
    ..CharRender::GENERIC
};

/// The classes' own mesh drawing, by class name.
pub(crate) const RULES: &[(&str, MeshRule)] = &[
    (PORTAL_CLASSES[0], MeshRule { rot: None, mesh: Some(portal_mesh) }),
    (PORTAL_CLASSES[1], MeshRule { rot: None, mesh: Some(portal_mesh) }),
    (PORTAL_CLASSES[2], MeshRule { rot: None, mesh: Some(portal_mesh) }),
    (AIMER, MeshRule { rot: None, mesh: Some(aimer_gun) }),
    (MISSILE, MeshRule { rot: None, mesh: Some(missile_mesh) }),
    (FARTBOMB, MeshRule { rot: None, mesh: Some(fart_bomb_mesh) }),
    (AIRFLAP, MeshRule { rot: None, mesh: Some(air_flap_mesh) }),
    (MEESEEKS_SUMMON, MeshRule { rot: None, mesh: Some(summon_mesh) }),
];

// ---- the pose hook ------------------------------------------------------------------------------------

/// The float curves its AnimBP reads by name (GetCurveValue), in the order the rule indexes them (`RICK_CURVES`).
const CURVES: CurveTable = CurveTable::new(&[
    "showSmallArm",
    "showArmCannon",
    "show_r_plasmabladeHandle",
    "show_rope",
    "showWhip",
    "showArmWhip",
    "BootsVis",
    "FaceModelVis",
    "show_machineGun2",
    "show_machineGun",
    "show_blaster",
    "SealTeamSixDynamicsDisable",
]);
const NC: usize = 12;
/// The headband's curve (CURVES).
const C_HEADBAND: usize = 11;

/// The Toggle Submesh Visibility calls in their order (@1998, 2343, 2274, 2205, 2136, 2067, 2412, 2481, 2550, then
/// the SmallArm rule @2735, then 2995, 3064, 3133): the CDO's CurveName and MeshTagName of each state.
const STATES: [(&str, &str); 12] = [
    ("showSmallArm", "SmallArm"),                 // SmallArmVisibility
    ("showArmCannon", "ArmCannon"),               // ArmCannonVisibility
    ("show_r_plasmabladeHandle", "PlasmaBlades"), // PlasmaBladesVisibility
    ("show_rope", "Rope"),                        // RopeVisibility
    ("showWhip", "Whip"),                         // WhipVisibility
    ("showArmWhip", "WhipArm"),                   // WhipArmVisibility
    ("show_curled_whip", "CoiledWhip"),           // CoiledWhipVisibility
    ("BootsVis", "Boots"),                        // BootsVisibility
    ("FaceModelVis", "Head2"),                    // FaceVisibility (and Head1 @2666)
    ("show_machineGun2", "MachineGun"),           // MachineGun2Visibility
    ("show_machineGun", "MachineGun"),            // MachineGunVisibility
    ("show_blaster", "Blaster"),                  // BlasterVisibility
];
const NS: usize = 12;
const S_SMALL: usize = 0;
const S_CANNON: usize = 1;
const S_WHIPARM: usize = 5;
const S_FACE: usize = 8;
/// The first state after the SmallArm rule @2735.
const S_AFTER_ARM: usize = 9;
/// NearlyEqual_FloatFloat's tolerance @46.
const NEARLY: f64 = 0.001;
const HEAD1: &str = "head1";
/// C020_AnimBPV2 @439: EqualEqual_ObjectObject(AppliedSkin, SkinData'C020_S02').
const SEAL_SKIN: &str = "C020_S02";
const SEAL_SLOT: &str = "SealTeamSkin";
const NO_SKIN: &str = "NoSkin";

/// A montage's NoSkin segments by sequence, each with the SealTeamSkin segment of the same index (`sealPlan`).
type SealPlan = IndexMap<String, (Segment, Segment)>;

/// `sealPlan` of every montage of the character with a SealTeamSkin track, by montage name (None: a track not on
/// the NoSkin segments' starts, warned: the NoSkin pose is drawn).
fn seal_plans(w: &World, f: FighterId) -> IndexMap<Name, Option<SealPlan>> {
    let mut out = IndexMap::new();
    for (name, m) in &w.char_data(f).fighter.montages {
        let ns = m.slots.iter().find(|s| s.slot == NO_SKIN);
        let st = m.slots.iter().find(|s| s.slot == SEAL_SLOT);
        let (Some(ns), Some(st)) = (ns, st) else { continue };
        if st.segments.is_empty() {
            continue;
        }
        let Some(key) = w.names.get(name) else { continue };
        let off = ns.segments.len() != st.segments.len()
            || ns.segments.iter().zip(&st.segments).any(|(g, s)| (g.start - s.start).abs() > 1e-4);
        let plan = if off {
            eprintln!(
                "warning: Rick pose: {name}'s SealTeamSkin track is not on the NoSkin segments: the NoSkin pose is drawn"
            );
            None
        } else {
            Some(ns.segments.iter().zip(&st.segments).map(|(g, s)| (g.anim.clone(), (g.clone(), s.clone()))).collect())
        };
        out.insert(key, plan);
    }
    out
}

/// `sealOf`: the SealTeamSkin sequence and its time for the NoSkin sequence `anim` at `time` of a montage's plan: the
/// montage position of that time, in the SealTeamSkin segment of the same index at its own rate (the air Fire segment
/// is anim 0-0.6667 at 1.326 on NoSkin, 0-0.5 at 0.9945 on SealTeamSkin), clamped to the segment. None: none (the sim
/// walks it already).
fn seal_of<'a>(plan: Option<&'a SealPlan>, anim: &str, time: f64) -> Option<(&'a str, f64)> {
    let (no, seal) = plan?.get(anim)?;
    if no.rate <= 0.0 {
        return None;
    }
    let pos = no.start + (time - no.a0) / no.rate;
    Some((&seal.anim, (seal.a0 + (pos - seal.start) * seal.rate).max(seal.a0).min(seal.a1)))
}

/// `rickPose`'s state on one view.
#[derive(Debug)]
struct RickPose {
    /// The tags the states and the Head1 rule write (lower case), and the view's part of each (None: none, as
    /// Blaster).
    tags: Vec<String>,
    part_of: Vec<Option<usize>>,
    st_tag: [usize; NS],
    head1: usize,
    /// Per state: its curve's index (None: on no sequence, reads 0).
    curve_ix: [Option<usize>; NS],
    /// The curves: the sequence's (tgt), the evaluation's (cur) and the pose the running fade started from (from).
    tgt: [f64; NC],
    cur: [f64; NC],
    from: [f64; NC],
    fades: u32,
    /// The states (Visible, PreviousFrameCurveValue).
    visible: [bool; NS],
    prev: [f64; NS],
    /// Per tag: the AnimBP's last write (None: none) and the sim's last visibility of the mesh (None: not seen).
    own: Vec<Option<bool>>,
    sim_vis: Vec<Option<bool>>,
    /// The World tick of the last set() (a gap: the fighter was dead between).
    last_tick: Option<i32>,
    live: bool,
    /// PoseOf::slaved: no AnimBP of its own (set does nothing).
    slaved: bool,
    /// PoseOf::stand_in: no character state (no slot mix).
    stand_in: bool,
    /// A C020_S02 view with an AnimBP of its own: the SealTeamSkin rule (the header).
    seal: bool,
    /// The SealTeamSkin plans (`seal_plans`), made at the first set() of a seal view.
    plans: Option<IndexMap<Name, Option<SealPlan>>>,
    /// The SealTeamSkin clips found missing (warned once each).
    missing: BTreeSet<String>,
    /// A seal view's headband (None: another skin, or a view with no AnimBP of its own).
    headband: Option<Headband>,
}

fn rick_pose(view: &mut CharView, _clips: &Clips, of: &PoseOf<'_>) -> Box<dyn PoseHook> {
    let mut tags: Vec<String> = Vec::new();
    for t in STATES.iter().map(|(_, t)| t.to_lowercase()).chain([HEAD1.to_owned()]) {
        if !tags.contains(&t) {
            tags.push(t);
        }
    }
    let ix = |t: &str| tags.iter().position(|x| x.eq_ignore_ascii_case(t)).expect("a state's tag");
    let st_tag = STATES.map(|(_, t)| ix(t));
    let head1 = ix(HEAD1);
    let curve_ix = STATES.map(|(c, _)| {
        let k = CURVES.find(c);
        assert!(k.is_some() || c == "show_curled_whip", "RICK_CURVES has no curve {c}");
        k
    });
    let part_of = tags.iter().map(|t| of.part_keys.iter().position(|k| k.eq_ignore_ascii_case(t))).collect();
    let n = tags.len();
    Box::new(RickPose {
        part_of,
        st_tag,
        head1,
        curve_ix,
        tgt: [0.0; NC],
        cur: [0.0; NC],
        from: [0.0; NC],
        fades: 0,
        visible: [false; NS],
        prev: [0.0; NS],
        own: vec![None; n],
        sim_vis: vec![None; n],
        last_tick: None,
        live: false,
        slaved: of.slaved,
        stand_in: of.stand_in,
        tags,
        seal: of.skin.name == SEAL_SKIN && !of.slaved,
        plans: None,
        missing: BTreeSet::new(),
        headband: (of.skin.name == SEAL_SKIN && !of.slaved).then(|| Headband::new(view)),
    })
}

impl RickPose {
    /// Toggle Submesh Visibility on state `k`: whether Visible flipped.
    fn toggle(&mut self, k: usize) -> bool {
        let v = self.curve_ix[k].map_or(0.0, |i| self.cur[i]);
        if (self.prev[k] - v).abs() <= NEARLY {
            return false;
        }
        let c = v.clamp(0.0, 1.0); // ClampAngle(v, 0, 1)
        if (c > self.prev[k]) != self.visible[k] {
            self.visible[k] = !self.visible[k];
            self.prev[k] = c;
            self.own[self.st_tag[k]] = Some(self.visible[k]);
            return true;
        }
        self.prev[k] = c;
        false
    }
}

impl RickPose {
    /// SetPropVisibility (@1867): the toggles in their order, Head1 by the face's, the SmallArm fallback (@2735); each
    /// write into `own`.
    fn set_prop_visibility(&mut self) {
        for k in 0..S_AFTER_ARM {
            if self.toggle(k) && k == S_FACE {
                self.own[self.head1] = Some(!self.visible[k]);
            }
        }
        if !self.visible[S_CANNON] && !self.visible[S_WHIPARM] && !self.visible[S_SMALL] {
            self.own[self.st_tag[S_SMALL]] = Some(true);
            self.visible[S_SMALL] = true;
            self.prev[S_SMALL] = 1.0;
        }
        for k in S_AFTER_ARM..NS {
            self.toggle(k);
        }
    }
}

impl PoseRules for RickPose {
    /// `sampleRule` (a seal view): the SealTeamSkin sequence in place of the NoSkin one the sim names.
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
        let (Some(plans), Some(m)) = (&self.plans, montage) else { return false };
        let name = &cx.clips.clips[clip].name;
        let Some((anim, t)) = seal_of(plans.get(&m).and_then(Option::as_ref), name, time) else { return false };
        let Some(c) = cx.clips.get(anim) else {
            if self.missing.insert(anim.to_owned()) {
                eprintln!("warning: Rick pose: clip {anim} is not loaded: the NoSkin pose is drawn");
            }
            return false;
        };
        cx.sample(c, t, out);
        true
    }

    /// This evaluation's curves (the view's crossfade of the targets), for the next update.
    fn rule(&mut self, cx: &mut PoseCx<'_>, p: &mut Pose, dt: f64) {
        if cx.fade_count != self.fades {
            self.fades = cx.fade_count;
            self.from = self.cur;
        }
        let w = cx.fade_weight;
        for k in 0..NC {
            self.cur[k] = if w >= 1.0 { self.tgt[k] } else { self.from[k] + (self.tgt[k] - self.from[k]) * w };
        }
        if let Some(h) = &mut self.headband
            && self.live
        {
            h.apply(p, dt);
        }
    }
}

impl PoseHook for RickPose {
    /// `preview()`: a view with no fighter: the AnimBP anew; its toggles run in preview_step.
    fn preview(&mut self, _view: &mut CharView, _of: &PreviewOf<'_>) {
        self.reset(&mut |_, _, _| {});
        self.live = false;
    }

    /// The preview's update (as gizmo.rs's): on BP_CharacterPreview the update (@1126, !IsPickleRick) reaches
    /// SetPropVisibility's toggles (@1867-1998) behind IsValid(VisibilityManager), the owning actor's (@35-220; the
    /// Fighter test @3267 ends only its own branch), and InitialVisibilitySet (PreviewView::new's
    /// SetupInitialVisibility): the toggles on the curves of the evaluation before, each write of this update put on
    /// its part (the AnimBP is the parts' only writer here), then the curves of the frame's sequence.
    fn preview_step(&mut self, view: &mut CharView, clips: &Clips, s: &PreviewStep<'_>) {
        self.own.fill(None);
        self.set_prop_visibility();
        for t in 0..self.tags.len() {
            if let (Some(on), Some(p)) = (self.own[t], self.part_of[t]) {
                view.set_part_visible(p, on);
            }
        }
        CURVES.fill(clips, s.anim, s.time, &mut self.tgt);
    }

    /// The AnimBP's update of the tick, before the view's pose: the toggles on the curves of the evaluation before,
    /// the parts' visibility, then this tick's curve targets.
    fn set(&mut self, w: &World, f: FighterId, view: &mut CharView, clips: &Clips) {
        self.live = !self.slaved;
        if self.last_tick.is_some_and(|t| w.flow.tick > t + 1) {
            // the respawn: SetupInitialVisibility's write is the later one (the states are kept)
            self.own.fill(None);
        }
        self.last_tick = Some(w.flow.tick);
        if !self.live {
            return;
        }
        let fi = w.fighter(f);
        if self.seal && self.plans.is_none() {
            self.plans = Some(seal_plans(w, f));
        }
        // BlueprintUpdateAnimation on the curves of the evaluation before (IsPickleRick is false: SetPropVisibility)
        if let Some(h) = &mut self.headband {
            h.set(w, f, view, self.cur[C_HEADBAND]);
        }
        self.set_prop_visibility();
        // the AnimBP's write until the sim changes the mesh (the later write); a tag the sim does not file (a skin's
        // own part: C020_S02's Rope, MachineGun) starts hidden, as SetupInitialVisibility leaves a mesh without
        // PrimaryMesh, until the AnimBP's curves write it
        let subs = &w.char_data(f).def.sub_meshes;
        for t in 0..self.tags.len() {
            let tag = &self.tags[t];
            let hidden = w.names.get(tag).is_some_and(|n| fi.pose.sub_hidden.contains(&n));
            let vis = subs.contains(tag) && !hidden;
            if self.sim_vis[t].is_some_and(|s| s != vis) {
                self.own[t] = None;
            }
            self.sim_vis[t] = Some(vis);
            if let Some(p) = self.part_of[t] {
                view.part_on[p] = self.own[t].unwrap_or(vis);
            }
        }
        // the sequences this tick's pose is sampled from: the fighter's anim, and the slot mix's tracks lerped in order
        let shown = shown_montage(w, f).and_then(|dash| {
            let m = if dash { &fi.montages.dash_montage } else { &fi.montages.montage };
            m.as_ref().map(|s| s.montage.name)
        });
        let plan = self.plans.as_ref().zip(shown).and_then(|(p, m)| p.get(&m)).and_then(Option::as_ref);
        match seal_of(plan, w.names.str(fi.anim.anim), f64::from(fi.anim.anim_time)) {
            Some((anim, t)) => CURVES.fill(clips, anim, t, &mut self.tgt),
            None => CURVES.fill(clips, w.names.str(fi.anim.anim), f64::from(fi.anim.anim_time), &mut self.tgt),
        }
        if let Some(s) = rick_of(w, f).filter(|_| !self.stand_in) {
            for x in &s.slot_mix.slot_mix {
                let clip = clips.get(w.names.str(x.anim));
                for k in 0..NC {
                    self.tgt[k] += (CURVES.value_of(clips, clip, k, f64::from(x.time)) - self.tgt[k]) * f64::from(x.w);
                }
            }
        }
    }

    /// `reset`: a new pawn, the AnimBP's state anew (init()).
    fn reset(&mut self, _part_scalar: &mut dyn FnMut(usize, &str, f64)) {
        self.tgt = [0.0; NC];
        self.cur = [0.0; NC];
        self.from = [0.0; NC];
        self.visible = [false; NS];
        self.prev = [0.0; NS];
        self.own.fill(None);
        self.sim_vis.fill(None);
        self.last_tick = None; // wasDead = false
        if let Some(h) = &mut self.headband {
            h.node = None;
            h.respawning.reset();
        }
    }

    /// Whether the mesh of a tag (lower case) is shown: the AnimBP's last write while the sim has not changed the
    /// mesh since, else the sim's.
    fn shown(&self, tag: &str, sim: bool) -> bool {
        match self.tags.iter().position(|t| t == tag) {
            Some(t) if self.live => self.own[t].unwrap_or(sim),
            _ => sim,
        }
    }
}

// ---- the C020_S02 headband (AnimDynamics) -----------------------------------------------------------------------

/// The node of the headband.
const HEADBAND: &str = "AnimGraphNode_AnimDynamics";
/// SelectFloat(0.9, 0, SealTeamSixDynamicsDisable < 0.1) (@3945-4020).
const HEADBAND_ALPHA: f64 = 0.9;
/// Less_FloatFloat's bound on the curve (@3986).
const HEADBAND_CURVE_MAX: f64 = 0.1;

/// The AnimDynamics node on a C020_S02 view: made from the fighter's C020_AnimBPV2 data at the first set.
#[derive(Debug)]
struct Headband {
    /// None before the first set; Some(None) when the view's mesh lacks the headband's bones.
    node: Option<Option<Box<dyn AnimNode>>>,
    cs: CsPose,
    comp: UeXf,
    alpha: f64,
    dilation: f64,
    respawning: RespawnFlag,
}

impl Headband {
    fn new(view: &CharView) -> Headband {
        Headband {
            node: None,
            cs: CsPose::new(&view.rig.names, &view.parents),
            comp: UeXf::default(),
            alpha: 0.0,
            dilation: 1.0,
            respawning: RespawnFlag::default(),
        }
    }

    /// The node made, then InitAnim's DeltaTime-0 tick (crate::anim_nodes header).
    fn make(&mut self, w: &World, f: FighterId, view: &CharView) -> Option<Box<dyn AnimNode>> {
        let skel = AnimNodeSkel { names: &view.rig.names, parents: &view.parents, rest: Some(&view.rest) };
        let Some(data) = w.char_data(f).fighter.anim_nodes.as_ref() else {
            eprintln!("warning: Rick pose: the fighter data has no anim nodes: the headband is not run");
            return None;
        };
        let mut n = nodes_of_data(data, &[HEADBAND], &skel).0.nodes.pop()?;
        n.initialize();
        n.pre_update(1.0);
        n.update(0.0, self.alpha);
        Some(n)
    }

    /// BlueprintUpdateAnimation's inputs (@3812-4196): the alpha int(IsSealTeamRick) x (curve < 0.1 ? 0.9 : 0) x
    /// !Is Hit Stunned x !Is Respawning (IsSealTeamRick: a seal view), on the curve of the evaluation before.
    fn set(&mut self, w: &World, f: FighterId, view: &CharView, curve: f64) {
        let respawning = self.respawning.step(w.flow.tick);
        let fi = w.fighter(f);
        let on = curve < HEADBAND_CURVE_MAX && !in_hitstun(w, f) && !respawning;
        self.alpha = if on { HEADBAND_ALPHA } else { 0.0 };
        if self.node.is_none() {
            self.node = Some(self.make(w, f, view));
        }
        self.dilation = if fi.hit.hitpause > 0.0 { f64::from(mvs_sim::consts::HITPAUSE_DILATION) } else { 1.0 };
        self.comp = mesh_xf(
            f64::from(-mvs_sim::fighter::mesh_wx(w, f)),
            f64::from(mvs_sim::fighter::mesh_wy(w, f)) + view.mesh_z * view.size_scale,
            f64::from(fi.body.facing_dir),
            view.mesh_scale * view.size_scale,
        );
    }

    /// The node's update and evaluation on the local pose (before the mirror node and the head scale: the graph's
    /// order, the header).
    fn apply(&mut self, p: &mut Pose, dt: f64) {
        let Some(Some(n)) = &mut self.node else { return };
        n.pre_update(1.0);
        n.update(dt * self.dilation, self.alpha);
        let mut cs = self.cs.begin(p);
        n.evaluate(&mut cs, &self.comp);
    }
}

// ---- the pawn props ----------------------------------------------------------------------------------

/// By component tag (lower case, the sim's sub_hidden's): the materials' key (the draws' actors entry) of the default
/// skin.
const PROPS: [(&str, &str); 6] = [
    ("blaster", "C020_S00.Blaster"),
    ("meseeks", "SM_MeeseeksBox"),
    ("portalgun", "C020_S00.PortalGun"),
    ("polymorphgun", "C020_S00.PolymorphGun"),
    ("mic", "C020_S00.Mic"),
    ("megaseed", "C020_S00.Megaseed"),
];
/// Comp_Pawn_MeshManager's role tags.
const MESH_ROLE_TAGS: [&str; 4] = ["RecieveArmor", "IgnoreArmor", "VisMesh", "PrimaryMesh"];

/// One pawn prop: its tag, its view and its attach.
#[derive(Debug)]
struct PawnProp {
    tag: String,
    view: StaticView,
    socket: String,
    rel: Xform,
}

/// `pawnProps`: Pawn_C020's static sub-meshes (the static sub-meshes: a StaticMeshComponent with a
/// Meshes tag and no UI tag), in the data's order, each with its skin's mesh and materials (propLook).
fn pawn_props(host: &ActorHost<'_>, w: &World, f: FighterId) -> Vec<Box<dyn PawnActor>> {
    let d = &w.char_data(f).fighter;
    let pick =
        usize::try_from(w.fighter(f).id).ok().and_then(|i| w.setup.players.get(i)).and_then(|p| p.skin.as_deref());
    let skin = crate::skins::skin_of(d, pick).ok();
    let default = crate::skins::skin_of(d, None).ok().map(|s| s.name.clone());
    let own_skin = skin.as_ref().map(|s| s.name.clone()).filter(|s| Some(s) != default.as_ref());
    let mut out: Vec<Box<dyn PawnActor>> = Vec::new();
    for c in d.components.pawn.values() {
        if c.ty != "StaticMeshComponent" {
            continue;
        }
        let Some(p) = c.p.as_obj() else { continue };
        let tags: Vec<&str> = p
            .get("ComponentTags")
            .and_then(Value::as_arr)
            .map_or(Vec::new(), |t| t.iter().filter_map(Value::as_str).collect());
        let Some(real) = tags.iter().find(|t| !MESH_ROLE_TAGS.contains(t)) else { continue };
        if tags.contains(&"UI") {
            continue;
        }
        let tag = real.to_lowercase();
        let st = match &own_skin {
            // (skinStatic: the skin's static entry of the tag, else the template's mesh in its own materials)
            Some(_) => host.skin_static(w, f, p),
            None => PROPS.iter().find(|(t, _)| *t == tag).map(|(_, k)| ((*k).to_owned(), host.selected_variant(w, f))),
        };
        let Some((key, variant)) = st else {
            warn_once(format!("warning: Rick: a static sub-mesh with no prop entry: {tag}"));
            continue;
        };
        let Some(mut view) = StaticView::new(host, w, f, &key, variant) else {
            warn_once(format!(
                "warning: Rick: the prop {tag} is not drawn (its mesh or materials are not loaded: {key})"
            ));
            continue;
        };
        view.visible = false;
        let rel = Xform {
            loc: xyz(p.get("RelativeLocation"), 0.0),
            rot: rel_rows(p.get("RelativeRotation")),
            scale: xyz(p.get("RelativeScale3D"), 1.0),
        };
        let socket = c.socket.clone().unwrap_or_else(|| "None".to_owned());
        out.push(Box::new(PawnProp { tag, view, socket, rel }));
    }
    out
}

impl PawnActor for PawnProp {
    /// `placePawnProps`: shown by the AnimBP's write while the sim has not changed the mesh since, else the sim's;
    /// never while dead or under AddInvisibility.
    fn present(&mut self, host: &ActorHost<'_>, w: &World, cx: &PawnCx<'_>, _alpha: f64, out: &mut Vec<ActorPose>) {
        self.view.visible = cx.shown(w, &self.tag);
        if !self.view.visible {
            return;
        }
        let Some(at) = host.socket(w, cx.fid, &self.socket, &self.rel) else { return };
        self.view.set(&at);
        let mut meshes = Vec::new();
        self.view.draw(&mut meshes);
        cx.outline(true, &mut meshes);
        out.push(ActorPose { meshes, ..ActorPose::default() });
    }
}

// ---- the portals -----------------------------------------------------------------------------------
//
// Rick's portals (PortalParent_C's children AutoTeleportPortal_C, VisualOnlyPortal_C, ExitPortalParent_C; offsets of
// PortalParent's ubergraph). The sim's view fields: spawn_tick, mesh_at, outro_tick, only_visuals_tick, through_tick /
// through_in, dead.
// Components (the class data, the same in the four classes): PortalMesh = SM_C020Portal under the Capsule at
// (0, -100, 0), yaw -90, scale (1, 195, 292), OverrideMaterials [C020Portal00_Mesh_MAT_Inst], bVisible false.
// VFXAttach (a SceneCaptureComponent2D used as an attach point) under PortalMesh, scale (1, .005128, .003425). The
// actor's rotation is the spawn's yaw -90 (Comp_Pawn_Portal MakeTransform @1982), so the mesh sits at the actor +
// (-100, 0, 0), yaw -180: the unit plane of SM_C020Portal (2 x 2 cm in UE Y / Z) spans 390 x 584. Its Z is the
// actor's (SpawnHitbox alone sits at Z 30). FacingLeft changes no drawing.
// - Delay(0.5) @5575 -> @451 PortalMesh.SetVisibility(true) (the sim's mesh_at), then @505-891 when
//   GetUIBrokerForFighter(the Instigator) succeeds: the vector Original on the mesh's materials = the broker's
//   FocusColor when its IsLocal, else TeamColor (SelectVector @827, Conv_LinearColorToVector: alpha 1)
//   (ActorHost::broker_colour).
// - MulticastFighterTeleportedEffects @18018 (the sim's through_tick): FighterThroughPortal.PlayFromStart @18276
//   (length 1, CurveFloat_0 linear 10 -> 0 over 0.3 s): each update sets the scalar WPO = alpha x 0.5 on the mesh
//   @18351 (and the float WPO on PortalVFX @16456: the effects').
// - PortalProjectileOnlyVisuals @18886 (VisualOnlyPortal, the sim's only_visuals_tick): VisualsScaleTimeline (length 1,
//   CurveFloat_1 linear 0 -> 1) at rate 1 / VisualsScaleDownDuration: PortalMesh's relative scale = VLerp(before,
//   before x 0.5, alpha) @19063. VFXAttach and what is attached to it shrink with it.
// Timelines tick with the actor from the frame after PlayFromStart, position + delta (as the dropped item's): the
// value at tick t is the curve at (t - start) x dt, unset at the start's own tick.
// The effects (SpawnVFX, the transitions, PreOutro, Outro) are mvs-render's (fx/chars/rick.rs), on this layout.
// Deviation: no outline on the portal mesh.

const PORTAL_CLASSES: [&str; 3] = ["AutoTeleportPortal_C", "VisualOnlyPortal_C", "ExitPortalParent_C"];
const PORTAL_MESH: &str = "SM_C020Portal";
const STEP: f64 = 1.0 / 60.0;
/// FighterThroughPortal's CurveFloat_0 (time, value, arrive, leave, mode 0 linear), timeline length 1.
const THROUGH: [CurveKey; 2] = [[0.0, 10.0, 0.0, 0.0, 0.0], [0.3, 0.0, 0.0, 0.0, 0.0]];
const THROUGH_LEN: f64 = 1.0;
const VISUALS_LEN: f64 = 1.0;

/// out = at + rot . (s * rel).
fn under(at: V3, rot: &Rows, s: V3, rel: V3) -> V3 {
    let (a, b, c) = (s[0] * rel[0], s[1] * rel[1], s[2] * rel[2]);
    std::array::from_fn(|k| at[k] + a * rot[k] + b * rot[3 + k] + c * rot[6 + k])
}

/// The spawn's actor rotation (0, -90, 0).
fn actor_rot() -> Rows {
    rotation_rows(0.0, -std::f64::consts::FRAC_PI_2, 0.0)
}

/// A portal class's component places (the class data of the owner's character; the same for every portal class).
#[derive(Clone, Copy, Debug)]
pub struct PortalLayout {
    /// PortalMesh's location under the actor.
    pub mesh_rel: V3,
    /// Its world rotation rows.
    pub mesh_rot: Rows,
    /// Its relative scale.
    pub mesh_scale: V3,
    /// VFXAttach's relative scale under PortalMesh.
    pub attach_rel: V3,
}

/// `layout`: from the class's components (None: no PortalMesh or VFXAttach).
pub fn portal_layout(cs: &Props) -> Option<PortalLayout> {
    let m = cs.get("PortalMesh")?.as_obj()?;
    let v = cs.get("VFXAttach")?.as_obj()?;
    let ar = actor_rot();
    Some(PortalLayout {
        mesh_rel: under([0.0; 3], &ar, [1.0; 3], xyz(m.get("RelativeLocation"), 0.0)),
        mesh_rot: compose_rows(&rel_rows(m.get("RelativeRotation")), &ar),
        mesh_scale: xyz(m.get("RelativeScale3D"), 1.0),
        attach_rel: xyz(v.get("RelativeScale3D"), 1.0),
    })
}

/// A portal's component transforms at a tick (`place`).
#[derive(Clone, Copy, Debug)]
pub struct PortalAt {
    /// The scale-down factor (1 .. 0.5).
    pub k: f64,
    pub mesh_loc: V3,
    pub mesh_scale: V3,
    /// VFXAttach's world scale.
    pub attach_scale: V3,
}

/// `place`: portal `p` (class defaults `cp`, root scale `size_scale`) at World tick `now`.
pub fn portal_at(lay: &PortalLayout, p: &PortalActor, cp: &Props, size_scale: Option<f64>, now: i32) -> PortalAt {
    let now = f64::from(now);
    let k = if p.only_visuals_tick >= 0.0 && now > f64::from(p.only_visuals_tick) {
        let dur = cp.get("VisualsScaleDownDuration").and_then(Value::as_f64).unwrap_or(1.0);
        let alpha =
            ((now - f64::from(p.only_visuals_tick)) * STEP / if dur > 0.0 { dur } else { 1.0 }).min(VISUALS_LEN);
        1.0 - 0.5 * alpha
    } else {
        1.0
    };
    // the root's scale (the sim's size_scale) over every component
    let g = size_scale.unwrap_or(1.0);
    let r = lay.mesh_rel;
    let mesh_scale = std::array::from_fn(|j| lay.mesh_scale[j] * k * g);
    PortalAt {
        k,
        mesh_loc: [r[0] * g, -f64::from(p.x) + r[1] * g, f64::from(p.y) + r[2] * g],
        mesh_scale,
        attach_scale: std::array::from_fn(|j| mesh_scale[j] * lay.attach_rel[j]),
    }
}

/// `richCurve(THROUGH)`'s WPO at World tick `now` (None: no transition ran yet, or its own tick).
pub fn portal_wpo(p: &PortalActor, now: i32) -> Option<f64> {
    let now = f64::from(now);
    (p.through_tick >= 0.0 && now > f64::from(p.through_tick))
        .then(|| rich_curve(&THROUGH, ((now - f64::from(p.through_tick)) * STEP).min(THROUGH_LEN)) * 0.5)
}

/// The portal's mesh: its view (None: not loaded), the layout, and what it has set.
#[derive(Debug)]
struct PortalMesh {
    id: ActorId,
    from: FighterId,
    spawn_tick: f64,
    lay: PortalLayout,
    view: Option<StaticView>,
    /// Original set.
    coloured: bool,
    /// The WPO last set (None: never).
    wpo: Option<f64>,
}

fn portal_of(w: &World, id: ActorId) -> Option<(&WorldActor, &PortalActor)> {
    let x = w.actors.actors.get(id)?;
    match &x.state {
        WorldActorState::PortalActor(p) => Some((x, p)),
        _ => None,
    }
}

fn portal_mesh(host: &mut ActorHost<'_>, w: &World, a: ActorRef, _cs: &ActorComponents) -> Option<Box<dyn ActorMesh>> {
    let ActorRef::Actor(id) = a else { return None };
    let (x, p) = portal_of(w, id)?;
    let Some(lay) = class_def(w, a).and_then(|d| portal_layout(&d.components)) else {
        warn_once(format!("warning: Rick: {} has no PortalMesh / VFXAttach in its data", w.names.str(x.cls)));
        return None;
    };
    let from = actor_def_owner(w, x.owner, x.cls);
    let view = StaticView::new(host, w, from, PORTAL_MESH, "");
    if view.is_none() {
        warn_once("warning: Rick: the portal mesh is not drawn (its mesh or materials are not loaded)".to_owned());
    }
    Some(Box::new(PortalMesh { id, from, spawn_tick: f64::from(p.spawn_tick), lay, view, coloured: false, wpo: None }))
}

impl PortalMesh {
    /// The portal views' tick, once a World step: the remake after a rollback, the colour
    /// and the WPO timeline.
    fn sync(&mut self, host: &mut ActorHost<'_>, w: &World) {
        let Some((x, p)) = portal_of(w, self.id) else { return };
        if f64::from(p.spawn_tick) != self.spawn_tick {
            // (another portal under the same id after a rollback)
            self.spawn_tick = f64::from(p.spawn_tick);
            self.view = StaticView::new(host, w, self.from, PORTAL_MESH, "");
            self.coloured = false;
            self.wpo = None;
        }
        let Some(view) = &mut self.view else { return };
        // PortalMesh shown, then its colour (@451, @891)
        if p.mesh_at >= 0.0 && !self.coloured {
            self.coloured = true;
            if let Some(c) = host.broker_colour(w, x.owner) {
                view.set_vector("Original", [c[0], c[1], c[2], 1.0]);
            }
        }
        // FighterThroughPortal
        if let Some(v) = portal_wpo(p, w.flow.tick)
            && self.wpo != Some(v)
        {
            self.wpo = Some(v);
            view.set_scalar("WPO", v);
        }
    }
}

impl ActorMesh for PortalMesh {
    fn place(
        &mut self,
        host: &mut ActorHost<'_>,
        w: &World,
        _at: &ActorPlace,
        _alpha: f64,
        shown: bool,
        out: &mut Vec<MeshPose>,
    ) {
        self.sync(host, w);
        let Some((x, p)) = portal_of(w, self.id) else { return };
        let Some(view) = &mut self.view else { return };
        let cp = class_def(w, ActorRef::Actor(self.id)).map(|d| &d.p);
        let empty = Props::default();
        let at = portal_at(&self.lay, p, cp.unwrap_or(&empty), x.size_scale.map(f64::from), w.flow.tick);
        let on = shown && p.mesh_at >= 0.0;
        view.visible = on;
        if !on {
            return;
        }
        view.set(&Xform { loc: at.mesh_loc, rot: self.lay.mesh_rot, scale: at.mesh_scale });
        view.draw(out);
    }

    fn tick(&mut self, host: &mut ActorHost<'_>, w: &World) {
        self.sync(host, w);
    }
}

// ---- the portal aimer's gun --------------------------------------------------------------------------
//
// C020PortalAimer_C (offsets of its ubergraph). The sim: x, y, gun_left, locally_controlled, dead.
// - The gun (BeginPlay @6596-7148): AddStaticMeshComponent, SetStaticMesh(SM_C020PortalGun), tag AimPortalGun, the
//   relative transform SelectTransform(FacingLeftTransform, FacingRightTransform, the Instigator's BPI_IsFacingLeft)
//   @6541, K2_AttachToComponent(the Instigator's Mesh, L_Hand) @6874, MeshManager.AddMesh @7148. No SetMaterial: the
//   mesh's own slots (0_MeeseeksProp_MIC and the base C020PortalGunGlass_MAT; the draws' SM_C020PortalGun). The
//   facing flip (Tick @9242 -> @9550-9657) sets the new facing's transform and attaches again: the sim's gun_left. In
//   the manager it is hidden with Rick's meshes (AddInvisibility), and not drawn while he is dead.
// - The end (EndPlay @12226 / ReleaseToPool @12442 -> @11656): RemoveMesh @11568 and K2_DestroyComponent @11622 only
//   when LocallyControlled (@11255); either way UWorld::DestroyActor unregisters every component the aimer owns, so
//   the gun stops drawing with the aimer, wherever it is attached: it goes with the actor here.
// The SkillShotIndicator (UI_Skillshot_Aimpunch_Target, screen space) is the HUD's.
// Unverified: the socket is the last rendered pose's (as the pawn props). Deviation: no outline on the gun.
// Deviation: the gun is placed each frame on the pose just presented, not once a tick.

const AIMER: &str = "C020PortalAimer_C";
const GUN_MESH: &str = "SM_C020PortalGun";
const GUN_SOCKET: &str = "L_Hand";

#[derive(Debug)]
struct AimerGun {
    id: ActorId,
    view: StaticView,
    /// The relative transform facing right and left.
    rel: [Xform; 2],
}

/// FTransform's rotation (a quaternion X, Y, Z, W) as rows of the rotated X, Y, Z axes
/// (FQuatRotationTranslationMatrix).
fn quat_rows(q: Option<&Value>) -> Rows {
    let o = q.and_then(Value::as_obj);
    let g = |k: &str, d: f64| o.and_then(|o| o.get(k)).and_then(Value::as_f64).unwrap_or(d);
    let (x, y, z, w) = (g("X", 0.0), g("Y", 0.0), g("Z", 0.0), g("W", 1.0));
    [
        1.0 - 2.0 * (y * y + z * z),
        2.0 * (x * y + w * z),
        2.0 * (x * z - w * y),
        2.0 * (x * y - w * z),
        1.0 - 2.0 * (x * x + z * z),
        2.0 * (y * z + w * x),
        2.0 * (x * z + w * y),
        2.0 * (y * z - w * x),
        1.0 - 2.0 * (x * x + y * y),
    ]
}

fn aimer_gun(host: &mut ActorHost<'_>, w: &World, a: ActorRef, _cs: &ActorComponents) -> Option<Box<dyn ActorMesh>> {
    let ActorRef::Actor(id) = a else { return None };
    let x = w.actors.actors.get(id)?;
    let Some(view) = StaticView::new(host, w, x.owner, GUN_MESH, "") else {
        warn_once("warning: Rick: the aimer gun is not drawn (its mesh or materials are not loaded)".to_owned());
        return None;
    };
    let p = class_def(w, a).map(|d| &d.p);
    let rel = ["FacingRightTransform", "FacingLeftTransform"].map(|k| {
        let t = p.and_then(|p| p.get(k)).and_then(Value::as_obj);
        if t.is_none() {
            warn_once(format!("warning: Rick: the aimer data has no {k}"));
        }
        Xform {
            loc: xyz(t.and_then(|t| t.get("Translation")), 0.0),
            rot: quat_rows(t.and_then(|t| t.get("Rotation"))),
            scale: xyz(t.and_then(|t| t.get("Scale3D")), 1.0),
        }
    });
    Some(Box::new(AimerGun { id, view, rel }))
}

impl ActorMesh for AimerGun {
    fn place(
        &mut self,
        host: &mut ActorHost<'_>,
        w: &World,
        _at: &ActorPlace,
        _alpha: f64,
        shown: bool,
        out: &mut Vec<MeshPose>,
    ) {
        let Some(x) = w.actors.actors.get(self.id) else { return };
        let WorldActorState::PortalAimer(s) = &x.state else { return };
        let fi = w.fighter(x.owner);
        let on = shown && fi.life.dead == 0.0 && !fi.vis.invisible;
        self.view.visible = on;
        if !on {
            return;
        }
        let Some(at) = host.socket(w, x.owner, GUN_SOCKET, &self.rel[usize::from(s.gun_left)]) else { return };
        self.view.set(&at);
        self.view.draw(out);
    }
}

// ---- the missile and the fart bomb -------------------------------------------------------------
//
// c020_WeaponModeMissile (not Iron Giant's WeaponModeMissile): StaticMesh C017_Rocket under the Capsule at
// (5.87, 0, -3.16), pitch -90, scale (4, 4, 6), CastShadow false; no rotation write in its graph (no Rotation
// timeline): the mesh keeps pitch -90 under the actor's rotation (bRotationFollowsVelocity: actor_rot). SetTeamColor
// @925 -> @739 SetMaterial(0, local ? C017_Rocket_Blue_Mat_Mat : index 0 ? ..._Red_.. : 1 ? ..._Blue_.. : None):
// ActorHost::variant (unverified there: an index past 1 is taken as blue, the build keeps the slot's own), dressed
// each frame by the owner (a reflect's SetTeamColor). Its trail (PS_C020_RocketTrail) is the effects' shared path.
//
// FartBomb_SpawnedActor (latent 2980 -> 30, 3035 -> 56): Mesh = C020_fartbomb_SkelMesh under the Capsule at
// (0, 0, -39.03), CastShadow false, AnimationSingleNode C020_FartBomb_Explode, not looping; material C020_prop2_MIC (no
// graph sets one).
// - BeginPlay @1619: the Instigator facing left -> SetActorScale3D(scale x (1, -1, 1)) @1852: the whole actor mirrored
//   (the projectile's facing holds the side).
// - Detonate -> Delay(0.4) -> @56 Mesh.K2_DestroyComponent: FartBomb.mesh_gone.
// - @3295 PlayAnimation(MouseTrap_SnapFast) (MulticastDamageTaken, the sim's snap_fast): the clip's skeleton is
//   MouseTrap_Skeleton, the mesh's C020_fartbomb_SkelMesh_Skeleton, and SetAnimationAsset 142877b40 nulls
//   CurrentAsset on a skeleton mismatch: nothing plays and the Explode clip stops.
//   Unverified: a single node instance with no asset is drawn in the reference pose (its Evaluate with no asset is
//   unread). @3149 PlayAnimation(null) (behind GroundDestroyed): a path the sim does not reach.
// Unverified: FSingleAnimationPlayData's bSavedPlaying, SavedPlayRate and SavedPosition are not serialized: the clip
// is taken to play from the spawn at rate 1 and hold its last frame. Its time is the actor's age in ticks of 1 / 60 s
// from the spawn tick.
// The cloud (EffectMulticast @3465, PS_C020_FartCloud on the Mesh) is the effects', on bomb_frame.
// Deviation: no outline on any of Rick's actors.

const MISSILE: &str = "c020_WeaponModeMissile_C";
const FARTBOMB: &str = "FartBomb_SpawnedActor_C";
const BOMB_MESH: &str = "C020_fartbomb_SkelMesh";

#[derive(Debug)]
struct MissileMesh {
    id: ProjectileId,
    view: StaticView,
    c: Comp,
}

fn missile_mesh(host: &mut ActorHost<'_>, w: &World, a: ActorRef, cs: &ActorComponents) -> Option<Box<dyn ActorMesh>> {
    let ActorRef::Projectile(id) = a else { return None };
    let pr = w.actors.projectiles.get(id)?;
    let owner = pr.owner?;
    let ComponentKind::StaticMesh { mesh, .. } = &cs.get("StaticMesh")?.kind else { return None };
    let c = comp_of(&class_def(w, a)?.components, "StaticMesh")?;
    let from = actor_def_owner(w, owner, pr.cls);
    let view = StaticView::new(host, w, from, mesh, host.variant(w, owner))?;
    Some(Box::new(MissileMesh { id, view, c }))
}

impl ActorMesh for MissileMesh {
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
        if let Some(o) = w.actors.projectiles.get(self.id).and_then(|p| p.owner) {
            self.view.dress(host, host.variant(w, o));
        }
        let (x, y) = at.at(alpha);
        self.view.set(&place_at(&self.c, &self.c.rot, self.c.rs, &at.rot, at.scale, [0.0, -x, y]));
        self.view.draw(out);
    }
}

/// `bombFrame`: the bomb's Mesh in the world: the actor's rotation `rot`, its root scale `k` with the mirror
/// (`facing` < 0), the Mesh's relative location `rel`, at sim (`x`, `y`).
pub fn bomb_frame(rot: &Rows, k: f64, facing: f64, size_scale: Option<f64>, x: f64, y: f64, rel: V3) -> Xform {
    // SetSizeScale's SetActorScale3D (k, k, k) @566 comes after the mirror @1852 and writes the scale whole: a grown
    // bomb (the sim's size_scale set) is drawn unmirrored.
    let s = [k, if facing < 0.0 && size_scale.is_none() { -k } else { k }, k];
    Xform { loc: under([0.0, -x, y], rot, s, rel), rot: *rot, scale: s }
}

#[derive(Debug)]
struct BombMesh {
    id: ProjectileId,
    from: FighterId,
    view: SkinnedActor,
    rel: V3,
    clip: Option<usize>,
    /// The age the pose was last sampled at (its time changes once a tick); Some(None): the rest pose shown.
    sampled: Option<Option<i32>>,
}

fn fart_bomb_mesh(
    host: &mut ActorHost<'_>,
    w: &World,
    a: ActorRef,
    cs: &ActorComponents,
) -> Option<Box<dyn ActorMesh>> {
    let ActorRef::Projectile(id) = a else { return None };
    let pr = w.actors.projectiles.get(id)?;
    let c = cs.get("Mesh")?;
    let ComponentKind::SkeletalMesh { mesh, anim, .. } = &c.kind else { return None };
    if mesh.as_str() != BOMB_MESH {
        return None;
    }
    // (the class's character: the owner, else a peer after a reflect)
    let from = actor_def_owner(w, pr.owner?, pr.cls);
    let (path, keys) = host.draws_of(w, from)?.actor(BOMB_MESH, None)?;
    let info = host.mesh(&path)?;
    let name = anim.as_deref().unwrap_or("None");
    let clip = host.clips_of(w, from).and_then(|c| c.get(name));
    if clip.is_none() {
        warn_once(format!("warning: Rick: the fart bomb clip is not loaded: {name}"));
    }
    Some(Box::new(BombMesh { id, from, view: SkinnedActor::new(info, &keys), rel: c.loc, clip, sampled: None }))
}

impl ActorMesh for BombMesh {
    fn place(
        &mut self,
        host: &mut ActorHost<'_>,
        w: &World,
        at: &ActorPlace,
        alpha: f64,
        shown: bool,
        out: &mut Vec<MeshPose>,
    ) {
        let Some(pr) = w.actors.projectiles.get(self.id) else { return };
        let ProjectileExt::FartBomb(b) = &pr.ext else { return };
        let on = shown && !b.mesh_gone;
        self.view.visible = on;
        if !on {
            return;
        }
        if b.snap_fast {
            if self.sampled != Some(None) {
                let rest = self.view.rest.clone();
                self.view.show(&rest);
            }
            self.sampled = Some(None);
        } else if let Some(c) = self.clip
            && self.sampled != Some(Some(pr.age))
        {
            self.sampled = Some(Some(pr.age));
            if let Some(clips) = host.clips_of(w, self.from) {
                self.view.play(clips, c, f64::from((pr.age - 1).max(0)) * STEP, false);
            }
        }
        let (x, y) = at.at(alpha);
        self.view.set(&bomb_frame(
            &at.rot,
            f64::from(pr.scale),
            f64::from(pr.facing),
            pr.size_scale.map(f64::from),
            x,
            y,
            self.rel,
        ));
        self.view.draw(out);
    }
}

// ---- the AirFlap -------------------------------------------------------------------------------
//
// MeeSeeks_AirFlap (World.actors): SkeletalMesh = Meeseeks_SkelMesh under the Capsule, AnimationSingleNode
// C020_Meeseeks_FlapAttack, not looping (its AnimClass Meeseeks_AnimBP_C not run, unverified). The actor is attached at
// the owner's Root socket with an identity relative transform (Comp_Spawner @10581 -> @10515); BeginPlay @259:
// SkeletalMesh.SetRelativeScale3D(the Instigator facing left ? (1, -1, 1) : 1).
// Unverified: the actor's rotation is taken as yaw -90 facing right and +90 facing left (the Root socket of the drawn
// pose is not read): with the mesh's mirror that is the right-facing picture reflected across the play plane's
// horizontal, as the sim's hit boxes have it (pushActorShapes, mirror).
// SetTeamColor @1181 -> @974: SetMaterial(0, FTrunc(colour) 0 -> Meeseeks_Red_MIC, 1 -> Meeseeks_Blue_MIC, else the
// slot's own). PS_Meeseeks_Flapping (the effects') under the SkeletalMesh at (0, 0, 375), no Team.
// The polymorph grenade's MineMesh (SM_C020Satchel) is bHiddenInGame and nothing shows it: not drawn (its aura and
// PS_NinjaBomb are the effects').

const AIRFLAP: &str = "MeeSeeks_AirFlap_C";
const MEESEEKS_MESH: &str = "Meeseeks_SkelMesh";

/// `meeseeksMesh`: mesh `mesh` of fighter `from`'s draws with the materials of colour index `ci` (0 red, 1 blue,
/// else the slot's own).
fn meeseeks_mesh(host: &ActorHost<'_>, w: &World, from: FighterId, mesh: &str, ci: Option<u8>) -> Option<SkinnedActor> {
    let variant = match ci {
        Some(0) => Some("red"),
        Some(1) => Some("blue"),
        _ => None,
    };
    let (path, keys) = host.draws_of(w, from)?.actor(mesh, variant)?;
    Some(SkinnedActor::new(host.mesh(&path)?, &keys))
}

#[derive(Debug)]
struct FlapMesh {
    id: ActorId,
    owner: FighterId,
    view: SkinnedActor,
    clip: Option<usize>,
    /// The place of the tick before and of the latest, and the age last played.
    prev: (f64, f64),
    cur: (f64, f64),
    age: Option<i32>,
}

fn air_flap_mesh(host: &mut ActorHost<'_>, w: &World, a: ActorRef, cs: &ActorComponents) -> Option<Box<dyn ActorMesh>> {
    let ActorRef::Actor(id) = a else { return None };
    let x = w.actors.actors.get(id)?;
    let from = actor_def_owner(w, x.owner, x.cls);
    let Some(view) = meeseeks_mesh(host, w, from, MEESEEKS_MESH, Some(host.colour(w, x.owner).color_index)) else {
        warn_once("warning: Rick: the AirFlap is not drawn (its mesh or materials are not loaded)".to_owned());
        return None;
    };
    let name = cs
        .get("SkeletalMesh")
        .and_then(|c| match &c.kind {
            ComponentKind::SkeletalMesh { anim, .. } => anim.clone(),
            _ => None,
        })
        .unwrap_or_else(|| "None".to_owned());
    let clip = host.clips_of(w, x.owner).and_then(|c| c.get(&name));
    if clip.is_none() {
        warn_once(format!("warning: Rick: the AirFlap clip is not loaded: {name}"));
    }
    Some(Box::new(FlapMesh { id, owner: x.owner, view, clip, prev: (0.0, 0.0), cur: (0.0, 0.0), age: None }))
}

impl FlapMesh {
    /// rickActors.actors() (once a World step): the place of the tick before and of the latest, and the clip at the
    /// age.
    fn sync(&mut self, host: &mut ActorHost<'_>, w: &World) {
        let Some(x) = w.actors.actors.get(self.id) else { return };
        let WorldActorState::AirFlap(f) = &x.state else { return };
        if self.age == Some(f.age) {
            return;
        }
        self.prev = if self.age.is_some() { self.cur } else { (f64::from(f.x), f64::from(f.y)) };
        self.cur = (f64::from(f.x), f64::from(f.y));
        self.age = Some(f.age);
        if let Some(c) = self.clip
            && let Some(clips) = host.clips_of(w, self.owner)
        {
            self.view.play(clips, c, f64::from((f.age - 1).max(0)) * STEP, false);
        }
    }
}

impl ActorMesh for FlapMesh {
    fn tick(&mut self, host: &mut ActorHost<'_>, w: &World) {
        self.sync(host, w);
    }

    fn place(
        &mut self,
        host: &mut ActorHost<'_>,
        w: &World,
        _at: &ActorPlace,
        alpha: f64,
        shown: bool,
        out: &mut Vec<MeshPose>,
    ) {
        self.sync(host, w);
        let Some(x) = w.actors.actors.get(self.id) else { return };
        let WorldActorState::AirFlap(f) = &x.state else { return };
        self.view.visible = shown;
        if !shown {
            return;
        }
        let left = f.facing < 0.0;
        let (px, py) = self.prev;
        let (cx, cy) = self.cur;
        let loc = [0.0, -(px + (cx - px) * alpha), py + (cy - py) * alpha];
        // the root's scale (the sim's size_scale)
        let k = x.size_scale.unwrap_or(1.0);
        let h = std::f64::consts::FRAC_PI_2;
        let rot = rotation_rows(0.0, if left { h } else { -h }, 0.0);
        self.view.set(&Xform {
            loc,
            rot,
            scale: [f64::from(k), if left { f64::from(-k) } else { f64::from(k) }, f64::from(k)],
        });
        self.view.draw(out);
    }
}

// ---- the Meeseeks summon --------------------------------------------------------------------------
//
// Meeseeks_SpawnedActor_C and Meeseeks_AnimBP. The sim: mvs-sim chars/rick/meeseeks.rs (a projectile).
// Components: Meeseeks_SkelMesh under the Capsule at (0, 0, -38), scale 1.155, AnimClass Meeseeks_AnimBP_C. No
// AnimationMode is serialized: the class default, AnimationBlueprint, so the AnimBP runs and AnimToPlay
// (Meeseeks_Attack_GolfSwing) is never used. golfClub (MrMeeseeks_GolfClub_SkelMesh, the same skeleton) under it,
// bVisible false, SetMasterPoseComponent(Meeseeks_SkelMesh) (UserConstructionScript @0): it shows the Meeseeks' pose;
// GolfClubVis @9234 shows it (the sim's golfing) and nothing hides it again (the AnimBP's ToggleSubmeshVisibility @930
// does nothing: VisibilityManager is never written). The StaticMesh and ParticleSystem of ParentProjectilePhysics have
// no asset.
// - The actor's rotation: yaw 90 while FacingLeft, -90 else (SetFacing @2907, Tick @6629 / @8584), and the AnimBP's
//   mirror (Meeseeks_MirrorTable) by BPI_SetIsFacingLeft (@389, @2716, @7590; its own update's cast to Meeseeks_Base
//   fails @1284): the sim's facing_left for both.
// - SetTeamColor @4644 -> @4900: SetMaterial(0, FTrunc(BPI_GetColorValue) 0 Meeseeks_Red_MIC, 1 Meeseeks_Blue_MIC,
//   else None: the slot's own Meeseeks_MAT) (the draws' Meeseeks_SkelMesh, variants red / blue).
// - HitShake.PlayFromStart @9201 (the sim's shake_at): each update @9906-10095 sets Meeseeks_SkelMesh's relative
//   location to StartingRelativeLocation (0, 0, -38, @42) + (curve x 50, 0, 0): along the actor's forward. CurveFloat_0
//   (the class's, length 0.2) below. The timeline ticks from the frame after PlayFromStart (as the dropped item's); at
//   its own tick the offset is the one before.
// Meeseeks_AnimBP (crate::actor_anim runs it, once a sim tick of the actor's age):
//   Root <- Mirror <- BlendListByBool_1 (IsFalling: velocity Z < -200 @1180; AirSlot / DefaultSlot, .1 s linear) <- the
//   two Slot nodes over the cached pose AfterMovement (neither bAlwaysUpdateSourcePose: the cached pose is updated only
//   while one of them passes it on above weight 1e-5) <- BlendListByBool (HasTakenHit; true BlendListByInt(HitNumber:
//   Meeseeks_HR_01 / _HR_02, looped, 0 s), false the machine Movement; .01 s linear).
//   Movement (entry Spawn, MaxTransitionsPerFrame 3, bSkipFirstUpdateTransition, not reinitialised on becoming
//   relevant): Spawn = Meeseeks_Spawn, Idle = the machine IdleState, Fall = Meeseeks_Stand
//   (looped), Land = Meeseeks_Spawn. IdleState (entry Idle, reinitialised on becoming relevant): Idle =
//   Meeseeks_Stand (looped), Wave = Meeseeks_Wave.
//   Variables: OnTakeDamage @1437 (bound to the owner's OnTakeAnyDamage @758: the sim's any_damage_at / any_damage_n)
//   HasTakenHit = true, HitNumber toggled @1529, RetriggerableDelay .2 @1556 -> @27 the alt idle timer set again and
//   HasTakenHit false @134. The alt idle timer (Initialize @822: AltIdleTriger every 4 s, looping) -> @1371
//   PlayAltIdle true, Delay .2 -> false.
//   Between the spawn and ActivateObject no montage plays, so the machine shows (Spawn, then Idle with the wave every
//   4 s). The montages (Runnig, GolfSwing, ChargeUppercut: both slots, blend in 0) are the sim's
//   (mvs_sim::actors::actor_montage); under them at full weight the cached pose is not updated.
// Unverified: the timers and delays are counted in ticks of the actor's age (latent_frames), from its first tick.
// Deviation: the view's AnimBP runs once a tick of the actor's age, at each World step and at a frame that shows a
//   new age (not at a presented frame only, while shown): a rollback's jump of several ticks runs the ticks
//   between (30 at most) with the latest state's inputs, and the instance runs on while a carry hides the actor.
// Deviation: the mirror is drawn as the reflection of the right-facing view across UE Y (as the fighters'). No outline.
// HeadVFXAttach (Head socket) is not drawn here: its users (BPI_GetVFXAttachComponent) were not read.

const MEESEEKS_SUMMON: &str = "Meeseeks_SpawnedActor_C";
const CLUB: &str = "golfClub";
const ZERO_W: f64 = 1e-5;
/// @1180.
const FALL_VZ: f64 = -200.0;
/// BlendListByBool_1, linear.
const AIR_BLEND: f64 = 0.1;
/// @9906-10095: the curve x 50.
const SHAKE_X: f64 = 50.0;
/// HitShake's CurveFloat_0 (the class's): time, value, arrive, leave, mode 0 linear / 1 cubic.
const SHAKE: [CurveKey; 6] = [
    [0.000_019_818_544, 0.570_200_9, 0.0, 0.0, 1.0],
    [0.028_785_646, -0.539_871_93, -1.757_973_9, -1.757_973_9, 1.0],
    [0.054_237_932, 0.474_886_9, 2.773_432_5, 2.773_432_5, 1.0],
    [0.100_453_94, -0.341_104_75, -1.296_531_1, -1.296_531_1, 1.0],
    [0.149_349_15, 0.181_966_78, 0.0, 0.0, 0.0],
    [0.2, 0.0, 0.0, 0.0, 1.0],
];
const SHAKE_LEN: f64 = 0.2;

/// Meeseeks_AnimBP's inputs.
#[derive(Debug, Default)]
struct Inputs {
    falling: bool,
    has_taken_hit: bool,
    hit_number: i32,
    play_alt_idle: bool,
}

fn seq(name: &'static str, looped: bool) -> Option<PoseDef<Inputs>> {
    Some(PoseDef::Seq(SeqDef::new(name, looped)))
}

fn state(name: &'static str, pose: Option<PoseDef<Inputs>>) -> StateDef<Inputs> {
    StateDef { name, pose, entry_rule: None, always_reset: false }
}

fn trans(
    from: &'static str,
    to: &'static str,
    xfade: f64,
    rule: fn(&Inputs, &dyn MachineQuery) -> bool,
) -> TransitionDef<Inputs> {
    TransitionDef { from, to, xfade, option: BlendOption::HermiteCubic, rule: Some(rule), auto: false }
}

fn idle_state() -> MachineDef<Inputs> {
    MachineDef {
        entry: "Idle",
        reinit_on_becoming_relevant: true,
        max_transitions_per_frame: 3,
        skip_first_update_transition: true,
        states: vec![state("Idle", seq("Meeseeks_Stand", true)), state("Wave", seq("Meeseeks_Wave", false))],
        transitions: vec![
            trans("Idle", "Wave", 0.0, |i, _| i.play_alt_idle),
            // @676
            trans("Wave", "Idle", 0.0, |i, q| q.remaining("Wave") == 0.0 || i.has_taken_hit),
        ],
    }
}

fn movement() -> MachineDef<Inputs> {
    MachineDef {
        entry: "Spawn",
        reinit_on_becoming_relevant: false,
        max_transitions_per_frame: 3,
        skip_first_update_transition: true,
        states: vec![
            state("Spawn", seq("Meeseeks_Spawn", false)),
            state("Idle", Some(PoseDef::Machine(idle_state()))),
            state("Fall", seq("Meeseeks_Stand", true)),
            state("Land", seq("Meeseeks_Spawn", false)),
        ],
        transitions: vec![
            trans("Spawn", "Fall", 0.1, |i, q| i.falling && q.remaining("Spawn") <= 0.1), // @533
            trans("Spawn", "Idle", 0.1, |i, q| !i.falling && q.remaining("Spawn") <= 0.1), // @391
            trans("Idle", "Fall", 0.2, |i, _| i.falling),
            trans("Fall", "Land", 0.0, |i, _| !i.falling),
            trans("Land", "Idle", 0.0, |_, q| q.remaining("Land") <= 0.05), // @221
        ],
    }
}

/// The AnimGraph under the slots (the cached pose AfterMovement) as one machine of one state: the HasTakenHit blend
/// list over the HR blend list and Movement.
fn graph() -> MachineDef<Inputs> {
    let hr = PoseDef::BoolList(BoolListDef {
        test: |i: &Inputs| i.hit_number == 0,
        t: Box::new(PoseDef::Seq(SeqDef::new("Meeseeks_HR_01", true))),
        f: Box::new(PoseDef::Seq(SeqDef::new("Meeseeks_HR_02", true))),
        blend_time: [0.0, 0.0],
        option: BlendOption::Linear,
    });
    let after = PoseDef::BoolList(BoolListDef {
        test: |i: &Inputs| i.has_taken_hit,
        t: Box::new(hr),
        f: Box::new(PoseDef::Machine(movement())),
        blend_time: [0.01, 0.01],
        option: BlendOption::Linear,
    });
    MachineDef {
        entry: "AfterMovement",
        reinit_on_becoming_relevant: false,
        max_transitions_per_frame: 1,
        skip_first_update_transition: false,
        states: vec![state("AfterMovement", Some(after))],
        transitions: Vec::new(),
    }
}

/// A slot's weight under the montage instance: its shown weight where it has a pose in the slot.
fn slot_weight(inst: Option<(&MontageState, &Montage)>, slot: &str) -> f64 {
    let Some((st, m)) = inst else { return 0.0 };
    let w = st.shown_weight(m, &|_| {});
    if w > ZERO_W && st.pose_in(m, slot).is_some() { w } else { 0.0 }
}

/// The summon's view.
#[derive(Debug)]
struct SummonView {
    sk: SkinnedActor,
    club: Option<SkinnedActor>,
    machine: ActorMachine<Inputs>,
    /// DefaultSlot, AirSlot.
    slots: [ActorSlot; 2],
    base: Pose,
    ground: Pose,
    air: Pose,
    out: Pose,
    /// The club's bones in the Meeseeks' order where they differ (-1: a bone the Meeseeks lacks), and its pose.
    club_map: Option<Vec<i32>>,
    club_pose: Option<Pose>,
    inp: Inputs,
    /// BlendListByBool_1's AirSlot weight, None before the first update.
    air_w: Option<f64>,
    last_age: Option<i32>,
    /// The timeline's last value.
    shake: f64,
    rel: V3,
    mesh_scale: f64,
    hit_frames: f64,
    alt_frames: f64,
    alt_on: f64,
}

impl SummonView {
    fn new(sk: SkinnedActor, club: Option<SkinnedActor>, clips: &Clips, rel: V3, mesh_scale: f64) -> SummonView {
        let n = sk.names.len();
        let mut club_map = None;
        if let Some(c) = &club
            && c.names.iter().enumerate().any(|(k, b)| sk.names.get(k) != Some(b))
        {
            let map: Vec<i32> =
                c.names.iter().map(|b| sk.names.iter().position(|x| x == b).map_or(-1, |k| k as i32)).collect();
            if map.iter().any(|&k| k < 0) {
                warn_once(
                    "warning: Meeseeks summon: the golf club has bones the Meeseeks lacks (shown at rest)".into(),
                );
            }
            club_map = Some(map);
        }
        let club_pose = club_map.as_ref().zip(club.as_ref()).map(|(_, c)| Pose::new(c.names.len()));
        let mut club = club;
        if let Some(c) = &mut club {
            c.visible = false;
        }
        let latent = mvs_sim::fighter::common::latent_frames;
        SummonView {
            machine: ActorMachine::new(graph(), n, clips),
            slots: [ActorSlot::new("DefaultSlot", n), ActorSlot::new("AirSlot", n)],
            base: Pose::new(n),
            ground: Pose::new(n),
            air: Pose::new(n),
            out: Pose::new(n),
            club_map,
            club_pose,
            inp: Inputs::default(),
            air_w: None,
            last_age: None,
            shake: 0.0,
            rel,
            mesh_scale,
            // RetriggerableDelay .2 @1556, AltIdleTrigerTime 4 @822, Delay .2 @1371
            hit_frames: f64::from(latent(0.2)),
            alt_frames: f64::from(latent(4.0)),
            alt_on: f64::from(latent(0.2)),
            sk,
            club,
        }
    }

    /// One tick of the anim instance at the actor's age.
    fn update(
        &mut self,
        s: &MeeseeksSummon,
        vy: f64,
        inst: Option<(&MontageState, &Montage)>,
        age: i32,
        clips: &Clips,
    ) {
        let inp = &mut self.inp;
        // GetVelocity: the simulating body's, else the movement component's
        let vz = if s.running { s.bvy } else { mvs_sim::math::narrow(vy) };
        inp.falling = vz < mvs_sim::math::narrow(FALL_VZ);
        let hit_at = s.any_damage_at;
        let age_f = f64::from(age);
        inp.has_taken_hit = hit_at >= 0 && age_f - f64::from(hit_at) < self.hit_frames;
        inp.hit_number = s.any_damage_n & 1;
        // the alt idle timer from the instance's initialise, set again at each hit's end
        let hit_end = if hit_at >= 0 { f64::from(hit_at) + self.hit_frames } else { -1.0 };
        let from = if hit_end >= 0.0 && age_f >= hit_end { hit_end } else { 0.0 };
        let k = ((age_f - from) / self.alt_frames).floor();
        inp.play_alt_idle = k >= 1.0 && age_f - (from + k * self.alt_frames) < self.alt_on;
        // BlendListByBool_1
        let target = if inp.falling { 1.0 } else { 0.0 };
        self.air_w = Some(match self.air_w {
            None => target,
            Some(a) => {
                let d = STEP / AIR_BLEND;
                if target > a { target.min(a + d) } else { target.max(a - d) }
            }
        });
        let aw = self.air_w.unwrap_or(0.0);
        // the cached pose is updated while a slot passes it on (bAlwaysUpdateSourcePose false); the instance's update
        // counter goes on every tick (the actor's age), so IdleState sees a hold as a missed update and restarts after
        // it
        let src =
            ((1.0 - aw) * (1.0 - slot_weight(inst, "DefaultSlot"))).max(aw * (1.0 - slot_weight(inst, "AirSlot")));
        if src > ZERO_W {
            self.machine.update_at(&self.inp, STEP, clips, i64::from(age));
        }
        // HitShake
        let sa = s.shake_at;
        if sa >= 0 && age > sa {
            self.shake = rich_curve(&SHAKE, (f64::from(age - sa) * STEP).min(SHAKE_LEN));
        }
    }

    #[allow(clippy::too_many_arguments, reason = "the place call (s, x, y, shown) with the view's reads")]
    fn place(
        &mut self,
        s: &MeeseeksSummon,
        pr: &Projectile,
        inst: Option<(&MontageState, &Montage)>,
        clips: &mut Clips,
        x: f64,
        y: f64,
        shown: bool,
    ) {
        self.sk.visible = shown;
        if let Some(c) = &mut self.club {
            c.visible = shown && s.golfing;
        }
        if !shown {
            return;
        }
        self.advance(s, pr, inst, clips);
        // the actor at UE (0, -x, y), yaw -90 facing right; facing left, the right-facing view reflected across UE Y
        let k = pr.scale;
        let sc = self.mesh_scale * f64::from(k);
        let right = actor_rot();
        let rot: Rows = std::array::from_fn(|j| right[j] * if s.facing_left && j % 3 == 1 { -1.0 } else { 1.0 });
        // the relative X along the actor's forward (rows 0..2)
        let fx = (self.rel[0] + self.shake * SHAKE_X) * f64::from(k);
        let loc = [rot[0] * fx, -x + rot[1] * fx, y + self.rel[2] * f64::from(k) + rot[2] * fx];
        let xf = Xform { loc, rot, scale: [sc; 3] };
        self.sk.set(&xf);
        if let Some(c) = &mut self.club {
            c.set(&xf);
        }
    }

    /// The anim instance's updates up to the actor's age and the pose they give.
    fn advance(
        &mut self,
        s: &MeeseeksSummon,
        pr: &Projectile,
        inst: Option<(&MontageState, &Montage)>,
        clips: &mut Clips,
    ) {
        let age = pr.age;
        if self.last_age != Some(age) {
            // (the ticks since the last presented one; a rollback back in time: one)
            let n = match self.last_age {
                Some(l) if age > l => (age - l).min(30),
                _ => 1,
            };
            for t in (0..n).rev() {
                self.update(s, f64::from(pr.vy), inst, age - t, clips);
            }
            self.last_age = Some(age);
            let SummonView { sk, machine, slots, base, ground, air, out, .. } = self;
            let skel = sk.skeleton(clips);
            machine.evaluate(clips, &skel, base);
            let g = if slots[0].apply(clips, &skel, inst, base, ground) { &*ground } else { &*base };
            let a = if slots[1].apply(clips, &skel, inst, base, air) { &*air } else { &*base };
            let aw = self.air_w.unwrap_or(0.0);
            if aw <= 0.0 {
                out.copy(g);
            } else if aw >= 1.0 {
                out.copy(a);
            } else {
                blend_pose(g, a, aw, out);
            }
            sk.show(out);
            if let Some(club) = &mut self.club
                && s.golfing
            {
                match (&self.club_map, &mut self.club_pose) {
                    (Some(m), Some(cp)) => {
                        let (o, r) = (&self.out, &club.rest);
                        for (b, &j) in m.iter().enumerate() {
                            let (from, j) = if j >= 0 { (o, j as usize) } else { (r, b) };
                            cp.t[b * 3..b * 3 + 3].copy_from_slice(&from.t[j * 3..j * 3 + 3]);
                            cp.s[b * 3..b * 3 + 3].copy_from_slice(&from.s[j * 3..j * 3 + 3]);
                            cp.r[b * 4..b * 4 + 4].copy_from_slice(&from.r[j * 4..j * 4 + 4]);
                        }
                        club.show(cp);
                    }
                    _ => club.show(&self.out),
                }
            }
        }
    }

    fn draw(&self, out: &mut Vec<MeshPose>) {
        self.sk.draw(out);
        if let Some(c) = &self.club {
            c.draw(out);
        }
    }
}

/// `summonRule`: the view, made anew for a new owner (a reflect's SetTeamColor; CanBeReflected is false in the
/// build).
#[derive(Debug)]
struct SummonMesh {
    id: ProjectileId,
    /// The spawning fighter: the class data, clips and meshes.
    inst: FighterId,
    owner: FighterId,
    /// Meeseeks_SkelMesh's and golfClub's meshes, its relative location and X scale.
    mesh: String,
    club: Option<String>,
    rel: V3,
    rs: f64,
    view: Option<SummonView>,
}

fn summon_view(host: &mut ActorHost<'_>, w: &World, m: &SummonMesh) -> Option<SummonView> {
    let ci = host.colour(w, m.owner).color_index;
    let sk = meeseeks_mesh(host, w, m.inst, &m.mesh, Some(ci))?;
    let club = m.club.as_deref().and_then(|c| meeseeks_mesh(host, w, m.inst, c, None));
    if m.club.is_some() && club.is_none() {
        warn_once("warning: Meeseeks summon: the golf club is not drawn (its mesh or materials are not loaded)".into());
    }
    let clips = host.clips_of(w, m.inst)?;
    for n in ["Meeseeks_Spawn", "Meeseeks_Stand", "Meeseeks_Wave", "Meeseeks_HR_01", "Meeseeks_HR_02"] {
        if clips.get(n).is_none() {
            warn_once(format!("warning: Meeseeks summon: clip not loaded {n}"));
        }
    }
    Some(SummonView::new(sk, club, clips, m.rel, m.rs))
}

fn summon_mesh(host: &mut ActorHost<'_>, w: &World, a: ActorRef, cs: &ActorComponents) -> Option<Box<dyn ActorMesh>> {
    let ActorRef::Projectile(id) = a else { return None };
    let pr = w.actors.projectiles.get(id)?;
    let ProjectileExt::MeeseeksSummon(s) = &pr.ext else { return None };
    let owner = pr.owner?;
    let inst = w.by_id(s.inst).unwrap_or(owner);
    let skel = |c: &crate::actors::Component| match &c.kind {
        ComponentKind::SkeletalMesh { mesh, .. } => Some(mesh.clone()),
        _ => None,
    };
    let c = cs.get(MEESEEKS_MESH)?;
    let mesh = skel(c)?;
    let club = cs.get(CLUB).and_then(skel);
    let mut m = SummonMesh { id, inst, owner, mesh, club, rel: c.loc, rs: c.scale[0], view: None };
    m.view = Some(summon_view(host, w, &m)?);
    Some(Box::new(m))
}

/// The actor mesh's montage instance and its montage.
fn summon_montage<'w>(
    w: &'w World,
    id: ProjectileId,
    s: &'w MeeseeksSummon,
) -> Option<(&'w MontageState, &'w Montage)> {
    let st = s.anim.m.as_ref()?;
    let m = montage_of(&w.data, w, ActorRef::Projectile(id), w.names.str(st.montage.name))?;
    Some((st, m))
}

impl ActorMesh for SummonMesh {
    fn place(
        &mut self,
        host: &mut ActorHost<'_>,
        w: &World,
        at: &ActorPlace,
        alpha: f64,
        shown: bool,
        out: &mut Vec<MeshPose>,
    ) {
        let Some(pr) = w.actors.projectiles.get(self.id) else { return };
        let ProjectileExt::MeeseeksSummon(s) = &pr.ext else { return };
        if let Some(o) = pr.owner
            && o != self.owner
        {
            self.owner = o;
            self.view = summon_view(host, w, self);
        }
        let inst = summon_montage(w, self.id, s);
        let (Some(v), Some(clips)) = (&mut self.view, host.clips_of(w, self.inst)) else { return };
        let (x, y) = at.at(alpha);
        v.place(s, pr, inst, clips, x, y, shown);
        v.draw(out);
    }

    fn tick(&mut self, host: &mut ActorHost<'_>, w: &World) {
        let Some(pr) = w.actors.projectiles.get(self.id) else { return };
        let ProjectileExt::MeeseeksSummon(s) = &pr.ext else { return };
        if pr.owner.is_some_and(|o| o != self.owner) {
            return;
        }
        let inst = summon_montage(w, self.id, s);
        let (Some(v), Some(clips)) = (&mut self.view, host.clips_of(w, self.inst)) else { return };
        v.advance(s, pr, inst, clips);
    }

    fn bones(&self, out: &mut Vec<(Arc<str>, Xform)>) {
        if let Some(v) = &self.view {
            v.sk.bones_world(out);
        }
    }
}

/// A warning once a run.
fn warn_once(msg: String) {
    static SEEN: std::sync::Mutex<Vec<String>> = std::sync::Mutex::new(Vec::new());
    let mut seen = SEEN.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    if !seen.contains(&msg) {
        eprintln!("{msg}");
        seen.push(msg);
    }
}
