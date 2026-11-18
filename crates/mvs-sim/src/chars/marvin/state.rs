//! Marvin's own fighter state (`MarvinState`): Pawn_C018's own variables, those
//! of C018_AnimBP the sim reads, and the live actors he owns. Offsets are Pawn_C018's ubergraph's. One record per group
//! of files, with its class defaults; the fields every file reads are `MarvinState`'s own. A spawned actor's own
//! variables are on the actor (its kind's class), not here; the reversal sweep's list is a local of
//! HandleReverseAllProjectiles: none is kept. Sim space: x = -UE Y, y = UE Z.

use crate::Real;
use crate::State;
use crate::actors::ActorRef;
use crate::chars::CharState;
use crate::fighter::FighterId;
use crate::fighter::anim::SlotMix;
use crate::world::World;

/// `MarvinPawnState` (pawn.rs, aim.rs): BeginPlay's AnimBP cache @3249-3366, the disintegration self-damage
/// (@2936-3003, @1261-1615, TakeDisintegrationDamage @4807-4975, EndDisintegrationDamageOnTimer @4976 -> MulticastEnd
/// @5001), BPI_SetDisintegrationHit @2931 -> @2834-2864, the wall-cling trace @5042-5927, the aim rotation @2903;
/// C018_AnimBP's sim slot mix (AimValue's TwoWayBlend over the 13 aim tracks). `mixBuf` is no state.
#[derive(State, Debug, Default)]
pub struct MarvinPawnState {
    /// C018_AnimBP's slot graph mix (the pose update writes it from the slotInput hook): the tracks mixed into the
    /// fighter's anim in order, which the box sampler reads for the hit and hurt shapes. No timed BlendListByBool and
    /// no SequencePlayer: `slot_blend` / `slot_players` stay `None`.
    pub slot_mix: SlotMix,
    /// Base_AnimBP_C018's WallBlendSpaceValue (the linked layer's variable, class default 0): C018_AnimBP's
    /// SetWallBlendSpaceValue @3511-3553 writes 1 - the pawn's WallClingFlagBlend there, from the pawn tick's wall-cling
    /// trace @5927 while IsWallSliding; it keeps its last value otherwise. CharHooks.wall_blend_space_value reads it.
    pub wall_blend_space_value: Real,
}

/// `MarvinUfoState` (ufo.rs): C018_Platform without the piloting, C018CannonBlast, C018RocketExplosion. Empty: the
/// UFO's state is its actor's (`MarvinState::ufo` the live one).
#[derive(State, Debug, Default)]
pub struct MarvinUfoState {}

/// `MarvinBubbleState` (bubble.rs): C018BubbleProj, C018BubblePop, Buff_C018BubbledProjectile,
/// Buff_C018BubbleHandleIncomingDamage. Empty: the bubble's state is its BubbleProj's and its buffs' ext.
#[derive(State, Debug, Default)]
pub struct MarvinBubbleState {}

/// `MarvinBlasterState` (blaster.rs, ray.rs): C018UpBlasterProj (+ Air), C018ComboBlasterProjVersion2,
/// C018DisintegrationRay (+ _Down), AnimNotify_C018DamageFromDisintegration, Condition_C018DisintegrationHasHit. Empty.
#[derive(State, Debug, Default)]
pub struct MarvinBlasterState {}

/// `MarvinFlagState` (flag.rs, homing.rs): C018Flag (+ C018AirFlag), Buff_C018HomingVictim, Buff_C018HomingProjectile.
/// Empty.
#[derive(State, Debug, Default)]
pub struct MarvinFlagState {}

/// `MarvinSwapState` (swap.rs, reverse.rs): C018SwapProjectile, Buff_C018WarpFighterHit, Buff_C018ProjectileWarpMove /
/// WarpEnemy, HandleReverseAllProjectiles / ReverseProjectile, Buff_C018ReverseProjectile,
/// AnimNotify_C018ReverseProjectiles. Empty.
#[derive(State, Debug, Default)]
pub struct MarvinSwapState {}

/// `MarvinPerkState` (perks.rs): C018_PerkPlatform, C018PerkCannonBlast / C018PerkRocketExplosion,
/// Buff_C018PerkHomingProjectile, Buff_C018_Redirect, Buff_C018_PerkProjectileSpeed, Buff_C018_Aim. Empty.
#[derive(State, Debug, Default)]
pub struct MarvinPerkState {}

