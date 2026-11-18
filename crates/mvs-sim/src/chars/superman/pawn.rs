//! Pawn_Superman's own logic: the ice meter (spendIce, hasIce, iceTick, iceTimerTick), the
//! pawn's own hooks of `mod.rs` (BeginPlay's writes, matchReset, the latent and timer passes, TryIceBreath), his
//! conditions (Condition_HasIceBreathSuperman, Condition_NoIceBreathSuperman, SupermanNotRapidPunch,
//! SupermanNotHoldingAllyCondition) and the notify AnimNotify_SupermanAimDash. Offsets are Pawn_Superman's ubergraph's.

use super::state::{ICE_DELAY_COOLDOWN, ICE_MAX_COUNT, ICE_RECHARGE_SPEED, SupermanState, superman, superman_of};
use super::state::{superman_mut, superman_w};
use crate::chars::CharState;
use crate::consts::DELTA_SECONDS;
use crate::events::NotifyRef;
use crate::fighter::FighterId;
use crate::fighter::anim::montages::montage_is_active;
use crate::fighter::control::raw_press_dir;
use crate::fighter::match_side::sfx;
use crate::input::{AttackBtn, Dir};
use crate::name;
use crate::registry::{CharHooks, Condition, NotifyHandler, Registry, RegistryError};
use crate::world::World;

/// The hooks of this file in Superman's CharHooks. `match_reset` is not set here: `mod.rs` joins
/// [`match_reset`] and `flight::match_reset`.
// Deviation: `pressed` and not `input_pre_triggered`: the build binds TryIceBreath to the pawn's InputPreTriggered,
// which BaseFighter broadcasts at the special press ahead of the press's own tests; the port runs it from `pressed`,
// behind the press's handling. The order changes nothing but the place of the sound among the press's events (the press
// cannot change IceCount: a pellet's BeginPlay does). Kept so for the per-tick trace.
pub fn hooks(h: &mut CharHooks) {
    h.init = Some(begin_play);
    h.tick = Some(ice_tick);
    h.timer_tick = Some(ice_timer_tick);
    h.pressed = Some(try_ice_breath);
}

/// ReceiveBeginPlay @1838 -> @1021, after the parent's: IceCount = IceMaxCount @1101 (the state's initial value) and
/// the capsule's size saved @1460 / @1509. The other branches are the HUD's widget (@1046, @30-536), the AnimBP
/// reference @1323 and the skin's (@1559-1683). The pawn has no EndPlay, reset or cleanup of its own: the meter, its
/// delay and its timer go on through a ring-out and the respawn.
fn begin_play(w: &mut World, f: FighterId) {
    let (half, radius) = {
        let c = &w.fighter(f).capsule;
        (c.cap_half_u, c.cap_radius_u)
    };
    let s = superman_w(w, f);
    s.cap_half = half;
    s.cap_radius = radius;
}

/// `mod.rs` own `matchReset`: a match reset spawns a new pawn: a new state (a full meter, no delay, no timer, the
/// AnimBP's defaults) and its BeginPlay again, after the fighter's reset has put the capsule's size back. Called by
/// `mod.rs`'s join ahead of flight.rs's.
pub fn match_reset(w: &mut World, f: FighterId) {
    w.fighter_mut(f).cs = CharState::Superman(SupermanState::default());
    begin_play(w, f);
}

/// `spendIce` (BPI_UpdateIceBreath @2110, authority), called by Superman_IceBreath's BeginPlay on its owner (one call
/// a pellet; ice.rs): an active IceRechargeTimer is cleared @2140-2184, IceCount - 1 with no test of the count and no
/// clamp @2204-2273, the multicast @2420, then RetriggerableDelay(IceDelayCooldown) @2452 (a pending one's
/// TimeRemaining is set again). A fighter that is no Superman takes nothing (the caller's BPI_Superman cast).
/// The pellet's BeginPlay runs inside the owner's mesh tick (the spawner notify) or later in the frame, after the
/// pawn's own latent pass of that frame (AActor::Tick: ReceiveTick, then ProcessLatentActions), so the delay's first
/// count is the next frame's ([`ice_tick`]).
// Unverified: a pellet the build spawns ahead of the pawn's latent pass of its frame (none is known) would count
// 149 passes from the spawn to the first point, not 150.
pub fn spend_ice(w: &mut World, f: FighterId) {
    let Some(s) = superman_mut(w, f) else { return };
    s.ice_timer = 0;
    s.ice_count -= 1.0;
    s.ice_delay = ICE_DELAY_COOLDOWN;
}

