//! The pictures a menu screen holds ahead for the screen one step away (the port's own).
//!
//! A picture fetched when a paint first asks for it comes in after the screen's first frames, so a
//! screen holds what the screen one step away draws and lets it go when it goes itself. The native renderer loads a
//! texture from the store when a draw first names it; `Pictures` is the list of what the screens hold (by owner)
//! and of the soft textures asked for, which the renderer loads ahead and keeps while held. The owners and the
//! lists follow what each screen draws (see each constant).

use indexmap::{IndexMap, IndexSet};

use super::view::MenuView;

/// The Lab, for its life: the pause menu's cards, perk rows and map picture (`panel_pictures`).
pub const PICTURES_PANEL: &str = "lab";
/// The Collection's portraits (the main menu's sidebar button).
pub const PICTURES_HOME: &str = "home";
/// The cards of the tab Match Options opens on (the PLAY button).
pub const PICTURES_MATCH_OPTIONS: &str = "home:matchoptions";
/// The main menu's promo thumbnails, held before its first frame until it goes.
pub const PICTURES_MAIN_MENU: &str = "mainmenu";
/// The pause menu while it is up: the tiles of its character and map selectors.
pub const PICTURES_PICKERS: &str = "lab:pickers";
/// A character selector's focused fighter's skins.
pub const PICTURES_SKINS: &str = "skins";
/// The Lab's perk page while it is up.
pub const PICTURES_LAB_PERKS: &str = "lab:perks";
/// A map's picture on a tile: about a quarter of the screen's width (the 256 pixel mip of a 2048 picture on a 1280
/// pixel screen).
pub const MAP_TILE: f64 = 0.25;

/// What the view's screens hold, by owner, and the soft textures asked for.
#[derive(Debug, Default)]
pub struct Pictures {
    held: IndexMap<String, IndexSet<String>>,
    wanted: IndexSet<String>,
    /// Counts the changes: the renderer looks again when it moved.
    pub version: u64,
}

impl Pictures {
    /// `owner` holds `names` (over what it held).
    pub fn hold<'a>(&mut self, owner: &str, names: impl IntoIterator<Item = &'a str>) {
        let set: IndexSet<String> = names.into_iter().filter(|n| !n.is_empty()).map(str::to_string).collect();
        self.held.insert(owner.to_string(), set);
        self.version += 1;
    }

    /// `owner` lets go of what it held.
    pub fn release(&mut self, owner: &str) {
        if self.held.shift_remove(owner).is_some() {
            self.version += 1;
        }
    }

    /// Every owner lets go (the view's screen is gone).
    pub fn release_all(&mut self) {
        if !self.held.is_empty() {
            self.held.clear();
            self.version += 1;
        }
    }

    /// A soft texture asked for (loadTexture).
    pub fn want(&mut self, name: &str) {
        if self.wanted.insert(name.to_string()) {
            self.version += 1;
        }
    }

    /// Every picture held or asked for, each once.
    pub fn all(&self) -> impl Iterator<Item = &str> {
        let mut seen = IndexSet::new();
        for s in self.held.values() {
            for n in s {
                seen.insert(n.as_str());
            }
        }
        for n in &self.wanted {
            seen.insert(n.as_str());
        }
        seen.into_iter()
    }

    /// What `owner` holds.
    pub fn of(&self, owner: &str) -> Option<&IndexSet<String>> {
        self.held.get(owner)
    }
}

/// UI_LAB_PlayerPanel_Character UpdateAppearance, WBP_CustomGamePlayer's icon: the skin's icon material (the
/// fighter's default skin's for a skin it lacks).
pub fn skin_icon(view: &MenuView, character: Option<&str>, skin: &str) -> Option<String> {
    let f = view.fighters().get(character?)?;
    // the name compared without case; '' names none
    let by_name = (!skin.is_empty()).then(|| f.skins.iter().find(|s| s.name.eq_ignore_ascii_case(skin))).flatten();
    Some(by_name.map_or_else(|| f.skin.icon.clone(), |s| s.icon.clone()))
}

/// A fighter's skins' icons (the skin selector's tiles).
pub fn skin_pictures(view: &MenuView, character: Option<&str>) -> Vec<String> {
    character
        .and_then(|c| view.fighters().get(c))
        .map_or_else(Vec::new, |f| f.skins.iter().map(|s| s.icon.clone()).collect())
}

/// The roster's portraits (a character selector's tiles).
pub fn roster_pictures(view: &MenuView) -> Vec<String> {
    view.roster().iter().map(|r| r.portrait.clone()).collect()
}

/// The maps' pictures (a map selector's tiles; None for a map the bundle lacks).
pub fn map_pictures(view: &MenuView, maps: &[&str]) -> Vec<Option<String>> {
    maps.iter().map(|n| view.map(n).map(|m| m.data.texture)).collect()
}

/// The pause menu's panels: each occupied slot's card and its perk row, and the map's picture. `perk_list` is
/// the perk page's list of pictures: a loadout's icons and the category backings.
pub fn panel_pictures(
    view: &MenuView,
    c: &super::training_config::LabConfig,
    perk_list: &dyn Fn(&[Option<String>]) -> Vec<String>,
) -> Vec<Option<String>> {
    let used: Vec<_> = c.players.iter().filter(|p| !p.account_id.is_empty()).collect();
    let mut out: Vec<Option<String>> = used.iter().map(|p| skin_icon(view, p.character.as_deref(), &p.skin)).collect();
    let perks: Vec<Option<String>> = used.iter().flat_map(|p| p.perks.iter().cloned()).collect();
    out.extend(perk_list(&perks).into_iter().map(Some));
    out.extend(map_pictures(view, &[c.selected_map.as_str()]));
    out
}
