//! The sim's constants: numbers read from the build (data files) or measured in the
//! Lab. A number is a [`Real`]; a sound event a `&str`, interned where an event names it.

#![allow(clippy::excessive_precision, reason = "the literals are written digit for digit as the build has them")]

use crate::Real;

pub const TICK_HZ: Real = 60.0;
pub const DT: Real = 1.0 / TICK_HZ;
// CharMoveComp (BaseFighter). What a pawn class overrides (walk speeds, jumps, capsule, mesh scale, movement
// sounds, taunts, the pickup montages) is per character.
/// UE default world gravity
pub const GRAVITY_Z: Real = 980.0;
/// g = 5145, confirmed by jump arcs
pub const GRAVITY_SCALE: Real = 5.25;
pub const GROUND_FRICTION: Real = 3.0;
/// GroundFriction x BrakingFrictionFactor
pub const BRAKING_FRICTION: Real = GROUND_FRICTION * 5.0;
pub const BRAKING_DECEL: Real = 10000.0;
// FighterMovementComponent.MoveCommandBraking (+0xb0c; BaseFighter sets 10000). The CMC's pre-move step
// (FUN_1410d5550) writes it to BrakingDecelerationWalking and BrakingDecelerationFalling while GroundFriction
// differs from its BeginPlay value (a move command sets it to 0), and the BeginPlay values (10000 / 0) otherwise.
pub const MOVE_COMMAND_BRAKING: Real = 10000.0;
/// UE ApplyVelocityBraking
pub const BRAKE_TO_STOP_VELOCITY: Real = 10.0;
/// foot / head traces of WallSkate and CornerCheck: capsule radius + 15
pub const WALL_SKATE_TRACE_REACH: Real = 15.0;
/// MaxStepHeight (BaseFighter's CharMoveComp; no pawn overrides it)
pub const MAX_STEP_HEIGHT: Real = 100.0;
// MaxDepenetrationWithGeometry (CMC +0x2a8), the clamp of a start-penetrating move's resolve (GetPenetrationAdjustment
// 1429309a0); the bodies a fighter is resolved out of are no pawns (the box's actor: 142fe4e20).
pub const MAX_DEPENETRATION: Real = 500.0;
/// MaxSimulationTimeStep 0.0166 < 1/60
pub const SUBSTEPS: Real = 2.0;
/// hitpause CustomTimeDilation
pub const HITPAUSE_DILATION: Real = 0.005;
/// MaxSimulationTimeStep
pub const MAX_SIM_STEP: Real = 0.0166;
/// walking capsule over its floor contact (memory: z 210.412 on top 18 + half 190)
pub const FLOOR_REST_GAP: Real = 2.412;
/// MaxAccelerationAir (GetMaxAcceleration while Falling, FMC native)
pub const MAX_ACCEL_AIR: Real = 4096.0;
/// BaseAirControl
pub const AIR_CONTROL: Real = 0.9;
/// stock default until the first UpdateAirControl
pub const AIR_CONTROL_BOOST_MULT: Real = 2.0;
pub const AIR_CONTROL_BOOST_THRESHOLD: Real = 25.0;
/// BaseFighter FallLateralFriction (holding against the motion)
pub const FALL_LATERAL_FRICTION: Real = 4.0;
/// CheckLateralFriction, within DASH_FALL_TIME of air after a dash
pub const DASH_FALL_FRICTION: Real = 1.2;
pub const DASH_FALL_TIME: Real = 0.1;
/// FastFallDelay
pub const FAST_FALL_DELAY: Real = 0.06;
/// FastFallStopSpeed
pub const FAST_FALL_STOP_SPEED: Real = -1000.0;
/// DownPressed: GetTimeSeconds - LastJumpTime >= 0.15
pub const FAST_FALL_JUMP_GAP: Real = 0.15;
pub const DASH_IMPULSE: Real = 1500.0;
/// GetDashPlayRate floor
pub const DASH_RATE_MIN: Real = 0.4;
pub const DASH_TIME: Real = 0.25;
/// IsQuickDash @75: GetGameTimeInSeconds - DashGameTime <= 0.3
pub const QUICK_DASH_TIME: Real = 0.3;
// Jumps (BaseFighter jump Blueprint)
pub const JUMP_STOP_MULT: Real = 0.175;
/// SingleJumpMinTime
pub const JUMP_MIN_TIME: Real = 0.15;
/// ACharacter JumpMaxCount (BaseFighter CDO)
pub const JUMP_MAX_COUNT: Real = 2.0;
/// DoubleJumpMinTime
pub const DOUBLE_JUMP_MIN_TIME: Real = 0.2;
// DeltaSeconds under the game's 60 fps cap, read from UWorld DeltaTimeSeconds (+0x5a8): measured frame time,
// 0.0166667-0.0166669, mostly 0.0166668. It advances the float32 world clock (TimeSeconds +0x598) and the
// latent delays; the movement integration keeps 1/60.
/// `Math.fround`: the double rounded to the nearest float32.
#[allow(clippy::cast_possible_truncation, reason = "Math.fround: the double rounded to float32")]
pub const DELTA_SECONDS: Real = 0.0166668_f64 as f32 as Real;

