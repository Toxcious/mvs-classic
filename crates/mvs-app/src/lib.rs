//! The game's program: the Win32 window and its swap chain, keyboard and
//! pad input into the sim's InputState, the match loop (the sim's 60 Hz ticks, the views, the renderer, the HUD and
//! the UI drawn on one back buffer), and the perf readouts.

pub mod app;
pub mod apply;
pub mod audio;
pub mod console;
pub mod fps;
pub mod import;
pub mod input;
pub mod lab_host;
pub mod lab_setup;
pub mod lab_tools;
pub mod launcher;
pub mod load_bench;
pub mod loading;
pub mod match_chat;
pub mod match_lab;
pub mod match_sweep;
pub mod menu_audio;
pub mod menus;
pub mod online;
pub mod pause;
pub mod perf;
pub mod platform;
pub mod pre_match_voice;
pub mod prepare;
pub mod results;
pub mod rumble;
pub mod run;
pub mod script;
pub mod self_check;
pub mod selftest;
pub mod shapes;
pub mod tape;
pub mod tutorial;
pub mod update;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// A Win32 call that failed.
    #[error("{what}: {source}")]
    Os { what: String, source: windows::core::Error },
    #[error(transparent)]
    Store(#[from] mvs_store::StoreError),
    #[error(transparent)]
    Data(#[from] mvs_data::DataError),
    #[error(transparent)]
    Render(#[from] mvs_render::Error),
    #[error(transparent)]
    View(#[from] mvs_view::Error),
    #[error(transparent)]
    Ui(#[from] mvs_ui::Error),
    /// A match the setup or the trace does not make.
    #[error("{0}")]
    Match(String),
}

pub type Result<T> = std::result::Result<T, Error>;
