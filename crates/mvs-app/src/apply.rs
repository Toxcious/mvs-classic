//! A settings change applied:
//! the Settings the menu screens write (`Cx.prefs`)
//! turned into the renderer's scalability, MPC_CentralPalette, the main menu's background, the frame rate limit, the
//! window's mode, size and display, the volumes' RTPCs and a save.
//!
//! The app diffs the Settings once a frame (`Shell::apply_settings`, called by the program's frame loop) instead of the
//! pages calling an apply step on their exit: a page writes the field and the next frame applies it, so every writer
//! (the Graphics and Audio pages, the pause menu, the launcher) is covered and a slider drags live. Each target keeps
//! the value it was last given, so a target that does not exist yet (no store, no sound engine) is given its value
//! when it appears. The save follows a second after the last change and again when the program ends.

use std::time::{Duration, Instant};

use mvs_audio::{Cmd, GameObj};
use mvs_data::sound::short_id;
use mvs_render::Scalability;
use mvs_settings::{AudioSettings, FRAME_RATE_UNLIMITED, Settings, WindowMode, WindowSettings};

use crate::Result;
use crate::app::Shell;
use crate::platform::{Window, displays, refresh_displays};
use mvs_ui::bundle::CENTRAL_PALETTE;
use mvs_ui::menu::prefs::{self, DisplayInfo, WindowInfo};

/// The save follows the last change by this long.
const SAVE_DELAY: Duration = Duration::from_secs(1);

/// The global game parameters of the Audio screen's volumes, in the order of
/// `AudioSettings`' fields.
pub const VOLUME_RTPCS: [&str; 5] =
    ["menu_slider_master", "menu_slider_sfx", "menu_slider_mus", "menu_slider_vo", "menu_slider_announcer"];

/// The audio profile's RTPC (menu_profile): the default profile, 0 (the Audio output and profile rows are not shown).
const PROFILE_RTPC: &str = "menu_profile";

/// What was last applied to each target, and the save's state.
#[derive(Debug)]
pub struct SettingsApply {
    scalability: Option<Scalability>,
    /// The frame rate limit and the display it was paced for (`Window::monitor_id`).
    frame_limit: Option<(u32, isize)>,
    volumes: Option<[f64; 5]>,
    /// MPC_CentralPalette's rows.
    palette: Option<Vec<[f32; 4]>>,
    window: Option<WindowSettings>,
    /// The main menu's background (`UiSettings::menu_background`).
    menu_background: Option<String>,
    /// The settings as last saved.
    saved: Settings,
    /// When the settings were first found changed since the last save.
    dirty: Option<Instant>,
    data: std::path::PathBuf,
}

impl SettingsApply {
    /// Nothing applied yet; `settings` are the ones on disk.
    pub fn new(settings: &Settings, data: &std::path::Path) -> SettingsApply {
        SettingsApply {
            scalability: None,
            frame_limit: None,
            volumes: None,
            palette: None,
            window: None,
            menu_background: None,
            saved: settings.clone(),
            dirty: None,
            data: data.to_path_buf(),
        }
    }

    /// The store's renderer and sound engine are gone: the next apply sets their settings anew on the next ones.
    pub(crate) fn store_closed(&mut self) {
        self.scalability = None;
        self.volumes = None;
        self.palette = None;
        self.menu_background = None;
    }

    /// Writes the settings when they differ from the saved ones, the known relays first merged with the file's (the
    /// launcher may have kept one since: `online::merge_known_on_disk`).
    pub fn save(&mut self, settings: &mut Settings) {
        self.dirty = None;
        if *settings == self.saved {
            return;
        }
        let mut base = self.saved.online.known_relays.clone();
        mvs_settings::online::merge_known_on_disk(&mut settings.online, &mut base, &self.data);
        match settings.save(&self.data) {
            Ok(()) => self.saved = settings.clone(),
            Err(e) => eprintln!("mvs: settings not saved: {e}"),
        }
    }
}

/// The renderer's three scalability settings of `s`.
pub fn scalability_of(s: &Settings) -> Scalability {
    Scalability {
        aa: s.graphics.anti_aliasing_quality,
        resolution_quality: s.graphics.resolution_quality,
        texture_quality: s.graphics.texture_quality,
    }
}

/// The volumes of `a` as the RTPCs take them, in `VOLUME_RTPCS`' order.
pub fn volumes_of(a: &AudioSettings) -> [f64; 5] {
    [a.master, a.sfx, a.music, a.dialogue, a.announcer]
}

