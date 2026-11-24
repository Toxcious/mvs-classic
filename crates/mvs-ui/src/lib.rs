//! The UI: a Slate subset over the ui kind's widget trees (instantiate, layout, widget animations,
//! paint to an element list), its text (FreeType faces from the font kind, the Game target's strings), the element
//! batches drawn on Direct3D 11 with the build's Slate shaders and UI materials, the match HUD and the results
//! screen. The UI draws over the renderer's picture: the caller draws the element list on the back buffer after
//! `Renderer::draw`.

pub mod bindings;
pub mod bundle;
pub mod canvas;
pub mod element;
pub mod hud;
pub mod key_names;
pub mod launcher;
pub mod menu;
pub mod results;
pub mod slate;
pub mod tape;
pub mod text;

pub use bundle::Bundle;
pub use element::{ElementList, Ui};
pub use text::{Fonts, GameText};

#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// An entry that does not read as its format says, or holds what the UI cannot draw.
    #[error("{what}: {why}")]
    Format { what: String, why: String },
    #[error(transparent)]
    Store(#[from] mvs_store::StoreError),
    #[error(transparent)]
    Render(#[from] mvs_render::Error),
}

pub type Result<T> = std::result::Result<T, Error>;
