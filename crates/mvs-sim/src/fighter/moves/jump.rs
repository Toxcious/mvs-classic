//! Jumps, air control, dash dance and fast fall.

use super::*;
use crate::combat::buffs::{BuffThruAtkK, buff_block_fast_fall, buff_signal, buff_thru_atk};
use crate::consts::{
    DASH_FALL_FRICTION, DASH_FALL_TIME, DASH_IMPULSE, DASH_JUMP_MULT, DASH_JUMP_NO_METER_ADD, DASH_JUMP_NO_METER_MULT,
    DASH_RATE_MIN, DELTA_SECONDS, DOUBLE_JUMP_GATE, DOUBLE_JUMP_MIN_TIME, FALL_LATERAL_FRICTION, FAST_FALL_DELAY,
    FAST_FALL_JUMP_BOOST, FAST_FALL_JUMP_GAP, FAST_FALL_STOP_SPEED, GRAVITY_SCALE, GRAVITY_Z, JUMP_AFTER_HITSTUN,
    JUMP_CONTROL_OVERRIDE, JUMP_MIN_TIME, QUICK_DASH_TIME, QUICK_FALL_VELOCITY,
};
use crate::fighter::anim::SinkKind;
use crate::fighter::anim::{
    active_grouped, active_instance, has_active, is_dash_anim, play_plain_montage, slot_blending_out,
    slot_jump_to_section, stop_dash_montage, stop_grouped_entry, stop_montage,
};
use crate::fighter::common::latent_frames;
use crate::fighter::match_side::{handle_air_move_desaturation, move_fx, sfx, tutorial_event_of, tutorial_world};
use crate::input::Btn;
use crate::math::{max, min, sign, to_int32};
use crate::registry::signals::{BuffSignal, BuffSignalArg};

/// `f.nav`: the navigation set the fighter reads (`SetRef`).
pub(super) fn nav_of(w: &World, f: FighterId) -> &crate::fighter::anim::NavSet {
    let ch = crate::fighter::body::def(w, f);
    match w.fighter(f).anim.nav {
        crate::fighter::anim::SetRef::Base => &ch.nav,
        crate::fighter::anim::SetRef::Named(n) => &ch.nav_sets[w.names.str(n)],
    }
}

/// `Fighter.activeMontageName`: GetCurrentActiveMontage's montage among `montage` and the grouped instances.
pub(super) fn active_montage_name(w: &World, f: FighterId) -> Option<Name> {
    let fi = w.fighter(f);
    let grouped = active_grouped(w, f).and_then(|i| fi.montages.grouped.as_ref().map(|g| g.0[i].m.montage.name));
    grouped.or_else(|| fi.montages.montage.as_ref().map(|m| m.montage.name))
}

/// `FAST_FALL_LANDED_BINDERS`: the buff classes whose BeginPlay binds FastFallLanded.
pub(super) fn fast_fall_landed_binder(cls: Name) -> bool {
    cls == crate::name!("Buff_Perk_WonderWoman_FastFallSlam_C")
}

fn latent(s: Real) -> i32 {
    to_int32(latent_frames(s))
}

/// The cut NetImpulse of EndJump: Z = JumpStopVelocity, Z override, type 3.
fn cut_impulse(w: &mut World, f: FighterId, vy: Real) {
    net_impulse(w, f, 0.0, vy, false, true, 3.0, false, 0.0);
}

/// `endJumpAtStart`: EndJump(0, QuickFall false, FastfallMult false), called by every attack and evade start
/// (ubergraph 14435; FastfallMult = false @131698). Under a modified gravity (@14519) no cut and the gate stays.
pub fn end_jump_at_start(w: &mut World, f: FighterId) {
    let fi = w.fighter_mut(f);
    fi.moves.ground_jump = None;
    fi.jump.jump_hold_timer = None;
    crate::fighter::set_jumping(w, f, false);
    let fi = w.fighter_mut(f);
    fi.platforms.dropping_down = false; // EndJump's body clears both
    fi.platforms.evading_through_platform = false;
    fi.jump.jump_cut_at = -1;
    fi.jump.quick_fall_at = -1;
    fi.jump.end_jump_latent = true;
    fi.fast_fall.ff_mult = false;
    let v = jump_velocity(w, f);
    let fi = w.fighter_mut(f);
    fi.jump.jump_v = v;
    if fi.moves.gravity_mod_count > 0 {
        return;
    }
    // The cut's gate @17547-17889.
    if fi.jump.jump_stop_gate && !fi.fast_fall.fast_falling && !fi.jump.block_jump_end_launch {
        let vs = jump_stop_mult(w, f) * w.fighter(f).jump.jump_v;
        cut_impulse(w, f, vs);
    }
    w.fighter_mut(f).jump.jump_stop_gate = false;
}

