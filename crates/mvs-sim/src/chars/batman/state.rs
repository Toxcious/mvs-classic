//! Batman's own fighter state (`BatmanState`): Pawn_BatmanV2's own variables and
//! Batman_AnimBP's, beside BaseFighter's. Offsets are Pawn_BatmanV2's ubergraph's. A block per file that writes it.

use crate::Real;
use crate::State;
use crate::chars::CharState;
use crate::combat::BuffRef;
use crate::consts::GRAVITY_SCALE;
use crate::fighter::FighterId;
use crate::fighter::anim::SlotMix;
use crate::world::World;

/// `BatmanState`.
#[derive(State, Debug)]
pub struct BatmanState {
    // ---- the pawn (pawn.rs, anim.rs: glide, occlusion, 2D hitboxes, aim weights)
    /// isGliding: ReadGlideInput @3175, SetGlideValueMulticast @5972, BPI_Cleanup @8023 (false).
    pub gliding: bool,
    /// Batman_AnimBP.IsGliding (BPI_SetIsGliding from ReadGlideInput @3325 and SetGlideValueMulticast @6135); BPI_Cleanup
    /// clears isGliding alone, so after a respawn in the glide the AnimBP keeps its true until the next change.
    pub anim_gliding: bool,
    /// ReadGlideInput's DoOnce on entering the glide (@3887-4163, Temp_bool_IsClosed_Variable).
    pub glide_once_closed: bool,
    /// OriginalGravityScale: BeginPlay @1826 saves CharacterMovement.GravityScale.
    pub original_gravity_scale: Real,
    /// BeginPlay's Delay(0.2) @1925 -> @181 UpdateHurtBoxes([HurtBoxGlide], false): ticks left, 0 when none.
    pub glide_hurt_delay: Real,
    /// The last BPI_IdleFade(On) (@7832): SetGlideValueMulticast @5991 and AnimNotify_Darkness. The view runs the timeline.
    pub idle_fade: bool,
    /// SpeedOn (BPI_GetSpeed): OccludedEvent @5235 true; the Delay's resume @15 and BPI_Cleanup false.
    pub speed_on: bool,
    /// OccludedSpeedBuff (@5216): never cleared after the buff's destruction.
    pub occluded_speed_buff: Option<BuffRef>,
    /// OccludedDecrementEvadeBuff (@5358): never cleared after the buff's destruction.
    pub occluded_evade_buff: Option<BuffRef>,
    /// NotOccludedEvent's Delay(1.0) @4794 -> @15: ticks left, 0 when none.
    pub not_occluded_delay: Real,
    /// Batman_AnimBP's AimDirection (@3391; AimDirectionInverted is its negation).
    pub aim_direction: Real,
    /// Batman_AnimBP's HorizontalDividedByMax (@1643 / @3743).
    pub horizontal_divided_by_max: Real,
    /// The aim slots' poses mixed into the fighter's pose in order (CharHooks.slot_mix); its `slot_blend` and
    /// `slot_players` stay `None` (Batman's slot graph has no timed node).
    pub slot_mix: SlotMix,
    /// `Fighter.sampleBoxes`' buffer for a `slot_mix` pose. Not state.
    #[state(skip)]
    pub mix_buf: Option<Vec<f32>>,

    // ---- the glide stack under the shapes (glide.rs)
    /// Batman_AnimBP's IsGlidingAlpha (@4239, float(IsGliding)): ModifyBone_4's alpha.
    pub glide_alpha: Real,
    /// BlendListByBool(IsGliding)'s weight of child 0 (the glide stack; BlendTime 0 in, 0.1 s out).
    pub glide_w: Real,
    /// The machine GlideAdditive: 0 GlideEnter, 1 GlideLoop.
    pub glide_sm: Real,
    /// Its state's player time.
    pub glide_sm_time: Real,
    /// The first update after it became relevant (no transition taken).
    pub glide_sm_first: bool,
    /// GlideLoop (BlendSpacePlayer_1): the input the filter kept and the normalised time.
    pub glide_loop_x: Real,
    pub glide_loop_t: Real,
    /// GlideAdd: the input the filter kept and the normalised time.
    pub glide_add_x: Real,
    pub glide_add_t: Real,
    /// SpringBone_4 (SpringJoint1) and SpringBone_3 (SpringJoint2): the bone's world place and velocity (UE X, Y, Z and
    /// their V), six each.
    pub springs: [Real; 12],
    /// The spring nodes' RemainingTime.
    pub spring_rem: Real,
    /// Whether the spring nodes have evaluated since the pawn was made.
    pub spring_init: bool,
    /// (Root Y - SpringJointN Y) in world space of the evaluation before: the next update's SpringJoint1_offsetY /
    /// SpringJoint2_offsetY times (FacingLeft ? 1 : -1).
    pub glide_j1: Real,
    pub glide_j2: Real,

