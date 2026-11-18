//! The Settings pages' rules: what each row lists, shows selected, and what a pick writes into the `Settings`. The
//! widgets (`widgets/preferences.rs`, `widgets/preferences_graphics.rs`) call these and only place the answers on
//! their rows; the program's frame loop applies what the pages wrote (`mvs-app` apply.rs), so a pick here is a write
//! of the field and nothing else.
//!
//! Deviation: the build's PreferencesManager (a save slot, setters, listeners, ApplyAudioPrefs) is the `Settings`
//! file and the app's per-frame diff; Mute On Lost Focus, the audio output mode and profile and the subtitle rows
//! have no field (the pages do not show them).

use mvs_settings::{
    AudioSettings, FRAME_RATE_UNLIMITED, GraphicsSettings, InputPlayerSettings, InputSettings, QUALITY_LEVELS,
    Settings, UiSettings, WindowMode,
};

/// The Audio page's volume rows, in the page's order (the order of `AudioSettings`' fields).
pub const VOLUME_ROWS: [&str; 5] =
    ["UI_MasterVolume", "UI_SoundVolume", "UI_MusicVolume", "UI_DialogVolume", "UI_AnnouncerVolume"];

/// The order Restore Defaults resets the rows in (the build's).
pub const RESET_ORDER: [&str; 5] =
    ["UI_DialogVolume", "UI_MasterVolume", "UI_MusicVolume", "UI_SoundVolume", "UI_AnnouncerVolume"];

/// The volume of the row `row` (an index into `VOLUME_ROWS`), 0..=1.
pub fn volume(a: &AudioSettings, row: usize) -> f64 {
    match row {
        0 => a.master,
        1 => a.sfx,
        2 => a.music,
        3 => a.dialogue,
        _ => a.announcer,
    }
}

/// Set-Audio-<X>Volume: FClamp(v, 0, 1), stored only.
pub fn set_volume(a: &mut AudioSettings, row: usize, v: f64) {
    let v = v.clamp(0.0, 1.0);
    match row {
        0 => a.master = v,
        1 => a.sfx = v,
        2 => a.music = v,
        3 => a.dialogue = v,
        _ => a.announcer = v,
    }
}

/// A row's index in `VOLUME_ROWS`.
pub fn volume_row(widget: &str) -> Option<usize> {
    VOLUME_ROWS.iter().position(|n| *n == widget)
}

/// The limits the Max Framerate row offers, frames per second (0: the display's rate; `FRAME_RATE_UNLIMITED`: none).
// Deviation: the build has no such row (its FrameRateLimit is left at 0 and the beta's own cap is 60 fps); the row is
// the port's own. A limit above the display's refresh, and Unlimited, draw without vsync (apply.rs frame_pacing).
pub const FRAME_RATE_LIMITS: [u32; 9] = [0, 30, 60, 90, 120, 144, 165, 240, FRAME_RATE_UNLIMITED];

/// Whether `limit` is the refresh of a display of `refresh_hz` (within 2%): the display's rate under another name.
pub fn frame_rate_is_refresh(limit: u32, refresh_hz: f64) -> bool {
    refresh_hz > 0.0 && limit != FRAME_RATE_UNLIMITED && (f64::from(limit) - refresh_hz).abs() < refresh_hz * 0.02
}

/// The limits offered on the window's display (`refresh_hz`, 0 when not known): all but the one its refresh matches,
/// the first always 0, the display's rate.
pub fn frame_rates_offered(refresh_hz: f64) -> Vec<u32> {
    FRAME_RATE_LIMITS.iter().copied().filter(|&r| r == 0 || !frame_rate_is_refresh(r, refresh_hz)).collect()
}

/// The limit the row shows selected: the saved one when it is offered, else 0 (a saved limit the display cannot reach
/// is kept and shows as the display's rate: every refresh is drawn under it).
pub fn frame_rate_shown(saved: u32, offered: &[u32]) -> u32 {
    if offered.contains(&saved) { saved } else { 0 }
}

/// A display as the Graphics page lists it (the app's `platform::Display`, given by the program).
#[derive(Clone, Debug, PartialEq)]
pub struct DisplayInfo {
    /// The system's name (`WindowSettings::display`).
    pub name: String,
    /// The display's size, pixels.
    pub size: [u32; 2],
    /// The refresh rate, Hz; 0 when unknown.
    pub refresh_hz: f64,
    pub primary: bool,
}