/// The swap chain's pacing for a frame rate limit on a display of `refresh_hz` (0: unknown): the presents a second (0:
/// none) and whether they wait on the refresh. The display's rate (0, or a limit the refresh matches) and a limit below
/// the refresh sync; a limit above it, and no limit, do not (the frames would be held to the refresh), and may tear.
pub fn frame_pacing(limit: u32, refresh_hz: f64) -> (u32, bool) {
    if limit == FRAME_RATE_UNLIMITED {
        (0, false)
    } else if limit == 0 || prefs::frame_rate_is_refresh(limit, refresh_hz) {
        (0, true)
    } else {
        (limit, refresh_hz <= 0.0 || f64::from(limit) < refresh_hz)
    }
}

/// The size a window change asks for (None: the window's own windowed size), from the window settings `old` last
/// applied to `new`: a size picked, or Windowed picked (the Graphics screen then sets its size,
/// @28448); a display or mode pick alone keeps the window's size.
pub fn asked_size(old: &WindowSettings, new: &WindowSettings) -> Option<[u32; 2]> {
    let sized = [old.width, old.height] != [new.width, new.height];
    let to_windowed = new.mode == WindowMode::Windowed && old.mode != WindowMode::Windowed;
    (sized || to_windowed).then_some([new.width, new.height])
}

/// The displays as the Graphics screen lists them (what the menu sink's `displays` answers, listed again: the screen
/// asks
/// when it opens).
pub fn display_infos() -> Vec<DisplayInfo> {
    refresh_displays()
        .iter()
        .map(|d| DisplayInfo { size: d.size(), name: d.name.clone(), refresh_hz: d.refresh_hz, primary: d.primary })
        .collect()
}

/// The window's state as the Graphics page reads it (what the menu sink's `window` answers).
pub fn window_info(w: &Window) -> WindowInfo {
    WindowInfo { mode: w.mode(), size: w.windowed_size(), display: w.display_index(&displays()) }
}

impl Shell {
    /// Gives every target the settings' value it does not have yet (the program's frame loop calls this each frame, and
    /// once the store opens): the renderer's scalability, the swap chain's frame rate limit, the volumes, a window
    /// change; then the save when the change is old enough.
    pub fn apply_settings(&mut self) -> Result<()> {
        let want = scalability_of(&self.settings);
        if self.apply.scalability != Some(want)
            && let Some(r) = &mut self.renderer
        {
            r.set_scalability(want)?;
            self.apply.scalability = Some(want);
        }
        // MPC_CentralPalette (PFGCamera's and BP_CharacterPreview's BeginPlay call Apply UI Material Colors and
        // bind OnUIPrefsChanged to call it again; the port's own: one collection for the program, set here and on each
        // change): the collection's defaults, then the UI preferences' colours, on the world's and the UI's materials.
        if let (Some(r), Some(u)) = (&mut self.renderer, &mut self.ui) {
            let want = u.menu.central_palette_rows(&self.settings.ui);
            if self.apply.palette.as_ref() != Some(&want) {
                r.set_collection(CENTRAL_PALETTE, &want);
                u.ui.materials.set_collection(r.gpu(), CENTRAL_PALETTE, &want);
                self.apply.palette = Some(want);
            }
        }
        // the main menu's background: read by the levels opened after it, and by the shown main menu at once
        let bg = &self.settings.ui.menu_background;
        if self.apply.menu_background.as_ref() != Some(bg)
            && let Some(r) = &mut self.renderer
        {
            r.set_menu_active_events(&crate::menus::active_events(bg));
            self.apply.menu_background = Some(bg.clone());
        }
        let limit = (self.settings.graphics.frame_rate_limit, self.window.monitor_id());
        if self.apply.frame_limit != Some(limit) {
            let (fps, vsync) = frame_pacing(limit.0, self.window.refresh_hz());
            self.swap.set_frame_limit(fps, vsync);
            self.apply.frame_limit = Some(limit);
        }
        let mut volumes = volumes_of(&self.settings.audio);
        // PandaGameInstance On Alt Tabbed (@94892 -> @8361): the master volume is 0 while the window is not the
        // foreground one and either it is minimised or Mute On Lost Focus is on.
        if !self.window.foreground() && (self.window.minimized() || self.settings.audio.mute_on_lost_focus) {
            volumes[0] = 0.0;
        }
        if self.apply.volumes != Some(volumes)
            && let Some(a) = &mut self.audio
        {
            // (global, no interpolation: ApplyAudioPrefs)
            for (name, v) in VOLUME_RTPCS.iter().zip(volumes) {
                a.send(0, Cmd::SetRtpc { id: short_id(name), value: v, obj: GameObj::GLOBAL, interp_ms: 0.0 });
            }
            a.send(0, Cmd::SetRtpc { id: short_id(PROFILE_RTPC), value: 0.0, obj: GameObj::GLOBAL, interp_ms: 0.0 });
            self.apply.volumes = Some(volumes);
        }
        match &self.apply.window {
            // (no placement yet: the window opened as the command line said, and the settings' state is the start)
            None => self.apply.window = Some(self.settings.window.clone()),
            Some(w) if *w != self.settings.window => {
                let size = asked_size(w, &self.settings.window);
                let all = refresh_displays();
                let display = self.settings.window.display.as_ref().and_then(|n| all.iter().find(|d| d.name == *n));
                self.window.set_window_mode(self.settings.window.mode, size, display)?;
                // the state the window took (its size kept inside its display, the display it is on)
                let w = &mut self.settings.window;
                [w.width, w.height] = self.window.windowed_size();
                w.display = all.get(self.window.display_index(&all)).map(|d| d.name.clone());
                self.apply.window = Some(w.clone());
            }
            Some(_) => {}
        }
        if self.settings != self.apply.saved {
            let since = *self.apply.dirty.get_or_insert_with(Instant::now);
            if since.elapsed() >= SAVE_DELAY {
                self.apply.save(&mut self.settings);
            }
        } else {
            self.apply.dirty = None;
        }
        Ok(())
    }