/// `endJumpRerun`: EndJump(0)'s RetriggerableDelay(0) @131806, whose link runs the body a second time in the next
/// pawn tick's latent pass.
// Unverified: from the bytecode and RetriggerableDelay's 4.26 source; neither case is in a capture.
pub fn end_jump_rerun(w: &mut World, f: FighterId) {
    w.fighter_mut(f).jump.end_jump_latent = false;
    crate::fighter::set_jumping(w, f, false);
    let fi = w.fighter_mut(f);
    fi.moves.ground_jump = None;
    fi.jump.jump_hold_timer = None;
    fi.jump.quick_fall_at = -1;
    if fi.moves.gravity_mod_count > 0 || !fi.jump.jump_stop_gate {
        return;
    }
    let fi = w.fighter(f);
    if !fi.fast_fall.fast_falling
        && !crate::fighter::in_hitstun(w, f)
        && fi.life.dead <= 0.0
        && !fi.jump.block_jump_end_launch
    {
        let vs = jump_stop_mult(w, f) * fi.jump.jump_v;
        cut_impulse(w, f, vs);
    }
    w.fighter_mut(f).jump.jump_stop_gate = false;
}

/// `checkLateralFriction`: CheckLateralFriction (BaseFighter, ubergraph 174711): FallLateralFriction while holding
/// against the motion, 1.2 in the first 0.1 s of air after a dash, else 0.
pub fn check_lateral_friction(w: &mut World, own: FighterId) {
    let f = own;
    let fi = w.fighter(f);
    let x = fi.controls.inp.x;
    let vx = fi.body.vx;
    let against = (x > 0.0 && vx < 0.0) || (x < 0.0 && vx > 0.0);
    let mut gate = !crate::fighter::in_hitstun(w, f)
        && ((fi.air.dash_fall_gate && fi.air.time_in_air <= DASH_FALL_TIME) || against);
    if gate && crate::fighter::is_attacking(w, f) {
        let flags = crate::fighter::control::attacking_flags(w, f);
        gate = flags_any(w, f, &flags, "AllowAirControl") || buff_thru_atk(w, f, BuffThruAtkK::Air);
    }
    let fi = w.fighter(f);
    let gate = gate && fi.air.air_control_gate && fi.moves.move_cmds.is_empty();
    w.fighter_mut(f).air.fall_lat_friction =
        if gate { if against { FALL_LATERAL_FRICTION } else { DASH_FALL_FRICTION } } else { 0.0 };
}

/// `lateralFrictionEvent`: the events that set AirControlGate and DashFallGate before CheckLateralFriction.
pub fn lateral_friction_event(w: &mut World, f: FighterId, air_gate: bool, dash_gate: bool) {
    let fi = w.fighter_mut(f);
    fi.air.air_control_gate = air_gate;
    fi.air.dash_fall_gate = dash_gate;
    check_lateral_friction(w, f);
}

/// `updateAirControl`: UpdateAirControl (BaseFighter @159399-159611).
pub fn update_air_control(w: &mut World, f: FighterId, v: Real) {
    let m = air_control_mult(w, f);
    let fi = w.fighter_mut(f);
    fi.air.air_control = if m == 1.0 { v } else { v * m };
    fi.air.air_boost_mult = m;
    grow_boost_threshold(w, f, m);
}

/// `tryJump`.
pub fn try_jump(w: &mut World, f: FighterId) -> bool {
    if try_wall_jump(w, f) {
        return true;
    }
    // Walking off keeps the ground jump, so the first jump in the air is a ground jump.
    let fi = w.fighter(f);
    if fi.body.on_floor || fi.jump.jumps == 0 {
        jump_stop_montage(w, f);
        do_jump(w, f, false);
        return true;
    }
    if crate::real(fi.jump.jumps) >= max_jumps(w, f) {
        return false;
    }
    // DoubleJumpAvailable: false at the ground jump press, Delay(0.15) sets it back.
    if fi.jump.ground_jump_tick >= 0 && fi.clock.tick - fi.jump.ground_jump_tick < latent(DOUBLE_JUMP_GATE) {
        return false;
    }
    jump_stop_montage(w, f);
    do_jump(w, f, true);
    true
}

