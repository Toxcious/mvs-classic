//! NetImpulse, the move commands and the root motion sources.

use super::*;
use crate::actors::ProjectileId;
use crate::combat::buffs::buff_signal;
use crate::consts::{GRAVITY_SCALE, MAX_ACCEL, SUBSTEPS, WALL_SKATE_TRACE_REACH};
use crate::fighter::body::{actor_y, cap_half, def, impart_base, set_grounded};
use crate::math::ue;
use crate::math::{max, min, sign, to_int32};
use crate::registry::entries::ActorHooks;
use crate::registry::signals::{BuffSignal, BuffSignalArg};
use mvs_data::value::Value;

/// Command_Movement FindNearAngleDirection: the difference (A1 + 180) - (A2 + 180) brought to the nearest turn (the
/// graph's adds @0-84, which round in float where A1 - A2 would not).
fn near_angle(a1: Real, a2: Real) -> (Real, bool, Real) {
    let d = (a1 + 180.0) - (a2 + 180.0);
    let mut sel = if d.abs() < (d - 360.0).abs() { d } else { d - 360.0 };
    #[allow(clippy::neg_cmp_op_on_partial_ord, reason = "the negated comparison !(|sel| < |d + 360|)")]
    if !(sel.abs() < (d + 360.0).abs()) {
        sel = d + 360.0;
    }
    // Round (142f817a0) then Conv_IntToFloat.
    (if sel < 0.0 { 1.0 } else { -1.0 }, sel < 0.0, crate::math::real(ue::round_to_int(sel)))
}

/// RotateAngleAxis((0, Y, Z), `deg`, (1, 0, 0)) (FMath::SinCos, 14136dfb0): the angle atan2(Y, Z) goes down by `deg`.
fn rot_yz(y: Real, z: Real, deg: Real) -> (Real, Real) {
    let r = ue::rotate_angle_axis([0.0, y, z], deg, [1.0, 0.0, 0.0]);
    (r[1], r[2])
}

/// Normal((0, a, b), 0.0001) (142f7adf0).
fn unit(a: Real, b: Real) -> (Real, Real) {
    let n = ue::normal([0.0, a, b], 1e-4);
    (n[1], n[2])
}

/// `netImpulse`: NetImpulse, stored for the next OnMovementUpdated. `late`: issued inside `physics` for this
/// frame's pawn tick, so it misses this frame's consumption. `depth`: the impulse's UE X.
// Unverified: every caller but AnimNotify_LaunchSelf's own launch is taken as passing X = 0.
#[allow(clippy::too_many_arguments, reason = "the call's parameters")]
pub fn net_impulse(
    w: &mut World,
    f: FighterId,
    vx: Real,
    vy: Real,
    xy: bool,
    z: bool,
    kind: Real,
    late: bool,
    depth: Real,
) {
    let im = Impulse { vx, vy, xy, z, kind: to_int32(kind), d: if depth != 0.0 { Some(depth) } else { None } };
    let fi = w.fighter_mut(f);
    if late {
        fi.moves.net_late = Some(im);
    } else {
        fi.moves.net = Some(im);
    }
}

/// `onMovementUpdated`: OnMovementUpdated (FUN_1410d5550), after the CMC's move: the stored NetImpulse becomes the
/// pending launch unless FMC CanImpulse is off (hitstun), which drops it; a stored NetFriction is applied.
pub fn on_movement_updated(w: &mut World, f: FighterId) {
    let stun = crate::fighter::in_hitstun(w, f);
    let fi = w.fighter_mut(f);
    if fi.moves.net.is_some() && !stun {
        fi.moves.impulse = fi.moves.net.take();
        fi.hit.kb_mode = false; // (Custom, 2) -> Falling
    }
    fi.moves.net = fi.moves.net_late.take();
    if let Some(nf) = fi.moves.net_friction.take() {
        set_net_friction(w, f, nf);
    }
    native_braking(w, f); // MoveCommandBraking into the braking decelerations
    apply_net_gravity(w, f);
}

/// `OVERRIDE_GRAVITY_SCALE`: OnMovementUpdated's GravityScale under OverrideGravity, the byte +0xec4 as a float.
pub const OVERRIDE_GRAVITY_SCALE: Real = 1.0;

