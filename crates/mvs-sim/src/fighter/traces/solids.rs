//! A stage's solid colliders besides its block and its pass-through platforms (StageData.solids,
//! every blocking Box and static mesh box / convex element of the gameplay level, in the match), the marks the movers
//! read on their boxes, and the spawned owners' place for the edge polygons
//!. Each solid's state is its `StageState::solids` entry.
//!
//! The play plane is UE X = 0 (sim x = -UE Y, y = UE Z): a solid is its section by that plane.
//! - A section that is a rectangle on the axes is a `SolidBox` (`StageState::boxes`), a box of the fighters' platforms
//!   and a Ground of `World::blockers` for the queries its profile blocks.
//! - Any other section is a [`StageSolidPoly`], a convex polygon.
//!
//! Deviation: the section at X = 0 is the capsule's centre plane: a solid that ends behind the plane within the
//! capsule's radius is not met, and a solid that does not reach X = 0 is left out.
//! Deviation: the stage block's own rules (its ledges and edge traces, the corner check, the respawn and the camera)
//! read `Arena2d::ground` alone, not the solids, also those tagged TraceEdges.
//! Unverified: the way out of a polygon is the separating axis of least depth over its edge normals and the play
//! plane's axes (the capsule's round ends are not axes of it), taken after the frame's move with no sweep; the
//! velocity into it is dropped; it is not in `World::blockers` nor in the box readers of [`static_solids`]: the
//! shape traces meet it as its polygon, the projectiles' and the items' moves through [`blocker_polys`] (each box
//! test's polygon twin: [`PolyBlock`]) and the stage queries (`world/stage_query.rs`), Banana Guard's walker, a
//! fighter's teleport spot test, a hold's release check and the view's seismic beam among them.
//! Not ported: a solid of object type Platform (`SOLID_ONE_WAY`: a Box the stage data does not take as a
//! pass-through platform; none known) is left out of the fighters' movement with a warning; its queries keep it.

use crate::Real;
use crate::actors::ActorRef;
use crate::consts::{
    MAX_DEPENETRATION, MAX_STEP_HEIGHT, SOLID_DETECT, SOLID_FLOOR, SOLID_ITEM, SOLID_NAV, SOLID_ONE_WAY, SOLID_STEP,
    SOLID_TRACE, SOLID_WALL,
};
use crate::fighter::FighterId;
use crate::fighter::body::PlatformRef;
use crate::math;
use crate::names::Name;
use crate::world::World;
use crate::world::spawned_platform::BlockKind;
use crate::world::stage::{RuntimeSolid, SolidBox, StageMoved, StageSolid};
use mvs_collision::Ground;
use mvs_collision::solids::{Out, Poly, Section, WALKABLE_Z, poly_floor, poly_out};
use mvs_data::stage::StageSolidData;
use std::io::Write as _;

/// What `kinematicOf` gives: the box's kinematic move of this step (the last pass's move less its teleports).
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct KinematicOfOut {
    pub dx: Real,
    pub dy: Real,
}

/// `StageSolidPoly`: a solid whose section is any other convex polygon: `poly`'s xs / ys
/// its corners counter-clockwise at the data place, moved by its `StageSolid`'s dx / dy. Its object type
/// (`POLY_TYPE`) is its data's `object_type`, read there; its Platform members `dead`, `dx`, `dy`, `vx`, `vy` are
/// constant (false, 0). A fighter meets it as `PlatformRef::Poly`.
#[derive(Clone, Debug)]
pub struct StageSolidPoly {
    /// Its `StageState::solids` index.
    pub solid: u32,
    pub flags: i32,
    pub poly: std::sync::Arc<Poly>,
    /// The highest and the lowest corner at the data place (Math.max / Math.min of `ys`).
    pub t0: Real,
    pub b0: Real,
}

impl StageSolidPoly {
    pub fn new(solid: u32, flags: i32, xs: Vec<f64>, ys: Vec<f64>) -> Self {
        let t0 = crate::math::narrow(ys.iter().fold(f64::NEG_INFINITY, |m, &y| crate::math::dbl::max(m, y)));
        let b0 = crate::math::narrow(ys.iter().fold(f64::INFINITY, |m, &y| crate::math::dbl::min(m, y)));
        StageSolidPoly { solid, flags, poly: std::sync::Arc::new(Poly { xs, ys }), t0, b0 }
    }
}

/// `Solid`: a solid in the plane: a box (its `StageState::boxes` index when the fighters meet it, `None` for one the
/// queries alone meet) or a polygon.
#[derive(Clone, Debug)]
pub enum Solid {
    Box(Option<u32>, SolidBox),
    Poly(StageSolidPoly),
}

impl Solid {
    /// Its `StageState::solids` index.
    pub fn solid(&self) -> u32 {
        match self {
            Solid::Box(_, b) => b.solid,
            Solid::Poly(p) => p.solid,
        }
    }

    pub fn flags(&self) -> i32 {
        match self {
            Solid::Box(_, b) => b.flags,
            Solid::Poly(p) => p.flags,
        }
    }
}

/// `Solids`: `all` every solid in the plane (`StageState::boxes`, the boxes no capsule meets, the polygons);
/// `fighters` the polygons the fighters' platforms take besides `StageState::boxes` (they block Pawn, not one-way).
#[derive(Clone, Debug, Default)]
pub struct Solids {
    pub all: Vec<Solid>,
    pub fighters: Vec<StageSolidPoly>,
}

// ---- solid mark

/// `markStaticSolid`: the stage solid's box is of object type WorldStatic (not
/// Platform): a body that ignores Platform still meets it as it meets the block. The mark is the solid's
/// (`StageSolid::is_static`), which its one box carries.
pub fn mark_static_solid(w: &mut World, solid: u32) {
    let run = w.stage_run.as_mut().expect("a stage solid with the stage's run");
    run.solids[solid as usize].is_static = true;
}

/// The `StageSolid` of a box of `StageState::boxes`.
fn box_solid(w: &World, b: u32) -> &StageSolid {
    let run = w.stage_run.as_ref().expect("a stage solid's box with the stage's run");
    &run.solids[run.boxes[b as usize].solid as usize]
}

/// `isStaticSolid`: the platform is a stage solid's box marked by [`stage_solids`] or a
/// run-time Floor box (marked at its spawn).
pub fn is_static_solid(w: &World, p: PlatformRef) -> bool {
    match p {
        PlatformRef::Solid(b) => box_solid(w, b).is_static,
        PlatformRef::Runtime(spec) => crate::fighter::body::runtime_solid(w, spec).is_some_and(|s| s.state.is_static),
        _ => false,
    }
}

/// A piece's kinematic move of this step (`kinematicOf` of its mark): the pass's move less its teleports.
fn moved_kinematic(m: &StageMoved) -> KinematicOfOut {
    KinematicOfOut { dx: m.tpx.map_or(m.mx, |t| m.mx - t), dy: m.tpy.map_or(m.my, |t| m.my - t) }
}

