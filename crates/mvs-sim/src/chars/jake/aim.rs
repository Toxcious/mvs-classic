//! The forward-air aim punch: JakeAimer_C ([`JakeAimer`], the stick's aim while the
//! punch charges), JakeAimPunchEatListener_C ([`JakeAimPunchEatListener`]), the notifies Hitframe_Jake_Aim /
//! UpdateHitframeDamage_Jake_Aim (the shared hit frame with the aim's knockback direction) and ResetAimDirection (only
//! jake.json names it: entered here); joined to chargeEnd (the aimer's PercentCharged). Its state: `JakeState::aim`,
//! `aimer`.
//!
//! Jake_Air_Aim_Punch_F_Montage: ResetAimDirection at -0.0001; Spawner JakeAimer_C 0.038 + 0.659 s (AttachToActor,
//! socket None, DestroyAllActorsOnNotifyEnd: the world's spawner end); ChargingAttack 0.230 + 0.391 s (section End);
//! Spawner JakeAimPunchEatListener_C 0.632 + 0.280 s (socket fml_un_C_prop1); Hitframe_Jake_Aim 0.716 + 0.083 s;
//! UpdateHitframeDamage_Jake_Aim 0.749 + 0.050 s. The JakeAimer's SkillShotIndicator is hidden at BeginPlay @768 (the
//! Arrow is bVisible false: the drawn part is the Sphere, AimPunchMat).

use super::pawn::{set_aim_direction, set_aim_distance_alpha};
use super::state::{jake, jake_of, jake_w};
use crate::Real;
use crate::State;
use crate::actors::world_actor::WorldActorState;
use crate::actors::{ActorClass, ActorId, ActorRef, WorldActor, WorldActorClass};
use crate::combat::hit::set_active_hit_props;
use crate::consts::DELTA_SECONDS;
use crate::events::NotifyRef;
use crate::fighter::common::{actor_data, latent_frames};
use crate::fighter::match_side::{SpawnRequest, notify_props};
use crate::fighter::traces::{TRACE_WALL_JUMP, TraceBody, sphere_trace_single};
use crate::fighter::{FighterId, actor_y, facing, percent_charged};
use crate::math::ue::{rotate_angle_axis, round_to_int};
use crate::math::{max, min, to_int32};
use crate::registry::entries::{SizeHooks, WorldActorHooks};
use crate::registry::notifies::{hitframe_begin_with, hitframe_end, update_hitframe_damage_begin};
use crate::registry::props::{flag, num, obj};
use crate::registry::{ActorEntry, ActorHooks, NotifyHandler, Registry, RegistryError};
use crate::world::World;
use crate::world::blast::{BlastBounds, blast_box_loop, blast_timer_exit, new_blast_bounds, spawn_box};
use crate::world::spawn_spot::spawn_place;
use crate::world::spawning::request_props;
use mvs_data::value::{Props, Value};

/// JakeAimer's class.
pub const AIMER: &str = "JakeAimer_C";

/// JakeAimPunchEatListener's class.
pub const AIM_PUNCH_EAT_LISTENER: &str = "JakeAimPunchEatListener_C";

/// `JakeAimer` (JakeAimer_C; what the view reads: `x`, `y`, `aim_x`, `aim_y` (unit length), `distance`).
#[derive(State, Debug, Default)]
pub struct JakeAimer {
    /// Its place (sim x, y).
    pub x: Real,
    pub y: Real,
    /// Its place relative to Jake's location (sim x along his facing, y): attached.
    pub dx: Real,
    pub dy: Real,
    /// ArrowAimVector (sim x, y).
    pub aim_x: Real,
    pub aim_y: Real,
    /// AimDistance = AimInfo.AimDistanceMax @373 (UE units).
    pub distance: Real,
    /// ArrowAimVector and StartAimVector in UE (Y, Z); X stays 0.
    pub ay: Real,
    pub az: Real,
    pub sy: Real,
    pub sz: Real,
    /// AimInfo (the class's) AimAngleMin / Max, AimSpeed, AimType.
    pub min: Real,
    pub max: Real,
    pub speed: Real,
    pub aim_type: Real,
    /// Percent Charged (ChargeEnd's SetChargeValueMulticast @7812 -> @7880).
    pub percent_charged: Real,
    /// The world tick of BeginPlay's BPI_SetStickAttackX / Y(0).
    pub zero_tick: i32,
    /// Delay(0.75) @7756 -> @15: frames left, 0: not running.
    pub wait: i32,
}

