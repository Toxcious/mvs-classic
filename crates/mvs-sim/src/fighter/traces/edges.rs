//! Comp_RunAlongEdgeTrace, an actor that rides the edges of the solids and platforms
//!: the edge polygons (the level's, the spawned owners'), the rider
//! ([`EdgeRider`]) and its attach / detach, the removal of a polygon. Stripe's saws (`chars::stripe::saw`) are its
//! riders; the tunnel carry (`actors::tunnel_carry`, Bugs Bunny's port) detaches a rider and attaches it again.
//!
//! The math of the polygons' queries is `mvs_collision::edges`; here are the polygons as the world gives them at a
//! call ([`build`]: the spawned ones are rebuilt at each call), the rider's state machine and
//! its latent actions.
//!
//! The classes that hold a rider (`edgeRiders`, `edgeRiderWalk`) come to the one
//! holder there is: a saw is a `ProjectileExt::Saw`, its rider a field of it.
//!
//! Unverified: assumed, not read in the exe: the elements' order, the crossings as the
//! registrations leave them, Line Intersection and the rotator math as stock 4.26, the order of two Delays done in one
//! pass, GetServerWorldTimeSeconds as the owner fighter's clock.
// Deviation: the polygon of a level owner whose `edges` flag was cleared (Comp_EdgeTrace.On Destroyed called without
// the actor's end) is not modelled: the Rust stage actors have no such flag, so only a dead owner is gone.

#![allow(
    clippy::cast_possible_truncation,
    clippy::cast_possible_wrap,
    clippy::cast_sign_loss,
    clippy::neg_cmp_op_on_partial_ord,
    clippy::too_many_arguments,
    reason = "polygon and tag ids are small non-negative integers; negated comparisons keep their NaN behaviour; the calls' parameters"
)]

use mvs_collision::edges::{
    self as ce, EdgePoly, EdgeSnap, Vec3, add, axis_angle, closest_point, compose, dist, eq, get_next_point, normal,
    qrot, rlerp, rotate_angle_axis, signed_angle, sub,
};
use mvs_collision::rotator::{normalize_axis, quat_of};

/// The edge math is mvs-collision's, in double, kept so the trace records keep their values (the World's float32
/// values widen in and narrow out at its calls).
// Deviation: the build's edge rider is Blueprint math in float (no PhysX sweep under it); this module stays double
// while mvs-collision's `edges` / `rotator` (Vec3 = [f64; 3]) do.
type Real = f64;
use crate::State;
use crate::actors::world_actor::actor_place;
use crate::actors::{ActorRef, ProjectileExt, ProjectileId, WorldActorState};
use crate::consts::DELTA_SECONDS;
use crate::fighter::common::warn_text;
use crate::world::World;

/// The Delays' resume offsets (`N_MOVE`, `N_NONE`, `N_TURN`, `N_ATTACH`, `N_START`).
const N_MOVE: i32 = 3070;
const N_NONE: i32 = 3157;
const N_TURN: i32 = 4402;
const N_ATTACH: i32 = 5883;
const N_START: i32 = 11537;

/// The owner classes whose graphs set their Comp_EdgeTrace's Update Intersections Every Frame while they move
/// (M008_Elevator_BP @49 / @15; ScoobyElevatorSwitch @1921 / @516). Unverified: these two are assumed the only movers.
const MOVERS: [&str; 2] = ["M008_Elevator_BP_C", "ScoobyElevator_C"];

/// One pending Delay node of a rider (`pend`): the node, the seconds left, the tick it was made in and whether it was
/// made while the rider's own actions ran.
#[derive(State, Debug, Default)]
pub struct EdgePend {
    pub node: i32,
    pub left: Real,
    pub made: i32,
    pub own: bool,
}