/// `jumpStopMontage`: StopMontageClient = Montage_Stop(0.0, GetCurrentActiveMontage); a grouped instance over it
/// is the active montage then.
pub fn jump_stop_montage(w: &mut World, f: FighterId) {
    if let Some(e) = active_grouped(w, f) {
        stop_grouped_entry(w, f, e, 0.0, false);
        return;
    }
    stop_montage(w, f, Some(0.0), None);
    stop_dash_montage(w, f, 0.0);
}

/// `dashDance`: DashDanceClient -> DashDanceLogic + DashDanceMulticast.
pub fn dash_dance(w: &mut World, f: FighterId, skip_anim: bool) {
    let dir = sign(w.fighter(f).controls.inp.x);
    if dir == 0.0 {
        return;
    }
    // DashDanceMulticast @140038-140052: DashTriggered(IsDashAnim) ahead of HandleDashAnim.
    let nav = nav_of(w, f);
    let (dash, dash_turn) = (nav.dash.clone(), nav.dash_turn.clone());
    let dash_anim = dir == w.fighter(f).body.facing_dir || dash_turn.is_none(); // IsDashAnim
    if !w.fighter(f).buffs.buffs.is_empty() {
        buff_signal(
            w,
            f,
            BuffSignal::DashTriggered,
            BuffSignalArg { value: Some(!skip_anim && dash_anim), ..Default::default() },
        );
    }
    // IsDashAnimAllowed @42-593.
    let fi = w.fighter(f);
    let dm_ok = fi.montages.dash_montage.as_ref().is_none_or(|m| is_dash_anim(w, f, m.montage.name));
    if !skip_anim && fi.montages.montage.is_none() && dm_ok && active_grouped(w, f).is_none() {
        let d = &w.data.chars[w.names.str(fi.ch)].fighter;
        let rbs = d.anim_set.get("RunBaseSpeed").and_then(|v| v.as_f64()).unwrap_or(1500.0);
        // `f.ch.meshScale`: the scaled one (a polymorph's size).
        let rate = max(crate::fighter::max_walk(w, f) / crate::math::narrow(rbs) / fi.scaled.mesh_scale, DASH_RATE_MIN);
        let quick = (fi.clock.time - fi.air.dash_game_time) <= QUICK_DASH_TIME;
        let qdt = d.pawn.get("QuickDashTurnAnim").and_then(|v| v.as_str()).filter(|s| !s.is_empty()).map(str::to_owned);
        let turn = if quick && qdt.is_some() { qdt } else { dash_turn };
        let name = if dash_anim { dash } else { turn };
        let name = name.and_then(|s| w.names.get(&s));
        let section = if quick { crate::name!("Quick") } else { crate::name!("Default") };
        play_plain_montage(w, f, name, rate, SinkKind::Dash, Some(section));
    }
    let fi = w.fighter_mut(f);
    fi.air.dash_game_time = fi.clock.time; // @174041, after HandleDashAnim
    net_impulse(w, f, dir * DASH_IMPULSE, 0.0, true, false, 1.0, false, 0.0); // DashDanceLogic
    let fi = w.fighter_mut(f);
    fi.air.dash_tick = crate::real(fi.clock.tick);
    sfx(w, f, Some(crate::name!("sfx_shared_combat_runtrail"))); // SFX_RunTrail, after DashDanceMulticast
    lateral_friction_event(w, f, true, true);
}

/// `navJump`: Comp_Pawn_MontageNavigation JumpToSectionLogic(section, montage) (ubergraph @1492): a montage blending
/// out on its own jumps the active instance instead.
pub fn nav_jump(w: &mut World, f: FighterId, m: MontageSlot, section: Name) {
    if w.fighter(f).shapes.disguise.is_some() {
        // the copy's play jumps the active instance as one blending out does; the jump reaches the second mesh too
        let target =
            if slot_blending_out(w, f, m) || m == MontageSlot::Foreign { active_instance(w, f) } else { Some(m) };
        if let Some(t) = target {
            slot_jump_to_section(w, f, t, section);
        }
        crate::combat::disguise::mirror_section_jump(w, f, Some(section));
        return;
    }
    let target = if slot_blending_out(w, f, m) { active_instance(w, f) } else { Some(m) };
    if let Some(t) = target {
        slot_jump_to_section(w, f, t, section);
    }
}

