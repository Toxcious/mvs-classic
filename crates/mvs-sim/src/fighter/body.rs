//! Where the fighter is: its place and velocity, the platforms under it, its capsule,
//! the walking boxes
//! and the platforms' dealings with the capsule.

use crate::Real;
use crate::State;
use crate::actors::ActorRef;
use crate::consts::{MAX_DEPENETRATION, MAX_STEP_HEIGHT, SOLID_STEP, SOLID_WALLJUMP};
use crate::fighter::FighterId;
use crate::math::{max, min, sign, to_int32};
use crate::world::World;
use mvs_collision::Ground;
use mvs_collision::capsule::{capsule_gap, capsule_sweep};
use mvs_data::def::CharDef;

/// A platform a fighter stands on or passes (a platform object): a stage platform by its place in the
/// stage's list (`Arena2d::stage_platforms`), a spawned actor's box (the actor and the box's index in it), a stage
/// solid's box (its place in `StageState::boxes`), a run-time solid's Floor box (its `RuntimeSolid::spec`: the place
/// in `World::runtime_solids` moves as solids leave it; a run-time pass-through platform is a `Stage` one), a stage
/// solid's polygon section (its place in `StageState::solids`).
#[derive(State, Copy, PartialEq, Eq, Debug)]
pub enum PlatformRef {
    Stage(u32),
    Spawned(ActorRef, u32),
    Solid(u32),
    Runtime(i32),
    Poly(u32),
}

/// The live run-time solid of `spec` (`None`: destroyed, out of the world).
pub fn runtime_solid(w: &World, spec: i32) -> Option<&crate::world::stage::RuntimeSolid> {
    w.runtime_solids.as_ref()?.iter().find(|s| s.spec == spec && !s.destroyed)
}

/// Kinematics.
#[derive(State, Debug, Default)]
pub struct Body {
    pub x: Real,
    pub y: Real,
    pub vx: Real,
    pub vy: Real,
    /// 1 right, -1 left.
    pub facing_dir: Real,
    pub on_floor: bool,
    pub prev_x: Real,
    pub prev_y: Real,
    pub floor_swept: Real,
    /// `walkReverted`: the walk loop after a landing took the ledge branch this move.
    pub walk_reverted: bool,
    pub vel_clamped: Real,
    /// The audio listener's offsets (`akX`, `akY`).
    pub ak_x: Real,
    pub ak_y: Real,
    /// The fighter's scale (`sizeScale`).
    pub size_scale: Real,
    /// `actorScale`: the actor's scale under a buff (XY, Z).
    pub actor_scale: Option<[Real; 2]>,
    /// `weightSet`.
    pub weight_set: Option<Real>,
    /// (the port's own, dev) The depth off the fight plane of the 3D feel test (`world::depth3d`), cm; 0 with the
    /// test off.
    pub d: Real,
}

/// `MOVE_START`: his place after `base_update` and the intended delta of his walk's last
/// substep (`d`, NaN when no walk ran), written in every CMC tick before the move that reads it.
#[derive(State, Copy, PartialEq, Debug, Default)]
pub struct MoveStart {
    pub tick: i32,
    pub x: Real,
    pub y: Real,
    pub d: Real,
}

/// The platforms.
#[derive(State, Debug, Default)]
pub struct Platforms {
    pub platforms: Vec<PlatformRef>,
    pub base: Option<PlatformRef>,
    /// The stage's pass-through platforms this fighter reads, by index.
    pub stage_platforms: Vec<u32>,
    pub ignore_platforms: bool,
    pub plat_ignored: i32,
    pub walk_drop: bool,
    pub walk_base: Option<PlatformRef>,
    pub dropping_down: bool,
    pub evading_through_platform: bool,
    pub down_press_count: i32,
    pub down_press_active: bool,
    pub down_window_tick: i32,
    pub pawn_vy: Real,
    pub swept_base: Option<PlatformRef>,
    pub on_edge: Option<bool>,
    /// `noWalkOffLedges`.
    pub no_walk_off_ledges: bool,
    /// `MOVE_START`: the place the based move started from.
    pub move_start: Option<MoveStart>,
    /// `SWEPT_P`: the walking box of the hit a hitstun move's `floor_swept` code stands for
    /// (`Some(None)`: World.ground), read in the same fighter's `collide`.
    pub swept_hit: Option<Option<PlatformRef>>,
}

/// The capsule (`capHalfU` and the rest).
#[derive(State, Debug, Default)]
pub struct Capsule {
    pub cap_half_u: Real,
    pub cap_radius_u: Real,
    pub cap_resized: bool,
    pub cmc_crouched: bool,
    pub capsule_off: bool,
}

/// `StagePlatform`: one of the stage's pass-through platforms (object type Platform), as
/// the fighters, the ground traces and the dropped items meet it.
#[derive(State, Debug, Default)]
pub struct StagePlatform {
    pub stage_index: i32,
    pub left: Real,
    pub right: Real,
    pub top: Real,
    pub bottom: Real,
    pub dx: Real,
    pub dy: Real,
    pub vx: Real,
    pub vy: Real,
    /// BasePlatformBP's Moving, on a moving one alone.
    pub moving: Option<bool>,
    /// The move's LastMove.Z (`PlatformRun`), on a moving one alone.
    pub last_move_z: Option<Real>,
}

/// `Span`: a run of a platform's pieces.
#[derive(Clone, Copy, PartialEq, Debug, Default)]
pub struct Span {
    pub left: Real,
    pub right: Real,
}

/// A platform as the movement reads it (a platform's fields), resolved from a [`PlatformRef`] at
/// the call: nothing of it is kept.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Plat {
    pub r: PlatformRef,
    pub dead: bool,
    pub left: Real,
    pub right: Real,
    pub top: Real,
    pub bottom: Real,
    pub dx: Real,
    pub dy: Real,
    pub vx: Real,
    pub vy: Real,
    /// A stage's pass-through platform: its place in the stage's list.
    pub stage_index: Option<u32>,
    /// One of the walking boxes (a stage solid's box).
    pub stage_solid: bool,
    /// A polygon solid (`heightAt` / `walkableAt`).
    pub poly: bool,
    pub fresh: bool,
    /// Its WallbounceCollision box.
    pub wall: bool,
    /// It answers HandlePlatformCollision's call (`ignorePlatformCollision`).
    pub ignore_hook: bool,
    /// The moving platform's LastMove.Z.
    pub last_move_z: Option<Real>,
}

impl Plat {
    fn of_box(r: PlatformRef, left: Real, right: Real, top: Real, bottom: Real) -> Plat {
        Plat {
            r,
            dead: false,
            left,
            right,
            top,
            bottom,
            dx: 0.0,
            dy: 0.0,
            vx: 0.0,
            vy: 0.0,
            stage_index: None,
            stage_solid: false,
            poly: false,
            fresh: false,
            wall: false,
            ignore_hook: false,
            last_move_z: None,
        }
    }

    /// The box as a [`Ground`] (a walking box's cast).
    pub fn ground(&self) -> Ground {
        Ground {
            left: f64::from(self.left),
            right: f64::from(self.right),
            bottom: f64::from(self.bottom),
            top: f64::from(self.top),
        }
    }
}

