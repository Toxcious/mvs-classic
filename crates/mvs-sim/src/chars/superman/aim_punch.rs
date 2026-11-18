//! Superman's aim punch (Locomotive Charge, Move List row 10;
//! Superman_GroundAimPunch_Montage). SupermanAimPunch_C ([`AimMarker`]), the marker he dashes to: its tick,
//! ActivateObject's dash and HideUI. The SniperPunch perk's multipliers are in `perks`. `aim_dash_from_x` /
//! `aim_dash_from_y` of the SupermanState are written here, `aim_on` by the pawn's AnimNotify_SupermanAimDash.
//!
//! The montage (length 2.5333 s; sections Default 0, Loop 0.3667 (loops), LoopEnd 1.3667), all by shared notifies:
//! Spawner of this class at 0.0476 (not attached, socket None, translation (0, 500, 250), its actors destroyed at
//! the notify's end 1.5245), ChargingAttack 0.1269-1.3667 (the button's release or the loop's end goes to
//! LoopEnd), ActivateSpawned at 1.3667, AimDash 1.3667-1.669, SetGravity 0 for 0.4703 s from 1.3667, the Hitframe
//! 1.6000-1.7387 (damage 14). No MoveCommand, no launch and no root motion: the dash is the one
//! K2_SetActorLocation below, and the punch is thrown where it put him.

use super::perks::{set_multipliers_server, sniper_buff};
use super::state::superman_mut;
use crate::Real;
use crate::State;
use crate::actors::{ActorClass, ActorId, ActorRef, WorldActorClass, world_actor::WorldActorState};
use crate::consts::{DELTA_SECONDS, SOLID_NOPLATFORM};
use crate::events::{Event, SoundKey};
use crate::fighter::body::{
    FIND_FLOOR_DIST, PlatformRef, handle_platform_collision, height_at, ignores_platform, plat, set_grounded,
    span_near, walkable_at,
};
use crate::fighter::common::{actor_data, latent_frames};
use crate::fighter::match_side::{SpawnRequest, notify_props};
use crate::fighter::moves::{recall_move, settle};
use crate::fighter::traces::solids::QUERY_WORLDSTATIC;
use crate::fighter::{FighterId, actor_y, cap_radius, capsule_half, facing, in_hitstun, stand_y};
use crate::math::max;
use crate::math::ue::{DEG_TO_RAD, SMALL_NUMBER, cosf, deg_atan2, rotate_angle_axis, safe_normal, sinf, size2d};
use crate::name;
use crate::names::Name;
use crate::registry::entries::{ActorEntry, ActorHooks, SizeHooks, WorldActorHooks};
use crate::registry::props::{flag, num_in, obj};
use crate::registry::{Registry, RegistryError};
use crate::world::World;
use crate::world::blast::{BlastBounds, blast_box_loop, blast_timer_exit, new_blast_bounds, spawn_box};
use crate::world::stage_query::static_shapes;
use mvs_data::value::Props;

const MONTAGE: Name = name!("Superman_GroundAimPunch_Montage");
const LOOP: Name = name!("sfx_cha_superman_groundaim_beam_loop");
const CLASS: &str = "SupermanAimPunch_C";