/// `jumpHorizontalVel`: CalcHorizontalJumpVel: 0 standing still or within 0.25 s of hitstun ending (unless an evade
/// came later); otherwise the speed clamped to the dash-jump limit, half-way turned to the stick's side.
pub fn jump_horizontal_vel(w: &mut World, f: FighterId, ff: bool) -> Real {
    let fi = w.fighter(f);
    let since_hs = fi.clock.time - fi.hit.hitstun_end_time;
    let since_ev = fi.clock.time - fi.evade.last_evade_time;
    let vx = fi.body.vx;
    if vx == 0.0 || (since_ev > since_hs && since_hs <= JUMP_AFTER_HITSTUN) {
        return 0.0;
    }
    let mw = crate::fighter::max_walk(w, f);
    // @606-758: |Velocity.Y| >= MaxWalkSpeed + LocalDashJumpDetectionThreshold (25): EvadeJump.
    if vx.abs() >= (mw + DASH_JUMP_NO_METER_ADD) {
        tutorial_event_of(w, f, crate::fighter::match_side::TutorialEvent::EvadeJump);
    }
    let fi = w.fighter(f);
    let lim = if fi.evade.last_evade_spent_meter {
        DASH_JUMP_MULT * mw
    } else {
        (mw + DASH_JUMP_NO_METER_ADD) * DASH_JUMP_NO_METER_MULT
    };
    let c = max(-lim, min(lim, fi.body.vx));
    // CalcHorizontalJumpVel @1118-1854: the factor is MoveX's sign, not its size.
    let v = c + (c.abs() * sign(fi.controls.inp.x) - c) * JUMP_CONTROL_OVERRIDE;
    if ff { v * 1.2 } else { v }
}

/// `doJump`: the ground and the double jump.
pub fn do_jump(w: &mut World, f: FighterId, air: bool) {
    let fi = w.fighter_mut(f);
    fi.fast_fall.ff_mult =
        if air { fi.fast_fall.ff_triggered } else { fi.fast_fall.ff_triggered || fi.jump.crouch_jump };
    crate::fighter::control::add_recent_attack(w, f, Btn::Jump, None);
    let fi = w.fighter_mut(f);
    let ff = fi.fast_fall.ff_mult;
    fi.fast_fall.ff_triggered = false;
    fi.jump.last_jump_time = fi.clock.time;
    let v = jump_velocity(w, f) * (if ff { 1.05 } else { 1.0 });
    if air {
        if ff {
            tutorial_event_of(w, f, crate::fighter::match_side::TutorialEvent::FastFallJump); // @6888
        }
        let hv = jump_horizontal_vel(w, f, ff);
        net_impulse(w, f, hv, v, true, true, 2.0, false, 0.0);
        tutorial_event_of(w, f, crate::fighter::match_side::TutorialEvent::DoubleJump); // @7283
    } else {
        w.fighter_mut(f).moves.ground_jump = Some(GroundJump { ff, v });
        tutorial_event_of(w, f, crate::fighter::match_side::TutorialEvent::Jump); // @1326
    }
    w.fighter_mut(f).jump.jump_v = v;
    crate::fighter::set_jumping(w, f, true);
    let fi = w.fighter_mut(f);
    fi.jump.jumps = if air { fi.jump.jumps + 1 } else { 1 };
    fi.air.air_attack_available = true;
    if !air {
        fi.jump.ground_jump_tick = fi.clock.tick;
    }
    fi.jump.jump_frame = 0;
    fi.jump.jump_released = false;
    if air {
        fi.jump.jump_stop_gate = true;
    }
    // CalcJumpStopTime; with FastfallMult the height x FastFallJumpBoostMult.
    let h = (if air { double_jump_height(w, f) } else { jump_height(w, f) })
        * (if ff { FAST_FALL_JUMP_BOOST } else { 1.0 });
    let t = calc_jump_stop_time(w, f, v, h);
    w.fighter_mut(f).jump.jump_cut_at = latent(t);
    if let Some(h) = w.fighter(f).hooks.char.end_jump {
        h(w, f); // a pawn's EndJump override
    }
    let fi = w.fighter_mut(f);
    fi.jump.jump_min_time = if air { DOUBLE_JUMP_MIN_TIME } else { JUMP_MIN_TIME };
    fi.jump.jump_start_time = fi.clock.time;
    fi.jump.quick_fall_at = -1; // BreakQuickFall
    fi.fast_fall.fast_falling = false;
    fast_fall_end(w, f); // IsFastFalling = false: jump @135811, double jump @6792
    lateral_friction_event(w, f, true, false); // JumpMulticast / DoubleJumpMulticast
    move_fx(w, f, if air { crate::events::MoveFxKind::DoubleJump } else { crate::events::MoveFxKind::Jump });
    if air {
        handle_air_move_desaturation(w, f); // HandleAirMoveDesaturation @8383
    }
}

