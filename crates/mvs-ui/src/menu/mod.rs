//! The menus: the view the user widgets live in (view.rs), the UI manager's focus, modals,
//! dialogs and transitions (manager.rs), the controllers of the widget Blueprints (widgets/, screens/), the
//! screens a level opens (screen.rs), the input (input.rs), the custom lobby (lobby/), the menu tape (tape.rs).
//!
//! The menus touch nothing outside the crate but through `MenuSink` (the sound) and their `MenuExit` (what the
//! program does next): the app crate runs them.

pub mod arcade;
pub mod blur;
pub mod connection_line;
pub mod controller;
pub mod cursor;
pub mod custom_text;
pub mod hydra;
pub mod input;
pub mod lobby;
pub mod local_play_match;
pub mod manager;
pub mod map_tag;
pub mod match_pause;
pub mod online;
pub mod online_line;
pub mod online_read;
pub mod online_text;
pub mod own_profile;
pub mod palette;
pub mod pictures;
pub mod prefs;
pub mod profile_rules;
pub mod registry;
pub mod rich_text;
pub mod scene;
pub mod screen;
pub mod screens;
pub mod scroll;
pub mod session;
pub mod stat_trackers;
pub mod tape;
pub mod toasts;
pub mod training_config;
pub mod tutorial;
pub mod view;
pub mod waves;
pub mod widgets;

pub use controller::{Controller, Cx, Event, Handler, MenuRun};
pub use input::MenuInput;
pub use manager::Manager;
pub use screen::MenuScreen;
pub use view::MenuView;

/// The menus' sound (Wwise event posts, event checks and RTPCs, and the switches and states the widgets set): the app
/// crate's glue to the sound engine.
pub trait MenuSink {
    /// PF_Post_Global_Event: the event posted on the UI's object.
    fn post(&mut self, event: &str);
    /// The event is one the soundbanks hold (a post of another does nothing).
    fn has_event(&self, event: &str) -> bool;
    /// SetRTPCValue, global, over `interp_ms`.
    fn set_rtpc(&mut self, name: &str, value: f64, interp_ms: f64);
    fn set_switch(&mut self, group: &str, value: &str);
    fn set_state(&mut self, group: &str, value: &str);
    /// Puts `text` on the clipboard (the lobby's room code button).
    fn clipboard(&mut self, _text: &str) {}
    /// Opens the folder `path` in the system's file browser (a desync report's notice).
    fn open_folder(&mut self, _path: &str) {}
    /// The announcer pack at `pack_path` was equipped: the game state speaks its equip line (UI_CollectionChooser
    /// @29359-29906).
    fn announcer_equipped(&mut self, _pack_path: &str) {}
    /// The displays the Graphics page lists, the primary first (none: no window).
    fn displays(&self) -> Vec<prefs::DisplayInfo> {
        Vec::new()
    }
    /// The window's mode, framed size and display (the Graphics page's start state).
    fn window(&self) -> prefs::WindowInfo {
        prefs::WindowInfo { mode: mvs_settings::WindowMode::Windowed, size: [1280, 720], display: 0 }
    }
}

/// A sink that plays nothing (no sound engine).
#[derive(Clone, Copy, Debug, Default)]
pub struct NoSound;

impl MenuSink for NoSound {
    fn post(&mut self, _event: &str) {}
    fn has_event(&self, _event: &str) -> bool {
        true
    }
    fn set_rtpc(&mut self, _name: &str, _value: f64, _interp_ms: f64) {}
    fn set_switch(&mut self, _group: &str, _value: &str) {}
    fn set_state(&mut self, _group: &str, _value: &str) {}
}

/// What the menus ask of the program when a frame ends.
#[derive(Debug)]
pub enum MenuExit {
    /// OpenLevel: the level and its options string.
    OpenLevel { level: String, options: String },
    /// A match to load.
    Match(Box<lobby::LobbyMatch>),
    /// The pre-match screen to open over the PreMatch level (BPI_OpenLevel(PreMatch)).
    PreMatch(Box<screens::pre_match::setup::PreMatchStart>),
    /// The game quit (QuitGame).
    Quit,
    /// Back to the launcher (the port's own: the launcher's home).
    Launcher,
}