/// The platform `p` as the movement reads it.
pub fn plat(w: &World, p: PlatformRef) -> Plat {
    match p {
        PlatformRef::Stage(i) => {
            let s = &w.stage.stage_platforms[i as usize];
            let mut v = Plat::of_box(p, s.left, s.right, s.top, s.bottom);
            v.dx = s.dx;
            v.dy = s.dy;
            v.vx = s.vx;
            v.vy = s.vy;
            v.stage_index = Some(i);
            v.last_move_z = s.last_move_z;
            v
        }
        PlatformRef::Solid(i) => {
            let run = w.stage_run.as_ref().expect("a stage solid's box with the stage's run");
            let b = &run.boxes[i as usize];
            let s = &run.solids[b.solid as usize];
            let m = &s.m;
            let mut v = Plat::of_box(p, b.l0 + m.dx, b.r0 + m.dx, b.t0 + m.dy, b.b0 + m.dy);
            v.dead = b.dead;
            v.dx = m.mx;
            v.dy = m.my;
            v.vx = m.vx;
            v.vy = m.vy;
            v.fresh = s.fresh;
            v.stage_solid = true;
            v
        }
        PlatformRef::Spawned(a, i) => crate::world::spawned_platform::spawned_plat(w, a, i),
        PlatformRef::Runtime(spec) => match runtime_solid(w, spec) {
            Some(rs) => {
                let g = rs.ground();
                let m = &rs.state.m;
                let mut v = Plat::of_box(
                    p,
                    crate::math::narrow(g.left),
                    crate::math::narrow(g.right),
                    crate::math::narrow(g.top),
                    crate::math::narrow(g.bottom),
                );
                v.dx = m.mx;
                v.dy = m.my;
                v.vx = m.vx;
                v.vy = m.vy;
                v.fresh = rs.state.fresh;
                v.stage_solid = true;
                v
            }
            // (removeRuntimeSolid: the box is dead, out of every list)
            None => {
                let mut v = Plat::of_box(p, 0.0, 0.0, 0.0, 0.0);
                v.dead = true;
                v.stage_solid = true;
                v
            }
        },
        // StageSolidPoly: its top and bottom moved by its StageSolid's dx / dy; `dead`, `dx`, `dy`, `vx`, `vy`
        // constant (false, 0); no `left` / `right` (undefined: NaN in its arithmetic) and no span.
        PlatformRef::Poly(i) => {
            let run = w.stage_run.as_ref().expect("a stage solid's polygon with the stage's run");
            let m = &run.solids[i as usize].m;
            let (t0, b0) = crate::fighter::traces::poly_extent(w, i);
            let mut v = Plat::of_box(p, Real::NAN, Real::NAN, t0 + m.dy, b0 + m.dy);
            v.poly = true;
            v
        }
    }
}

/// `spanAt`: the run of pieces with collision under x.
pub fn span_at(w: &World, p: &Plat, x: Real) -> Option<Span> {
    match p.r {
        PlatformRef::Spawned(a, i) => crate::world::spawned_platform::spawned_span_at(w, a, i, x),
        PlatformRef::Poly(_) => None,
        _ => (x >= p.left && x <= p.right).then_some(Span { left: p.left, right: p.right }),
    }
}

/// `spanNear`: the run within r of x.
pub fn span_near(w: &World, p: &Plat, x: Real, r: Real) -> Option<Span> {
    match p.r {
        PlatformRef::Spawned(a, i) => crate::world::spawned_platform::spawned_span_near(w, a, i, x, r),
        PlatformRef::Poly(_) => None,
        _ => (x > p.left - r && x < p.right + r).then_some(Span { left: p.left, right: p.right }),
    }
}

/// `clampX`.
pub fn clamp_x(w: &World, p: &Plat, x: Real) -> Real {
    match p.r {
        PlatformRef::Spawned(a, i) => crate::world::spawned_platform::spawned_clamp_x(w, a, i, x),
        PlatformRef::Poly(_) => x,
        _ => max(p.left, min(p.right, x)),
    }
}

/// `heightAt` (a polygon solid's flat-base floor at x); the top for a box.
pub fn height_at(w: &World, p: &Plat, x: Real, r: Real) -> Real {
    if p.poly { crate::fighter::traces::poly_height_at(w, p.r, x, r) } else { p.top }
}

/// `walkableAt` (a polygon solid's flat-base floor at x is walkable).
pub fn walkable_at(w: &World, p: &Plat, x: Real, r: Real) -> bool {
    p.poly && crate::fighter::traces::poly_walkable_at(w, p.r, x, r)
}

/// `canStepUpOn` (`None`: the platform has no such answer).
pub fn can_step_up_on_plat(w: &World, p: &Plat, x: Real, r: Real) -> Option<bool> {
    match p.r {
        PlatformRef::Solid(i) => {
            let run = w.stage_run.as_ref().expect("a stage solid's box with the stage's run");
            Some(run.boxes[i as usize].flags & SOLID_STEP != 0)
        }
        PlatformRef::Spawned(a, i) => crate::world::spawned_platform::spawned_can_step_up_on(w, a, i, x, r),
        PlatformRef::Runtime(spec) => Some(runtime_solid(w, spec).is_some_and(|rs| rs.flags & SOLID_STEP != 0)),
        PlatformRef::Poly(i) => Some(crate::fighter::traces::poly_flags(w, i) & SOLID_STEP != 0),
        PlatformRef::Stage(_) => None,
    }
}

/// `wallBounceMultiplier` (`None`: the platform has no such answer).
fn wall_bounce_multiplier(w: &mut World, f: FighterId, p: &Plat) -> Option<Real> {
    match p.r {
        PlatformRef::Spawned(a, i) => crate::world::spawned_platform::spawned_wall_bounce_multiplier(w, a, i, f),
        _ => None,
    }
}

/// `blocksWallJump` (`None`: the platform has no such answer).
pub fn blocks_wall_jump(w: &World, p: &Plat) -> Option<bool> {
    match p.r {
        PlatformRef::Solid(i) => {
            let run = w.stage_run.as_ref().expect("a stage solid's box with the stage's run");
            let b = &run.boxes[i as usize];
            Some(run.solids[b.solid as usize].enabled && b.flags & SOLID_WALLJUMP != 0)
        }
        PlatformRef::Spawned(a, i) => crate::world::spawned_platform::spawned_blocks_wall_jump(w, a, i),
        PlatformRef::Runtime(spec) => {
            Some(runtime_solid(w, spec).is_some_and(|rs| rs.state.enabled && rs.flags & SOLID_WALLJUMP != 0))
        }
        PlatformRef::Stage(_) | PlatformRef::Poly(_) => None,
    }
}

/// The platform's `ignores(f)` (`false` where it has none).
fn ignores_hook(w: &World, f: FighterId, p: &Plat) -> bool {
    match p.r {
        PlatformRef::Solid(i) => {
            let run = w.stage_run.as_ref().expect("a stage solid's box with the stage's run");
            !run.solids[run.boxes[i as usize].solid as usize].enabled
        }
        PlatformRef::Spawned(a, i) => crate::world::spawned_platform::spawned_ignores(w, a, i, f),
        PlatformRef::Runtime(spec) => runtime_solid(w, spec).is_some_and(|rs| !rs.state.enabled),
        PlatformRef::Poly(i) => {
            let run = w.stage_run.as_ref().expect("a stage solid's polygon with the stage's run");
            !run.solids[i as usize].enabled
        }
        PlatformRef::Stage(_) => false,
    }
}

// ---- the fighter's capsule and floor getters

/// The fighter's character definition (`f.ch` unscaled: the size scale is applied by the getters that read it).
pub fn def(w: &World, f: FighterId) -> &CharDef {
    &w.data.chars[w.names.str(w.fighter(f).ch)].def
}

/// A length of the def as `f.ch` holds it: times the actor's scale (`a`: XY or Z) or the size scale, in float.
fn scaled(w: &World, f: FighterId, v: Real, z: bool) -> Real {
    let b = &w.fighter(f).body;
    if let Some(a) = b.actor_scale {
        return v * if z { a[1] } else { a[0] };
    }
    if b.size_scale == 1.0 { v } else { v * b.size_scale }
}

/// `f.ch.crouchedHalf`.
pub fn crouched_half(w: &World, f: FighterId) -> Real {
    scaled(w, f, crate::math::narrow(def(w, f).crouched_half), true)
}

/// `f.ch.capHalf` (the template's, scaled: what hangs on the mesh).
pub fn ch_cap_half(w: &World, f: FighterId) -> Real {
    scaled(w, f, crate::math::narrow(def(w, f).cap_half), true)
}

/// `f.ch.capRadius` (the template's, scaled).
pub fn ch_cap_radius(w: &World, f: FighterId) -> Real {
    scaled(w, f, crate::math::narrow(def(w, f).cap_radius), false)
}

/// `f.ch.restGap`.
pub fn rest_gap(w: &World, f: FighterId) -> Real {
    crate::math::narrow(def(w, f).rest_gap)
}

/// `capHalf`: the root capsule's half height at the actor's scale.
pub fn cap_half(w: &World, f: FighterId) -> Real {
    let fi = w.fighter(f);
    if let Some(a) = fi.body.actor_scale {
        return fi.capsule.cap_half_u * a[1];
    }
    let k = fi.body.size_scale;
    if k == 1.0 { fi.capsule.cap_half_u } else { fi.capsule.cap_half_u * k }
}