/// `JakeAimPunchEatListener` (JakeAimPunchEatListener_C): of its own nothing is kept (Deviation aim c); the parent's
/// blast-box state is ([`listener_spawn`]).
#[derive(State, Debug, Default)]
pub struct JakeAimPunchEatListener {
    pub age: i32,
    /// Its place relative to Jake's location at the spawn (sim x along his facing, y).
    pub dx: Real,
    pub dy: Real,
    /// SpawnedActorParent's CheckBlastBoxTimer: its next expiry.
    pub blast_at: crate::Clock,
    /// BlastBoxBoundsMin / Max, stored by the BeginPlay loop.
    pub blast_bounds: BlastBounds,
}

fn aimer_of(w: &World, id: ActorId) -> Option<&JakeAimer> {
    match &w.actors.actors.get(id)?.state {
        WorldActorState::JakeAimer(s) => Some(s),
        _ => None,
    }
}

fn aimer_mut(w: &mut World, id: ActorId) -> Option<&mut JakeAimer> {
    match &mut w.actors.actors.get_mut(id)?.state {
        WorldActorState::JakeAimer(s) => Some(s),
        _ => None,
    }
}

/// The JakeAimer_C's place (its `x`, `y`).
pub fn aimer_place(_w: &World, _a: &WorldActor, s: &JakeAimer) -> Option<(Real, Real)> {
    Some((s.x, s.y))
}

/// The aimer's ChargeEnd binding (@1659-1682, bound in BeginPlay): SetChargeValueMulticast(the owner's
/// Comp_Pawn_ChargeAttackManager.PercentCharged) @7812 -> Percent Charged @7880. aim.rs's part of the joined chargeEnd,
/// after pawn.rs's.
pub fn charge_end(w: &mut World, f: FighterId, _n: NotifyRef) {
    let Some(ActorRef::Actor(id)) = jake_of(w, f).and_then(|j| j.aimer) else { return };
    if w.actors.actors.get(id).is_none_or(|a| a.dead) {
        return;
    }
    let pc = percent_charged(w, f);
    if let Some(m) = aimer_mut(w, id) {
        m.percent_charged = pc;
    }
}

/// DegAtan2 (FMath::Atan2 x 180 / pi).
fn deg_atan2(y: Real, x: Real) -> Real {
    crate::math::ue::deg_atan2(y, x)
}

/// FVector::RotateAngleAxis(v, deg, (1, 0, 0)) on (Y, Z): Y' = C.Y - S.Z, Z' = S.Y + C.Z.
fn rot_x(y: Real, z: Real, deg: Real) -> (Real, Real) {
    let [_, y2, z2] = rotate_angle_axis([0.0, y, z], deg, [1.0, 0.0, 0.0]);
    (y2, z2)
}

/// JakeAimer.FindNearAngleDirection @0-776: d = (A1 + 180) - (A2 + 180); the one of d, d - 360 (tested first), d + 360
/// with the smallest absolute value; ReturnMult = it < 0 ? 1 : -1, Positive = it < 0, AngleDifrence = Round(it).
struct Near {
    mult: Real,
    pos: bool,
    diff: Real,
}

fn near_angle(a1: Real, a2: Real) -> Near {
    let d = (a1 + 180.0) - (a2 + 180.0);
    let (dm, dp) = ((d - 360.0), (d + 360.0));
    let s = if d.abs() < dm.abs() { d } else { dm };
    let r = if s.abs() < dp.abs() { s } else { dp };
    Near { mult: if r < 0.0 { 1.0 } else { -1.0 }, pos: r < 0.0, diff: crate::math::real(round_to_int(r)) }
}

