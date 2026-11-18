//! Knockback, hitstun and the bounce, and BaseFighter's HurtBoxFloorBounce capsule.

#![allow(clippy::neg_cmp_op_on_partial_ord, reason = "the negated tests are kept for their NaN result")]

use super::knockback::{calculate_di, compute_knock, knockback_dir, vertical_launch_mod};
use super::{
    HitParams, Launch, def_of, ffa_damage, fighter_data, hit_pause_time_changed, juggle_check, juggle_landed,
    knocked_back_include_self, record_damage, register_hit_from, reset_combo_res, set_grounded, stun_changed,
};
use crate::Real;
use crate::actors::ActorRef;
use crate::combat::buffs::{apply_knock_back_multipliers, apply_taken_damage_mods, buff_signal};
use crate::combat::decay::DecayHit;
use crate::combat::defense::absorb_temp_health;
use crate::combat::holds::hold_knocked_back;
use crate::combat::packet::HitPacket;
use crate::consts::{
    AIR_CONTROL, BOUNCE_FLOOR_MULT, BOUNCE_HITPAUSE_MAX, BOUNCE_HITPAUSE_MIN, BOUNCE_KB_CAP, BOUNCE_MIN_FLOOR_VEL,
    BOUNCE_MIN_SPEED_FLOOR, BOUNCE_MIN_SPEED_NOT_FLOOR, BOUNCE_RAPID_COUNT, BOUNCE_RAPID_TIME, BOUNCE_UP_MOD,
    BRAKING_FRICTION_FACTOR, DAMAGE_DECAY_MOD, DELTA_SECONDS, DI_COMBO_RESET_FRAMES, DOWNSPIKE_FRICTION, DT, GRAVITY_Z,
    KB_FRICTION, KB_MULT, SUBSTEPS, TRACE_OFFSET_02,
};
use crate::events::Event;
use crate::fighter::FighterId;
use crate::fighter::anim::{
    anim_direction, cond_signal, set_hit_react_info, stop_anim_montage, stop_dash_montage, stop_grouped, stop_montage,
};
use crate::fighter::body::{PlatformRef, floor_list};
use crate::fighter::match_side::{end_move_buff, surface_sfx};
use crate::fighter::moves::{
    Impulse, apply_impulse, bf_landed, bounce_mult, cap_fall, collide, di_mult, fall_move, fast_fall_landed, land,
    landed_on_solid_ground, launch_on_hit, ledge_step, max_accel, no_collision_move, phys_flying, reset_air,
    reset_wall_fatigue, resolve_resize, set_terminal, update_air_control, walk_delta,
};
use crate::fighter::tick::{FrameMode, controller_rumble, hit_rumble};
use crate::fighter::{cap_half, cap_radius, no_collision, weight};
use crate::math::ue;
use crate::math::{max, min, round, sign, to_int32};
use crate::registry::signals::{BuffSignal, BuffSignalArg, CondSignal};
use crate::world::World;
use mvs_collision::capsule::capsule_gap;
use mvs_data::value::{Props, Value};

/// ApplyKnockback's keep rule for one axis (LocalFinalVelY @2425-2826 / 5989-6162, LocalFinalVelZ @2897-3298 /
/// 6238-6411): with the same sign, or either value 0 (SignOfFloat), the launch replaces the fighter's velocity only
/// when its magnitude is larger; with opposite signs only when 0.75 x the launch is larger than it.
fn keep_knockback_axis(launch: Real, vel: Real) -> Real {
    let (a, b) = (sign(launch), sign(vel));
    let k = if a == 0.0 || b == 0.0 || a == b { launch } else { launch * 0.75 };
    if k.abs() > vel.abs() { launch } else { vel }
}

/// BaseFighter BounceWithoutHitStunMinSpeed.
pub const BOUNCE_NO_HITSTUN_MIN_SPEED: Real = 4000.0;

/// The HitstunTimer in the timer manager's tick (FTimerManager::Tick 142e45520, after the frame's tick groups, so
/// at the start of the next beginFrame, with the last frame's tick and clock). A timer set or unpaused during the
/// frame is started here without counting it; a paused one stands still. Fired: RemoveHitstunFromKnockBack
/// (HitstunHandler ubergraph 10382 -> 7427-7530): with no hitstun or buff stun source left,
/// DamageManager.PreviousAttacker = null, then the hitstun end, whose RemoveHitstunSource broadcast writes the
/// AnimBP's Is Hit Stunned: the frame's own updates (the move, the AnimBP, a hit taken) still ran in hitstun.
pub fn hitstun_timer_tick(w: &mut World, f: FighterId) {
    let x = w.fighter_mut(f);
    if x.hit.hitstun <= 0.0 || x.hit.hitstun_paused {
        return;
    }
    if x.hit.hitstun_pending {
        x.hit.hitstun_pending = false;
        return;
    }
    x.hit.hitstun_counted += 1.0;
    x.hit.hitstun -= 1.0;
    if x.hit.hitstun > 0.0 {
        return;
    }
    x.hit.launch = None;
    x.attackers.previous_attacker = None;
    hitstun_ended(w, f);
}

/// K2_GetTimerRemainingTimeHandle(HitstunTimer) (FTimerManager 142b26730): float(ExpireTime - InternalTime) for an
/// active timer, the stored remaining time for a paused one, the whole Rate for one still pending from its set:
/// all three are the Rate less the counted ticks.
pub fn hitstun_remaining(w: &World, f: FighterId) -> Real {
    let h = &w.fighter(f).hit;
    h.hitstun_rate - h.hitstun_counted * DELTA_SECONDS
}

/// Comp_Pawn_HitstunHandler ReceiveTick (ubergraph 9144 -> @9154-9447; TG_DuringPhysics, after the frame's hits
/// and the CMC): while the HitstunTimer exists and the owner's Comp_HitPause GetHitPauseTime > 0,
/// K2_PauseTimerHandle (@9366; a pending timer keeps its full time); on the first tick without a pause,
/// K2_UnPauseTimerHandle (@9427), which puts it on the pending list: that frame's timer tick starts it and the next
/// one is the first it counts. A pause of n frames taken with the timer running costs n + 2 timer ticks
/// (PauseTimer 142e3f700, UnPauseTimer 142e46bc0, FTimerManager::Tick 142e45520).
pub fn hitstun_handler_tick(w: &mut World, f: FighterId) {
    let x = w.fighter_mut(f);
    if x.hit.hitstun <= 0.0 {
        return;
    }
    if x.hit.hitpause > 0.0 || x.frame.frame_mode == FrameMode::Paused {
        x.hit.hitstun_paused = true;
        x.hit.hitstun_pending = false;
    } else if x.hit.hitstun_paused {
        x.hit.hitstun_paused = false;
        x.hit.hitstun_pending = true;
    }
}

/// What `applyKnockback` takes: the fighter's launch.
pub type ApplyKnockbackL = Launch;