/// `capRadius`: the root capsule's radius at the actor's scale.
pub fn cap_radius(w: &World, f: FighterId) -> Real {
    let fi = w.fighter(f);
    if let Some(a) = fi.body.actor_scale {
        return fi.capsule.cap_radius_u * a[0];
    }
    let k = fi.body.size_scale;
    if k == 1.0 { fi.capsule.cap_radius_u } else { fi.capsule.cap_radius_u * k }
}

/// `capsuleHalf`: the crouched half height while crouched.
pub fn capsule_half(w: &World, f: FighterId) -> Real {
    if w.fighter(f).capsule.cmc_crouched { max(crouched_half(w, f), cap_radius(w, f)) } else { cap_half(w, f) }
}

/// `actorY`: the actor's place (the crouched capsule's centre).
pub fn actor_y(w: &World, f: FighterId) -> Real {
    w.fighter(f).body.y - (cap_half(w, f) - capsule_half(w, f))
}

/// The `grounded` setter: off the floor has no base.
pub fn set_grounded(w: &mut World, f: FighterId, v: bool) {
    let fi = w.fighter_mut(f);
    fi.body.on_floor = v;
    if !v {
        fi.platforms.base = None;
    }
}

/// `standY`: his walking height over his floor (over `floor_top`).
pub fn stand_y_of(w: &World, f: FighterId) -> Real {
    floor_top(w, f) + cap_half(w, f) + rest_gap(w, f)
}

/// `noCollision`: the capsule off, the pull's move, or a buff that switched the actor's collision off.
pub fn no_collision(w: &World, f: FighterId) -> bool {
    let fi = w.fighter(f);
    fi.capsule.capsule_off || fi.moves.pull.is_some() || crate::combat::buffs::buff_collision_off(w, f)
}

// ---- floors

/// FindFloor's sweep and line distance under the capsule's bottom: max(MAX_FLOOR_DIST 2.4, MaxStepHeight 100 +
/// 2.4001) (FindFloor 14292b400); also the floor after an unswept SetActorLocation.
pub const FIND_FLOOR_DIST: Real = 102.4;
/// [`FIND_FLOOR_DIST`] in double, at the collision's and the anim's double calls.
const FIND_FLOOR_DIST_F64: f64 = 102.4;

/// Two tops at one height (a seam of equal tops), compared in double (the collision's boxes).
const SEAM_EQ_F64: f64 = 1e-3;

/// A walking box of the list: `None` for World.ground.
type Floor = Option<PlatformRef>;

/// `isFloorBox`: p as one of the walking boxes besides World.ground: a stage solid's box with its collision on.
fn is_floor_box(w: &World, f: FighterId, p: &Plat) -> bool {
    p.stage_solid && !p.dead && !ignores_hook(w, f, p)
}

/// The walking boxes with their identities: [World.ground, the stage solids' boxes that are on], in that order, read
/// as the iteration goes.
pub(crate) fn floor_boxes(w: &World, f: FighterId) -> impl Iterator<Item = (Ground, Floor)> + '_ {
    let rest = w.fighter(f).platforms.platforms.iter().filter_map(move |&r| {
        let p = plat(w, r);
        is_floor_box(w, f, &p).then(|| (p.ground(), Some(r)))
    });
    std::iter::once((w.stage.ground, None)).chain(rest)
}

/// `floorList`: the list in the World's kept buffer (`Scratch::floors`), handed back by
/// [`floor_list_done`].
pub fn floor_list(w: &mut World, f: FighterId) -> Vec<Ground> {
    let mut l = std::mem::take(&mut w.scratch.floors);
    l.clear();
    l.extend(floor_boxes(w, f).map(|(g, _)| g));
    l
}

/// The buffer of [`floor_list`] back to the World.
pub fn floor_list_done(w: &mut World, l: Vec<Ground>) {
    w.scratch.floors = l;
}

/// `floorTop`: the height of the floor a walker stands on: his base's top, World.ground's without one, the
/// flat-base floor under him on a polygon solid.
pub fn floor_top(w: &World, f: FighterId) -> Real {
    let fi = w.fighter(f);
    match fi.platforms.base {
        Some(r) => {
            let b = plat(w, r);
            if b.poly { height_at(w, &b, fi.body.x, cap_radius(w, f)) } else { b.top }
        }
        None => crate::math::narrow(w.stage.ground.top),
    }
}

/// The geometry of a walking box (World.ground for `None`).
fn floor_ground(w: &World, g: Floor) -> Ground {
    match g {
        None => w.stage.ground,
        Some(r) => plat(w, r).ground(),
    }
}

/// A walking box's bounds as the build holds them, in float32: each bound narrowed once, so that an edge test and
/// the clamp to the same edge (`bound ± R`) are both summed in float32 and agree on the next tick.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Bounds {
    pub left: Real,
    pub right: Real,
    pub bottom: Real,
    pub top: Real,
}

impl Bounds {
    pub(crate) fn of(g: &Ground) -> Self {
        let n = crate::math::narrow;
        Self { left: n(g.left), right: n(g.right), bottom: n(g.bottom), top: n(g.top) }
    }
}

/// `floorHolder`: the walking box that holds a walker at x on base b (`Some(None)`: World.ground; `None`: none, he
/// leaves the floor). A base that is no walking box holds him while his centre is within R of its span.
// Deviation: such a base has no seam to another box at its height (a walk R past its end falls), and no polygon
// under the base is looked for.
pub fn floor_holder(w: &World, f: FighterId, b: Floor, x: Real) -> Option<Floor> {
    if let Some(r) = b {
        let p = plat(w, r);
        if !p.stage_solid && !p.poly {
            return span_near(w, &p, x, cap_radius(w, f)).map(|_| b);
        }
        if p.poly {
            return poly_floor_at(w, f, &p, x);
        }
    }
    let h = box_holder(w, f, b, x);
    poly_over(w, f, b, x, h)
}

/// `reach`: the reach of FindFloor's sweep around a walker's capsule bottom: FIND_FLOOR_DIST under it, and over it
/// the bottom of ComputeFloorDist 142926820's second sweep, 0.9 (H - R).
// Unverified: the result's clamp to -max(2.4, R) and AdjustFloorHeight's swept move up are not modelled (a walker's
// floor rises a few units a frame on a flank); the base's half width stays R for the shrunk sweep (polyFloor).
fn reach(w: &World, f: FighterId, b: Floor) -> (Real, Real) {
    let top = match b {
        Some(r) => {
            let p = plat(w, r);
            if p.poly { height_at(w, &p, w.fighter(f).body.x, cap_radius(w, f)) } else { p.top }
        }
        None => crate::math::narrow(w.stage.ground.top),
    };
    let bottom = top + rest_gap(w, f);
    (bottom - FIND_FLOOR_DIST, bottom + 0.9 * (cap_half(w, f) - cap_radius(w, f)))
}

/// `livePoly`: a polygon of the fighter's platforms the walker's capsule meets (its collision on).
fn live_poly(w: &World, f: FighterId, p: &Plat) -> bool {
    p.poly && !p.dead && !ignores_hook(w, f, p)
}

/// `polyOver`: on a box (or World.ground): the box rule's floor `h`, or the highest polygon whose flat-base floor
/// at x is walkable, over h's top and within the reach.
fn poly_over(w: &World, f: FighterId, b: Floor, x: Real, h: Option<Floor>) -> Option<Floor> {
    let r = cap_radius(w, f);
    let mut best = h;
    let mut top = match h {
        Some(g) => crate::math::narrow(floor_ground(w, g).top),
        None => Real::NEG_INFINITY,
    };
    let mut rc: Option<(Real, Real)> = None;
    for &pr in &w.fighter(f).platforms.platforms {
        let p = plat(w, pr);
        if !live_poly(w, f, &p) || !walkable_at(w, &p, x, r) {
            continue;
        }
        let y = height_at(w, &p, x, r);
        if y <= top {
            continue;
        }
        let c = *rc.get_or_insert_with(|| reach(w, f, b));
        if y < c.0 || y > c.1 {
            continue;
        }
        best = Some(Some(pr));
        top = y;
    }
    best
}