/// ReceiveBeginPlay @1099: BaseFighterRef / JakePawn, SetAimInfoMulticast(FacingLeft, ...) -> @496: the class's own
/// AimInfo (the parameter is not read): StartAimVector = (0, FacingLeft ? Y : -Y, Z) of AimInfo.StartAimDirection
/// @532-735; AimType 0 -> @1294: ArrowAimVector = StartAimVector (the Arrow shown only to its local player: not drawn);
/// @1451 BPI_SetAimDirection(0), Percent Charged = 0, ChargeEnd bound (the joined `charge_end`); @1746
/// BPI_SetStickAttackX(0) / Y(0): StickAttackX / Y are BaseFighter variables the player controller's NormalAttackX / Y
/// axis events write every frame ahead of the actors' tick: the zero is read by the aimer's tick of its spawn frame only
/// (`zero_tick`), the next frame's axis event writes the stick again.
fn aimer_spawn(w: &mut World, f: FighterId, r: &SpawnRequest) -> (WorldActorState, bool) {
    let data = w.data.clone();
    let cls = w.names.str(r.cls);
    let a = actor_data(w, &data, ActorRef::Fighter(f), r.cls)
        .unwrap_or_else(|| panic!("{cls}: no actor data for its spawn (the importer gives every spawned class)"));
    let np = request_props(w, f, r);
    let at = spawn_place(w, f, &np);
    let empty = Props::new();
    let info = obj(&a.p, "AimInfo").unwrap_or(&empty);
    let face = facing(w, f);
    let sd = obj(info, "StartAimDirection").unwrap_or(&empty);
    let y = num(sd, "Y").unwrap_or(0.0);
    let mut m = JakeAimer {
        x: at.x,
        y: at.y,
        dx: (at.x - w.fighter(f).body.x) * face,
        dy: at.y - actor_y(w, f),
        min: num(info, "AimAngleMin").unwrap_or(0.0),
        max: num(info, "AimAngleMax").unwrap_or(0.0),
        speed: num(info, "AimSpeed").unwrap_or(0.0),
        distance: num(info, "AimDistanceMax").unwrap_or(0.0), // @373
        aim_type: num(info, "AimType").unwrap_or(0.0),
        zero_tick: w.flow.tick,
        ..JakeAimer::default()
    };
    m.sy = if face < 0.0 { y } else { -y };
    m.sz = num(sd, "Z").unwrap_or(0.0);
    if m.aim_type == 0.0 {
        m.ay = m.sy;
        m.az = m.sz;
    }
    m.aim_x = -m.ay; // sim x = -UE Y
    m.aim_y = m.az;
    (WorldActorState::JakeAimer(m), false)
}

/// The rest of BeginPlay that needs the actor's id: BPI_SetAimDirection(0), the pawn's aimer.
fn aimer_spawned(w: &mut World, id: ActorId) {
    let Some(f) = w.actors.actors.get(id).map(|a| a.owner) else { return };
    set_aim_direction(w, f, 0.0);
    jake_w(w, f).aimer = Some(ActorRef::Actor(id));
}

