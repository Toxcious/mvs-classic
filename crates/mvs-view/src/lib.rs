//! What the renderer draws a tick with, GPU-free: the frame input (the camera, each drawn mesh's pose and
//! materials), the views' posing from the `World`, the match camera, and the pose tape reader.

// (the NaN-aware tests kept as written: `!(a > b)` is not `a <= b`)
#![allow(clippy::neg_cmp_op_on_partial_ord)]

/// A body not written yet: panics naming who writes it; its arguments count as used.
#[allow(unused_macros)]
macro_rules! stub {
    ($what:literal $(, $arg:expr)* $(,)?) => {{
        $(let _ = &$arg;)*
        unimplemented!($what)
    }};
}

pub mod actor_anim;
pub mod actor_view;
pub mod actors;
pub mod anim;
pub mod anim_nodes;
pub mod blend;
pub mod camera;
mod chars;
pub mod court_ball;
pub mod curves;
pub mod frame;
pub mod free_cam;
pub mod ghost;
pub mod heart_skin;
mod items;
pub mod layered;
pub mod math;
pub mod mesh;
pub mod meshes;
mod montage_mesh;
pub mod pose;
pub mod preview;
pub mod rotator;
mod second;
mod shared;
pub mod skin_intro;
pub mod skins;
/// The Iron Giant's missile turn, his timelines' cubic and his turret's bolt ring, which his effects read (mvs-render
/// fx/chars/iron_giant.rs).
pub use chars::iron_giant::{
    bolt_at, bolt_count, cubic as giant_cubic, expand as bolt_expand, missile_launched, missile_roll,
};
pub mod tape;
mod tutorial_meeseeks;
pub mod view;
pub mod virtual_bone;
pub mod wind_cam;

pub use camera::MatchCamera;
pub use chars::bugs::{at_on as rocket_at_on, rocket_squash_scale};
/// Marvin's cannon blast growth and his UFO's ship place, aim turn and beam socket, which his effects ride (mvs-render
/// fx/chars/marvin.rs).
pub use chars::marvin::{
    MOUNT as MARVIN_UFO_MOUNT, aim_yaw as marvin_aim_yaw, growth as marvin_growth, ship_place as marvin_ship_place,
};
pub use frame::{
    ActorPose, Camera, FighterPose, FrameInput, FrameTime, M4, MaterialRef, MeshPose, SceneryPlace, SecondKind,
    SecondRef, WarmMesh,
};
pub use items::ItemAssets;
pub use items::class_view::particle_components as item_particle_components;
pub use pose::{HudPoint, ViewPoser};
pub use preview::{PreviewAt, PreviewView};

/// What Stripe's effects (mvs-render) read of his actors' views.
pub mod stripe {
    pub use crate::chars::stripe::{
        BoomPlace, SawCall, SawToggle, ally_saw_place, bomb_drawn, boom_place, boom_place_of, saw_of,
    };
}
/// What Rick's effects (mvs-render) read of his actors' views.
pub mod rick {
    pub use crate::chars::rick::{PortalAt, PortalLayout, bomb_frame, portal_at, portal_layout, portal_wpo};
}
pub use tape::{PoseTape, TapeFrame};

#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// An entry or a tape that does not read as its format says.
    #[error("{what}: {why}")]
    Format { what: String, why: String },
    #[error(transparent)]
    Store(#[from] mvs_store::StoreError),
    #[error(transparent)]
    Io(#[from] std::io::Error),
}

pub type Result<T> = std::result::Result<T, Error>;
