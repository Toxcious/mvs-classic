//! What the HUD's widgets share: the broker's player data, the UI preferences, the palette and the widget
//! animations' clocks.

use mvs_sim::World;
use mvs_sim::fighter::FighterId;

use crate::bundle::{AnimData, Bundle, Rgba};
use crate::slate::{Tree, Vis, WidgetId, apply_anim};
use crate::text::GameText;

/// UI_IGv3_WidgetLocation Attached (NewEnumerator0 = 1) and Fixed (NewEnumerator1 = 2); UnifyChildren copies it to
/// every broker-aware child.
pub const LOCATION_ATTACHED: u8 = 1;
pub const LOCATION_FIXED: u8 = 2;

/// UI_Broker_PlayerData as the broker holds it for a player.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct BrokerPlayer {
    pub is_me: bool,
    pub is_bot: bool,
    pub is_local: bool,
    pub is_ally: bool,
    pub team: Rgba,
    pub focus: Rgba,
    pub username: String,
    /// UI_Broker SetCharacterForPlayer: the character data's CharacterSelectPortraitMaterial (None: the portrait
    /// stays Hidden).
    pub portrait: Option<String>,
    /// The widgets' Player Index (the broker's: the fighter's).
    pub index: usize,
}

/// How a HUD part finds a player index's fighter and a fighter's player index: a player's index is its fighter id.
/// (The port's own Waves run, where every fighter is a mode pawn: by the pawns' bound slots, and only the local
/// player's slot stands for one fighter; the enemies share the other slot and none of them stands for it.)
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct PlayerMap {
    /// A Waves run: the local player's index.
    waves: Option<usize>,
}

impl PlayerMap {
    pub(crate) fn of(setup: &super::HudSetup) -> PlayerMap {
        PlayerMap { waves: (setup.mode == super::MODE_WAVES).then_some(setup.me) }
    }

    /// The fighter of player index `i` (None: none stands for it).
    pub(crate) fn fighter(self, w: &World, i: usize) -> Option<FighterId> {
        let Some(me) = self.waves else { return w.players.get(i).copied() };
        let slot = i32::try_from(i).ok().filter(|_| i == me)?;
        w.all_ids().into_iter().find(|&f| w.fighter(f).life.slot == Some(slot))
    }

    /// The player index of a fighter (None: a minion, an unbound pawn).
    pub(crate) fn index(self, w: &World, f: FighterId) -> Option<usize> {
        if self.waves.is_some() {
            return usize::try_from(mvs_sim::world::pawns::slot_of(w, f)).ok();
        }
        usize::try_from(w.fighter(f).id).ok()
    }
}

/// Struct_Preferences_UI: the fields the match widgets' RespondToPreferences read.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Prefs {
    pub world_show_percentage: bool,
    pub world_show_cooldowns: bool,
    pub world_show_username: bool,
    pub world_show_dodge_meters: bool,
    pub hud_show_percentage: bool,
    pub hud_show_cooldowns: bool,
    pub hud_show_username: bool,
    pub hud_show_dodge_meters: bool,
    pub hud_position_bottom: bool,
}

impl Prefs {
    /// The saved preferences' fields.
    // Deviation: read at the match's start: PM.OnUIPrefsChanged does not reach the match's widgets (the
    // RespondToPreferences of a change made in the pause menu's settings is not ported; it shows from the next match).
    pub fn of(u: &mvs_settings::UiSettings) -> Prefs {
        Prefs {
            world_show_percentage: u.world_show_percentage,
            world_show_cooldowns: u.world_show_cooldowns,
            world_show_username: u.world_show_username,
            world_show_dodge_meters: u.world_show_dodge_meters,
            hud_show_percentage: u.hud_show_percentage,
            hud_show_cooldowns: u.hud_show_cooldowns,
            hud_show_username: u.hud_show_username,
            hud_show_dodge_meters: u.hud_show_dodge_meters,
            hud_position_bottom: u.hud_position_bottom,
        }
    }
}

/// The object path of the brush resource the bundle names `name`: a UI material, else a texture's entry
/// (slate.rs resolves a design-time brush the same way); None: the bundle has neither.
pub fn brush_path(b: &Bundle, name: &str) -> Option<String> {
    b.material_path(name).map(str::to_string).or_else(|| b.textures.get(name).map(|t| t.entry().to_string()))
}

/// UI_Functions PaletteForIndex(index).UI over the bundle's CentralPalette rows through the colour bindings `bound`
/// (mvs_view::skins::palette_for_index_in).
pub fn palette_ui(b: &Bundle, index: usize, bound: [&str; 5]) -> Rgba {
    mvs_view::skins::palette_for_index_in(&b.palette, index, bound)
}

/// UI_Functions GetLocalPlayUsername's PlayerNumberLong: Format of the EX_TextConst @406 (namespace "", this key),
/// argument `number`.
const PLAYER_NUMBER_LONG: &str = "22DA84EB4E553B31132B10B58E0277B4";
/// PlayerNumberShort: Format of the EX_TextConst @765, an invariant text (no key, no Game.locres line): its source
/// string, argument `player`.
const PLAYER_NUMBER_SHORT: &str = "P{player}";