/// Comp_Pawn_DamageManager ApplyKnockback, run by DMHitPauseEnd (ubergraph 11934 -> 11919) and by nothing else.
/// @299-370: with VSize(KnockbackLaunchVelocity) not > 0 it returns at once (no LaunchCharacter, no
/// BPI_SetBaseFighterVelocity, no SetMovementMode(6, 2), the flags not cleared). Returns false for that case.
/// `pending`: the launch is left for the frame's CMC tick to take (Fighter.beginFrame's free launch, ahead of the
/// controller); the hitstun step runs inside the CMC tick and sets the velocity at once.
pub fn apply_knockback(w: &mut World, f: FighterId, l: &mut ApplyKnockbackL, pending: bool) -> bool {
    l.di = true;
    if l.vx == 0.0 && l.vy == 0.0 {
        return false;
    }
    // The vertical mod, then CalculateDI (grounded = IsWalking).
    l.vy = vertical_launch_mod(l.vy);
    // @689 KnockbackVelBeforeKBI: the launch after the vertical mod, before CalculateDI.
    let (mut no_di_x, mut no_di_y) = (l.vx, l.vy);
    let dm = di_mult(w, f);
    let x = w.fighter(f);
    (l.vx, l.vy) = calculate_di(
        l.vx,
        l.vy,
        x.controls.inp.x,
        x.controls.inp.y,
        x.body.on_floor,
        l.wall,
        l.kbi,
        x.attackers.di_combo,
        dm,
    );
    // @1057-1193 and @2335-3298: LocalFighterVel is 0 with ZeroOutVelocityForKnockback, else GetVelocity(); with
    // HitStunnedBeforeHit and (HitByMultipleAttackers or ForceAdditiveKnockback) each axis of the launch after DI
    // goes through the keep rule against it, otherwise the launch replaces the velocity. @4383-4405 then clear the
    // three flags. The same rule on the launch before DI (@1207-2264, @5340-5854) only feeds DI_Visual_Multicast.
    let a = &x.attackers;
    if a.hit_stunned_before_hit && (a.hit_by_multiple_attackers || a.force_additive_kb) {
        let (vel_x, vel_y) = if a.zero_out_vel_for_kb { (0.0, 0.0) } else { (x.body.vx, x.body.vy) };
        l.vx = keep_knockback_axis(l.vx, vel_x);
        l.vy = keep_knockback_axis(l.vy, vel_y);
        no_di_x = keep_knockback_axis(no_di_x, vel_x);
        no_di_y = keep_knockback_axis(no_di_y, vel_y);
    }
    // @3705-3915: NotEqual_VectorVector(no-DI, final, 0.0001) -> DI_Visual_Multicast(no-DI launch,
    // MostRecentAttackerActor), before LaunchCharacter; at K2_GetActorLocation @7206, the launch point.
    if (no_di_x - l.vx).abs() > 0.0001 || (no_di_y - l.vy).abs() > 0.0001 {
        let attacker = x.attackers.most_recent_attacker.map_or(-1, |a| w.fighter(a).id);
        let (id, px, py) = (x.id, x.body.x, super::actor_y(w, f));
        w.fighter_mut(f).events.push(Event::DiVisual { fighter: id, attacker, vx: no_di_x, vy: no_di_y, x: px, y: py });
    }
    let x = w.fighter_mut(f);
    x.attackers.hit_stunned_before_hit = false;
    x.attackers.force_additive_kb = false;
    x.attackers.zero_out_vel_for_kb = false;
    // LaunchCharacter(override XY and Z), then BPI_SetTerminalVelocity(false, |launch|, true). It replaces a
    // PendingLaunchVelocity no CMC tick has taken yet (CancelJump's, at a hit without a pause).
    if pending {
        // ACharacter::LaunchCharacter (FUN_142902f70) with both overrides, CMC Launch 142933e50:
        // PendingLaunchVelocity = the launch, over whatever was pending. The frame's CMC (HandlePendingLaunch
        // FUN_1410c8e70, `apply_impulse`) sets the velocity and the mode.
        x.moves.impulse = Some(Impulse { vx: l.vx, vy: l.vy, xy: true, z: true, kind: 0, d: None });
        x.moves.hit_launch = true;
    } else {
        if l.vy > 0.0 {
            set_grounded(x, false);
        }
        if x.moves.hit_launch {
            x.moves.impulse = None;
        }
        x.moves.hit_launch = false;
        x.body.vx = l.vx;
        x.body.vy = l.vy;
    }
    // AdditiveKnockbackVel, then BPI_SetBaseFighterVelocity(AdditiveKnockbackVel). The hitpause ends on a world
    // timer (Comp_HitPause K2_SetTimerDelegate), after the pawn tick, so the next CMC's stage hit reads the launch.
    x.hit.add_kb_vx = l.vx;
    x.hit.add_kb_vy = l.vy;
    x.hit.base_vx = l.vx;
    x.hit.base_vy = l.vy;
    let speed = ue::size2d([l.vx, l.vy]); // @4427-4477 LocalKnockbackSpeed = VSize(AdditiveKnockbackVel)
    set_terminal(w, f, false, speed, true);
    // @4710-4887: BounceWithoutHitStunMinSpeed < LocalKnockbackSpeed sets CanBounceWithoutHitStun (never clears).
    if BOUNCE_NO_HITSTUN_MIN_SPEED < speed {
        w.fighter_mut(f).hit.can_bounce_no_hitstun = true;
    }
    true
}

/// The movement mode after ApplyKnockback's launch where the hitstun step does not follow it (Fighter.beginFrame:
/// the launch of a pause with no knockback source, a fighter in control or under a buff stun). ApplyKnockback
/// @3947 LaunchCharacter leaves the launch pending, then @4045-4111 SetMovementMode(6, 2) whenever the True
/// Hitstun Timer exists, at once (no IsMovingOnGround test). Without the timer the mode is HandlePendingLaunch's
/// alone. A level launch (Z exactly 0) on the floor is left walking. A fighter still Flying: Custom 2 replaces the
/// mode with the timer.
pub fn free_launch_mode(w: &mut World, f: FighterId, vy: Real) {
    let x = w.fighter_mut(f);
    if x.hit.true_hitstun <= 0.0 {
        return;
    }
    x.moves.flying = false;
    if vy != 0.0 {
        set_grounded(x, false);
    }
    if !x.body.on_floor {
        x.hit.kb_mode = true;
    }
}