/// `EdgeRider`: a Comp_RunAlongEdgeTrace's variables (the ubergraph's persistent ones among them): the edge it is
/// attached to (-1 none), its direction, speed and place on the edge. `moveSpeed`, `rotateSpeed`, `attachOnStart`
/// and `rotateAlong` are the component's defaults over the class's (Base Move Speed 1000, Base Rotate Speed 0.08,
/// Clockwise true, Attach On Start false, Rotate Along Edges false); Tag is None for every rider.
#[derive(State, Debug)]
pub struct EdgeRider {
    pub move_speed: Real,
    pub rotate_speed: Real,
    pub attach_on_start: bool,
    pub rotate_along: bool,
    /// Generator valid (BeginPlay's GetActorOfClass).
    pub generated: bool,
    pub clockwise: bool,
    pub active: bool,
    /// Initial Velocity, Local Offset Clockwise Degrees, Local Offset Position, Pivot Point (UE: X depth, Y, Z).
    pub iv: [Real; 3],
    pub locd: Real,
    pub lop: [Real; 3],
    pub pivot: [Real; 3],
    /// Polygon ids, -1 none.
    pub attached: i32,
    pub last_attached: i32,
    /// The polygons whose On Edges Removed it is bound to, in bind order.
    pub bound: Vec<i32>,
    /// CurrentTravelDirection.
    pub ctd: [Real; 3],
    /// Attach To Edge's Get Closest Point on Polygon outputs, Get Next Point's Destination, Get Signed Angle Between's
    /// angle, the start time (Temp_float_Start_Time_Variable_3) and the attach turn's rotators.
    pub cp_poly: i32,
    pub cp_near: [Real; 3],
    pub cp_last: i32,
    pub cp_next: i32,
    pub a_dest: [Real; 3],
    pub ang: Real,
    pub st3: Real,
    pub init2: [Real; 3],
    pub final2: [Real; 3],
    /// Move Along Edge's: the event's Next Index and Start Point, Get Next Point's outputs, Calculate Turn Amt's
    /// angle, the corner turn's start time and rotators, the move's start time, the ubergraph's own Start Point.
    pub ev_next: i32,
    pub ev_start: [Real; 3],
    pub d1: [Real; 3],
    pub ao1: i32,
    pub io1: i32,
    pub i1: bool,
    pub ccw: Real,
    pub st2: Real,
    pub init1: [Real; 3],
    pub final1: [Real; 3],
    pub st: Real,
    pub sp: [Real; 3],
    /// The pending Delay nodes, in the order made.
    pub pend: Vec<EdgePend>,
    /// The rider's own actions run now (`passing === r`): a Delay made then counts from the next frame.
    #[state(skip)]
    pub passing: bool,
}

impl Default for EdgeRider {
    /// `newEdgeRider()`.
    fn default() -> Self {
        EdgeRider {
            move_speed: 1000.0,
            rotate_speed: 0.08,
            attach_on_start: false,
            rotate_along: false,
            generated: false,
            clockwise: true,
            active: false,
            iv: [0.0; 3],
            locd: 0.0,
            lop: [0.0; 3],
            pivot: [0.0; 3],
            attached: -1,
            last_attached: -1,
            bound: Vec::new(),
            ctd: [0.0; 3],
            cp_poly: -1,
            cp_near: [0.0; 3],
            cp_last: 0,
            cp_next: 0,
            a_dest: [0.0; 3],
            ang: 0.0,
            st3: 0.0,
            init2: [0.0; 3],
            final2: [0.0; 3],
            ev_next: 0,
            ev_start: [0.0; 3],
            d1: [0.0; 3],
            ao1: -1,
            io1: 0,
            i1: false,
            ccw: 0.0,
            st2: 0.0,
            init1: [0.0; 3],
            final1: [0.0; 3],
            st: 0.0,
            sp: [0.0; 3],
            pend: Vec::new(),
            passing: false,
        }
    }
}

// ------------------------------------------------------------------ the polygons

/// What a polygon's owner is, for `edgePolyOf`'s lookups: the level's keep the data's marks.
#[derive(Clone, Copy, PartialEq, Debug)]
enum Owner {
    Level { floor: bool, platform: i32, solid: i32, placed: i32 },
    Spawned,
}

/// One element of the set at a call.
struct Elem {
    poly: EdgePoly,
    owner: Owner,
}

/// The tag, interned for the query (every tag is None; strings are compared).
fn tag_id(t: &str) -> u32 {
    t.bytes().fold(0x811c_9dc5_u32, |h, b| (h ^ u32::from(b)).wrapping_mul(0x0100_0193))
}

/// The number of polygons the level registers (`edgeSet`'s `polys.length`): `None` where the level has no generator.
fn level_count(w: &World) -> Option<usize> {
    let edges = w.data.stage.edges.as_ref()?;
    Some(edges.iter().filter(|d| !d.points.is_empty()).count())
}

/// A spawned owner's box (`Box`).
struct Bx {
    left: Real,
    right: Real,
    top: Real,
    bottom: Real,
}

/// `spawnedPoly`: the polygon of a spawned owner at (x, y) now, registered at `at`.
fn spawned_poly(id: i32, x: Real, y: Real, at: (Real, Real), b: &Bx, dead: bool) -> Elem {
    let pts: [Vec3; 4] = [
        [0.0, (x - b.right), (b.top - y)],
        [0.0, (x - b.left), (b.top - y)],
        [0.0, (x - b.left), (b.bottom - y)],
        [0.0, (x - b.right), (b.bottom - y)],
    ];
    let (y0, z0) = ((-at.0), at.1);
    Elem {
        poly: EdgePoly {
            id,
            tag: tag_id("None"),
            gone: dead,
            moves: false,
            flat: [0.0, (-x), y],
            flat0: [0.0, y0, z0],
            lp: ce::polar_sort(&pts, &ce::low_left(&pts)),
        },
        owner: Owner::Spawned,
    }
}

