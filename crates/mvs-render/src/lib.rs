//! The renderer on Direct3D 11: the build's cooked vertex and pixel shaders created unchanged from the store and fed
//! the build's own inputs (the View, Primitive and lightmap rows, the vertex factories, the material uniform
//! buffers); the stage's scenery, deferred lighting, translucency, post, the fighters' and the particles' draws.

/// A body not written yet: panics naming who writes it; its arguments count as used.
#[allow(unused_macros)]
macro_rules! stub {
    ($what:literal $(, $arg:expr)* $(,)?) => {{
        $(let _ = &$arg;)*
        unimplemented!($what)
    }};
}

pub mod assets;
pub mod capture;
pub mod cmesh;
pub mod decal;
pub mod draw;
pub mod fighters;
pub mod fx;
pub mod gpu;
pub mod hole;
pub mod image;
pub mod ism;
pub mod landscape;
pub mod lava;
pub mod lighting;
pub mod look;
pub mod material;
pub mod menuscene;
pub mod particles;
pub mod port_icons;
pub mod post;
pub mod renderer;
pub mod scene;
pub mod scenery;
mod shaders;
pub mod stage;
pub mod stagedraw;
pub mod swap;
pub mod targets;
mod texture_quality;
pub mod translucency;
pub mod view;
pub mod warm;
pub mod wind;

pub use assets::Assets;
pub use gpu::Gpu;
pub use renderer::{Frame, FrameTimes, Passes, Renderer, Scalability, TrimmedAll};
pub use swap::SwapChain;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// A D3D11 or DXGI call that failed.
    #[error("{what}: {source}")]
    Device { what: String, source: mvs_d3d::core::Error },
    /// An entry that does not read as its format says, or holds what the renderer cannot draw.
    #[error("{what}: {why}")]
    Format { what: String, why: String },
    #[error(transparent)]
    Store(#[from] mvs_store::StoreError),
    #[error(transparent)]
    Material(#[from] mvs_material::Error),
    #[error(transparent)]
    View(#[from] mvs_view::Error),
    #[error(transparent)]
    Vfx(#[from] mvs_vfx::Error),
}

pub type Result<T> = std::result::Result<T, Error>;