/// The kinematic move of the pass-through platform `i` of `Arena2d::stage_platforms`: a moving one's
/// (`makeMovingPlatforms`' markKinematic over its `PlatformRun`) or a run-time Platform's
/// (`addRuntimeSolid`'s over its state); `None` for an unmarked one.
fn platform_kinematic(w: &World, i: u32) -> Option<KinematicOfOut> {
    if w.stage.stage_platforms.get(i as usize)?.moving == Some(true)
        && let Some(r) = w.stage_run.as_ref().and_then(|r| r.platforms.iter().find(|p| p.index == i))
    {
        return Some(moved_kinematic(&r.m));
    }
    let rs = w.runtime_solids.as_ref()?;
    let s = rs.iter().find(|s| s.solid == crate::world::stage::RuntimeBox::Platform(i))?;
    Some(moved_kinematic(&s.state.m))
}

/// `kinematicOf`: the box's kinematic move of this step (`markKinematic`: a stage
/// solid's box, a run-time solid's, a moving platform's Box), `None` for a box that is no marked piece.
pub fn kinematic_of(w: &World, p: PlatformRef) -> Option<KinematicOfOut> {
    match p {
        PlatformRef::Solid(b) => Some(moved_kinematic(&box_solid(w, b).m)),
        PlatformRef::Runtime(spec) => crate::fighter::body::runtime_solid(w, spec).map(|s| moved_kinematic(&s.state.m)),
        PlatformRef::Stage(i) => platform_kinematic(w, i),
        // (a polygon is not marked: StageSolidPoly is no SolidBox)
        PlatformRef::Spawned(..) | PlatformRef::Poly(_) => None,
    }
}

/// `staticOf`: the marked boxes of a list.
pub fn static_of(w: &World, list: &[SolidBox]) -> Vec<SolidBox> {
    let Some(run) = w.stage_run.as_ref() else { return Vec::new() };
    list.iter().filter(|b| run.solids.get(b.solid as usize).is_some_and(|s| s.is_static)).copied().collect()
}

/// `staticOf` over `World::blockers`' boxes (`isStaticSolid` of each): the ones that are a marked box (a stage
/// solid's box of object type WorldStatic, a run-time Floor box), in the list's order, into `out` (cleared).
// Deviation: the mark is not read off the box object; `World::blockers` gives boxes without their identity, so a
// box is taken as marked when a marked live box lies at its place (as `sphere_static`): a pass-through platform or a
// spawned box at the very place of a marked solid would be taken as one.
pub fn statics_among(w: &World, list: &[Ground], out: &mut Vec<Ground>) {
    out.clear();
    if !w.may_hold_static_solids() {
        return;
    }
    let run = w.stage_run.as_ref();
    let has_solids = run.is_some_and(|r| !r.solids.is_empty());
    let made;
    let all = match &w.scratch.solids {
        Some(s) => Some(&**s),
        None if has_solids => {
            made = solids_of(w);
            Some(&made)
        }
        None => None,
    };
    for g in list {
        let solid = all.is_some_and(|all| {
            all.all.iter().any(|x| match x {
                Solid::Box(_, b) => {
                    !b.dead && run.is_some_and(|r| r.solids[b.solid as usize].is_static) && solid_box_ground(w, b) == *g
                }
                Solid::Poly(_) => false,
            })
        });
        if solid || crate::world::stage::runtime_floor_boxes(w).any(|s| s.state.is_static && s.ground() == *g) {
            out.push(*g);
        }
    }
}

/// `isStaticSolid` of a box of `World::blockers` (see [`statics_among`]).
pub fn is_static_ground(w: &World, g: Ground) -> bool {
    let mut out = Vec::new();
    statics_among(w, std::slice::from_ref(&g), &mut out);
    !out.is_empty()
}

/// The `Ground` of a solid's box where it is this tick (`SolidBox`'s getters: its data place moved
/// by its `StageSolid`'s dx / dy).
pub fn solid_box_ground(w: &World, b: &SolidBox) -> Ground {
    let run = w.stage_run.as_ref().expect("a stage solid's box with the stage's run");
    let m = &run.solids[b.solid as usize].m;
    Ground {
        left: f64::from(b.l0 + m.dx),
        right: f64::from(b.r0 + m.dx),
        bottom: f64::from(b.b0 + m.dy),
        top: f64::from(b.t0 + m.dy),
    }
}

/// A box of `World.ground` / `World.blockers` as a rigid body's static (the body step reads
/// `kinematicOf` and `isStaticSolid` of the box itself): its kinematic move this step and its WorldStatic stage solid
/// mark.
// Deviation: the marks are not read off the box object; `World::blockers` gives boxes without their identity, so
// the box is looked up by its place: among the run-time Floor boxes, the stage solids' boxes (the fighters' ones, then
// the ones the queries alone meet) and the pass-through platforms, the first of equal boxes. Two marked pieces at one
// place with different marks would read the first's.
pub fn sphere_static(w: &World, g: Ground) -> mvs_collision::rigid_sphere::Static {
    let of = |k: Option<KinematicOfOut>, st: bool| mvs_collision::rigid_sphere::Static {
        g,
        kinematic: k.map(|k| (f64::from(k.dx), f64::from(k.dy))),
        static_solid: st,
        poly: None,
    };
    if g == w.stage.ground {
        return of(None, false);
    }
    for s in crate::world::stage::runtime_floor_boxes(w) {
        if s.ground() == g {
            return of(Some(moved_kinematic(&s.state.m)), s.state.is_static);
        }
    }
    if let Some(run) = w.stage_run.as_ref() {
        for b in &run.boxes {
            if !b.dead && solid_box_ground(w, b) == g {
                let s = &run.solids[b.solid as usize];
                return of(Some(moved_kinematic(&s.m)), s.is_static);
            }
        }
        if !run.solids.is_empty() {
            let made;
            let all = match &w.scratch.solids {
                Some(s) => s,
                None => {
                    made = solids_of(w);
                    &made
                }
            };
            for x in &all.all {
                if let Solid::Box(None, b) = x
                    && solid_box_ground(w, b) == g
                {
                    let s = &run.solids[b.solid as usize];
                    return of(Some(moved_kinematic(&s.m)), s.is_static);
                }
            }
        }
    }
    for (i, p) in w.stage.stage_platforms.iter().enumerate() {
        if (Ground {
            left: f64::from(p.left),
            right: f64::from(p.right),
            bottom: f64::from(p.bottom),
            top: f64::from(p.top),
        }) == g
        {
            let i = u32::try_from(i).expect("a platform index");
            if let Some(k) = platform_kinematic(w, i) {
                return of(Some(k), false);
            }
        }
    }
    of(None, false)
}