/// `calcJumpStopTime`: T = (v - sqrt(max(v^2 - 2g(H - 1.5 vs^2/g), 0)))/g, vs = JumpStopVelocityMultiplier x v.
fn calc_jump_stop_time(w: &World, f: FighterId, v: Real, h: Real) -> Real {
    let g = GRAVITY_Z * GRAVITY_SCALE;
    let vs = jump_stop_mult(w, f) * v;
    (v - max(v * v - 2.0 * g * (h - (1.5 * vs * vs) / g), 0.0).sqrt()) / g
}

/// `cancelJump`: BaseFighter CancelJump (ubergraph 100320): Jumping = false; with LastJumpVelocity > JumpVelocity,
/// LastJumpVelocity = JumpVelocity and LaunchCharacter((0, 0, JumpVelocity), false, true) (stock FUN_142902f70).
// Unverified: no capture of a hit on a fighter whose last ground jump was boosted.
pub fn cancel_jump(w: &mut World, f: FighterId) {
    crate::fighter::set_jumping(w, f, false);
    let jv = jump_velocity(w, f);
    #[allow(clippy::neg_cmp_op_on_partial_ord, reason = "the negated comparison !(lastJumpV > v): a NaN returns")]
    if !(w.fighter(f).moves.last_jump_v > jv) {
        return;
    }
    let fi = w.fighter_mut(f);
    fi.moves.last_jump_v = jv;
    fi.moves.impulse = Some(Impulse { vx: fi.body.vx, vy: jv, xy: true, z: true, kind: 0, d: None });
    fi.moves.hit_launch = true;
}

/// `issueGroundJump`: the pawn tick's ground jump (HandleNewJumpMovement).
// Deviation: the hold timer runs only while a tutorial runs (its one effect is the HoldJump event).
// Unverified: JumpFromGround's move-command term (GetMoveCommandsCurrentDirection Z > 0, @135253) is left out.
pub fn issue_ground_jump(w: &mut World, f: FighterId) {
    let fi = w.fighter_mut(f);
    fi.moves.jump_launched = false; // 139047
    let Some(j) = fi.moves.ground_jump.take() else { return };
    if tutorial_world(w, f) {
        if !(is_falling(w, f) || is_flying(w, f) || w.fighter(f).hit.kb_mode) {
            let t = calc_jump_stop_time(w, f, j.v, jump_height(w, f)) - 0.1;
            w.fighter_mut(f).jump.jump_hold_timer = if t > 0.0 {
                Some(HoldTimer { left: -1.0, ticks: to_int32(crate::combat::hit::timer_ticks(Some(t))) })
            } else {
                None
            };
        }
        if j.ff {
            tutorial_event_of(w, f, crate::fighter::match_side::TutorialEvent::FastFallJump);
        }
    }
    let fi = w.fighter_mut(f);
    fi.jump.jump_stop_gate = true; // ubergraph 135800
    fi.moves.jump_launched = true;
    fi.moves.last_jump_v = j.v; // 135780 -> 69632-69664
    let hv = jump_horizontal_vel(w, f, j.ff);
    net_impulse(w, f, hv, j.v, true, true, 2.0, false, 0.0);
}

/// `jumpHoldTimerTick`: JumpHoldTimer in the frame's timer pass; HandleHoldJumpTutorialEvent @162525: HoldJump.
pub fn jump_hold_timer_tick(w: &mut World, f: FighterId) {
    let fi = w.fighter_mut(f);
    let Some(t) = &mut fi.jump.jump_hold_timer else { return };
    if t.left < 0.0 {
        t.left = crate::real(t.ticks);
        return;
    }
    t.left -= 1.0;
    if t.left > 0.0 {
        return;
    }
    fi.jump.jump_hold_timer = None;
    tutorial_event_of(w, f, crate::fighter::match_side::TutorialEvent::HoldJump);
}