/// The aim @2025-5950 (AimType 0): the stick's 8-way direction, the move axes when StickAttackX + StickAttackY == 0,
/// as (Y, Z) = (sign X, -sign Y) (the world direction opposite the stick: +MoveX is -Y), and the arrow turned away
/// from it at AimSpeed x DT within the limits StartAimVector rotated by (FacingLeft ? Max : -Min) and
/// (FacingLeft ? Min : -Max); held with |difference| >= 179, within 1 degree of the limit on its side (Max != Min),
/// or with no input; the step clamped to the rounded distance to that limit. X is set to 0 (no normalisation).
/// Then LineTraceSingle(channel 2, WallJump) from loc + arrow x 150 to loc + arrow x 800 (self ignored): alpha =
/// blocking ? 1 - NormalizeToRange(|loc - ImpactPoint|, 240, 850) : 0, and SetArrowRotationServer(arrow,
/// FMax(alpha, Lerp(0.4, 0, Percent Charged))) -> @6061.
// Deviation: DT is the world's step (DELTA_SECONDS); a dilated actor's DeltaSeconds is not modelled. (aim b)
fn aim(w: &mut World, id: ActorId, f: FighterId) {
    let Some(m) = aimer_of(w, id) else { return };
    if m.aim_type != 0.0 {
        return; // the switch @2025: other types do nothing
    }
    let (mut ay, mut az, sy, sz, mn, mx, speed, pc, zero_tick, dx, dy) =
        (m.ay, m.az, m.sy, m.sz, m.min, m.max, m.speed, m.percent_charged, m.zero_tick, m.dx, m.dy);
    let face = facing(w, f);
    let fl = face < 0.0;
    // StickAttackX / Y of the owner as its tick reads them (BeginPlay's zeros, then the right stick; Y up positive).
    let (sx, stick_y) = if w.flow.tick == zero_tick {
        (0.0, 0.0)
    } else {
        let i = &w.fighter(f).controls.inp;
        (i.rsx.unwrap_or(0.0), i.rsy.unwrap_or(0.0))
    };
    let use_move = (sx + stick_y) == 0.0;
    let ix = if use_move { w.fighter(f).controls.inp.x } else { sx };
    let iy = if use_move { w.fighter(f).controls.inp.y } else { stick_y };
    let s4 = if ix == 0.0 {
        0.0
    } else if ix > 0.0 {
        1.0
    } else {
        -1.0
    };
    let s5 = if iy == 0.0 {
        0.0
    } else if iy > 0.0 {
        -1.0
    } else {
        1.0
    };
    let a2 = deg_atan2(s4, s5);
    let l3 = rot_x(sy, sz, if fl { mx } else { -mn });
    let l4 = rot_x(sy, sz, if fl { mn } else { -mx });
    let (a3, a4) = (deg_atan2(l3.0, l3.1), deg_atan2(l4.0, l4.1));
    let a5 = deg_atan2(ay, az);
    let (n0, n1, n2) = (near_angle(a5, a4), near_angle(a5, a3), near_angle(a5, a2));
    let sel = (if n2.pos { n1.diff } else { n0.diff }).abs();
    let hold = n2.diff.abs() >= 179.0 || (mx != mn && sel <= 1.0) || (ix == 0.0 && iy == 0.0);
    if !hold {
        let step = speed * DELTA_SECONDS;
        let amt = if mx == mn { step } else { min(max(step, 0.0), sel) };
        (ay, az) = rot_x(ay, az, amt * n2.mult);
    }
    // The trace from the aimer's location (attached: Jake's - 100 in Z).
    let (px, py) = (w.fighter(f).body.x + dx * face, actor_y(w, f) + dy);
    if let Some(m) = aimer_mut(w, id) {
        m.ay = ay;
        m.az = az;
        m.x = px;
        m.y = py;
    }
    let (vx, vy) = (-ay, az);
    let h = sphere_trace_single(
        w,
        px + vx * 150.0,
        py + vy * 150.0,
        px + vx * 800.0,
        py + vy * 800.0,
        0.0,
        TRACE_WALL_JUMP,
        &[],
        Some(ActorRef::Actor(id)),
    );
    let mut alpha = 0.0;
    if let Some(h) = h.filter(|h| h.blocking) {
        let (ex, ey) = (px - h.ix, py - h.iy);
        alpha = 1.0 - ((((ex * ex + ey * ey).sqrt()) - 240.0) / 610.0);
    }
    let lerp = 0.4 + (pc * (0.0 - 0.4));
    set_arrow(w, id, f, max(alpha, lerp));
}

/// SetArrowRotationMulticast @6061-7550: ArrowAimVector; BPI_SetAimDIstanceAlpha(alpha); BPI_SetAimDirection(
/// NormalizeToRange(a, s + Min, s + Max) x 2 - 1) with a / s the angles DegAtan2(Z, FacingLeft ? Y : -Y) of the arrow
/// and of StartAimVector; JakePawn.AimPunchVector = ArrowAimVector (the Pawn_Jake cast of BeginPlay).
fn set_arrow(w: &mut World, id: ActorId, f: FighterId, alpha: Real) {
    let fl = facing(w, f) < 0.0;
    let Some(m) = aimer_mut(w, id) else { return };
    m.aim_x = -m.ay;
    m.aim_y = m.az;
    let (ay, az, sy, sz, mn, mx) = (m.ay, m.az, m.sy, m.sz, m.min, m.max);
    set_aim_distance_alpha(w, f, alpha);
    let a12 = deg_atan2(az, if fl { ay } else { -ay });
    let a13 = deg_atan2(sz, if fl { sy } else { -sy });
    let (lo, hi) = ((a13 + mn), (a13 + mx));
    set_aim_direction(w, f, (((a12 - lo) / (hi - lo)) * 2.0) - 1.0);
    let s = &mut jake_w(w, f).aim;
    s.aim_punch_y = ay;
    s.aim_punch_z = az;
}

/// ReceiveTick @7628: with authority the aim (@2025), then Delay(0.75) @7756 (one running Delay: a later call is
/// ignored; created in the tick, it counts this frame) -> @15: K2_DestroyActor unless being destroyed.
fn aimer_step(w: &mut World, id: ActorId) {
    let Some(a) = w.actors.actors.get(id) else { return };
    if a.dead {
        return;
    }
    let f = a.owner;
    aim(w, id, f);
    let Some(m) = aimer_mut(w, id) else { return };
    if m.wait == 0 {
        m.wait = to_int32(latent_frames(0.75));
    }
    m.wait -= 1;
    if m.wait == 0
        && let Some(a) = w.actors.actors.get_mut(id)
    {
        a.dead = true;
    }
}