/// `stepHitstun`: the CMC step of a fighter in hitstun.
pub fn step_hitstun(w: &mut World, f: FighterId) {
    let dt = DT;
    let mut kb_slide = None;
    if let Some(mut l) = w.fighter_mut(f).hit.launch.take() {
        // ApplyKnockback at the end of hitpause. A zero launch (a hit with hitstun and no knockback) changes
        // neither the velocity nor the movement mode.
        let zero = if l.di { l.vx == 0.0 && l.vy == 0.0 } else { !apply_knockback(w, f, &mut l, false) };
        w.fighter_mut(f).hit.launch = Some(l);
        if w.fighter(f).moves.hit_launch {
            apply_impulse(w, f); // CancelJump's launch, not replaced (a zero launch after no pause)
        }
        // A fighter hit during a ControlSpeed command is still Flying when the pause ends before the manager's
        // end logic ran: ApplyKnockback @3947 / @4111 with a True Hitstun Timer end it; a zero launch leaves it.
        if !zero {
            w.fighter_mut(f).moves.flying = false;
        }
        let x = w.fighter(f);
        let no_col = no_collision(w, f);
        // The first sweeping move after a SetCapsuleSize starts with the penetration resolve (the physics step's
        // resolveResize; a flying move tests its own delta in physFlying).
        if x.capsule.cap_resized && !x.moves.flying && !no_col {
            resolve_resize(w, f);
        }
        let x = w.fighter(f);
        // A fighter with no collision (the rescue pull of one still in hitstun): nothing to hit, so no floor, no
        // landing and no bounce, in any mode; the pull's timeline sets the location after this tick.
        if no_col {
            no_collision_move(w, f, dt);
        } else if x.moves.flying {
            phys_flying(w, f, dt, false);
        } else if x.body.on_floor && x.body.vy < 0.0 && x.hit.true_hitstun > 0.0 {
            // A downward launch on the ground: the knockback mode's move hits the floor at its start, so the
            // landing (and the bounce, in the physics step's collide) come from the same path as an airborne victim's.
            let x = w.fighter_mut(f);
            set_grounded(x, false);
            x.hit.kb_mode = true;
            kb_slide = Some(phys_knockback(w, f, dt, 0.0, true));
        } else if x.body.on_floor {
            // A downward launch on the ground hits the floor at once.
            if x.body.vy < 0.0 && try_bounce(w, f, 0.0, 1.0, true, 1.0) {
                return;
            }
            // Landed: stock Walking with no acceleration, so CalcVelocity braking (the fighter CMC's ProcessLanded
            // FUN_1410d9860 calls SetPostLandedPhysics in Falling and in the knockback mode alike).
            w.fighter_mut(f).body.vy = 0.0;
            for _ in 0..2 {
                let px = w.fighter(f).body.x;
                let d = walk_delta(w, f, px, dt / SUBSTEPS, true);
                if ledge_step(w, f, d) {
                    break;
                }
            }
        } else if if zero { x.hit.kb_mode } else { x.hit.true_hitstun > 0.0 } {
            w.fighter_mut(f).hit.kb_mode = true;
            kb_slide = Some(phys_knockback(w, f, dt, 0.0, true));
        } else {
            // Falling with no input: no lateral change; gravity down to the terminal velocity (NewFallVelocity),
            // 2 substeps whose floor sweep lands or bounces.
            let h = dt / SUBSTEPS;
            for s in 0..2 {
                let x = w.fighter(f);
                let vy0 = x.body.vy;
                let vy1 = vy0 - GRAVITY_Z * x.moves.gravity_scale * h;
                let vy1 = cap_fall(w, f, vy1);
                let x = w.fighter_mut(f);
                x.body.vy = vy1;
                let (px, py, vx) = (x.body.x, x.body.y, x.body.vx);
                if fall_move(
                    w,
                    f,
                    px,
                    py,
                    vx * h,
                    0.5 * (vy0 + vy1) * h,
                    h,
                    crate::real(s),
                    true,
                    (SUBSTEPS - crate::real(s) - 1.0) * h,
                ) {
                    break;
                }
            }
        }
    } else {
        let x = w.fighter(f);
        if x.capsule.cap_resized && !x.moves.flying && !no_collision(w, f) {
            resolve_resize(w, f); // ahead of this frame's stage test (collide)
        }
    }
    tick_true_hitstun(w, f);
    // Landed without a bounce: OnLanded; the launch carries on along the floor until hitstun ends. (PhysFlying
    // slides on a floor and does not land.)
    let x = w.fighter(f);
    if !x.moves.flying && !no_collision(w, f) && collide(w, f, true, kb_slide) {
        hitstun_landed(w, f);
    }
    let x = w.fighter_mut(f);
    if x.hit.hitpause > 0.0 {
        return; // bounced
    }
    if x.hit.hitstun <= 0.0 {
        // A launch with no HitstunTimer (AddDecayedHitStun @3723: RemoveHitstunFromKnockBack at the hit): keep the
        // current velocity and return to normal control. Under a buff stun that cleared the timer (AddBuffHitStun
        // @166) the buff's source is still on: RemoveHitstunFromKnockBack returned at CheckHitstunActive (@7478), so
        // no PreviousAttacker reset and no end sequence here; the stun's end runs it (`buff_stun_end`).
        x.hit.launch = None;
        if x.buffs.buff_stunned {
            return;
        }
        x.attackers.previous_attacker = None;
        hitstun_ended(w, f);
    }
}

/// Comp_Pawn_HitstunHandler, no hitstun or buff stun source left (ubergraph 1132-2689).
pub fn hitstun_ended(w: &mut World, f: FighterId) {
    // RemoveHitstunSource (HitstunHandler @0-204): only with a source, it broadcasts
    // OnIsHitStunned(CheckHitstunActive).
    let x = w.fighter_mut(f);
    if x.attackers.stun_source {
        x.attackers.stun_source = false;
        let stunned = x.moves.pull.is_some() || x.buffs.buff_stunned;
        stun_changed(w, f, stunned);
    }
    // broadcast HitStunEnded(FighterChar, null) (@1159): with the DamageManager's HitStunEnded bound (a knockback
    // hit's HitReactionMulticast @3912 / @3935), its handler (DamageManager ubergraph 11939) unbinds it (@12013)
    // and calls StopAnimMontage(HitStunMontage = null) (@12054): the current active montage blends out with its
    // own BlendOut. Taken ahead of the writes below, which follow the broadcast in the build (@1188-2689).
    // Unverified: the delegate's other handlers (the smoke trail's HitStunEnd, bound at BeginPlay @6215; the
    // buffs') are taken after it, the order of the invocation list (bind order) not read.
    let x = w.fighter_mut(f);
    if x.attackers.hit_stun_ended_bound {
        x.attackers.hit_stun_ended_bound = false;
        stop_anim_montage(w, f);
    }
    let x = w.fighter_mut(f);
    x.hit.add_kb_vx = 0.0; // AdditiveKnockbackVel zeroed
    x.hit.add_kb_vy = 0.0;
    // @1821-1908: UnsetHitByMultipleAttackers and SetForceAdditiveKnockback(false). The buff stun's end comes here
    // too and leaves PreviousAttacker.
    x.attackers.hit_by_multiple_attackers = false;
    x.attackers.force_additive_kb = false;
    set_terminal(w, f, true, 0.0, false);
    let x = w.fighter_mut(f);
    x.fast_fall.ff_triggered = false; // BPI_SetFastFallTriggered(false)
    x.hit.hitstun_end_time = x.clock.time;
    let id = x.id;
    x.events.push(Event::SmokeTrail { fighter: id, on: false }); // HitStunEnded -> StopSmokeTrailMulticast
    buff_signal(w, f, BuffSignal::HitStunEnded, BuffSignalArg::default());
    if let Some(h) = w.fighter(f).hooks.char.hit_stun_ended {
        h(w, f); // the pawn's (CharHooks.hitStunEnded)
    }
    update_air_control(w, f, AIR_CONTROL); // BPI_ResetAirControl: BaseAirControl
    let x = w.fighter_mut(f);
    x.attackers.combo_reset_tick = to_int32(crate::real(x.clock.tick) + DI_COMBO_RESET_FRAMES);
    reset_combo_res(w, f); // AntiInfiniteComboCounter 0 and BPI_ResetComboResMap
}

/// A spawned platform's box (a platform's `canPassThrough` is defined): no landing on it is on solid ground.
fn base_is_spawned(base: Option<PlatformRef>) -> bool {
    matches!(base, Some(PlatformRef::Spawned(..)))
}

/// A floor hit in hitstun: the fighter CMC's ProcessLanded (FUN_1410d9860) goes to Walking (Velocity.Z 0) from
/// Falling and from the knockback mode alike, and Landed resets the jump data. A floor bounce lands too: BFBounce
/// runs from the capsule's Hit event inside the move, the landing follows, and the bounce's launch waits for its
/// hitpause to end.
pub fn hitstun_landed(w: &mut World, f: FighterId) {
    bf_landed(w, f); // OnBFLanded @110711
    // A fast fall triggered in hitstun lands here: fast_fall_landed below.
    if !w.fighter(f).hit.kb_mode {
        juggle_landed(w, f);
    }
    let x = w.fighter_mut(f);
    set_grounded(x, true);
    x.platforms.dropping_down = false; // OnLanded
    x.platforms.evading_through_platform = false;
    x.hit.kb_mode = false;
    x.body.vy = 0.0;
    // The same Landed event as outside hitstun (BaseFighter ubergraph @110711; the physics step's land): BlockLandReset
    // @110973 returns ahead of the air resets, after the wall fatigue reset of a floor that is no spawned platform.
    landed_on_solid_ground(w, f);
    let x = w.fighter_mut(f);
    if x.crouch.block_land_reset {
        x.fast_fall.fast_falling = false; // the mode change's (@128281), as resetAir
        if !base_is_spawned(x.platforms.base) {
            reset_wall_fatigue(w, f);
        }
        fast_fall_landed(w, f);
        return;
    }
    reset_air(w, f, false, false);
    fast_fall_landed(w, f);
}