/// `edgeSet` and `elements`: the set at this call, the level's polygons then the spawned owners' in id order; `None`
/// where the level has no generator. A spawned polygon's id is the level's polygon count + the actor's id.
fn build(w: &World) -> Option<Vec<Elem>> {
    let data = w.data.stage.edges.as_ref()?;
    let placed = w.data.stage.placed.as_deref().unwrap_or(&[]);
    let run = w.stage_run.as_ref();
    let mut out: Vec<Elem> = Vec::new();
    for d in data.iter().filter(|d| !d.points.is_empty()) {
        let id = out.len() as i32;
        let pi = placed.iter().position(|p| p.name == d.owner).map_or(-1, |i| i as i32);
        let pts: Vec<Vec3> = d.points.iter().map(|p| [0.0, p[0], p[1]]).collect();
        let (y0, z0) = (d.loc[1], d.loc[2]);
        let (platform, solid) = (d.platform.unwrap_or(-1), d.solid.unwrap_or(-1));
        // gone: the owner's actor dead or its `edges` flag off (`isGone`: a Cromulons speaker's,
        // a Batcave car's, a Court wall's)
        let actor_at = if pi < 0 { None } else { run.and_then(|r| r.actors.iter().position(|a| a.index == pi as u32)) };
        let gone = actor_at.is_some_and(|k| {
            run.is_some_and(|r| {
                use crate::world::stage_actors::StageActorOwn as O;
                r.actors[k].dead
                    || match &r.actors[k].own {
                        O::Speaker(s) => !s.edges,
                        O::BatcaveCar(c) => !c.edges,
                        O::M010Wall(m) => !m.edges,
                        _ => false,
                    }
            })
        });
        // Get Flattened Location: the owner's place less / plus its move
        let (mut dx, mut dy) = (0.0, 0.0);
        if solid >= 0 {
            if let Some(st) = run.and_then(|r| r.solids.get(solid as usize)) {
                (dx, dy) = (st.m.dx, st.m.dy);
            }
        } else if platform >= 0
            && let Some(p) = w.stage.stage_platforms.get(platform as usize)
        {
            (dx, dy) = (p.dx, p.dy);
        }
        out.push(Elem {
            poly: EdgePoly {
                id,
                tag: tag_id(&d.tag),
                gone,
                moves: MOVERS.contains(&d.cls.as_str()),
                flat: [0.0, y0 - f64::from(dx), z0 + f64::from(dy)],
                flat0: [0.0, y0, z0],
                lp: ce::polar_sort(&pts, &ce::low_left(&pts)),
            },
            owner: Owner::Level { floor: d.floor.unwrap_or(false), platform, solid, placed: pi },
        });
    }
    let n = out.len() as i32;
    // the SpawnedPlatformParent children: their Collision part, or Marvin's UFO's `edgeBox` before it (reads
    // `a.edgeBox ?? pl.collision`: chars::marvin::ufo's)
    for (id, a) in w.actors.actors.iter() {
        let Some(pl) = &a.plat else { continue };
        let Some(at) = pl.edge_at else { continue };
        let own = match &a.state {
            WorldActorState::Ufo(u) => crate::chars::marvin::ufo::edge_box(w, a, u),
            _ => None,
        };
        let Some(part) = own.or_else(|| pl.collision.and_then(|i| pl.parts.get(usize::from(i)))) else { continue };
        let Some((x, y, _)) = actor_place(w, ActorRef::Actor(id)) else { continue };
        let b = Bx {
            left: f64::from(part.left),
            right: f64::from(part.right),
            top: f64::from(part.top),
            bottom: f64::from(part.bottom),
        };
        out.push(spawned_poly(
            n + a.seq,
            f64::from(x),
            f64::from(y),
            {
                let (a, b) = at;
                (f64::from(a), f64::from(b))
            },
            &b,
            a.dead,
        ));
    }
    // the ships: their Collision piece
    for (_, sh) in w.actors.ships.iter() {
        let Some(at) = sh.edge_at else { continue };
        let Some(pc) = sh.pieces.iter().find(|q| w.names.str(q.name) == "Collision") else { continue };
        let b = Bx {
            left: f64::from(sh.x + pc.dx - pc.hw),
            right: f64::from(sh.x + pc.dx + pc.hw),
            top: f64::from(sh.y + pc.dy + pc.hh),
            bottom: f64::from(sh.y + pc.dy - pc.hh),
        };
        out.push(spawned_poly(
            n + sh.seq,
            f64::from(sh.x),
            f64::from(sh.y),
            {
                let (a, b) = at;
                (f64::from(a), f64::from(b))
            },
            &b,
            sh.dead,
        ));
    }
    if !w.actors.ships.is_empty() && !w.actors.actors.is_empty() {
        out.sort_by_key(|e| e.poly.id);
    }
    Some(out)
}