    /// The window placed as the saved window settings say (at the start of
    /// `mvs --play`): its mode, its size kept inside its display, the saved display (missing: the primary).
    pub fn place_window(&mut self) -> Result<()> {
        let w = &self.settings.window;
        let all = refresh_displays();
        let display = w.display.as_ref().and_then(|n| all.iter().find(|d| d.name == *n)).or(all.first());
        self.window.set_window_mode(w.mode, Some([w.width, w.height]), display)?;
        self.apply.window = Some(w.clone());
        Ok(())
    }
}

impl Drop for Shell {
    fn drop(&mut self) {
        // (a change younger than the save delay is saved with the program's end)
        self.apply.save(&mut self.settings);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn volumes_follow_the_rtpc_order() {
        let a = AudioSettings {
            master: 0.1,
            sfx: 0.2,
            music: 0.3,
            dialogue: 0.4,
            announcer: 0.5,
            mute_on_lost_focus: true,
        };
        assert_eq!(volumes_of(&a), [0.1, 0.2, 0.3, 0.4, 0.5]);
        assert_eq!(VOLUME_RTPCS[2], "menu_slider_mus");
    }

    #[test]
    fn the_menu_background_is_the_one_active_event() {
        assert_eq!(crate::menus::active_events(&Settings::default().ui.menu_background), ["event_002"]);
        assert_eq!(crate::menus::active_events("event_003"), ["event_003"]);
        assert!(crate::menus::active_events(mvs_settings::MENU_BACKGROUND_OWN).is_empty());
    }

    #[test]
    fn a_display_pick_keeps_the_windows_size() {
        let old = WindowSettings { mode: WindowMode::Windowed, width: 1600, height: 900, display: None };
        let moved = WindowSettings { display: Some("DISPLAY2".into()), ..old.clone() };
        assert_eq!(asked_size(&old, &moved), None);
        let sized = WindowSettings { width: 1024, height: 576, ..old.clone() };
        assert_eq!(asked_size(&old, &sized), Some([1024, 576]));
        let full = WindowSettings { mode: WindowMode::WindowedFullscreen, ..old.clone() };
        assert_eq!(asked_size(&old, &full), None);
        // (Windowed picked: 1024 x 576, even when the settings held it already)
        let back = WindowSettings { mode: WindowMode::Windowed, ..full.clone() };
        assert_eq!(asked_size(&full, &back), Some([1600, 900]));
    }

    #[test]
    fn frame_pacing_syncs_at_or_below_the_refresh() {
        assert_eq!(frame_pacing(0, 144.0), (0, true));
        assert_eq!(frame_pacing(60, 144.0), (60, true));
        assert_eq!(frame_pacing(144, 144.0), (0, true));
        assert_eq!(frame_pacing(240, 144.0), (240, false));
        assert_eq!(frame_pacing(FRAME_RATE_UNLIMITED, 144.0), (0, false));
        assert_eq!(frame_pacing(240, 0.0), (240, true));
    }

    #[test]
    fn scalability_takes_the_graphics_fields() {
        let mut s = Settings::default();
        s.graphics.anti_aliasing_quality = 1;
        s.graphics.resolution_quality = 66.5;
        s.graphics.texture_quality = 4;
        assert_eq!(scalability_of(&s), Scalability { aa: 1, resolution_quality: 66.5, texture_quality: 4 });
        assert_eq!(scalability_of(&Settings::default()), Scalability::default());
    }
}
