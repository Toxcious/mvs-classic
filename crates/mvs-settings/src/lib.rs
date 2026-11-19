//! The player's settings:
//! the graphics, audio and interface fields the port keeps in one JSON file,
//! `settings.json` in the data folder, read once at the start and written whole on each change.
//!
//! Reading takes each known field of its own type and range; a field that is missing, of another type or out of
//! range takes its default, and a file that is missing or does not read is the defaults. Writing goes to
//! `settings.json.part`, then a rename over the file.
//!
//! Deviation: one file holds the folders, the window and the name,
//! the GameUserSettings and the preferences.

mod input;
pub mod online;
mod saved;
pub mod stats;

use std::path::{Path, PathBuf};

use serde::Serialize;
use serde::de::DeserializeOwned;
use serde_json::{Map, Value};

pub use input::{InputPlayerSettings, InputSettings, LOCAL_PLAYERS};
pub use saved::{
    AssetRef, CharacterPrefs, HydraDefaultsRun, HydraSettings, LabSettings, LocalPlayPlayer, LocalPlaySettings,
    RememberedPerks,
};

/// The settings file's name in the data folder.
pub const FILE: &str = "settings.json";

#[derive(Debug, thiserror::Error)]
pub enum SettingsError {
    #[error("{0}: {1}")]
    Io(PathBuf, std::io::Error),
}

/// The settings file.
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct Settings {
    pub launcher: LauncherSettings,
    pub window: WindowSettings,
    pub graphics: GraphicsSettings,
    pub audio: AudioSettings,
    pub ui: UiSettings,
    /// The Controls screen's input preferences and key bindings.
    pub input: InputSettings,
    pub profile: ProfileSettings,
    /// The Collection's equipped cosmetics and each character's taunts and selected skin (HydraPreferences).
    pub hydra: HydraSettings,
    /// The perks the pre-match screen remembers per character.
    pub perks: RememberedPerks,
    /// The Lab Tools' switch and page: one Lab visit's, never saved (`LabSettings`).
    #[serde(skip)]
    pub lab: LabSettings,
    /// The Local Play screen's preferences.
    pub local_play: LocalPlaySettings,
    /// The online config's fields the player set, and the relays they said yes to.
    pub online: online::OnlineSetting,
}

/// The launcher's folders.
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct LauncherSettings {
    /// The folder of the store and setup.json; None: the data folder itself.
    pub data_root: Option<PathBuf>,
    /// The beta's folder the player picked last (setup's step 1); None before setup.
    pub beta_dir: Option<PathBuf>,
}

/// The window's modes (the build's EWindowMode less its exclusive full screen).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WindowMode {
    /// A framed window.
    #[default]
    Windowed,
    /// A borderless window covering its display.
    WindowedFullscreen,
}

/// The window's saved state, read before the window opens.
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct WindowSettings {
    pub mode: WindowMode,
    /// The windowed client size in physical pixels; 0: the first-run size.
    pub width: u32,
    pub height: u32,
    /// The system's name of the display the window was last on (`\\.\DISPLAY1`); None: the primary.
    pub display: Option<String>,
}

/// UGameUserSettings as the port keeps it and Struct_Preferences_Graphics's field
///, with the `stat fps` switch the Graphics page shows (UIPreferences.showFPS).
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct GraphicsSettings {
    /// sg.AntiAliasingQuality, 0..=4.
    pub anti_aliasing_quality: u8,
    /// sg.ResolutionQuality: the 3D view's percent of the window, 10..=100.
    pub resolution_quality: f64,
    /// sg.TextureQuality, 0..=4, applied at once (the renderer's set_scalability).
    pub texture_quality: u8,
    /// FrameRateLimit: the most frames drawn per second, 0..=1000; 0: every refresh of the display;
    /// `FRAME_RATE_UNLIMITED`: no limit.
    pub frame_rate_limit: u32,
    /// ui-showFPS: the frame rate line.
    pub show_fps: bool,
    /// SkipCharacterOcclusionSorting: PFGCamera's priority captures destroyed at its BeginPlay.
    pub skip_character_occlusion_sorting: bool,
}

