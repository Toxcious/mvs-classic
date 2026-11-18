//! The pawn core of Pawn_Velma: Velma_AnimBP's slot mix and its HitStun, the
//! hair pair, BPI_Velma's setters the other files call (`set_aim_value`, `set_carrying`), HasDodgeMeterCondition,
//! Velma_ProjectileShieldActor (the shared `shield` kind, `actors::shield_actor`) and AnimNotify_VisionGlasses; its
//! hooks (`pawnHooks`).

use crate::Real;
use crate::actors::WorldActorClass;
use crate::actors::shield_actor;
use crate::consts::MAX_EVADE_COUNT;
use crate::events::NotifyRef;
use crate::fighter::anim::SlotMix;
use crate::fighter::anim::slot_blend::slot_blend_reset;
use crate::fighter::anim::slot_mix::set_sub_mesh;
use crate::fighter::{FighterId, facing};
use crate::name;
use crate::names::Name;
use crate::registry::entries::{Condition, NotifyHandler};
use crate::registry::{CharHooks, Registry, RegistryError};
use crate::world::World;

use crate::math::{max, min};

use super::state::{velma, velma_mut, velma_of, velma_w};

/// Velma_ProjectileShieldActor_C: a WW_ShieldActor child with no functions of its own.
/// Deviation: her WallbounceCollision is ProjectileDestructible with Pawn Ignore, so in the build it blocks no capsule;
/// the shared `shield` kind makes it a wall part. The same today only because the shield ignores every fighter while
/// younger than KnockbackCollisionBufferTime (1.0 s) and lives about 0.32 s.
pub const SHIELD_ACTOR: &str = "Velma_ProjectileShieldActor_C";

/// `slotInput`: Velma_AnimBP's slot graph inputs (`not HitStun and IsCarryingAlly`, IsCrouching, the aim pair), as
/// the AnimBP holds them at this update (`pawn.rs` header: HitStun from HitStunStart / HitStunEnd, IsCarryingAlly from
/// BPI_IsCarrying, IsCrouching from On Crouch Changed, AimDirection from BPI_SetAimDirection).
// Deviation: the port does not update a dead pawn's AnimBP (the build's mesh still ticks), so the lists come back from a
// respawn where the death left them, and step on from there. Deviation: the AnimBP binds CrouchChange after a Delay 0
// at its init (BlueprintInitializeAnimation @8236 -> BindAnimationEvents), so a crouch change in a new pawn's first
// tick would be missed there; the port reads isCrouching from the first tick.
fn slot_input(w: &World, f: FighterId, n: Name) -> Real {
    let s = velma(w, f);
    if n == name!("AnimGraphNode_BlendListByBool") {
        if !s.anim_hit_stun && s.carrying_ally { 1.0 } else { 0.0 }
    } else if n == name!("IsCrouching") {
        if w.fighter(f).crouch.crouching { 1.0 } else { 0.0 }
    } else if n == name!("AimDirection") {
        s.aim_direction
    } else if n == name!("AimDirectionInverted") {
        -s.aim_direction
    } else {
        Real::NAN
    }
}

/// `slotMix`: the pawn record's mix.
fn slot_mix(w: &World, f: FighterId) -> Option<&SlotMix> {
    velma_of(w, f).map(|s| &s.pawn.slot_mix)
}

/// [`slot_mix`], to write.
fn slot_mix_mut(w: &mut World, f: FighterId) -> Option<&mut SlotMix> {
    velma_mut(w, f).map(|s| &mut s.pawn.slot_mix)
}

/// `hitStunStart`: HitstunHandler.HitStunStart -> Velma_AnimBP HitStunStart @9364.
fn hit_stun_start(w: &mut World, f: FighterId) {
    velma_w(w, f).anim_hit_stun = true;
}

/// `hitStunEnded`: Velma_AnimBP HitStunEnd @9376-9387.
fn hit_stun_ended(w: &mut World, f: FighterId) {
    let s = velma_w(w, f);
    s.anim_hit_stun = false;
    s.carrying_ally = false;
}

/// `init` and `turned` (`hair`): Pawn_Velma SetRotationLogic, after the parent's (@1641): SetSubMeshVisibility(LeftHair,
/// FacingLeft) @1660 and (RightHair, !FacingLeft) @1757, as Pawn_WonderWomanV3's. The spawn's SetRotationLogic sets the
/// pair the first time. A skin without the two meshes (her default one) changes nothing (`set_sub_mesh`: no such mesh).
fn hair(w: &mut World, f: FighterId) {
    let fc = facing(w, f);
    set_sub_mesh(w, f, name!("LeftHair"), fc < 0.0);
    set_sub_mesh(w, f, name!("RightHair"), fc > 0.0);
}

/// `death`: the slot mix emptied (the port does not update a dead pawn's AnimBP).
fn death(w: &mut World, f: FighterId) {
    velma_w(w, f).pawn.slot_mix.slot_mix.clear();
}