/// `warnOnce` of a message keyed by `key`: the message is built at the first warning alone (a per-call path's key is
/// written to a stack buffer, no allocation while warned).
fn warn_keyed(w: &World, key: std::fmt::Arguments, msg: impl FnOnce() -> String) {
    let mut buf = [0u8; 160];
    let mut c = std::io::Cursor::new(&mut buf[..]);
    let fits = c.write_fmt(key).is_ok();
    let n = usize::try_from(c.position()).expect("a buffer place");
    let built;
    let key = if fits {
        std::str::from_utf8(&buf[..n]).expect("a key of formatted text")
    } else {
        built = key.to_string();
        &built
    };
    let mut seen = w.warned.borrow_mut();
    if seen.contains(key) {
        return;
    }
    seen.insert(Box::from(key));
    drop(seen);
    eprintln!("[sim] {}", msg());
}

// ---- solids

/// `kindFlag`: the stage data's bit of a `World.blockers` kind (dynamic: none).
fn kind_flag(kind: BlockKind) -> i32 {
    match kind {
        BlockKind::Trace => SOLID_TRACE,
        BlockKind::Nav => SOLID_NAV,
        BlockKind::Item => SOLID_ITEM,
        BlockKind::Detect => SOLID_DETECT,
        BlockKind::Dynamic => 0,
    }
}

/// `pawn`: the bits of a solid that blocks Pawn.
const PAWN: i32 = SOLID_FLOOR | SOLID_WALL;

/// The world's solids without their marks (`stageSolids`' list), made from the stage's data and `StageState::boxes`
/// (which no step changes).
fn solids_of(w: &World) -> Solids {
    let mut s = Solids::default();
    let Some(run) = w.stage_run.as_ref() else { return s };
    if run.solids.is_empty() {
        return s;
    }
    let data = w.data.stage.solids.as_deref().unwrap_or(&[]);
    for (i, st) in run.solids.iter().enumerate() {
        let i = u32::try_from(i).expect("a solid index");
        let d = data
            .get(i as usize)
            .filter(|d| d.id == st.id)
            .unwrap_or_else(|| panic!("stage solid {i}: no data of that id"));
        // (a shape that blocks nothing, a placed actor's overlap box: ScoobyElevator's DustVFXCollision; its actor's
        // own sim reads it, no query here meets it)
        if d.flags & !SOLID_STEP == 0 {
            continue;
        }
        let Some(x) = section(d) else { continue };
        if let Some(bx) = x.box_ {
            // the fighters' one (StageState::boxes), else one for the queries alone
            let own = run.boxes.iter().position(|b| b.solid == i).map(|b| u32::try_from(b).expect("a box index"));
            let b = own.map_or(
                SolidBox {
                    dead: false,
                    solid: i,
                    flags: d.flags,
                    l0: crate::math::narrow(bx[0]),
                    r0: crate::math::narrow(bx[1]),
                    t0: crate::math::narrow(bx[3]),
                    b0: crate::math::narrow(bx[2]),
                },
                |b| run.boxes[b as usize],
            );
            s.all.push(Solid::Box(own, b));
            continue;
        }
        let sol = StageSolidPoly::new(i, d.flags, x.xs, x.ys);
        // (a one-way one: warned by solidBoxes)
        if d.flags & PAWN != 0 && d.flags & SOLID_ONE_WAY == 0 {
            s.fighters.push(sol.clone());
        }
        s.all.push(Solid::Poly(sol));
    }
    s
}

/// `stageSolids`: the world's solids, made at the first call with solids and kept by the
/// World (`Scratch::solids`), its WorldStatic boxes marked ([`mark_static_solid`]).
// Deviation: the boxes are marked at every call (not once as the list is made): a state loaded from
// before the first call is marked again.
pub fn stage_solids(w: &mut World) -> std::sync::Arc<Solids> {
    let s = match &w.scratch.solids {
        Some(s) => std::sync::Arc::clone(s),
        None => {
            let s = std::sync::Arc::new(solids_of(w));
            if w.stage_run.as_ref().is_some_and(|r| !r.solids.is_empty()) {
                w.scratch.solids = Some(std::sync::Arc::clone(&s));
            }
            s
        }
    };
    for x in &s.all {
        if let Solid::Box(_, b) = x
            && b.flags & SOLID_ONE_WAY == 0
        {
            mark_static_solid(w, b.solid);
        }
    }
    s
}

/// The name of a solid's data.
fn solid_name(w: &World, s: u32) -> &str {
    let run = w.stage_run.as_ref().expect("the stage's run");
    let id = usize::try_from(run.solids[s as usize].id).expect("a solid's data id");
    &w.data.stage.solids.as_deref().expect("the stage's solids")[id].name
}

/// `solidBlockers`: `World.blockers`' boxes of the stage's solids that block a query of
/// the kind, with their collision on, added to its list (`None`: none yet; `base` is copied for the first one).
/// Kind dynamic (WorldDynamic): the stage data writes no bit for it; a solid of object type Platform (one-way), whose
/// response to WorldDynamic the data lacks, is warned and left out (none known).
pub fn solid_blockers(
    w: &mut World,
    kind: BlockKind,
    base: &[Ground],
    out: Option<Vec<Ground>>,
) -> Option<Vec<Ground>> {
    solid_blockers_marked(w, kind, base, out, None)
}

/// [`solid_blockers`] that also pushes each box's mark (the solid's data id) to `marks`.
pub fn solid_blockers_marked(
    w: &mut World,
    kind: BlockKind,
    base: &[Ground],
    out: Option<Vec<Ground>>,
    marks: Option<&mut Vec<crate::world::spawned_platform::BoxMark>>,
) -> Option<Vec<Ground>> {
    let all = stage_solids(w);
    solid_blockers_of(w, &all, kind, base, out, marks)
}

/// [`solid_blockers_marked`] that reads the World alone: the solids as kept, or made for the call where none are kept
/// yet, their boxes not marked (no box reader reads the mark).
pub fn solid_blockers_read(
    w: &World,
    kind: BlockKind,
    base: &[Ground],
    out: Option<Vec<Ground>>,
    marks: Option<&mut Vec<crate::world::spawned_platform::BoxMark>>,
) -> Option<Vec<Ground>> {
    with_solids(w, |all| solid_blockers_of(w, all, kind, base, out, marks))
}

