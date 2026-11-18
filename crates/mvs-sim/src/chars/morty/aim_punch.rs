//! The aim punch: C019AimPunch, an `actors::lists::AimPunch` of the World's typed lists
//! (C019_Attack_Special_Air_F's aimer: its move, its activation that places the owner).
//!
//! C019AimPunch_C (Morty's air side special: the point he teleports to). Sim space: x = -UE Y, y = UE Z; the graph's
//! own math is kept in UE (Y, Z). AnimNotify_Spawner without AttachToActor, socket None: CompSpawnActor starts from
//! the owner's location - (0, 0, 100) and adds the notify's translation with Y mirrored by the facing.
//! ReceiveBeginPlay: Delay(0.2) -> K2_DestroyActor when the owner's current montage is not
//! C019_Attack_Special_Air_F_Montage. ReceiveTick: the pivot (AnglePoint) is the owner's location + LocationOffset
//! with Y mirrored; the actor moves by (0, -sign(MoveX), sign(MoveY)) x DT x AimSpeed; the cone of StartAimVector
//! rotated by AimAngleMin / Max keeps it inside; r = max(min(|P - pivot|, AimDistance), AimDistanceMin).
//! ActivateObject: nothing while the owner's CheckHitstunActive. Else CapsuleTraceSingle (radius 80, half height 100,
//! trace type 5 ProjectileGroundNoPlatform: the stage block) from the owner's location to ServerLocation - Normal(
//! ServerLocation - the owner's location) x 250; the owner is set to the hit's Location, or to the end without a hit,
//! and the actor destroys itself.
// Unverified: the actor is ticked with the other spawned actors, after the fighters, from its spawn frame on, and the
// Delay counts that frame; the trace is a swept capsule in the play plane
// without the engine's pull-back from the surface; PF_Velocity_Ak and the object_marker_distance RTPC on the actor's
// Ak component are not ported.

use crate::Real;
use crate::actors::ActorRef;
use crate::actors::AimPunchId;
use crate::actors::lists::AimPunch;
use crate::consts::DELTA_SECONDS;
use crate::fighter::common::{actor_data, latent_frames};
use crate::fighter::match_side::{SpawnRequest, notify_props};
use crate::fighter::moves::V2;
use crate::fighter::{FighterId, actor_y, facing, in_hitstun};
use crate::math::ue::{SMALL_NUMBER, cosf, deg_atan2, deg_to_rad, rotate_angle_axis, safe_normal, sinf, size2d};
use crate::math::{max, to_int32};
use crate::name;
use crate::names::Name;
use crate::registry::props::{flag, num, num_in, obj};
use crate::world::World;
use crate::world::blast::{blast_box_loop, blast_timer_exit, new_blast_bounds, spawn_box};

const MONTAGE: Name = name!("C019_Attack_Special_Air_F_Montage");
const CLASS: Name = name!("C019AimPunch_C");

/// `new AimPunch(f, a, r.p)`.
pub fn new_aim_punch(w: &mut World, f: FighterId, r: &SpawnRequest) -> AimPunch {
    let n = notify_props(w, f, r.n);
    let t = obj(n, "Transforms").and_then(|t| obj(t, "Translation"));
    let (ty, tz) = (t.and_then(|t| num(t, "Y")).unwrap_or(0.0), t.and_then(|t| num(t, "Z")).unwrap_or(0.0));
    let x = w.fighter(f).body.x + ty * facing(w, f);
    let y = actor_y(w, f) - 100.0 + tz;
    let data = w.data.clone();
    let (doe, force) = actor_data(w, &data, ActorRef::Fighter(f), CLASS).map_or((false, false), |d| {
        (flag(&d.p, "DestroyOnExitedArena?"), flag(&d.p, "ForceSpawnPositionInsideBlastBox"))
    });
    let mut ap = AimPunch {
        cls: CLASS,
        seq: -1,
        x,
        y,
        prev_x: x,
        prev_y: y,
        server_x: 0.0,
        server_y: 0.0,
        age: 0,
        dead: false,
        blast_at: f64::from(0.1_f32),
        blast_bounds: new_blast_bounds(),
        check_in: to_int32(latent_frames(0.2)),
        owner: f,
    };
    // SpawnedActorParent's BeginPlay blast-box loop at the spawn place.
    let sb = spawn_box(w);
    let (mut bx, mut by, mut px, mut py) = (ap.x, ap.y, ap.prev_x, ap.prev_y);
    if blast_box_loop(&sb, &mut bx, &mut by, Some((&mut px, &mut py)), force, &mut ap.blast_bounds, doe) {
        ap.dead = true;
    }
    ap.x = bx;
    ap.y = by;
    ap.prev_x = px;
    ap.prev_y = py;
    ap
}