/// `applyNetGravity`: a stored NetGravity is taken, else with the override byte set GravityScale = 1.0
/// (FUN_1410d5550 @1410d5ae0-1410d5b08).
pub fn apply_net_gravity(w: &mut World, f: FighterId) {
    let m = &mut w.fighter_mut(f).moves;
    if let Some(g) = m.net_gravity {
        m.gravity_scale = g;
    } else if m.gravity_override {
        m.gravity_scale = OVERRIDE_GRAVITY_SCALE;
    }
    m.net_gravity = None;
}

/// `overrideGravity`: UFighterMovementComponent OverrideGravity (exec 141252450): the byte +0xec4 = 1.
pub fn override_gravity(w: &mut World, f: FighterId) {
    w.fighter_mut(f).moves.gravity_override = true;
}

/// `clearGravityOverride`: ClearGravityOverride (exec 1412510e0): GravityScale = the kept 5.25, the byte 0.
pub fn clear_gravity_override(w: &mut World, f: FighterId) {
    let m = &mut w.fighter_mut(f).moves;
    m.gravity_scale = GRAVITY_SCALE;
    m.gravity_override = false;
}

/// `dropImpulses`: the pending launches only; a stored NetFriction is left for the next OnMovementUpdated.
// Unverified: that the CanImpulse-off branch of FUN_1410d5550 leaves +0xb88 / +0xb8c set is not read on its own.
pub fn drop_impulses(w: &mut World, f: FighterId) {
    let m = &mut w.fighter_mut(f).moves;
    m.net = None;
    m.net_late = None;
    m.impulse = None;
    m.hit_launch = false;
    m.ground_jump = None;
}

/// `applyImpulse`: HandlePendingLaunch before the CMC's move (FUN_1410d5550 -> Launch, the fighter's override
/// FUN_1410c8e70): each axis replaces the velocity when overridden, else adds to it.
// Unverified: the plane constraint taking a depth velocity off the move is not read natively.
pub fn apply_impulse(w: &mut World, f: FighterId) {
    let fi = w.fighter_mut(f);
    let Some(im) = fi.moves.impulse.take() else { return };
    fi.moves.hit_launch = false;
    let lx = if im.xy { im.vx } else { fi.body.vx + im.vx };
    let launch_z = if im.z { im.vy } else { fi.body.vy + im.vy }; // PendingLaunchVelocity.Z (+0x3cc)
    // HandlePendingLaunch acts only when PendingLaunchVelocity is not zero.
    if lx == 0.0 && launch_z == 0.0 && im.d.is_none_or(|d| d == 0.0 || d.is_nan()) {
        return;
    }
    fi.body.vx = lx;
    fi.body.vy = launch_z;
    if fi.body.on_floor && launch_z != 0.0 {
        // SetMovementMode(Falling) off a moving base adds its velocity to the launch.
        let b = fi.platforms.base;
        set_grounded(w, f, false);
        if let Some(b) = b {
            impart_base(w, f, b);
        }
    }
    if launch_z != 0.0 {
        w.fighter_mut(f).moves.flying = false;
    }
}

/// `pendingLaunchVector`: FMC HasPendingLaunch / GetPendingLaunchVector.
pub fn pending_launch_vector(w: &World, f: FighterId) -> Option<(Real, Real)> {
    let fi = w.fighter(f);
    fi.moves
        .impulse
        .as_ref()
        .map(|im| (if im.xy { im.vx } else { fi.body.vx + im.vx }, if im.z { im.vy } else { fi.body.vy + im.vy }))
}

/// `clampVel`: the shared speed clamp (LaunchSelf, the manager's end): forward speed in [min.X, max.X], vertical
/// in [min.Y, max.Y], a 0 bound open; an all-zero result becomes (0, 0.1). `k` is the multiplier (1 by default).
pub fn clamp_vel(w: &World, f: FighterId, mn: V2, mx: V2, k: Real) -> (Real, Real) {
    let lo = if mn.x == 0.0 { Real::NEG_INFINITY } else { mn.x };
    let hi = if mx.x == 0.0 { Real::INFINITY } else { mx.x };
    let zlo = if mn.y == 0.0 { Real::NEG_INFINITY } else { mn.y };
    let zhi = if mx.y == 0.0 { Real::INFINITY } else { mx.y };
    let fi = w.fighter(f);
    let (sx, sy) = pending_launch_vector(w, f).unwrap_or((fi.body.vx, fi.body.vy));
    let fd = fi.body.facing_dir;
    let vx = max(lo, min(hi, sx * fd)) * fd * k;
    let vy = max(zlo, min(zhi, sy)) * k;
    if vx.abs() <= 1e-4 && vy.abs() <= 1e-4 { (0.0, 0.1) } else { (vx, vy) }
}