/// The window's state as the Graphics page reads it.
#[derive(Clone, Debug, PartialEq)]
pub struct WindowInfo {
    pub mode: WindowMode,
    /// The framed client size (what a return from borderless takes).
    pub size: [u32; 2],
    /// The index in the displays list of the display the window is on.
    pub display: usize,
}

/// The size a pick of Windowed sets (WindowMode @28448: 1024 x 576).
pub const WINDOWED_PICK_SIZE: [u32; 2] = [1024, 576];

/// The window-mode menu's item ids (the build's exclusive `fullscreen` is not an item).
pub fn mode_id(m: WindowMode) -> &'static str {
    match m {
        WindowMode::Windowed => "windowed",
        WindowMode::WindowedFullscreen => "windowedFullscreen",
    }
}

/// The mode a window-mode item id names.
pub fn mode_of_id(id: &str) -> Option<WindowMode> {
    match id {
        "windowed" => Some(WindowMode::Windowed),
        "windowedFullscreen" => Some(WindowMode::WindowedFullscreen),
        _ => None,
    }
}

/// Builder W (U@9162-10258): the class's windowed resolutions no larger than the display's, in their order.
pub fn resolution_options(all: &[[u32; 2]], display: Option<&DisplayInfo>) -> Vec<[u32; 2]> {
    match display {
        Some(d) => all.iter().copied().filter(|r| r[0] <= d.size[0] && r[1] <= d.size[1]).collect(),
        None => Vec::new(),
    }
}

/// A resolution item's label, "X x Y".
pub fn resolution_label(r: [u32; 2]) -> String {
    format!("{} x {}", r[0], r[1])
}

/// The item of `options` that is the window's current size.
pub fn resolution_selected(options: &[[u32; 2]], w: &WindowInfo) -> Option<usize> {
    options.iter().position(|r| *r == w.size)
}

/// A display item's label: the name, the primary's with `primary` (" (Primary)") after it.
pub fn display_label(d: &DisplayInfo, primary: &str) -> String {
    if d.primary { format!("{}{}", d.name, primary) } else { d.name.clone() }
}

/// The frame-rate item's label: "<n> fps", `unlimited` for no limit, or the display's rate for 0.
pub fn frame_rate_label(limit: u32, refresh_hz: f64, none: &str, unlimited: &str) -> String {
    if limit == FRAME_RATE_UNLIMITED {
        unlimited.to_owned()
    } else if limit > 0 {
        format!("{limit} fps")
    } else if refresh_hz > 0.0 {
        format!("{} Hz ({none})", refresh_hz.round())
    } else {
        none.to_owned()
    }
}

/// WindowMode @34273: the mode picked is written; Windowed also sets 1024 x 576. Returns whether the resolution row
/// shows (Windowed).
pub fn pick_window_mode(s: &mut Settings, mode: WindowMode) -> bool {
    s.window.mode = mode;
    if mode == WindowMode::Windowed {
        s.window.width = WINDOWED_PICK_SIZE[0];
        s.window.height = WINDOWED_PICK_SIZE[1];
    }
    mode == WindowMode::Windowed
}

/// Resolution @34268: the size picked, unless it is the window's current one. Returns whether it changed.
pub fn pick_resolution(s: &mut Settings, r: [u32; 2], w: &WindowInfo) -> bool {
    if r == w.size {
        return false;
    }
    s.window.width = r[0];
    s.window.height = r[1];
    true
}

/// DisplayMonitor @35508: the window moves to the display `d`.
pub fn pick_display(s: &mut Settings, d: &DisplayInfo) {
    s.window.display = Some(d.name.clone());
}

/// Init Resolution Scale (Fn@0-226): the id of the item of Round(normalized x 10) x 10, None where the frame is no
/// taller than 360 pixels (Min 100: no normalized value).
pub fn resolution_scale_id(g: &GraphicsSettings, frame_height: f64) -> Option<String> {
    let n = g.resolution_scale_normalized(frame_height);
    n.is_finite().then(|| ((n * 10.0).round() as i32 * 10).to_string())
}

