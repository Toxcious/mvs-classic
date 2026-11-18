//! Batman_AnimBP's aim slots in the sim (Batman_AnimBP; Batman_Special_F_Montage, Batman_Attack_Air_F_v1_Montage):
//! the montages whose tracks sit in the AimUp / AimCenter / AimDown or the AimBack / AimForward slots show the
//! weighted sum of them, as C019_AnimBP's does.
//!   TwoWayBlend_2 (A slot AimCenter, B slot AimUp, alpha AimDirection), TwoWayBlend_1 (A that, B slot AimDown, alpha
//!   AimDirectionInverted); TwoWayBlend (A slot AimBack, B slot AimForward, alpha HorizontalDividedByMax).
//! Each a float alpha, scale 1, bias 0, no blend time; the node's update clamps it to 0..1 and takes A alone at
//! 0.00001 and under, B alone at 0.99999 and over (FAnimNode_TwoWayBlend). The tracks of a montage at its position,
//! weights Center (1 - u)(1 - d), Up u(1 - d), Down d, or Back 1 - h, Forward h: the heaviest is the fighter's anim /
//! anim time and the others `BatmanState.slot_mix` (mix_slots), so the hit and hurt shapes and the drawn pose are one
//! mix. No montage of his has tracks in both sets (Batman_Special_F / Special_Air_F: Up, Center, Down;
//! Batman_Attack_Air_F_v1 and Batman_Nav_WallJump: Back, Forward); the other set's slots pass their source on.
//! Deviation: the nested blend is one weighted sum and the shapes mix the sampled box tracks linearly.

use super::glide::glide_update;
use super::state::{batman, batman_mut, batman_w};
use crate::Real;
use crate::fighter::FighterId;
use crate::fighter::anim::blend_spaces::fall_mix;
use crate::fighter::anim::montages::instance_data;
use crate::fighter::anim::slot_blend::SlotPart;
use crate::fighter::anim::update::shown_uid;
use crate::fighter::anim::{SlotMix, mix_slots};
use crate::fighter::body::def;
use crate::fighter::max_walk;
use crate::math::ue::deg_atan2;
use crate::registry::CharHooks;
use crate::world::World;

/// The hooks of this file in Batman's CharHooks.
pub fn hooks(h: &mut CharHooks) {
    h.slot_mix = Some(slot_mix);
    h.slot_mix_mut = Some(slot_mix_mut);
    h.anim_update = Some(anim_update);
    h.pose_updated = Some(slot_compose);
}

/// CharHooks.slotMix: Batman_AnimBP's aim slots' mix.
fn slot_mix(w: &World, f: FighterId) -> Option<&SlotMix> {
    Some(&batman(w, f).slot_mix)
}

/// `slotMix`, written (CharHooks.slot_mix_mut).
fn slot_mix_mut(w: &mut World, f: FighterId) -> Option<&mut SlotMix> {
    batman_mut(w, f).map(|s| &mut s.slot_mix)
}

const UP_DOWN: [&str; 3] = ["AimCenter", "AimUp", "AimDown"];
const BACK_FWD: [&str; 2] = ["AimBack", "AimForward"];

/// TwoWayBlend's clamp and band.
fn alpha(a: Real) -> Real {
    if a >= 0.99999 {
        1.0
    } else if a > 0.00001 {
        a
    } else {
        0.0
    }
}

/// The update's aim (@2666-3460, valid CompSpawner): ProjectileAimVector with Y and Z both nonzero gives
/// NormalizeToRange(FacingLeft ? DegAtan2(Z, Y) : -DegAtan2(-Z, -Y), -45, 45) x 2 - 1, else 0 (@3775).
/// AimDirectionInverted = -AimDirection. Not clamped here (the air aim range [-60, 35] passes -1). Float steps as the
/// build's: DegAtan2 (FMath::Atan2, one multiply by 180 / pi), then the range and the x 2 - 1.
pub fn aim_direction_of(y: Real, z: Real, facing_left: bool) -> Real {
    if !(y != 0.0 && z != 0.0) {
        return 0.0;
    }
    let (fy, fz) = (y, z);
    let a = if facing_left { deg_atan2(fz, fy) } else { -deg_atan2(-fz, -fy) };
    ((a + 45.0) / 90.0) * 2.0 - 1.0
}