/// `launchOnHit`: AnimNotify_LaunchSelf_AutoGenFunc (@0-2501), run at its owner's HitEnemyMulticast.
pub fn launch_on_hit(w: &mut World, f: FighterId) {
    let fi = w.fighter(f);
    let Some(p) = &fi.hit.launch_on_hit else { return };
    let (clamp, mn, mx, cff, dy, dz) = (p.speed_clamp, p.min, p.max, p.can_fast_fall, p.dir_y, p.dir_z);
    if clamp {
        let (vx, vy) = clamp_vel(w, f, mn, mx, 1.0);
        net_impulse(w, f, vx, vy, true, true, 0.0, false, 0.0);
    } else if !(fi.crouch.pushing_down && cff) {
        let fd = fi.body.facing_dir;
        let (xy, z) = crate::combat::hit::launch_on_hit_overrides(w, f);
        net_impulse(w, f, dy * fd, dz, xy, z, 0.0, false, 0.0);
    }
}

fn prop_num(p: &Props, k: &str) -> Real {
    crate::math::narrow(p.get(k).and_then(Value::as_f64).unwrap_or(0.0))
}

fn prop_truthy(p: &Props, k: &str) -> bool {
    p.get(k).is_some_and(truthy)
}

/// `adjustStickInfluence`: AnimNotify_MoveCommand.AdjustStickInfluence: the stick digitised at 0.3 relative to
/// the facing.
pub fn adjust_stick_influence(w: &World, f: FighterId, p: &Props) -> V2 {
    let mv = p
        .get("MoveVector")
        .and_then(Value::as_obj)
        .map_or(V2::default(), |o| V2 { x: prop_num(o, "X"), y: prop_num(o, "Y") });
    // VSize2D(AdjustedVector) @415.
    let len = ue::size2d([mv.x, mv.y]);
    let fi = w.fighter(f);
    let fx = fi.controls.inp.x * fi.body.facing_dir;
    let sx = if fx >= 0.3 {
        1.0
    } else if fx <= -0.3 {
        -1.0
    } else {
        0.0
    };
    let iy = fi.controls.inp.y;
    let sy = if iy >= 0.3 {
        1.0
    } else if iy <= -0.3 {
        -1.0
    } else {
        0.0
    };
    if prop_truthy(p, "StickOverridesDirection") && (sx != 0.0 || sy != 0.0) {
        // Normal2D (142f7acd0) x VSize2D @702-777.
        let n = ue::safe_normal2d([sx, sy]);
        return V2 { x: n[0] * len, y: n[1] * len };
    }
    let side = |s: Real, pos: &str, neg: &str| {
        if s > 0.0 {
            prop_num(p, pos)
        } else if s < 0.0 {
            -prop_num(p, neg)
        } else {
            0.0
        }
    };
    let x = if prop_truthy(p, "XStickMoveInfluence") {
        mv.x + side(sx, "XStickInfluencePos", "XStickInfluenceNeg")
    } else {
        mv.x
    };
    let y = if prop_truthy(p, "YStickMoveInfluence") {
        mv.y + side(sy, "YStickInfluencePos", "YStickInfluenceNeg")
    } else {
        mv.y
    };
    if prop_truthy(p, "UseAdditiveStickInfluence") {
        return V2 { x, y };
    }
    // Normal2D x VSize2D @1587-1624.
    let n = ue::safe_normal2d([x, y]);
    V2 { x: n[0] * len, y: n[1] * len }
}