/// `AimMarker` (SupermanAimPunch_C). Sim space: x = -UE Y, y = UE Z; the graph's own math is kept in UE (Y, Z).
///
/// The spawn (AnimNotify_Spawner without AttachToActor, socket None): the owner's location - (0, 0, 100) + the
/// notify's translation with Y mirrored by the facing; the actor is not attached.
/// ReceiveBeginPlay @9074: BaseFighterRef; for a locally controlled owner IsSniper from the perk, the pawn's line
/// widget of the facing's side hidden @9904 and sfx_cha_superman_groundaim_beam_loop posted on the actor's Ak
/// component @10026; then Delay(0.2) @10126 -> @78: K2_DestroyActor when the owner's current montage is not
/// Superman_GroundAimPunch_Montage.
/// ReceiveTick @4843 (locally controlled owner), C019AimPunch's statement for statement: the pivot (AnglePoint)
/// is the owner's location + LocationOffset with Y mirrored (200 behind the owner, 190 up). The actor's location
/// moves by (0, -sign(MoveX), sign(MoveY)) x DT x AimSpeed (each axis 0 without input; not normalised): P. The
/// cone: StartAimVector with Y mirrored (forward), rotated about X by (FacingLeft ? AimAngleMin : -AimAngleMax)
/// and by (FacingLeft ? AimAngleMax : -AimAngleMin), as angles DegAtan2(Y, Z): a1 and a2, and their directions
/// (DegSin, DegCos). The nearer of the two infinite lines through the pivot (FindClosestPointOnLine; the second
/// one on a tie) gives the boundary direction. r = max(min(|P - pivot|, IsSniper ? SniperAimDistance :
/// AimDistance), AimDistanceMin). The new location is pivot + Normal(P - pivot) x r when the angle of P - pivot
/// is inside [a2, a1] (inclusive; for a2 > a1: when it is outside [a1, a2]), else pivot + boundary x r; X = 0.
/// K2_SetActorLocation (sweep; the root has no collision) @8553, SetLocationServer @8593 (ServerLocation = it:
/// the replicated value is the local one here), UpdateSkillshotUI @8616 (the drawing: `bounds`, and what the view
/// reads from x / y / pivot). The same tick runs without input, so the marker follows the owner through the
/// clamp: 500 to 1550 from the pivot, within 35 degrees of level.
/// ActivateObject @314 (AnimNotify_ActivateSpawned): [`activate`]. HideUI @11830 (from ActivateObject and
/// ReceiveEndPlay): [`hide_ui`].
// Unverified: the actor is ticked with the other spawned actors, after the fighters, from its spawn frame on, and
// the Delay counts that frame; so ActivateObject (inside the owner's montage tick) reads the ServerLocation of the
// tick before. Doubles instead of floats. PF_Velocity_Ak and the object_marker_distance RTPC on the actor's Ak
// component @8630-8994 are not ported.
// IsSniper (the SniperPunch perk): BeginPlay's CheckHasBuff(Buff_Superman_SniperPunch_C) @9377-9579 on the owner,
// in the locally controlled branch (@9249); `sniper` below. The tick's reach takes SniperAimDistance with it
// (@8030), and ActivateObject's block @1376-2652 hands the dash's length to the buff ([`activate`]).
// Unverified: IsSniper is written on the owning client alone, and ActivateObject reads the actor's own copy
// wherever it runs; the port takes the owner as locally controlled, as for the tick (ServerLocation is the local
// one).
#[derive(State, Debug, Default)]
pub struct AimMarker {
    pub x: Real,
    pub y: Real,
    /// Before this tick's move (drawing).
    pub prev_x: Real,
    pub prev_y: Real,
    /// ServerLocation (the class default until the first tick).
    pub server_x: Real,
    pub server_y: Real,
    /// AnglePoint, written by each tick (the line widget's other end).
    pub pivot_x: Real,
    pub pivot_y: Real,
    /// UpdateSkillshotUI's once-gate @10242: PS_SupermanSkillBounds attached to the owner's root at the AnglePoint
    /// of the first tick (KeepWorld). 0 until then, else the owner's facing at that tick (the emitter's rotation
    /// and the side of the offset it keeps from the owner).
    pub bounds: Real,
    /// HideUI has run: the emitter destroyed, both line widgets hidden, the loop stopped.
    pub hidden: bool,
    /// The beam loop is sounding (posted at BeginPlay; the event leaves with the first tick).
    pub looping: bool,
    pub age: i32,
    /// BeginPlay's Delay(0.2), the latent ticks left.
    pub check_in: Real,
    /// IsSniper (BeginPlay @9579); the trace writes its key only when true.
    pub sniper: bool,
    /// SpawnedActorParent's CheckBlastBoxTimer: its next expiry.
    pub blast_at: crate::Clock,
    /// BlastBoxBoundsMin / Max, stored by the BeginPlay loop.
    pub blast_bounds: BlastBounds,
}

fn marker(w: &World, id: ActorId) -> Option<&AimMarker> {
    match &w.actors.actors.get(id)?.state {
        WorldActorState::AimMarker(m) => Some(m),
        _ => None,
    }
}

fn marker_mut(w: &mut World, id: ActorId) -> Option<&mut AimMarker> {
    match &mut w.actors.actors.get_mut(id)?.state {
        WorldActorState::AimMarker(m) => Some(m),
        _ => None,
    }
}