/// True Hitstun Timer -> Remove Knockback Friction: SetMovementMode(Falling). A tick in the frame the timer was set
/// in is not one the engine's timer counts (it is pending until that frame's timer tick), so the remaining time
/// leaves it out.
pub fn tick_true_hitstun(w: &mut World, f: FighterId) {
    let x = w.fighter_mut(f);
    let h = &mut x.hit;
    if h.true_hitstun <= 0.0 {
        return;
    }
    if x.clock.tick > h.true_hitstun_set_tick {
        h.true_hitstun_counted += 1.0;
        h.true_hitstun_tick_at = x.clock.tick;
    }
    h.true_hitstun -= 1.0;
    if h.true_hitstun == 0.0 {
        // Remove Knockback Friction @10319: SetMovementMode(Falling) also ends MOVE_Flying. Under a stun suspension the
        // HitstunHandler's tick writes Flying again (@9533), so the mode a Blueprint set before the suspension stays.
        h.kb_mode = false;
        if !x.buffs.suspended {
            x.moves.flying = false;
        }
    }
}

/// K2_GetTimerRemainingTimeHandle(True Hitstun Timer), as `hitstun_remaining`: the Rate less the timer ticks counted
/// before the call (a tick already made in the current frame is one the engine has not counted yet).
pub fn true_hitstun_remaining(w: &World, f: FighterId) -> Real {
    let x = w.fighter(f);
    let h = &x.hit;
    let n = h.true_hitstun_counted - if h.true_hitstun_tick_at == x.clock.tick { 1.0 } else { 0.0 };
    h.true_hitstun_rate - max(n, 0.0) * DELTA_SECONDS
}

/// AddDecayedHitStun's True Hitstun Timer block (HitstunHandler @1418-1812): with the timer existing, the new
/// KBFrictionHitStun replaces it only when K2_GetTimerRemainingTimeHandle < it (@1544-1620, float seconds, strict);
/// set (@1630-1812): cleared and K2_SetTimerDelegate(KBFrictionHitStun, no loop), pending in its frame; a Time of 0
/// sets no timer. `lead`: the port's own ticks of the timer left in the frame it is set in (`try_bounce`).
pub fn set_true_hitstun(w: &mut World, f: FighterId, frames: Real, sec: Real, lead: Real) {
    if w.fighter(f).hit.true_hitstun > 0.0 && !(true_hitstun_remaining(w, f) < sec) {
        return;
    }
    let x = w.fighter_mut(f);
    let h = &mut x.hit;
    h.true_hitstun = if frames > 0.0 { frames + lead } else { 0.0 };
    h.true_hitstun_rate = sec;
    h.true_hitstun_counted = 0.0;
    h.true_hitstun_set_tick = x.clock.tick;
    h.true_hitstun_tick_at = -1;
}

/// PhysKnockback (native FUN_1410d8100), 2 substeps of 1/120 s (MaxSimulationTimeStep 0.0166). Each substep:
/// CalcVelocity with Friction = KnockbackFriction and BrakingDecelerationFlying 0, which with no acceleration
/// brakes the whole velocity by KnockbackFriction x BrakingFrictionFactor; then the knockback friction using the
/// full frame time (x(1 - min(dt KF, 1)), downward Z x(1 - min(dt KF Downspike, 1))); then NewFallVelocity (FMC
/// +0x6a8, 0x1410d4780: no terminal clamp in Custom 2) and the trapezoid move. With the stick CalcVelocity is the
/// stock one (FMC +0x6b8) with Acceleration = stick x MaxAcceleration and MaxSpeed = MaxCustomMovementSpeed. A
/// floor hit lands or bounces like PhysFalling's. CalcVelocity's own shapes (1429217d0): IsExceedingMaxSpeed, the
/// over-speed rescale GetSafeNormal(OldV) x MaxSpeed, the direction change by GetSafeNormal(A) and Size(V), the input
/// acceleration through GetClampedToMaxSize; MaxSpeed (MaxCustomMovementSpeed x the analog modifier) a float.
/// Returns the substeps' end velocity Z x their timeTick, summed: the Z of the slides after a side hit (`collide`'s
/// `kb_slide`).
pub fn phys_knockback(w: &mut World, f: FighterId, dt: Real, ax: Real, hitstun: bool) -> Real {
    let x = w.fighter(f);
    let g = GRAVITY_Z * x.moves.gravity_scale;
    let fr = 1.0 - min(dt * KB_FRICTION, 1.0);
    let fr_down = 1.0 - min(dt * KB_FRICTION * DOWNSPIKE_FRICTION, 1.0);
    let h = dt / SUBSTEPS;
    let brake = max(1.0 - KB_FRICTION * BRAKING_FRICTION_FACTOR * h, 0.0);
    let a = ax * max_accel(w, f);
    let maxv = crate::math::narrow(def_of(w, x.ch).max_custom_speed) * min(ax.abs(), 1.0);
    let exceeds = |x: Real, y: Real| ue::is_exceeding_max_speed([x, y, 0.0], maxv);
    let mut slide = 0.0;
    for s in 0..2 {
        let x = w.fighter(f);
        let (vx0, vy0) = (x.body.vx, x.body.vy);
        let (mut vx, mut vy) = (vx0, vy0);
        let over = exceeds(vx, vy);
        if a == 0.0 || over {
            vx *= brake;
            vy *= brake;
            if over && ue::size_squared([vx, vy, 0.0]) < maxv * maxv && a * vx0 > 0.0 {
                let n = ue::safe_normal([vx0, vy0, 0.0], ue::SMALL_NUMBER);
                vx = n[0] * maxv;
                vy = n[1] * maxv;
            }
        } else {
            let dir = ue::safe_normal([a, 0.0, 0.0], ue::SMALL_NUMBER)[0];
            let vs = ue::size([vx, vy, 0.0]);
            let k = min(h * KB_FRICTION, 1.0);
            vx -= (vx - dir * vs) * k;
            vy -= vy * k;
        }
        if a != 0.0 {
            let lim = if exceeds(vx, vy) { ue::size([vx, vy, 0.0]) } else { maxv };
            vx += a * h;
            [vx, vy, _] = ue::clamped_to_max_size([vx, vy, 0.0], lim);
        }
        vx *= fr;
        vy *= if vy > 0.0 { fr } else { fr_down };
        vy -= g * h;
        let x = w.fighter_mut(f);
        x.body.vx = vx;
        x.body.vy = vy;
        let (px, py) = (x.body.x, x.body.y);
        let (dx, dy) = (0.5 * (vx0 + vx) * h, 0.5 * (vy0 + vy) * h);
        slide += vy * h;
        if fall_move(w, f, px, py, dx, dy, h, crate::real(s), hitstun, (SUBSTEPS - crate::real(s) - 1.0) * h) {
            break;
        }
    }
    slide
}

/// BFGetBounceVelocity (BFBounce's v; also Tree Fort's pillars): BaseFighterVelocity per axis, and where that is
/// exactly 0 the last launch divided by KnockbackMultiplier. Sim space.
pub fn bounce_velocity(w: &World, f: FighterId) -> (Real, Real) {
    let h = &w.fighter(f).hit;
    (
        if h.base_vx != 0.0 { h.base_vx } else { h.add_kb_vx / KB_MULT },
        if h.base_vy != 0.0 { h.base_vy } else { h.add_kb_vy / KB_MULT },
    )
}

