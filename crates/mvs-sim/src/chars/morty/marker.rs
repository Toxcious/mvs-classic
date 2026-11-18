//! The marker: C019Marker, an `actors::lists::Marker` of the World's typed lists (its
//! timeline, its trail, its safe location for the recall). actors.rs's marker functions call it.
//!
//! C019Marker_C (Morty's down special). The actor stays where it was spawned (BeginPlay: EnableProjectileMovement =
//! false, ProjectileMovement.Deactivate) and records its owner's route in its Spline component; ActivateObject moves
//! the owner back along it. Sim space: x = -UE Y, y = UE Z.
//! Comp_Spawner CompSpawnActor (ubergraph 978-2417): the actor's X, the socket's (Y, Z) or, with socket None, the
//! owner's actor location - (0, 0, 100); plus the notify translation (Y forward); yaw 90 facing left, else -90, so
//! the actor's local X is the owner's facing direction.
// Deviation: the attach / pick-up handlers left from Garnet's marker are not ported; the actor is ticked with the
// other spawned actors, after the fighters (ReceiveTick, the latent Delay, the timeline, then the timers); the
// spline is kept in world axes in doubles (the build holds it in the component's local space in floats) and without
// the depth axis (the default points' local Y of -0.002); ReparamStepsPerSegment 10 is the stock constructor's value
// (the constructor is not read in the exe).
// `!(a >= b)` is true for a NaN.
#![allow(clippy::neg_cmp_op_on_partial_ord)]

use crate::Real;
use crate::actors::ActorRef;
use crate::actors::MarkerId;
use crate::actors::lists::{Marker, Pt2, SplinePt};
use crate::actors::spline::{REPARAM_STEPS, build_reparam_table, location_at_distance};
use crate::combat::HitPacket;
use crate::consts::DELTA_SECONDS;
use crate::fighter::anim::{put_pose, sampled_boxes};
use crate::fighter::body::{ch_cap_half, ch_cap_radius};
use crate::fighter::common::{actor_data, actor_def, char_data, knock_back_shapes, latent_frames};
use crate::fighter::match_side::SpawnRequest;
use crate::fighter::moves::recall_move;
use crate::fighter::{FighterId, actor_y, facing, mesh_wx, mesh_wy};
use crate::math::ue::{SMALL_NUMBER, safe_normal, size2d};
use crate::math::{max, min, to_int32};
use crate::registry::props::{arr, flag, num, obj, text};
use crate::world::World;
use crate::world::blast::{blast_box_loop, blast_timer_exit, new_blast_bounds, spawn_box};
use mvs_data::value::Props;

/// What one marker tick ended with (`step`'s return).
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub enum MarkerEnd {
    /// The Delay(3.5) fired on a marker that is not active (ActivateMontage).
    Auto,
    /// The MoveAlongPath timeline finished.
    Done,
    /// The MaxLifetime timer.
    Life,
    /// CheckBlastBoxTimer found it outside its stored bounds.
    Blast,
}

/// A segment of the rewind trail (`Seg`): its ends and their tangents, sim space.
#[derive(Copy, Clone, PartialEq, Debug, Default)]
pub struct Seg {
    pub ax: Real,
    pub ay: Real,
    pub atx: Real,
    pub aty: Real,
    pub bx: Real,
    pub by: Real,
    pub btx: Real,
    pub bty: Real,
}

fn mk(w: &World, m: MarkerId) -> &Marker {
    w.actors.markers.get(m).expect("a live marker")
}

fn mk_mut(w: &mut World, m: MarkerId) -> &mut Marker {
    w.actors.markers.get_mut(m).expect("a live marker")
}

/// The class's data (`p`).
fn class_p(w: &World, m: &Marker) -> Props {
    actor_def(w, ActorRef::Fighter(m.owner), m.cls).map(|d| d.p.clone()).unwrap_or_default()
}

/// The owner's location (x, actorY).
fn owner_at(w: &World, f: FighterId) -> (Real, Real) {
    (w.fighter(f).body.x, actor_y(w, f))
}