/// The set as the queries of `mvs_collision::edges` read it.
fn snap(w: &World) -> EdgeSnap {
    build(w).map(|v| v.into_iter().map(|e| e.poly).collect())
}

/// What a polygon of the level is named by (`edgePolyOf`'s `ref`).
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum EdgeRef {
    Floor,
    Platform(i32),
    Solid(i32),
    Placed(i32),
    /// A spawned owner's actor id (`seq`).
    Actor(i32),
}

/// `edgePolyOf`: a polygon's id (-1 none): the stage block's (floor), a pass-through platform's (its
/// StageData.platforms index), a solid's (its StageData.solids id), a placed actor's (its StageData.placed index) or a
/// spawned owner's (its actor id); -1 also where the level has no generator.
pub fn edge_poly_of(w: &World, r: EdgeRef) -> i32 {
    let Some(set) = build(w) else { return -1 };
    let Some(n) = level_count(w) else { return -1 };
    if let EdgeRef::Actor(a) = r {
        return set.iter().find(|e| e.poly.id == n as i32 + a).map_or(-1, |e| e.poly.id);
    }
    set.iter()
        .filter(|e| matches!(e.owner, Owner::Level { .. }))
        .find(|e| match (e.owner, r) {
            (Owner::Level { floor, .. }, EdgeRef::Floor) => floor,
            (Owner::Level { platform, .. }, EdgeRef::Platform(p)) => platform == p,
            (Owner::Level { solid, .. }, EdgeRef::Solid(s)) => solid == s,
            (Owner::Level { placed, .. }, EdgeRef::Placed(p)) => placed == p,
            _ => false,
        })
        .map_or(-1, |e| e.poly.id)
}

/// `edgePolyLive`: still in the generator.
pub fn edge_poly_live(w: &World, id: i32) -> bool {
    build(w).is_some_and(|set| set.iter().find(|e| e.poly.id == id).is_some_and(|e| !e.poly.gone))
}

/// `edgePolyFlat`: its owner's flattened location now (UE, X 0); `None` for no such polygon.
pub fn edge_poly_flat(w: &World, id: i32) -> Option<Vec3> {
    build(w)?.iter().find(|e| e.poly.id == id).map(|e| e.poly.flat)
}

/// `edgePolyPoints`: its Local Points (UE, relative to [`edge_poly_flat`]), sorted; empty for none.
pub fn edge_poly_points(w: &World, id: i32) -> Vec<Vec3> {
    build(w).and_then(|set| set.into_iter().find(|e| e.poly.id == id)).map(|e| e.poly.lp).unwrap_or_default()
}

// ------------------------------------------------------------------ the removal of a polygon

/// `edgesRemoved`: the stage's call where a placed owner's Comp_EdgeTrace.On Destroyed runs (its state already set):
/// On Edges Removed to the riders bound to its polygon.
pub fn edges_removed(w: &mut World, placed: u32) {
    let ids: Vec<i32> = match build(w) {
        Some(set) => set
            .iter()
            .filter_map(|e| match e.owner {
                Owner::Level { placed: p, .. } if p == placed as i32 => Some(e.poly.id),
                _ => None,
            })
            .collect(),
        None => return,
    };
    for id in ids {
        removed_for(w, id);
    }
}

/// `spawnedEdgesEnd`'s call (`solids::spawned_edges_end`): the polygon of the spawned owner with actor id `seq`.
pub fn spawned_removed(w: &mut World, seq: i32) {
    if let Some(n) = level_count(w) {
        removed_for(w, n as i32 + seq);
    }
}

/// `removedFor`: On Edges Removed to the riders bound to polygon `id` (On Attached Edges Removed @11815: On Run Along
/// Disabled, then Detach From Edge). Unverified: the riders' order (the delegate's invocation order) is the list's.
/// A repeated call broadcasts again, as the build's repeated On Destroyed.
fn removed_for(w: &mut World, id: i32) {
    let hits: Vec<ProjectileId> = crate::chars::stripe::saw::live_saws(w)
        .into_iter()
        .filter(|&s| rider(w, s).is_some_and(|r| r.bound.contains(&id)))
        .collect();
    for s in hits {
        crate::chars::stripe::saw::on_disabled(w, s);
        detach_from_edge(w, s);
    }
}

// ------------------------------------------------------------------ the rider's holder

/// The saw's rider.
fn rider(w: &World, a: ProjectileId) -> Option<&EdgeRider> {
    match &w.actors.projectiles.get(a)?.ext {
        ProjectileExt::Saw(s) => Some(&s.rider),
        _ => None,
    }
}