/// The class default of a BaseFighter property: the pawn's, else BaseFighter's.
fn cdo<'a>(w: &'a World, f: FighterId, k: &str) -> Option<&'a Value> {
    let d = fighter_data(w, w.fighter(f).ch);
    d.pawn.get(k).or_else(|| d.base.get(k))
}

/// BaseFighter WallBounceForceVertMin (CDO false) until BPI_SetWallBounceVerticalMin (@188877) writes it. Read by
/// BFBounce @6541-6642. No reset on a ring-out.
fn wall_bounce_force_vert_min(w: &World, f: FighterId) -> bool {
    w.fighter(f)
        .hit
        .wall_bounce_force_vert_min
        .unwrap_or_else(|| cdo(w, f, "WallBounceForceVertMin").and_then(Value::as_bool) == Some(true))
}

/// BaseFighter WallBounceForcedVertMinAmount (CDO 0.3), as `wall_bounce_force_vert_min`.
fn wall_bounce_forced_vert_min_amount(w: &World, f: FighterId) -> Real {
    w.fighter(f).hit.wall_bounce_forced_vert_min_amount.unwrap_or_else(|| {
        crate::math::narrow(cdo(w, f, "WallBounceForcedVertMinAmount").and_then(Value::as_f64).unwrap_or(0.0))
    })
}

/// BPI_SetWallBounceVerticalMin(FoceMin, MinAmount) (callers JakeSpitCharacterHitBox, ProjectileAimer_SpitJake).
pub fn bpi_set_wall_bounce_vertical_min(w: &mut World, f: FighterId, force: bool, amount: Real) {
    let h = &mut w.fighter_mut(f).hit;
    h.wall_bounce_force_vert_min = Some(force);
    h.wall_bounce_forced_vert_min_amount = Some(amount);
}

/// BFBounce's KnockBackDirectionVector (@6411-6642): Normal(LocalBounceVelocity, 0.0001) as (Y, Z), Z raised to
/// WallBounceForcedVertMinAmount when WallBounceForceVertMin (every bounce type). The hit's Calculate Knock Back
/// normalises it again (`knockback_dir`).
fn bounce_dir(w: &World, f: FighterId, bx: Real, by: Real) -> (Real, Real) {
    let n = ue::normal([bx, by, 0.0], ue::KINDA_SMALL_NUMBER);
    let (dx, mut dy) = (n[0], n[1]);
    if wall_bounce_force_vert_min(w, f) {
        dy = max(dy, wall_bounce_forced_vert_min_amount(w, f));
    }
    (dx, dy)
}

