//! Reindog's render module: the entry, the AnimBP's pose hook and the classes' mesh
//! rules.
//!
//! Pawn_Reindog on the render side. PlayerDataReadyMulticast's team tint (@315-869) goes to SM_SeismicReactor, a pawn
//! component that is bHiddenInGame and that nothing shows: not drawn; VisualCrouch's +40 on the mesh (@1799-2198) is
//! Mesh.RelativeLocation, the sim's (meshZ).
//!
//! The pose beside the shared one (Creature_AnimBP). The graph, input first (LinkID ->
//! ChildProperties):
//!   the linked layer (Base_AnimBP_Reindog's BaseMovementMachine: the shared pose, the sim's clip) -> SaveCachedPose
//!   -> BlendListByBool (bActiveValue <- Is holding a rock; BlendPose 0, true: a LayeredBoneBlend of Creature_Idle
//!   over Spine1-3; BlendPose 1, false: the cached pose itself) -> Slot DefaultSlot (bAlwaysUpdateSourcePose true)
//!   -> Slot DashSlot (false) -> SaveCachedPose_1 "BeforeFace" ->
//!   ApplyAdditive (Additive: SequencePlayer_1 Creature_Heart_Pulse, alpha 1) -> the mirror node
//!   (Creature_MirrorTableV2) -> LocalToComponent -> ModifyBone_1 (fml_un_COM, rotation BMM_Replace in
//!   BCS_ComponentSpace <- MakeRotator(0, Spin_Rotation, 0), alpha <- float(COM_Spin)) -> ModifyBone_2 (VB Spine)
//!   -> the skins' Trail / AnimDynamics / SpringBone nodes -> ModifyBone (fml_un_beard, roll +30 additive, alpha <-
//!   AdjustBeard) -> ... -> ModifyBone_3 (fml_un_C_head scale <- CreatureHeadScale) -> ComponentToLocal.
//!
//! Drawn here:
//! - ApplyAdditive: Creature_Heart_Pulse (SequencePlayer_1: StartPosition 0, PlayRate 1, bLoopAnimation; its time
//!   keeps running whatever the slots play, as the node sits above them) at the node's Alpha 1.0 (a constant: no pin,
//!   curve or bool feeds it), on the pose the slots and the BlendListByBool give, before the mirror and ModifyBone_1
//!   (so before the spin, in this rule). The clip's tracks: fml_un_C_gem's translation (a constant +13.287 cm on the
//!   parent's Z) and scale, and the scales of the elf and chef skins' gems (fml_un_C_elf_gem, fml_un_rdg_chef_C_gem),
//!   a beat over frames 0-17 of 31 (0.5 s); the other tracks are the additive identity. The store's clip is the
//!   build's cooked delta (its ABPT_AnimScaled base, Creature_Hear_Null, is constant).
//!   Unverified: hitpause: the mesh's CustomTimeDilation 0.005 slows the player while the fighter's hitpause > 0, the
//!   same reading as Batman's pose.
//! - ModifyBone_1, the spin of AnimNotify_ReindogSpin (Reindog_AllyItem_Montage, 0.2751 + 0.2462 s). The update is
//!   the sim's (mvs-sim chars/creature/pawn.rs: Spin_Rotation's FInterpTo_Constant toward -360, COM_Spin from the
//!   notify's begin / end and StopRotation, with the build's one-update lag of the queued events); set() reads the
//!   values this frame's update left (ReindogState com_spin / spin_rotation). The evaluation's alpha is
//!   float(COM_Spin): the rotation in the model's space replaced by the pitch (PoseCx::turn_com, the same Replace as
//!   the hit turn).
//!
//! Handled elsewhere:
//!   - DefaultSlot / DashSlot: the sim's montage slots.
//!   - The mirror: the view draws a left facing as the reflection of the right-facing pose (Unverified: the mirror
//!     node taken as a full reflection). It sits before ModifyBone_1, whose Replace is a pitch about the axis the
//!     reflection keeps, so the spin is taken as it is at both facings.
//!   - ModifyBone_3 (fml_un_C_head; ScaleMode BMM_Additive in BCS_BoneSpace, Scale <- CreatureHeadScale, Alpha 1.0):
//!     the shared head-scale step (the entry's head_scale), after this rule.
//!
//! The menu preview (PoseHook::preview): a view with no fighter runs no skin node but draws the
//! beard. Unverified: whether a menu's anim instance gets OnSkinApplied is not read.
//!
//! Not ported:
//!   - BlendListByBool (Is holding a rock): nothing writes the variable, the class default false keeps BlendPose 1.
//!   - ModifyBone_2 (VB Spine: a virtual bone the view does not hold and nothing attaches to).
//!
//! The skins' nodes after ModifyBone_2 (25 Trail, 32 AnimDynamics, 4 SpringBone, the ModifyBone on fml_un_beard):
//! skin_controls, which draws the beard and runs the skin nodes of the fighter data, each under its gate.

