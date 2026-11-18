//! Being hit: hit pause, hitstun, the launch, bounces, the hit reaction's anim inputs and the hurt shapes
//! (the fighter's fields, written by the hit, launch and knockback code).

#![allow(clippy::neg_cmp_op_on_partial_ord, reason = "the negated tests are kept for their NaN result")]

pub mod hurt;
pub mod knockback;
pub mod launch;

pub use hurt::{box_hurt_hit, hurt_hit, hurt_rank, hurt_touched};
pub use knockback::{
    KbStunTimesOut, calculate_di, compute_knock, cumulative_di, damage_tier, di_angle, hitpause_scale, kb_stun_times,
    knockback_dir, knockback_distance, knockback_friction_stun, knockback_to_hitstun, timer_count, timer_ticks,
    vertical_launch_mod,
};
pub use launch::{
    ApplyKnockbackL, BOUNCE_NO_HITSTUN_MIN_SPEED, FloorBounceCapsuleOut, apply_knockback, bounce_effects,
    bounce_velocity, bpi_set_wall_bounce_vertical_min, floor_bounce_capsule, free_launch_mode, hitstun_ended,
    hitstun_handler_tick, hitstun_landed, hitstun_remaining, hitstun_timer_tick, is_bouncing, phys_knockback,
    set_true_hitstun, step_hitstun, tick_true_hitstun, true_hitstun_remaining, try_bounce,
};

use crate::Real;
use crate::State;
use crate::actors::ActorRef;
use crate::combat::DataEnum;
use crate::combat::buffs::buff_signal;
use crate::combat::decay::DecayHit;
use crate::combat::defense::absorb_temp_health;
use crate::combat::holds::{buff_stun_end, hold_knocked_back, struggle_ui};
use crate::consts::{
    ASSIST_WINDOW, ATTACK_DECAY_VALUES, COMBO_RES_DECAY_MOD, COMBO_RES_MOVE_COUNT, DELTA_SECONDS, HIT_WOBBLE_CURVE,
    HIT_WOBBLE_MULT, OFF_LEDGE_TRACE_LENGTH, WOBBLE_SPEED,
};
use crate::events::{Event, NotifyRef};
use crate::fighter::anim::{
    anim_direction, set_hit_react_info, stop_dash_montage, stop_grouped, stop_montage, update_pose,
};
use crate::fighter::common::{char_data, peers};
use crate::fighter::match_side::{FfaDamage, end_move_buff, ground_trace};
use crate::fighter::moves::{cancel_jump, drop_impulses, lateral_friction_event, set_terminal, update_air_control};
use crate::fighter::{Fighter, FighterId, actor_y, in_hitstun, most_recent_assist_attacker, signal_bound};
use crate::math::ue;
use crate::math::{max, round, to_int32};
use crate::names::Name;
use crate::registry::signals::{BuffSignal, BuffSignalArg, FighterSignal};
use crate::world::World;
use crate::world::flow::{DelegateCall, DelegateList, PawnList, broadcast_bound, update_ffa_team_damage};
use indexmap::{IndexMap, IndexSet};
use mvs_data::{CharDef, FighterData};

/// `ActiveHit`: a hit box notify that is on.
#[derive(State, Debug)]
pub struct ActiveHit {
    pub n: NotifyRef,
    pub idx: i32,
    /// The victims it hit, by fighter id.
    pub hit_set: IndexSet<i32>,
    /// TotalChargedTime read at NotifyBegin.
    pub charged_time: Real,
    /// AttackID (the attacker's attackCount) at NotifyBegin.
    pub attack_id: i32,
    /// HitBox.AttackAnimation (the montage) object name, for combo decay.
    pub attack_name: Name,
    pub open_pose: Option<(Name, Real)>,
    pub copy_pose: Option<(Name, Real)>,
    /// The packet a notify handler gave it (`h.n = { ...h.n, p }`: a sweet spot's, a conditional hitframe's, its
    /// HitBoxTags cut down by `hitBoxesOff`); `None`: its notify's.
    pub p: Option<std::sync::Arc<mvs_data::value::Props>>,
}

/// `launch`.
#[derive(State, Debug, Default)]
pub struct Launch {
    pub vx: Real,
    pub vy: Real,
    pub di: bool,
    pub wall: bool,
    pub kbi: Real,
}

/// `hitPauseTimer`.
#[derive(State, Debug, Default)]
pub struct HitPauseTimer {
    pub sec: Real,
    pub tick: i32,
}

/// BPI-written `launchOnHit`.
#[derive(State, Debug, Default)]
pub struct LaunchOnHit {
    pub speed_clamp: bool,
    pub min: crate::fighter::moves::V2,
    pub max: crate::fighter::moves::V2,
    pub can_fast_fall: bool,
    pub dir_y: Real,
    pub dir_z: Real,
    /// `!!p.XYOverride`, `!!p.ZOverride`: the NetImpulse's overrides.
    pub xy: bool,
    pub z: bool,
}

/// `f.launchOnHit.xy` and `.z`: the bound launch's NetImpulse overrides; none
/// bound: `(false, false)`.
pub fn launch_on_hit_overrides(w: &World, f: FighterId) -> (bool, bool) {
    w.fighter(f).hit.launch_on_hit.as_ref().map_or((false, false), |l| (l.xy, l.z))
}

/// Hit pause, hitstun, true hitstun, the launch and bounces.
#[derive(State, Debug, Default)]
pub struct HitState {
    pub hitpause: Real,
    pub hitstun: Real,
    pub hitstun_paused: bool,
    pub hitstun_pending: bool,
    pub hitstun_rate: Real,
    pub hitstun_counted: Real,
    pub mesh_x: Real,
    pub armor_shake: Option<Real>,
    pub bouncing: bool,
    pub mesh_z: Real,
    pub bounce_z_offset: Real,
    pub bounce_type: i32,
    pub no_wall_bounce: bool,
    pub floor_bounce_hurt: bool,
    pub in_hit_pause_pose: bool,
    pub queued_hit_pause: Real,
    pub queued_hit_pause_sec: Real,
    pub hit_pause_timer: Option<HitPauseTimer>,
    pub launch: Option<Launch>,
    pub true_hitstun: Real,
    pub true_hitstun_rate: Real,
    pub true_hitstun_counted: Real,
    pub true_hitstun_set_tick: i32,
    pub true_hitstun_tick_at: i32,
    pub kb_mode: bool,
    pub can_bounce_no_hitstun: bool,
    pub free_launch: bool,
    pub terminal_v: Real,
    pub prev_bounce_time: Real,
    pub rapid_bounce_count: i32,
    pub base_vx: Real,
    pub base_vy: Real,
    pub add_kb_vx: Real,
    pub add_kb_vy: Real,
    pub bounce_fx_until: Real,
    pub wind_on: bool,
    pub hitstun_end_time: Real,
    pub last_hitstun_time: Real,
    pub launch_on_hit: Option<LaunchOnHit>,
    pub wall_bounce_force_vert_min: Option<bool>,
    pub wall_bounce_forced_vert_min_amount: Option<Real>,
    /// BPI-written: the anim's bounce value.
    pub anim_bounce: Option<Real>,
    /// The hurt shapes without overlap events, by shape index.
    pub hurt_off: IndexSet<i32>,
}

/// The hit reaction's anim inputs.
#[derive(State, Debug, Default)]
pub struct HitReact {
    pub hit_neutral: bool,
    pub hit_react_int: i32,
    pub kb_facing: bool,
    pub hr_branch: Name,
    pub hr_intro: bool,
    pub hr_times: IndexMap<Name, crate::Real>,
    pub hit_pause_time: Real,
    pub hit_stun_kb_vx: Real,
    pub hit_stun_kb_vy: Real,
    pub direction_soft: Real,
    pub knock_back_angle: Real,
    pub fall_lean: Real,
    pub bounce_pitch: Real,
    /// `wobbleTimes`: front, back.
    pub wobble_times: (crate::Real, crate::Real),
    /// BPI-written: HitRotationOff.
    pub hit_rotation_off: Option<bool>,
}