fn loop_key(seq: i32) -> SoundKey {
    SoundKey { tag: name!("smaim"), n: seq }
}

/// The spawn (`new AimMarker`): the owner's place + the notify's translation; then SpawnedActorParent's BeginPlay
/// blast-box loop at that place (the class's BeginPlay @9074 calls the parent's first; DestroyOnExitedArena? the
/// parent's default, no ForceSpawnPositionInsideBlastBox), as C019AimPunch's (`chars/morty/aim_punch.rs`).
// Unverified: a marker the loop destroys runs none of its own BeginPlay here (the build's goes on behind the destroy:
// the loop's post on a destroyed actor's Ak component, the Delay).
fn spawn(w: &mut World, f: FighterId, r: &SpawnRequest) -> (WorldActorState, bool) {
    let n = notify_props(w, f, r.n);
    let t = obj(n, "Transforms").and_then(|t| obj(t, "Translation"));
    let (ty, tz) =
        (t.and_then(|t| crate::registry::props::num(t, "Y")), t.and_then(|t| crate::registry::props::num(t, "Z")));
    let x = w.fighter(f).body.x + ty.unwrap_or(0.0) * facing(w, f);
    let y = actor_y(w, f) - 100.0 + tz.unwrap_or(0.0);
    let mut m = AimMarker {
        x,
        y,
        prev_x: x,
        prev_y: y,
        check_in: latent_frames(0.2),
        sniper: sniper_buff(w, f).is_some(),
        blast_at: f64::from(0.1_f32),
        blast_bounds: new_blast_bounds(),
        ..AimMarker::default()
    };
    let data = w.data.clone();
    let p = actor_data(w, &data, ActorRef::Fighter(f), r.cls).map(|d| &d.p);
    let (doe, force) =
        p.map_or((false, false), |p| (flag(p, "DestroyOnExitedArena?"), flag(p, "ForceSpawnPositionInsideBlastBox")));
    let sb = spawn_box(w);
    let (mut bx, mut by, mut px, mut py) = (m.x, m.y, m.prev_x, m.prev_y);
    let dead = blast_box_loop(&sb, &mut bx, &mut by, Some((&mut px, &mut py)), force, &mut m.blast_bounds, doe);
    (m.x, m.y, m.prev_x, m.prev_y) = (bx, by, px, py);
    (WorldActorState::AimMarker(m), dead)
}

// The tick's math (nothing is made per tick): DegAtan2(Y, Z), a (Y, Z) vector turned about X by d degrees
// (RotateAngleAxis(v, d, (1, 0, 0)): a unit axis, which the node's GetSafeNormal keeps), DegSin / DegCos, and the
// distance VSize(P - FindClosestPointOnLine(P, pivot, (dy, dz))) in UE (Y, Z).
// Unverified: FindClosestPointOnLine as the 4.26 source has it (origin + SafeDir x ((P - origin) . SafeDir), SafeDir
// the direction's GetSafeNormal), not read in the exe.
fn rot(y: Real, z: Real, d: Real) -> [Real; 2] {
    let [_, ry, rz] = rotate_angle_axis([0.0, y, z], d, [1.0, 0.0, 0.0]);
    [ry, rz]
}

/// DegSin, DegCos (142f68970 / 142f688e0): the CRT's sinf / cosf of A x (pi / 180).
fn deg_sin_cos(a: Real) -> (Real, Real) {
    (sinf(a * DEG_TO_RAD), cosf(a * DEG_TO_RAD))
}

fn line_off(qy: Real, qz: Real, py: Real, pz: Real, dy: Real, dz: Real) -> Real {
    let [_, ny, nz] = safe_normal([0.0, dy, dz], SMALL_NUMBER);
    let k = (qy - py) * ny + (qz - pz) * nz;
    let (cy, cz) = (py + ny * k, pz + nz * k);
    size2d([qy - cy, qz - cz])
}