fn rider_mut(w: &mut World, a: ProjectileId) -> Option<&mut EdgeRider> {
    match &mut w.actors.projectiles.get_mut(a)?.ext {
        ProjectileExt::Saw(s) => Some(&mut s.rider),
        _ => None,
    }
}

/// Runs `f` on the saw's rider taken out of the saw (its host's calls reach the saw, never the rider).
fn with_rider<T>(w: &mut World, a: ProjectileId, f: impl FnOnce(&mut World, &mut EdgeRider) -> T) -> Option<T> {
    let mut r = std::mem::take(rider_mut(w, a)?);
    let out = f(w, &mut r);
    if let Some(s) = rider_mut(w, a) {
        *s = r;
    }
    Some(out)
}

/// `EdgeHost.now`: GetServerWorldTimeSeconds, the owner fighter's clock.
fn now(w: &World, a: ProjectileId) -> Real {
    let f = w.actors.projectiles.get(a).and_then(|p| p.owner).expect("a saw has its owner");
    f64::from(w.fighter(f).clock.time)
}

/// `EdgeHost.loc`: K2_GetActorLocation (UE).
fn host_loc(w: &World, a: ProjectileId) -> Vec3 {
    let p = w.actors.projectiles.get(a).expect("a saw");
    [0.0, f64::from(-p.x), f64::from(p.y)]
}

/// `EdgeHost.rot`: K2_GetActorRotation (read only with Rotate Along Edges, false for the saw).
fn host_rot(w: &World, a: ProjectileId) -> Vec3 {
    let p = w.actors.projectiles.get(a).expect("a saw");
    [0.0, if p.facing < 0.0 { 90.0 } else { -90.0 }, 0.0]
}

/// `EdgeHost.setLoc`: K2_SetActorLocation, no sweep, no teleport.
fn host_set_loc(w: &mut World, a: ProjectileId, y: Real, z: Real) {
    let p = w.actors.projectiles.get_mut(a).expect("a saw");
    p.x = crate::math::narrow(-y);
    p.y = crate::math::narrow(z);
}

// ------------------------------------------------------------------ the rider

/// `riderBeginPlay`: ReceiveBeginPlay @11378: Generator = GetActorOfClass(EdgeTraceGenerator_C); Attach On Start on
/// the authority: Delay 0.5.
pub fn rider_begin_play(w: &mut World, a: ProjectileId) {
    let generated = w.data.stage.edges.is_some();
    let tick = w.flow.tick;
    if let Some(r) = rider_mut(w, a) {
        r.generated = generated;
        if r.attach_on_start {
            delay(r, tick, N_START, 0.5);
        }
    }
}

/// `delay`: a Delay node pending is not made again.
fn delay(r: &mut EdgeRider, tick: i32, node: i32, secs: Real) {
    if r.pend.iter().any(|p| p.node == node) {
        return;
    }
    r.pend.push(EdgePend { node, left: secs, made: tick, own: r.passing });
}

/// `riderLatent`: the rider's Delays of this frame (once a frame, in the holder's leftover pass).
pub fn rider_latent(w: &mut World, a: ProjectileId) {
    if rider(w, a).is_none_or(|r| r.pend.is_empty()) {
        return;
    }
    let tick = w.flow.tick;
    with_rider(w, a, |w, r| {
        let prev = r.passing;
        r.passing = true;
        let nodes: Vec<i32> = r.pend.iter().map(|p| p.node).collect();
        for node in nodes {
            let Some(i) = r.pend.iter().position(|p| p.node == node) else { continue };
            if r.pend[i].own && r.pend[i].made == tick {
                continue;
            }
            r.pend[i].left -= f64::from(DELTA_SECONDS);
            if r.pend[i].left > 0.0 {
                continue;
            }
            r.pend.remove(i);
            resume(w, r, a, node);
        }
        r.passing = prev;
    });
}

fn resume(w: &mut World, r: &mut EdgeRider, a: ProjectileId, node: i32) {
    match node {
        N_MOVE => at909(w, r, a),
        N_NONE => {
            // @3212: Move Along Edge(the event's Next Index, the owner's location)
            let l = host_loc(w, a);
            move_along_edge(w, r, a, r.ev_next, l, 0);
        }
        N_TURN => at3315(w, r, a),
        N_ATTACH => at4457(w, r, a),
        N_START => {
            // @5938: Attach To Edge(Clockwise, the owner's location, 0, 0, 0)
            let l = host_loc(w, a);
            attach_to_edge_with(w, r, a, r.clockwise, l, [0.0; 3], [0.0; 3], 0.0);
        }
        _ => {}
    }
}

/// Set Attached Multicast @12257 and the binds around it.
fn set_attached(r: &mut EdgeRider, poly: i32) {
    if r.attached != r.last_attached {
        r.last_attached = r.attached;
    }
    r.attached = poly;
}

