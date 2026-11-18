//! The clip decoder and pose sampler (with the store's entry formats), the sim's skeleton pose and the
//! montage player, giving the sim's own bits. The montage player runs over the bound data's
//! montage records (`mvs_data::fighter::Montage`) and a state its owner keeps; the skeleton reads the data's box index
//! (`mvs_data::boxes`). Its unit tests are its own; the sim's per-tick trace checks it at integration.
//!
//! A weight or a time the sim passes is `impl Into<f64>` and a result is an `f64`: the skeleton's kernels and the
//! montage player compute in float32 as the build does (the arguments narrow, exact for the sim's float32; a result
//! is a widened float32). The montage state keeps its position widened; each step narrows it, so a section or segment
//! time of the data enters as the float the asset holds. The view's own clip sampler (`clip`) and
//! [`montage::alpha_to_blend_option`] stay in double.

// The port keeps the sim's shape: `!(a > b)` is true for a NaN where `a <= b` is not; a clamp written out keeps -0
// where f64::clamp keeps the sign of its input; index loops and long argument lists follow the sim's functions.
#![allow(
    clippy::neg_cmp_op_on_partial_ord,
    clippy::manual_clamp,
    clippy::needless_range_loop,
    clippy::too_many_arguments
)]

pub mod blend_space;
pub mod clip;
pub mod curve;
pub mod format;
pub mod montage;
pub mod pose;
pub mod skel_pose;