/// One actor tick.
fn step(w: &mut World, id: ActorId) {
    let Some(a) = w.actors.actors.get(id) else { return };
    let (f, cls, seq) = (a.owner, a.cls, a.seq);
    let data = w.data.clone();
    let none = Props::new();
    let p = actor_data(w, &data, ActorRef::Fighter(f), cls).map_or(&none, |d| &d.p);
    let face = facing(w, f);
    let fl = face < 0.0;
    let fi = w.fighter(f);
    let (fx, fid) = (fi.body.x, fi.id);
    let (inp_x, inp_y) = (fi.controls.inp.x, fi.controls.inp.y);
    let fy = actor_y(w, f);
    let playing = w.fighter(f).montages.montage.as_ref().is_some_and(|m| m.montage.name == MONTAGE);
    let g = |k: &str, sub: &str| num_in(p, k, sub).unwrap_or(0.0);
    let gn = |k: &str| crate::registry::props::num(p, k).unwrap_or(0.0);
    let doe = flag(p, "DestroyOnExitedArena?");
    let Some(m) = marker_mut(w, id) else { return };
    m.age += 1;
    // SpawnedActorParent's CheckBlastBoxExit on its 0.1 s looping timer, the spawn frame not counted, ahead of the
    // tick's move (as C019AimPunch's).
    if blast_timer_exit(doe, &mut m.blast_at, &m.blast_bounds, crate::real(m.age - 1), m.x, m.y) {
        if let Some(a) = w.actors.actors.get_mut(id) {
            a.dead = true;
        }
        return;
    }
    m.prev_x = m.x;
    m.prev_y = m.y;
    let post = !m.looping && !m.hidden;
    if post {
        // BeginPlay's post, sent here: the actor has its number by its first tick, which is in its spawn frame.
        m.looping = true;
    }
    let off_y = g("LocationOffset", "Y");
    let py = -fx + if fl { off_y } else { -off_y };
    let pz = fy + g("LocationOffset", "Z");
    // MoveXAxisValue x -1, MoveYAxisValue x -1
    let (mx, my) = (-inp_x, -inp_y);
    let step = DELTA_SECONDS * gn("AimSpeed");
    let qy = -m.x
        + (if mx == 0.0 {
            0.0
        } else if mx > 0.0 {
            1.0
        } else {
            -1.0
        }) * step;
    let qz = m.y
        + (if my == 0.0 {
            0.0
        } else if my > 0.0 {
            -1.0
        } else {
            1.0
        }) * step;
    let vy = g("StartAimVector", "Y") * (if fl { 1.0 } else { -1.0 });
    let vz = g("StartAimVector", "Z");
    let (amax, amin) = (gn("AimAngleMax"), gn("AimAngleMin"));
    let (t1, t2) = if fl { (amin, amax) } else { (-amax, -amin) };
    let [r1y, r1z] = rot(vy, vz, t1);
    let [r2y, r2z] = rot(vy, vz, t2);
    let (a1, a2) = (deg_atan2(r1y, r1z), deg_atan2(r2y, r2z));
    let (d1y, d1z) = deg_sin_cos(a1);
    let (d2y, d2z) = deg_sin_cos(a2);
    let (wy, wz) = (qy - py, qz - pz);
    let second = line_off(qy, qz, py, pz, d2y, d2z) <= line_off(qy, qz, py, pz, d1y, d1z);
    let len = size2d([wy, wz]);
    let [_, uy, uz] = safe_normal([0.0, wy, wz], SMALL_NUMBER); // GetDirectionUnitVector(pivot, P) @7765
    let ang = deg_atan2(uy, uz);
    let lim = if m.sniper { gn("SniperAimDistance") } else { gn("AimDistance") };
    let r = max(if len <= lim { len } else { lim }, gn("AimDistanceMin"));
    let inside = ang >= a1 && ang <= a2;
    let free = if a2 > a1 { !inside } else { ang >= a2 && ang <= a1 };
    let (dy, dz) = if free {
        (uy, uz)
    } else if second {
        (d2y, d2z)
    } else {
        (d1y, d1z)
    };
    let (ny, nz) = (py + dy * r, pz + dz * r);
    m.x = -ny;
    m.y = nz;
    m.server_x = m.x;
    m.server_y = m.y;
    m.pivot_x = -py;
    m.pivot_y = pz;
    if m.bounds == 0.0 {
        m.bounds = face;
    }
    let mut gone = false;
    if m.check_in > 0.0 {
        m.check_in -= 1.0;
        gone = m.check_in == 0.0 && !playing;
    }
    if post {
        w.events.push(Event::SoundStart { fighter: fid, ev: LOOP, key: loop_key(seq) });
    }
    if gone && let Some(a) = w.actors.actors.get_mut(id) {
        a.dead = true;
    }
}