fn bind(r: &mut EdgeRider, poly: i32) {
    if poly >= 0 && !r.bound.contains(&poly) {
        r.bound.push(poly);
    }
}

fn unbind(r: &mut EdgeRider, poly: i32) {
    if let Some(i) = r.bound.iter().position(|&p| p == poly) {
        r.bound.remove(i);
    }
}

/// Attached.Get Flattened Location (Accessed None: 0).
fn attached_flat(w: &World, r: &EdgeRider) -> Vec3 {
    edge_poly_flat(w, r.attached).unwrap_or([0.0; 3])
}

/// `attachToEdge` on the saw's rider: Attach To Edge @9991 (Clockwise, Attach Point, Initial Velocity, Local Offset
/// Position, Local Offset Clockwise Degrees; vectors UE (X, Y, Z)).
pub fn attach_to_edge(
    w: &mut World,
    a: ProjectileId,
    clockwise: bool,
    attach_point: Vec3,
    initial_velocity: Vec3,
    local_offset: Vec3,
    offset_degrees: Real,
) {
    with_rider(w, a, |w, r| {
        attach_to_edge_with(w, r, a, clockwise, attach_point, initial_velocity, local_offset, offset_degrees)
    });
}

#[allow(clippy::too_many_arguments, reason = "the call's parameters")]
fn attach_to_edge_with(
    w: &mut World,
    r: &mut EdgeRider,
    a: ProjectileId,
    clockwise: bool,
    attach_point: Vec3,
    initial_velocity: Vec3,
    local_offset: Vec3,
    offset_degrees: Real,
) {
    r.iv = initial_velocity;
    r.locd = offset_degrees;
    r.clockwise = clockwise;
    // (Generator None: Get Closest Point on Polygon's outputs are None)
    let c = if r.generated {
        closest_point(&snap(w), &attach_point, tag_id("None"), r.attached)
    } else {
        ce::Closest { poly: -1, near: [0.0; 3], last: 0, next: 0 }
    };
    r.cp_poly = c.poly;
    r.cp_near = c.near;
    r.cp_last = c.last;
    r.cp_next = c.next;
    if c.poly < 0 {
        return;
    }
    // Initialize Multicast(true, Local Offset Position, (0, Y, Z)) @12106
    r.active = true;
    r.lop = local_offset;
    r.pivot = [0.0, c.near[1], c.near[2]];
    set_attached(r, c.poly);
    bind(r, c.poly);
    let local = sub(&[0.0, c.near[1], c.near[2]], &attached_flat(w, r));
    let g = get_next_point(&snap(w), r.attached, if clockwise { c.next } else { c.last }, &local, clockwise, -1);
    r.a_dest = g.dest;
    if !r.rotate_along {
        return at4872(w, r, a);
    }
    let dir = sub(&g.dest, &local);
    let rv = rotate_angle_axis(&r.iv, if clockwise { r.locd } else { -r.locd }, &[-1.0, 0.0, 0.0]);
    r.ang = signed_angle(&rv, &dir, &[1.0, 0.0, 0.0]);
    // (Rotate Component To Client-Only: nothing on the authority) @9612 -> @8777
    r.st3 = now(w, a);
    let rot = host_rot(w, a);
    r.init2 = rot;
    r.final2 = compose(&axis_angle(&[0.0, -1.0, 0.0], if clockwise { r.ang } else { -r.ang }), &rot);
    if !rot_equals(&r.final2, &rot, 1.0) {
        return at5460(w, r, a);
    }
    at4872(w, r, a);
}

/// @5460: the attach turn's step, then Delay(0) -> @4457.
fn at5460(w: &mut World, r: &mut EdgeRider, a: ProjectileId) {
    if !r.active {
        return;
    }
    let t = rlerp(&r.init2, &r.final2, alpha(now(w, a) - r.st3, r.rotate_speed));
    set_owner_rotation(w, r, a, t);
    delay(r, w.flow.tick, N_ATTACH, 0.0);
}

fn at4457(w: &mut World, r: &mut EdgeRider, a: ProjectileId) {
    if alpha(now(w, a) - r.st3, r.rotate_speed) < 1.0 {
        return at5460(w, r, a);
    }
    if !r.active {
        return;
    }
    let f = r.final2;
    set_owner_rotation(w, r, a, f);
    at4872(w, r, a);
}

/// @4872: On Attach To Edge(Normal(Destination - the near point, local)) (not bound), Move Along Edge(next or last,
/// that point).
fn at4872(w: &mut World, r: &mut EdgeRider, a: ProjectileId) {
    let local = sub(&[0.0, r.cp_near[1], r.cp_near[2]], &attached_flat(w, r));
    let n = if r.clockwise { r.cp_next } else { r.cp_last };
    move_along_edge(w, r, a, n, local, 0);
}