/// `jumpTimers`: jump release, EndJump and the quick fall, as jumpFrame counted from the jump's frame.
pub fn jump_timers(w: &mut World, own: FighterId, attacking: bool) {
    let s = own;
    let fi = w.fighter_mut(s);
    if fi.jump.jump_frame < 0 {
        return;
    }
    let f = fi.jump.jump_frame;
    let mut from_release = false;
    if !fi.controls.inp.jump && !fi.jump.jump_released {
        // ButtonRelease: elapsed = GetTimeSeconds - JumpStartTime (float32).
        fi.jump.jump_released = true;
        if fi.jump.jump_stop_gate {
            fi.jump.end_jump_latent = false;
            let elapsed = fi.clock.time - fi.jump.jump_start_time;
            let mn = fi.jump.jump_min_time;
            #[allow(clippy::neg_cmp_op_on_partial_ord, reason = "the negated comparison !(elapsed < min)")]
            {
                from_release = !(elapsed < mn);
            }
            fi.jump.jump_cut_at = if from_release { f } else { f + latent(mn - elapsed) };
        }
    }
    if fi.jump.jump_cut_at >= 0 && f >= fi.jump.jump_cut_at {
        fi.jump.jump_cut_at = -1;
        fi.jump.jump_hold_timer = None; // K2_ClearAndInvalidateTimerHandle @14435
        crate::fighter::set_jumping(w, s, false);
        let fi = w.fighter_mut(s);
        fi.platforms.dropping_down = false;
        fi.platforms.evading_through_platform = false;
        if fi.moves.gravity_mod_count > 0 {
            fi.jump.quick_fall_at = -1;
        } else if fi.jump.jump_stop_gate {
            let vs = jump_stop_mult(w, s) * w.fighter(s).jump.jump_v;
            let fi = w.fighter(s);
            if !fi.fast_fall.fast_falling
                && !crate::fighter::in_hitstun(w, s)
                && fi.life.dead <= 0.0
                && !fi.jump.block_jump_end_launch
            {
                cut_impulse(w, s, vs);
            }
            let fi = w.fighter_mut(s);
            fi.jump.jump_stop_gate = false;
            fi.jump.quick_fall_at =
                f + latent((2.0 * vs) / (GRAVITY_Z * GRAVITY_SCALE)) + (if from_release { 1 } else { 0 });
        }
    }
    let fi = w.fighter(s);
    if fi.jump.quick_fall_at >= 0 && f >= fi.jump.quick_fall_at {
        w.fighter_mut(s).jump.quick_fall_at = -1;
        // Additive NetImpulse(0, 0, -1200, type 6) (@16835 / @187942).
        let fi = w.fighter(s);
        if !fi.fast_fall.fast_falling
            && !attacking
            && !crate::fighter::in_hitstun(w, s)
            && fi.life.dead <= 0.0
            && !fi.jump.block_jump_end_launch
            && !fi.body.on_floor
        {
            net_impulse(w, s, 0.0, QUICK_FALL_VELOCITY, false, false, 6.0, false, 0.0);
        }
    }
    w.fighter_mut(s).jump.jump_frame += 1;
}

/// `fastFallBlock`: BlockFastFallStacks > 0 or BlockFastFallTime > 0.
fn fast_fall_block(w: &World, f: FighterId) -> bool {
    fast_fall_stacks(w, f) || w.fighter(f).fast_fall.block_ff_time.unwrap_or(0.0) > 0.0
}

/// `fastFallStacks`: the BPI_SetBlockFastFall callers.
// Deviation: BPI_SetBlockFastFall writes one bool, so the build keeps the last writer's; the port ORs the writers.
fn fast_fall_stacks(w: &World, f: FighterId) -> bool {
    has_active(w, f, crate::name!("AnimNotify_BlockFastFall_C"), -1.0)
        || crate::chars::iron_giant::giant_hover_fast_fall_block(w, f)
        || fly_is_flying(w, f)
        || buff_block_fast_fall(w, f)
}

/// `fastFallTick`: Tick > HandleFastFallTrigger, then HandleBlockFastFallTime. `late`: called inside `physics`,
/// ahead of the frame's OnMovementUpdated.
pub fn fast_fall_tick(w: &mut World, f: FighterId, attacking: bool, late: bool) {
    let stacks = fast_fall_stacks(w, f);
    let fi = w.fighter_mut(f);
    let block = stacks || fi.fast_fall.block_ff_time.unwrap_or(0.0) > 0.0;
    // BPI_SetBlockFastFall (@98075-98337) broadcasts OnIsFastFalling(false) at each call that leaves it set.
    if stacks && fi.fast_fall.fast_falling {
        fi.fast_fall.anim_fast_fall_off = Some(true);
    } else if !fi.fast_fall.fast_falling && fi.fast_fall.anim_fast_fall_off.is_some() {
        fi.fast_fall.anim_fast_fall_off = None;
    }
    fast_fall_handler(w, f, block, attacking, late);
    let fi = w.fighter_mut(f);
    if let Some(t) = fi.fast_fall.block_ff_time
        && t > 0.0
    {
        fi.fast_fall.block_ff_time = Some(max(0.0, t - DELTA_SECONDS));
    }
}