/// `new Marker(f, r.attackId, a, r.p, spawnBox(w))`.
pub fn new_marker(w: &mut World, f: FighterId, r: &SpawnRequest) -> Marker {
    let notify = crate::world::spawning::request_props(w, f, r);
    let data = w.data.clone();
    let def = actor_data(w, &data, ActorRef::Fighter(f), r.cls).expect("the marker's class data");
    let (p, comps) = (&def.p, &def.components);
    let cap_radius = obj(comps, "Capsule").and_then(|c| num(c, "CapsuleRadius")).unwrap_or(0.0);
    let empty = Props::new();
    let hit = HitPacket::from_props(&w.names, obj(p, "Hit Box Damage").unwrap_or(&empty), None);
    let fc = facing(w, f);
    let t = obj(&notify, "Transforms").and_then(|t| obj(t, "Translation"));
    let (ty, tz) = (t.and_then(|t| num(t, "Y")).unwrap_or(0.0), t.and_then(|t| num(t, "Z")).unwrap_or(0.0));
    let sn = format!("socket:{}", text(&notify, "Socket").unwrap_or(""));
    let ch = w.fighter(f).ch;
    let si = w.names.get(&sn).and_then(|n| w.fixed.samplers.get(&ch).and_then(|b| b.by_name.get(&n).copied()));
    let (mut x, mut y) = (w.fighter(f).body.x, actor_y(w, f) - 100.0);
    if let Some(si) = si
        && let Some(pose) = sampled_boxes(w, f)
    {
        let o = w.fixed.samplers[&ch].offsets[usize::try_from(si).expect("a shape index")];
        let o = usize::try_from(o).expect("a box offset");
        x = mesh_wx(w, f) + crate::real(pose[o]) * fc;
        y = mesh_wy(w, f) + crate::real(pose[o + 1]);
        put_pose(w, pose);
    }
    x += ty * fc;
    y += tz;
    // The parent's BeginPlay loop (ForceSpawnPositionInsideBlastBox: the clamp, never the destroy), then
    // ReceiveBeginPlay (ubergraph 5002, authority, not attached, @5096 / @5188): the location clamped to the bounds
    // that loop stored (BlastBoxBoundsMin + BlastBoxBuffer, BlastBoxBoundsMax - BlastBoxBuffer).
    let mut s = new_blast_bounds();
    let (force, doe) = (flag(p, "ForceSpawnPositionInsideBlastBox"), flag(p, "DestroyOnExitedArena?"));
    blast_box_loop(&spawn_box(w), &mut x, &mut y, None, force, &mut s, doe);
    let b = obj(p, "BlastBoxBuffer");
    let (by, bz) = (b.and_then(|b| num(b, "Y")).unwrap_or(0.0), b.and_then(|b| num(b, "Z")).unwrap_or(0.0));
    x = min(max(x, s.left + by), s.right - by);
    y = min(max(y, s.bottom + bz), s.top - bz);
    let pos = obj(comps, "Spline").and_then(|c| obj(c, "SplineCurves")).and_then(|c| obj(c, "Position"));
    let pts = pos
        .map_or(&[][..], |c| arr(c, "Points"))
        .iter()
        .map(|q| {
            let q = q.as_obj();
            let at = |k: &str, a: &str| q.and_then(|q| obj(q, k)).and_then(|v| num(v, a)).unwrap_or(0.0);
            SplinePt {
                x: x + at("OutVal", "X") * fc,
                y: y + at("OutVal", "Z"),
                tx: at("LeaveTangent", "X") * fc,
                ty: at("LeaveTangent", "Z"),
                auto: None,
            }
        })
        .collect();
    let life = num(p, "MaxLifetime").unwrap_or(0.0);
    let mut k = 0.0;
    if life > 0.0 {
        while !(crate::consts::clock(k) > f64::from(life)) {
            k += 1.0;
        }
    }
    Marker {
        lifetime_pause: None,
        dodge: None,
        cls: r.cls,
        elem: None,
        seq: -1,
        x,
        y,
        facing: fc,
        age: 0,
        dead: false,
        blast_bounds: s,
        blast_at: f64::from(0.1_f32),
        hit,
        charge: None,
        pts,
        current_damage: 0.0,
        cap_radius,
        active: false,
        moving: false,
        focus_x: x,
        focus_y: y,
        move_start: Pt2::default(),
        move_end: Pt2::default(),
        can_add: true,
        drop_x: x,
        drop_y: y,
        spline_at: f64::from(0.25_f32),
        auto_tick: to_int32(latent_frames(3.5)),
        life_ticks: if life > 0.0 { to_int32(k + 1.0) } else { 0 },
        tl_pos: 0.0,
        tl_rate: 0.0,
        len: 0.0,
        table: Vec::new(),
        saved: Vec::new(),
        meshes: 0,
        trail_end: Pt2::default(),
        owner: f,
        attack_id: r.attack_id,
    }
}