/// `polyFloorAt`: on a polygon b: the highest of its walkable flat-base floor at x, the walking boxes' tops under
/// the base and the other polygons' walkable floors, within the reach.
// Unverified: the boxes' own seam rule (an equal top keeps the box he came from) is not taken here: the highest
// wins.
// Deviation: the stage's pass-through platforms, spawned boxes and ship runs are not among the floors looked at
// here or in poly_over (FindFloor sweeps every Pawn-blocking shape).
fn poly_floor_at(w: &World, f: FighterId, b: &Plat, x: Real) -> Option<Floor> {
    let r = cap_radius(w, f);
    let (lo, hi) = reach(w, f, Some(b.r));
    let mut best: Option<Floor> = None;
    let mut top = Real::NEG_INFINITY;
    if walkable_at(w, b, x, r) {
        best = Some(Some(b.r));
        top = height_at(w, b, x, r);
    }
    for (g, id) in floor_boxes(w, f) {
        let gb = Bounds::of(&g);
        if x < gb.left - r || x > gb.right + r || gb.top <= top || gb.top < lo || gb.top > hi {
            continue;
        }
        best = Some(id);
        top = crate::math::narrow(g.top);
    }
    for &pr in &w.fighter(f).platforms.platforms {
        let p = plat(w, pr);
        if pr == b.r || !live_poly(w, f, &p) || !walkable_at(w, &p, x, r) {
            continue;
        }
        let y = height_at(w, &p, x, r);
        if y <= top || y < lo || y > hi {
            continue;
        }
        best = Some(Some(pr));
        top = y;
    }
    best
}

/// `boxHolder`: the box rule (b a walking box or `None` for World.ground).
fn box_holder(w: &World, f: FighterId, b: Floor, x: Real) -> Option<Floor> {
    let r = cap_radius(w, f);
    let own = floor_ground(w, b);
    let ob = Bounds::of(&own);
    if x >= ob.left - r && x <= ob.right + r {
        return Some(b);
    }
    let h = own.top;
    let g = w.stage.ground;
    let gb = Bounds::of(&g);
    if b.is_some() && (g.top - h).abs() <= SEAM_EQ_F64 && x >= gb.left - r && x <= gb.right + r {
        return Some(None);
    }
    for &pr in &w.fighter(f).platforms.platforms {
        if Some(pr) == b {
            continue;
        }
        let p = plat(w, pr);
        if !is_floor_box(w, f, &p) {
            continue;
        }
        if (f64::from(p.top) - h).abs() <= SEAM_EQ_F64 && x >= p.left - r && x <= p.right + r {
            return Some(Some(pr));
        }
    }
    step_down(w, f, b, own, x)
}

/// `stepDown`: the walking box FindFloor meets under a walker at x past the end of the tops at his box's height.
fn step_down(w: &World, f: FighterId, own_id: Floor, own: Ground, x: Real) -> Option<Floor> {
    let r = cap_radius(w, f);
    let h = own.top;
    let lo = h + f64::from(rest_gap(w, f)) - FIND_FLOOR_DIST_F64;
    let mut best: Option<(Ground, Floor)> = None;
    for (g, id) in floor_boxes(w, f) {
        if id == own_id
            || g.top >= h
            || g.top < lo
            || x < crate::math::narrow(g.left) - r
            || x > crate::math::narrow(g.right) + r
        {
            continue;
        }
        if best.is_none_or(|(bg, _)| g.top > bg.top) {
            best = Some((g, id));
        }
    }
    best.map(|(_, id)| id)
}

/// `rebase`: a walker whose own box no longer holds him but another one does: that box is his base, and at
/// another height he is put at his walking height over it, Velocity.Z 0.
// Deviation: AdjustFloorHeight's move down is a swept SafeMoveUpdatedComponent; here he is put at his walking
// height over the lower top unswept, at the frame's end, not in the substep.
pub fn rebase(w: &mut World, f: FighterId) {
    let fi = w.fighter(f);
    let b = fi.platforms.base;
    if !fi.body.on_floor {
        return;
    }
    if let Some(r) = b {
        let p = plat(w, r);
        if !p.stage_solid && !p.poly {
            return;
        }
    }
    let x = fi.body.x;
    let Some(nb) = floor_holder(w, f, b, x) else { return };
    if nb == b {
        return;
    }
    let top = floor_top(w, f);
    w.fighter_mut(f).platforms.base = nb;
    if floor_top(w, f) != top {
        let y = stand_y_of(w, f);
        let fi = w.fighter_mut(f);
        fi.body.y = y;
        fi.body.vy = 0.0;
    }
}

/// `clampFloor`: the ledge branch's clamp at the end of the walker's floor (bCanWalkOffLedges false).
// Deviation: on a polygon the last walkable place between his frame's start and x is found by bisection, as the
// box's clamp, not the substep's revert.
pub fn clamp_floor(w: &World, f: FighterId, b: Floor, x: Real) -> Real {
    let r = cap_radius(w, f);
    let prev_x = w.fighter(f).body.prev_x;
    if let Some(br) = b {
        let p = plat(w, br);
        if p.poly {
            if walkable_at(w, &p, x, r) {
                return x;
            }
            let mut ok = prev_x;
            let mut bad = x;
            if !walkable_at(w, &p, ok, r) {
                return x;
            }
            for _ in 0..24 {
                let m = (ok + bad) / 2.0;
                if walkable_at(w, &p, m, r) {
                    ok = m;
                } else {
                    bad = m;
                }
            }
            return ok;
        }
        if !p.stage_solid {
            return match span_near(w, &p, prev_x, r).or_else(|| span_near(w, &p, x, r)) {
                Some(s) => max(s.left - r, min(s.right + r, x)),
                None => clamp_x(w, &p, x),
            };
        }
    }
    let ob = Bounds::of(&floor_ground(w, b));
    max(ob.left - r, min(ob.right + r, x))
}

/// `ledgeCorner`: whether the top corner of box g at ex is a ledge: no other walking box holds the corner point.
// Deviation: the other half of the build's ledge rule (no other top within the 102.4 reach below) is a walker's
// rule (floor_holder) and not taken for the fall's round.
pub fn ledge_corner(w: &World, f: FighterId, g: &Ground, ex: Real) -> bool {
    !floor_boxes(w, f).any(|(o, _)| holds_point(g, &o, ex))
}

/// `holdsPoint`.
// Deviation: the box's identity is not kept; a box of the same bounds is the same one
// here.
fn holds_point(g: &Ground, o: &Ground, ex: Real) -> bool {
    o != g
        && ex >= crate::math::narrow(o.left)
        && ex <= crate::math::narrow(o.right)
        && g.top >= o.bottom
        && g.top <= o.top
}

/// `floorUnder`: the walking box under a capsule at (x, y) that x is within R of and
/// whose top lies under the centre and no more than FindFloor's 102.4 under the capsule's bottom.
// Unverified: the build's FindFloor after an unswept SetActorLocation is taken as this test.
pub fn floor_under(w: &mut World, f: FighterId, x: Real, y: Real) -> Option<Ground> {
    let r = cap_radius(w, f);
    let bottom = y - cap_half(w, f);
    let mut best: Option<Ground> = None;
    for (g, _) in floor_boxes(w, f) {
        let gb = Bounds::of(&g);
        if x < gb.left - r || x > gb.right + r || gb.top >= y || bottom - gb.top > FIND_FLOOR_DIST {
            continue;
        }
        if best.is_none_or(|b| g.top > b.top) {
            best = Some(g);
        }
    }
    best
}

/// `onPlatformFloor`: OnPlatform_BTD's test of the floor he walks on (CurrentFloor's hit component's object type
/// == ECC_GameTraceChannel7 Platform).
// Unverified: a base of no known object type (a spawned box) is taken as Platform.
pub fn on_platform_floor(w: &World, f: FighterId) -> bool {
    let fi = w.fighter(f);
    let b = if fi.body.on_floor { fi.platforms.base } else { None };
    match b {
        None => false,
        Some(PlatformRef::Solid(i)) => crate::world::stage::solid_object_type_is_platform(w, i),
        Some(PlatformRef::Poly(i)) => crate::fighter::traces::solid_object_type(w, i).is_none_or(|t| t == "Platform"),
        Some(_) => true,
    }
}

// ---- platforms

/// `baseUpdate`: the CMC tick's dealings with a moving floor, ahead of the movement.
pub fn base_update(w: &mut World, f: FighterId, walking: bool) {
    base_follow(w, f, walking);
    let fi = w.fighter_mut(f);
    fi.platforms.move_start = Some(MoveStart { tick: fi.clock.tick, x: fi.body.x, y: fi.body.y, d: Real::NAN });
}