/// Comp_Actor_Damage's CurrentDamage back at the StartingDamage (`f.damage = Math.fround(f.startingDamage)`): a new
/// pawn's ApplyDamage (`World` spawn and reset) and ResetFighterLogic (a respawn). Every write of
/// `Fighter::stats.damage` is combat's: this, the adds of a hit, a launch and a direct damage,
/// and the heal (`applyHealing`, Shaggy's sandwich among its callers).
pub fn reset_damage(f: &mut crate::fighter::Fighter) {
    f.stats.damage = f.stats.starting_damage;
}

/// `HitParams`: Calculate Knock Back's inputs.
#[derive(Clone, Copy, PartialEq, Debug, Default)]
pub struct HitParams {
    /// Already charge-multiplied.
    pub damage: Real,
    pub base_knock_back: Real,
    pub knock_back_scaling: Real,
    pub scaling_start_dmg: Real,
    pub hit_pause_time: Real,
    pub hit_stun_mult: Real,
    pub charge_kb_mult: Real,
    /// AnimNotify_Hitframe ChargeHitPauseMult.
    pub charge_hit_pause_mult: Real,
    /// Comp_Pawn_ChargeAttack TotalChargedTime, read at the Hitframe's NotifyBegin.
    pub total_charged_time: Real,
    /// The victim's Comp_Actor_Damage Weight.
    pub weight: Real,
}

/// What Calculate Knock Back gives.
#[derive(Clone, Copy, PartialEq, Debug, Default)]
pub struct KnockResult {
    /// KnockbackDistance.
    pub dist: Real,
    /// dist x KnockbackMultiplier, before buff mods and the damage tier.
    pub speed: Real,
    /// Path A damage tier on KnockbackLaunchVelocity.
    pub tier: Real,
    /// HitPauseTime with the charge term.
    pub hit_pause_time: Real,
    pub hitpause_frames: i32,
    /// The scaled pause passed to SetHitPauseTime (float32).
    pub hitpause_sec: Real,
    /// Seconds.
    pub hitstun: Real,
    pub hitstun_frames: i32,
    /// KBFrictionHitStun: the True Hitstun Timer that keeps the knockback movement mode.
    pub kb_stun_frames: i32,
    pub kb_stun_sec: Real,
}

// ---- the fighter's reads the hit code takes (the Fighter's getters)

/// The character's CharDef.
pub(crate) fn def_of(w: &World, ch: Name) -> &CharDef {
    &char_data(w, ch).def
}

/// The character's FighterData.
pub(crate) fn fighter_data(w: &World, ch: Name) -> &FighterData {
    &char_data(w, ch).fighter
}

/// The `Fighter.grounded` setter: off the floor, no movement base.
pub(crate) fn set_grounded(x: &mut Fighter, v: bool) {
    x.body.on_floor = v;
    if !v {
        x.platforms.base = None;
    }
}

/// `Fighter.onFfaDamage`: BPI_UpdateFFATeamDamage where the match binds it.
pub(crate) fn ffa_damage(w: &mut World, f: FighterId, d: Real) {
    match w.fighter(f).life.on_ffa_damage {
        FfaDamage::None => {}
        FfaDamage::UpdateTeamDamage => update_ffa_team_damage(w, f, d),
    }
}

// ---- the hit's own code

/// AdditiveKBMode (UserDefinedEnum; the NewEnumeratorN names carry their own values, read from the enum asset's
/// Names): 0 Normal, 1 AlwaysAdditive, 2 NeverAdditive.
fn additive_kb_mode(w: &World, v: Option<DataEnum>) -> i32 {
    match v {
        None => 0,
        Some(DataEnum::Value(n)) => n,
        Some(DataEnum::Name(n)) => {
            let s = w.names.str(n);
            let d = s.len() - s.trim_end_matches(|c: char| c.is_ascii_digit()).len();
            if d == 0 { 0 } else { s[s.len() - d..].parse::<i32>().unwrap_or(0) }
        }
    }
}

/// Comp_Pawn_DamageManager RecordDamage, called by Comp_Actor_Damage OnTakeAnyDamage with the amount it adds to
/// CurrentDamage and ApplyDamage's DamageCauser (the hit's DamageDealer): a positive amount from a fighter, or from
/// an actor whose Instigator is one, goes to that fighter's DamageDealt (BPI_ChangeDamageDealt). No team or self
/// check. A dealer with no PlayerState (a minion) pops at @285, a NotPlayerCharacter target at @534: no record either
/// way; the FFA team-damage call @549 still runs (the callers' `ffa_damage`). Then the fighter's OnTakeAnyDamage
/// reaches the actors bound to it, after Comp_Actor_Damage's handler (FighterSignal::TakeAnyDamage; an ApplyDamage
/// of 0 broadcasts nothing).
pub fn record_damage(w: &mut World, f: FighterId, damage: Real, dealer: Option<FighterId>) {
    if let Some(d) = dealer
        && damage > 0.0
        && !w.fighter(d).life.is_minion
        && !w.fighter(f).life.not_player_character
    {
        w.fighter_mut(d).stats.damage_dealt += damage;
    }
    if damage != 0.0 {
        signal_bound(w, f, FighterSignal::TakeAnyDamage, false, damage, Name::NONE, dealer);
    }
}

/// A direct GameplayStatics ApplyDamage(f, amount, ..., causer) on a fighter, outside DamageAndLaunch (no hit, no
/// hitpause, no knockback): ApplyDamage stops only at amount 0, then Comp_Actor_Damage OnTakeAnyDamage (@546-910):
/// Comp_Armor CalculateTempHealthDamage's share first (`absorb_temp_health`), CurrentDamage += the rest (float32),
/// RecordDamage(the rest, causer), then BPI_UpdateFFATeamDamage (@549, reached with no instigator too). The
/// DamageType is read by none of these. The caller does its own tests (IsInvulnerable?, the team). Returns what
/// CurrentDamage gained.
// Unverified: APawn::TakeDamage's ShouldTakeDamage 142c7df20 (its tests are not decoded) is left out, as in every
// other ApplyDamage of the port: a fighter is taken to accept the damage.
pub fn apply_direct_damage(w: &mut World, f: FighterId, amount: Real, dealer: Option<FighterId>) -> Real {
    if amount == 0.0 {
        return 0.0;
    }
    let taken = absorb_temp_health(w, f, amount);
    let x = w.fighter_mut(f);
    x.stats.damage += taken;
    record_damage(w, f, taken, dealer);
    ffa_damage(w, f, taken);
    taken
}

/// DamageAndLaunch light path (BaseKnockBack <= 0 and HitStunMult <= 0, e.g. armor): damage and hitpause only; the
/// current action carries on. Comp_Actor_Damage @772-818: CurrentDamage = IncomingDamage + CurrentDamage, no cap
/// (the only clamp is the hit's own 0..10000, DamageAndLaunch @2499), the sum rounded to float32 at the write.
/// `hitpause_sec`: the default is `hitpause_frames / 60`.
pub fn receive_light_hit(w: &mut World, f: FighterId, damage: Real, hitpause_frames: Real, hitpause_sec: Real) {
    let x = w.fighter_mut(f);
    x.stats.damage += damage;
    take_hit_pause(w, f, hitpause_frames, hitpause_sec);
}

/// DamageAndLaunch @4687-4974, the light path and the launch path alike: with LocalScalingHitPause > 0,
/// Comp_HitPause SetHitPauseTime only when GetHitPauseTime (the HitPauseTimer's remaining time, 0 without one) is
/// below it; a pause of 0 leaves a running one alone (@10775). So a shorter follow-up hit neither cuts the pause nor
/// broadcasts OnHitPauseTimeChanged. SetHitPauseTime also drops a queued pause (ubergraph 1194).
pub fn take_hit_pause(w: &mut World, f: FighterId, frames: Real, sec: Real) {
    if !(sec > 0.0) || !(hit_pause_remaining(w, f) < sec) {
        return;
    }
    hit_pause_time_changed(w, f, sec);
    let x = w.fighter_mut(f);
    x.hit.hitpause = frames;
    x.hit.queued_hit_pause = 0.0;
}