/// `Marker.step`: one actor tick. MulticastDetach (@12302-12401: DestroyOnExitedArena? false, then
/// CheckForBlastBox) has no caller in the build (the attach handlers left from Garnet's marker).
pub fn step(w: &mut World, m: MarkerId) -> Option<MarkerEnd> {
    let (fx, fy) = owner_at(w, mk(w, m).owner);
    let doe = flag(&class_p(w, mk(w, m)), "DestroyOnExitedArena?");
    let x = mk_mut(w, m);
    x.age += 1;
    // ReceiveTick -> UpdateSpline(false); ActivateObject turns the actor's tick off.
    if !x.active {
        update_spline(x, fx, fy);
    }
    if x.age == x.auto_tick && !x.active {
        return Some(MarkerEnd::Auto);
    }
    if x.moving && timeline_tick(w, m) {
        return Some(MarkerEnd::Done);
    }
    // The timers: CheckBlastBoxTimer (the parent's BeginPlay sets it first), AddSplineTimer (0.25 s, looping) ->
    // AddSplineCheck, then MaxLifetime.
    let x = mk_mut(w, m);
    let frames = crate::real(x.age - 1);
    if blast_timer_exit(doe, &mut x.blast_at, &x.blast_bounds, frames, x.x, x.y) {
        return Some(MarkerEnd::Blast);
    }
    if !x.active && crate::consts::clock(frames) > x.spline_at {
        x.spline_at += f64::from(0.25_f32);
        add_spline_check(w, m);
    }
    let x = mk(w, m);
    if x.life_ticks > 0 && x.age >= x.life_ticks {
        return Some(MarkerEnd::Life);
    }
    None
}

/// `Marker.timelineTick`: one update of the MoveAlongPath timeline; true when it finished. `step` runs it for a
/// marker that is moving. The Delay(3.5) activation happens inside the actor's own tick (its latent actions: 1947
/// -> ActivateMontage -> ActivateObject -> PlayFromStart at 2351, with no latent node between), after `step`
/// returned `Auto`: the world calls this once more in that frame.
// Unverified: that the timeline component, whose tick PlayFromStart turns on in the middle of the frame, is still
// ticked in that frame is the stock rule (FTickTaskLevel::AddTickFunction puts a function enabled during a tick
// group on the newly spawned list; measured for a manager enabled mid-frame), not read for a timeline in this build.
pub fn timeline_tick(w: &mut World, m: MarkerId) -> bool {
    // MoveAlongPath: length 1, PathAlpha linear 0 -> 1, play rate 1 / MoveOverTime. Each update sets the owner at
    // the spline's location at distance Lerp(length, 0, PathAlpha) with a sweep (its capsule has no collision
    // meanwhile).
    let x = mk_mut(w, m);
    x.tl_pos += DELTA_SECONDS * x.tl_rate;
    let a = min(x.tl_pos, 1.0);
    let d = x.len + (0.0 - x.len) * a;
    let q = location_at_distance(&x.pts, &x.table, d);
    let owner = x.owner;
    recall_move(w, owner, q.x, q.y);
    let x = mk_mut(w, m);
    // The update's mesh part (16702-17845): with the last mesh's point at or past the distance that mesh is
    // destroyed (one an update), then the last one left ends at the spline's location at the distance.
    if x.meshes > 0 && x.table[table_at((x.meshes - 1) * REPARAM_STEPS)].d >= d {
        x.meshes -= 1;
    }
    x.trail_end = Pt2 { x: q.x, y: q.y };
    x.focus_x = x.move_start.x + a * (x.move_end.x - x.move_start.x);
    x.focus_y = x.move_start.y + a * (x.move_end.y - x.move_start.y);
    if x.tl_pos >= 1.0 {
        x.moving = false;
        return true;
    }
    false
}