/// `addMoveCmd`: Comp_Actor_MoveCommandManager add: the launch through NetImpulse(MoveDelta / Total, XY, Z, 7) and
/// NetFriction(0), both taken after the next CMC's move. MoveDelta = MoveVector x CharacterSpeedModifier x
/// max(GetChargeValue(3), 1) (ubergraph @5155, @5603-6149); then BPI_SetTerminalVelocity(false, VSize(MoveDelta),
/// OnlyIfGreater) (@7161-8037).
#[allow(clippy::too_many_arguments, reason = "the call's parameters")]
pub fn add_move_cmd(
    w: &mut World,
    f: FighterId,
    idx: i32,
    total: Real,
    mv: V2,
    xy: bool,
    z: bool,
    end: &MoveEnd,
    block_down: bool,
    wall_skate: bool,
    ctl: &MoveCtl,
) {
    if crate::fighter::in_hitstun(w, f) {
        return;
    }
    let t = max(total, 1e-6);
    let sm = w.fighter(f).buffs.speed_mod * max(crate::combat::elem::charge_movement(w, f), 1.0);
    let fi = w.fighter_mut(f);
    let dy = (if block_down { max(mv.y, 0.0) } else { mv.y }) * sm;
    let vx = (mv.x * sm * fi.body.facing_dir) / t;
    fi.moves.move_cmds.push(MoveCmd {
        end: end.clone(),
        idx,
        total: t,
        cur: 0.0,
        vx,
        vy: dy / t,
        bx: vx,
        by: dy / t,
        wall_skate,
        corner: false,
        up_gate: true,
        side_gate: false,
        old_foot: false,
        ctl: ctl.clone(),
    });
    if wall_skate {
        fi.moves.wall_skate = true; // AddMoveCommandNotify: BPI_SetWallSkate(true)
    }
    fi.moves.move_end = Some(end.clone());
    set_terminal(w, f, false, ue::size2d([mv.x * sm, mv.y * sm]), true); // @7987-8037, before BlockDownMovement
    check_lateral_friction(w, f); // MoveCommandStart
    net_impulse(w, f, vx, dy / t, xy, z, 7.0, false, 0.0);
    w.fighter_mut(f).moves.net_friction = Some(true); // NetFriction(0.0)
    crate::fighter::set_jumping(w, f, false);
    w.fighter_mut(f).jump.jump_frame = -1; // SetJumping(false), JumpStopGate(false)
}

/// `completeRemoveMove`: Comp_Actor_MoveCommandManager CompleteMoveCommand (@8546): CompleteRemoveMove, then the
/// broadcast of MoveCommandCompleted (@8569 -> @10), whether a command was found or not.
pub fn complete_remove_move(w: &mut World, f: FighterId, idx: i32) {
    let i = w.fighter(f).moves.move_cmds.iter().position(|c| c.idx == idx);
    if let Some(i) = i {
        let m = &mut w.fighter_mut(f).moves;
        if m.move_cmds[i].wall_skate {
            m.wall_skate = false; // BPI_SetWallSkate(false)
        }
        remove_move_cmd(w, f, i);
    }
    move_command_completed(w, f, i.is_some());
}

/// The broadcast @10: the buffs' binders, then the projectiles' (live when it begins).
// Deviation: the delegate's own order (bind order) is not kept between a buff's binding and a projectile's.
fn move_command_completed(w: &mut World, f: FighterId, found: bool) {
    buff_signal(w, f, BuffSignal::MoveCommandCompleted, BuffSignalArg { found: Some(found), ..Default::default() });
    let ids: Vec<ProjectileId> = w.actors.projectiles.ids().collect();
    for id in ids {
        let Some(pr) = w.actors.projectiles.get(id) else { continue };
        if pr.dead {
            continue;
        }
        let h = crate::registry::registry().actors.get(w.names.str(pr.cls)).and_then(|e| match &e.hooks {
            ActorHooks::Projectile(p) => p.move_command_completed,
            _ => None,
        });
        if let Some(h) = h {
            h(w, id, f, found);
        }
    }
}

/// `removeMoveCmd`.
pub fn remove_move_cmd(w: &mut World, f: FighterId, i: usize) {
    let m = &mut w.fighter_mut(f).moves;
    let c = m.move_cmds.remove(i);
    m.move_end = Some(c.end);
}

/// `moveCommandsReset`: a new pawn's Comp_Actor_MoveCommandManager and the FMC values the commands write.
pub fn move_commands_reset(w: &mut World, f: FighterId) {
    let fi = w.fighter_mut(f);
    fi.moves.move_cmds.clear();
    fi.branching.pending_move_ends.clear();
    fi.moves.move_end = None;
    set_net_friction(w, f, false);
    native_braking(w, f);
    let m = &mut w.fighter_mut(f).moves;
    m.net_friction = None;
    m.net = None; // the FMC's stored NetImpulse
    m.net_late = None;
    m.flying = false;
    m.fly_input = None;
    m.fluid_friction = 0.0;
}