/// ReceiveEndPlay @1084 -> HideUI @917: the arrow and the spline (not drawn). The ChargeEnd binding goes with it.
fn aimer_end(w: &mut World, id: ActorId) {
    let Some(f) = w.actors.actors.get(id).map(|a| a.owner) else { return };
    if jake_of(w, f).is_some_and(|j| j.aimer == Some(ActorRef::Actor(id))) {
        jake_w(w, f).aimer = None;
    }
}

/// JakeAimPunchEatListener: BeginPlay @10 sets Pawn_Jake.CanGrabEat = false; an overlap of a BPI_BaseFighter of the
/// owner's team AddUnique's it to OverlappedActor and sets CanGrabEat = true; the end overlap of the last one sets it
/// false. CanGrabEat's one reader is JakeGrabCondition, which no montage of the data names: nothing a player sees comes
/// of it.
/// Its ReceiveBeginPlay @147 calls the parent's first: SpawnedActorParent's blast-box loop at the spawn place (the
/// Spawner's socket fml_un_C_prop1, attached; DestroyOnExitedArena? the parent's default, no
/// ForceSpawnPositionInsideBlastBox), then its CheckBlastBoxExit timer ([`listener_step`]).
// Deviation: the actor lives its spawner's span and does nothing; CanGrabEat is not kept. (aim c)
// Deviation: the place the timer tests is the spawn's offset from Jake carried with him (his facing mirrors it); the
// socket's own motion through the 0.28 s of the notify is not followed.
fn listener_spawn(w: &mut World, f: FighterId, r: &SpawnRequest) -> (WorldActorState, bool) {
    let np = request_props(w, f, r);
    let at = spawn_place(w, f, &np);
    let data = w.data.clone();
    let p = actor_data(w, &data, ActorRef::Fighter(f), r.cls).map(|d| &d.p);
    let (doe, force) =
        p.map_or((false, false), |p| (flag(p, "DestroyOnExitedArena?"), flag(p, "ForceSpawnPositionInsideBlastBox")));
    let (mut x, mut y) = (at.x, at.y);
    let mut s = JakeAimPunchEatListener {
        blast_at: f64::from(0.1_f32),
        blast_bounds: new_blast_bounds(),
        ..JakeAimPunchEatListener::default()
    };
    let dead = blast_box_loop(&spawn_box(w), &mut x, &mut y, None, force, &mut s.blast_bounds, doe);
    s.dx = (x - w.fighter(f).body.x) * facing(w, f);
    s.dy = y - actor_y(w, f);
    (WorldActorState::JakeAimPunchEatListener(s), dead)
}

/// One actor tick: SpawnedActorParent's CheckBlastBoxExit on its 0.1 s looping timer, the spawn frame not counted.
fn listener_step(w: &mut World, id: ActorId) {
    let Some(a) = w.actors.actors.get(id) else { return };
    let (f, cls) = (a.owner, a.cls);
    let data = w.data.clone();
    let doe = actor_data(w, &data, ActorRef::Fighter(f), cls).is_some_and(|d| flag(&d.p, "DestroyOnExitedArena?"));
    let (fx, fy, face) = (w.fighter(f).body.x, actor_y(w, f), facing(w, f));
    let Some(a) = w.actors.actors.get_mut(id) else { return };
    let WorldActorState::JakeAimPunchEatListener(s) = &mut a.state else { return };
    s.age += 1;
    let (x, y) = (fx + s.dx * face, fy + s.dy);
    if blast_timer_exit(doe, &mut s.blast_at, &s.blast_bounds, crate::real(s.age - 1), x, y) {
        a.dead = true;
    }
}

/// The knockback direction both Jake_Aim notifies build: MakeVector2D(AimPunchVector.Y, AimPunchVector.Z) (UE Y, Z;
/// with the data's IgnoreFacing the packet's X is taken as UE Y).
fn aim_direction(w: &World, f: FighterId) -> Value {
    let s = &jake(w, f).aim;
    let mut o = Props::new();
    o.insert("X".into(), Value::Num(f64::from(s.aim_punch_y)));
    o.insert("Y".into(), Value::Num(f64::from(s.aim_punch_z)));
    Value::Obj(o)
}