/// Comp_HitPause GetHitPauseTime: K2_GetTimerRemainingTimeHandle(HitPauseTimer), 0 when the handle is invalid.
/// Remaining = ExpireTime - InternalTime: set in an actor tick, the timer starts at that frame's timer tick, so k
/// frames later k - 1 deltas have run (a pending timer keeps its whole rate).
pub fn hit_pause_remaining(w: &World, f: FighterId) -> Real {
    let x = w.fighter(f);
    let Some(t) = &x.hit.hit_pause_timer else { return 0.0 };
    if !(x.hit.hitpause > 0.0) {
        return 0.0;
    }
    t.sec - max(crate::real(x.clock.tick) - crate::real(t.tick) - 1.0, 0.0) * DELTA_SECONDS
}

/// DamageAndLaunch @816-890 and @909-1607, straight after RegisterHitFromFighter on every hit that passes
/// CanBeHitBy. With no PreviousAttacker: HitByMultipleAttackers = false; with another one: true; with the same one
/// it is left (so it stays set until the hitstun ends). Either way PreviousAttacker = the owner.
/// ForceAdditiveKnockback is set by an AlwaysAdditive (1) hit with (BaseKnockBack > 0 and KnockBackScaling > 0) or
/// HitStunMult > 0. HitStunnedBeforeHit = CheckHitstunActive, before this hit adds its own source.
/// ZeroOutVelocityForKnockback is set unless (HitStunnedBeforeHit and (HitByMultipleAttackers or
/// ForceAdditiveKnockback) and the mode is not NeverAdditive (2)); nothing clears it here.
/// `att`: LocalDmgDlrOwner (@764-890, @8521-8641): the dealer when it is a fighter, else its Instigator when that is
/// one, else the dealer actor itself (a level actor with no Instigator: `register_level_hit`).
pub fn note_attacker(w: &mut World, f: FighterId, att: ActorRef, h: &DecayHit) {
    let mode = additive_kb_mode(w, h.additive_kb_mode);
    let x = w.fighter_mut(f);
    match x.attackers.previous_attacker {
        None => x.attackers.hit_by_multiple_attackers = false,
        Some(p) if p != att => x.attackers.hit_by_multiple_attackers = true,
        Some(_) => {}
    }
    x.attackers.previous_attacker = Some(att);
    let bkb = h.launch_knock_back.unwrap_or(h.base_knock_back);
    if mode == 1 && ((bkb > 0.0 && h.knock_back_scaling.unwrap_or(0.0) > 0.0) || h.hit_stun_mult.unwrap_or(0.0) > 0.0) {
        x.attackers.force_additive_kb = true;
    }
    let stunned = in_hitstun(w, f);
    let x = w.fighter_mut(f);
    x.attackers.hit_stunned_before_hit = stunned;
    let a = &x.attackers;
    if !(a.hit_stunned_before_hit && (a.hit_by_multiple_attackers || a.force_additive_kb) && mode != 2) {
        x.attackers.zero_out_vel_for_kb = true;
    }
}

/// DamageManager SetMostRecentAttacker(attacker, FriendlyFireAllowed false): see `register_hit_from`. Also the
/// state manager's call at a buff stun's start, with the stun buff's Instigator (ApplyStunBuffs @680).
pub fn set_most_recent_attacker(w: &mut World, f: FighterId, att: FighterId) {
    if w.fighter(att).team.team == w.fighter(f).team.team {
        return;
    }
    let x = w.fighter_mut(f);
    x.attackers.assist_attacker =
        if Some(att) != x.attackers.most_recent_attacker { x.attackers.most_recent_attacker } else { None };
    x.attackers.assist_clear_tick = assist_clear_tick(x.clock.tick);
    x.attackers.most_recent_attacker = Some(att);
}

/// The tick the AssistWindowTimer set at `tick` ends on.
fn assist_clear_tick(tick: i32) -> i32 {
    to_int32(crate::real(tick) + round(ASSIST_WINDOW * 60.0))
}

/// The off-ledge trace under MostRecentAttacker's pawn (@5650-6077), at every hit.
// Unverified: the attacker of a projectile's hit is traced where its fighter stands now, also while that fighter
// waits for its respawn (the build needs the controller's pawn to be valid; not read for that case).
fn trace_attacker_off_ledge(w: &mut World, f: FighterId) {
    if let Some(mra) = w.fighter(f).attackers.most_recent_attacker {
        let (mx, my) = (w.fighter(mra).body.x, actor_y(w, mra));
        let on = ground_trace(w, mra, mx, my, false, OFF_LEDGE_TRACE_LENGTH);
        w.fighter_mut(f).attackers.last_attacker_off_ledge = !on;
    }
}

/// DamageAndLaunch's RegisterHitFromFighter part for a hit whose dealer is a level actor with no Instigator (The
/// Court's hoop: its own Comp_Pawn_HitBoxHandler.pfgDoDamage(null, the fighter, ...)). The decay part does nothing
/// (GetAttackDecayFromHitbox finds neither a fighter nor a spawned actor); PreviousAttacker is the dealer
/// (LocalDmgDlrOwner @8641). SetMostRecentAttacker(the dealer, false) behind the same BaseKnockBack gate: its team
/// test passes (the dealer's Comp_Actor_Destructible TeamIndex -1, @748-1290), the Pawn cast fails (@185 -> @518), so
/// MostRecentAttacker and the assist stay; the AssistWindowTimer is cleared and, with an assist attacker, set again
/// (@591-716). Not ported: MostRecentAttackerActor = the dealer (@518) and the attack contexts (@537-564), which
/// nothing of the port reads. Then the off-ledge trace under MostRecentAttacker, as for any hit.
pub fn register_level_hit(w: &mut World, f: FighterId, dealer: ActorRef, h: &DecayHit) {
    note_attacker(w, f, dealer, h);
    let gate = h.launch_knock_back.unwrap_or(h.base_knock_back);
    let assist = most_recent_assist_attacker(w, f).is_some();
    let x = w.fighter_mut(f);
    if gate > 0.0 && assist {
        x.attackers.assist_clear_tick = assist_clear_tick(x.clock.tick);
    }
    trace_attacker_off_ledge(w, f);
}

/// DamageManager RegisterHitFromFighter (before GetHitStunDecay). A bounce hit (AttackID -13) only counts for the
/// DI combo. Otherwise, for an AttackID the attacker has not landed (or AttackDecayType 6): the attacker records it;
/// a hit with BaseKnockBack counts its combo-resistance name, and the third count of one name (MoveCountThreshold)
/// turns combo resistance on; the DI combo counter goes up; then the decay name is pushed to the front of the
/// attacker's AttackDecayArray (types 0/1/4/6; 2/3 once per decay sequence; 5 never), trimmed to
/// len(AttackDecayValues).
/// `from_hitbox` false: a buff actor's own handler dealt the hit: the RegisterHitFromFighter call does nothing for it
/// (GetAttackDecayFromHitbox finds no fighter or spawned actor); the attacker's note, SetMostRecentAttacker and the
/// trace are taken as for any hit. `att`: LocalDmgDlrOwner (PreviousAttacker, SetMostRecentAttacker); `src`:
/// GetAttackDecayFromHitbox's fighter (@591-791: a spawned actor's GetOwner), whose decay lists the hit fills (default
/// `att`)
pub fn register_hit_from(w: &mut World, f: FighterId, att: FighterId, h: &DecayHit, from_hitbox: bool, src: FighterId) {
    note_attacker(w, f, ActorRef::Fighter(att), h);
    // DamageAndLaunch @5302..5569 (after the damage and the hitpause, on the light path too): SetMostRecentAttacker
    // only when LocalHitInformation.BaseKnockBack x ChargeKnockBackMultiplier > 0 (the hit's after CalculateArmor),
    // so a hit without base knockback changes neither the attacker nor the assist.
    let gate = h.launch_knock_back.unwrap_or(h.base_knock_back);
    if gate > 0.0 {
        set_most_recent_attacker(w, f, att);
    }
    trace_attacker_off_ledge(w, f);
    if !from_hitbox {
        return;
    }
    if h.attack_id == -13 {
        w.fighter_mut(f).attackers.di_combo += 1.0;
        return;
    }
    let s = w.fighter_mut(src);
    if s.decay.landed_attack_ids.contains(&h.attack_id) && h.decay_type != 6 {
        return;
    }
    s.decay.landed_attack_ids.insert(h.attack_id);
    // HitBoxDamage.BaseKnockBack > 0 (@1007) on the hit DamageAndLaunch passes at @732, after CalculateArmor wrote
    // KnockBackOut into it (@7758 -> 479): a hit whose knockback an armor pack takes whole counts no name. `gate`
    // has that sign (the charge multiplier is positive).
    let x = w.fighter_mut(f);
    if gate > 0.0 {
        let n = x.decay.combo_res_map.get(&h.combo_name).copied().unwrap_or(0.0) + 1.0;
        x.decay.combo_res_map.insert(h.combo_name, n);
        if n >= COMBO_RES_MOVE_COUNT {
            x.decay.combo_resistance = true;
        }
    }
    x.attackers.di_combo += 1.0;
    if h.decay_type == 5 {
        return;
    }
    let s = w.fighter_mut(src);
    if h.decay_type == 2 || h.decay_type == 3 {
        let seq = s.decay.current_decay_sequence;
        if s.decay.landed_decay_sequences.contains(&seq) {
            return;
        }
        s.decay.landed_decay_sequences.push(seq);
    }
    s.decay.attack_decay.insert(0, h.decay_name);
    if s.decay.attack_decay.len() > ATTACK_DECAY_VALUES.len() {
        s.decay.attack_decay.remove(ATTACK_DECAY_VALUES.len());
    }
    // The pawn's BPI_InsertAttackDecay override, behind the parent's.
    if let Some(hk) = w.fighter(src).hooks.char.attack_decay_inserted {
        hk(w, src);
    }
}