/// HideUI @11830: PS_SkillBounds destroyed, both of the pawn's line widgets hidden, and
/// sfx_cha_superman_groundaim_beam_loop_stop posted on the actor's Ak component @11765.
// Unverified: the stop event is sent as the loop's stop with the port's default fade, its own action's fade not
// read; the loop sounds from the owner, not from the marker's own place.
fn hide_ui(w: &mut World, id: ActorId) {
    let Some(a) = w.actors.actors.get(id) else { return };
    let (f, seq) = (a.owner, a.seq);
    let Some(m) = marker_mut(w, id) else { return };
    m.hidden = true;
    if !m.looping {
        return;
    }
    m.looping = false;
    let fighter = w.fighter(f).id;
    w.events.push(Event::SoundStop { fighter, key: loop_key(seq), fade: None, ev: None, curve: None, cut: false });
}

/// ActivateObject @314, in the graph's order (O the owner's location, M the ServerLocation):
/// 1. HideUI @324. Nothing more while the owner's CheckHitstunActive @338: the actor stays (the Spawner notify's
///    end destroys it).
/// 2. T = SelectVector(M, O + (0, 0, 400), pick) @902, pick = FacingLeft ? O.Y > M.Y : M.Y > O.Y (@826 / @864; the
///    select's index is BPI_IsFacingLeft). FacingLeft's forward is +Y (the tick mirrors StartAimVector (0, 1, 0)
///    for the other facing), so M is picked only when it is BEHIND the owner; the tick keeps the marker at least
///    209 ahead (the pivot 200 behind him, the least radius 500, 35 degrees), so with a ticked marker T is always
///    the point 400 over him. Not an "in front" test: the bytecode is as here.
/// 3. dir = Normal(T - O) @1001; D = M - dir x 250 @1089 (from M, whichever T was): with a ticked marker the
///    point 250 under it.
/// 4. Trace 1 @1135: CapsuleTraceSingle O -> D, radius 80, half height 100, trace type 5
///    (ProjectileGroundNoPlatform: of the port's geometry the stage block alone; a pass-through platform does not
///    block it). P1 = the hit's Location, the swept capsule's centre at the contact, or D without a hit.
/// 5. The Sniper perk's block @1376-2652: with IsSniper and the owner's Buff_Superman_SniperPunch (CheckHasBuff
///    @1390, the cast @1606), Vector_Distance(P1, O) / SniperAimDistance clamped to 0..1 @2513-2605 goes to the
///    buff's SetMultipliersServer @2652 (`perks`): P1 is trace 1's again (the block recomputes T and D from the same
///    O and M and breaks the same OutHit @2225-2388). DashLocationMulticast(O) @2767: the owner's
///    Superman_AnimBP.AimDashFromLocation = O @12065 (the look-at of the dash pose, with aimOn).
/// 6. Trace 2 @3611: CapsuleTraceSingleForObjects P1 + (0, 0, 260) -> P1, radius 92.0 and half height 260.0 (two
///    literals of the graph: his capsule template's size, not read from the capsule), object types [0]
///    WorldStatic: the stage block. P2 = the hit's Location, or P1 without a hit @4685. A trace that starts inside
///    the block reports its start (native): P1 + (0, 0, 260), still inside (a wall trace 1 stopped at, 80 from it,
///    is 12 inside the 92 capsule).
/// 7. SetDashLocationMulticast(P2 x (0, 1, 1)) @4790 -> the owner's K2_SetActorLocation (no sweep, no teleport)
///    @15: [`dash_to`]. K2_DestroyActor @299.
///
/// The stage's solids join both (`static_grounds`: trace 1 those that block type 5, trace 2 the WorldStatic ones).
// Unverified: of the rest, both traces see the stage block alone. WorldStatic bodies of other pawns' spawned actors
// (Wonder Woman's WW_SphereBlast / WW_BracerShield capsules have object type WorldStatic with collision on) may
// block trace 2 in the build, and a SpawnedPlatformParent piece either trace; neither is tested. CheckHitstunActive
// is taken as inHitstun. The traces are swept capsules in the play plane (capsuleSweep).
fn activate(w: &mut World, id: ActorId) {
    hide_ui(w, id);
    let Some(a) = w.actors.actors.get(id) else { return };
    let (f, cls) = (a.owner, a.cls);
    if in_hitstun(w, f) {
        return;
    }
    let Some(m) = marker(w, id) else { return };
    let (mx, my, sniper) = (m.server_x, m.server_y, m.sniper);
    let ox = w.fighter(f).body.x;
    let oy = actor_y(w, f);
    // UE Y = -x: O.Y > M.Y is mx > ox.
    let pick = if facing(w, f) < 0.0 { mx > ox } else { mx < ox };
    let (tx, ty) = if pick { (mx, my) } else { (ox, oy + 400.0) };
    let (lx, ly) = (tx - ox, ty - oy);
    let [_, ux, uy] = safe_normal([0.0, lx, ly], SMALL_NUMBER); // GetDirectionUnitVector(O, T) @1001
    let dx = mx - ux * 250.0;
    let dy = my - uy * 250.0;
    let gs = static_shapes(w, SOLID_NOPLATFORM, name!("Superman aim punch trace 1"));
    let t1 = gs.view().capsule_sweep_all(ox, oy, dx, dy, 80.0, 100.0);
    let (p1x, p1y) = match t1 {
        None => (dx, dy),
        Some(t) => (ox + (dx - ox) * t, oy + (dy - oy) * t),
    };
    let sb = if sniper { sniper_buff(w, f) } else { None };
    // Vector_Distance(P1, O) in float32 (FVector::Dist: the components, their squares, the sum and the root each
    // rounded; the depth axis is 0 in the port's plane), then / SniperAimDistance @2513-2605.
    if let Some(sb) = sb {
        let ddx = p1x - ox;
        let ddy = p1y - oy;
        let dist = ((ddx * ddx) + (ddy * ddy)).sqrt();
        let data = w.data.clone();
        let reach = actor_data(w, &data, ActorRef::Fighter(f), cls)
            .and_then(|d| crate::registry::props::num(&d.p, "SniperAimDistance"))
            .unwrap_or(0.0);
        set_multipliers_server(w, f, sb, dist / reach);
    }
    if let Some(s) = superman_mut(w, f) {
        s.aim_dash_from_x = ox;
        s.aim_dash_from_y = oy;
    }
    let gs2 = static_shapes(w, QUERY_WORLDSTATIC, name!("Superman aim punch trace 2"));
    let t2 = gs2.view().capsule_sweep_all(p1x, p1y + 260.0, p1x, p1y, 92.0, 260.0);
    dash_to(w, f, p1x, t2.map_or(p1y, |t| p1y + 260.0 * (1.0 - t)));
    if let Some(a) = w.actors.actors.get_mut(id) {
        a.dead = true;
    }
}

