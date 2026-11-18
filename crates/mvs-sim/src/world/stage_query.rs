//! The stage's shapes a query meets, boxes and polygons alike: `World::blockers`' boxes with the polygon stage solids
//! beside them ([`PolyBlock`], `fighter::traces::solids::blocker_polys`), and the stage block with the solids a reader
//! of the block meets (`static_grounds` with their polygon twins). A reader takes a [`StageShapes`] (or its borrowed
//! [`Shapes`]) where it took a list of boxes, and asks each shape the query it asked a box: a box keeps the reader's
//! own box rule, a polygon answers by [`PolyBlock`]'s (`closest` for the box's clamp, `top_at` for its top, `inside`,
//! `mtd`, and `sweep` / `capsule_sweep` / `seg_dist2`, the shape traces' convex sweep, for a sweep, an overlap or a ray;
//! `rigid` for a rigid body's static).
//! A stage with no polygon solid gives no polygon: every reader answers there as it did on boxes alone.
// Unverified: a polygon is met by the box readers' rules over its nearest point, its upper outline and the shape
// traces' convex sweep, not PhysX's convex queries.

use crate::Real;
use crate::fighter::traces::solids::{blocker_polys, blocker_polys_read, sphere_static, static_solids_and_polys};
use crate::names::Name;
use crate::world::World;
use crate::world::spawned_platform::BlockKind;
use mvs_collision::Ground;

pub use crate::fighter::traces::solids::PolyBlock;

/// The boxes and the polygon solids a query meets.
#[derive(Clone, Debug, Default)]
pub struct StageShapes {
    pub boxes: Vec<Ground>,
    pub polys: Vec<PolyBlock>,
}

impl StageShapes {
    /// The borrowed view.
    pub fn view(&self) -> Shapes<'_> {
        Shapes { boxes: &self.boxes, polys: &self.polys }
    }

    /// Boxes alone (a list with no polygon twin).
    pub fn of_boxes(boxes: Vec<Ground>) -> StageShapes {
        StageShapes { boxes, polys: Vec::new() }
    }

    /// Its shapes in order, the boxes first.
    pub fn iter(&self) -> impl Iterator<Item = ShapeRef<'_>> {
        self.view().iter()
    }
}

/// The boxes and the polygon solids a move meets besides the stage block, borrowed.
#[derive(Clone, Copy, Debug, Default)]
pub struct Shapes<'a> {
    pub boxes: &'a [Ground],
    pub polys: &'a [PolyBlock],
}

impl<'a> Shapes<'a> {
    /// None.
    pub const NONE: Shapes<'static> = Shapes { boxes: &[], polys: &[] };

    /// The boxes alone (a list of `World::blockers` that has no polygon twin).
    pub fn boxes(boxes: &[Ground]) -> Shapes<'_> {
        Shapes { boxes, polys: &[] }
    }

    /// Its shapes in order, the boxes first.
    pub fn iter(self) -> impl Iterator<Item = ShapeRef<'a>> {
        self.boxes.iter().map(ShapeRef::Box).chain(self.polys.iter().map(ShapeRef::Poly))
    }

    /// `capsuleSweepAll`: the upright capsule (radius r, half height hh) whose centre moves a -> b against every
    /// shape (a box by `capsuleSweep`, a polygon by [`PolyBlock::capsule_sweep`]): the first contact's time.
    #[allow(clippy::too_many_arguments, reason = "a sweep and its shape")]
    pub fn capsule_sweep_all(self, ax: Real, ay: Real, bx: Real, by: Real, r: Real, hh: Real) -> Option<Real> {
        let mut best = crate::fighter::traces::capsule_sweep_all(ax, ay, bx, by, r, hh, self.boxes);
        for q in self.polys {
            if let Some(t) = q.capsule_sweep(ax, ay, bx, by, r, hh)
                && best.is_none_or(|b| t < b)
            {
                best = Some(t);
            }
        }
        best
    }
}