    // ---- grapple (grapple.rs: Pawn_BatmanV2 @613-1659, @389-612, @4735)
    /// GrappleInfoRecieved.
    pub grapple_info: bool,
    /// GrappleDestination (the sim's x and y: UE Y = -x, Z = y).
    pub grapple_dest_x: Real,
    pub grapple_dest_y: Real,
    /// GrappleTime.
    pub grapple_time: Real,
    /// 'Grapple Move Notify': the notify's index in its montage (the move command's key); -1 none.
    pub grapple_notify: Real,
    /// GrappleMoveTimer: the fighter tick whose timer pass it fires in; -1 none.
    pub grapple_timer_at: Real,
    /// SkipImpulseValidation (read by BaseFighter ValidateMoveCommand alone, not ported).
    pub skip_impulse_validation: bool,
}

impl Default for BatmanState {
    /// The field initial values.
    fn default() -> Self {
        BatmanState {
            gliding: false,
            anim_gliding: false,
            glide_once_closed: false,
            original_gravity_scale: GRAVITY_SCALE,
            glide_hurt_delay: 0.0,
            idle_fade: false,
            speed_on: false,
            occluded_speed_buff: None,
            occluded_evade_buff: None,
            not_occluded_delay: 0.0,
            aim_direction: 0.0,
            horizontal_divided_by_max: 0.0,
            slot_mix: SlotMix::default(),
            mix_buf: None,
            glide_alpha: 0.0,
            glide_w: 0.0,
            glide_sm: 0.0,
            glide_sm_time: 0.0,
            glide_sm_first: true,
            glide_loop_x: 0.0,
            glide_loop_t: 0.0,
            glide_add_x: 0.0,
            glide_add_t: 0.0,
            springs: [0.0; 12],
            spring_rem: 0.0,
            spring_init: false,
            glide_j1: 0.0,
            glide_j2: 0.0,
            grapple_info: false,
            grapple_dest_x: 0.0,
            grapple_dest_y: 0.0,
            grapple_time: 0.0,
            grapple_notify: -1.0,
            grapple_timer_at: -1.0,
            skip_impulse_validation: false,
        }
    }
}

// ---- the call surface of the state

/// `batmanOf`: a fighter's Batman state, `None` when it is not a Batman (a failed cast to BPI_Batman).
pub fn batman_of(w: &World, f: FighterId) -> Option<&BatmanState> {
    match &w.fighter(f).cs {
        CharState::Batman(s) => Some(s),
        _ => None,
    }
}

/// `batmanOf`, to write.
pub fn batman_mut(w: &mut World, f: FighterId) -> Option<&mut BatmanState> {
    match &mut w.fighter_mut(f).cs {
        CharState::Batman(s) => Some(s),
        _ => None,
    }
}

/// `batman` (his own code's read, the fighter a Batman). A panic is a hook entered for a fighter of another character.
pub fn batman(w: &World, f: FighterId) -> &BatmanState {
    batman_of(w, f).unwrap_or_else(|| panic!("fighter {f:?} is not a Batman"))
}

/// `batman`, to write.
pub fn batman_w(w: &mut World, f: FighterId) -> &mut BatmanState {
    batman_mut(w, f).unwrap_or_else(|| panic!("fighter {f:?} is not a Batman"))
}