fn table_at(i: i32) -> usize {
    usize::try_from(i).expect("a reparam table index")
}

/// `addSplineCheck` (ubergraph 3529): with CanAddNewSpline and the owner 250 or more from DropNewPointCheck, that
/// sphere moves to the owner and AddNewSpline(true) -> AddNewSplineMulticast: Spline.AddSplinePoint(the owner's
/// location) and UpdateSpline.
fn add_spline_check(w: &mut World, m: MarkerId) {
    let (fx, fy) = owner_at(w, mk(w, m).owner);
    let x = mk_mut(w, m);
    if !x.can_add || !(size2d([fx - x.drop_x, fy - x.drop_y]) >= 250.0) {
        return;
    }
    x.drop_x = fx;
    x.drop_y = fy;
    x.pts.push(SplinePt { x: fx, y: fy, tx: 0.0, ty: 0.0, auto: None });
    update_spline(x, fx, fy);
}

/// `updateSpline` (ubergraph 13866), N points: the last point is set to the owner's location (`fx`, `fy`). The calls
/// with index N are out of range (GetLocationAtSplinePoint clamps to N - 1, SetTangentAtSplinePoint does nothing),
/// which leaves: tangent(N - 1) = unit(P[N-2] -> P[N-1]) x |P[N-2] P[N-1]| / 2 and
/// tangent(N - 2) = unit(P[N-3] -> P[N-1]) x (|P[N-1] P[N-2]| + |P[N-3] P[N-2]|) / 2, index N - 3 clamped to 0.
/// The spline meshes it also lays out are the view's.
pub fn update_spline(x: &mut Marker, fx: Real, fy: Real) {
    let n = x.pts.len();
    if n < 2 {
        return;
    }
    let p = &mut x.pts;
    p[n - 1].x = fx;
    p[n - 1].y = fy;
    let (last, a) = (p[n - 1], p[n - 2]);
    let d2 = size2d([last.x - a.x, last.y - a.y]);
    let u1 = unit(last.x - a.x, last.y - a.y);
    p[n - 1].tx = u1.0 * (d2 / 2.0);
    p[n - 1].ty = u1.1 * (d2 / 2.0);
    let c = p[n.saturating_sub(3)];
    let u2 = unit(last.x - c.x, last.y - c.y);
    let s = (size2d([last.x - a.x, last.y - a.y]) + size2d([c.x - a.x, c.y - a.y])) / 2.0;
    p[n - 2].tx = u2.0 * s;
    p[n - 2].ty = u2.1 * s;
}

/// `Marker.activate`: ActivateObject (ubergraph 8719) up to StartMoveAlongPath: AddNewSpline(false) (the last point
/// and the tangents once more), the tick and the timer off, the timeline's rate
/// FMax(1 / (ShouldMoveOverTime ? MoveOverTime : length / MoveSpeed), 0.01), SetTestLocations and PlayFromStart.
pub fn activate(w: &mut World, m: MarkerId) {
    let (fx, fy) = owner_at(w, mk(w, m).owner);
    let p = class_p(w, mk(w, m));
    let x = mk_mut(w, m);
    x.active = true;
    update_spline(x, fx, fy);
    x.can_add = false;
    x.len = build_reparam_table(&x.pts, &mut x.table);
    let t = if flag(&p, "ShouldMoveOverTime") {
        num(&p, "MoveOverTime").unwrap_or(0.0)
    } else {
        x.len / num(&p, "MoveSpeed").unwrap_or(1.0)
    };
    x.tl_rate = max(if t != 0.0 { 1.0 / t } else { 0.0 }, 0.01);
    // SetTestLocations: the spline's locations at Lerp(length, 0, 1 / 0.75 / 0.5 / 0.25 / 0), in that order.
    let len = x.len;
    let at = |x: &Marker, d: Real| {
        let q = location_at_distance(&x.pts, &x.table, d);
        Pt2 { x: q.x, y: q.y }
    };
    x.saved = [1.0, 0.75, 0.5, 0.25, 0.0].iter().map(|&k| at(x, len + (0.0 - len) * k)).collect();
    x.move_start = at(x, len);
    x.move_end = at(x, 0.0);
    x.meshes = i32::try_from(x.pts.len()).expect("a point count") - 1;
    x.trail_end = x.move_start;
    x.tl_pos = 0.0;
    x.moving = true;
}

