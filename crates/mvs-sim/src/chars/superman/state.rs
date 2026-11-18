//! Superman's own fighter state (`SupermanState`): Pawn_Superman's own
//! variables and Superman_AnimBP's, beside BaseFighter's. Offsets are Pawn_Superman's ubergraph's unless a block names
//! another graph. Every field is declared here, a block per file that writes it.

use crate::Real;
use crate::State;
use crate::chars::CharState;
use crate::fighter::FighterId;
use crate::world::World;

/// IceMaxCount (Default__Pawn_Superman_C).
pub const ICE_MAX_COUNT: Real = 28.0;
/// IceDelayCooldown (s).
pub const ICE_DELAY_COOLDOWN: Real = 2.5;
/// IceRechargeSpeed (s; a float variable): `Math.fround(0.2)`.
pub const ICE_RECHARGE_SPEED: Real = 0.2f32 as Real;

/// `SupermanState`.
#[derive(State, Debug)]
pub struct SupermanState {
    // ---- the pawn and the ice meter (pawn.rs: state.rs's functions and mod.rs's hooks; ice.rs spends it)
    /// IceCount: IceMaxCount at BeginPlay @1101; BPI_UpdateIceBreath takes 1 with no clamp @2204-2273 (it may stand
    /// below 0); the recharge adds 1 @704-773.
    pub ice_count: Real,
    /// The RetriggerableDelay(IceDelayCooldown) of BPI_UpdateIceBreath @2452: its TimeRemaining (float seconds), -1
    /// none.
    pub ice_delay: Real,
    /// IceRechargeTimer @620: 0 none, 1 set and pending for the frame's timer pass, 2 active.
    pub ice_timer: i32,
    /// The timer manager's clock since the pass that activated it.
    pub ice_timer_t: crate::Clock,
    /// Its ExpireTime on that clock.
    pub ice_timer_at: crate::Clock,
    /// UI IceBreath.ShakeEvent() calls (TryIceBreath @3702), counted.
    pub ice_shakes: i32,
    /// SupermanCapsuleHight: the root capsule's CapsuleHalfHeight (unscaled) saved at BeginPlay @1460.
    pub cap_half: Real,
    /// SupermanCapsuleRadius (@1509).
    pub cap_radius: Real,
    /// Superman_AnimBP.AimOn (AnimNotify_SupermanAimDash Begin / End @121; pawn.rs).
    pub aim_on: bool,

    // ---- flight.rs: Superman_AnimBP's flight inputs
    /// AngleToVelocityAlpha.
    pub angle_to_velocity_alpha: Real,
    /// Direction (degrees).
    pub direction: Real,
    /// DirectionSoft.
    pub direction_soft: Real,
    /// FlyAngle.
    pub fly_angle: Real,
    /// FlyLean.
    pub fly_lean: Real,

    // ---- aim_punch.rs: Superman_AnimBP.AimDashFromLocation (the port's x / y)
    pub aim_dash_from_x: Real,
    pub aim_dash_from_y: Real,
}

impl Default for SupermanState {
    /// The field initial values.
    fn default() -> Self {
        SupermanState {
            ice_count: ICE_MAX_COUNT,
            ice_delay: -1.0,
            ice_timer: 0,
            ice_timer_t: 0.0,
            ice_timer_at: 0.0,
            ice_shakes: 0,
            cap_half: 0.0,
            cap_radius: 0.0,
            aim_on: false,
            angle_to_velocity_alpha: 0.0,
            direction: 0.0,
            direction_soft: 0.0,
            fly_angle: 0.0,
            fly_lean: 0.0,
            aim_dash_from_x: 0.0,
            aim_dash_from_y: 0.0,
        }
    }
}

// ---- the call surface of the state

/// `supermanOf`: a fighter's Superman state, `None` when it is not a Superman.
pub fn superman_of(w: &World, f: FighterId) -> Option<&SupermanState> {
    match &w.fighter(f).cs {
        CharState::Superman(s) => Some(s),
        _ => None,
    }
}

/// `supermanOf`, to write.
pub fn superman_mut(w: &mut World, f: FighterId) -> Option<&mut SupermanState> {
    match &mut w.fighter_mut(f).cs {
        CharState::Superman(s) => Some(s),
        _ => None,
    }
}

/// `superman` (his own code's read, the fighter a Superman). A panic is a hook entered for a fighter of another
/// character.
pub fn superman(w: &World, f: FighterId) -> &SupermanState {
    superman_of(w, f).unwrap_or_else(|| panic!("fighter {f:?} is not a Superman"))
}

/// `superman`, to write.
pub fn superman_w(w: &mut World, f: FighterId) -> &mut SupermanState {
    superman_mut(w, f).unwrap_or_else(|| panic!("fighter {f:?} is not a Superman"))
}