/// GetHitStunDecay. AttackDecayStacks output: combo resistance gives len(AttackDecayValues) (and SourceFighter =
/// DamageDealer), the disabled / type 4 / 5 return 0. `att` None: a dealer with no fighter behind it (a level actor),
/// always with `enabled` false.
pub fn hit_stun_decay(w: &mut World, f: FighterId, att: Option<FighterId>, h: &DecayHit, enabled: bool) -> Real {
    let x = w.fighter_mut(f);
    x.attackers.last_decay_stacks = 0.0;
    if x.decay.combo_resistance {
        x.attackers.last_decay_stacks = len_real(ATTACK_DECAY_VALUES.len());
        return COMBO_RES_DECAY_MOD;
    }
    let Some(att) = att else { return 1.0 };
    if !enabled || h.decay_type == 4 || h.decay_type == 5 {
        return 1.0;
    }
    let vals = ATTACK_DECAY_VALUES;
    let a = w.fighter(att);
    let mut arr = a.decay.attack_decay.clone();
    if !a.decay.landed_attack_ids.contains(&h.attack_id) {
        arr.insert(0, h.decay_name);
        if arr.len() > vals.len() {
            arr.remove(vals.len());
        }
    }
    let stacks = arr.iter().filter(|n| **n == h.decay_name).count().min(vals.len());
    let mut total = 0.0;
    for v in &vals[..stacks] {
        total += v;
    }
    // MaximumAttackDecay with every stack (@2753), NoAttackDecay for a total <= 0 (@2940), AnyAttackDecay for > 0
    // (@3127).
    if crate::fighter::match_side::tutorial_world(w, f) {
        use crate::fighter::match_side::{TutorialEvent as T, tutorial_event_of};
        if stacks == vals.len() {
            tutorial_event_of(w, f, T::MaximumAttackDecay);
        }
        tutorial_event_of(w, f, if total <= 0.0 { T::NoAttackDecay } else { T::AnyAttackDecay });
    }
    w.fighter_mut(f).attackers.last_decay_stacks = len_real(stacks);
    1.0 - total
}

/// A list length as a number.
fn len_real(n: usize) -> Real {
    crate::real(u32::try_from(n).unwrap_or(u32::MAX))
}

/// ResetComboResMap (HitstunHandler hitstun end).
pub fn reset_combo_res(w: &mut World, f: FighterId) {
    let x = w.fighter_mut(f);
    x.decay.combo_res_map.clear();
    x.decay.combo_resistance = false;
}

/// AddHitstunSource (HitstunHandler @0-204): with no source yet, one more and OnIsHitStunned(CheckHitstunActive).
pub fn add_stun_source(w: &mut World, f: FighterId) {
    let x = w.fighter_mut(f);
    if x.attackers.stun_source {
        return;
    }
    x.attackers.stun_source = true;
    stun_changed(w, f, true);
}

/// On Player Hit Stun Changed (PandaGameMode @0-305): stunned: K2_ClearAndInvalidateTimerHandle(
/// HitStunCountResetEvent); not stunned: ResetHitstunCount(player data, fighter) -> ubergraph 34198 -> 20768-20844:
/// K2_SetTimerDelegate(OnNextTickToResetHitstun, 0.01, no loop) into the same handle. The delegate is the same each
/// time, so a second set restarts the one timer, and its event (34240 -> 34203) resets the player data of the latest
/// ResetHitstunCount call. The timer is pending in the frame it is set in (`World::combo_reset`).
pub fn stun_changed(w: &mut World, f: FighterId, stunned: bool) {
    let id = w.fighter(f).id;
    let r = &mut w.combo_reset;
    if stunned {
        r.fighter = -1;
    } else {
        r.fighter = id;
        r.pending = true;
        r.left = crate::real(timer_count(0.01));
    }
}

/// HitstunHandler RemoveHitstunFromKnockBack (ubergraph 10382 -> 7427): RemoveHitstunSource; with a source still on
/// (CheckHitstunActive: the buff stun's) nothing more; else DamageManager.PreviousAttacker = null and the hitstun
/// end (@1132). The HitstunTimer is not cleared here; the stun that follows at once (AddBuffHitStun @166) clears it.
/// Callers: Buff_Charm's MulticastRemoveHitstun, Comp_Pawn_Portal's MulticastCatch.
pub fn remove_hitstun_from_knock_back(w: &mut World, f: FighterId) {
    let x = w.fighter_mut(f);
    if x.attackers.stun_source {
        x.attackers.stun_source = false;
        let stunned = x.moves.pull.is_some() || x.buffs.buff_stunned;
        stun_changed(w, f, stunned);
    }
    let x = w.fighter_mut(f);
    if x.buffs.buff_stunned {
        return;
    }
    x.attackers.previous_attacker = None;
    hitstun_ended(w, f);
}