/// `tickMoveCmds`: the manager tick: the command timers; the first tick that finds the list empty runs the end
/// logic once. In hitstun every command completes on its first tick without a launch (Command_Movement
/// TickMoveTime @1243-1415) and the end logic follows a tick later without the stop (ubergraph @685-699).
pub fn tick_move_cmds(w: &mut World, f: FighterId, dt: crate::Real) {
    if !w.fighter(f).moves.move_cmds.is_empty() {
        let stun = crate::fighter::in_hitstun(w, f);
        let mut i = w.fighter(f).moves.move_cmds.len();
        while i > 0 {
            i -= 1;
            move_cmd_step(w, f, i, dt);
            let m = &mut w.fighter_mut(f).moves;
            let c = &mut m.move_cmds[i];
            if stun || c.cur + dt >= c.total {
                m.fluid_friction = 0.0; // a completed command leaves FluidFriction at 0
                remove_move_cmd(w, f, i);
            } else {
                c.cur += dt;
            }
        }
        return;
    }
    let Some(e) = w.fighter_mut(f).moves.move_end.take() else { return };
    set_terminal(w, f, true, 0.0, false); // BPI_SetTerminalVelocity(true, 0, false)
    let m = &mut w.fighter_mut(f).moves;
    m.wall_skate = false; // BPI_SetWallSkate(false)
    m.flying = false; // IsFlying @699 -> SetMovementMode(Falling) @781
    m.fly_input = None;
    if !crate::fighter::in_hitstun(w, f) && !w.fighter(f).moves.cmd_jumped && (e.stop || e.clamp) {
        // StopOnEnd: NetImpulse((0, 0.1, IsWalking ? 0 : 0.1)), world +Y = screen left.
        let (vx, vy) = if e.clamp {
            clamp_vel(w, f, e.min, e.max, 1.0)
        } else {
            (-0.1, if w.fighter(f).body.on_floor { 0.0 } else { 0.1 })
        };
        net_impulse(w, f, vx, vy, true, true, 0.0, false, 0.0);
        w.fighter_mut(f).fast_fall.ff_triggered = false; // BPI_SetFastFallTriggered(false)
    }
    // NetFriction(base) replaces a stored NetFriction(0) and takes effect with the stop impulse.
    w.fighter_mut(f).moves.net_friction = Some(false);
    set_max_accel(w, f, MAX_ACCEL); // MaxAcceleration = 4096 @843
}

/// `skateTrace`: a line trace (channel 2) forward from the capsule's axis at height `dz`, length radius + 15.
pub fn skate_trace(w: &World, f: FighterId, dz: Real) -> Option<CornerTraceOut> {
    let fi = w.fighter(f);
    let y = fi.body.y + dz;
    let x = fi.body.x;
    let r = crate::fighter::body::cap_radius(w, f);
    trace_block(w, f, x, y, x + fi.body.facing_dir * (r + WALL_SKATE_TRACE_REACH), y)
}

/// `cornerTrace`: AnimNotify_MoveCommand Received_NotifyTick's two line traces (channel 2; head @600, foot
/// @1163) from the actor's location +/- BaseFighter's own Capsule half height (GetScaledCapsuleSize @323, @886)
/// along the forward by its radius + 15. `side` +1 the head trace, -1 the foot trace.
// Unverified: with the root crouched to a sphere the foot trace starts below the capsule's bottom; a start inside
// the block counts as a hit (trace_block).
pub fn corner_trace(w: &mut World, f: FighterId, side: i32) -> Option<CornerTraceOut> {
    let k = w.fighter(f).body.size_scale;
    let ch = def(w, f);
    let (ih, ir) = (ch.inner_cap_half, ch.inner_cap_radius);
    let y = actor_y(w, f)
        + crate::real(side)
            * crate::math::narrow(if k == 1.0 { ih } else { f64::from(crate::math::narrow(ih * f64::from(k))) });
    let r = if k == 1.0 { ir } else { f64::from(crate::math::narrow(ir * f64::from(k))) };
    let fi = w.fighter(f);
    let x = fi.body.x;
    trace_block(w, f, x, y, x + fi.body.facing_dir * crate::math::narrow(r + f64::from(WALL_SKATE_TRACE_REACH)), y)
}