/// The walk of both over `all`.
fn solid_blockers_of(
    w: &World,
    all: &Solids,
    kind: BlockKind,
    base: &[Ground],
    mut out: Option<Vec<Ground>>,
    mut marks: Option<&mut Vec<crate::world::spawned_platform::BoxMark>>,
) -> Option<Vec<Ground>> {
    let bit = kind_flag(kind);
    let all = &all.all;
    let run = w.stage_run.as_ref();
    let enabled = |s: u32| run.is_some_and(|r| r.solids[s as usize].enabled);
    if bit == 0 {
        for s in all {
            if s.flags() & SOLID_ONE_WAY != 0 && enabled(s.solid()) {
                warn_keyed(w, format_args!("solidBlockers one-way {kind:?} {}", s.solid()), || {
                    format!(
                        "stage solid {}: its response to {kind:?} is not in the stage data, left out",
                        solid_name(w, s.solid())
                    )
                });
            }
        }
        return out;
    }
    for s in all {
        match s {
            // (a polygon is no box: a reader meets it through `blocker_polys`)
            Solid::Poly(_) => {}
            Solid::Box(_, b) => {
                // `on`: the box's StageSolid's collision
                if b.flags & bit == 0 || !enabled(b.solid) {
                    continue;
                }
                let g = solid_box_ground(w, b);
                out.get_or_insert_with(|| base.to_vec()).push(g);
                if let Some(m) = marks.as_deref_mut() {
                    let id = run.expect("the stage's run").solids[b.solid as usize].id;
                    m.push(crate::world::spawned_platform::BoxMark::Solid(id));
                }
            }
        }
    }
    out
}

/// The warning on a polygon solid a reader of boxes alone ([`static_solids`] with no polygon list) would meet.
fn warn_poly(w: &World, solid: u32) {
    warn_keyed(w, format_args!("solid polygon {solid}"), || {
        format!("stage solid {}: a polygon section, not met by a reader of boxes alone", solid_name(w, solid))
    });
}

/// A polygon solid where it stands this tick, as a mover meets it besides `World::blockers`' boxes: its
/// `StageState::solids` index, its section, its move from the data place and whether it is of object type
/// WorldStatic (met as the stage block is, whatever the mover's response to Platform).
#[derive(Clone, Debug)]
pub struct PolyBlock {
    pub solid: u32,
    pub poly: std::sync::Arc<Poly>,
    pub ox: Real,
    pub oy: Real,
    pub is_static: bool,
}

impl PolyBlock {
    /// The point of the polygon nearest (x, y) ((x, y) inside it): the box readers' clamp to a box.
    pub fn closest(&self, x: Real, y: Real) -> (Real, Real) {
        let (qx, qy) = mvs_collision::solids::poly_closest(&self.poly, self.ox, self.oy, x, y);
        (crate::math::narrow(qx), crate::math::narrow(qy))
    }

    /// Its upper outline at x: the box readers' top.
    pub fn top_at(&self, x: Real) -> Real {
        crate::math::narrow(mvs_collision::solids::poly_top(&self.poly, self.ox, self.oy, x))
    }

    /// The least way out of it of the capsule at (x, y) (radius r, half axis h along (ux, uy)): capsule_mtd's rule.
    #[allow(clippy::too_many_arguments, reason = "a place and the capsule's shape")]
    pub fn mtd(&self, x: Real, y: Real, r: Real, h: Real, ux: Real, uy: Real) -> mvs_collision::capsule_mtd::Mtd {
        mvs_collision::solids::capsule_poly_mtd(&self.poly, self.ox, self.oy, x, y, r, h, ux, uy)
    }

    /// Whether (x, y) is inside it (on its outline counts): the box readers' `left <= x <= right` and the same for y.
    pub fn inside(&self, x: Real, y: Real) -> bool {
        mvs_collision::solids::poly_inside(&self.poly, self.ox, self.oy, x, y)
    }

    /// The box round it.
    pub fn bounds(&self) -> Ground {
        mvs_collision::solids::poly_bounds(&self.poly, self.ox, self.oy)
    }

    /// The sweep of the query shape (a box of half extents qx, qy rounded by qr) from (x0, y0) by (dx, dy) against it:
    /// the shape traces' convex sweep (`mvs_collision::traces`, its body the polygon extruded along the depth).
    #[allow(clippy::too_many_arguments, reason = "a sweep and its shape")]
    pub fn sweep(
        &self,
        x0: Real,
        y0: Real,
        dx: Real,
        dy: Real,
        qx: Real,
        qy: Real,
        qr: Real,
    ) -> Option<mvs_collision::traces::Hit> {
        let q = mvs_collision::traces::Query {
            x0: f64::from(x0),
            y0: f64::from(y0),
            dx: f64::from(dx),
            dy: f64::from(dy),
            qx: f64::from(qx),
            qy: f64::from(qy),
            qr: f64::from(qr),
            qd: 0.0,
        };
        // (one hull scratch per thread: a sweep writes all of it before reading it)
        thread_local! {
            static TRACER: std::cell::RefCell<mvs_collision::traces::Tracer> =
                std::cell::RefCell::new(mvs_collision::traces::Tracer::default());
        }
        TRACER.with_borrow_mut(|t| t.sweep_corners(&q, &self.poly.xs, &self.poly.ys, self.ox, self.oy))
    }

    /// [`PolyBlock::sweep`] of an upright capsule (radius r, half height hh) whose centre moves a -> b: the time
    /// (0..1) of the first touch, 0 when it starts in touch: `capsuleSweep`'s answer for a box.
    #[allow(clippy::too_many_arguments, reason = "a sweep and its shape")]
    pub fn capsule_sweep(&self, ax: Real, ay: Real, bx: Real, by: Real, r: Real, hh: Real) -> Option<Real> {
        let s = math::max(hh - r, 0.0);
        self.sweep(ax, ay, bx - ax, by - ay, 0.0, s, r).map(|h| crate::math::narrow(h.time))
    }

    /// Whether the box of centre (x, y) and half extents (hx, hy) meets it (touching counts).
    pub fn box_overlaps(&self, x: Real, y: Real, hx: Real, hy: Real) -> bool {
        self.sweep(x, y, 0.0, 0.0, hx, hy, 0.0).is_some()
    }

    /// The least way out of it of the box of centre (x, y) and half extents (hx, hy): its normal and depth (the shape
    /// traces' overlap at the start), `None` where they do not meet.
    pub fn box_mtd(&self, x: Real, y: Real, hx: Real, hy: Real) -> Option<(Real, Real, Real)> {
        let h = self.sweep(x, y, 0.0, 0.0, hx, hy, 0.0)?;
        Some((crate::math::narrow(h.nx), crate::math::narrow(h.ny), crate::math::narrow(h.depth)))
    }

    /// A sphere of radius r traced straight down at x from z0 to z1 against it: the sphere's centre and the impact
    /// point's height at the hit, and whether it started inside (both then z0).
    pub fn down(&self, x: Real, z0: Real, z1: Real, r: Real) -> Option<(Real, Real, bool)> {
        let h = self.sweep(x, z0, 0.0, z1 - z0, 0.0, 0.0, r)?;
        Some(if h.initial { (z0, z0, true) } else { (crate::math::narrow(h.y), crate::math::narrow(h.iy), false) })
    }

    /// The line from a to b against it: its first hit (time 0 for a start inside it).
    pub fn ray(&self, ax: Real, ay: Real, bx: Real, by: Real) -> Option<mvs_collision::traces::Hit> {
        self.sweep(ax, ay, bx - ax, by - ay, 0.0, 0.0, 0.0)
    }