/// `GraphicsSettings::frame_rate_limit`'s value of no limit: frames drawn as fast as they can be, not waiting on the
/// display's refresh.
pub const FRAME_RATE_UNLIMITED: u32 = u32::MAX;

/// sg.AntiAliasingQuality's levels.
pub const QUALITY_LEVELS: u8 = 5;

impl Default for GraphicsSettings {
    fn default() -> GraphicsSettings {
        GraphicsSettings {
            // sg.AntiAliasingQuality's start value (cvar default 140728120, FQualityLevels::SetDefaults 142d5c770),
            // what mvs-render's FXAA draws today.
            // Deviation: the exe's first-run autodetect (PreferencesManager AutodetectGraphicsSettings,
            // GraphicsPresetByGPU) replaces it; the port has none.
            anti_aliasing_quality: 3,
            // sg.ResolutionQuality's start value (140729580)
            resolution_quality: 100.0,
            // the default texture quality
            texture_quality: 3,
            frame_rate_limit: 0,
            show_fps: false,
            skip_character_occlusion_sorting: false,
        }
    }
}

/// r.MinYResolutionFor3DView's value (GetMinYResolutionFor3DView 142b20aa0 clamps the cvar to 200..8192; its default
/// is 360).
const MIN_Y_RESOLUTION_FOR_3D_VIEW: f64 = 360.0;

impl GraphicsSettings {
    /// MinResolutionScale (UpdateResolutionQuality 142a99050): max(10, r.MinYResolutionFor3DView 360 / H x 100), `h` the
    /// frame's height in pixels (the build takes the desktop's in windowed fullscreen, the window's otherwise: the
    /// 3D view's frame is that either way); 10 when the height is not known (0).
    pub fn min_resolution_scale(h: f64) -> f64 {
        if h > 0.0 { (MIN_Y_RESOLUTION_FOR_3D_VIEW / h * 100.0).max(10.0) } else { 10.0 }
    }

    /// GetResolutionScaleInformationEx 142a7fe50: CurrentScaleNormalized = (ResolutionQuality - Min) / (100 - Min); NaN
    /// when Min is 100 (a frame no taller than 360 pixels).
    pub fn resolution_scale_normalized(&self, h: f64) -> f64 {
        let min = GraphicsSettings::min_resolution_scale(h);
        (self.resolution_quality - min) / (100.0 - min)
    }

    /// SetResolutionScaleNormalized 142a92a10 -> SetResolutionScaleValueEx 142a92a50: ResolutionQuality =
    /// clamp(Min + (100 - Min) x n, Min, 100), no rounding.
    pub fn set_resolution_scale_normalized(&mut self, n: f64, h: f64) {
        let min = GraphicsSettings::min_resolution_scale(h);
        self.resolution_quality = (min + (100.0 - min) * n).clamp(min, 100.0);
    }
}

/// Struct_Preferences_Audio's five volumes, each 0..=1.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct AudioSettings {
    pub master: f64,
    pub sfx: f64,
    pub music: f64,
    pub dialogue: f64,
    pub announcer: f64,
    /// Mute On Lost Focus (muteMusicWhenLostFocus): the master volume is 0 while the window is not the foreground one.
    pub mute_on_lost_focus: bool,
}

impl Default for AudioSettings {
    fn default() -> AudioSettings {
        AudioSettings { master: 1.0, sfx: 1.0, music: 1.0, dialogue: 1.0, announcer: 1.0, mute_on_lost_focus: true }
    }
}