fn fast_fall_handler(w: &mut World, f: FighterId, block: bool, attacking: bool, late: bool) {
    let fi = w.fighter_mut(f);
    if fi.platforms.down_window_tick >= 0 && fi.clock.tick >= fi.platforms.down_window_tick {
        // DownReleasedLogic's Delay(0.3)
        fi.platforms.down_window_tick = -1;
        fi.platforms.down_press_count = 0;
        fi.platforms.down_press_active = false;
    }
    // DownPressed / DownReleased called directly (the bot's crouch tasks).
    let btn = fi.controls.inp.down_btn;
    if btn != fi.crouch.down_btn {
        fi.crouch.down_btn = btn;
        if btn {
            down_pressed(w, f, block, attacking, late);
        } else {
            down_released(w, f, block, attacking, late);
        }
    }
    let fi = w.fighter(f);
    let my = fi.controls.inp.y;
    let down = my <= -0.4;
    if fi.crouch.down_stick {
        let d = &w.data.chars[w.names.str(fi.ch)].fighter;
        let ccw = d.pawn.get("CanCrouchWalk").is_some_and(super::physics::truthy);
        if my > (if ccw { -0.1 } else { -0.4 }) {
            w.fighter_mut(f).crouch.down_stick = false;
            down_released(w, f, block, attacking, late);
        } else if !fi.fast_fall.ff_triggered {
            down_pressed(w, f, block, attacking, late);
        }
    } else if down {
        let fi = w.fighter_mut(f);
        fi.fast_fall.ff_triggered = false;
        fi.crouch.down_stick = true;
        down_pressed(w, f, block, attacking, late);
    }
}

/// `downPressed`: DownPressedLogic.
// Deviation: Was Attacking During Fastfall is written only while a tutorial runs.
pub fn down_pressed(w: &mut World, f: FighterId, block: bool, attacking: bool, late: bool) {
    // (GetMovementEnabled in the same test, @117331)
    if crate::fighter::in_hitstun(w, f) || w.fighter(f).life.dead > 0.0 || !super::movement_enabled(w, f) {
        return;
    }
    w.fighter_mut(f).crouch.pushing_down = true;
    if tutorial_world(w, f) {
        w.fighter_mut(f).fast_fall.was_attacking_during_fastfall = Some(attacking);
    }
    let fi = w.fighter(f);
    if !fi.body.on_floor || fi.moves.cmd_jumped || fi.moves.ground_jump.is_some() {
        if fi.platforms.dropping_down {
            // Dropping through a platform (3663 -> 3828): the test at once, without the Delay (@3895).
            if (fi.clock.time - fi.jump.last_jump_time) >= FAST_FALL_JUMP_GAP && !block {
                w.fighter_mut(f).fast_fall.ff_triggered = true;
                trigger_fast_fall(w, f, block, late);
            }
        } else if fi.fast_fall.ff_check_tick < 0 {
            let at = fi.clock.tick + latent(FAST_FALL_DELAY) - 1;
            w.fighter_mut(f).fast_fall.ff_check_tick = at;
        }
    } else if !attacking && fi.platforms.down_press_active && fi.controls.prefs.double_crouch_jump_down {
        w.fighter_mut(f).platforms.down_press_count += 1;
    }
}

/// `fastFallDelayFire`: the FastFallDelay's body (DownPressed's air path @3713-4219 -> TriggerFastFall @292-434),
/// in the pawn's latent pass.
pub fn fast_fall_delay_fire(w: &mut World, f: FighterId) {
    let fi = w.fighter(f);
    if fi.fast_fall.ff_check_tick < 0 || fi.clock.tick < fi.fast_fall.ff_check_tick {
        return;
    }
    w.fighter_mut(f).fast_fall.ff_check_tick = -1;
    let block = fast_fall_block(w, f);
    let fi = w.fighter(f);
    if fi.crouch.pushing_down && (fi.clock.time - fi.jump.last_jump_time) >= FAST_FALL_JUMP_GAP && !block {
        w.fighter_mut(f).fast_fall.ff_triggered = true;
        trigger_fast_fall(w, f, block, false);
    }
}

