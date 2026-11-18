//! Garnet's own fighter state (`GarnetState`): Pawn_Garnet's own variables and
//! those of her own classes that live on the pawn, beside BaseFighter's. One record per file, every field declared here
//! whichever file writes it; a field read by two files lives in the record of the file that writes it, the fields every
//! file reads are `GarnetState`'s own, written through pawn.rs's BPI_Garnet functions.
//! Sim space: x = -UE Y, y = UE Z.

use crate::Real;
use crate::State;
use crate::actors::{ActorRef, ProjectileId};
use crate::chars::CharState;
use crate::fighter::FighterId;
use crate::fighter::anim::SlotMix;
use crate::world::World;

/// `GarnetPawnState` (pawn.rs, aim.rs): BeginPlay, BPI_Garnet, Garnet_AnimBP's sim part, the conditions.
#[derive(State, Debug, Default)]
pub struct GarnetPawnState {
    /// MarkerLocationBeforeDestroy (sim x, y): MulticastSetIdleGauntlets @12619-12669 saves the marker's location when
    /// the incoming one and IdleGauntlet are both valid. Read by ChargeToGauntlet's destination (dash.rs).
    pub marker_location_before_destroy_x: Real,
    pub marker_location_before_destroy_y: Real,
    /// Garnet_AnimBP's slot graph mix (fighter/anim graph_slots writes it at each pose update from aim.rs slot_input).
    pub slot_mix: SlotMix,
}

/// `GarnetAimState` (aim.rs / pawn.rs): SetAimDirection @9777 and its AimResetTimeline, SetGarnetAimFlag @12250,
/// SetReadAimFromMovement @10019, the Tick's aim @13946-14522 and charging-to-marker aim @13705-13920.
#[derive(State, Debug, Default)]
pub struct GarnetAimState {
    /// AimDirection: -1 down .. 1 up.
    pub aim_direction: Real,
    /// AimDirectionBeforeReset: SetAimDirection(Reset) @9791.
    pub aim_direction_before_reset: Real,
    /// AimResetTimeline: playing, and its position (s, length 0.5).
    pub reset_on: bool,
    pub reset_pos: Real,
    /// AimFlag: BPI_SetGarnetAimFlag @12250 (AnimNotify_GarnetAimFlag).
    pub aim_flag: bool,
    /// SetAimValueFromMovement / ReadAimAsHorizontalMovement: BPI_SetReadAimFromMovement @10019 / @10038.
    pub set_aim_value_from_movement: bool,
    pub read_aim_as_horizontal_movement: bool,
}

/// `GarnetGroundState` (shockwave.rs): SetGroundPositions @7917 -> @4316, the Tick's probes @7927-8580, Do Ground
/// Trace, GarnetShockwaveHitBox (+ Left / Right).
#[derive(State, Debug, Default)]
pub struct GarnetGroundState {
    /// LeftPositions / RightPositions: sim (x, y) pairs, x0 y0 x1 y1 ...
    pub left_positions: Vec<Real>,
    pub right_positions: Vec<Real>,
    /// LeftPosition / RightPosition: the probes (sim x, y).
    pub left_x: Real,
    pub left_y: Real,
    pub right_x: Real,
    pub right_y: Real,
    /// IsCheckingLeft / IsCheckingRight: on at SetGroundPositions @4831 / @4842, off at a probe's miss @8265 / @8580.
    pub checking_left: bool,
    pub checking_right: bool,
}

/// `GarnetGauntletsState` (gauntlets.rs): empty, the gloves' variables are their projectile's.
#[derive(State, Debug, Default)]
pub struct GarnetGauntletsState {}

/// `GarnetZoneState` (zone.rs): the electric charge.
#[derive(State, Debug, Default)]
pub struct GarnetZoneState {
    /// ChargingElectricity: BPI_CheckChargeStatus Charging.
    pub charging_electricity: bool,
    /// ElectricityFullyCharged: BPI_CheckChargeStatus FullyCharged.
    pub electricity_fully_charged: bool,
}

/// `GarnetMarkerState` (marker.rs): empty, the marker's state is its actor's.
#[derive(State, Debug, Default)]
pub struct GarnetMarkerState {}