/// BFBounce: in hitstun (out of hitpause), hitting the stage with |v . n| > 1100, or a floor with |v| > 700,
/// bounces. The bounce is a hit from MostRecentAttacker dealt through its HitBoxHandler: 1 damage, BaseKnockBack
/// |bounce| / (KnockbackMultiplier x 1.5) along the bounce, HitStunMult 0 (hitstun is kept), AttackID -13, and
/// hitpause Lerp(0.07, 0.15, |v . n| / |v|). With no HitstunTimer (@1539-1591 -> 4375-4734) the test is instead
/// !isEvading && !IsAttacking && !Jumping && |v . n| > BounceWithoutHitStunMinSpeed (4000) &&
/// CanBounceWithoutHitStun, and the flow joins the same bounce (@1888) with the hitstun end in place of the hitstun.
/// The gate @303-656: GetHitPauseTime <= 0, CanWallBounce?, HasAuthority and a MostRecentAttackerActor (or a
/// MostRecentFriendlyFire); the instigator @759 is MostRecentAttackerActor when valid, else MostRecentFriendlyFire.
/// `landing`: whether a floor-type bounce lands him; false for a hit on the stage block's top edge that slid on.
/// `hit_mult`: BFBounce's HitCollisionBounceMultiplier (a box actor's BPI_GetWallBounceMultiplier; 1 otherwise),
/// the factor of the bounce velocity @3983.
// Unverified: HitReactionMulticast's other steps on the free bounce (move commands, pending impulses) are taken as
// in receiveHit's montage stop only.
pub fn try_bounce(w: &mut World, f: FighterId, nx: Real, ny: Real, landing: bool, hit_mult: Real) -> bool {
    let x = w.fighter(f);
    let ff = if x.attackers.most_recent_attacker.is_some() { None } else { x.attackers.most_recent_friendly_fire };
    let Some(att) = x.attackers.most_recent_attacker.or(ff) else { return false };
    if x.hit.hitpause > 0.0 || x.hit.no_wall_bounce {
        return false;
    }
    let free = x.hit.hitstun <= 0.0;
    let attacking = x.charge.attack_is_active > 0 || x.charge.begin_attack;
    if free && (!x.hit.can_bounce_no_hitstun || x.evade.evading || attacking || x.moves.cmd_jumped) {
        return false;
    }
    let (vx, vy) = bounce_velocity(w, f);
    let len = ue::size2d([vx, vy]); // @972 VSize(LocalImpactVelocity)
    if len == 0.0 {
        return false;
    }
    let proj = vx * nx + vy * ny;
    let pass = if free {
        proj.abs() > BOUNCE_NO_HITSTUN_MIN_SPEED
    } else {
        proj.abs() > BOUNCE_MIN_SPEED_NOT_FLOOR || (ny > 0.9 && len > BOUNCE_MIN_SPEED_FLOOR)
    };
    if !pass {
        return false;
    }
    let x = w.fighter_mut(f);
    let since = x.clock.time - x.hit.prev_bounce_time;
    if since < BOUNCE_RAPID_TIME && crate::real(x.hit.rapid_bounce_count) > BOUNCE_RAPID_COUNT {
        x.hit.rapid_bounce_count = 0;
        return false;
    }
    // Mirror by the normal, lean 20% toward up off floors and keep the speed; floors leave at least 2200 up.
    // @3445-3634: Normal(MirrorVectorByNormal(V, N), 0.0001), VLerp toward (0, 0, 1) ((B - A) alpha + A), Normal
    // again, x the speed.
    let (mx, my) = (vx - 2.0 * proj * nx, vy - 2.0 * proj * ny);
    let [mx, my, _] = ue::normal([mx, my, 0.0], ue::KINDA_SMALL_NUMBER);
    let up = if ny > 0.1 { BOUNCE_UP_MOD } else { 0.0 };
    let (lx, ly) = ((0.0 - mx) * up + mx, (1.0 - my) * up + my);
    let [lx, ly, _] = ue::normal([lx, ly, 0.0], ue::KINDA_SMALL_NUMBER);
    // BFBounce @3268: the speed is ImpactVelocityLength x BounceMultiplier (the misc movement fold), ahead of the
    // floor minimum.
    let bmul = bounce_mult(w, f);
    let sp = if bmul == 1.0 { len } else { len * bmul };
    let mut bx = lx * sp;
    let mut by = ly * sp;
    let floor = ny > 0.0 && nx.abs() < 0.1;
    let ceiling = ny < 0.0 && nx.abs() < 0.1;
    if ny > 0.1 {
        by = max(by, BOUNCE_MIN_FLOOR_VEL);
    }
    if hit_mult != 1.0 {
        bx *= hit_mult; // @3983, behind the floor minimum
        by *= hit_mult;
    }
    if let Some(ff) = ff {
        friendly_fire_bounce(w, f, ff, bx, by);
        return false;
    }
    let x = w.fighter_mut(f);
    x.hit.rapid_bounce_count = if since < BOUNCE_RAPID_TIME { x.hit.rapid_bounce_count + 1 } else { 0 };
    x.hit.prev_bounce_time = x.clock.time;
    x.hit.bouncing = true; // IsBouncing = LocalBounceType (always 1..6)
    x.hit.anim_bounce = None; // its OnIsBouncing broadcasts give Base_AnimBP the same type
    // FacePlanting (moving the way the fighter faces) picks the even bounce type.
    let face_plant = if vx >= 0.0 { x.body.facing_dir > 0.0 } else { x.body.facing_dir < 0.0 };
    x.hit.bounce_type = match (floor, ceiling, face_plant) {
        (true, _, true) => 4,
        (true, _, false) => 3,
        (_, true, true) => 6,
        (_, true, false) => 5,
        (_, _, true) => 2,
        _ => 1,
    };
    x.hit.floor_bounce_hurt = floor;
    let bounce_type = x.hit.bounce_type;
    // The hit (DamageAndLaunch, path A). @6328 VSize(LocalBounceVelocity).
    let bl = ue::size2d([bx, by]);
    let hp = HitParams {
        damage: 1.0,
        base_knock_back: min(bl / (KB_MULT * BOUNCE_FLOOR_MULT), BOUNCE_KB_CAP),
        knock_back_scaling: 0.0,
        scaling_start_dmg: 0.0,
        hit_pause_time: BOUNCE_HITPAUSE_MIN + (BOUNCE_HITPAUSE_MAX - BOUNCE_HITPAUSE_MIN) * (proj.abs() / len),
        hit_stun_mult: 0.0,
        charge_kb_mult: 1.0,
        charge_hit_pause_mult: 0.0,
        total_charged_time: 0.0,
        weight: weight(w, f),
    };
    // ApplyBuffDamageMods (MostRecentAttacker is not the owner), then GetHitStunDecay with the bounce's type 5: only
    // combo resistance decays it (damage x0.6), then DamageAndLaunch @2400-2555 FClamp(Damage, 0, 10000).
    // Unverified: the buffed bounce damage is not captured.
    // RegisterHitFromFighter @732 comes before the taken mods @1621-1960; nothing between reads what it writes.
    let dh = DecayHit {
        attack_id: -13,
        decay_type: 5,
        decay_name: crate::names::Name::NONE,
        combo_name: crate::names::Name::NONE,
        base_knock_back: 0.0,
        launch_knock_back: Some(hp.base_knock_back),
        additive_kb_mode: None,
        knock_back_scaling: None,
        hit_stun_mult: None,
    };
    register_hit_from(w, f, att, &dh, true, att);
    // The taken conditions' Attacker: the bounce's dealer, no hit box; no Instigator handed on.
    // Unverified: the bounce's damage alone goes through the taken pass (a 1.0 probe); its HitPause, direction and
    // effect are not taken from it (the bounce's DamageAndLaunch not read for them).
    let bm = apply_taken_damage_mods(w, f, 1.0, 0.0, 0.0, None, false, Some(ActorRef::Fighter(att)), None)[0];
    let res = w.fighter(f).decay.combo_resistance;
    let bd = min(max(if res { bm * DAMAGE_DECAY_MOD } else { bm }, 0.0), 10000.0); // @2309, @2499
    let bt = absorb_temp_health(w, f, bd);
    let post = bt + w.fighter(f).stats.damage; // no cap; the float32 CurrentDamage Calculate Knock Back reads
    let k = compute_knock(&hp, post);
    // KnockBackDirectionVector (@6411-6788): Normal(LocalBounceVelocity), its Z raised to
    // WallBounceForcedVertMinAmount when WallBounceForceVertMin, as (Y, Z); Calculate Knock Back takes its Normal2D.
    let (dx, dy) = bounce_dir(w, f, bx, by);
    let (dx, dy) = knockback_dir(dx, dy);
    let mut lvx = dx * k.speed;
    let mut lvy = dy * k.speed;
    if lvy == 0.0 && lvx != 0.0 {
        lvy = 0.001;
    }
    (lvx, lvy) = apply_knock_back_multipliers(w, att, lvx, lvy, false);
    (lvx, lvy) = apply_knock_back_multipliers(w, f, lvx, lvy, true);
    lvx *= k.tier;
    lvy *= k.tier;
    // Calculate Knock Back @4461 with the bounce packet's HitReaction 2 (BFBounce @6864): SpikeBack -> Medium on the
    // bounced fighter. The packet's ControllerRumble is null (@7424): no dealer rumble.
    controller_rumble(w, f, hit_rumble(Some(2.0)));
    bounce_effects(w, f, nx, ny, bounce_type);
    record_damage(w, f, bt, Some(att));
    ffa_damage(w, f, bt);
    crate::world::flow::broadcast_take_any_damage(w, ActorRef::Fighter(f), bd, Some(ActorRef::Fighter(att)));
    w.fighter_mut(f).stats.damage = post;
    // HitReactionMulticast, in hitstun or not (DamageAndLaunch @9984; the bounce passes its zero test
    // @2025-2456): the HitStunEnded binding (@3912 / @3935), AddHitStunFromKnockBack (@4047), Montage_Stop(0.0,
    // null) (@4306) and RemoveQueuedHitPause (@4415).
    w.fighter_mut(f).attackers.hit_stun_ended_bound = true;
    // AddHitStunFromKnockBack: the hitstun end in place of a hitstun start, ahead of the stops. A buff-stunned
    // fighter is on this branch whenever a buff stun cleared its HitstunTimer (AddBuffHitStun @166):
    // RemoveHitstunFromKnockBack (ubergraph 7427) returns at CheckHitstunActive @7478 with the buff's source on.
    if free && !w.fighter(f).buffs.buff_stunned {
        w.fighter_mut(f).attackers.previous_attacker = None;
        hitstun_ended(w, f);
    }
    // In hitstun, AddDecayedHitStun keeps the timer (HitStunMult 0, @444) and the stops end what began in
    // hitstun: a montage a free attack's turn started after the hit, a pawn's own.
    stop_montage(w, f, Some(0.0), None);
    stop_dash_montage(w, f, 0.0);
    stop_grouped(w, f, None, 0.0);
    w.fighter_mut(f).hit.queued_hit_pause = 0.0;
    // DamageAndLaunch's KnockedBack broadcast @10471, after HitReactionMulticast (@9984) and ahead of
    // HitDispatchMulticast (@10701): BaseKnockBack, the bounce packet's direction (IgnoreFacing) and the
    // DamageDealer, the bounce's instigator (pfgDoDamage @15630, LocalWallBounceInstigator @759). Its handlers in
    // receiveHit's order (unverified there): the holds, Buff_MoveToPosition's DestroyOnKnockedBack, then the
    // pawn's classes' bindings. The bounce's HitStunMult is 0, so no buff stun ends (@2488).
    // Deviation: BaseKnockBack is the packet's (the port's bounce runs no CalculateArmor).
    w.fighter_mut(f).attackers.knocked_back_count += 1;
    let kb_arg = (!w.fighter(f).buffs.buffs.is_empty()).then(|| BuffSignalArg {
        by: Some(crate::fighter::common::actor_by(w, ActorRef::Fighter(att), Some(att))),
        base_kb: Some(hp.base_knock_back),
        hit_dir: Some([dx, dy]),
        ..BuffSignalArg::default()
    });
    hold_knocked_back(w, f, false, kb_arg);
    end_move_buff(w, f); // Buff_MoveToPosition OnOwnerKnockedBack (DestroyOnKnockedBack, @683-968)
    if let Some(h) = w.fighter(f).hooks.char.knocked_back {
        h(w, f, hp.base_knock_back, dx, dy, Some(ActorRef::Fighter(att)));
    }
    knocked_back_include_self(w, f, hp.base_knock_back, dx, dy, Some(ActorRef::Fighter(att)), hp.damage);
    // As in receiveHit: the bounce's DamageAndLaunch stores KnockBackVelocity before it sets the pause.
    set_hit_react_info(w, f, lvx, lvy, true);
    hit_pause_time_changed(w, f, k.hitpause_sec);
    w.fighter_mut(f).hit.hitpause = crate::real(k.hitpause_frames);
    // BFBounce @8431, after its pfgDoDamage: OnIsBouncing -> Base_AnimBP OnIsBouncingChanged @154-441:
    // KnockBackAngle = the direction of the bounce velocity.
    let angle = anim_direction(w, f, bx, by);
    w.fighter_mut(f).react.knock_back_angle = angle;
    // The bounce's DamageAndLaunch reaches AddDecayedHitStun's OnHitStunExtended (@1840) and the
    // KnockedBackIncludeSelf broadcast (@10564): dealer = MostRecentAttacker, Damage = the hit's own 1.
    w.fighter_mut(f).attackers.hitstun_count += 1;
    juggle_check(w, f, att, 1.0);
    // HitEnemyMulticast(owner) @9653 first on the bounce path too (IsBounce @9619 -> @9633): an AnimNotify_LaunchSelf
    // with LaunchOnHit bound to it; then HitEnemyPlayerLight @9681 to the instigator's condition objects.
    // Unverified: its place among the bounce's other steps is taken, not read.
    launch_on_hit(w, att);
    cond_signal(w, att, CondSignal::HitEnemyLight);
    // HitStunMult 0: MinHitStunClamp 0.3 s. A bounce inside the normal CMC step comes before that step's True
    // Hitstun Timer tick (cmcTick), one in the hitstun step after it: the timer first counts on the next frame.
    let lead = if w.fighter(f).frame.frame_mode == FrameMode::Normal { 1.0 } else { 0.0 };
    set_true_hitstun(w, f, crate::real(k.kb_stun_frames), k.kb_stun_sec, lead);
    let x = w.fighter_mut(f);
    x.hit.launch = Some(Launch { vx: lvx, vy: lvy, di: false, wall: true, kbi: 1.0 });
    x.hit.free_launch = free;
    if !floor || !landing {
        set_grounded(x, false);
    } else if free {
        land(w, f);
    } else {
        hitstun_landed(w, f);
    }
    let x = w.fighter(f);
    let ev = Event::Hit {
        attacker: w.fighter(att).id,
        victim: x.id,
        damage: 1.0,
        // PlayHitSound's Damage: BFBounce's 1.0 x the instigator's dealt mods (pfgDoDamage @5822), before charge
        // and armor; no On Hit Audio.
        pain_damage: 1.0,
        speed: ue::size2d([lvx, lvy]),
        hitpause: crate::real(k.hitpause_frames),
        lockout: crate::real(k.hitpause_frames) + x.hit.hitstun,
        x: x.body.x,
        y: x.body.y,
        light: false,
    };
    w.fighter_mut(f).events.push(ev);
    // pfgDoDamage's tail once DamageAndLaunch went through (@21522 LocalHasHit): with IsWallBounce, @22708 ->
    // HitEnemyMulticastDispatch @22722 -> @21815 -> HitAnything @22208 of the instigator's handler, with BFBounce's
    // null HitBox (@7803): what the instigator's pawn bound to it (CharHooks.hitAnything).
    // Unverified: the packet handed on holds the bounce's own fields alone; no listener reads it without a hit box.
    crate::world::catch_up::hit_player(w, att); // @22038: BFHitPlayer on the instigator, ahead of HitAnything
    if let Some(h) = w.fighter(att).hooks.char.hit_anything {
        let p = HitPacket {
            damage: Some(1.0),
            base_knock_back: Some(hp.base_knock_back),
            hit_pause_time: Some(hp.hit_pause_time),
            hit_stun_mult: Some(0.0),
            ..HitPacket::default()
        };
        h(w, att, ActorRef::Fighter(f), &p, false);
    }
    if !w.fighter(att).buffs.buffs.is_empty() {
        let arg =
            BuffSignalArg { hit_actor: Some(ActorRef::Fighter(f)), damage: Some(1.0), ..BuffSignalArg::default() };
        buff_signal(w, att, BuffSignal::HitAnything, arg); // the same broadcast's buff binds
    }
    // The same OnIsBouncing broadcast @8431 to its buff binders (Buff_CollateralDamageTracker), after the whole
    // pfgDoDamage @7803, so the tracker's hit comes after the bounce's own.
    let arg = BuffSignalArg { bounce: Some(crate::real(bounce_type)), ..BuffSignalArg::default() };
    buff_signal(w, f, BuffSignal::IsBouncing, arg);
    // OnComponentHit's other binding on the pawn's capsule, after BFBounce (CharHooks.capsuleHit).
    if let Some(h) = w.fighter(f).hooks.char.capsule_hit {
        h(w, f);
    }
    true
}