/// pawn.rs's part of the joined matchReset (after mod.rs's new state): a new pawn's anim instance (the lists at
/// INDEX_NONE), then the hair pair again (SetupInitialVisibility and SpawnCharacter's SetRotationLogic).
pub fn match_reset(w: &mut World, f: FighterId) {
    if let Some(b) = velma_w(w, f).pawn.slot_mix.slot_blend.as_mut() {
        slot_blend_reset(b);
    }
    hair(w, f);
}

/// `setAimValue` (BPI_SetAimValue @7001 -> Velma_AnimBP BPI_SetAimDirection @9247): AimDirection from the pitch in
/// degrees (MapRangeClamped(pitch, -30, 30, -1, 1)). Called by beam.rs.
pub fn set_aim_value(w: &mut World, f: FighterId, pitch: Real) {
    let pct = min(max((pitch + 30.0) / 60.0, 0.0), 1.0);
    velma_w(w, f).aim_direction = -1.0 + (pct * 2.0);
}

/// `setCarrying` (BPI_IsCarrying @6642 -> Velma_AnimBP @9344: IsCarryingAlly). Called by carry.rs.
pub fn set_carrying(w: &mut World, f: FighterId, on: bool) {
    velma_w(w, f).carrying_ally = on;
}

/// HasDodgeMeterCondition Check @0-463: BPI_GetEvadeCount < BPI_GetMaxEvadeCount (`getEvadeCount`,
/// `maxEvadeCount`).
fn has_dodge_meter(w: &mut World, f: FighterId) -> bool {
    // MaxEvadeCount is BaseFighter's class default (no pawn of the data sets another).
    w.fighter(f).evade.evade_count < MAX_EVADE_COUNT
}

/// AnimNotify_VisionGlasses NotifyBegin: BPI_SetGlasses(false) -> GlassesOff @2764-3076: GlassesOffCount + 1, the
/// GlassesOff_C widget (a full-screen BackgroundBlur) made anew and AddToViewport(0) @2548-2697. NotifyEnd:
/// BPI_SetGlasses(true) -> GlassesOn @3081-3348: GlassesOffCount - 1, the widget destroyed at 0 or less @2717. The
/// widget is up while the count is above 0 (a new pawn's count is 0: its state is made anew); ReceiveDestroyed @6133,
/// AbductPlayer @6233 and EndOfMatch @6333 destroy it with the pawn or the match. The HUD draws it (`mvs-ui`
/// hud/chars/velma.rs).
// Deviation: both events are gated on IsPlayingOnline(GetPlayerController(0)) && IsLocallyControlled() @2764 / @3081,
// which is each machine's own (the World is the same on every peer): the count is kept for every Velma, and the HUD
// applies the gate (an online match, this machine's pawn). The build's count of a pawn the gate refuses stays 0.
fn glasses_off(w: &mut World, f: FighterId, _n: NotifyRef, _key: i32) {
    if let Some(s) = velma_mut(w, f) {
        s.glasses_off_count = s.glasses_off_count.wrapping_add(1);
    }
}

/// AnimNotify_VisionGlasses NotifyEnd: GlassesOn (`glasses_off`).
fn glasses_on(w: &mut World, f: FighterId, _n: NotifyRef, _key: i32) {
    if let Some(s) = velma_mut(w, f) {
        s.glasses_off_count = s.glasses_off_count.wrapping_sub(1);
    }
}

/// The hooks of this file in Velma's CharHooks (`pawnHooks`: slotInput, slotMix, hitStunStart, hitStunEnded, init,
/// turned, death; its matchReset part is composed in mod.rs).
pub fn hooks(h: &mut CharHooks) {
    h.slot_input = Some(slot_input);
    h.slot_mix = Some(slot_mix);
    h.slot_mix_mut = Some(slot_mix_mut);
    h.hit_stun_start = Some(hit_stun_start);
    h.hit_stun_ended = Some(hit_stun_ended);
    h.init = Some(hair);
    h.turned = Some(hair);
    h.death = Some(death);
}

/// This file's registry entries: HasDodgeMeterCondition_C, Velma_ProjectileShieldActor_C (WW_ShieldActor's kind on
/// its own class data), AnimNotify_VisionGlasses_C.
pub fn register(r: &mut Registry) -> Result<(), RegistryError> {
    r.conditions.enter("HasDodgeMeterCondition_C", Condition::Fn(has_dodge_meter))?;
    r.actors.enter(
        SHIELD_ACTOR,
        shield_actor::entry(
            WorldActorClass::VelmaProjectileShieldActor,
            &["WW_ShieldActor_C", "SpawnedPlatformParent_C"],
        ),
    )?;
    r.notifies.enter(
        "AnimNotify_VisionGlasses_C",
        NotifyHandler { begin: Some(glasses_off), end: Some(glasses_on), ..NotifyHandler::default() },
    )
}