    /// The square of the distance from the segment a -> b to it (0 where the segment meets it).
    pub fn seg_dist2(&self, ax: Real, ay: Real, bx: Real, by: Real) -> Real {
        let h = self.sweep(ax, ay, bx - ax, by - ay, 0.0, 0.0, 0.0);
        if h.is_some() {
            return 0.0;
        }
        // the nearest of the segment's ends to the polygon, and of its corners to the segment
        let mut best = Real::INFINITY;
        for (px, py) in [(ax, ay), (bx, by)] {
            let (qx, qy) = self.closest(px, py);
            best = math::min(best, (qx - px) * (qx - px) + (qy - py) * (qy - py));
        }
        let (ex, ey) = (bx - ax, by - ay);
        let ee = ex * ex + ey * ey;
        for (&cx, &cy) in self.poly.xs.iter().zip(&self.poly.ys) {
            let (cx, cy) = (crate::math::narrow(cx) + self.ox, crate::math::narrow(cy) + self.oy);
            let t = if ee > 0.0 { math::max(0.0, math::min(1.0, ((cx - ax) * ex + (cy - ay) * ey) / ee)) } else { 0.0 };
            let (sx, sy) = (ax + ex * t - cx, ay + ey * t - cy);
            best = math::min(best, sx * sx + sy * sy);
        }
        best
    }

    /// It as a static of the rigid bodies' step (`mvs_collision::rigid_sphere`): no kinematic move (a polygon is no
    /// marked piece), its WorldStatic mark.
    pub fn rigid(&self) -> mvs_collision::rigid_sphere::Static {
        mvs_collision::rigid_sphere::Static {
            g: self.bounds(),
            kinematic: None,
            static_solid: self.is_static,
            poly: Some(mvs_collision::rigid_sphere::PolyAt {
                poly: std::sync::Arc::clone(&self.poly),
                ox: f64::from(self.ox),
                oy: f64::from(self.oy),
            }),
        }
    }
}

/// The polygon solid `p` where it stands, as a [`PolyBlock`].
fn poly_block(w: &World, p: &StageSolidPoly) -> PolyBlock {
    let (ox, oy) = solid_off(w, p.solid);
    let is_static = solid_object_type(w, p.solid) == Some("WorldStatic");
    PolyBlock { solid: p.solid, poly: std::sync::Arc::clone(&p.poly), ox, oy, is_static }
}

/// The polygon solids that block a query of `kind` (`World::blockers`' rule for the boxes: the solid's bit for the
/// kind, its collision on), into `out` (cleared first). None for kind dynamic, whose bit the data does not write.
// Unverified: the movers that read them (the projectiles' ProjectileMovement, the items' bodies and flights) meet a
// polygon by the box readers' rules with its nearest point for the box's clamp, its upper outline for the box's top and
// a separating-axis way out for the MTD, not PhysX's convex sweep.
pub fn blocker_polys(w: &mut World, kind: BlockKind, out: &mut Vec<PolyBlock>) {
    out.clear();
    if kind_flag(kind) == 0 || w.stage_run.as_ref().is_none_or(|r| r.solids.is_empty()) {
        return;
    }
    let all = stage_solids(w);
    blocker_polys_of(w, &all, kind, out);
}

/// [`blocker_polys`] that reads the World alone (the solids as kept, or made for the call), into `out` (cleared).
pub fn blocker_polys_read(w: &World, kind: BlockKind, out: &mut Vec<PolyBlock>) {
    out.clear();
    if kind_flag(kind) == 0 || w.stage_run.as_ref().is_none_or(|r| r.solids.is_empty()) {
        return;
    }
    with_solids(w, |all| blocker_polys_of(w, all, kind, out));
}

/// The walk of both over `all`, pushed to `out`.
fn blocker_polys_of(w: &World, all: &Solids, kind: BlockKind, out: &mut Vec<PolyBlock>) {
    let bit = kind_flag(kind);
    let run = w.stage_run.as_ref().expect("the stage's run");
    for s in &all.all {
        if let Solid::Poly(p) = s
            && p.flags & bit != 0
            && p.flags & SOLID_ONE_WAY == 0
            && run.solids[p.solid as usize].enabled
        {
            out.push(poly_block(w, p));
        }
    }
}

/// The polygon solids that block the fighter's capsule (his platforms' polygons with their collision on), into `out`
/// (cleared): the polygon twin of his walking boxes (`body::floor_list`).
pub fn fighter_polys(w: &World, f: FighterId, out: &mut Vec<PolyBlock>) {
    out.clear();
    let Some(run) = w.stage_run.as_ref() else { return };
    for &pr in &w.fighter(f).platforms.platforms {
        if let PlatformRef::Poly(i) = pr
            && run.solids[i as usize].enabled
        {
            out.push(with_poly(w, i, |p| poly_block(w, p)));
        }
    }
}

/// The `StageState::solids` index of the stage solid whose box is `g` where it stands (a box of `World::blockers`
/// gives no identity: looked up by its place, the fighters' boxes first, as [`sphere_static`]).
pub fn solid_of_box(w: &World, g: &Ground) -> Option<u32> {
    let run = w.stage_run.as_ref()?;
    if run.solids.is_empty() {
        return None;
    }
    with_solids(w, |all| {
        all.all.iter().find_map(|x| match x {
            Solid::Box(_, b) if run.solids[b.solid as usize].enabled && solid_box_ground(w, b) == *g => Some(b.solid),
            _ => None,
        })
    })
}

/// `QUERY_WORLDSTATIC`: an object query for WorldStatic (it looks at the object type alone): every WorldStatic solid.
pub const QUERY_WORLDSTATIC: i32 = 0;
/// `QUERY_UNKNOWN`: a query whose channel the stage data writes no bit for: the block's profile only, another one
/// warned and left out (unverified).
pub const QUERY_UNKNOWN: i32 = -1;

/// `solidType`: the solid's object type by its profile, -1 for a Custom one (the stage data writes no object type:
/// unverified).
fn solid_type(w: &World, profile: &str) -> i32 {
    if profile == "Custom" { -1 } else { super::profile_type(w, profile) }
}

/// `metAsBlock`: whether the solid is met by a reader of the stage block whose query is `flag`: a solid of the block's
/// own profile answers every query as the block does; another profile's solid by its response to the query, the flag
/// the reader names, and only when it is of the block's object type, WorldStatic.
/// `name` names the solid in the warnings.
fn met_as_block(w: &World, flags: i32, profile: &str, name: &str, flag: i32, why: Name) -> bool {
    if flags & SOLID_ONE_WAY != 0 {
        return false;
    }
    let block = w.data.stage.floor.profile.as_deref().unwrap_or("Floor");
    if profile == block {
        return true;
    }
    let ty = solid_type(w, profile);
    let why = w.names.str(why);
    if ty < 0 {
        warn_keyed(w, format_args!("metAsBlock type {name} {why}"), || {
            format!("stage solid {name}: profile {profile} has no object type in the data: not met by {why}")
        });
        return false;
    }
    if flag == QUERY_WORLDSTATIC {
        return ty == 0;
    }
    if flag == QUERY_UNKNOWN {
        warn_keyed(w, format_args!("metAsBlock unknown {name} {why}"), || {
            format!("stage solid {name}: profile {profile}'s response to {why} is not in the data: not met")
        });
        return false;
    }
    if flags & flag == 0 {
        return false;
    }
    if ty != 0 {
        warn_keyed(w, format_args!("metAsBlock object {name} {why}"), || {
            format!("stage solid {name}: object type {ty} of profile {profile}: {why} is taken not to meet it")
        });
        return false;
    }
    true
}