/// ResolutionScale @33470: SetResolutionScaleNormalized(id / 100).
pub fn pick_resolution_scale(g: &mut GraphicsSettings, id: &str, frame_height: f64) {
    if let Ok(v) = id.parse::<f64>() {
        g.set_resolution_scale_normalized(v / 100.0, frame_height);
    }
}

/// AA @31832: SetAntiAliasingQuality(index), 0..=4.
pub fn pick_anti_aliasing(g: &mut GraphicsSettings, index: usize) {
    g.anti_aliasing_quality = index.min(usize::from(QUALITY_LEVELS) - 1) as u8;
}

/// Texture @32248: SetTextureQuality(index). Returns whether the level differs (the panel then shows its restart
/// dialog on exit: TextureSettingsChangedDispatch).
pub fn pick_texture(g: &mut GraphicsSettings, index: usize) -> bool {
    let v = index.min(usize::from(QUALITY_LEVELS) - 1) as u8;
    let changed = g.texture_quality != v;
    g.texture_quality = v;
    changed
}

/// CharacterOcclusion @34293: SkipCharacterOcclusionSorting = !on.
pub fn pick_character_occlusion(g: &mut GraphicsSettings, on: bool) {
    g.skip_character_occlusion_sorting = !on;
}

// ---------- the Controls page's input preferences (Get-Input-* / Set-Input-*) ----------

/// The bools of Struct_Preferences_Input a switch of the Additional Options reads and writes (InputBoolKey).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InputBool {
    Vibration,
    SwappedAttackStyleGround,
    SwappedAttackStyleAir,
    DeadzoneLarge,
    InputBufferHold,
    DoubleCrouchJumpDown,
    UpToJump,
    AttackPicksUpItems,
    IgnoreRightStickDiagonals,
}

fn input_bool_field(p: &mut InputPlayerSettings, k: InputBool) -> &mut bool {
    match k {
        InputBool::Vibration => &mut p.vibration,
        InputBool::SwappedAttackStyleGround => &mut p.swapped_attack_style_ground,
        InputBool::SwappedAttackStyleAir => &mut p.swapped_attack_style_air,
        InputBool::DeadzoneLarge => &mut p.deadzone_large,
        InputBool::InputBufferHold => &mut p.input_buffer_hold,
        InputBool::DoubleCrouchJumpDown => &mut p.double_crouch_jump_down,
        InputBool::UpToJump => &mut p.up_to_jump,
        InputBool::AttackPicksUpItems => &mut p.attack_picks_up_items,
        InputBool::IgnoreRightStickDiagonals => &mut p.ignore_right_stick_diagonals,
    }
}

/// Get-Input-<X>(Player Index): the entry, or the getter's own value past the array (false).
pub fn input_bool(s: &InputSettings, k: InputBool, player: usize) -> bool {
    s.players.get(player).is_some_and(|p| {
        let mut p = *p;
        *input_bool_field(&mut p, k)
    })
}

/// Set-Input-<X>(value, Player Index): players 0..3.
pub fn set_input_bool(s: &mut InputSettings, k: InputBool, v: bool, player: usize) {
    if let Some(p) = s.players.get_mut(player) {
        *input_bool_field(p, k) = v;
    }
}

/// Get-Input-FastfallMethod (0 past the array).
pub fn input_fastfall(s: &InputSettings, player: usize) -> u8 {
    s.players.get(player).map_or(0, |p| p.fastfall_type)
}

pub fn set_input_fastfall(s: &mut InputSettings, v: u8, player: usize) {
    if let Some(p) = s.players.get_mut(player) {
        p.fastfall_type = v;
    }
}

/// Get-Input-RightStickStyle (0 past the array).
pub fn input_right_stick(s: &InputSettings, player: usize) -> u8 {
    s.players.get(player).map_or(0, |p| p.right_stick_style)
}

pub fn set_input_right_stick(s: &mut InputSettings, v: u8, player: usize) {
    if let Some(p) = s.players.get_mut(player) {
        p.right_stick_style = v;
    }
}