/// `moveCmdStep`: Command_Movement TickMoveTime after the launch (every manager tick, the completing one too; not
/// in hitstun): the wall skate's foot trace, the ControlSpeed path, the WallSkate gates' NetImpulse.
pub fn move_cmd_step(w: &mut World, f: FighterId, i: usize, dt: Real) {
    if crate::fighter::in_hitstun(w, f) {
        return;
    }
    let mut foot = false;
    if w.fighter(f).moves.move_cmds[i].wall_skate {
        let h = skate_trace(w, f, -cap_half(w, f));
        foot = h.is_some_and(|h| h.ny.abs() < 0.1);
    }
    let fi = w.fighter_mut(f);
    let fr = fi.moves.move_cmds[i].ctl.friction;
    fi.moves.fluid_friction = fr;
    if fi.moves.move_cmds[i].ctl.speed > 0.0 {
        control_step(w, f, i, foot, dt);
        return;
    }
    let grounded = fi.body.on_floor;
    let c = &mut fi.moves.move_cmds[i];
    if if foot { !c.up_gate } else { !c.side_gate } {
        return;
    }
    // VSize, then Normal((X, Y, IsWalking ? FMax(Z, 0) : Z), 0.0001).
    let v = ue::size2d([c.vx, c.vy]);
    let z = if grounded { max(c.vy, 0.0) } else { c.vy };
    let (nx, nz) = unit(c.vx, z);
    c.up_gate = !foot;
    c.side_gate = foot;
    if foot {
        let (a, b) = (if nx >= 0.0 { 0.001 } else { -0.001 }, if nz >= 0.0 { 1.0 } else { -1.0 });
        net_impulse(w, f, a * v, b * v, true, true, 8.0, false, 0.0);
    } else {
        net_impulse(w, f, nx * v, (if nz != 0.0 { nz } else { -1.0 }) * v, true, true, 8.0, false, 0.0);
    }
}

/// `controlStep`: Command_Movement TickMoveTime with ControlSpeed > 0: SetMovementMode(Flying), then MoveVector
/// turns about the depth axis by ControlSpeed x dt degrees toward the stick (digital, 8-way), cut at the angle
/// clamp; AddMovementInput(direction x |MoveVector|) feeds the next CMC tick.
pub fn control_step(w: &mut World, f: FighterId, i: usize, foot: bool, dt: Real) {
    let fi = w.fighter_mut(f);
    fi.moves.flying = true;
    fi.hit.kb_mode = false;
    set_grounded(w, f, false);
    let fi = w.fighter_mut(f);
    let sx = sign(fi.controls.inp.x);
    let sy = sign(fi.controls.inp.y);
    let left = fi.body.facing_dir < 0.0;
    let c = &mut fi.moves.move_cmds[i];
    let k = c.ctl.clone();
    let y = -c.vx;
    let z = c.vy;
    let cur = ue::deg_atan2(y, z);
    let st = near_angle(cur, ue::deg_atan2(sx, -sy));
    let b1 = rot_yz(-c.bx, c.by, if left { k.min } else { -k.max });
    let b2 = rot_yz(-c.bx, c.by, if left { k.max } else { -k.min });
    let d1 = near_angle(cur, ue::deg_atan2(b1.0, b1.1)).2;
    let d2 = near_angle(cur, ue::deg_atan2(b2.0, b2.1)).2;
    let lim = (if st.1 { d2 } else { d1 }).abs();
    let keep = st.2.abs() >= 179.0 || (k.max != k.min && lim <= 1.0) || (sx == 0.0 && sy == 0.0);
    let step = if k.max == k.min { k.speed * dt } else { max(0.0, min(k.speed * dt, lim)) };
    let (ry, rz) = if keep { (y, z) } else { rot_yz(y, z, step * st.0) };
    // VSize @5134, Normal(.., 0.0001) @5226 (flying: no IsWalking clamp).
    let mag = ue::size2d([ry, rz]);
    let (ny, nz) = unit(ry, rz);
    let (mut dy, mut dz) = (ny, nz);
    if foot {
        dy = if ny >= 0.0 { 0.001 } else { -0.001 };
        dz = if nz >= 0.0 { 1.0 } else { -1.0 };
    } else if c.old_foot {
        dy = if y >= 0.0 { 1.0 } else { -1.0 };
        dz = if c.corner { 0.0 } else { -1.0 };
    }
    let m2 = max(mag - k.decay * dt, 0.0);
    c.vx = -dy * m2;
    c.vy = dz * m2;
    c.old_foot = foot;
    fi.moves.fly_input = Some([-dy * mag, dz * mag]);
}