/// HitstunHandler AddHitStunFromKnockbackMulticast(KnockBack, KnockBackVelocity, HitStunMult, HitStunDecayMod)
/// (ubergraph 7881 -> AddHitStunFromKnockBack -> 10387) as an actor calls it, outside DamageAndLaunch
/// (JakeHouse_Actor @8297, JakeStretch; BouncyBread, BouncePad, WW_RadialBlast): KnockBackVelocity = the velocity
/// (@10387, sim space), AddDecayedHitStun(KnockBack, HitStunMult, HitStunDecayMod) (@278), then
/// BPI_SetTerminalVelocity(false, VSize(KnockBackVelocity), true) (@509-559). AddDecayedHitStun as `receive_hit`
/// takes it (HitstunHandler @71-3703):
/// - no HitstunTimer and HitStun x DecayMod not above 0: RemoveHitstunFromKnockBack (the hitstun end);
/// - no HitstunTimer: the start (AddHitstunSource, SetCanImpulse(false), UpdateAirControl(0), HitStunStart);
/// - a running timer: replaced only by a longer one (HitStunStart, the struggle widget's hide);
/// - then LastHitStunTime, the True Hitstun Timer (KBFrictionHitStun, no DecayMod) and OnHitStunExtended.
///
/// No launch, pause, montage stop or KnockedBack broadcast: the caller makes its own.
pub fn add_hit_stun_from_knockback(w: &mut World, f: FighterId, kb: Real, vx: Real, vy: Real, mult: Real, decay: Real) {
    let x = w.fighter_mut(f);
    x.react.hit_stun_kb_vx = vx;
    x.react.hit_stun_kb_vy = vy;
    let sec = knockback_to_hitstun(kb, mult) * decay;
    let frames = crate::real(timer_count(sec));
    let fresh = x.hit.hitstun <= 0.0;
    if fresh && frames <= 0.0 {
        remove_hitstun_from_knock_back(w, f);
    }
    let start = if fresh { frames > 0.0 } else { sec > hitstun_remaining(w, f) };
    if start {
        // A start ends a buff stun first: the fresh start's BuffstunTimer clear and RemoveHitStunFromBuff
        // (@2454-2542), the replacement's (@984-1038); as receive_hit's hold_knocked_back -> buff_stun_end.
        buff_stun_end(w, f, false);
        if fresh {
            add_stun_source(w, f);
            drop_impulses(w, f);
        } else {
            struggle_ui(w, f, 0.0, false);
        }
        let h = &mut w.fighter_mut(f).hit;
        h.hitstun = frames;
        h.hitstun_rate = sec;
        h.hitstun_counted = 0.0;
        h.hitstun_paused = false;
        h.hitstun_pending = true;
        lateral_friction_event(w, f, false, false);
        if fresh {
            update_air_control(w, f, 0.0);
        }
        if let Some(hk) = w.fighter(f).hooks.char.hit_stun_start {
            hk(w, f); // HitStunStart's other bindings (CharHooks.hitStunStart)
        }
    }
    let x = w.fighter_mut(f);
    x.hit.last_hitstun_time = x.clock.time;
    let t = kb_stun_times(kb, mult);
    set_true_hitstun(w, f, t.kb_stun_frames, t.kb_stun_sec, 0.0);
    w.fighter_mut(f).attackers.hitstun_count += 1;
    set_terminal(w, f, false, ue::size2d([vx, vy]), true);
}

/// OnJuggleCheckImpl(Attackee = this, Damage, Attacker) (PandaGameMode @0-4033), from KnockedBackIncludeSelf
/// (DamageAndLaunch @10564: the owner, ..., DamageDealer, ..., LocalHitInformation.Damage, which @8777 has set back to
/// LocalUnmodifiedDamage: the hit's own Damage before the charge multiplier, armor, buff mods and decay). The
/// attacker's player data is that of the dealer when it is a pawn, else of its Instigator. Its LastPlayerKnockedBack
/// valid and this fighter (@572-1022): IncJuggleCount(Damage) (@1315); else SetLastPlayerKnockedBack,
/// ResetJuggleCount, IncJuggleCount (@3190-3829). Then this fighter's IncHitstunDamage(Damage) and
/// OnPlayerJuggled(Attackee, JuggleCount, FTrunc(TotalJuggleDamage), attacker, HitstunComboCount,
/// FTrunc(TotalHitstunComboDamage)) (@1469-2084). The Quest_Weekly_Juggle count at 3 (@2529-2856) is not ported.
// Unverified: IsValid(LastPlayerKnockedBack) is taken as true once set (the pawn is taken to outlive its ring-outs);
// a dealer whose player data is not found (no such dealer here) would end the function at @279.
pub fn juggle_check(w: &mut World, f: FighterId, att: FighterId, damage: Real) {
    let d = damage;
    let fid = w.fighter(f).id;
    let a = w.fighter_mut(att);
    if a.attackers.last_knocked_back != crate::real(fid) {
        a.attackers.last_knocked_back = crate::real(fid);
        a.attackers.juggle_count = 0;
        a.attackers.juggle_damage = 0.0;
    }
    a.attackers.juggle_count += 1;
    a.attackers.juggle_damage += d;
    let (aid, jc, jd) = (a.id, a.attackers.juggle_count, a.attackers.juggle_damage);
    let x = w.fighter_mut(f);
    x.attackers.hitstun_damage += d;
    let ev = Event::ComboCounts {
        fighter: fid,
        juggler: aid,
        juggle_count: jc,
        juggle_damage: crate::real(ue::trunc_to_int(jd)),
        hitstun_count: x.attackers.hitstun_count,
        hitstun_damage: crate::real(ue::trunc_to_int(x.attackers.hitstun_damage)),
    };
    x.events.push(ev);
}

/// DamageAndLaunch's KnockedBackIncludeSelf (@10564, right after KnockedBack @10471) for the run-time binders: the
/// direction (sim axes here) as a Vector2D, X = world Y, Y = world Z. Called after the KnockedBack handlers
/// (`receive_hit`, `try_bounce`).
// Unverified: PandaGameMode's OnJuggleCheck (bound at the spawn, ahead of any run-time binder) is the juggle counter
// (`juggle_check`), run where the port runs it; the order against these binders is not kept.
pub fn knocked_back_include_self(
    w: &mut World,
    f: FighterId,
    base_kb: Real,
    dx: Real,
    dy: Real,
    dealer: Option<ActorRef>,
    damage: Real,
) {
    let call = DelegateCall::KnockedBackIncludeSelf { base_knock_back: base_kb, dir_x: -dx, dir_y: dy, dealer, damage };
    broadcast_bound(w, DelegateList::Pawn(f, PawnList::KnockedBackIncludeSelf), &call);
}

/// OnMovementModeChangedImpl (PandaGameMode @0-939): a character that goes from Falling (3) to Walking (1): every
/// player whose LastPlayerKnockedBack is that character gets ResetJuggleCount (the pawn reference stays). A landing
/// out of the knockback movement mode (custom, 5) is not one.
pub fn juggle_landed(w: &mut World, f: FighterId) {
    let fid = crate::real(w.fighter(f).id);
    for i in 0..peers(w).len() {
        let p = peers(w)[i];
        let a = &mut w.fighter_mut(p).attackers;
        if a.last_knocked_back != fid {
            continue;
        }
        a.juggle_count = 0;
        a.juggle_damage = 0.0;
    }
}

/// HitReactionMulticast's zero test (Comp_Pawn_DamageManager ubergraph @2025-2456, three EqualEqual_FloatFloat, exact
/// float compares, and a VSize): (KnockBackDistance == 0 && HitStunMult == 0) || (KnockBackDistance == 0 &&
/// HitPauseTime == 0) || (VSize(KnockBackVelocity) == 0 && HitStunMult == 0) skips @3731-4522. The arguments are
/// DamageAndLaunch's @9984: Calculate Knock Back's KnockbackDistance, KnockbackLaunchVelocity,
/// LocalHitInformation.HitStunMult (after CalculateArmor and ApplyBuffDamageMods) and LocalHitInformation.HitPauseTime.
pub fn hit_reaction_zero(dist: Real, vx: Real, vy: Real, hit_stun_mult: Real, hit_pause_time: Real) -> bool {
    let d0 = dist == 0.0;
    let h0 = hit_stun_mult == 0.0;
    let p0 = hit_pause_time == 0.0;
    let v0 = ue::size2d([vx, vy]) == 0.0;
    (d0 && h0) || (d0 && p0) || (v0 && h0)
}

