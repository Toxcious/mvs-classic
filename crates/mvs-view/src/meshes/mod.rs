//! Meshes the runtime builds itself each frame, GPU-free: a cable component's tube and spline mesh components'
//! slices. Their geometry is in UE world space (cm); the renderer
//! draws it with the component's `#local` / `#spline` material.

pub mod cable;
pub mod spline;

pub use cable::{Cable, CableDef};
pub use spline::{AlphaRule, SplineMesh, SplineSlice};