/// Struct_Preferences_UI's fields (less showFPS: `GraphicsSettings::show_fps`).
/// world_* show on the fighter (the attached HUD, the fighter itself), hud_* on the scoreboard. The colour_* fields
/// hold the CentralPalette row's name.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct UiSettings {
    pub world_show_percentage: bool,
    pub world_show_cooldowns: bool,
    pub world_show_username: bool,
    pub world_show_dodge_meters: bool,
    pub hud_show_percentage: bool,
    pub hud_show_cooldowns: bool,
    pub hud_show_username: bool,
    pub hud_show_dodge_meters: bool,
    pub hud_position_bottom: bool,
    pub world_show_ally_outlines: bool,
    pub world_show_enemy_outlines: bool,
    pub world_show_my_outline: bool,
    pub world_use_team_colored_skins: bool,
    pub world_show_di: bool,
    pub cursor_speed_multiplier: f64,
    pub color_ally: String,
    pub color_enemy: String,
    pub color_me: String,
    pub color_team3: String,
    pub color_team4: String,
    /// (the port's own) This player shows on the relay's online list (presence
    /// `listed`).
    pub show_in_online_list: bool,
    /// (the port's own) Other players may invite this one (presence `invites`); off, no
    /// invite is shown.
    pub invites: bool,
    /// (the port's own) The chat of online matches is shown: the bubbles at the portraits and
    /// the chat box on Enter.
    pub chat_in_match: bool,
    /// (the port's own) The main menu's background: a slug of MainMenuBackground's Event Lookup Table (its material
    /// drawn), or `MENU_BACKGROUND_OWN` for the background's own picture (the Trophy Room).
    // Deviation: the build picks it from the server calendar's active events (empty offline: the Trophy Room); here
    // the player picks it on the home screen (mvs-ui widgets/home.rs), Festiversus by default (the user's choice).
    pub menu_background: String,
}

/// `UiSettings::menu_background`'s value of MainMenuBackground's own material.
pub const MENU_BACKGROUND_OWN: &str = "";

/// `UiSettings::menu_background`'s default: Festiversus's MI_MenuBG_Event002.
pub const MENU_BACKGROUND_DEFAULT: &str = "event_002";

impl Default for UiSettings {
    /// The PreferencesClass default object (PM SetAllToDefaults, the Recommended preset).
    fn default() -> UiSettings {
        UiSettings {
            world_show_percentage: true,
            world_show_cooldowns: true,
            world_show_username: true,
            world_show_dodge_meters: true,
            hud_show_percentage: false,
            hud_show_cooldowns: false,
            hud_show_username: true,
            hud_show_dodge_meters: false,
            hud_position_bottom: false,
            world_show_ally_outlines: true,
            world_show_enemy_outlines: true,
            world_show_my_outline: true,
            world_use_team_colored_skins: true,
            world_show_di: false,
            cursor_speed_multiplier: 1.0,
            color_ally: "Blue1".into(),
            color_enemy: "Red1".into(),
            color_me: "Yellow1".into(),
            color_team3: "Green1".into(),
            color_team4: "Purple1".into(),
            show_in_online_list: true,
            invites: true,
            chat_in_match: true,
            menu_background: MENU_BACKGROUND_DEFAULT.into(),
        }
    }
}

/// The player's profile.
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct ProfileSettings {
    /// The player's name ('' until asked).
    pub name: String,
    /// The SaveGame slot "PreviewCharacter" (UI_PreMatch's Save Chosen Characters): the roster slug of the fighter
    /// the player chose last, '' before one.
    pub last_character: String,
}

/// `o[key]` as a `T` that `ok` accepts, else `or`.
fn take<T: DeserializeOwned>(o: &Map<String, Value>, key: &str, or: T, ok: impl Fn(&T) -> bool) -> T {
    o.get(key).and_then(|v| serde_json::from_value::<T>(v.clone()).ok()).filter(|v| ok(v)).unwrap_or(or)
}

/// `o[key]` as an object (an empty one when it is not).
fn section<'a>(o: &'a Map<String, Value>, key: &str, empty: &'a Map<String, Value>) -> &'a Map<String, Value> {
    o.get(key).and_then(Value::as_object).unwrap_or(empty)
}

const fn any<T>(_: &T) -> bool {
    true
}

fn unit(v: &f64) -> bool {
    (0.0..=1.0).contains(v)
}