/// `Marker.safeLocation`: CheckTestLocations (0-1581): per saved location CapsuleOverlapActors (the owner's original
/// radius and half height; object types [0, 8]; the marker and the owner ignored). Nothing found: that location
/// (1363). Else the actors are walked until one is not a BP_FighterCharacter, which sets LocalLocationIsBlocked
/// (1172); the location is taken when the flag is clear (1461), and the flag is never cleared, so once a location
/// held something else a later one holding only fighters is passed over too. (0, 0, 0) when none is taken.
/// The object types by DefaultEngine.ini's channel order (the object channels in order after the six stock ones):
/// 0 WorldStatic, 8 DamageDestructible (ECC_GameTraceChannel4); an object query looks at the body's type only, not
/// at its responses. What the port holds of those types with its query collision on:
/// - the stage block (profile Floor: no ObjectTypeName, serialized ECC_WorldStatic);
/// - a fighter's shape on the collision profile KnockBack (`knock_back_shapes`: the FullBody sphere of Pawn_Shaggy /
///   Pawn_C019's SCS, on the bone fml_un_C_spine_c; Pawn_C017's WallBounce, its FullBody being a HitBox body; no
///   ObjectTypeName either, serialized ECC_WorldStatic) while a Hitframe whose tags reach it is on (BaseFighter
///   124803: HitBoxesOff([FullBody]) at BeginPlay; HitBoxesOn sets QueryAndPhysics, Comp_Pawn_HitBoxHandler 16817 /
///   17186), the hit-react clips' sequence Hitframes included: the one body that makes the fighters-only case;
/// - a C019_MortarAim's Hurtbox (profile DamageDestructible, never switched): a capsule on the Aim component.
///
/// Not found: the stage's platforms and a ship's pieces (Platform, index 10), so a recall can end inside one; a
/// fighter's other bodies; the marker's C019MarkerCameraFocus (NoCollision); the blast box's boxes; grenades, shots
/// and the whip, the sandwich item; and the maps' BlockPortalCollision_C Box (object type Portal, index 15).
/// `gs`: the stage block and the stage's WorldStatic solids (`static_grounds(QUERY_WORLDSTATIC)`, with the polygon
/// solids: met by the capsule's segment within r of the polygon); `fighters`: the
/// match's; the mortars are the World's.
// Unverified: a profile without an ObjectTypeName is taken at the serialized type (the loader's path for it is not
// read in the exe); the sphere is tested in the play plane at its bone's depth against a capsule at depth 0.
pub fn safe_location(w: &World, m: MarkerId, gs: crate::world::stage_query::Shapes, fighters: &[FighterId]) -> Pt2 {
    let x = mk(w, m);
    let owner = x.owner;
    // BPI_GetOriginalCapsuleHalfHeightAndRadius (CheckTestLocations @150): the size BaseFighter saved at BeginPlay
    // (@124896 / @124945), not the root capsule's current one: the template stays here.
    let r = ch_cap_radius(w, owner);
    let h = ch_cap_half(w, owner) - r;
    let mut blocked = false; // LocalLocationIsBlocked
    for q in &x.saved {
        let mut other = false;
        for g in gs.boxes {
            let qx = max(crate::math::narrow(g.left), min(q.x, crate::math::narrow(g.right))) - q.x;
            let py =
                max(q.y - h, min(max(crate::math::narrow(g.bottom), min(q.y, crate::math::narrow(g.top))), q.y + h));
            let qy = max(crate::math::narrow(g.bottom), min(py, crate::math::narrow(g.top))) - py;
            if qx * qx + qy * qy < r * r {
                other = true;
            }
        }
        if gs.polys.iter().any(|p| p.seg_dist2(q.x, q.y - h, q.x, q.y + h) < r * r) {
            other = true;
        }
        for (_, mo) in w.actors.mortars.iter() {
            if other {
                break;
            }
            if mo.dead {
                continue;
            }
            other = crate::chars::morty::mortar::hurt_meets_capsule(w, mo, q.x, q.y, r, h);
        }
        let mut fighter = false;
        for &f in fighters {
            if other || fighter {
                break;
            }
            if f == owner || w.fighter(f).montages.active_hits.is_empty() {
                continue;
            }
            fighter = full_body_meets(w, f, q.x, q.y, r, h);
        }
        if other {
            blocked = true;
        } else if !fighter || !blocked {
            return *q;
        }
    }
    Pt2 { x: 0.0, y: 0.0 }
}