/// `hasIce` (BPI_CheckIceBreath @0-34): IceCount > 0; a fighter that is no Superman false (the failed BPI_Superman
/// cast of Condition_HasIceBreathSuperman @149).
pub fn has_ice(w: &World, f: FighterId) -> bool {
    superman_of(w, f).is_some_and(|s| s.ice_count > 0.0)
}

/// IceRechargeTimerEvent @3216 -> @704 (no authority test): IceCount + 1, the multicast @920, and at IceCount >=
/// IceMaxCount the timer is cleared @952-1000. No clamp: a call at the maximum would pass it (nothing calls it
/// there: the timer is cleared at the maximum and the delay starts below it).
fn recharge(s: &mut SupermanState) {
    s.ice_count += 1.0;
    if s.ice_count >= ICE_MAX_COUNT {
        s.ice_timer = 0;
    }
}

/// The pawn's latent actions, after its ReceiveTick, whatever its state and on the world's delta (the delay counts in
/// a hit pause too): FDelayAction's TimeRemaining -= DeltaSeconds, done at <= 0 (150 passes for 2.5 s). Its link
/// @597: the looping timer is set @620 (pending until this frame's timer pass) and the body falls through to the
/// first + 1 @704, so the first point comes back in the pass the delay ends in and the timer's own first call 0.2 s
/// after that pass.
fn ice_tick(w: &mut World, f: FighterId) {
    let s = superman_w(w, f);
    if s.ice_delay < 0.0 {
        return;
    }
    s.ice_delay -= DELTA_SECONDS;
    if s.ice_delay > 0.0 {
        return;
    }
    s.ice_delay = -1.0;
    s.ice_timer = 1;
    recharge(s);
}

/// The frame's timer pass (FTimerManager::Tick, after every tick group). A timer set since the last pass becomes
/// active with ExpireTime = InternalTime + Rate, so the frame it was set in does not count; an active one:
/// InternalTime += DeltaSeconds and with InternalTime > ExpireTime (strict) the delegate runs CallCount =
/// trunc((InternalTime - ExpireTime) / Rate) + 1 times (a call that clears the timer ends them), then ExpireTime +=
/// (float)(CallCount x Rate): 12 passes a point at 60 Hz. The times are kept since the activating pass (sums of
/// float32 values in a double are exact here), as the Iron Giant's turret does.
fn ice_timer_tick(w: &mut World, f: FighterId) {
    let s = superman_w(w, f);
    if s.ice_timer == 0 {
        return;
    }
    if s.ice_timer == 1 {
        s.ice_timer = 2;
        s.ice_timer_t = 0.0;
        s.ice_timer_at = f64::from(ICE_RECHARGE_SPEED);
        return;
    }
    s.ice_timer_t += f64::from(DELTA_SECONDS);
    // (a negated test: a NaN time ends the pass)
    #[allow(clippy::neg_cmp_op_on_partial_ord, reason = "`!(a > b)` keeps the NaN result")]
    if !(s.ice_timer_t > s.ice_timer_at) {
        return;
    }
    let calls = ((s.ice_timer_t - s.ice_timer_at) / f64::from(ICE_RECHARGE_SPEED)).trunc() + 1.0;
    let mut i = 0.0;
    while i < calls && s.ice_timer == 2 {
        recharge(s);
        i += 1.0;
    }
    if s.ice_timer == 2 {
        s.ice_timer_at += f64::from(crate::math::narrow(calls * f64::from(ICE_RECHARGE_SPEED)));
    }
}

/// TryIceBreath @3520, bound at BeginPlay @1280-1303 to the pawn's InputPreTriggered(InputType, Direction): with the
/// Special button (Enum_InputButton 1), the direction Neutral (Enum_InputDirection8Way 4) and IceCount == 0 @3620 the
/// meter's widget shakes @3702 and ui_ability_cooldown is posted @3464. A count below 0 gives neither.
fn try_ice_breath(w: &mut World, f: FighterId, b: AttackBtn) {
    if b != AttackBtn::Special || superman(w, f).ice_count != 0.0 || raw_press_dir(w, f) != Dir::Neutral {
        return;
    }
    superman_w(w, f).ice_shakes += 1;
    sfx(w, f, Some(name!("ui_ability_cooldown")));
}