/// `ticks` frames of [`DELTA_SECONDS`] on the timer manager's clock: FTimerManager's InternalTime is a double sum of
/// the float frame times, exact as the product `k * DELTA_SECONDS` in double.
pub fn clock(ticks: Real) -> crate::Clock {
    f64::from(ticks) * f64::from(DELTA_SECONDS)
}
/// ground jump: DoubleJumpAvailable false, Delay(0.15)
pub const DOUBLE_JUMP_GATE: Real = 0.15;
pub const QUICK_FALL_VELOCITY: Real = -1200.0;
pub const DASH_JUMP_MULT: Real = 1.1875;
pub const JUMP_CONTROL_OVERRIDE: Real = 0.5;
pub const MAX_JUMPS: Real = 2.0;
pub const AIR_EVADE_MAX: Real = 2.0;
pub const AIR_SPECIAL_MAX: Real = 2.0;
// Comp_Pawn_Rage CDO (Shaggy's component overrides only RageChargeIncrement and the montages).
pub const RAGE_THRESHOLD: Real = 2.0;
/// ExtraHungryRageThreshold: IsExtraHungryPerk and the sandwich held (GetRageThreshold)
pub const EXTRA_HUNGRY_RAGE_THRESHOLD: Real = 1.5;
pub const RAGE_CHARGE_INCREMENT: Real = 1.0;
pub const RAGE_CHARGE_DECREMENT: Real = 1.0;
pub const RAGE_BUFF_DURATION: Real = 15.0;
// Comp_Pawn_Rage CDO MinResidualScale / MaxResidualScale (uniform): the charge residual's scale over the meter.
pub const RAGE_RESIDUAL_SCALE: [Real; 2] = [0.5, 1.0];
// EnableVFX(true) posts the loop; EnableVFX(false) posts the stop once per destroyed particle component.
pub const SFX_RAGE_LOOP: &str = "sfx_cha_shaggy_charge_loop";
pub const SFX_RAGE_LOOP_STOP: &str = "sfx_cha_shaggy_charge_loop_stop";
// BaseFighter's own Capsule component (Capsule_GEN_VARIABLE, not overridden by Pawn_Shaggy), returned by
// BPI_GetCapsuleCollision; Crouch / UnCrouch resize only the root CollisionCylinder.
pub const BF_CAPSULE_HALF: Real = 96.0;
pub const BF_CAPSULE_RADIUS: Real = 42.0;
// Evades
// BaseFighter CDO evade meter; Pawn_Shaggy overrides EvadeInvulnerabilityRechargeTime (6).
/// EvadeInvulnerableTime before the first UpdateEvadeValues
pub const EVADE_INVULN: Real = 0.225;
pub const EVADE_BASE_INVULN: Real = 0.21;
pub const MAX_EVADE_COUNT: Real = 100.0;
pub const EVADE_METER_COST: Real = 8.3;
pub const EVADE_RECHARGE_AMOUNT: Real = 8.3;
/// fallback when the pawn has none
pub const EVADE_RECHARGE_TIME: Real = 9.0;
/// BPI_DecrementEvadeCount(8) on landing or taking a hit
pub const EVADE_HIT_REFUND: Real = 8.0;
pub const SPAWN_INVULN: Real = 1.5;
/// BaseFighter CDO EvadeBlinkFresnelIntensity
pub const EVADE_BLINK_FRESNEL: Real = 2.0;
pub const RESPAWN_DELAY: Real = 2.0;
/// Comp_Pawn_DamageManager AssistWindow (the ClearAssist timer)
pub const ASSIST_WINDOW: Real = 5.0;
/// DamageAndLaunch: the line trace under the attacker (LastAttackerWasOffLedge)
pub const OFF_LEDGE_TRACE_LENGTH: Real = 2000.0;
/// respawn invulnerability = round((delay + SPAWN_INVULN) * 60) = 92 f
pub const SPAWN_INVULN_DELAY: Real = 0.03;
/// respawn velocity, set directly; NewFallVelocity clamps it to -terminal
pub const RESPAWN_VELOCITY_Z: Real = -7500.0;
// Ring-out: the CollisionComp box in hitstun, otherwise the box padded by these (both sides x2 side pad).
pub const BLAST_PAD_SIDE: Real = 100.0;
pub const BLAST_PAD_TOP: Real = 300.0;
pub const BLAST_PAD_BOTTOM: Real = 20.0;
// BFBounce locals
/// divides BaseKnockBack with KnockbackMultiplier
pub const BOUNCE_FLOOR_MULT: Real = 1.5;
pub const BOUNCE_MIN_SPEED_FLOOR: Real = 700.0;
pub const BOUNCE_MIN_SPEED_NOT_FLOOR: Real = 1100.0;
pub const BOUNCE_UP_MOD: Real = 0.2;
pub const BOUNCE_MIN_FLOOR_VEL: Real = 2200.0;
pub const BOUNCE_RAPID_TIME: Real = 0.4;
pub const BOUNCE_RAPID_COUNT: Real = 3.0;
pub const BOUNCE_HITPAUSE_MIN: Real = 0.07;
pub const BOUNCE_HITPAUSE_MAX: Real = 0.15;
pub const BOUNCE_KB_CAP: Real = 5000.0;
/// retriggerable timer from the end of hitstun
pub const DI_COMBO_RESET_FRAMES: Real = 24.0;
// Comp_Pawn_DamageManager CDO: combo decay.
/// Comp_Pawn_BranchManager AttackSpamTimeWindow
pub const ATTACK_SPAM_WINDOW: Real = 0.75;
/// AddRecentAttackToQueue: entries kept
pub const RECENT_ATTACK_QUEUE: Real = 4.0;
pub const ATTACK_DECAY_VALUES: [Real; 7] = [0.0, 0.0, 0.0, 0.8, 0.1, 0.05, 0.025];
/// applied damage when HitStunDecayMod < 1
pub const DAMAGE_DECAY_MOD: Real = 0.6;
/// MoveCountThreshold
pub const COMBO_RES_MOVE_COUNT: Real = 3.0;
/// GetHitStunDecay with ComboResistanceEnabled
pub const COMBO_RES_DECAY_MOD: Real = 0.1;
pub const KB_UP_MOD: Real = 1.07;
// PhysKnockback (native FUN_1410d8100): stock PhysFalling plus knockback friction, run while the True Hitstun
// Timer (KBFrictionHitStun) lasts. KnockbackFriction is set to 0.375 in BaseFighter BeginPlay;
// DownspikeFrictionModifier 0.25 (CDO). BrakingFrictionFactor is 5 for BaseFighter and Shaggy.
pub const KB_FRICTION: Real = 0.375;
pub const DOWNSPIKE_FRICTION: Real = 0.25;
pub const BRAKING_FRICTION_FACTOR: Real = 5.0;
// MaxCustomMovementSpeed (stock GetMaxSpeed in Custom modes) and MaxAcceleration (the FMC's GetMaxAcceleration
// outside Falling, native 1410c4a50), read from game memory. GetMinAnalogSpeed is 0 in Custom modes.
// MAX_ACCEL is the template's MaxAcceleration; a fighter's own is kept apart (its writers move it off 4096).
pub const MAX_ACCEL: Real = 4096.0;
pub const KB_DOWN_MOD: Real = 0.875;
/// DashJumpNoMeterMultiplier
pub const DASH_JUMP_NO_METER_MULT: Real = 1.0;
/// (MaxWalk + 25) x DashJumpNoMeterMultiplier
pub const DASH_JUMP_NO_METER_ADD: Real = 25.0;
/// CalcHorizontalJumpVel: no carry-over this soon after hitstun ends
pub const JUMP_AFTER_HITSTUN: Real = 0.25;
/// FastFallJumpBoostMult (height); velocity x (1 + (b - 1) / 2)
pub const FAST_FALL_JUMP_BOOST: Real = 1.1;
// Knockback (decoded Calculate Knock Back)
pub const KB_EXP_CURVE: Real = 6e-5;
pub const KB_SPEED_CAP: Real = 8250.0;
pub const KB_MULT: Real = 1.45;
/// BaseFighter Comp_Actor_Damage Weight; pawns override it (Shaggy 68)
pub const KB_WEIGHT: Real = 100.0;
/// Comp_Pawn_DamageManager HitPauseMultiplier (CDO, no overrides), read on the victim
pub const HIT_PAUSE_MULT: Real = 1.0;
// Comp_Pawn_HitstunHandler CDO
/// KnockBackToHitStunConv
pub const HITSTUN_CONV: Real = 0.007;
/// KnockBackToHitStunFallOffExp
pub const HITSTUN_EXP: Real = 0.515;
/// MinHitStunClamp
pub const HITSTUN_FLOOR: Real = 0.3;
// Wall slide
/// WallSlideTerminalVelocity
pub const WALL_SLIDE_TERMINAL: Real = 230.0;
/// WallSlideSlowMultiplier, MoveY >= 0.5
pub const WALL_SLIDE_SLOW_MULT: Real = 0.25;
/// WallStallPenaltyMultiplierMax
pub const WALL_STALL_MULT_MAX: Real = 6.0;
/// WallStallTimer → SetWallStallPenaltyCapped
pub const WALL_STALL_TIME: Real = 10.0;
/// WallFatigueTimer (looping IncrementEvadeCount once capped)
pub const WALL_FATIGUE_TIME: Real = 0.5;
/// capped when WallStallTouchCount > 10
pub const WALL_STALL_TOUCH_CAP: Real = 10.0;
/// box trace centre at loc + CapsuleHalfHeight x 0.45
pub const WALL_SLIDE_TRACE_Z: Real = 0.45;
/// box trace length radius + 40
pub const WALL_SLIDE_TRACE_REACH: Real = 40.0;
/// box half height
pub const WALL_SLIDE_TRACE_HALF: Real = 60.0;
/// line traces radius + 30
pub const WALL_JUMP_TRACE_REACH: Real = 30.0;
pub const WALL_JUMP_PUSH: Real = 600.0;
/// HitNoWallSlideCD
pub const HIT_NO_WALL_SLIDE_CD: Real = 0.5;
// BaseFighter HandleHitShake defaults; HitWobbleCurve (Panda_Main/Blueprints/Curves/HitWobbleCurve) keys
// [time, value], all RCIM_Linear, constant extrapolation.
/// HitWobbleMultiplier
pub const HIT_WOBBLE_MULT: Real = 30.0;
/// WobbleSpeed
pub const WOBBLE_SPEED: Real = 50.0;
pub const HIT_WOBBLE_CURVE: [(Real, Real); 4] =
    [(-0.0020746887, 0.9992769), (0.49765015, 0.9992769), (0.5003463, -0.0036678314), (1.0018482, -0.0036678314)];