/// The owner's K2_SetActorLocation(no sweep, no teleport) @15: the actor's place alone changes; the velocity and the
/// movement mode stay, and the movement component's next tick meets the new place: Walking finds its floor again
/// (none: StartFalling), Falling sweeps on from there, and a capsule left inside geometry is moved out by the next
/// sweeping move alone.
/// A walker here (PhysWalking 14293b840, FindFloor 14292b400, ComputeFloorDist 142926820, AdjustFloorHeight
/// 14291b150): the next movement tick's one floor query comes after its move and finds a top up to 102.4 under his
/// capsule's bottom, or one his capsule is sunk into while its centre is still above that top; AdjustFloorHeight has
/// no clamp and puts him at the walking height over it. So he keeps walking, on the stage block or on a platform his
/// capsule does not ignore. No such floor makes him a faller at once, with no velocity. The montage's SetGravity 0
/// (to 1.837 s) begins in the same montage tick and lands after the next movement tick (NetGravity), so he takes one
/// tick of his own gravity (85.75 u/s down), keeps that speed through the Hitframe and falls faster when the notify
/// ends.
/// The platforms' ignore list is the one of this frame's pawn tick (HandlePlatformCollision, with the place he
/// dashed from): the dash is made in the mesh tick after it, and the next movement tick comes before the next pawn
/// tick. The port's own call of the frame ran ahead of the move, so it is made again here.
// Deviation: he is put at the walking height at once. The build's standing walker sunk d into a platform is first
// lifted to min(d, 74) + 2.275 over its top (the MTD pop, then the floor distance clamped at -74) and comes down to
// 2.15 in the tick after; a moving one is there in the first substep.
// Deviation: the fall starts here, where the build's starts in the next movement tick (at once for a walker without
// velocity; for a moving one, with the montage's StopWalkOffEdge, CheckFall 142923aa0 refuses and RevertMove zeroes
// the velocity each tick: he hangs in Walking until a tick without a move). A capsule inside the stage block is moved
// out by his next sweeping move (settle).
fn dash_to(w: &mut World, f: FighterId, x: Real, y: Real) {
    let walking = w.fighter(f).body.on_floor;
    if walking {
        let vy = w.fighter(f).body.vy;
        handle_platform_collision(w, f, vy);
    }
    recall_move(w, f, x, y);
    if walking {
        if let Some(floor) = floor_after_teleport(w, f, x, y) {
            w.fighter_mut(f).platforms.base = match floor {
                Floor::Stage => None,
                Floor::On(p) => Some(p),
            };
            let sy = stand_y(w, f);
            w.fighter_mut(f).body.y = sy;
            return;
        }
        set_grounded(w, f, false);
        let b = &mut w.fighter_mut(f).body;
        b.vx = 0.0;
        b.vy = 0.0;
    }
    settle(w, f);
}