/// `forceMoveToPoint`: a MoveToForce root motion source; `ease`: Buff_MoveToPosition BeginMove's time curve with
/// bEaseIn and bEaseOut (@1115 / @1222, EaseInOutFloatCurve). The id (14291ca20 -> 142a4e750): a uint16 counter
/// that wraps to 1, never 0 (the failed add's return).
// Deviation: the build's counter is one for the process; the port's is the fighter's (an id is only compared with
// its own CMC's sources).
#[allow(clippy::too_many_arguments, reason = "the call's parameters")]
pub fn force_move_to_point(
    w: &mut World,
    f: FighterId,
    dest_x: Real,
    dest_actor_y: Real,
    duration: Real,
    finish_mode: Real,
    set_vx: Real,
    set_vy: Real,
    clamp: Real,
    ease: bool,
) -> Real {
    #[allow(clippy::neg_cmp_op_on_partial_ord, reason = "the negated comparison !(duration > 1e-8): a NaN returns")]
    if !(duration > 1e-8) {
        return 0.0;
    }
    if let Some(id) = w.fighter(f).moves.force_move.as_ref().map(|s| s.id) {
        remove_force_move(w, f, id);
    }
    let ay = actor_y(w, f);
    let fi = w.fighter_mut(f);
    let id = if fi.moves.force_move_seq >= 65535 { 1 } else { fi.moves.force_move_seq + 1 };
    fi.moves.force_move_seq = id;
    fi.moves.force_move = Some(ForceMove {
        id,
        sx: fi.body.x,
        sy: ay,
        tx: dest_x,
        ty: dest_actor_y,
        dur: duration,
        time: 0.0,
        done: false,
        mode: to_int32(finish_mode),
        clamp,
        set_vx,
        set_vy,
        ease,
    });
    crate::real(id)
}

// EaseInOutFloatCurve (Panda_Main/Blueprints/MovementCurves): two cubic keys, tangent weights none.
// Unverified: EvaluateFloatCurveAtFraction and the MoveToForce source's use of the time curve are from stock 4.26
// source, not read in the exe.
const EASE_T0: Real = -0.2;
const EASE_T1: Real = 1.0;
const EASE_V0: Real = 1.2200326e-8;
const EASE_V1: Real = 1.0;
const EASE_LEAVE0: Real = 0.0;
const EASE_ARRIVE1: Real = -0.002324257;

/// `easeInOut`: FRichCurve::Eval of the cubic segment at the fraction mapped over the curve's time range.
pub fn ease_in_out(fraction: Real) -> Real {
    #[allow(clippy::neg_cmp_op_on_partial_ord, reason = "the negated comparison !(fraction > 0)")]
    if !(fraction > 0.0) {
        return EASE_V0;
    }
    if fraction >= 1.0 {
        return EASE_V1;
    }
    let t = EASE_T0 + (EASE_T1 - EASE_T0) * fraction;
    let gap = EASE_T1 - EASE_T0;
    let u = (t - EASE_T0) / gap;
    let v = 1.0 - u;
    let p1 = EASE_V0 + (EASE_LEAVE0 * gap) / 3.0;
    let p2 = EASE_V1 - (EASE_ARRIVE1 * gap) / 3.0;
    v * v * v * EASE_V0 + 3.0 * v * v * u * p1 + 3.0 * v * u * u * p2 + u * u * u * EASE_V1
}

/// `removeForceMove`: RemoveRootMotionSourceByID; it takes effect at the next CleanUpInvalidRootMotion.
pub fn remove_force_move(w: &mut World, f: FighterId, id: i32) {
    let m = &mut w.fighter_mut(f).moves;
    let Some(s) = m.force_move.as_ref().filter(|s| s.id == id) else { return };
    m.force_finish = Some(ForceFinish { mode: s.mode, clamp: s.clamp, set_vx: s.set_vx, set_vy: s.set_vy });
    m.force_move = None;
}

/// `forceMoveCleanUp`: CleanUpInvalidRootMotion: the removed source's FinishVelocity.
pub fn force_move_clean_up(w: &mut World, own: FighterId) {
    if let Some(id) = w.fighter(own).moves.force_move.as_ref().filter(|s| s.done).map(|s| s.id) {
        remove_force_move(w, own, id);
    }
    let fi = w.fighter_mut(own);
    let Some(f) = fi.moves.force_finish.take() else { return };
    if f.mode == 2 {
        if fi.body.vx.abs() > f.clamp {
            fi.body.vx = if fi.body.vx < 0.0 { -f.clamp } else { f.clamp };
        }
        fi.body.vy = min(fi.body.vy, f.clamp);
    } else if f.mode == 1 {
        fi.body.vx = f.set_vx;
        fi.body.vy = f.set_vy;
    }
}