/// `staticSolids`: the stage's box solids, collision on, that a reader of the stage block
/// (`why`, for the warnings) meets besides it for the query `flag` (a SOLID_* bit, [`QUERY_WORLDSTATIC`] or
/// [`QUERY_UNKNOWN`]): the run-time Floor boxes first (the block's profile on the tutorial level), their boxes into
/// `out`.
// Deviation: a run-time solid is named by its spec number in the warnings (not its class: `RuntimeSolid`
// keeps no spec here). Its profile is Floor (a Floor box).
pub fn static_solids(w: &mut World, flag: i32, why: Name, out: &mut Vec<Ground>) {
    static_solids_and_polys(w, flag, why, out, None);
}

/// [`static_solids`] that gives the polygon solids it meets to `polys` (as [`PolyBlock`]s) where the box reader
/// warns of them.
pub fn static_solids_and_polys(
    w: &mut World,
    flag: i32,
    why: Name,
    out: &mut Vec<Ground>,
    mut polys: Option<&mut Vec<PolyBlock>>,
) {
    let n = w.runtime_solids.as_ref().map_or(0, Vec::len);
    for i in 0..n {
        let r: &RuntimeSolid = &w.runtime_solids.as_ref().expect("the list")[i];
        if r.solid != crate::world::stage::RuntimeBox::Floor || !r.state.enabled {
            continue;
        }
        let (flags, g) = (r.flags, r.ground());
        let mut name = [0u8; 32];
        let mut c = std::io::Cursor::new(&mut name[..]);
        write!(c, "run-time solid {}", r.spec).expect("a short name");
        let k = usize::try_from(c.position()).expect("a buffer place");
        let name = std::str::from_utf8(&name[..k]).expect("formatted text");
        if met_as_block(w, flags, "Floor", name, flag, why) {
            out.push(g);
        }
    }
    if w.stage_run.as_ref().is_none_or(|r| r.solids.is_empty()) {
        return;
    }
    let data = std::sync::Arc::clone(&w.data);
    let sd = data.stage.solids.as_deref().unwrap_or(&[]);
    let all = stage_solids(w);
    for s in &all.all {
        let st = &w.stage_run.as_ref().expect("the stage's run").solids[s.solid() as usize];
        if !st.enabled {
            continue;
        }
        let d = &sd[usize::try_from(st.id).expect("a solid's data id")];
        if !met_as_block(w, s.flags(), &d.profile, &d.name, flag, why) {
            continue;
        }
        match s {
            Solid::Poly(p) => match polys.as_deref_mut() {
                Some(ps) => ps.push(poly_block(w, p)),
                None => warn_poly(w, p.solid),
            },
            Solid::Box(_, b) => out.push(solid_box_ground(w, b)),
        }
    }
}

/// `capsuleSweepAll`: `capsuleSweep` over a list (staticGrounds): the first contact's
/// time.
pub fn capsule_sweep_all(ax: Real, ay: Real, bx: Real, by: Real, r: Real, hh: Real, gs: &[Ground]) -> Option<Real> {
    mvs_collision::solids::capsule_sweep_all(ax, ay, bx, by, r, hh, gs).map(crate::math::narrow)
}

/// `withStatics`: a list of boxes with the [`static_solids`] ahead of it (the list itself
/// on a stage without them).
pub fn with_statics(w: &mut World, flag: i32, why: Name, list: &[Ground]) -> Vec<Ground> {
    let mut out = Vec::new();
    static_solids(w, flag, why, &mut out);
    out.extend_from_slice(list);
    out
}

/// `staticGrounds`: the stage block and [`static_solids`]: the list a reader of the stage
/// block alone walks (the block first).
pub fn static_grounds(w: &mut World, flag: i32, why: Name) -> Vec<Ground> {
    let mut out = vec![w.stage.ground];
    static_solids(w, flag, why, &mut out);
    out
}

/// `section`: the solid's section by X = 0 in sim space, `None` where it does not reach
/// the plane.
pub fn section(d: &StageSolidData) -> Option<Section> {
    mvs_collision::solids::section(&crate::world::stage::shape_of(d))
}

// ---- the polygon solids

/// The world's solids as kept (`Scratch::solids`), made here when the World holds none yet (a state loaded from a
/// frame).
fn with_solids<R>(w: &World, f: impl FnOnce(&Solids) -> R) -> R {
    match &w.scratch.solids {
        Some(s) => f(s),
        None => f(&solids_of(w)),
    }
}

/// The polygon of the solid `i` (a `PlatformRef::Poly`), handed to `f`.
pub fn with_poly<R>(w: &World, i: u32, f: impl FnOnce(&StageSolidPoly) -> R) -> R {
    with_solids(w, |s| {
        let p = s
            .all
            .iter()
            .find_map(|x| match x {
                Solid::Poly(p) if p.solid == i => Some(p),
                _ => None,
            })
            .unwrap_or_else(|| panic!("stage solid {i}: no polygon section"));
        f(p)
    })
}

/// A polygon's highest and lowest corner at its data place (`t0`, `b0`).
pub fn poly_extent(w: &World, i: u32) -> (Real, Real) {
    with_poly(w, i, |p| (p.t0, p.b0))
}

/// A polygon's `StageSolidData.flags`.
pub fn poly_flags(w: &World, i: u32) -> i32 {
    with_poly(w, i, |p| p.flags)
}

/// A stage solid's `objectType` (`POLY_TYPE`, `SolidBox.objectType`: its data's `object_type`), by its
/// `StageState::solids` index.
pub fn solid_object_type(w: &World, i: u32) -> Option<&str> {
    let run = w.stage_run.as_ref()?;
    let id = usize::try_from(run.solids[i as usize].id).ok()?;
    w.data.stage.solids.as_ref()?.get(id)?.object_type.as_deref()
}

/// The solid index of a polygon's `PlatformRef`.
fn poly_index(p: PlatformRef) -> u32 {
    match p {
        PlatformRef::Poly(i) => i,
        _ => panic!("not a stage solid's polygon: {p:?}"),
    }
}