/// `receiveHit`: a hit with knockback taken. `dir_x` / `dir_y`: Calculate Knock Back's KnockbackDirection (the hit's
/// KnockBackDirectionVector as the dealer aims it, or the vacuum direction), for the AnimBP's KnockBackAngle.
/// `hitstun_sec`: the HitstunTimer's new duration in float seconds (ConvertKnockBackToHitstun's HitStun x
/// HitStunDecayMod); `hitstun_frames` is the timer ticks it takes.
pub fn receive_hit(w: &mut World, f: FighterId, o: &ReceiveHitO) {
    // Comp_Actor_Damage @772-818: no cap; the amount (ApplyDamage's float) and the sum are float32.
    let x = w.fighter_mut(f);
    x.stats.damage += o.damage;
    x.attackers.knocked_back_count += 1;
    let buff_stunned = x.buffs.buff_stunned;
    // HitReactionMulticast's zero test (@2025-2456, `hit_reaction_zero`): a hit it takes (`zero`) pops at @2470 to
    // the flows pushed at @2000-2020 (the damage overlay, the hit VFX and camera shake, the charge sounds,
    // PlayHitSound), which every hit runs after @3731's steps; it skips all of @3731-4522: the binding,
    // AddHitStunFromKnockBack (the KnockBackVelocity store, AddDecayedHitStun with its hitstun start or end and the
    // buff stun's end, the terminal velocity), Montage_Stop, RemoveQueuedHitPause and the AnimNotifyMove root motion
    // removal. The rest of DamageAndLaunch is not in the multicast and runs for it too: the damage, the KnockedBack
    // broadcasts (@10471, @10564), HitDispatchMulticast (@10701), the pause (@4974), HitByEnemy (@3723) and the
    // launch at the pause's end.
    let zero = hit_reaction_zero(o.kb_dist, o.vx, o.vy, o.hit_stun_mult, o.hit_pause_time);
    // HitReactionMulticast past its zero test (@2456 -> @3731): the DamageManager's HitStunEnded bound to the
    // HitstunHandler's delegate (@3912 / @3935, an AddUnique) ahead of AddHitStunFromKnockBack (@4047), so a hitstun
    // end inside it (the buff stun's RemoveHitStunFromBuff @2488, here in hold_knocked_back; a hit with no hitstun,
    // @3723) already runs the binding's StopAnimMontage (`hitstun_ended`).
    if !zero {
        x.attackers.hit_stun_ended_bound = true;
    }
    // The KnockedBack broadcast's arguments for the buffs bound to it: the DamageDealer, BaseKnockBack and the hit's
    // direction.
    // Deviation: hitDir is the hit's raw vector (the packet's direction as the caller turned it, unnormalised), not
    // Calculate Knock Back's KnockbackDirection output; no ported binder reads it.
    let dir_x = o.dir_x.unwrap_or(o.vx);
    let dir_y = o.dir_y.unwrap_or(o.vy);
    let kb_arg = match o.dealer {
        Some(dealer) if !x.buffs.buffs.is_empty() => {
            // `actorBy(dealer, att when the dealer is the attacker)`.
            let att = o.att.filter(|a| ActorRef::Fighter(*a) == dealer);
            Some(BuffSignalArg {
                by: Some(crate::fighter::common::actor_by(w, dealer, att)),
                base_kb: Some(o.base_kb.unwrap_or(0.0)),
                hit_dir: Some([dir_x, dir_y]),
                ..BuffSignalArg::default()
            })
        }
        _ => None,
    };
    // Unverified: the KnockedBack handlers' order (here and in `try_bounce`): the holds and buffs
    // (hold_knocked_back), Buff_MoveToPosition, then the pawn's classes' bindings (CharHooks.knockedBack, below),
    // though ProcessMulticastDelegate calls them in bind order and some of the hook's binders bind late (Buff_Tasty,
    // Buff_VelmaAttachToAlly @6622).
    hold_knocked_back(w, f, !zero && o.hitstun_frames > 0.0, kb_arg);
    end_move_buff(w, f); // Buff_MoveToPosition OnOwnerKnockedBack (DestroyOnKnockedBack, @683-968): K2_DestroyActor
    // AddDecayedHitStun with no HitstunTimer running and a HitStun that is not above 0 (HitStunMult 0; HitstunHandler
    // @71-101 -> 1869-2118 -> 3723): RemoveHitstunFromKnockBack at the hit and nothing of the hitstun start. The
    // hitstun end runs inside AddHitStunFromKnockBack (@4047), ahead of the Montage_Stop (@4306),
    // RemoveQueuedHitPause (@4415), the KnockedBack broadcast (@10471), the pause (@4974) and AddDecayedHitStun's own
    // True Hitstun Timer (@1418) and OnHitStunExtended (@1840). The launch at the pause's end goes to a fighter in
    // control, as after a bounce out of hitstun (`free_launch`).
    let free = o.hitstun_frames <= 0.0 && w.fighter(f).hit.hitstun <= 0.0;
    // With the buff's source on at the hit, RemoveHitstunFromKnockBack stops there (HitstunHandler 7427): the
    // hitstun end is the buff stun's own (`hold_knocked_back`, or the buff's end later).
    if !zero && free && !buff_stunned {
        w.fighter_mut(f).attackers.previous_attacker = None;
        hitstun_ended(w, f);
    }
    // HitReactionMulticast Montage_Stop(0.0, null) (@4306) stops every instance that is not stopped yet with blend
    // 0; they are terminated one per frame from the next montage update. An instance already stopped is skipped (exe
    // FUN_1428412a0): the fading ones keep their blend-out time.
    if !zero {
        stop_montage(w, f, Some(0.0), None);
        stop_dash_montage(w, f, 0.0);
        stop_grouped(w, f, None, 0.0); // every instance of every group
    }
    // The move commands stay for the manager: FighterMovementComponent::InvalidateMoveQueue (ApplyKnockback
    // @3572-3673) is an empty native in this build (exec 141251ea0), so they complete at the manager's next tick (in
    // hitstun) and its end logic runs a tick later. A ControlSpeed command's Flying stays until that end logic
    // (Comp_Actor_MoveCommandManager @685-781), or until ApplyKnockback's launch when the pause ends first
    // (`step_hitstun`). A pending impulse is dropped: the HitstunHandler start turns FMC CanImpulse off, and the CMC
    // then clears the pending NetImpulse without launching.
    if !free && !zero {
        drop_impulses(w, f);
    }
    // Calculate Knock Back @266, ahead of the hit reaction; its launch is not a NetImpulse, so it comes after the
    // drop above.
    cancel_jump(w, f);
    if !zero {
        w.fighter_mut(f).hit.queued_hit_pause = 0.0; // RemoveQueuedHitPause (ubergraph 4415)
    }
    // The pause comes after the hit reaction: the launch path runs HitReactionMulticast (@9984) and
    // HitDispatchMulticast (@10701: SetHitReactInfo), then jumps (@10735) to @3589 and reaches SetHitPauseTime at
    // @4974, behind the longer-only guard. Between the two (@10471) the KnockedBack broadcast reaches Base_AnimBP
    // OnKnockedBack: KnockBackAngle = the direction of KnockbackDirection.
    // Deviation: a caller that does not pass the direction gives the launch's, which differs from it where the
    // launch is not along the hit's direction (a kept pending launch, a vacuum blend, per-axis buff mods).
    let angle = anim_direction(w, f, dir_x, dir_y);
    w.fighter_mut(f).react.knock_back_angle = angle;
    // The same broadcast's first argument is LocalHitInformation.BaseKnockBack (after CalculateArmor @7758); a pawn's
    // own handler bound to it (Pawn_C017 OnKnockBack @20500) reads it. Its second is KnockbackDirection, its fourth
    // the DamageDealer.
    let base_kb = o.base_kb.unwrap_or(0.0);
    let dealer = o.dealer.or(o.att.map(ActorRef::Fighter));
    if let Some(hk) = w.fighter(f).hooks.char.knocked_back {
        hk(w, f, base_kb, dir_x, dir_y, dealer);
    }
    knocked_back_include_self(w, f, base_kb, dir_x, dir_y, dealer, o.raw_damage.unwrap_or(o.damage));
    set_hit_react_info(w, f, o.vx, o.vy, !zero);
    take_hit_pause(w, f, o.hitpause_frames, o.hitpause_sec.unwrap_or(o.hitpause_frames / 60.0));
    // AddDecayedHitStun with the HitstunTimer existing (@101-444): the new duration (@115-275: HitStun x
    // HitStunDecayMod through ApplyOutsideHitstunClamp, whose clamp is 0 = none) replaces the timer only if it is
    // strictly above K2_GetTimerRemainingTimeHandle(HitstunTimer), in float seconds, or with CanOverideLastHit (set
    // by AddBuffHitStun @1193, which clears the HitstunTimer first, and cleared by every start here, so never true
    // with a timer). Replaced (@454-1398): the timers cleared and set again (K2_SetTimerDelegate @900: pending, not
    // paused), StopStunMontage, HitStunStart; no AddHitstunSource, SetIgnoreMoveInput, UpdateAirControl or
    // SetCanImpulse, which only the start without a timer has (@2556, @3384-3659). Not replaced (@444 pop): the timer
    // stays (hitstun_handler_tick pauses either for the hit's pause). All three go on to LastHitStunTime (@44), the
    // True Hitstun Timer block (@1418-1812) and OnHitStunExtended (@1840).
    let sec = o.hitstun_sec.unwrap_or(o.hitstun_frames / 60.0);
    let fresh = w.fighter(f).hit.hitstun <= 0.0;
    // A zero hit runs no AddDecayedHitStun (@4047 skipped): no start, no LastHitStunTime, True Hitstun Timer or
    // OnHitStunExtended.
    let start = !zero && if fresh { o.hitstun_frames > 0.0 } else { sec > hitstun_remaining(w, f) };
    if fresh && start {
        add_stun_source(w, f); // AddHitstunSource (@2556), only on the start without a timer
    }
    // Replaced, @570 EndEscapeServer(null, null) (authority): EscapeAvailable off and UpdateEscapeUI(false), the
    // struggle widget's hide.
    if !fresh && start {
        struggle_ui(w, f, 0.0, false);
    }
    if start {
        let h = &mut w.fighter_mut(f).hit;
        h.hitstun = o.hitstun_frames;
        h.hitstun_rate = sec;
        h.hitstun_counted = 0.0;
        h.hitstun_paused = false;
        h.hitstun_pending = true;
    }
    if !zero {
        let x = w.fighter_mut(f);
        x.hit.last_hitstun_time = x.clock.time;
        set_true_hitstun(w, f, o.kb_stun_frames, o.kb_stun_sec.unwrap_or(o.kb_stun_frames / 60.0), 0.0);
        // OnHitStunExtended (@1840, every AddDecayedHitStun) -> PandaGameMode OnPlayerHitStunExtended
        w.fighter_mut(f).attackers.hitstun_count += 1;
    }
    if let Some(att) = o.att {
        juggle_check(w, f, att, o.raw_damage.unwrap_or(o.damage)); // KnockedBackIncludeSelf (@10564)
    }
    let x = w.fighter_mut(f);
    x.hit.launch = Some(Launch { vx: o.vx, vy: o.vy, di: false, wall: false, kbi: o.kbi });
    // A zero hit leaves the hitstun as it was (no AddDecayedHitStun): its launch goes to a fighter in control when no
    // HitstunTimer runs, though the hit's computed hitstun is above 0 (KnockBackDistance 0 and HitPauseTime 0 with a
    // HitStunMult: ConvertKnockBackToHitstun's floor).
    x.hit.free_launch = free || (zero && x.hit.hitstun <= 0.0);
    hit_camera_shake(w, f, o.vx, o.vy);
    if !free && !zero {
        // HitStunStart (@1398 / @3703, not on a kept timer): AirControlGate and DashFallGate off; the start without a
        // timer (@3520): UpdateAirControl(0).
        if start {
            lateral_friction_event(w, f, false, false);
            if let Some(hk) = w.fighter(f).hooks.char.hit_stun_start {
                hk(w, f); // HitStunStart's other bindings (CharHooks.hitStunStart)
            }
        }
        if fresh {
            update_air_control(w, f, 0.0);
        }
    }
    w.fighter_mut(f).jump.jump_frame = -1;
    update_pose(w, f);
}