/// Get-Input-Horizontal (`vertical` false) / VerticalDeadzone: the percent / 100 (0.3 past the array).
pub fn input_dead_zone(s: &InputSettings, vertical: bool, player: usize) -> f64 {
    s.players.get(player).map_or(0.3, |p| p.dead_zone()[usize::from(vertical)])
}

/// Set-Input-Horizontal / VerticalDeadzone: Clamp(int, 10, 90).
pub fn set_input_dead_zone(s: &mut InputSettings, vertical: bool, v: f64, player: usize) {
    if let Some(p) = s.players.get_mut(player) {
        let v = (v.trunc() as i32).clamp(10, 90);
        if vertical {
            p.vertical_deadzone = v;
        } else {
            p.horizontal_deadzone = v;
        }
    }
}

/// Get-Input-InputBufferCap: 10000 for a stored value above 30 (31 past the array).
pub fn input_buffer_cap(s: &InputSettings, player: usize) -> i32 {
    s.players.get(player).map_or(31, InputPlayerSettings::buffer_cap)
}

/// Set-Input-InputBufferCap: Clamp(Frames, 0, 30 + 1).
pub fn set_input_buffer_cap(s: &mut InputSettings, v: f64, player: usize) {
    if let Some(p) = s.players.get_mut(player) {
        p.input_buffer_cap = (v.trunc() as i32).clamp(0, 31);
    }
}

/// Set-Input-AllDefaults(Legacy, Player Index), in the build's order (the buffer cap's 10000 stored as its clamp,
/// 31). Not reset: ignoreRightStickDiagonals. (LockMouseToViewport for player 0: not kept.)
pub fn set_input_all_defaults(s: &mut InputSettings, legacy: bool, player: usize) {
    let Some(p) = s.players.get_mut(player) else { return };
    *p = InputPlayerSettings {
        swapped_attack_style_ground: legacy,
        input_buffer_cap: 31,
        ignore_right_stick_diagonals: p.ignore_right_stick_diagonals,
        ..InputPlayerSettings::default()
    };
}

/// A local player's fight-read preferences as the getters give them, for the match set-up (MatchPlayer.input): a cap
/// above 30 is none.
pub fn sim_input_prefs(s: &InputSettings, player: usize) -> mvs_sim::setup::InputPrefs {
    let d = mvs_sim::setup::InputPrefs::default();
    let Some(p) = s.players.get(player) else { return d };
    let cap = p.buffer_cap();
    mvs_sim::setup::InputPrefs {
        fastfall: u8::from(p.fastfall_type == 1),
        double_crouch_jump_down: p.double_crouch_jump_down,
        ignore_right_stick_diagonals: p.ignore_right_stick_diagonals,
        swapped_ground: p.swapped_attack_style_ground,
        swapped_air: p.swapped_attack_style_air,
        input_buffer_cap: if cap <= 30 { u16::try_from(cap).unwrap_or(0) } else { d.input_buffer_cap },
        input_buffer_hold: p.input_buffer_hold,
        attack_picks_up_items: p.attack_picks_up_items,
    }
}

// ---------- the Interface page's UI preferences (Set-UI-*) ----------

/// The pairs of world / hud fields a position row writes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UiElements {
    Percentage,
    Cooldowns,
    Username,
    DodgeMeters,
}

fn element_fields(u: &mut UiSettings, k: UiElements) -> (&mut bool, &mut bool) {
    match k {
        UiElements::Percentage => (&mut u.world_show_percentage, &mut u.hud_show_percentage),
        UiElements::Cooldowns => (&mut u.world_show_cooldowns, &mut u.hud_show_cooldowns),
        UiElements::Username => (&mut u.world_show_username, &mut u.hud_show_username),
        UiElements::DodgeMeters => (&mut u.world_show_dodge_meters, &mut u.hud_show_dodge_meters),
    }
}

/// Set-UI-Show<X>Elements(world, hud).
pub fn set_ui_show_elements(u: &mut UiSettings, k: UiElements, world: bool, hud: bool) {
    let (w, h) = element_fields(u, k);
    *w = world;
    *h = hud;
}