// ItemActorParent CDO: the idle timer is ItemIdleLife - ItemFadeTime, then the blink timeline (ItemFadeTime).
pub const ITEM_IDLE_LIFE: Real = 15.0;
pub const ITEM_FADE_TIME: Real = 3.0;
// Movement sounds: the pawn's Comp_Actor_Audio_Container values (per character), posted by BaseFighter's
// SFX_* functions through Lib_Audio. SFX_Wall_Exit and SFX_Wall_Jump really are the crouch-exit and jump events on
// Shaggy. BaseFighter's own (SFX Wall Slide CDO value, SFX_FastFall constant) are below.
// BlastBoxPlayerExit HandleBlastBoxDeathFX: PF_Post_Event(mix_stop_all_character_sfx) on the fighter, then
// Comp_Actor_VO InterruptVO(10): ExecuteActionOnPlayingID(Stop, CurrentAkEventID, 10 ms, curve 4).
pub const SFX_STOP_ALL_CHARACTER: &str = "mix_stop_all_character_sfx";
pub const VO_INTERRUPT_FADE: Real = 0.01;
// BaseFighter Trace Offset 01 / 02 (Z) for the surface-sound traces; PF_Surface_Effect_Trace's line length and sphere.
pub const TRACE_OFFSET_01: Real = 150.0;
pub const TRACE_OFFSET_02: Real = 125.0;
pub const SURFACE_LINE_LENGTH: Real = 200.0;
pub const SURFACE_SPHERE_RADIUS: Real = 150.0;
pub const SFX_WALL_SLIDE: &str = "sfx_shared_foley_medium_gen_wall_slide";
pub const SFX_FAST_FALL: &str = "sfx_shared_combat_fastfall";
// ClientOnlyAudioCueAirAttackCD (ubergraph 140832): PF_Post_Event on the fighter, locally controlled pawns only
// (every pawn offline).
pub const SFX_AIR_ATTACK_CD: &str = "ui_ability_cooldown_alt";
// BPI_ArmorDodgeCooldown (ubergraph @108020-108054): PF_Post_Event on the fighter, locally controlled pawns only
// (every pawn offline).
pub const SFX_ARMOR_COOLDOWN: &str = "sfx_shared_combat_armor_cooldown";
// Comp_Actor_Audio_Container class defaults that Pawn_Shaggy keeps.
pub const SFX_EVADE: &str = "sfx_shared_combat_evade";
pub const SFX_RUN_TRAIL: &str = "sfx_shared_combat_runtrail";
pub const SFX_BODY_FALL: &str = "sfx_shared_foley_medium_bodyfall";
pub const SFX_WALL_BOUNCE: &str = "sfx_shared_foley_medium_wallbounce";
pub const SFX_FALL_WIND: &str = "sfx_shared_gen_fallwind";
/// WallFatigueFX
pub const SFX_WALL_FATIGUE: &str = "ui_ability_wallfatigue";
/// Comp_Pawn_AbilityManager UpdateAbilityAmmo
pub const SFX_ABILITY_AMMO: &str = "ui_ability_ammo";
// Comp_Pawn_AbilityManager class defaults "SFX Cooldown 0", "1" and "2" (a failed TriggerAbilityCheck).
pub const SFX_ABILITY_COOLDOWN: &str = "ui_ability_cooldown";
// BaseFighter ReceiveTick: RTPC cha_wind_rush = MapRangeClamped(Velocity.Z x DT, 0, -53, 1, 0), interpolation
// chaWindRush_Interp (no CDO value: 0 ms). The wind stops with ExecuteActionOnPlayingID(Stop, 10 ms, linear).
pub const WIND_RUSH_RANGE: Real = -53.0;
pub const WIND_STOP_FADE: Real = 0.01;
/// SFX_Wall_Exit: ExecuteActionOnPlayingID(Stop, WallSlideID, 100 ms)
pub const WALL_SLIDE_STOP_FADE: Real = 0.1;

/// SOLID_*: a stage solid's flags.
pub const SOLID_FLOOR: i32 = 1;
pub const SOLID_WALL: i32 = 2;
pub const SOLID_ONE_WAY: i32 = 4;
pub const SOLID_TRACE: i32 = 8;
pub const SOLID_NAV: i32 = 16;
pub const SOLID_ITEM: i32 = 32;
pub const SOLID_DETECT: i32 = 64;
pub const SOLID_WALLJUMP: i32 = 128;
pub const SOLID_EXPLODE: i32 = 256;
pub const SOLID_NOPLATFORM: i32 = 512;
pub const SOLID_STEP: i32 = 1024;