/// The first active Hitframe of `f` whose tags reach a shape on the KnockBack profile (GetHitBoxes: with Use 2D
/// Hitboxes a tag other than FullBody finds tag + "2D"): that shape's sphere (its radius x HitBoxScale.X x the bone's
/// scale, at the bone's depth) meets the capsule at (`qx`, `qy`) of radius `r` and segment half `h`.
fn full_body_meets(w: &World, f: FighterId, qx: Real, qy: Real, r: Real, h: Real) -> bool {
    let fi = w.fighter(f);
    let d = char_data(w, fi.ch);
    let kb = knock_back_shapes(d);
    let b = &w.fixed.samplers[&fi.ch];
    for hit in fi.montages.active_hits.values() {
        let tags = crate::fighter::anim::montages::hit_tags_of(w, f, hit);
        let shape = tags.iter().find_map(|t| {
            let list = if d.def.use_2d && t != "FullBody" { b.tagged_suffixed(t, "2D") } else { b.tagged(t) };
            list.iter().copied().find(|&i| kb.iter().any(|k| *k == d.boxes.shapes[i as usize].name))
        });
        let Some(i) = shape else { continue };
        let Some(pose) = sampled_boxes(w, f) else { return false };
        let iu = usize::try_from(i).expect("a shape index");
        let o = usize::try_from(b.offsets[iu]).expect("a box offset");
        let p = crate::combat::hit::hit_packet(w, f, hit);
        let sx = p.and_then(|p| obj(p, "HitBoxScale")).and_then(|s| num(s, "X")).unwrap_or(1.0);
        let fr = d.boxes.shapes[iu].radius * f64::from(sx) * f64::from(crate::real(pose[o + 3]));
        let dx = mesh_wx(w, f) + crate::real(pose[o]) * facing(w, f) - qx;
        let sy = mesh_wy(w, f) + crate::real(pose[o + 1]);
        let dy = sy - max(qy - h, min(sy, qy + h));
        let dz = crate::real(pose[o + 2]);
        put_pose(w, pose);
        return dx * dx + dy * dy + dz * dz < (r + crate::math::narrow(fr)) * (r + crate::math::narrow(fr));
    }
    false
}

/// `Marker.trail`: the rewind trail's spline meshes (the view's; sim space, tangents as the points'). AddSplineMesh
/// adds one in BeginPlay and one with every added point, so there is one per segment; UpdateSpline (ubergraph
/// 12419) lays mesh i from point i to point i + 1 (SetStartAndEnd with their locations and tangents; with four or
/// more meshes only those from index 2 on, the earlier points no longer moving). The MoveAlongPath update (16702)
/// destroys the last mesh when its point's distance along the spline >= Lerp(length, 0, PathAlpha), then ends the
/// last one at the spline's location at that distance, with the next point's tangent. A mesh the update does not
/// reach keeps its layout. Written over `out`.
pub fn trail(x: &Marker, out: &mut Vec<Seg>) {
    let p = &x.pts;
    let pn = i32::try_from(p.len()).expect("a point count");
    let n = usize::try_from(if x.active { x.meshes } else { pn - 1 }.max(0)).expect("a mesh count");
    out.clear();
    for i in 0..n {
        let (a, b) = (p[i], p[i + 1]);
        let e = if x.active && i == n - 1 { x.trail_end } else { Pt2 { x: b.x, y: b.y } };
        out.push(Seg { ax: a.x, ay: a.y, atx: a.tx, aty: a.ty, bx: e.x, by: e.y, btx: b.tx, bty: b.ty });
    }
}