// CompSpawner.ProjectileAimVector (UE world Y, Z): the fighter's proj_aim, written by a ProjectileAimer's tick
// (the aimer tick @13463, AimType 3 as 0) and decayed by ResetAim's timer; Harley's AnimBP reads the
// same. CompSpawner is the owner's Comp_Spawner_C, set at the AnimBP's init (@16-201, retried after a Delay(0) while
// invalid): every fighter has the component, so the gate @2666 is always passed.
// Deviation: the update reads the vector the step left, as Harley's (the aimer's tick against the mesh's in a frame
// is not read: one frame early at most).

/// CharacterMovement.GetMaxSpeed, the stock one (no FighterMovementComponent override: FMC vtable; stock
/// 1429302b0): the knockback's Custom mode MaxCustomMovementSpeed, Flying MaxFlySpeed (BaseFighter's 0: the product
/// alone), Walking MaxWalkSpeedCrouched while crouched, else (Walking, Falling) MaxWalkSpeed, the speed buffs' value.
fn get_max_speed(w: &World, f: FighterId) -> Real {
    let fi = w.fighter(f);
    if fi.hit.kb_mode {
        return crate::math::narrow(def(w, f).max_custom_speed);
    }
    if fi.moves.flying {
        return crate::math::narrow(def(w, f).max_fly_speed);
    }
    if fi.body.on_floor && fi.capsule.cmc_crouched {
        crate::math::narrow(def(w, f).max_walk_crouched)
    } else {
        max_walk(w, f)
    }
}

/// BlueprintUpdateAnimation with a valid Fighter (@1103-1693): HorizontalDividedByMax = Velocity.Y x (FacingLeft ? 1 :
/// -1) / CharacterMovement.GetMaxSpeed(), the product alone when GetMaxSpeed is 0 (@3488-3743). UE Y = -x, so that is
/// the speed along the facing. Stepped after the montage advance of the tick, as the stock UpdateAnimation orders it
/// (montages, BlueprintUpdateAnimation, the graph), with the velocity of that moment.
pub fn anim_update(w: &mut World, f: FighterId, dilation: Real, pre: bool) {
    if pre {
        return;
    }
    let max = get_max_speed(w, f);
    let fi = w.fighter(f);
    let facing = fi.body.facing_dir;
    let fwd = fi.body.vx * facing;
    let (ay, az) = (fi.items.proj_aim.y, fi.items.proj_aim.z);
    let s = batman_w(w, f);
    s.horizontal_divided_by_max = if max != 0.0 { fwd / max } else { fwd };
    s.aim_direction = aim_direction_of(ay, az, facing < 0.0);
    slot_compose(w, f);
    // the glide stack under the shapes: its update and its springs' evaluation
    glide_update(w, f, dilation);
}

/// The mix, wherever the pose is written (CharHooks.poseUpdated) and after the update above.
pub fn slot_compose(w: &mut World, f: FighterId) {
    let data = w.data.clone();
    let sm = shown_uid(w, f);
    let (mut up_down, mut back_fwd) = (false, false);
    if let Some(m) = sm.and_then(|u| instance_data(&data, w, f, u)) {
        for t in &m.slots {
            match t.slot.as_str() {
                "AimCenter" | "AimUp" | "AimDown" => up_down = true,
                "AimBack" | "AimForward" => back_fwd = true,
                _ => {}
            }
        }
    }
    let mut out = std::mem::take(&mut batman_w(w, f).slot_mix.slot_mix);
    let ad = batman(w, f).aim_direction;
    let h = batman(w, f).horizontal_divided_by_max;
    if sm.is_some() && up_down {
        let (u, d) = (alpha(ad), alpha(-ad));
        let parts = [
            (SlotPart::Slot(UP_DOWN[0]), (1.0 - u) * (1.0 - d)),
            (SlotPart::Slot(UP_DOWN[1]), u * (1.0 - d)),
            (SlotPart::Slot(UP_DOWN[2]), d),
        ];
        mix_slots(w, f, sm, &parts, &mut out, None);
    } else if sm.is_some() && back_fwd {
        let h = alpha(h);
        let parts = [(SlotPart::Slot(BACK_FWD[0]), 1.0 - h), (SlotPart::Slot(BACK_FWD[1]), h)];
        mix_slots(w, f, sm, &parts, &mut out, None);
    } else {
        // no aim slot: AirState Fall's blend space, or nothing
        fall_mix(w, f, &mut out);
    }
    batman_w(w, f).slot_mix.slot_mix = out;
}