/// Refresh UI Toggles' position mapping: both on -> both, on the fighter -> player, else scoreboard.
pub fn position_id(u: &UiSettings, k: UiElements) -> &'static str {
    let (w, h) = match k {
        UiElements::Percentage => (u.world_show_percentage, u.hud_show_percentage),
        UiElements::Cooldowns => (u.world_show_cooldowns, u.hud_show_cooldowns),
        UiElements::Username => (u.world_show_username, u.hud_show_username),
        UiElements::DodgeMeters => (u.world_show_dodge_meters, u.hud_show_dodge_meters),
    };
    match (w, h) {
        (true, true) => "both",
        (true, false) => "player",
        _ => "scoreboard",
    }
}

/// A position row's item id as the pair it writes (the handlers' NotEqual_StriStri: another id writes nothing).
pub fn position_pair(id: &str) -> Option<(bool, bool)> {
    match id.to_lowercase().as_str() {
        "player" => Some((true, false)),
        "scoreboard" => Some((false, true)),
        "both" => Some((true, true)),
        _ => None,
    }
}

/// The outlines of the page (Set-UI-ToggleWorld<X>Outlines, Set World My Outline).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Outline {
    Ally,
    Enemy,
    My,
}

pub fn outline(u: &UiSettings, who: Outline) -> bool {
    match who {
        Outline::Ally => u.world_show_ally_outlines,
        Outline::Enemy => u.world_show_enemy_outlines,
        Outline::My => u.world_show_my_outline,
    }
}

pub fn set_outline(u: &mut UiSettings, who: Outline, v: bool) {
    match who {
        Outline::Ally => u.world_show_ally_outlines = v,
        Outline::Enemy => u.world_show_enemy_outlines = v,
        Outline::My => u.world_show_my_outline = v,
    }
}

/// Get-UI-Color Binding's index (PaletteEntryType GLOBAL-Ally 0 .. GLOBAL-Team4 4): its CentralPalette row.
pub fn color_binding(u: &UiSettings, b: usize) -> &str {
    match b {
        0 => &u.color_ally,
        1 => &u.color_enemy,
        2 => &u.color_me,
        3 => &u.color_team3,
        _ => &u.color_team4,
    }
}

/// Every colour binding's row, by PaletteEntryType GLOBAL-Ally 0 .. GLOBAL-Team4 4.
pub fn color_rows(u: &UiSettings) -> [&str; 5] {
    std::array::from_fn(|b| color_binding(u, b))
}

/// Set Color Binding(index, row).
pub fn set_color_binding(u: &mut UiSettings, b: usize, row: &str) {
    let f = match b {
        0 => &mut u.color_ally,
        1 => &mut u.color_enemy,
        2 => &mut u.color_me,
        3 => &mut u.color_team3,
        _ => &mut u.color_team4,
    };
    row.clone_into(f);
}

/// Set-UI-CursorSpeedMultiplier: FClamp(v, 0.5, 1.5), into the struct's float32.
pub fn set_cursor_speed(u: &mut UiSettings, v: f64) {
    u.cursor_speed_multiplier = f64::from(v.clamp(0.5, 1.5) as f32);
}

/// Refresh UI Toggles' cursor row (@4042-4225): FTrunc(10 x (cursorSpeedMultiplier - 0.5)) in the Kismet's float32
/// (steps 2, 4 and 8 come back one lower).
pub fn cursor_speed_step(u: &UiSettings) -> f64 {
    let m = u.cursor_speed_multiplier as f32;
    f64::from((10.0f32 * (m - 0.5)).trunc())
}

/// Set-UI-RecommendedPreset (Fn@0..154): names both, the rest on the fighter, the scoreboard on top, the three default
/// colours, DI off. Team3 / Team4 colours and cursor speed are left.
pub fn set_recommended_preset(u: &mut UiSettings) {
    set_ui_show_elements(u, UiElements::Username, true, true);
    set_ui_show_elements(u, UiElements::Cooldowns, true, false);
    set_ui_show_elements(u, UiElements::Percentage, true, false);
    set_ui_show_elements(u, UiElements::DodgeMeters, true, false);
    u.hud_position_bottom = false;
    default_colors(u);
    u.world_show_di = false;
}