// ---- the conditions and the notify

/// Condition_HasIceBreathSuperman Check @0-125: the owner's BPI_CheckIceBreath (IceCount > 0); an owner without
/// BPI_Superman false @149. The rows of Superman_IceBreath_Montage (special neutral on the ground) and
/// Superman_Air_IceBreath_Montage (in the air) in the pawn's start tables and in BranchGroup_SupermanBase /
/// GroundtoAir carry it: no ice breath at 0 or below.
fn has_ice_breath(w: &mut World, f: FighterId) -> bool {
    has_ice(w, f)
}

/// Condition_NoIceBreathSuperman Check @0-58, a child of Condition_HasIceBreathSuperman with its own Check: Not(the
/// parent's Check). The two montages' ConditionalJumpToSection ticks to End carry it.
fn no_ice_breath(w: &mut World, f: FighterId) -> bool {
    !has_ice(w, f)
}

/// SupermanNotRapidPunch Check @0-263: not (Montage_IsPlaying(Superman_N_Attack_Rapid_Montage) or
/// Montage_IsPlaying(Superman_N_Attack_Rapid_2_Montage)) on the owner's first SkeletalMeshComponent's anim instance.
/// The pawn's ground side attack row (Superman_N_Attack_01_Montage) carries it; the branch groups' row of the same
/// montage does not. Montage_IsPlaying(montage) is the montage's ActiveMontagesMap entry with bPlaying
/// (FUN_142840760): an instance from its play to its first Stop, the automatic blend-out's included. bPlaying is
/// false on a paused instance alone, and nothing in the port pauses a montage instance (a hit pause is the actor's
/// time dilation).
fn not_rapid_punch(w: &mut World, f: FighterId) -> bool {
    !(montage_is_active(w, f, name!("Superman_N_Attack_Rapid_Montage"))
        || montage_is_active(w, f, name!("Superman_N_Attack_Rapid_2_Montage")))
}

/// SupermanNotHoldingAllyCondition Check @0-389: BPI_GetFighterCharacter == 10 (Pawn_Superman's byte; another
/// fighter leaves the return value unwritten: false @156 -> @389) and Not(BPI_GetIsHoldingAlly) @249-324.
/// BPI_SetIsHoldingAlly is called by SupermanPickUpAlly alone, which Superman_Flying_Montage activates and nothing
/// spawns (dormant), so IsHoldingAlly stays false: true for a Superman. SupermanFlightHit and SupermanGrabListener
/// read it.
fn not_holding_ally(w: &mut World, f: FighterId) -> bool {
    w.names.str(w.fighter(f).ch) == super::KEY
}

/// AnimNotify_SupermanAimDash (Superman_GroundAimPunch_Montage f82-100.149) Received_NotifyBegin @121: the mesh's
/// Superman_AnimBP.AimOn = true. A pose input alone.
fn aim_dash_begin(w: &mut World, f: FighterId, _n: NotifyRef, _key: i32) {
    superman_w(w, f).aim_on = true;
}

/// Received_NotifyEnd @121: AimOn = false.
fn aim_dash_end(w: &mut World, f: FighterId, _n: NotifyRef, _key: i32) {
    superman_w(w, f).aim_on = false;
}

/// This file's registry entries (the conditions and the notify).
pub fn register(r: &mut Registry) -> Result<(), RegistryError> {
    r.conditions.enter("Condition_HasIceBreathSuperman_C", Condition::Fn(has_ice_breath))?;
    r.conditions.enter("Condition_NoIceBreathSuperman_C", Condition::Fn(no_ice_breath))?;
    r.conditions.enter("SupermanNotRapidPunch_C", Condition::Fn(not_rapid_punch))?;
    r.conditions.enter("SupermanNotHoldingAllyCondition_C", Condition::Fn(not_holding_ally))?;
    r.notifies.enter(
        "AnimNotify_SupermanAimDash_C",
        NotifyHandler { begin: Some(aim_dash_begin), end: Some(aim_dash_end), ..NotifyHandler::default() },
    )
}