/// `setWalkDelta`: the walk's last substep's intended delta.
pub fn set_walk_delta(w: &mut World, f: FighterId, d: Real) {
    let fi = w.fighter_mut(f);
    let tick = fi.clock.tick;
    if let Some(a) = fi.platforms.move_start.as_mut()
        && a.tick == tick
    {
        a.d = d;
    }
}

/// `shiftMoveStart`: the start-penetration resolve moved the capsule by (dx, dy).
pub fn shift_move_start(w: &mut World, f: FighterId, dx: Real, dy: Real) {
    let fi = w.fighter_mut(f);
    let tick = fi.clock.tick;
    if let Some(a) = fi.platforms.move_start.as_mut()
        && a.tick == tick
    {
        a.x += dx;
        a.y += dy;
    }
}

fn base_follow(w: &mut World, f: FighterId, walking: bool) {
    if no_collision(w, f) || w.fighter(f).life.dead > 0.0 {
        return;
    }
    let Some(br) = w.fighter(f).platforms.base else {
        if w.fighter(f).body.on_floor {
            let top = w.stage.ground.top;
            lift_onto(w, f, crate::math::narrow(top));
        }
        return;
    };
    let b = plat(w, br);
    if b.dead {
        let flying = w.fighter(f).moves.flying;
        w.fighter_mut(f).platforms.walk_drop = walking && !flying;
        set_grounded(w, f, false);
    } else if ignores_platform(w, f, br) {
        let fi = w.fighter_mut(f);
        fi.platforms.walk_drop = fi.body.on_floor;
        fi.platforms.walk_base = Some(br);
        set_grounded(w, f, false);
    } else {
        let blocked = follow_base(w, f, b.dx, b.dy);
        // A follow the stage block or a spawned box stopped leaves the capsule off its rest over the box.
        // Deviation: the rest is taken in that tick, whatever its distance (no R + 2.15 limit a tick).
        let x = w.fighter(f).body.x;
        let mut lost = floor_holder(w, f, Some(br), x).is_none();
        if blocked && !lost {
            let b = plat(w, br);
            if (w.fighter(f).body.y - cap_half(w, f) - b.top).abs() > FIND_FLOOR_DIST {
                lost = true;
            } else {
                // AdjustFloorHeight's move to the rest is a swept SafeMoveUpdatedComponent: a box over him stops it
                let dy = stand_y_of(w, f) - w.fighter(f).body.y;
                let k = swept_share(w, f, 0.0, dy);
                w.fighter_mut(f).body.y += dy * k;
            }
        }
        if lost {
            set_grounded(w, f, false);
            if walking && !w.fighter(f).moves.flying {
                let fi = w.fighter_mut(f);
                fi.platforms.walk_drop = true;
                fi.platforms.walk_base = Some(br);
            } else {
                impart_base(w, f, br);
            }
        } else {
            rebase(w, f); // over the next box at his box's height: that one is his base
            let top = floor_top(w, f);
            lift_onto(w, f, top);
        }
    }
}

/// `liftOnto`: a face that rose to his feet, higher than the floor he stands on and under his capsule's centre,
/// becomes his floor.
fn lift_onto(w: &mut World, f: FighterId, floor: Real) {
    let fi = w.fighter(f);
    let (x, y, base) = (fi.body.x, fi.body.y, fi.platforms.base);
    let mut k = 0;
    while k < w.fighter(f).platforms.platforms.len() {
        let pr = w.fighter(f).platforms.platforms[k];
        k += 1;
        let p = plat(w, pr);
        if Some(pr) == base
            || p.dead
            || p.top <= floor
            || p.top > y
            || span_at(w, &p, x).is_none()
            || ignores_platform(w, f, pr)
        {
            continue;
        }
        w.fighter_mut(f).platforms.base = Some(pr);
        let sy = stand_y_of(w, f);
        w.fighter_mut(f).body.y = sy;
        return;
    }
}

/// `followBase`: UpdateBasedMovement's move by the base's delta (142950510), a swept MoveUpdatedComponent. True
/// when the sweep stopped the move short.
// Deviation: a stage's pass-through platform is not swept (met by the walk that follows); a ship is met by the run
// of pieces nearest his start alone (spanNear); no pull-back; from a start in touch the move is made whole when it
// does not take the capsule's axis nearer the box, else not at all (no slide along it).
fn follow_base(w: &mut World, f: FighterId, dx: Real, dy: Real) -> bool {
    if dx == 0.0 && dy == 0.0 {
        return false;
    }
    let k = swept_share(w, f, dx, dy);
    let fi = w.fighter_mut(f);
    fi.body.x += dx * k;
    fi.body.y += dy * k;
    k < 1.0
}

/// `sweptShare`: the share of the move (dx, dy) the capsule makes before it meets the stage block or a spawned box.
fn swept_share(w: &World, f: FighterId, dx: Real, dy: Real) -> Real {
    let r = cap_radius(w, f);
    let hh = cap_half(w, f);
    let fi = w.fighter(f);
    let (x, y) = (fi.body.x, fi.body.y);
    let mut k = follow_sweep(x, y, r, hh, dx, dy, &w.stage.ground);
    for &pr in &fi.platforms.platforms {
        if k == 0.0 {
            break;
        }
        let p = plat(w, pr);
        if Some(pr) == fi.platforms.base || p.dead || ignores_platform(w, f, pr) {
            continue;
        }
        let Some(s) = span_near(w, &p, x, r + dx.abs()) else { continue };
        if min(y, y + dy) - hh >= p.top || max(y, y + dy) + hh <= p.bottom {
            continue;
        }
        let g = Ground {
            left: f64::from(s.left),
            right: f64::from(s.right),
            top: f64::from(p.top),
            bottom: f64::from(p.bottom),
        };
        k = min(k, follow_sweep(x, y, r, hh, dx, dy, &g));
    }
    k
}

/// `followSweep`: the share of the move (0..1) the capsule makes before it meets the box g.
fn follow_sweep(x: Real, y: Real, r: Real, hh: Real, dx: Real, dy: Real, g: &Ground) -> Real {
    match capsule_sweep(x, y, x + dx, y + dy, r, hh, g) {
        None => 1.0,
        Some(t) if t > 0.0 => crate::math::narrow(t),
        Some(_) => {
            if capsule_gap(x + dx, y + dy, r, hh, g) >= capsule_gap(x, y, r, hh, g) {
                1.0
            } else {
                0.0
            }
        }
    }
}

/// `floorActor`: CMC CurrentFloor.HitResult's HitActor while the fighter walks: the platform he stands on (`None` on
/// the stage block and in the air).
// Unverified: CurrentFloor is cleared by the switch out of Walking (4.26 source, not read in the exe).
pub fn floor_actor(w: &World, f: FighterId) -> Option<PlatformRef> {
    let fi = w.fighter(f);
    if fi.body.on_floor { fi.platforms.base } else { None }
}

/// `ignoresPlatform`: a stage platform by the fighter's bit, a box by its own list.
pub fn ignores_platform(w: &World, f: FighterId, p: PlatformRef) -> bool {
    if let PlatformRef::Stage(i) = p {
        return (w.fighter(f).platforms.plat_ignored >> i) & 1 == 1;
    }
    ignores_hook(w, f, &plat(w, p))
}