/// `downReleased`: DownReleasedLogic.
pub fn down_released(w: &mut World, f: FighterId, block: bool, attacking: bool, late: bool) {
    let fi = w.fighter_mut(f);
    fi.crouch.pushing_down = false;
    fi.platforms.dropping_down = false;
    set_crouching(w, f, false);
    let fi = w.fighter_mut(f);
    fi.platforms.down_press_active = true;
    if fi.platforms.down_window_tick < 0 {
        fi.platforms.down_window_tick = fi.clock.tick + latent(0.3);
    }
    if fi.controls.prefs.fastfall == 1 {
        return; // 1 Tap
    }
    let fi = w.fighter(f);
    if fi.fast_fall.fast_falling && !crate::fighter::in_hitstun(w, f) && !block && !attacking && fi.life.dead <= 0.0 {
        // E_FastfallType 0 Hold: NetImpulse(Z = FastFallStopSpeed, Z override, type 5).
        if !fi.body.on_floor {
            net_impulse(w, f, 0.0, FAST_FALL_STOP_SPEED, false, true, 5.0, late, 0.0);
        }
        w.fighter_mut(f).fast_fall.fast_falling = false; // @3405
        fast_fall_end(w, f);
        set_terminal(w, f, true, 0.0, false);
    }
}

/// `fastFallStart`: FastFallStartLocation, kept only on a fighter with a buff of a class bound to FastFallLanded.
// Deviation: a binder given in the middle of a fast fall lands without the broadcast.
pub fn fast_fall_start(w: &mut World, f: FighterId) {
    let bound = w.fighter(f).buffs.buffs.iter().any(|b| b.frames > 0.0 && fast_fall_landed_binder(b.cls));
    if bound {
        let y = crate::fighter::body::actor_y(w, f);
        w.fighter_mut(f).fast_fall.fast_fall_start_y = Some(y);
    }
}

/// `fastFallEnd`.
pub fn fast_fall_end(w: &mut World, f: FighterId) {
    w.fighter_mut(f).fast_fall.fast_fall_start_y = None;
}

/// `triggerFastFall`: TriggerFastFall; gate: in the air and !(BlockFastFall || IsWallSliding).
pub fn trigger_fast_fall(w: &mut World, f: FighterId, block: bool, late: bool) {
    if tutorial_world(w, f) {
        tutorial_event_of(w, f, crate::fighter::match_side::TutorialEvent::FastFall); // @100
        if w.fighter(f).fast_fall.was_attacking_during_fastfall == Some(true) {
            tutorial_event_of(w, f, crate::fighter::match_side::TutorialEvent::FastfallDuringAttack); // @253
        }
    }
    let fi = w.fighter_mut(f);
    if fi.body.on_floor || block || fi.wall.wall_sliding {
        return;
    }
    fi.fast_fall.fast_falling = true;
    fi.fast_fall.anim_fast_fall_off = None; // OnIsFastFalling(IsFastFalling) @645
    fast_fall_start(w, f); // @673-701
    move_fx(w, f, crate::events::MoveFxKind::FastFall); // SFX_FastFall
    let ffs = fast_fall_speed(w, f);
    if w.fighter(f).body.vy <= -ffs {
        return;
    }
    set_terminal(w, f, false, ffs, true);
    // Hitstun: the HitstunHandler turned FMC CanImpulse off (+0xed4 bit 0).
    if crate::fighter::in_hitstun(w, f) {
        return;
    }
    net_impulse(w, f, 0.0, -ffs, false, true, 4.0, late, 0.0);
}

/// `capFall`: NewFallVelocity (FMC +0x6a8, 0x1410d4780): the part along the gravity direction capped at
/// TerminalVelocity, following the scale's sign.
pub fn cap_fall(w: &World, f: FighterId, vy: Real) -> Real {
    let fi = w.fighter(f);
    let s = fi.moves.gravity_scale;
    let t = fi.hit.terminal_v;
    if s > 0.0 {
        return if vy < -t { -t } else { vy };
    }
    if s < 0.0 && vy > t { t } else { vy }
}

/// `setTerminal`: SetTerminalVelocity(Reset, Velocity, OnlyIfGreater).
pub fn set_terminal(w: &mut World, f: FighterId, reset: bool, v: Real, only_if_greater: bool) {
    let t = if reset {
        crate::fighter::common::default_terminal(&w.data.chars[w.names.str(w.fighter(f).ch)])
    } else if only_if_greater {
        max(w.fighter(f).hit.terminal_v, v.abs())
    } else {
        v.abs()
    };
    w.fighter_mut(f).hit.terminal_v = t;
}