use mvs_sim::World;
use mvs_sim::actors::projectile::{Projectile, ProjectileExt};
use mvs_sim::chars::creature::state::reindog_of;
use mvs_sim::fighter::FighterId;
use mvs_vfx::math::rotation_rows;

use super::{CharRender, PoseHook, PoseOf, PreviewOf};
use crate::actors::{MeshRule, Rows};
use crate::anim::{Clips, HeadScaleNode, Pose, apply_additive};
use crate::view::{CharView, PoseCx, PoseRules};

mod skin_controls;

use skin_controls::SkinDynamics;

pub(crate) const RENDER: CharRender = CharRender {
    // Creature_AnimBP: ModifyBone_3 (scale Additive, bone space, <- CreatureHeadScale; Pawn_Reindog @1744)
    head_scale: &[HeadScaleNode {
        node: "ModifyBone_3",
        bone: "fml_un_C_head",
        factor: 1.0,
        gated: false,
        moves: [0.0; 3],
        scale: true,
    }],
    pose: Some(creature_pose),
    ..CharRender::GENERIC
};

/// The classes' own mesh drawing, by class name: SeismicReactor_C's mesh is hidden in
/// game and missing from the dumps, Fastball_C has no mesh and no drawn component (its ThrownMulticast has no caller).
pub(crate) const RULES: &[(&str, MeshRule)] = &[
    ("SeismicReactor_C", MeshRule { rot: Some(reactor_rot), mesh: Some(nothing) }),
    ("Fastball_C", MeshRule { rot: None, mesh: Some(nothing) }),
];

/// SeismicReactor_C's rotation (ProjectileFxRule.rot): yaw Comp_Spawner's FacingLeft ? 90 : -90 plus RotatingMovement's
/// spin (SeismicReactor.spin), pitch and roll 0 (no bRotationFollowsVelocity); as mvs-sim chars/creature/seismic.rs yaw
/// (change both).
fn reactor_rot(_w: &World, pr: &Projectile) -> Option<Rows> {
    let spin = match &pr.ext {
        ProjectileExt::SeismicReactor(r) => r.spin,
        _ => 0.0,
    };
    let yaw = (if pr.facing > 0.0 { -90.0 } else { 90.0 }) + spin;
    Some(rotation_rows(0.0, f64::from(yaw) * std::f64::consts::PI / 180.0, 0.0))
}

/// A view that draws nothing (no place, no end).
#[derive(Debug)]
struct Nothing;

impl crate::actors::ActorMesh for Nothing {
    fn place(
        &mut self,
        _host: &mut crate::actors::ActorHost<'_>,
        _w: &World,
        _at: &crate::actors::ActorPlace,
        _alpha: f64,
        _shown: bool,
        _out: &mut Vec<crate::frame::MeshPose>,
    ) {
    }
}

fn nothing(
    _host: &mut crate::actors::ActorHost<'_>,
    _w: &World,
    _a: mvs_sim::actors::ActorRef,
    _cs: &crate::actors::ActorComponents,
) -> Option<Box<dyn crate::actors::ActorMesh>> {
    Some(Box::new(Nothing))
}