/// `handlePlatformCollision`: BaseFighter HandlePlatformCollision (ubergraph 161605
/// -> 160377, then the loop at 105314).
// Unverified: the ignore list filters the capsule's own sweeps only, the Box's bounds are its box, Velocity.Z in a
// hitpause is the CMC's. Deviation: the lock's other callers (buff stun move, teleport to ally) are not ported.
pub fn handle_platform_collision(w: &mut World, f: FighterId, vy: Real) {
    let fi = w.fighter(f);
    let has_stage = !fi.platforms.stage_platforms.is_empty();
    let spawned = fi.platforms.platforms.iter().any(|&r| {
        let p = plat(w, r);
        p.ignore_hook && !p.wall && !p.dead
    });
    if !has_stage && !spawned {
        return;
    }
    let pd = &fi.platforms;
    let ip = (fi.crouch.pushing_down && crate::fighter::in_air(w, f))
        || pd.dropping_down
        || vy > 0.0
        || pd.evading_through_platform
        || pd.down_press_count >= 1
        || crate::fighter::anim::has_active(w, f, crate::name!("AnimNotify_IgnorePlatforms_C"), -1.0);
    w.fighter_mut(f).platforms.ignore_platforms = ip;
    // @133833-133991: the drop's Delay(0.1), whose link sends DropThroughPlatform unless the movement base's owner is
    // a BasePlatformBP (drop_tutorial_tick). Deviation: the Delay runs only while a tutorial runs.
    // Unverified: the Delay is taken to fire in the pawn's latent pass as jump's fast_fall_delay_fire.
    let fi = w.fighter(f);
    let pd = &fi.platforms;
    if ip
        && fi.tutorial.drop_tutorial_at.is_none()
        && (pd.dropping_down || pd.down_press_count >= 1 || pd.evading_through_platform)
        && crate::fighter::match_side::tutorial_world(w, f)
    {
        let at = fi.clock.tick + to_int32(crate::fighter::common::latent_frames(0.1)) - 1;
        w.fighter_mut(f).tutorial.drop_tutorial_at = Some(at);
    }
    // GetScaledCapsuleRadius / HalfHeight (BasePlatformBP @1582, @1654): the current size
    let r = cap_radius(w, f);
    let fi = w.fighter(f);
    let (x, bottom) = (fi.body.x, fi.body.y - cap_half(w, f));
    let mut bits = 0i32;
    // ShouldIgnoreCollision @0-1148: the top less FMax(LastMove.Z, 0), the Box's bounds where the platform is now.
    for &i in &fi.platforms.stage_platforms {
        let p = &w.stage.stage_platforms[i as usize];
        let top = match p.last_move_z {
            Some(z) => p.top - max(z, 0.0),
            None => p.top,
        };
        if ip || bottom + 10.0 <= top || x - r > p.right || x + r < p.left {
            bits |= 1i32.wrapping_shl(p.stage_index.cast_unsigned());
        }
    }
    w.fighter_mut(f).platforms.plat_ignored = bits;
    if spawned {
        let mut k = 0;
        while k < w.fighter(f).platforms.platforms.len() {
            let pr = w.fighter(f).platforms.platforms[k];
            k += 1;
            let p = plat(w, pr);
            if !p.dead && !p.wall && p.ignore_hook {
                crate::world::spawned_platform::spawned_ignore_platform_collision(w, pr, f, ip, vy);
            }
        }
    }
}

/// `dropTutorialTick`: the platform drop's Delay(0.1) link: DropThroughPlatform when
/// the movement base's owner is not a BasePlatformBP (@21763 -> @21874).
pub fn drop_tutorial_tick(w: &mut World, f: FighterId) {
    let fi = w.fighter(f);
    let Some(at) = fi.tutorial.drop_tutorial_at else { return };
    if fi.clock.tick < at {
        return;
    }
    let on_stage_platform = matches!(fi.platforms.base, Some(PlatformRef::Stage(_)));
    w.fighter_mut(f).tutorial.drop_tutorial_at = None;
    if !on_stage_platform {
        crate::fighter::match_side::tutorial_event_of(
            w,
            f,
            crate::fighter::match_side::TutorialEvent::DropThroughPlatform,
        );
    }
}

/// `collideStagePlatforms`: a fighter in the air against the pass-through platforms, after a move without a floor
/// sweep. Deviation: the capsule's centre decides, a frame's move at a time.
pub fn collide_stage_platforms(w: &mut World, f: FighterId, hitstun: bool) -> bool {
    if w.fighter(f).body.vy > 0.0 {
        return false;
    }
    let half = cap_half(w, f);
    let mut k = 0;
    while k < w.fighter(f).platforms.stage_platforms.len() {
        let i = w.fighter(f).platforms.stage_platforms[k];
        k += 1;
        let pr = PlatformRef::Stage(i);
        let fi = w.fighter(f);
        let p = &w.stage.stage_platforms[i as usize];
        if ignores_platform(w, f, pr) || fi.body.y > p.top + half || fi.body.x < p.left || fi.body.x > p.right {
            continue;
        }
        w.fighter_mut(f).platforms.base = Some(pr);
        let sy = stand_y_of(w, f);
        w.fighter_mut(f).body.y = sy;
        if hitstun && crate::combat::hit::try_bounce(w, f, 0.0, 1.0, true, 1.0) {
            w.fighter_mut(f).platforms.base = None;
            return false;
        }
        if !hitstun
            && !(crate::fighter::moves::bounce_free(w, f) && crate::combat::hit::try_bounce(w, f, 0.0, 1.0, true, 1.0))
        {
            crate::fighter::moves::land(w, f);
        }
        return true;
    }
    false
}

/// `impartBase`.
pub fn impart_base(w: &mut World, f: FighterId, b: PlatformRef) {
    let p = plat(w, b);
    let fi = w.fighter_mut(f);
    fi.body.vx += p.vx;
    fi.body.vy += p.vy;
}

/// `stageSolidHit(f, p, ox, oy)` on a platform: a stage solid's box or polygon alone has a hit owner.
fn solid_hit(w: &mut World, f: FighterId, p: PlatformRef, ox: Real, oy: Real) {
    match p {
        PlatformRef::Solid(i) => crate::world::stage::stage_solid_box_hit(w, f, i, ox, oy),
        PlatformRef::Poly(i) => crate::world::stage::stage_solid_hit(w, f, Some(i), ox, oy),
        _ => {}
    }
}