/// FClamp(SafeDivide(t, over), 0, 1), 1 where over is 0.
fn alpha(t: Real, over: Real) -> Real {
    if over == 0.0 { 1.0 } else { ce::clamp(t / over, 0.0, 1.0) }
}

/// EqualEqual_RotatorRotator(A, B, tolerance): FRotator::Equals, each axis's difference normalised.
fn rot_equals(a: &Vec3, b: &Vec3, tol: Real) -> bool {
    normalize_axis(a[0] - b[0]).abs() <= tol
        && normalize_axis(a[1] - b[1]).abs() <= tol
        && normalize_axis(a[2] - b[2]).abs() <= tol
}

/// Move Along Edge @9719 (`depth`: the same frame's nested calls, against a runaway).
fn move_along_edge(w: &mut World, r: &mut EdgeRider, a: ProjectileId, next_index: i32, start: Vec3, depth: i32) {
    if depth > 256 {
        warn_text(
            w,
            "edges: Move Along Edge nested past 256 calls in one frame (no edge of length > 0.01 ahead): stopped",
        );
        return;
    }
    r.ev_next = next_index;
    r.ev_start = start;
    let g = get_next_point(&snap(w), r.attached, next_index, &r.ev_start, r.clockwise, r.last_attached);
    r.d1 = g.dest;
    r.ao1 = g.poly;
    r.io1 = g.index;
    r.i1 = g.hit;
    if eq(&r.d1, &r.ev_start, 0.01) {
        if !r.active {
            return;
        }
        return at1988(w, r, a, depth);
    }
    let (s, e) = (r.ev_start, r.d1);
    calculate_turn_amt(w, r, a, &s, &e);
    if !r.rotate_along {
        return at3730(w, r, a);
    }
    // (Rotate Component To Client-Only: nothing on the authority) @8670 -> @8152
    r.st2 = now(w, a);
    let rot = host_rot(w, a);
    r.init1 = rot;
    r.final1 = compose(&axis_angle(&[0.0, -1.0, 0.0], r.ccw), &rot);
    if !rot_equals(&r.final1, &rot, 1.0) {
        return at3979(w, r, a);
    }
    at3730(w, r, a);
}

/// Calculate Turn Amt: the new direction against CurrentTravelDirection about -X; On Turn Corner where there was
/// one.
fn calculate_turn_amt(w: &mut World, r: &mut EdgeRider, a: ProjectileId, s: &Vec3, e: &Vec3) {
    let nd = normal(&sub(e, s), 1e-4);
    let ang = signed_angle(&nd, &r.ctd, &[-1.0, 0.0, 0.0]);
    if !eq(&r.ctd, &[0.0; 3], 1e-4) {
        crate::chars::stripe::saw::on_turn_corner(w, a, normal(&sub(e, s), 1e-4).map(crate::math::narrow));
    }
    r.ctd = normal(&sub(e, s), 1e-4);
    r.ccw = if r.clockwise { ang } else { -ang };
}

/// @3979: the corner turn's step, then Delay(0) -> @3315.
fn at3979(w: &mut World, r: &mut EdgeRider, a: ProjectileId) {
    if !r.active {
        return;
    }
    let t = rlerp(&r.init1, &r.final1, alpha(now(w, a) - r.st2, r.rotate_speed));
    set_owner_rotation(w, r, a, t);
    delay(r, w.flow.tick, N_TURN, 0.0);
}

fn at3315(w: &mut World, r: &mut EdgeRider, a: ProjectileId) {
    if alpha(now(w, a) - r.st2, r.rotate_speed) < 1.0 {
        return at3979(w, r, a);
    }
    if !r.active {
        return;
    }
    let f = r.final1;
    set_owner_rotation(w, r, a, f);
    at3730(w, r, a);
}

/// @3730: (Move Component To Client-Only: nothing on the authority) the move's start time, then the move's step.
fn at3730(w: &mut World, r: &mut EdgeRider, a: ProjectileId) {
    r.st = now(w, a);
    at2465(w, r, a);
}

/// The segment's alpha: (now - start) / (|Start - Destination| / Base Move Speed), 1 for a zero time (Divide_FloatFloat
/// by 0 gives 0).
fn move_alpha(w: &World, r: &EdgeRider, a: ProjectileId) -> Real {
    alpha(now(w, a) - r.st, if r.move_speed == 0.0 { 0.0 } else { dist(&r.ev_start, &r.d1) / r.move_speed })
}

fn lerp_place(w: &World, r: &EdgeRider, t: Real) -> Vec3 {
    let (f, s, d) = (attached_flat(w, r), r.ev_start, r.d1);
    [f[0] + s[0] + t * (d[0] - s[0]), f[1] + s[1] + t * (d[1] - s[1]), f[2] + s[2] + t * (d[2] - s[2])]
}