/// The move (dx, dy) of the solid `i` from its data place.
fn solid_off(w: &World, i: u32) -> (Real, Real) {
    let m = &w.stage_run.as_ref().expect("a stage solid's polygon with the stage's run").solids[i as usize].m;
    (m.dx, m.dy)
}

/// `polyFloor` of the polygon `p`: whether the flat base of half width r at x finds a
/// walkable floor, and its height (-Infinity: the base misses it).
fn floor_on(w: &World, p: PlatformRef, x: Real, r: Real) -> (bool, Real) {
    let i = poly_index(p);
    let (ox, oy) = solid_off(w, i);
    let (ok, y) = with_poly(w, i, |q| poly_floor(&q.poly, ox, oy, x, r));
    (ok, crate::math::narrow(y))
}

/// `StageSolidPoly.heightAt`: the flat-base floor's height at x, the polygon's top where
/// the base misses it.
pub fn poly_height_at(w: &World, p: PlatformRef, x: Real, r: Real) -> Real {
    let (_, h) = floor_on(w, p, x, r);
    if h == Real::NEG_INFINITY {
        let i = poly_index(p);
        poly_extent(w, i).0 + solid_off(w, i).1
    } else {
        h
    }
}

/// `StageSolidPoly.walkableAt`.
pub fn poly_walkable_at(w: &World, p: PlatformRef, x: Real, r: Real) -> bool {
    floor_on(w, p, x, r).0
}

/// `polyOut` of the solid `i` where it is now: the depth, -1 for none; `last` keeps the
/// latest answer that found an overlap (`OUT_N` / `OUT_C`, which only such a call writes).
fn out_of(w: &World, i: u32, at: (Real, Real), r: Real, hs: Real, last: &mut Out) -> Real {
    let (ox, oy) = solid_off(w, i);
    match with_poly(w, i, |q| poly_out(&q.poly, ox, oy, at.0, at.1, r, hs)) {
        Some(o) => {
            *last = o;
            crate::math::narrow(o.depth)
        }
        None => -1.0,
    }
}

/// `EPS`.
const EPS: Real = 1e-3;

/// `outOfPolys`: the move out of the polygons of the fighter's platforms after his move
/// (the physics collide, behind the boxes), as the movement takes them: a capsule already inside at the move's start
/// is resolved as SafeMoveUpdatedComponent resolves a penetration, along the least way out (the MTD) by its depth +
/// 0.125, at most MaxDepenetrationWithGeometry (500); one its move took in stops where the move's sweep meets the
/// face (the last free place along the move, by bisection). The velocity into the face is dropped along the slide
/// normal: the face's, or for a walker whose contact lies above his lower hemisphere's centre its horizontal part
/// (SlideAlongSurface's 2D normal on the ground). A walker is not put under his floor (Unverified: the floor's own
/// depenetration is not modelled). In hitstun the bounce test on the face's normal.
/// A fall's contact lands where PhysFalling's IsValidLandingSpot 142933610 takes it at a corner: a walkable
/// ImpactNormal (Z >= 0.71: under the lower hemisphere's centre and within the edge tolerance with it), then
/// FindFloor's flat base walkable there; ProcessLanded with the polygon as the base, stood on its flat-base floor.
/// A walker's contact with a walkable normal goes on up the face (MoveAlongFloor's ramp branch). Else it goes to
/// StepUp 14294d210 where the component lets him (CanCharacterStepUpOn, SOLID_STEP), the move did not start inside
/// it, and the impact lies over his floor and not over his lower hemisphere's centre; the step holds where the
/// flat-base floor at the move's end is walkable, at most MaxStepHeight (100) over his floor, with the capsule free of
/// the polygon there: he stands on it (his base).
// Deviation: a landing ends the frame's move there (no walk for the substep's rest, as the box landings of fallMove
// have); the start-penetrating landing takes the MTD's normal with the same walkable test (the build's is Normal.Z at
// least 1e-4, then FindFloor); StepUp's lift, forward and down sweeps are one test of the place at the move's end (the
// down sweep's hit is the flat-base floor's, not the capsule's).
// Deviation: the contact's normal is the polygon's way out at the end of the frame's move or at the bisected
// contact, not a swept hit's ImpactNormal; a capsule already inside is a hit too (the resolve's).
fn out_of_polys(w: &mut World, f: FighterId, hitstun: bool, walking: bool) -> bool {
    use crate::combat::hit::launch::try_bounce;
    use crate::fighter::body::{cap_half, cap_radius, floor_top, rest_gap, stand_y_of};
    use crate::fighter::moves::{bounce_free, land};
    use crate::world::stage::stage_solid_hit;
    let r = cap_radius(w, f);
    let hs = cap_half(w, f) - r;
    let mut k = 0;
    while k < w.fighter(f).platforms.platforms.len() {
        let pr = w.fighter(f).platforms.platforms[k];
        k += 1;
        let PlatformRef::Poly(i) = pr else { continue };
        if !w.stage_run.as_ref().expect("the stage's run").solids[i as usize].enabled {
            continue;
        }
        let mut last = Out { depth: 0.0, nx: 0.0, ny: 0.0, cy: 0.0 };
        let (fx, fy) = (w.fighter(f).body.x, w.fighter(f).body.y);
        let depth = out_of(w, i, (fx, fy), r, hs, &mut last);
        if depth < 0.0 {
            continue;
        }
        let (mut nx, mut ny, mut cy) = (last.nx, last.ny, last.cy);
        let xe = fx; // the move's end
        let (x0, y0) = (w.fighter(f).body.prev_x, w.fighter(f).body.prev_y);
        let started = out_of(w, i, (x0, y0), r, hs, &mut last) >= 0.0;
        if started {
            // inside at the move's start: the resolve along the MTD
            let k = crate::math::min(depth + 0.125, MAX_DEPENETRATION);
            let b = &mut w.fighter_mut(f).body;
            b.x += crate::math::narrow(nx * f64::from(k));
            b.y += crate::math::narrow(ny * f64::from(k));
            if walking {
                let sy = stand_y_of(w, f);
                let b = &mut w.fighter_mut(f).body;
                b.y = crate::math::max(b.y, sy);
            }
        } else {
            // the move took it in: its sweep stops at the face, the last free place along the move (bisection)
            let (dx, dy) = (fx - x0, fy - y0);
            let (mut lo, mut hi): (Real, Real) = (0.0, 1.0);
            for _ in 0..24 {
                let t = (lo + hi) / 2.0;
                if out_of(w, i, (x0 + dx * t, y0 + dy * t), r, hs, &mut last) >= 0.0 {
                    hi = t;
                } else {
                    lo = t;
                }
            }
            out_of(w, i, (x0 + dx * hi, y0 + dy * hi), r, hs, &mut last); // the face's normal at the contact
            nx = last.nx;
            ny = last.ny;
            cy = last.cy;
            let b = &mut w.fighter_mut(f).body;
            b.x = x0 + dx * lo;
            b.y = y0 + dy * lo;
        }
        let x = w.fighter(f).body.x;
        if !walking
            && ny >= WALKABLE_Z
            && nx.abs() * f64::from(r) <= f64::from(r - 0.15)
            && poly_walkable_at(w, pr, x, r)
        {
            // the landing (as collidePlatforms on a box's top)
            w.fighter_mut(f).platforms.base = Some(pr);
            let sy = stand_y_of(w, f);
            w.fighter_mut(f).body.y = sy;
            if hitstun && try_bounce(w, f, crate::math::narrow(nx), crate::math::narrow(ny), true, 1.0) {
                w.fighter_mut(f).platforms.base = None;
                stage_solid_hit(w, f, Some(i), crate::math::narrow(nx), crate::math::narrow(ny));
                return false;
            }
            if !hitstun
                && !(bounce_free(w, f) && try_bounce(w, f, crate::math::narrow(nx), crate::math::narrow(ny), true, 1.0))
            {
                land(w, f);
            }
            stage_solid_hit(w, f, Some(i), crate::math::narrow(nx), crate::math::narrow(ny)); // the landing's hit, after Landed
            return true;
        }
        if walking && !started && ny >= WALKABLE_Z && poly_walkable_at(w, pr, xe, r) {
            // MoveAlongFloor's walkable-ramp branch (a hit with Time > 0 and a walkable normal): the move goes on along
            // the face, no CanStepUp; the floor check after it finds the polygon.
            // Deviation: the move along the ramp is the place at the move's end on its flat-base floor, as the step
            // up's.
            let h = poly_height_at(w, pr, xe, r);
            let sy = h + cap_half(w, f) + rest_gap(w, f);
            if h > floor_top(w, f) - EPS && out_of(w, i, (xe, sy), r, hs, &mut last) < 0.0 {
                let fi = w.fighter_mut(f);
                fi.body.x = xe;
                fi.platforms.base = Some(pr);
                fi.body.y = sy;
                stage_solid_hit(w, f, Some(i), crate::math::narrow(nx), crate::math::narrow(ny));
                continue;
            }
        }
        if walking && !started && poly_flags(w, i) & SOLID_STEP != 0 && cy <= f64::from(w.fighter(f).body.y - hs) {
            let floor = floor_top(w, f);
            if cy > f64::from(floor) && poly_walkable_at(w, pr, xe, r) {
                let h = poly_height_at(w, pr, xe, r);
                let sy = h + cap_half(w, f) + rest_gap(w, f);
                if h > floor - EPS && h - floor <= MAX_STEP_HEIGHT && out_of(w, i, (xe, sy), r, hs, &mut last) < 0.0 {
                    // the step up: on its top at the move's end
                    let fi = w.fighter_mut(f);
                    fi.body.x = xe;
                    fi.platforms.base = Some(pr);
                    fi.body.y = sy;
                    // the move's side hit (OnComponentHit comes with the sweep, StepUp or not)
                    stage_solid_hit(w, f, Some(i), crate::math::narrow(nx), crate::math::narrow(ny));
                    continue;
                }
            }
        }
        let bounced = !walking
            && (hitstun || bounce_free(w, f))
            && try_bounce(w, f, crate::math::narrow(nx), crate::math::narrow(ny), false, 1.0);
        stage_solid_hit(w, f, Some(i), crate::math::narrow(nx), crate::math::narrow(ny)); // the solid's OnComponentHit
        if bounced {
            return false;
        }
        let (mut sx, mut sy) = (nx, ny);
        if walking && cy > f64::from(w.fighter(f).body.y - hs) {
            sx = if nx > 0.0 {
                1.0
            } else if nx < 0.0 {
                -1.0
            } else {
                0.0
            };
            sy = 0.0;
        }
        let b = &mut w.fighter_mut(f).body;
        let vn = b.vx * crate::math::narrow(sx) + b.vy * crate::math::narrow(sy);
        if vn < 0.0 {
            b.vx -= vn * crate::math::narrow(sx);
            if !walking {
                b.vy -= vn * crate::math::narrow(sy);
            }
        }
    }
    false
}