impl Settings {
    /// The settings a saved value holds, field by field: a field missing, of another type or out
    /// of range takes its default.
    pub fn from_value(v: &Value) -> Settings {
        let empty = Map::new();
        let root = v.as_object().unwrap_or(&empty);
        let d = Settings::default();
        let s = section(root, "launcher", &empty);
        let launcher = LauncherSettings {
            data_root: take(s, "data_root", d.launcher.data_root, any),
            beta_dir: take(s, "beta_dir", d.launcher.beta_dir, any),
        };
        let s = section(root, "window", &empty);
        let window = WindowSettings {
            mode: take(s, "mode", d.window.mode, any),
            width: take(s, "width", d.window.width, any),
            height: take(s, "height", d.window.height, any),
            display: take(s, "display", d.window.display, any),
        };
        let s = section(root, "graphics", &empty);
        let g = d.graphics;
        let graphics = GraphicsSettings {
            anti_aliasing_quality: take(s, "anti_aliasing_quality", g.anti_aliasing_quality, |v| *v < QUALITY_LEVELS),
            resolution_quality: take(s, "resolution_quality", g.resolution_quality, |v| (10.0..=100.0).contains(v)),
            texture_quality: take(s, "texture_quality", g.texture_quality, |v| *v < QUALITY_LEVELS),
            frame_rate_limit: take(s, "frame_rate_limit", g.frame_rate_limit, |v| {
                *v <= 1000 || *v == FRAME_RATE_UNLIMITED
            }),
            show_fps: take(s, "show_fps", g.show_fps, any),
            skip_character_occlusion_sorting: take(
                s,
                "skip_character_occlusion_sorting",
                g.skip_character_occlusion_sorting,
                any,
            ),
        };
        let s = section(root, "audio", &empty);
        let a = d.audio;
        let audio = AudioSettings {
            master: take(s, "master", a.master, unit),
            sfx: take(s, "sfx", a.sfx, unit),
            music: take(s, "music", a.music, unit),
            dialogue: take(s, "dialogue", a.dialogue, unit),
            announcer: take(s, "announcer", a.announcer, unit),
            mute_on_lost_focus: take(s, "mute_on_lost_focus", a.mute_on_lost_focus, any),
        };
        let s = section(root, "ui", &empty);
        let u = d.ui;
        let ui = UiSettings {
            world_show_percentage: take(s, "world_show_percentage", u.world_show_percentage, any),
            world_show_cooldowns: take(s, "world_show_cooldowns", u.world_show_cooldowns, any),
            world_show_username: take(s, "world_show_username", u.world_show_username, any),
            world_show_dodge_meters: take(s, "world_show_dodge_meters", u.world_show_dodge_meters, any),
            hud_show_percentage: take(s, "hud_show_percentage", u.hud_show_percentage, any),
            hud_show_cooldowns: take(s, "hud_show_cooldowns", u.hud_show_cooldowns, any),
            hud_show_username: take(s, "hud_show_username", u.hud_show_username, any),
            hud_show_dodge_meters: take(s, "hud_show_dodge_meters", u.hud_show_dodge_meters, any),
            hud_position_bottom: take(s, "hud_position_bottom", u.hud_position_bottom, any),
            world_show_ally_outlines: take(s, "world_show_ally_outlines", u.world_show_ally_outlines, any),
            world_show_enemy_outlines: take(s, "world_show_enemy_outlines", u.world_show_enemy_outlines, any),
            world_show_my_outline: take(s, "world_show_my_outline", u.world_show_my_outline, any),
            world_use_team_colored_skins: take(s, "world_use_team_colored_skins", u.world_use_team_colored_skins, any),
            world_show_di: take(s, "world_show_di", u.world_show_di, any),
            cursor_speed_multiplier: take(s, "cursor_speed_multiplier", u.cursor_speed_multiplier, |v| {
                v.is_finite() && *v > 0.0
            }),
            color_ally: take(s, "color_ally", u.color_ally, |v: &String| !v.is_empty()),
            color_enemy: take(s, "color_enemy", u.color_enemy, |v: &String| !v.is_empty()),
            color_me: take(s, "color_me", u.color_me, |v: &String| !v.is_empty()),
            color_team3: take(s, "color_team3", u.color_team3, |v: &String| !v.is_empty()),
            color_team4: take(s, "color_team4", u.color_team4, |v: &String| !v.is_empty()),
            show_in_online_list: take(s, "show_in_online_list", u.show_in_online_list, any),
            invites: take(s, "invites", u.invites, any),
            chat_in_match: take(s, "chat_in_match", u.chat_in_match, any),
            menu_background: take(s, "menu_background", u.menu_background, any),
        };
        let input = input::input_from(root, &empty);
        let s = section(root, "profile", &empty);
        let profile = ProfileSettings {
            name: take(s, "name", d.profile.name, any),
            last_character: take(s, "last_character", d.profile.last_character, any),
        };
        let hydra = saved::hydra_from(root, &empty);
        let perks = saved::perks_from(root);
        let local_play = saved::local_play_from(root, &empty);
        // (an online.json matters only to an old one-backend form, which no settings.json of this program holds)
        let online = online::online_setting_of(root.get("online").unwrap_or(&Value::Null), &online::ONLINE_DEFAULTS);
        Settings {
            launcher,
            window,
            graphics,
            audio,
            ui,
            input,
            profile,
            hydra,
            perks,
            lab: LabSettings::default(),
            local_play,
            online,
        }
    }