/// `MarvinDisint`: Pawn_C018's disintegration variables (pawn.rs's; ray.rs's ray and notify write them through
/// pawn.rs's setters).
#[derive(State, Debug, Default)]
pub struct MarvinDisint {
    /// IsTakingDisintegrationDamage: BPI_SetDamageFromDisintegration(take, inAir) @2936-3003 (pawn.rs).
    pub taking: bool,
    /// DisintegrationHasHit: BPI_SetDisintegrationHit @2931 -> @2834-2864 (pawn.rs; caller C018DisintegrationRay,
    /// ray.rs). Read by BPI_DisintegrationHasHit (DisintegrationHasHit or not IsTakingDisintegrationDamage) and the
    /// condition.
    pub has_hit: bool,
    /// DisintegrationDamageIncrementTImer / EndDisintegrationDamageTimer: the looping increment timer (0.19 s) and the
    /// one-shot Air / Ground max-duration timer (1 / 3 s) @1261-1615 (pawn.rs disint_timers, in the frame's timer
    /// pass). Each: 0 none, 1 set in this frame's tick groups (pending until the timer pass), 2 active; its
    /// InternalTime since it went active (`inc_t` / `end_t`, seconds) and its ExpireTime (`inc_at` / `end_at`).
    pub inc_timer: i32,
    pub inc_t: crate::Clock,
    pub inc_at: crate::Clock,
    pub end_timer: i32,
    pub end_t: crate::Clock,
    pub end_at: crate::Clock,
    /// The duration the max-duration timer was set with: SelectFloat(Air, Ground, InAir) @1435.
    pub end_rate: Real,
}

/// `MarvinState`.
#[derive(State, Debug, Default)]
pub struct MarvinState {
    /// pawn.rs, aim.rs.
    pub pawn: MarvinPawnState,
    /// ufo.rs (`ufo` below is the live actor).
    pub platform: MarvinUfoState,
    /// bubble.rs.
    pub bubble: MarvinBubbleState,
    /// blaster.rs, ray.rs.
    pub blaster: MarvinBlasterState,
    /// flag.rs, homing.rs.
    pub flag: MarvinFlagState,
    /// swap.rs, reverse.rs.
    pub swap: MarvinSwapState,
    /// perks.rs.
    pub perks: MarvinPerkState,
    // The fields every file reads. Writers in brackets.
    /// Anim Aim Rotation Degrees: BPI_SetAnimAimRotation @2903 (pawn.rs; callers AnimNotify_C018SetAnimAimRotation
    /// and _HasPerkSetAnimAimRotation, pawn.rs), read by BPI_GetAnimAimRotation (the blasters, the UFO's aim).
    pub aim_deg: Real,
    /// C018_AnimBP AimValue: the slot mix's input (aim.rs writes it from `aim_deg`, the view draws the aim slots).
    pub aim_value: Real,
    /// pawn.rs (ray.rs calls the setters).
    pub disint: MarvinDisint,
    /// Pawn_C018 WallClingFlagBlend (class default 0): the wall-cling trace @5042-5927 writes it (pawn.rs);
    /// C018_AnimBP passes 1 - it to Base_AnimBP's SetWallBlendSpaceValue @3511-3553 (`pawn.wall_blend_space_value`);
    /// the view reads it for the flag drag.
    pub wall_cling_blend: Real,
    /// The pawn's Cannon variable: the live C018_Platform (or C018_PerkPlatform), `None` when none. BPI_SetCannon
    /// @1616 (pawn.rs set_cannon; caller the UFO's BeginPlay @11515, ufo.rs / perks.rs) writes it; BPI_IsCannonSpawned
    /// (IsValid) and BPI_IsCannonReadyToFire (CurrentCannonAmmo > 0 and ReadyToFire) read it: the conditions
    /// (pawn.rs), the view.
    pub ufo: Option<ActorRef>,
}

/// `mvOf`: any fighter's; `None` when it is not a Marvin (a failed cast to Pawn_C018 / BPI_C018).
pub fn marvin_of(w: &World, f: FighterId) -> Option<&MarvinState> {
    match &w.fighter(f).cs {
        CharState::Marvin(s) => Some(s),
        _ => None,
    }
}

/// `mvOf`, to write.
pub fn marvin_mut(w: &mut World, f: FighterId) -> Option<&mut MarvinState> {
    match &mut w.fighter_mut(f).cs {
        CharState::Marvin(s) => Some(s),
        _ => None,
    }
}

/// `mv`: his own code's read (the fighter is a Marvin: his hooks). A panic is a hook entered for a fighter of another
/// character.
pub fn marvin(w: &World, f: FighterId) -> &MarvinState {
    marvin_of(w, f).unwrap_or_else(|| panic!("fighter {f:?} is not a Marvin"))
}

/// [`marvin`], to write.
pub fn marvin_w(w: &mut World, f: FighterId) -> &mut MarvinState {
    marvin_mut(w, f).unwrap_or_else(|| panic!("fighter {f:?} is not a Marvin"))
}