/// Attacker side (Comp_Pawn_HitBoxHandler, ApplyHitPauseToDamageDealer is always true): if the remaining pause is
/// under the hit's raw HitPauseTime, the scaled pause replaces it, or with QueueHitPauseUntilPose waits for the next
/// HitPausePoseFrames window unless already inside one. `hitpause_sec`: the default is `hitpause_frames / 60`.
pub fn on_dealt_hit(
    w: &mut World,
    f: FighterId,
    hitpause_frames: Real,
    raw_hit_pause: Real,
    queue_until_pose: bool,
    hitpause_sec: Real,
) {
    w.fighter_mut(f).montages.montage_hit = true;
    dealt_hit_pause(w, f, hitpause_frames, raw_hit_pause, queue_until_pose, hitpause_sec);
}

/// The pause alone (pfgDoDamage @16817-17442, @23160), also for a hit on an item, which lands no attack ID.
pub fn dealt_hit_pause(
    w: &mut World,
    f: FighterId,
    hitpause_frames: Real,
    raw_hit_pause: Real,
    queue_until_pose: bool,
    hitpause_sec: Real,
) {
    // GetHitPauseTime < HitPauseTime in seconds, as the victim side (take_hit_pause).
    if !(hit_pause_remaining(w, f) < raw_hit_pause) {
        return;
    }
    if w.fighter(f).hit.in_hit_pause_pose || !queue_until_pose {
        set_hit_pause(w, f, hitpause_frames, hitpause_sec);
    } else {
        let h = &mut w.fighter_mut(f).hit;
        h.queued_hit_pause = hitpause_frames;
        h.queued_hit_pause_sec = hitpause_sec;
    }
}

/// BaseFighter HandleHitShake (ReceiveTick): in hitstun with hitpause left and not bouncing, Mesh X =
/// HitWobbleMultiplier * HitWobbleCurve(cos(GameTimeSinceCreation * WobbleSpeed)); otherwise the offset is left as
/// it is. While bouncing, a non-zero BounceZOffset puts the mesh at (0, 0, BounceZOffset + MeshStartZLocation). It
/// reads the HitPauseTimer's remaining time (0 once the handle is invalid), so it runs after the timers due at this
/// tick. GetGameTimeSinceCreation is the float32 world clock less CreationTime.
pub fn handle_hit_shake(w: &mut World, f: FighterId) {
    if !(in_hitstun(w, f) && w.fighter(f).hit.hitpause > 0.0) {
        return;
    }
    let x = w.fighter_mut(f);
    if x.hit.bouncing {
        if x.hit.bounce_z_offset != 0.0 {
            x.hit.mesh_x = 0.0;
            x.hit.mesh_z = x.hit.bounce_z_offset;
        }
        return;
    }
    let since = x.clock.time - x.clock.creation_time; // GetGameTimeSinceCreation, float32
    x.hit.mesh_x = HIT_WOBBLE_MULT * eval_curve(&HIT_WOBBLE_CURVE, ue::cosf(since * WOBBLE_SPEED));
}

/// BFHitPauseEnd (ubergraph 143454): the mesh goes back to (0, 0, MeshStartZLocation) unless bIsCrouched (the
/// crouched mesh sits higher by the capsule shrink), and IsBouncing clears.
pub fn hit_pause_end(w: &mut World, f: FighterId) {
    let x = w.fighter_mut(f);
    if !x.capsule.cmc_crouched {
        x.hit.mesh_x = 0.0;
        x.hit.mesh_z = 0.0;
    }
    x.hit.bouncing = false;
    x.hit.anim_bounce = None; // the OnIsBouncing(0) broadcast @139762: Base_AnimBP's IsBouncing too
    x.react.bounce_pitch = 0.0; // @143426
    x.hit.bounce_z_offset = 0.0;
    x.hit.floor_bounce_hurt = false;
    // Comp_HitPause then broadcasts OnHitPauseEnded and OnHitPauseTimeChanged(0, 0) (@806, @825): the AnimBP's
    // binding is the write below; the buffs' (Buff_MoveToActor2 OnHitPause) after it.
    x.react.hit_pause_time = 0.0;
    if !x.buffs.buffs.is_empty() {
        buff_signal(
            w,
            f,
            BuffSignal::HitPauseTimeChanged,
            BuffSignalArg { duration: Some(0.0), ..BuffSignalArg::default() },
        );
    }
}