    /// `<dir>/settings.json`; the defaults when it is missing or does not read.
    pub fn load(dir: &Path) -> Settings {
        let v = std::fs::read(dir.join(FILE)).ok().and_then(|b| serde_json::from_slice::<Value>(&b).ok());
        Settings::from_value(&v.unwrap_or(Value::Null))
    }

    /// Writes the settings whole to `<dir>/settings.json` (the folder made when missing), through
    /// `settings.json.part` and a rename.
    pub fn save(&self, dir: &Path) -> Result<(), SettingsError> {
        let io = |p: &Path| {
            let p = p.to_path_buf();
            move |e| SettingsError::Io(p, e)
        };
        std::fs::create_dir_all(dir).map_err(io(dir))?;
        let file = dir.join(FILE);
        let part = dir.join(format!("{FILE}.part"));
        let text = serde_json::to_string_pretty(self).expect("the settings serialize");
        std::fs::write(&part, text).map_err(io(&part))?;
        std::fs::rename(&part, &file).map_err(io(&file))
    }
}

/// The data folder: `arg` (the exe's `--data <dir>`); else for an exe built in a Cargo workspace (its path holds a
/// `target` folder beside a `Cargo.toml`) the workspace's `.dev` folder; else `data` at the top of the release folder
/// (mvs_release::app_root: the launcher's and the game's share it).
pub fn data_dir(arg: Option<&Path>) -> PathBuf {
    if let Some(a) = arg {
        return a.to_path_buf();
    }
    let exe = std::env::current_exe().unwrap_or_default();
    for d in exe.ancestors() {
        if d.file_name().is_some_and(|n| n == "target")
            && let Some(root) = d.parent()
            && root.join("Cargo.toml").is_file()
        {
            return root.join(".dev");
        }
    }
    exe.parent()
        .map_or_else(|| PathBuf::from(mvs_release::DATA_DIR), |p| mvs_release::app_root(p).join(mvs_release::DATA_DIR))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn scratch(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("mvs-settings-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        d
    }

    #[test]
    fn resolution_scale_runs_between_min_and_100() {
        // 720 -> 50, 1080 -> 33.33, 1440 -> 25, 3600 and up -> 10 (the exe's examples)
        assert_eq!(GraphicsSettings::min_resolution_scale(720.0), 50.0);
        assert!((GraphicsSettings::min_resolution_scale(1080.0) - 100.0 / 3.0).abs() < 1e-9);
        assert_eq!(GraphicsSettings::min_resolution_scale(1440.0), 25.0);
        assert_eq!(GraphicsSettings::min_resolution_scale(4000.0), 10.0);
        assert_eq!(GraphicsSettings::min_resolution_scale(0.0), 10.0);
        let mut g = GraphicsSettings::default();
        // item 0 is Min, item 100 is 100 %, item 50 at 1080 is 66.67 %
        g.set_resolution_scale_normalized(0.0, 1080.0);
        assert!((g.resolution_quality - 100.0 / 3.0).abs() < 1e-9);
        g.set_resolution_scale_normalized(0.5, 1080.0);
        assert!((g.resolution_quality - 200.0 / 3.0).abs() < 1e-9);
        assert!((g.resolution_scale_normalized(1080.0) - 0.5).abs() < 1e-9);
        g.set_resolution_scale_normalized(2.0, 1080.0);
        assert_eq!(g.resolution_quality, 100.0);
        // a frame no taller than 360 px has no range
        assert!(g.resolution_scale_normalized(360.0).is_nan());
    }

    #[test]
    fn nothing_saved_is_the_defaults() {
        let d = scratch("none");
        assert_eq!(Settings::load(&d), Settings::default());
        std::fs::create_dir_all(&d).unwrap();
        std::fs::write(d.join(FILE), "not json").unwrap();
        assert_eq!(Settings::load(&d), Settings::default());
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn a_bad_field_takes_its_default_and_the_rest_stays() {
        let s = Settings::from_value(&json!({
            "window": { "mode": "windowed_fullscreen", "width": -5, "height": 900, "display": 3 },
            "graphics": { "anti_aliasing_quality": 9, "resolution_quality": 75.5, "texture_quality": "high",
                          "frame_rate_limit": 144, "show_fps": true },
            "audio": { "master": 0.5, "music": 2.0, "sfx": null },
            "ui": { "color_me": "", "color_ally": "Green2", "cursor_speed_multiplier": 0 },
            "profile": { "name": "Sam" },
            "unknown": 1
        }));
        let d = Settings::default();
        assert_eq!(s.window.mode, WindowMode::WindowedFullscreen);
        assert_eq!((s.window.width, s.window.height, s.window.display.clone()), (0, 900, None));
        assert_eq!(s.graphics.anti_aliasing_quality, d.graphics.anti_aliasing_quality);
        assert_eq!(s.graphics.resolution_quality, 75.5);
        assert_eq!(s.graphics.texture_quality, d.graphics.texture_quality);
        assert_eq!((s.graphics.frame_rate_limit, s.graphics.show_fps), (144, true));
        assert_eq!((s.audio.master, s.audio.music, s.audio.sfx), (0.5, 1.0, 1.0));
        assert_eq!((s.ui.color_me.as_str(), s.ui.color_ally.as_str()), ("Yellow1", "Green2"));
        assert_eq!(s.ui.cursor_speed_multiplier, 1.0);
        // (no menu background saved: Festiversus's)
        assert_eq!(s.ui.menu_background, MENU_BACKGROUND_DEFAULT);
        assert_eq!(s.profile.name, "Sam");
        assert_eq!(s.launcher, d.launcher);
    }

    #[test]
    fn a_save_reads_back_whole() {
        let d = scratch("save");
        let mut s = Settings::default();
        s.launcher.data_root = Some(PathBuf::from("D:/MVS Classic"));
        s.launcher.beta_dir = Some(PathBuf::from("E:/beta"));
        s.window = WindowSettings {
            mode: WindowMode::WindowedFullscreen,
            width: 1600,
            height: 900,
            display: Some(r"\\.\DISPLAY2".into()),
        };
        s.graphics.resolution_quality = 66.6;
        s.audio.announcer = 0.25;
        s.ui.hud_position_bottom = true;
        s.ui.menu_background = MENU_BACKGROUND_OWN.into();
        s.input.players[2].up_to_jump = true;
        s.input.players[1].input_buffer_cap = 12;
        s.input.bindings = Some(indexmap::IndexMap::from([("Attack-P0".to_string(), vec!["J".to_string()])]));
        s.profile.name = "Velma".into();
        s.save(&d).unwrap();
        assert!(!d.join(format!("{FILE}.part")).exists());
        assert_eq!(Settings::load(&d), s);
        // a second save replaces the file
        s.profile.name = "Fred".into();
        s.save(&d).unwrap();
        assert_eq!(Settings::load(&d).profile.name, "Fred");
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn the_data_folder_is_the_argument_when_given() {
        assert_eq!(data_dir(Some(Path::new("X:/d"))), PathBuf::from("X:/d"));
        // (a test binary runs from the workspace's target folder)
        assert!(data_dir(None).ends_with(".dev"));
    }
}
