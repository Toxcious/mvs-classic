//! The sim's geometry queries, in the build's float32: the upright capsule against a box
//! (`capsule`), the sphere sweep of the bots' traces (`sweep`), the spawned capsule against a fighter
//! (`capsule_overlap`), the stage solids' sections and their polygon queries (`solids`), the shape traces' sweep of
//! one body (`traces`), the hit / hurt overlaps (`boxes`), the simulating bodies' physics steps (`rigid_sphere`), the
//! edge traces' polygon queries and their rotator math (`edges`, `rotator`).
//!
//! Each module ports the queries only: what a caller in the sim does with the answer (a landing, a bounce, a hit) and
//! the gathers that read the world (the traces' gather, the edges' elements) stay in the sim; their results come in
//! as arguments.
//!
//! A scalar argument of a query is `impl Into<f64>` and the shapes' fields (`Ground`, `Body`, `Sphere`, ...) and the
//! results are f64: the sim's float32 widens exactly, the double data (the stage floor, the solver's settings) narrows
//! once at the query's entry, and every query computes in float32 and widens its float32 result.
//!
//! The PhysX stand-ins (every module but `edges` and `rotator`, which port Blueprint nodes) run as the build's PhysX
//! entries do, with FTZ + DAZ (MXCSR 0x9FC0): a denormal argument reads as zero (`arg`) and a denormal result is
//! flushed (`out`, `ftz`).

// `!(a < b)` is the sim's own test, true for a NaN: partial_cmp would hide that.
#![allow(clippy::neg_cmp_op_on_partial_ord)]
// A clamp is written out as a ternary, comparison for comparison (the trial keeps f64::clamp out).
#![allow(clippy::manual_clamp)]

pub mod boxes;
pub mod capsule;
pub mod capsule_mtd;
pub mod capsule_overlap;
pub mod edges;
pub mod rigid_sphere;
pub mod rotator;
pub mod solids;
pub mod sweep;
pub mod traces;

/// A box of the play plane (sim x = -UE Y, y = UE Z): the fighter's Ground and the rigid sphere's SphereBox.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Ground {
    pub left: f64,
    pub right: f64,
    pub bottom: f64,
    pub top: f64,
}

/// A `Ground` narrowed to float32 at a query's entry.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(crate) struct Box2 {
    pub left: f32,
    pub right: f32,
    pub bottom: f32,
    pub top: f32,
}

impl Ground {
    pub(crate) fn narrow(&self) -> Box2 {
        Box2 { left: arg(self.left), right: arg(self.right), bottom: arg(self.bottom), top: arg(self.top) }
    }
}

/// FTZ: a denormal float32 is zero, its sign kept (the PhysX entries' MXCSR 0x9FC0).
// Unverified: the flush is applied to the queries' arguments, results and the solver's stored rows, not to every
// intermediate operation: a denormal intermediate needs operands below 1e-19 and none of the queries' inputs is.
#[inline]
pub(crate) fn ftz(x: f32) -> f32 {
    if x.is_subnormal() { 0.0f32.copysign(x) } else { x }
}

/// A query's argument narrowed to float32 (exact for the sim's float32, one rounding for double data), a denormal
/// read as zero (DAZ).
#[inline]
#[allow(clippy::cast_possible_truncation, reason = "the narrowing to the build's float32 is the point")]
pub(crate) fn arg(x: impl Into<f64>) -> f32 {
    ftz(x.into() as f32)
}

/// A float32 result widened for the f64 interface, a denormal flushed (FTZ).
#[inline]
pub(crate) fn out(x: f32) -> f64 {
    f64::from(ftz(x))
}

/// Math.min(a, b) on float32 (NaN wins, -0 below +0): JavaScript's rule, exact through the widening.
#[inline]
#[allow(clippy::cast_possible_truncation, reason = "the result is one of the float32 arguments")]
pub(crate) fn min(a: f32, b: f32) -> f32 {
    mvs_math::js::min(f64::from(a), f64::from(b)) as f32
}

/// Math.max(a, b) on float32.
#[inline]
#[allow(clippy::cast_possible_truncation, reason = "the result is one of the float32 arguments")]
pub(crate) fn max(a: f32, b: f32) -> f32 {
    mvs_math::js::max(f64::from(a), f64::from(b)) as f32
}

/// Math.max(a, b, c).
#[inline]
pub(crate) fn max3(a: f32, b: f32, c: f32) -> f32 {
    max(max(a, b), c)
}