/// `collidePlatforms`: a fighter in the air against the spawned platforms' pieces, after its move. Returns true on
/// a landing. `kb_slide`: `collide`'s (the frame's PhysKnockback slide Z without stick).
// Deviation: the first tests are a box of the capsule's radius and half height, a frame's move at a time (no
// substep sweep or pull-back); 'from above' is the capsule's bottom of the frame's start against the face before
// the ship's move; a bounce off a piece in hitstun is taken as off the stage block; the way out is the 2D capsule's
// against the run as one box, taken after the frame's move instead of before it for a capsule that was not inside
// at the frame's start, and the resolve's sweeps for a place that is not free are left out.
pub fn collide_platforms(w: &mut World, f: FighterId, hitstun: bool, kb_slide: Option<Real>) -> bool {
    let r = cap_radius(w, f);
    let half = cap_half(w, f);
    let mut k = 0;
    while k < w.fighter(f).platforms.platforms.len() {
        let pr = w.fighter(f).platforms.platforms[k];
        k += 1;
        let p = plat(w, pr);
        if p.dead || ignores_platform(w, f, pr) {
            continue;
        }
        let fi = w.fighter(f);
        let Some(s) = span_near(w, &p, fi.body.x, r) else { continue };
        let (t, b) = (p.top, p.bottom);
        if fi.body.y - half >= t || fi.body.y + half <= b {
            continue;
        }
        if fi.body.x >= s.left && fi.body.x <= s.right && fi.body.prev_y - half >= t - p.dy - 1.0 {
            if fi.body.vy > 0.0 {
                w.fighter_mut(f).body.y = t + half;
                continue;
            }
            w.fighter_mut(f).platforms.base = Some(pr);
            let sy = stand_y_of(w, f);
            w.fighter_mut(f).body.y = sy;
            let m = hit_mult(w, f, &p);
            if hitstun && crate::combat::hit::try_bounce(w, f, 0.0, 1.0, true, m) {
                w.fighter_mut(f).platforms.base = None;
                solid_hit(w, f, pr, 0.0, 1.0);
                return false;
            }
            if !hitstun
                && !(crate::fighter::moves::bounce_free(w, f)
                    && crate::combat::hit::try_bounce(w, f, 0.0, 1.0, true, m))
            {
                crate::fighter::moves::land(w, f);
            }
            solid_hit(w, f, pr, 0.0, 1.0); // the landing's hit, after Landed
            return true;
        }
        if fi.body.prev_y + half <= b - p.dy + 1.0 {
            w.fighter_mut(f).body.y = b - half;
            let m = hit_mult(w, f, &p);
            let bounced = (hitstun || crate::fighter::moves::bounce_free(w, f))
                && crate::combat::hit::try_bounce(w, f, 0.0, -1.0, true, m);
            solid_hit(w, f, pr, 0.0, -1.0);
            if bounced {
                return false;
            }
            let fi = w.fighter_mut(f);
            if fi.body.vy > 0.0 {
                fi.body.vy = 0.0;
            }
        } else {
            let (nx, ny);
            let mut slid = false;
            let (prev_x, prev_y) = (fi.body.prev_x, fi.body.prev_y);
            let (d0, n0x, n0y) = cap_out(s.left, s.right, b, t, prev_x, prev_y, r, half - r);
            if d0 >= 0.0 {
                // The move starts in penetration: out of the nearer face by depth + 0.125 (GetPenetrationAdjustment
                // 1429309a0, clamped to MaxDepenetrationWithGeometry 500), and the same Delta swept again.
                // Deviation: the resolve is taken as free and the retry's slide keeps the move along the face; a
                // retry back in through the top lands a frame late.
                (nx, ny) = (n0x, n0y);
                let k = min(d0 + 0.125, 500.0);
                let fi = w.fighter_mut(f);
                let (dx, dy) = (fi.body.x - prev_x, fi.body.y - prev_y);
                let (px, py) = (prev_x + nx * k, prev_y + ny * k);
                fi.body.x = px + dx;
                fi.body.y = py + dy;
                let (x, y) = (fi.body.x, fi.body.y);
                if cap_out(s.left, s.right, b, t, x, y, r, half - r).0 < 0.0 {
                    continue;
                }
                let into = dx * nx + dy * ny;
                let fi = w.fighter_mut(f);
                fi.body.x -= nx * into;
                fi.body.y -= ny * into;
                // In the knockback mode without stick (PhysKnockback's hit handling is PhysFalling's, native_pt20
                // L1) a side face's retry meets the face 0.125 away, inside PullBackHit's share of the substep's
                // move: at time 0 in both substeps, so the capsule holds at the resolved place and the substeps'
                // slides (ComputeSlideVector along the face) carry the whole Z. The second substep has no teleport
                // (bJustTeleported is the resolve's), so its velocity is the slide's over the time left: nothing
                // into the face, Z unchanged. Live read (a fighter knocked into WW_ShieldActor's
                // WallbounceCollision short of its mid-plane): the build holds x at the face through the bounce's
                // pause and rises 23.999 over the bounce's frame, the sum of the substeps' end velocity Z x timeTick
                // (the port's trapezoid move rose 0.634 more).
                // Unverified: the slide of the end velocity is 4.26 PhysFalling's limited air control Adjusted
                // ((VelocityNoAirControl + AirControlDeltaV) x timeTick), not read in PhysKnockback 1410d8100; a
                // substep whose move into the face is under max(1.25, 0.125 x its length) meets the face later in
                // the substep (taken at 0); with the stick the slide keeps the trapezoid move.
                if let Some(s) = kb_slide
                    && ny == 0.0
                {
                    fi.body.x = px;
                    fi.body.y = py + s;
                    slid = true;
                }
            } else {
                let (x, y) = (fi.body.x, fi.body.y);
                let (depth, n1x, n1y) = cap_out(s.left, s.right, b, t, x, y, r, half - r);
                if depth < 0.0 {
                    continue;
                }
                (nx, ny) = (n1x, n1y);
                let fi = w.fighter_mut(f);
                fi.body.x += nx * depth;
                fi.body.y += ny * depth;
            }
            if nx.abs() >= ny.abs() {
                let side = if nx < 0.0 { -1.0 } else { 1.0 };
                let m = hit_mult(w, f, &p);
                let bounced = (hitstun || crate::fighter::moves::bounce_free(w, f))
                    && crate::combat::hit::try_bounce(w, f, side, 0.0, true, m);
                solid_hit(w, f, pr, side, 0.0);
                if slid {
                    w.fighter_mut(f).body.vx = 0.0; // the slide's velocity, under the bounce's pending launch
                }
                if bounced {
                    return false;
                }
                let fi = w.fighter_mut(f);
                if sign(fi.body.vx) == -side {
                    fi.body.vx = 0.0;
                }
            } else if ny < 0.0 {
                let m = hit_mult(w, f, &p);
                let bounced = (hitstun || crate::fighter::moves::bounce_free(w, f))
                    && crate::combat::hit::try_bounce(w, f, 0.0, -1.0, true, m);
                solid_hit(w, f, pr, 0.0, -1.0);
                if bounced {
                    return false;
                }
                let fi = w.fighter_mut(f);
                if fi.body.vy > 0.0 {
                    fi.body.vy = 0.0;
                }
            } else {
                hit_mult(w, f, &p);
                solid_hit(w, f, pr, nx, ny); // out over the top with no landing
            }
        }
    }
    false
}

/// `capsuleOut` with its `OUT_N`: the depth (negative: no overlap) and the way out's normal.
#[allow(clippy::too_many_arguments, reason = "the call's parameters")]
fn cap_out(
    left: Real,
    right: Real,
    bottom: Real,
    top: Real,
    x: Real,
    y: Real,
    r: Real,
    hs: Real,
) -> (Real, Real, Real) {
    crate::fighter::common::capsule_out_n(left, right, bottom, top, x, y, r, hs)
}

/// `capsuleOut`'s depth alone.
#[allow(clippy::too_many_arguments, reason = "the call's parameters")]
fn cap_out1(left: Real, right: Real, bottom: Real, top: Real, x: Real, y: Real, r: Real, hs: Real) -> Real {
    cap_out(left, right, bottom, top, x, y, r, hs).0
}

/// `hitMult`: BaseFighter's capsule hit on a spawned box (ubergraph @128863-129102): its
/// BPI_GetWallBounceMultiplier(self), 1.0 for any other box.
// Deviation: the build asks it on each capsule hit event of the move (OnComponentHit); the port at
// collide_platforms' branches only.
fn hit_mult(w: &mut World, f: FighterId, p: &Plat) -> Real {
    wall_bounce_multiplier(w, f, p).unwrap_or(1.0)
}

/// `walkOutOfPlatforms`: a walking fighter against the spawned platforms' boxes that are not his base, after his
/// move: the walk's sweep stops at a box's side, and a move that starts inside one is pushed out first.
// Deviation: the MTD is the full 2D capsule's against the box (capsule_out), after the frame's move; for a box he
// walked into, only a way out to a side is taken (along x); the step up is not modelled as StepUp's lift, forward
// and down sweeps; a box he stood in before it moved is not told apart from one he walked into.
pub fn walk_out_of_platforms(w: &mut World, f: FighterId) {
    let fi = w.fighter(f);
    let st = fi.platforms.move_start.filter(|a| a.tick == fi.clock.tick);
    let (x0, y0) = match st {
        Some(a) => (a.x, a.y),
        None => (fi.body.prev_x, fi.body.prev_y),
    };
    // bZeroDelta: Delta = Velocity x timeTick, IsNearlyZero (1e-4 a component): the intended move of his walk's last
    // substep. Without a walk this tick, what he moved.
    let d = st.map_or(Real::NAN, |a| a.d);
    #[allow(clippy::eq_op, reason = "d === d: a NaN test")]
    let still = if d == d { d.abs() <= 1e-4 } else { (fi.body.x - x0).abs() <= 1e-4 && (fi.body.y - y0).abs() <= 1e-4 };
    let r = cap_radius(w, f);
    let hs = cap_half(w, f) - r;
    let floor = floor_top(w, f);
    let mut k = 0;
    while k < w.fighter(f).platforms.platforms.len() {
        let pr = w.fighter(f).platforms.platforms[k];
        k += 1;
        let p = plat(w, pr);
        if Some(pr) == w.fighter(f).platforms.base || p.dead || ignores_platform(w, f, pr) {
            continue;
        }
        if still {
            if pop_out(w, f, &p, r, hs) {
                return;
            }
            continue;
        }
        let (x, y) = (w.fighter(f).body.x, w.fighter(f).body.y);
        let Some(s) = span_near(w, &p, x, r) else { continue };
        let in_start = cap_out1(s.left, s.right, p.bottom, p.top, x0, y0, r, hs) >= 0.0;
        let rose = in_start
            && (p.fresh || cap_out1(s.left - p.dx, s.right - p.dx, p.bottom - p.dy, p.top - p.dy, x0, y0, r, hs) < 0.0);
        if !rose && p.top - floor <= MAX_STEP_HEIGHT && can_step_up_on_plat(w, &p, x, r) == Some(true) {
            continue;
        }
        let (depth, nx, ny) = cap_out(s.left, s.right, p.bottom, p.top, x, y, r, hs);
        if depth < 0.0 {
            continue;
        }
        if rose && ny > nx.abs() && span_at(w, &p, x).is_some() {
            w.fighter_mut(f).platforms.base = Some(pr);
            let sy = stand_y_of(w, f);
            w.fighter_mut(f).body.y = sy;
            return;
        }
        if nx.abs() < ny.abs() {
            continue;
        }
        let k = if in_start { min(depth + 0.125, MAX_DEPENETRATION) } else { depth };
        let fi = w.fighter_mut(f);
        fi.body.x += nx * k;
        if sign(fi.body.vx) == -sign(nx) {
            fi.body.vx = 0.0;
        }
        solid_hit(w, f, pr, if nx < 0.0 { -1.0 } else { 1.0 }, 0.0); // the walk's side hit
    }
}