/// @2465: the move's step, then Delay(0) -> @909.
fn at2465(w: &mut World, r: &mut EdgeRider, a: ProjectileId) {
    if !r.active {
        return;
    }
    let p = lerp_place(w, r, move_alpha(w, r, a));
    set_owner_location(w, r, a, p);
    delay(r, w.flow.tick, N_MOVE, 0.0);
}

fn at909(w: &mut World, r: &mut EdgeRider, a: ProjectileId) {
    let t = move_alpha(w, r, a);
    if t < 1.0 {
        return at2465(w, r, a);
    }
    if !r.active {
        return;
    }
    let p = lerp_place(w, r, t);
    set_owner_location(w, r, a, p);
    at1988(w, r, a, 0);
}

/// @1988: the segment's end. Crossed onto another polygon: Start Point = the end (world), unbound from the polygon,
/// Set Attached Multicast, bound to the new one, Start Point less its place, Move Along Edge(Insert Index Out, Start
/// Point). Crossed with no polygon (Attach Out None, which no crossing record leaves): Delay(0), then Move Along
/// Edge(the event's Next Index, the owner's location) @3212. No crossing: Move Along Edge(Insert Index Out,
/// Destination).
fn at1988(w: &mut World, r: &mut EdgeRider, a: ProjectileId, depth: i32) {
    if !r.i1 {
        r.sp = r.d1;
        let sp = r.sp;
        return move_along_edge(w, r, a, r.io1, sp, depth + 1);
    }
    if r.ao1 < 0 {
        return delay(r, w.flow.tick, N_NONE, 0.0);
    }
    r.sp = add(&attached_flat(w, r), &r.d1);
    let at = r.attached;
    unbind(r, at);
    set_attached(r, r.ao1);
    let at = r.attached;
    bind(r, at);
    r.sp = sub(&r.sp, &attached_flat(w, r));
    let sp = r.sp;
    move_along_edge(w, r, a, r.io1, sp, depth + 1);
}

/// Set Owner Location @11592: Pivot Point = the place; the owner at Pivot - Get Relative Offset (Local Offset Position
/// rotated by the owner's rotation), no sweep.
fn set_owner_location(w: &mut World, r: &mut EdgeRider, a: ProjectileId, p: Vec3) {
    if !r.active {
        return;
    }
    r.pivot = p;
    let rot = host_rot(w, a);
    let o = qrot(&quat_of(rot[0], rot[1], rot[2]), &r.lop);
    host_set_loc(w, a, r.pivot[1] - o[1], r.pivot[2] - o[2]);
}

/// Set Owner Rotation @11820: K2_SetActorRotation (the saw's is no-op), then Set Owner Location(Pivot Point).
fn set_owner_rotation(w: &mut World, r: &mut EdgeRider, a: ProjectileId, _rot: Vec3) {
    if !r.active {
        return;
    }
    let p = r.pivot;
    set_owner_location(w, r, a, p);
}

/// `detachFromEdge`: Detach From Edge @11926: Initialize Multicast(false, 0, 0), Attached None,
/// CurrentTravelDirection 0, On Detach From Edge (not bound). LastAttached and the binds stay.
pub fn detach_from_edge(w: &mut World, a: ProjectileId) {
    if let Some(r) = rider_mut(w, a) {
        r.active = false;
        r.lop = [0.0; 3];
        r.pivot = [0.0; 3];
        r.attached = -1;
        r.ctd = [0.0; 3];
    }
}

// ------------------------------------------------------------------ what the tunnel carry calls

/// `edgeRiderOf(w, a.cls, a)` as the tunnel carry reads it: `None` for an actor whose class has no rider (read as
/// undefined), else whether it is attached (`rider.attached >= 0`).
pub fn rider_attached(w: &World, a: ActorRef) -> Option<bool> {
    match a {
        ActorRef::Projectile(id) => rider(w, id).map(|r| r.attached >= 0),
        _ => None,
    }
}

/// `detachFromEdge(w, rider, host, a)` on the actor's rider.
pub fn detach_rider(w: &mut World, a: ActorRef) {
    if let ActorRef::Projectile(id) = a {
        detach_from_edge(w, id);
    }
}

/// `attachToEdge(w, rider, host, a, rider.clockwise, [0, -x, y], rider.iv, rider.lop, rider.locd)` on the actor's
/// rider: attached again at (x, y) with its own parameters (the tunnel carry's end).
pub fn attach_rider_at(w: &mut World, a: ActorRef, x: Real, y: Real) {
    let ActorRef::Projectile(id) = a else { return };
    with_rider(w, id, |w, r| {
        let (cw, iv, lop, locd) = (r.clockwise, r.iv, r.lop, r.locd);
        attach_to_edge_with(w, r, id, cw, [0.0, -x, y], iv, lop, locd);
    });
}