/// GetDirectionUnitVector: (To - From).GetSafeNormal(), zero under a squared size of 1e-8.
fn unit(dx: Real, dy: Real) -> (Real, Real) {
    let n = safe_normal([0.0, dx, dy], SMALL_NUMBER);
    (n[1], n[2])
}

#[cfg(test)]
mod tests {
    use super::*;

    fn marker(pts: Vec<SplinePt>) -> Marker {
        Marker {
            lifetime_pause: None,
            dodge: None,
            cls: crate::names::Name::NONE,
            elem: None,
            seq: 0,
            x: 0.0,
            y: 0.0,
            facing: 1.0,
            age: 0,
            dead: false,
            blast_bounds: new_blast_bounds(),
            blast_at: 0.0,
            hit: HitPacket::default(),
            charge: None,
            pts,
            current_damage: 0.0,
            cap_radius: 0.0,
            active: false,
            moving: false,
            focus_x: 0.0,
            focus_y: 0.0,
            move_start: Pt2::default(),
            move_end: Pt2::default(),
            can_add: true,
            drop_x: 0.0,
            drop_y: 0.0,
            spline_at: 0.0,
            auto_tick: 0,
            life_ticks: 0,
            tl_pos: 0.0,
            tl_rate: 0.0,
            len: 0.0,
            table: Vec::new(),
            saved: Vec::new(),
            meshes: 0,
            trail_end: Pt2::default(),
            owner: FighterId::new(0, 0),
            attack_id: 0,
        }
    }

    fn pt(x: Real, y: Real) -> SplinePt {
        SplinePt { x, y, tx: 0.0, ty: 0.0, auto: None }
    }

    #[test]
    fn update_spline_sets_the_last_two_tangents() {
        let mut m = marker(vec![pt(0.0, 0.0), pt(100.0, 0.0), pt(0.0, 0.0)]);
        update_spline(&mut m, 100.0, 100.0);
        // last: unit((100, 0) -> (100, 100)) x 100 / 2
        assert_eq!((m.pts[2].x, m.pts[2].y, m.pts[2].tx, m.pts[2].ty), (100.0, 100.0, 0.0, 50.0));
        // middle: unit((0, 0) -> (100, 100)) x (100 + 100) / 2
        let k = 100.0 * crate::math::ue::inv_sqrt(20000.0) * 100.0;
        assert_eq!((m.pts[1].tx, m.pts[1].ty), (k, k));
        assert_eq!((m.pts[0].tx, m.pts[0].ty), (0.0, 0.0));
    }

    #[test]
    fn update_spline_with_one_point_does_nothing() {
        let mut m = marker(vec![pt(5.0, 5.0)]);
        update_spline(&mut m, 100.0, 100.0);
        assert_eq!((m.pts[0].x, m.pts[0].y), (5.0, 5.0));
    }

    #[test]
    fn unit_is_zero_under_the_threshold() {
        assert_eq!(unit(1e-5, 0.0), (0.0, 0.0));
        let k = crate::math::ue::inv_sqrt(25.0); // InvSqrt: 0.59999996, 0.79999995
        assert_eq!(unit(3.0, 4.0), (3.0 * k, 4.0 * k));
    }

    #[test]
    fn trail_lays_one_mesh_per_segment_and_ends_the_last_at_the_trail_end() {
        let mut m = marker(vec![pt(0.0, 0.0), pt(10.0, 0.0), pt(20.0, 5.0)]);
        let mut out = Vec::new();
        trail(&m, &mut out);
        assert_eq!(out.len(), 2);
        assert_eq!((out[1].ax, out[1].bx, out[1].by), (10.0, 20.0, 5.0));
        m.active = true;
        m.meshes = 1;
        m.trail_end = Pt2 { x: 4.0, y: 1.0 };
        trail(&m, &mut out);
        assert_eq!(out.len(), 1);
        assert_eq!((out[0].ax, out[0].bx, out[0].by), (0.0, 4.0, 1.0));
        m.meshes = 0;
        trail(&m, &mut out);
        assert!(out.is_empty());
    }
}