/// `collidePolys`: a fighter in the air; true on a landing.
pub fn collide_polys(w: &mut World, f: FighterId, hitstun: bool) -> bool {
    out_of_polys(w, f, hitstun, false)
}

/// `walkOutOfPolys`: a walker.
pub fn walk_out_of_polys(w: &mut World, f: FighterId) {
    out_of_polys(w, f, false, true);
}

// ---- edges

/// `holderOf`: where an owner keeps its place: a SpawnedPlatformParent child's SpawnedPlatform, a ship itself;
/// `None` for an actor of another class. `Some(edge_at)`. StevenCollisionBlock keeps its box in a `plat` here (a plain
/// PlatformBox in `solids` would have no `plat`: no holder).
fn holder_edge_at(w: &World, o: ActorRef) -> Option<Option<(Real, Real)>> {
    match o {
        ActorRef::Ship(id) => w.actors.ships.get(id).map(|s| s.edge_at),
        ActorRef::Actor(id)
            if w.actors
                .actors
                .get(id)
                .is_some_and(|a| matches!(a.state, crate::actors::world_actor::WorldActorState::StevenBlock(_))) =>
        {
            None
        }
        ActorRef::Actor(id) => crate::world::spawned_platform::spawned_edge_at(w, id),
        _ => None,
    }
}

/// `edgeBeginPlay`: Comp_EdgeTrace's BeginPlay on a spawned owner (a SpawnedPlatformParent
/// child after its parent's part, a ship): its place, where the level has a generator (`edgeSet`: the stage's edge
/// data).
pub fn edge_begin_play(w: &mut World, o: ActorRef) {
    if holder_edge_at(w, o).is_none() || w.data.stage.edges.is_none() {
        return;
    }
    let Some((x, y, _)) = crate::actors::world_actor::actor_place(w, o) else { return };
    match o {
        ActorRef::Ship(id) => {
            if let Some(s) = w.actors.ships.get_mut(id) {
                s.edge_at = Some((x, y));
            }
        }
        ActorRef::Actor(id) => {
            if let Some(pl) = crate::world::spawned_platform::plat_mut(w, id) {
                pl.edge_at = Some((x, y));
            }
        }
        _ => {}
    }
}

/// `spawnedEdgesEnd`: its Comp_EdgeTrace.On Destroyed with the actor's end: On Edges
/// Removed to the riders bound to its polygon (`edges::spawned_removed`).
pub fn spawned_edges_end(w: &mut World, o: ActorRef) {
    if !matches!(holder_edge_at(w, o), Some(Some(_))) {
        return;
    }
    let seq = match o {
        ActorRef::Ship(id) => w.actors.ships.get(id).map(|s| s.seq),
        ActorRef::Actor(id) => w.actors.actors.get(id).map(|a| a.seq),
        _ => None,
    };
    if let Some(seq) = seq {
        crate::fighter::traces::edges::spawned_removed(w, seq);
    }
}