/// `AimPunch.activate(boxes)`: where the owner is set, if anywhere.
pub fn aim_punch_activate(w: &mut World, ap: AimPunchId, boxes: crate::world::stage_query::Shapes) -> Option<V2> {
    let a = w.actors.aim_punches.get(ap)?;
    let f = a.owner;
    if in_hitstun(w, f) {
        return None;
    }
    let (sx, sy) = (a.server_x, a.server_y);
    let ax = w.fighter(f).body.x;
    let ay = actor_y(w, f);
    let (dx, dy) = (sx - ax, sy - ay);
    let n = safe_normal([0.0, dx, dy], SMALL_NUMBER); // GetDirectionUnitVector @842
    let bx = sx - n[1] * 250.0;
    let by = sy - n[2] * 250.0;
    let t = boxes.capsule_sweep_all(ax, ay, bx, by, 80.0, 100.0);
    if let Some(a) = w.actors.aim_punches.get_mut(ap) {
        a.dead = true;
    }
    Some(match t {
        None => V2 { x: bx, y: by },
        Some(t) => V2 { x: ax + (bx - ax) * t, y: ay + (by - ay) * t },
    })
}

/// DegAtan2 (@3888).
fn deg(y: Real, z: Real) -> Real {
    deg_atan2(y, z)
}

/// A (Y, Z) vector turned about X by `d` degrees: RotateAngleAxis(v, d, (1, 0, 0)) (@3783).
fn rot(y: Real, z: Real, d: Real) -> (Real, Real) {
    let r = rotate_angle_axis([0.0, y, z], d, [1.0, 0.0, 0.0]);
    (r[1], r[2])
}

/// (DegSin(a), DegCos(a)) (@3934, @3971: the CRT of a x pi/180).
fn dir(a: Real) -> (Real, Real) {
    (sinf(deg_to_rad(a)), cosf(deg_to_rad(a)))
}

/// The distance from (wy, wz) to the line through the pivot along `d` (FindClosestPointOnLine normalises it with
/// GetSafeNormal; VSize @5052).
fn off(wy: Real, wz: Real, d: (Real, Real)) -> Real {
    let n = safe_normal([0.0, d.0, d.1], SMALL_NUMBER);
    let (ny, nz) = (n[1], n[2]);
    let k = wy * ny + wz * nz;
    size2d([wy - ny * k, wz - nz * k])
}

/// The sign of `v` times `pos` (0 for 0).
fn sgn(v: Real, pos: Real) -> Real {
    if v == 0.0 {
        0.0
    } else if v > 0.0 {
        pos
    } else {
        -pos
    }
}