/// The Legacy preset (the bar's @16803).
pub fn set_legacy_preset(u: &mut UiSettings) {
    set_ui_show_elements(u, UiElements::Username, true, true);
    set_ui_show_elements(u, UiElements::Cooldowns, false, true);
    set_ui_show_elements(u, UiElements::Percentage, false, true);
    set_ui_show_elements(u, UiElements::DodgeMeters, false, true);
    u.hud_position_bottom = true;
}

/// Colour bindings 0, 1, 2 to PM's Default Color - Allies / Enemies / Me.
pub fn default_colors(u: &mut UiSettings) {
    let d = UiSettings::default();
    for b in 0..3 {
        set_color_binding(u, b, color_binding(&d, b));
    }
}

/// The preset tail @17222: the default colours, every outline and team colours on.
pub fn preset_tail(u: &mut UiSettings) {
    default_colors(u);
    u.world_show_ally_outlines = true;
    u.world_show_enemy_outlines = true;
    u.world_show_my_outline = true;
    u.world_use_team_colored_skins = true;
}

/// RefreshPresets (@18242): Legacy or Recommended when the compared fields match one, else Custom. Not compared: the
/// dodge meters, my outline, the colours, Knockback Influence, cursor speed.
pub fn preset_id(u: &UiSettings) -> &'static str {
    let both = u.world_show_ally_outlines && u.world_show_enemy_outlines && u.world_use_team_colored_skins;
    let legacy = !u.world_show_cooldowns
        && !u.world_show_percentage
        && u.world_show_username
        && u.hud_show_cooldowns
        && u.hud_show_percentage
        && u.hud_show_username
        && u.hud_position_bottom
        && both;
    let recommended = !u.hud_position_bottom
        && !u.hud_show_percentage
        && !u.hud_show_cooldowns
        && u.world_show_percentage
        && u.world_show_cooldowns
        && u.world_show_username
        && u.hud_show_username
        && both;
    if legacy {
        "legacy"
    } else if recommended {
        "recommended"
    } else {
        "custom"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn input_rows_clamp_and_the_sim_prefs() {
        let mut s = InputSettings::default();
        set_input_dead_zone(&mut s, false, 95.7, 1);
        set_input_dead_zone(&mut s, true, 4.0, 1);
        assert_eq!(s.players[1].horizontal_deadzone, 90);
        assert_eq!(input_dead_zone(&s, true, 1), 0.1);
        assert_eq!(input_buffer_cap(&s, 0), 10000);
        set_input_buffer_cap(&mut s, 12.9, 0);
        assert_eq!(input_buffer_cap(&s, 0), 12);
        set_input_fastfall(&mut s, 1, 0);
        set_input_bool(&mut s, InputBool::AttackPicksUpItems, false, 0);
        let p = sim_input_prefs(&s, 0);
        assert_eq!((p.fastfall, p.input_buffer_cap, p.attack_picks_up_items), (1, 12, false));
        // a cap above 30 is none: the default's
        set_input_buffer_cap(&mut s, 40.0, 0);
        assert_eq!(sim_input_prefs(&s, 0).input_buffer_cap, mvs_sim::setup::InputPrefs::default().input_buffer_cap);
        assert_eq!(sim_input_prefs(&InputSettings::default(), 2), mvs_sim::setup::InputPrefs::default());
        set_input_bool(&mut s, InputBool::IgnoreRightStickDiagonals, true, 0);
        set_input_all_defaults(&mut s, true, 0);
        let p = &s.players[0];
        assert!(p.swapped_attack_style_ground && p.ignore_right_stick_diagonals);
        assert_eq!((p.input_buffer_cap, p.fastfall_type), (31, 0));
    }

    #[test]
    fn interface_rows_and_presets() {
        let mut u = UiSettings::default();
        assert_eq!(preset_id(&u), "recommended");
        set_legacy_preset(&mut u);
        assert_eq!(preset_id(&u), "legacy");
        assert_eq!(position_id(&u, UiElements::Cooldowns), "scoreboard");
        assert_eq!(position_id(&u, UiElements::Username), "both");
        let (w, h) = position_pair("Player").expect("an id of the row");
        set_ui_show_elements(&mut u, UiElements::Percentage, w, h);
        assert_eq!(position_id(&u, UiElements::Percentage), "player");
        assert_eq!(position_pair("none"), None);
        assert_eq!(preset_id(&u), "custom");
        set_outline(&mut u, Outline::Ally, false);
        set_color_binding(&mut u, 1, "Green1");
        set_recommended_preset(&mut u);
        preset_tail(&mut u);
        assert_eq!(preset_id(&u), "recommended");
        assert!(outline(&u, Outline::Ally));
        assert_eq!(color_rows(&u), color_rows(&UiSettings::default()));
        // steps 2, 4 and 8 come back one lower (float32)
        set_cursor_speed(&mut u, 0.7);
        assert_eq!(cursor_speed_step(&u), 1.0);
        set_cursor_speed(&mut u, 3.0);
        assert_eq!(u.cursor_speed_multiplier, 1.5);
    }

    fn display(w: u32, h: u32) -> DisplayInfo {
        DisplayInfo { name: "DISPLAY1".into(), size: [w, h], refresh_hz: 144.0, primary: true }
    }

    #[test]
    fn frame_rates_leave_out_the_refresh() {
        let u = FRAME_RATE_UNLIMITED;
        assert_eq!(frame_rates_offered(60.0), [0, 30, 90, 120, 144, 165, 240, u]);
        assert_eq!(frame_rates_offered(143.9), [0, 30, 60, 90, 120, 165, 240, u]);
        assert_eq!(frame_rates_offered(0.0), FRAME_RATE_LIMITS);
        assert_eq!(frame_rate_shown(60, &frame_rates_offered(60.0)), 0);
        assert_eq!(frame_rate_shown(240, &frame_rates_offered(60.0)), 240);
        assert_eq!(frame_rate_label(0, 144.0, "Display Rate", "u"), "144 Hz (Display Rate)");
        assert_eq!(frame_rate_label(60, 144.0, "x", "u"), "60 fps");
        assert_eq!(frame_rate_label(u, 144.0, "x", "Unlimited"), "Unlimited");
    }

    #[test]
    fn resolutions_fit_the_display() {
        let all = [[1024, 576], [1920, 1080], [2560, 1440]];
        assert_eq!(resolution_options(&all, Some(&display(1920, 1080))), [[1024, 576], [1920, 1080]]);
        assert!(resolution_options(&all, None).is_empty());
        let w = WindowInfo { mode: WindowMode::Windowed, size: [1920, 1080], display: 0 };
        assert_eq!(resolution_selected(&[[1024, 576], [1920, 1080]], &w), Some(1));
        let mut s = Settings::default();
        assert!(!pick_resolution(&mut s, [1920, 1080], &w));
        assert!(pick_resolution(&mut s, [1024, 576], &w));
        assert_eq!((s.window.width, s.window.height), (1024, 576));
    }

    #[test]
    fn windowed_pick_sets_the_size() {
        let mut s = Settings::default();
        assert!(!pick_window_mode(&mut s, WindowMode::WindowedFullscreen));
        assert!(pick_window_mode(&mut s, WindowMode::Windowed));
        assert_eq!((s.window.width, s.window.height), (1024, 576));
        assert_eq!(mode_of_id(mode_id(WindowMode::WindowedFullscreen)), Some(WindowMode::WindowedFullscreen));
    }

    #[test]
    fn scale_ids_round_trip() {
        let mut g = GraphicsSettings::default();
        assert_eq!(resolution_scale_id(&g, 1080.0).as_deref(), Some("100"));
        pick_resolution_scale(&mut g, "50", 1080.0);
        assert_eq!(resolution_scale_id(&g, 1080.0).as_deref(), Some("50"));
        assert!(resolution_scale_id(&g, 360.0).is_none());
    }

    #[test]
    fn volumes_clamp_and_texture_reports_a_change() {
        let mut a = AudioSettings::default();
        set_volume(&mut a, 2, 1.7);
        set_volume(&mut a, 4, -1.0);
        assert_eq!((volume(&a, 2), volume(&a, 4)), (1.0, 0.0));
        assert_eq!(volume_row("UI_MusicVolume"), Some(2));
        let mut g = GraphicsSettings::default();
        assert!(!pick_texture(&mut g, 3));
        assert!(pick_texture(&mut g, 1));
        pick_anti_aliasing(&mut g, 9);
        assert_eq!(g.anti_aliasing_quality, 4);
    }
}