/// `forceMovePrepare`: PrepareRootMotion of the MoveToForce source: the velocity that moves the actor to the
/// source's point at its time after `dt`, capped at the expected speed + 0.5.
pub fn force_move_prepare(w: &mut World, f: FighterId, dt: Real) -> Option<(Real, Real)> {
    let ay = actor_y(w, f);
    let fi = w.fighter_mut(f);
    let x = fi.body.x;
    let s = fi.moves.force_move.as_mut()?;
    let mut sim = dt;
    if s.time + sim >= s.dur {
        sim = s.dur - s.time + 1e-4;
    }
    if sim < 0.0 {
        sim = 0.0;
    }
    let f1 = if s.ease { ease_in_out((s.time + sim) / s.dur) } else { (s.time + sim) / s.dur };
    let cx = s.sx + (s.tx - s.sx) * f1;
    let cy = s.sy + (s.ty - s.sy) * f1;
    // PrepareRootMotion (142a60150): the reciprocal of the tick time, then multiplies.
    let inv = 1.0 / dt;
    let mut fx = (cx - x) * inv;
    let mut fy = (cy - ay) * inv;
    if fx.abs() > 1e-4 || fy.abs() > 1e-4 {
        let f0 = if s.ease { ease_in_out(s.time / s.dur) } else { s.time / s.dur };
        let ex = (cx - (s.sx + (s.tx - s.sx) * f0)) * inv;
        let ey = (cy - (s.sy + (s.ty - s.sy) * f0)) * inv;
        let exp = ue::size2d([ex, ey]);
        if fx * fx + fy * fy > (exp + 0.5) * (exp + 0.5) {
            // FVector::Normalize (no `== 1` test), then x the expected speed.
            let (n, _) = ue::normalize([0.0, fx, fy]);
            fx = n[1] * exp;
            fy = n[2] * exp;
        }
    }
    s.time += sim;
    if s.time >= s.dur {
        s.done = true;
    }
    Some((fx, fy))
}

/// `physForceMove`: the CMC tick under an override source: PhysFalling's substeps move by the force.
pub fn phys_force_move(w: &mut World, f: FighterId, dt: Real, fx: Real, fy: Real) {
    let fi = w.fighter_mut(f);
    fi.moves.fly_input = None;
    fi.body.vx = fx;
    fi.body.vy = fy;
    if fi.body.on_floor {
        #[allow(clippy::neg_cmp_op_on_partial_ord, reason = "the negated comparison !(fy > 1e-8)")]
        if !(fy > 1e-8) {
            ledge_step(w, f, fx * dt);
            collide(w, f, false, None);
            return;
        }
        set_grounded(w, f, false);
    }
    let h = dt / SUBSTEPS;
    let mut s = 0.0;
    while s < SUBSTEPS {
        let fi = w.fighter(f);
        let (x, y) = (fi.body.x, fi.body.y);
        if fall_move(w, f, x, y, fx * h, fy * h, h, s, false, (SUBSTEPS - s - 1.0) * h) {
            break;
        }
        s += 1.0;
    }
    collide(w, f, false, None);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn near_angle_takes_the_nearest_turn() {
        // 10 - 350 = -340: the turn the other way, +20.
        assert_eq!(near_angle(10.0, 350.0), (-1.0, false, 20.0));
        // 350 - 10 = 340: -20.
        assert_eq!(near_angle(350.0, 10.0), (1.0, true, -20.0));
        assert_eq!(near_angle(30.0, 30.0), (-1.0, false, 0.0));
    }

    #[test]
    fn ease_in_out_ends() {
        assert_eq!(ease_in_out(0.0), EASE_V0);
        assert_eq!(ease_in_out(Real::NAN), EASE_V0);
        assert_eq!(ease_in_out(1.0), EASE_V1);
        let m = ease_in_out(0.5);
        assert!(m > 0.0 && m < 1.0);
    }

    #[test]
    fn rot_yz_turns_down() {
        let (y, z) = rot_yz(0.0, 1.0, 90.0);
        // sin / cos of 90 degrees through float32 pi: y one step above -1, z is -2^-23
        assert_eq!((y, z), (-0.9999999, -1.1920929e-7));
    }
}