/// One shape of a list.
#[derive(Clone, Copy, Debug)]
pub enum ShapeRef<'a> {
    Box(&'a Ground),
    Poly(&'a PolyBlock),
}

/// A shape found: a box (the stage block or one of the list, by its place) or the polygon `polys[i]`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Met {
    Box(Ground),
    Poly(usize),
}

/// A shape held after the query (a box by its place, a polygon whole).
#[derive(Clone, Debug)]
pub enum StageShape {
    Box(Ground),
    Poly(PolyBlock),
}

impl StageShape {
    /// The point of it nearest (x, y) ((x, y) inside it): a box's clamp.
    pub fn closest(&self, x: Real, y: Real) -> (Real, Real) {
        match self {
            StageShape::Box(g) => (
                crate::math::max(crate::math::narrow(g.left), crate::math::min(x, crate::math::narrow(g.right))),
                crate::math::max(crate::math::narrow(g.bottom), crate::math::min(y, crate::math::narrow(g.top))),
            ),
            StageShape::Poly(p) => p.closest(x, y),
        }
    }

    /// The box round it (a box itself).
    pub fn bounds(&self) -> Ground {
        match self {
            StageShape::Box(g) => *g,
            StageShape::Poly(p) => p.bounds(),
        }
    }
}

impl World {
    /// [`World::blockers`] with the polygon solids that block a query of `kind` ([`blocker_polys`]).
    pub fn blocker_shapes(&mut self, kind: BlockKind) -> StageShapes {
        let mut out = StageShapes::default();
        self.blocker_shapes_into(kind, &mut out);
        out
    }

    /// [`World::blocker_shapes`] into `out` (cleared first).
    pub fn blocker_shapes_into(&mut self, kind: BlockKind, out: &mut StageShapes) {
        self.blockers_into(kind, &mut out.boxes);
        blocker_polys(self, kind, &mut out.polys);
    }

    /// [`World::blocker_shapes`] that reads the World alone ([`World::blockers_read`], [`blocker_polys_read`]): the
    /// view's.
    pub fn blocker_shapes_read(&self, kind: BlockKind) -> StageShapes {
        let mut out = StageShapes { boxes: self.blockers_read(kind), polys: Vec::new() };
        blocker_polys_read(self, kind, &mut out.polys);
        out
    }
}

/// `staticGrounds` with its polygons: the stage block and the stage's solids a reader of the block meets for the query
/// `flag` (`static_solids`' rule), the block first.
pub fn static_shapes(w: &mut World, flag: i32, why: Name) -> StageShapes {
    let mut out = StageShapes { boxes: vec![w.stage.ground], polys: Vec::new() };
    static_solids_and_polys(w, flag, why, &mut out.boxes, Some(&mut out.polys));
    out
}

/// `withStatics` with its polygons: the solids a reader of the block meets ahead of `list`.
pub fn with_static_shapes(w: &mut World, flag: i32, why: Name, list: &[Ground]) -> StageShapes {
    let mut out = StageShapes::default();
    static_solid_shapes(w, flag, why, &mut out);
    out.boxes.extend_from_slice(list);
    out
}

/// The shapes as the rigid bodies' statics (`mvs_collision::rigid_sphere`): each box by `sphere_static` (its marks
/// looked up by its place), then each polygon ([`PolyBlock::rigid`]).
pub fn rigid_statics(w: &World, s: &StageShapes) -> Vec<mvs_collision::rigid_sphere::Static> {
    let mut out: Vec<_> = s.boxes.iter().map(|&g| sphere_static(w, g)).collect();
    out.extend(s.polys.iter().map(PolyBlock::rigid));
    out
}

/// `static_solids` with its polygons, added to `out`.
pub fn static_solid_shapes(w: &mut World, flag: i32, why: Name, out: &mut StageShapes) {
    static_solids_and_polys(w, flag, why, &mut out.boxes, Some(&mut out.polys));
}
