//! Custom stages, the port's own: a layout file over a base stage of the build (its scenery with pieces hidden,
//! moved and added, and play shapes of its own), and the map's stage data composed from the base's and the layout.
//! No window, renderer or store: the game and the editor compose with the same code. The scenery's side of the
//! layout is applied where the scenery is read (`mvs-render`'s `scenery/layout.rs`).

mod compose;
mod layout;

pub use compose::{
    BLAST_BOX, ComposedStage, MAX_PLATFORMS, MeshBounds, PARK_DEPTH, PARK_HALF, START_STYLES, WorldBox, apply, box_of,
    check, compose_stage, piece_box,
};
pub use layout::{BoxSpec, FloorSpec, ItemArea, Layout, Light, Piece, PlatformSpec};

/// The sim's name of every custom stage starts so (`Custom_<key>`): the sim enters each with an empty stage entry.
pub const CUSTOM_PREFIX: &str = "Custom_";

/// Why a layout is refused.
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum LayoutError {
    #[error("the layout's JSON: {0}")]
    Json(String),
    #[error("{0} platforms: a stage holds at most 32 (a fighter's platforms to ignore are one 32-bit mask)")]
    TooManyPlatforms(usize),
    #[error("no start {tag} tagged {style}")]
    NoStart { tag: String, style: &'static str },
    #[error("no blast box CollisionComp")]
    NoBlastBox,
    #[error("the piece {piece} is baked but comes from the stage {stage}: a borrowed piece is movable")]
    BakedFromOtherStage { piece: String, stage: String },
    #[error("hide {instance}: the kept block {block} names it")]
    HiddenInBlock { instance: String, block: String },
    #[error("{0}")]
    Invalid(String),
}

#[cfg(test)]
mod tests;