/// BFBounce with no MostRecentAttackerActor and a MostRecentFriendlyFire (LocalEnemyInstigatedWallBounce false,
/// @721-759): the same bounce velocity and packet as an enemy's (@6264-7706), but HitPauseTime 0.04 in place of the
/// Lerp (@6050 SelectFloat), dealt by the friend's HitBoxHandler pfgDoDamage(..., self, ..., IsWallBounce true)
/// @7803. The friend is of the victim's team: DamageAndLaunch's refused branch, whose ForceKnockback launches him
/// with no damage and no RegisterHitFromFighter, broadcasting KnockedBack, and pfgDoDamage's HitSuccessful is false:
/// BFBounce returns at @7926 -> @9667, so none of the bounce's own state follows. The caller takes the contact as no
/// bounce.
// Unverified: HitSuccessful false on the refused branch with IsWallBounce true is taken from refusedAllyHit's
// decode (pfgDoDamage @7985-8023 -> @15812), not re-read for the bounce's call.
fn friendly_fire_bounce(w: &mut World, f: FighterId, ff: FighterId, bx: Real, by: Real) {
    let bl = ue::size2d([bx, by]);
    let (dx, dy) = bounce_dir(w, f, bx, by);
    let num = Value::Num;
    let vec = |x: Real, y: Real| {
        let mut v = Props::new();
        v.insert("X".into(), num(f64::from(x)));
        v.insert("Y".into(), num(f64::from(y)));
        Value::Obj(v)
    };
    let mut p = Props::new();
    for (k, v) in [
        ("Damage", num(1.0)),
        ("BaseKnockBack", num(f64::from(min(bl / (KB_MULT * BOUNCE_FLOOR_MULT), BOUNCE_KB_CAP)))),
        ("KnockBackScaling", num(0.0)),
        // The build's X is UE Y (-sim x); IgnoreFacing turns it back.
        ("KnockBackDirectionVector", vec(-dx, dy)),
        ("AttackTag", Value::Str("None".into())),
        ("HitReaction", num(2.0)),
        ("HitPauseTime", num(0.04)),
        ("HitStunMult", num(0.0)),
        ("IgnoreFacing", Value::Bool(true)),
        ("AttackID", num(-13.0)),
        ("DoLOSCheck", Value::Bool(false)),
        ("ForceKnockback", Value::Bool(true)),
        ("ControllerRumble", Value::Null),
        ("AttackDecayType", num(5.0)),
        ("AdditiveKBMode", num(0.0)),
        ("KBScalingStartingDmg", num(0.0)),
        ("IgnoreKBI", Value::Bool(false)),
        ("KBI_Multiplier", num(1.0)),
        ("PlayOnHitEffects", Value::Bool(false)),
    ] {
        p.insert(k.into(), v);
    }
    let hp = crate::combat::HitPacket::from_props(&w.names, &p, None);
    crate::combat::hits::refused_ally_hit(w, ff, f, &hp, true, ActorRef::Fighter(ff));
}