/// SequencePlayer_1's Sequence.
const PULSE: &str = "Creature_Heart_Pulse";

/// The player's time at the first tick's pose.
// Deviation: every view is posed twice before the first tick (at the match's start and after the warm-up frames),
// each one STEP of the player's clock; the build's player starts with the pawn's AnimInstance.
const PULSE_START: f64 = 2.0 / 60.0;

/// `creaturePose`'s state on one view.
#[derive(Debug)]
struct CreaturePose {
    pulse: Option<usize>,
    pulse_len: f64,
    pulse_rate: f64,
    ta: Pose,
    /// SequencePlayer_1's time.
    pulse_time: f64,
    dilation: f64,
    /// COM_Spin and Spin_Rotation (degrees).
    com_spin: bool,
    spin_rotation: f64,
    /// PoseOf::slaved or PoseOf::stand_in: no Reindog state read (a slaved view and a
    /// Polymorph stand-in have no character state).
    no_state: bool,
    /// The skin's nodes after the spin (None: none on this skin).
    dyn_: Option<SkinDynamics>,
}

fn creature_pose(view: &mut CharView, clips: &Clips, of: &PoseOf<'_>) -> Box<dyn PoseHook> {
    let pulse = clips.get(PULSE);
    if pulse.is_none() {
        eprintln!("warning: Reindog pose: {PULSE} is not loaded: the gem does not beat");
    }
    let (pulse_len, pulse_rate) =
        pulse.map_or((0.0, 1.0), |c| (clips.clips[c].anim.length, clips.clips[c].anim.rate_scale));
    Box::new(CreaturePose {
        pulse,
        pulse_len,
        pulse_rate,
        ta: Pose::new(view.rest.n),
        pulse_time: PULSE_START,
        dilation: 1.0,
        com_spin: false,
        spin_rotation: 0.0,
        no_state: of.slaved || of.stand_in,
        dyn_: SkinDynamics::new(view, &of.skin.slug, &of.skin.name, of.slaved),
    })
}

impl PoseRules for CreaturePose {
    fn rule(&mut self, cx: &mut PoseCx<'_>, p: &mut Pose, dt: f64) {
        // ApplyAdditive: the player's update, then the additive at alpha 1
        if let Some(c) = self.pulse
            && self.pulse_len > 0.0
        {
            self.pulse_time = (self.pulse_time + dt * self.dilation * self.pulse_rate) % self.pulse_len;
            if self.pulse_time < 0.0 {
                self.pulse_time += self.pulse_len;
            }
            apply_additive(cx.clips, c, cx.rig, cx.rest, self.pulse_time, p, &mut self.ta);
        }
        if self.com_spin {
            let h = self.spin_rotation * std::f64::consts::PI / 360.0;
            cx.turn_com(p, [0.0, 0.0, h.sin(), h.cos()]);
        }
        if let Some(d) = &mut self.dyn_ {
            d.apply(p, dt);
        }
    }
}

impl PoseHook for CreaturePose {
    /// `preview()`: a view with no fighter: no spin, and the skin's nodes told so.
    fn preview(&mut self, _view: &mut CharView, _of: &PreviewOf<'_>) {
        self.com_spin = false;
        self.spin_rotation = 0.0;
        self.dilation = 1.0;
        if let Some(d) = &mut self.dyn_ {
            d.preview();
        }
    }

    fn set(&mut self, w: &World, f: FighterId, view: &mut CharView, _clips: &Clips) {
        let s = if self.no_state { None } else { reindog_of(w, f) };
        self.com_spin = s.is_some_and(|s| s.com_spin);
        self.spin_rotation = s.map_or(0.0, |s| f64::from(s.spin_rotation));
        self.dilation =
            if w.fighter(f).hit.hitpause > 0.0 { f64::from(mvs_sim::consts::HITPAUSE_DILATION) } else { 1.0 };
        if let Some(d) = &mut self.dyn_ {
            d.set(w, f, view, self.dilation, s.is_some_and(|s| s.is_respawning), self.com_spin);
        }
    }
}