/// `GarnetDashState` (dash.rs): empty.
#[derive(State, Debug, Default)]
pub struct GarnetDashState {}

/// `GarnetHoldState` (hold.rs): empty.
#[derive(State, Debug, Default)]
pub struct GarnetHoldState {}

/// `GarnetSingState` (sing.rs): SetSinging @7911 -> @7786, MulticastIsSigning @18948, ApplySongBuffToArea @19780, the
/// Sing Sphere's overlaps @13142 / @13578, Buff_GarnetStackingSinging.
#[derive(State, Debug, Default)]
pub struct GarnetSingState {
    /// OverlappingSingActors: the enemy spawned actors in the Sing Sphere (projectiles, items, world actors).
    pub overlapping_sing_actors: Vec<ActorRef>,
    /// The spawned actors the Sing Sphere overlaps, of any team, in the order they began.
    pub inside: Vec<ActorRef>,
    /// SingingVFX is valid (MulticastIsSigning(true) @19045).
    pub singing_vfx: bool,
    /// ApplySongBuffToArea's ubergraph locals: the last call's Entering, the retries, the Delay(0) @538 pending.
    pub song_entering: bool,
    pub song_retries: Real,
    pub song_retry_pending: bool,
}

/// `GarnetStopProjState` (stop_projectile.rs): empty, its state is the buff's on the stopped actor.
#[derive(State, Debug, Default)]
pub struct GarnetStopProjState {}

/// `GarnetState`.
#[derive(State, Debug, Default)]
pub struct GarnetState {
    pub pawn: GarnetPawnState,
    pub aim: GarnetAimState,
    pub ground: GarnetGroundState,
    pub gauntlets: GarnetGauntletsState,
    pub zone: GarnetZoneState,
    pub marker: GarnetMarkerState,
    pub dash: GarnetDashState,
    pub hold: GarnetHoldState,
    pub sing: GarnetSingState,
    pub stopproj: GarnetStopProjState,
    /// IdleGauntlet: the marker out, a GarnetMarker2 (marker.rs). BPI_SetIdleGauntlet @4054, MulticastSetIdleGauntlets
    /// @12533, BPI_ResetGauntlets clears it.
    pub idle_gauntlet: Option<ProjectileId>,
    /// ProjectileGauntlets: the side special's gloves in flight (gauntlets.rs), MulticastSetProjectileGauntlet @15214.
    pub projectile_gauntlets: Option<ProjectileId>,
    /// LocationBeforeMove (sim x, y): ChargeToGauntlet(true)'s capture of her own @6469 (dash.rs).
    pub location_before_move_x: Real,
    pub location_before_move_y: Real,
    /// ChargingToMarker: ChargeToGauntlet @6469 (dash.rs).
    pub charging_to_marker: bool,
    /// IsActiveSinging: MulticastIsSigning @18948 (sing.rs). Not BPI_IsSinging's answer (always false).
    pub is_active_singing: bool,
}

// ---- the call surface of the state

/// `garnetOf`: the fighter's Garnet state, `None` for a fighter of another character.
pub fn garnet_of(w: &World, f: FighterId) -> Option<&GarnetState> {
    match &w.fighter(f).cs {
        CharState::Garnet(s) => Some(s),
        _ => None,
    }
}

/// `garnetOf`, to write.
pub fn garnet_mut(w: &mut World, f: FighterId) -> Option<&mut GarnetState> {
    match &mut w.fighter_mut(f).cs {
        CharState::Garnet(s) => Some(s),
        _ => None,
    }
}

/// `garnet` (her own code's read). A panic is a hook entered for a fighter of another character.
pub fn garnet(w: &World, f: FighterId) -> &GarnetState {
    garnet_of(w, f).unwrap_or_else(|| panic!("fighter {f:?} is not a Garnet"))
}

/// `garnet`, to write.
pub fn garnet_w(w: &mut World, f: FighterId) -> &mut GarnetState {
    garnet_mut(w, f).unwrap_or_else(|| panic!("fighter {f:?} is not a Garnet"))
}