/// BaseFighter.IsBouncing (Enum_BounceType: 1 WallBack, 2 WallFront, 3 FloorBack, 4 FloorFront, 5 CeilingBack,
/// 6 CeilingFront): BFBounce's LocalBounceType from the bounce on, 0 again at BFHitPauseEnd.
pub fn is_bouncing(w: &World, f: FighterId) -> i32 {
    let h = &w.fighter(f).hit;
    if h.bouncing { h.bounce_type } else { 0 }
}

/// Wall Bounce Multicast -> BFBounceEffects. LocalBounceAngle = DegAtan2(N.Y, N.Z), remapped by facing for the wall
/// types; the VFX roll is -90 on floors (types 3/4), 90 on ceilings (5/6), else that angle. BFBounceEffects (unless
/// BounceEffectsBuffer is up) spawns PS_HitBounceImpact at the impact point with roll (FacingLeft ? 1 : -1) x
/// (90 + roll), scale 1.35. The buffer is cleared by a Delay(0.2) in the pawn graph, created here in the CMC tick:
/// it blocks the bounces of 12 CMC ticks.
pub fn bounce_effects(w: &mut World, f: FighterId, nx: Real, ny: Real, kind: i32) {
    let x = w.fighter_mut(f);
    let left = x.body.facing_dir < 0.0;
    let mut angle = ue::deg_atan2(-nx, ny); // @8134 DegAtan2(N.Y, N.Z)
    if kind == 2 {
        angle = if left { (-angle) + 270.0 } else { angle + 270.0 };
    } else if kind == 1 {
        angle = if left { (-angle) + 450.0 } else { angle - 270.0 };
    }
    // BFBounce @8404 BouncePitchAngleDegrees = LocalBounceAngle and @8431 OnIsBouncing -> Base_AnimBP
    // OnIsBouncingChanged @27: the Bounce state's pitch, on every bounce.
    x.react.bounce_pitch = angle;
    let roll = match kind {
        5 | 6 => 90.0,
        3 | 4 => -90.0,
        _ => angle,
    };
    if crate::real(x.clock.tick) < x.hit.bounce_fx_until {
        return;
    }
    x.hit.bounce_fx_until = crate::real(x.clock.tick) + round(0.2 / DT);
    // ImpactPoint of the capsule sweep: the capsule's support point along -N. On a vertical wall the capsule's
    // segment is parallel to the box face and PhysX's GJK sweep (PxGeometryQuery::sweep) returns the point at the
    // same fraction s of both half lengths: Z = centre - s h with s = (centre - box centre) / (box half height + h).
    // Past |s| = 1 the cap sphere meets the box's edge, so the point stays at the box's top or bottom.
    let (ch, cr) = (cap_half(w, f), cap_radius(w, f));
    let h = ch - cr;
    // The box hit: of the walking boxes (World.ground and the stage solids'), the one the capsule is nearest
    // (World.ground on a tie).
    // Deviation: the hit's own component is not carried to here; the bounce is tested after the move, with the
    // capsule against the box it stopped at. A bounce off another body (a platform, a ship's piece) still takes
    // World.ground's height.
    let mut g = w.stage.ground;
    let gs = floor_list(w, f);
    let (fx, fy) = (w.fighter(f).body.x, w.fighter(f).body.y);
    if gs.len() > 1 {
        let mut best = capsule_gap(fx, fy, cr, ch, &g);
        for gi in &gs[1..] {
            let d = capsule_gap(fx, fy, cr, ch, gi);
            if d < best {
                best = d;
                g = *gi;
            }
        }
    }
    let s = (fy - crate::math::narrow((g.top + g.bottom) / 2.0))
        / crate::math::narrow((g.top - g.bottom) / 2.0 + f64::from(h));
    let px = fx - nx * cr;
    let py = if ny == 0.0 {
        max(crate::math::narrow(g.bottom), min(fy - s * h, crate::math::narrow(g.top)))
    } else {
        fy - ny * cr - sign(ny) * h
    };
    let roll = (if left { 1.0 } else { -1.0 }) * (90.0 + roll);
    let x = w.fighter_mut(f);
    x.events.push(Event::BounceVfx { x: px, y: py, roll });
    // BounceZOffset (BFBounce 2401-3061, 5109-5837; set with the effects, so not on a buffered bounce): walls take
    // the impact point's height over the capsule centre, ceilings its height over the capsule bottom, each pulled
    // back by the radius toward 0 (-R when above, +R otherwise); floors leave 0.
    if matches!(kind, 1 | 2 | 5 | 6) {
        let d = py - if kind <= 2 { fy } else { fy - ch };
        x.hit.bounce_z_offset = d + if d > 0.0 { -cr } else { cr };
    } else {
        x.hit.bounce_z_offset = 0.0;
    }
    // Then PF_Surface_Effect_Trace (sphere): SFX_BodyFall from the actor location - Trace Offset 02 for bounce
    // types 3/4, else SFX_Wall_Bounce from the actor location.
    if kind == 3 || kind == 4 {
        let ev = crate::name!("sfx_shared_foley_medium_bodyfall");
        surface_sfx(w, f, ev, fy - TRACE_OFFSET_02, true, None, Some(fx));
    } else {
        let ev = crate::name!("sfx_shared_foley_medium_wallbounce");
        surface_sfx(w, f, ev, fy, true, None, Some(fx));
    }
}

/// `floorBounceCapsule`: BaseFighter's HurtBoxFloorBounce, a HurtBox capsule on the root capsule, at the actor's
/// scale. The construction script (BaseFighter @1407-1586) puts it at relative Z -(CapsuleComponent
/// .CapsuleHalfHeight - its CapsuleHalfHeight), the root capsule's template half height (unscaled), whatever
/// RelativeLocation its own template has: its bottom on the root capsule's bottom. `dz`: its centre below the actor
/// location; the half height times the scale's Z, the radius times min(X, Y) (UCapsuleComponent). `None`: the data
/// has no such component. Its template's half height and radius are the pawn's over BaseFighter's (40 / 22).
pub fn floor_bounce_capsule(w: &World, f: FighterId) -> Option<FloorBounceCapsuleOut> {
    let x = w.fighter(f);
    let d = &fighter_data(w, x.ch).components;
    let k = "HurtBoxFloorBounce_GEN_VARIABLE";
    let p = d.pawn.get(k).or_else(|| d.base.get(k))?.p.as_obj()?;
    let half = p.get("CapsuleHalfHeight").and_then(Value::as_f64).unwrap_or(40.0);
    let r = p.get("CapsuleRadius").and_then(Value::as_f64).unwrap_or(22.0);
    let (kz, kr) = match x.body.actor_scale {
        Some([xy, z]) => (z, xy),
        None => (x.body.size_scale, x.body.size_scale),
    };
    let dz = def_of(w, x.base_ch).cap_half - half;
    Some(if kz == 1.0 && kr == 1.0 {
        FloorBounceCapsuleOut {
            dz: crate::math::narrow(dz),
            half: crate::math::narrow(half),
            r: crate::math::narrow(r),
        }
    } else {
        FloorBounceCapsuleOut {
            dz: crate::math::narrow(dz * f64::from(kz)),
            half: crate::math::narrow(half * f64::from(kz)),
            r: crate::math::narrow(r * f64::from(kr)),
        }
    })
}

/// What `floorBounceCapsule` gives.
#[derive(Clone, Debug)]
pub struct FloorBounceCapsuleOut {
    pub dz: Real,
    pub half: Real,
    pub r: Real,
}