/// The floor a walker has: the stage block or a platform.
#[derive(Clone, Copy)]
enum Floor {
    Stage,
    On(PlatformRef),
}

/// The floor a walker set down by the dash has at the movement component's next tick: the highest top under him that
/// lies no more than 102.4 under his capsule's bottom and under his capsule's centre. A top the capsule's centre is
/// under is not found (the line trace from the centre starts inside the box).
/// Each by the walker's flat-base rule: a box while x is within R of its span (the stage floor
/// and the stage solids' boxes: R past their ends, read in the exe), a stage platform, a ship's run of pieces or a
/// spawned box while x is within R of its span (`span_near`), a polygon solid where its flat-base floor at x is
/// walkable, at that floor's height.
fn floor_after_teleport(w: &World, f: FighterId, x: Real, y: Real) -> Option<Floor> {
    let bottom = y - capsule_half(w, f);
    let centre = y;
    let r = cap_radius(w, f);
    let ok = |top: Real| bottom - top <= FIND_FLOOR_DIST && top < centre;
    let mut out = None;
    let mut best = Real::NEG_INFINITY;
    let g = w.stage.ground;
    if x >= crate::math::narrow(g.left) - r && x <= crate::math::narrow(g.right) + r && ok(crate::math::narrow(g.top)) {
        out = Some(Floor::Stage);
        best = crate::math::narrow(g.top);
    }
    for &i in &w.fighter(f).platforms.stage_platforms {
        let pr = PlatformRef::Stage(i);
        let p = plat(w, pr);
        if p.top <= best || span_near(w, &p, x, r).is_none() || !ok(p.top) || ignores_platform(w, f, pr) {
            continue;
        }
        out = Some(Floor::On(pr));
        best = p.top;
    }
    for &pr in &w.fighter(f).platforms.platforms {
        let p = plat(w, pr);
        if p.dead || p.wall || ignores_platform(w, f, pr) {
            continue;
        }
        let top = if p.poly {
            if !walkable_at(w, &p, x, r) {
                continue;
            }
            height_at(w, &p, x, r)
        } else if span_near(w, &p, x, r).is_none() {
            continue;
        } else {
            p.top
        };
        if top <= best || !ok(top) {
            continue;
        }
        out = Some(Floor::On(pr));
        best = top;
    }
    out
}

/// The class's entry. The Sphere is hidden and has no collision read and the bounds emitter rides the owner's root:
/// nothing reads its size.
pub fn register(r: &mut Registry) -> Result<(), RegistryError> {
    let hooks = WorldActorHooks {
        spawn: Some(spawn),
        step: Some(step),
        activate: Some(activate),
        end: Some(hide_ui),
        size: Some(SizeHooks::default()),
        ..WorldActorHooks::default()
    };
    r.actors
        .enter(CLASS, ActorEntry::new(ActorClass::World(WorldActorClass::SupermanAimPunch), ActorHooks::World(hooks)))
}