/// AnimNotify_Hitframe_Jake_Aim: the base Hitframe's HitBoxesOn packet (@1059-2318) on a Pawn_Jake owner only (@243:
/// another owner returns with nothing done), KnockBackDirectionVector = (AimPunchVector.Y, .Z) @855. NotifyEnd:
/// HitBoxesOff(HitBoxTags), the base's.
fn hitframe_begin(w: &mut World, f: FighterId, n: NotifyRef, idx: i32) {
    if jake_of(w, f).is_none() {
        return;
    }
    let mut p = notify_props(w, f, n).clone();
    p.insert("KnockBackDirection".into(), aim_direction(w, f));
    hitframe_begin_with(w, f, n, idx, Some(p));
}

/// Both Jake_Aim notifies' NotifyEnd: the shared Hitframe's (HitBoxesOff(HitBoxTags)) whatever the begin did.
fn shared_end(w: &mut World, f: FighterId, n: NotifyRef, idx: i32) {
    hitframe_end(w, f, n, idx);
}

/// AnimNotify_UpdateHitframeDamage_Jake_Aim: with the owner's Comp_Pawn_ChargeAttack.PercentCharged >= 0.75 @93 and a
/// Pawn_Jake owner @406, UpdateHitBoxDamage @2065 with the notify's values, KnockBackDirectionVector =
/// (AimPunchVector.Y, .Z), HitPauseTime as it is and VacuumKnockback false: the shared UpdateHitframeDamage's packet.
/// NotifyEnd @0-229: HitBoxesOff(HitBoxTags) whatever the begin did (the shared end).
#[allow(clippy::neg_cmp_op_on_partial_ord, reason = "negated test, kept for its NaN result")]
fn update_begin(w: &mut World, f: FighterId, n: NotifyRef, idx: i32) {
    if !(percent_charged(w, f) >= 0.75) || jake_of(w, f).is_none() {
        return;
    }
    update_hitframe_damage_begin(w, f, n, idx);
    let mut p = notify_props(w, f, n).clone();
    p.insert("KnockBackDirection".into(), aim_direction(w, f));
    p.insert("VacuumKnockback".into(), Value::Bool(false));
    set_active_hit_props(w, f, idx, p);
}

/// AnimNotify_ResetAimDirection @121: BPI_SetAimDirection(0.0) on the owner's AnimBP (Jake_AnimBP @8544; only
/// jake.json names the class).
fn reset_aim_begin(w: &mut World, f: FighterId, _n: NotifyRef, _idx: i32) {
    if jake_of(w, f).is_some() {
        set_aim_direction(w, f, 0.0);
    }
}

/// This file's registry entries (JakeAimer_C, JakeAimPunchEatListener_C, the three notifies).
pub fn register(r: &mut Registry) -> Result<(), RegistryError> {
    r.actors.enter(
        AIMER,
        ActorEntry::new(
            ActorClass::World(WorldActorClass::JakeAimer),
            ActorHooks::World(WorldActorHooks {
                // no shape of the port's and nothing drawn: its aim is a vector
                size: Some(SizeHooks::default()),
                spawn: Some(aimer_spawn),
                spawned: Some(aimer_spawned),
                step: Some(aimer_step),
                end: Some(aimer_end),
                bodies: Some(|_: &World, _: ActorId, _: &mut Vec<TraceBody>| {}),
                ..WorldActorHooks::default()
            }),
        ),
    )?;
    r.actors.enter(
        AIM_PUNCH_EAT_LISTENER,
        ActorEntry::new(
            ActorClass::World(WorldActorClass::JakeAimPunchEatListener),
            ActorHooks::World(WorldActorHooks {
                // a listener: nothing of it is kept or drawn
                size: Some(SizeHooks::default()),
                spawn: Some(listener_spawn),
                step: Some(listener_step),
                bodies: Some(|_: &World, _: ActorId, _: &mut Vec<TraceBody>| {}),
                ..WorldActorHooks::default()
            }),
        ),
    )?;
    r.notifies.enter(
        "AnimNotify_Hitframe_Jake_Aim_C",
        NotifyHandler { begin: Some(hitframe_begin), end: Some(shared_end), ..NotifyHandler::default() },
    )?;
    r.notifies.enter(
        "AnimNotify_UpdateHitframeDamage_Jake_Aim_C",
        NotifyHandler { begin: Some(update_begin), end: Some(shared_end), ..NotifyHandler::default() },
    )?;
    r.notifies.enter(
        "AnimNotify_ResetAimDirection_C",
        NotifyHandler { begin: Some(reset_aim_begin), ..NotifyHandler::default() },
    )
}