/// UI_Functions GetLocalPlayUsername(Player Index)'s outputs.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct LocalPlayName {
    pub username: String,
    /// PlayerNumberShort
    pub short: String,
    /// PlayerNumberLong
    pub long: String,
    /// Custom Name Set
    pub custom: bool,
}

/// GetLocalPlayUsername's format texts, read from the player's Game.locres once.
#[derive(Clone, Debug, Default)]
pub struct PlayerNumbers {
    long: String,
    /// The Local Play settings' usernames by player index (Get-LocalPlay-SettingsForPlayer); a player past the
    /// list has an empty one.
    // Unverified: the native game has no Local Play settings yet: the list is empty.
    settings: Vec<String>,
}

impl PlayerNumbers {
    pub fn new(text: &GameText) -> PlayerNumbers {
        PlayerNumbers { long: text.get("", PLAYER_NUMBER_LONG).unwrap_or_default().to_string(), settings: Vec::new() }
    }

    /// GetLocalPlayUsername(Player Index) (@54-1045): both numbers are Player Index + 1; Username = the settings
    /// username, or PlayerNumberLong when it is empty (@920); Custom Name Set = the username is not empty and not
    /// PlayerNumberShort (@882).
    pub fn local_play_username(&self, player: usize) -> LocalPlayName {
        let n = (player + 1).to_string();
        let settings = self.settings.get(player).map_or("", String::as_str);
        let long = self.long.replace("{number}", &n);
        let short = PLAYER_NUMBER_SHORT.replace("{player}", &n);
        LocalPlayName {
            username: if settings.is_empty() { long.clone() } else { settings.to_string() },
            custom: !settings.is_empty() && settings != short,
            short,
            long,
        }
    }
}

/// UI_Functions Conditional Visibility.
pub fn vis_if(b: bool, t: Vis, f: Vis) -> Vis {
    if b { t } else { f }
}

/// A visibility that paints (Widget.shown).
pub fn shown(v: Vis) -> bool {
    v != Vis::Collapsed && v != Vis::Hidden
}

pub fn lerp_color(a: Rgba, b: Rgba, t: f64) -> Rgba {
    [0, 1, 2, 3].map(|i| a[i] + (b[i] - a[i]) * t)
}

/// A colour as a material parameter.
pub fn c4(c: Rgba) -> [f32; 4] {
    c.map(|x| x as f32)
}

pub fn clamp01(x: f64) -> f64 {
    x.clamp(0.0, 1.0)
}

/// A widget animation of a class, held by the HUD (the bundle is not kept past `Hud::new`).
#[derive(Clone, Debug, Default)]
pub struct Anim(pub AnimData);

impl Anim {
    pub fn of(b: &Bundle, class: &str, name: &str) -> Anim {
        Anim(b.anim(class, name).clone())
    }

    /// The last valid frame's time, seconds: where a one-shot player ends (UUMGSequencePlayer::Tick evaluates
    /// Duration - 1 tick and finishes).
    pub fn end(&self) -> f64 {
        (self.0.range[1] - 1.0 - self.0.range[0]) / self.0.res
    }

    /// A looping player's period, seconds.
    pub fn period(&self) -> f64 {
        (self.0.range[1] - self.0.range[0]) / self.0.res
    }

    /// The tracks at `sec` (the events since `from` are the caller's).
    pub fn apply(&self, t: &mut Tree, owner: WidgetId, sec: f64, from: f64) {
        apply_anim(t, owner, &self.0, sec, from);
    }

    pub fn at(&self, t: &mut Tree, owner: WidgetId, sec: f64) {
        apply_anim(t, owner, &self.0, sec, sec);
    }
}

/// `n` as Conv_IntToText with grouping puts it (FText::AsNumber, en: `toLocaleString('en-US')` of an integer).
pub fn grouped(n: i64) -> String {
    let s = n.unsigned_abs().to_string();
    let mut out = String::with_capacity(s.len() + s.len() / 3 + 1);
    if n < 0 {
        out.push('-');
    }
    for (i, ch) in s.chars().enumerate() {
        if i > 0 && (s.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(ch);
    }
    out
}

/// A number as JavaScript's String(n) writes it for the values the HUD shows (whole numbers, short fractions).
pub fn js_num(n: f64) -> String {
    if n.fract() == 0.0 && n.abs() < 1e21 { format!("{}", n as i64) } else { format!("{n}") }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn grouping() {
        assert_eq!(grouped(0), "0");
        assert_eq!(grouped(999), "999");
        assert_eq!(grouped(1000), "1,000");
        assert_eq!(grouped(1234567), "1,234,567");
        assert_eq!(grouped(-1200), "-1,200");
    }

    #[test]
    fn local_play_names() {
        let n = PlayerNumbers { long: "Player {number}".into(), settings: Vec::new() }.local_play_username(1);
        assert_eq!(
            (n.username.as_str(), n.short.as_str(), n.long.as_str(), n.custom),
            ("Player 2", "P2", "Player 2", false)
        );
    }
}