/// A walker on a base beside World.ground (the stage block) whose top stands over his floor meets the block's side as
/// MoveAlongFloor's sweep does any blocking box (PhysWalking 14293b840 -> MoveAlongFloor 142935be0): within
/// MaxStepHeight (100) of his floor StepUp takes him onto its top, which becomes his floor; higher, the sweep stops him
/// at its side (the walked-into rule of `walk_out_of_platforms`: the 2D capsule's way out along x, depth + 0.125 from a
/// start in touch). True when he stepped onto it. Throne Room's side elevators run under the block's sides and go down
/// beside it; with no such test he walked into the block and was popped back out by the next move's depenetration.
// Deviation: the step is taken on touch, not as StepUp's lift, forward and down sweeps; a way out up or down is left
// to the other rules, as for the boxes.
// Unverified: the block's CanCharacterStepUpOn is taken as Yes (the stock default; the stage data does not carry the
// floor's flags).
pub fn walk_out_of_ground(w: &mut World, f: FighterId) -> bool {
    if w.fighter(f).platforms.base.is_none() {
        return false; // the block is his floor
    }
    let g = Bounds::of(&w.stage.ground);
    let floor = floor_top(w, f);
    if g.top <= floor {
        return false;
    }
    let r = cap_radius(w, f);
    let hs = cap_half(w, f) - r;
    let fi = w.fighter(f);
    let (x, y) = (fi.body.x, fi.body.y);
    let (depth, nx, ny) = cap_out(g.left, g.right, g.bottom, g.top, x, y, r, hs);
    if depth < 0.0 {
        return false;
    }
    if g.top - floor <= MAX_STEP_HEIGHT {
        w.fighter_mut(f).platforms.base = None;
        let sy = stand_y_of(w, f);
        w.fighter_mut(f).body.y = sy;
        return true;
    }
    if nx.abs() < ny.abs() {
        return false;
    }
    let st = fi.platforms.move_start.filter(|a| a.tick == fi.clock.tick);
    let (x0, y0) = match st {
        Some(a) => (a.x, a.y),
        None => (fi.body.prev_x, fi.body.prev_y),
    };
    let in_start = cap_out1(g.left, g.right, g.bottom, g.top, x0, y0, r, hs) >= 0.0;
    let k = if in_start { min(depth + 0.125, MAX_DEPENETRATION) } else { depth };
    let fi = w.fighter_mut(f);
    fi.body.x += nx * k;
    if sign(fi.body.vx) == -sign(nx) {
        fi.body.vx = 0.0;
    }
    false
}

/// `popOut`: a still walker's floor check against a box: true when it popped him out and he falls.
fn pop_out(w: &mut World, f: FighterId, p: &Plat, r: Real, hs: Real) -> bool {
    let (x, y) = (w.fighter(f).body.x, w.fighter(f).body.y);
    let Some(s) = span_at(w, p, x) else { return false };
    if y <= p.bottom || y >= p.top {
        return false;
    }
    let (depth, nx, ny) = cap_out(s.left, s.right, p.bottom, p.top, x, y, r, hs);
    if depth < 0.0 {
        return false;
    }
    // bCanWalkOffLedges false: the ledge branch comes first: CheckFall with bMustJump, no pop.
    if crate::fighter::moves::can_walk_off_ledges(w, f) {
        let k = min(depth + 0.125, MAX_DEPENETRATION);
        let fi = w.fighter_mut(f);
        fi.body.x += nx * k;
        fi.body.y += ny * k;
    }
    let b = w.fighter(f).platforms.base; // grounded = false clears it; StartFalling imparts the old base's velocity
    set_grounded(w, f, false);
    w.fighter_mut(f).body.vy = 0.0;
    if let Some(b) = b {
        impart_base(w, f, b);
    }
    true
}

/// `liftOverPlatforms`: a fighter in the air against the spawned platforms' pieces in
/// its own hitpause: the dilated move starts in penetration and is resolved along the face's normal.
// Deviation: the same box test as collide_platforms; a dilated move under MoveComponent's minimum is not swept in the
// engine and would leave the overlap for that frame; a fall over 1500 would reach the face inside the pause.
pub fn lift_over_platforms(w: &mut World, f: FighterId) {
    let half = cap_half(w, f);
    let mut k = 0;
    while k < w.fighter(f).platforms.platforms.len() {
        let pr = w.fighter(f).platforms.platforms[k];
        k += 1;
        let p = plat(w, pr);
        let x = w.fighter(f).body.x;
        if p.dead || span_at(w, &p, x).is_none() || ignores_platform(w, f, pr) {
            continue;
        }
        let fi = w.fighter_mut(f);
        if fi.body.y - half >= p.top || fi.body.y + half <= p.bottom {
            continue;
        }
        if fi.body.prev_y - half >= p.top - p.dy - 1.0 {
            fi.body.y = p.top + half;
        }
    }
}

/// The movement base's floor height under `x` (`heightAt(x, r)` of a polygon solid, else its `top`), read by
/// `Fighter.standY`.
pub fn base_top(w: &World, b: PlatformRef, x: Real, r: Real) -> Real {
    height_at(w, &plat(w, b), x, r)
}

/// `floorUnder(f, x, y)` as the movement base keeps it: the floor under (x, y)
/// unless it is `World.ground`.
pub fn floor_under_base(w: &mut World, f: FighterId, x: Real, y: Real) -> Option<PlatformRef> {
    let r = cap_radius(w, f);
    let bottom = y - cap_half(w, f);
    let mut best: Option<(Ground, Floor)> = None;
    for (g, fl) in floor_boxes(w, f) {
        let gb = Bounds::of(&g);
        if x < gb.left - r || x > gb.right + r || gb.top >= y || bottom - gb.top > FIND_FLOOR_DIST {
            continue;
        }
        if best.is_none_or(|(b, _)| g.top > b.top) {
            best = Some((g, fl));
        }
    }
    best.and_then(|(_, fl)| fl)
}

/// A platform's box as a surface-sound trace reads it: the span's left and right, the platform's top and
/// bottom.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct TraceSpan {
    pub left: Real,
    pub right: Real,
    pub top: Real,
    pub bottom: Real,
}

/// The surface-sound trace's read: `None` for a platform that is dead, has `trace === false` or no span there; else
/// `p.spanNear(x, near)` (`near` given) or `p.spanAt(x)`, with the platform's top and bottom. A spawned box's and a
/// stage solid's box's `trace` are read; a run-time solid's box is taken as traced.
// Unverified: a run-time solid's `trace` (the tutorial's alone) is not read.
pub fn trace_span(w: &World, p: PlatformRef, x: Real, near: Option<Real>) -> Option<TraceSpan> {
    let v = plat(w, p);
    if v.dead {
        return None;
    }
    if let PlatformRef::Spawned(a, i) = p
        && !crate::world::spawned_platform::spawned_trace(w, a, i)
    {
        return None;
    }
    // `p.trace === false` (the ground trace): a stage solid switched off, or whose profile does not block the
    // trace channel (SolidBox.trace), is not met.
    if let PlatformRef::Solid(b) = p {
        let run = w.stage_run.as_ref().expect("a stage solid's box with the stage's run");
        if !run.boxes[b as usize].trace(run) {
            return None;
        }
    }
    let s = match near {
        Some(r) => span_near(w, &v, x, r),
        None => span_at(w, &v, x),
    }?;
    Some(TraceSpan { left: s.left, right: s.right, top: v.top, bottom: v.bottom })
}