/// `AimPunch.step`.
pub fn aim_punch_step(w: &mut World, id: AimPunchId) {
    let Some(a) = w.actors.aim_punches.get(id) else { return };
    let f = a.owner;
    let data = w.data.clone();
    let p = actor_data(w, &data, ActorRef::Fighter(f), CLASS).map(|d| &d.p);
    let doe = p.is_some_and(|p| flag(p, "DestroyOnExitedArena?"));
    let face = facing(w, f);
    let fl = face < 0.0;
    let fx = w.fighter(f).body.x;
    let fy = actor_y(w, f);
    let (inp_x, inp_y) = (w.fighter(f).controls.inp.x, w.fighter(f).controls.inp.y);
    let playing = w.fighter(f).montages.montage.as_ref().is_some_and(|m| m.montage.name == MONTAGE);
    let g = |k: &str, sub: &str| p.and_then(|p| num_in(p, k, sub)).unwrap_or(0.0);
    let gn = |k: &str| p.and_then(|p| num(p, k)).unwrap_or(0.0);
    let Some(a) = w.actors.aim_punches.get_mut(id) else { return };
    a.age += 1;
    if blast_timer_exit(doe, &mut a.blast_at, &a.blast_bounds, crate::real(a.age - 1), a.x, a.y) {
        a.dead = true;
        return;
    }
    a.prev_x = a.x;
    a.prev_y = a.y;
    let off_y = g("LocationOffset", "Y");
    let py = -fx + if fl { off_y } else { -off_y };
    let pz = fy + g("LocationOffset", "Z");
    // MoveXAxisValue x -1, MoveYAxisValue x -1
    let (mx, my) = (-inp_x, -inp_y);
    let step = DELTA_SECONDS * gn("AimSpeed");
    let qy = -a.x + sgn(mx, 1.0) * step;
    let qz = a.y + sgn(my, -1.0) * step;
    let vy = g("StartAimVector", "Y") * (if fl { 1.0 } else { -1.0 });
    let vz = g("StartAimVector", "Z");
    let (amax, amin) = (gn("AimAngleMax"), gn("AimAngleMin"));
    let r1 = rot(vy, vz, if fl { amin } else { -amax });
    let r2 = rot(vy, vz, if fl { amax } else { -amin });
    let a1 = deg(r1.0, r1.1);
    let a2 = deg(r2.0, r2.1);
    let d1 = dir(a1);
    let d2 = dir(a2);
    let (wy, wz) = (qy - py, qz - pz);
    let bd = if off(wy, wz, d2) <= off(wy, wz, d1) { d2 } else { d1 };
    let len = size2d([wy, wz]); // VSize @5210
    let u = safe_normal([0.0, wy, wz], SMALL_NUMBER); // GetDirectionUnitVector @5302
    let (uy, uz) = (u[1], u[2]);
    let ang = deg(uy, uz);
    let lim = if p.is_some_and(|p| flag(p, "IsSniper")) { gn("SniperAimDistance") } else { gn("AimDistance") };
    let r = max(if len <= lim { len } else { lim }, gn("AimDistanceMin"));
    let free = if a2 > a1 { !(ang >= a1 && ang <= a2) } else { ang >= a2 && ang <= a1 };
    let (ny, nz) = if free { (py + uy * r, pz + uz * r) } else { (py + bd.0 * r, pz + bd.1 * r) };
    a.x = -ny;
    a.y = nz;
    a.server_x = a.x;
    a.server_y = a.y;
    if a.check_in > 0 {
        a.check_in -= 1;
        if a.check_in == 0 && !playing {
            a.dead = true;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rot_quarter_turn() {
        let (y, z) = rot(1.0, 0.0, 90.0);
        // sin / cos of 90 degrees through float32 pi: y is -2^-23, z one step below 1
        assert_eq!((y, z), (-1.1920929e-7, 0.9999999));
    }

    #[test]
    fn line_offset_is_perpendicular_distance() {
        assert_eq!(off(3.0, 4.0, (1.0, 0.0)), 4.0);
        // a zero direction measures from the pivot
        assert_eq!(off(3.0, 4.0, (0.0, 0.0)), 5.0);
    }

    #[test]
    fn deg_of_axes() {
        assert_eq!(deg(1.0, 0.0), 90.0);
    }
}