/// Comp_Pawn_DamageManager PlayCameraShake (HitReactionMulticast with PlayOnHitEffects): VeryLight when
/// ForceSmallHitVFX (vacuum knockback only, not ported), Large over 3000 unless NotPlayerCharacter, Medium over 800,
/// else Light. KnockbackVelocity is the launch before DI and the vertical mod. NotPlayerCharacter is false for a
/// player (the BaseFighter default; only Pawn_VelmaAIBotPolice and the NPC pawns Steven and Velma spawn set it, and
/// BPI_UpdateIsNotPlayerCharacter has no caller). Such a victim over 3000 takes Medium (@667-753).
pub fn hit_camera_shake(w: &mut World, f: FighterId, vx: Real, vy: Real) {
    let v = ue::size2d([vx, vy]);
    let cls = if v > 3000.0 && !w.fighter(f).life.not_player_character {
        crate::name!("CameraShakeLarge_C")
    } else if v > 800.0 {
        crate::name!("CameraShakeMedium_C")
    } else {
        crate::name!("CameraShakeLight_C")
    };
    w.fighter_mut(f).events.push(Event::CameraShake { cls });
}

/// Comp_HitPause SetHitPauseTime (old movement path) broadcasts OnHitPauseTimeChanged(HitPauseTime, AmountChanged)
/// before it restarts HitPauseTimer: AmountChanged = HitPauseTime - the running timer's remaining time, or
/// HitPauseTime when none runs. Comp_Pawn_BranchManager OwnerHitPauseChanged (HitPauseTime > 0) adds it to every
/// AttackSpamTimes stamp, so a pause pushes the spam window back by the pause added.
pub fn hit_pause_time_changed(w: &mut World, f: FighterId, sec: Real) {
    if !(sec > 0.0) {
        return;
    }
    // Base_AnimBP OnHitPauseTimeChanged (bound to Comp_HitPause's delegate, so in the same call): HitPauseTime, and
    // with HitPauseTime > 0 KB_Facing = HitStunManager.KnockBackVelocity.Y < 0 when FacingLeft, > 0 otherwise (the
    // knockback points away from the facing). On DamageAndLaunch's launch path HitReactionMulticast's
    // AddHitStunFromKnockBack (@9984; HitstunHandler ubergraph 10387) has already stored the new KnockBackVelocity
    // when the flow reaches SetHitPauseTime (@10735 -> 3589 -> 4974), so this reads the launch of the hit that sets
    // the pause; a hit whose pause is not set leaves KB_Facing as it was. The attacker's pause and a light hit read
    // the last launch the fighter took (zero before the first one).
    w.fighter_mut(f).react.hit_pause_time = sec;
    if let Some(hk) = w.fighter(f).hooks.char.hit_pause_time_changed {
        hk(w, f, sec); // the pawn's AnimBP's own binding (C023B_AnimBP AddPauseTime)
    }
    let x = w.fighter_mut(f);
    x.react.kb_facing =
        if x.body.facing_dir < 0.0 { x.react.hit_stun_kb_vx > 0.0 } else { x.react.hit_stun_kb_vx < 0.0 };
    // Then (@293-602) DirectionSoft = the direction of the same KnockBackVelocity, at once.
    let (kx, ky) = (x.react.hit_stun_kb_vx, x.react.hit_stun_kb_vy);
    let soft = anim_direction(w, f, kx, ky);
    w.fighter_mut(f).react.direction_soft = soft;
    let running = {
        let x = w.fighter(f);
        x.hit.hit_pause_timer.is_some() && x.hit.hitpause > 0.0
    };
    let amount = if running { sec - hit_pause_remaining(w, f) } else { sec };
    let x = w.fighter_mut(f);
    for v in x.controls.attack_spam_times.values_mut() {
        *v += amount;
    }
    x.hit.hit_pause_timer = Some(HitPauseTimer { sec, tick: x.clock.tick });
    // The buffs bound to it (Buff_MoveToActor2 OnHitPause), after the components'.
    // Unverified: the delegate's own order (bind order: the AnimBP and the branch manager at their BeginPlay, a buff
    // at its own) is taken as components first; no other ported binder reads the order.
    if !x.buffs.buffs.is_empty() {
        buff_signal(
            w,
            f,
            BuffSignal::HitPauseTimeChanged,
            BuffSignalArg { duration: Some(sec), ..BuffSignalArg::default() },
        );
    }
}

/// SpawnedActorParent BPI_DoesApplyHitpauseToOwner (ApplyHitPauseToOwner): the HitBoxHandler gives the actor's
/// owner's Comp_HitPause SetHitPauseTime(LocalScalingHitPause) when its GetHitPauseTime is under the hit's unscaled
/// HitPauseTime (pfgDoDamage 17586-18248).
pub fn owner_hit_pause(w: &mut World, f: FighterId, frames: Real, sec: Real, raw: Real) {
    if !(hit_pause_remaining(w, f) < raw) {
        return; // seconds
    }
    set_hit_pause(w, f, frames, sec);
}

/// SetHitPauseTime: replaces the running pause (min 0.01 s) and drops a queued one.
pub fn set_hit_pause(w: &mut World, f: FighterId, frames: Real, sec: Real) {
    hit_pause_time_changed(w, f, max(sec, 0.01));
    let h = &mut w.fighter_mut(f).hit;
    h.hitpause = max(frames, 1.0);
    h.queued_hit_pause = 0.0;
}

/// FRichCurve::Eval for linear keys with constant extrapolation.
fn eval_curve(keys: &[(Real, Real)], t: Real) -> Real {
    if t <= keys[0].0 {
        return keys[0].1;
    }
    for i in 1..keys.len() {
        let (t1, v1) = keys[i];
        if t < t1 {
            let (t0, v0) = keys[i - 1];
            return v0 + ((v1 - v0) * (t - t0)) / (t1 - t0);
        }
    }
    keys[keys.len() - 1].1
}

// ---- the call surface's argument types

/// What `receiveHit` takes (a struct literal).
#[derive(Clone, Debug)]
pub struct ReceiveHitO {
    pub damage: Real,
    pub vx: Real,
    pub vy: Real,
    pub hitpause_frames: Real,
    pub hitpause_sec: Option<Real>,
    pub hitstun_frames: Real,
    pub hitstun_sec: Option<Real>,
    pub kb_stun_frames: Real,
    pub kb_stun_sec: Option<Real>,
    pub kbi: Real,
    pub dir_x: Option<Real>,
    pub dir_y: Option<Real>,
    pub att: Option<FighterId>,
    pub dealer: Option<ActorRef>,
    pub raw_damage: Option<Real>,
    pub base_kb: Option<Real>,
    pub kb_dist: Real,
    pub hit_stun_mult: Real,
    pub hit_pause_time: Real,
}

/// The packet a hit on the fighter's list reads (`h.n.p`, ActiveHit), borrowed: the props a
/// notify handler gave it, else its notify's (`None` only for a notify the data does not have).
pub fn hit_packet<'a>(w: &'a World, f: FighterId, h: &'a ActiveHit) -> Option<&'a mvs_data::value::Props> {
    match &h.p {
        Some(p) => Some(p),
        None => crate::fighter::anim::any_notify(w, f, h.n).map(|n| &n.p),
    }
}

/// [`hit_packet`], as its own copy (empty for a notify the data does not have).
pub fn hit_props(w: &World, f: FighterId, h: &ActiveHit) -> mvs_data::value::Props {
    hit_packet(w, f, h).cloned().unwrap_or_default()
}

/// `h.n = { ...h.n, p }` of the hit at `key` in `activeHits` (sweetSpot, conditionalPacket,
/// UpdateHitframeDamage; hitBoxesOff): its packet from now on.
pub fn set_active_hit_props(w: &mut World, f: FighterId, key: i32, p: mvs_data::value::Props) {
    let p = std::sync::Arc::new(p);
    w.fighter_mut(f).montages.active_hit_write(key, |h| h.p = Some(p.clone()));
}

/// `sweet.hitSet = own.hitSet` (AnimNotify_SweetSpotHitframe): the hit at `to` takes the
/// victim set of the hit at `from` as its own, the one set both add to from then on.
// Deviation: the build's one set is two sets here, kept equal: `phases::hit_set_add` and the reset's clear
// write the partner key too (`sweet_partner`), so a victim added through either is in both.
pub fn share_hit_set(w: &mut World, f: FighterId, from: i32, to: i32) {
    let set = w.fighter(f).montages.active_hits.get(&from).map(|h| h.hit_set.clone()).unwrap_or_default();
    w.fighter_mut(f).montages.active_hit_write(to, |h| h.hit_set = set.clone());
}
