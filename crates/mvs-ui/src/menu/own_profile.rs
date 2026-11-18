//! The player's own profile: the profile page's data, standing in for the build's GameProfileManager.GetPlayerProfile,
//! and the equipped profile icon with the brushes of the sites that draw it.
//!
//! The own profile comes from the stats file (`Session::stats`): the online matches' records and totals the player's
//! own game kept. The build keeps the icon on the Hydra server: GameProfileManager GetMyProfileIcon reads the
//! logged-in profile's ProfileIcon {Slug, AssetPath}, SetMyProfileIcon sends set_profile_icon, and offline the icon is
//! empty, so every site shows its tree's default (MI_Profile's PFG_Dude). The port keeps it in its Hydra settings
//! (`menu::hydra`, seeded by SetDefaultProfileIcon when the catalogue is in).
//! Deviation (the port shows the online layout, as the nav's profile slot): the icon is drawn offline too.
//!
//! Not here: another player's profile (profile.get / profile.ask, the net process) and the stats file's `add`.

use mvs_protocol::profile::ProfileWire;
use mvs_settings::stats::{StatsProfile, StatsReset};

use super::controller::Cx;
use super::hydra::{cosmetic_by_slug, cosmetics, picture_name};
use super::lobby::loadout::asset_name;
use super::view::MenuView;
use crate::bundle::CosmeticItem;
use crate::slate::Mid;

/// The id the records' own player takes: the port has no account id offline.
pub const OWN_ID: &str = "";

/// A profile with nothing in it (the build's class default PlayerProfile under the player's own account).
pub fn empty_profile(id: &str, name: &str) -> ProfileWire {
    ProfileWire {
        id: id.to_string(),
        name: name.to_string(),
        icon: String::new(),
        totals: mvs_protocol::profile::ProfileTotals { results: Vec::new(), ..Default::default() },
        trackers: Default::default(),
        matches: Vec::new(),
    }
}

/// GetMyProfileIcon: the stored icon's catalogue item (None: none stored, or the catalogue not in).
pub fn my_profile_icon(cx: &Cx<'_>) -> Option<CosmeticItem> {
    let slug = &cx.prefs.hydra.profile_icon.slug;
    if slug.is_empty() {
        return None;
    }
    cosmetic_by_slug(cosmetics(cx.view)?, slug).cloned()
}

/// Its ProfileIconData asset name, as the lobby and the online list carry it ("" none).
pub fn my_profile_icon_asset(h: &mvs_settings::HydraSettings) -> String {
    let r = &h.profile_icon;
    if r.slug.is_empty() { String::new() } else { asset_name(&r.path).to_string() }
}

/// A ProfileIconData by its asset name (another player's ProfileIconPath; None: none, or not in the catalogue).
pub fn profile_icon_by_asset(view: &MenuView, asset: &str) -> Option<CosmeticItem> {
    if asset.is_empty() {
        return None;
    }
    cosmetics(view)?.profile_icon.iter().find(|e| asset_name(&e.path) == asset).cloned()
}

/// Whose icon a pre-match banner draws.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BannerIconOf<'a> {
    /// The local player's own: the equipped ProfileIcon's slug.
    Slug(&'a str),
    /// Another human's (the server's ProfileIconPath asset name).
    Asset(&'a str),
}

/// The profile icon a pre-match banner's nameplate draws (BAN Set Profile Icon): the ProfileIconData's TextureRef as
/// the store texture path `texture` finds for its picture name (None: no icon, not in the catalogue or no such
/// texture: the nameplate's own picture stays). A bot's drawn icon is its slug, as the custom lobby keeps it.
pub fn banner_icon_texture(
    c: &crate::bundle::Cosmetics,
    of: BannerIconOf<'_>,
    texture: impl Fn(&str) -> Option<String>,
) -> Option<String> {
    let item = match of {
        BannerIconOf::Slug(s) if !s.is_empty() => c.profile_icon.iter().find(|e| e.slug == s),
        BannerIconOf::Asset(a) if !a.is_empty() => c.profile_icon.iter().find(|e| asset_name(&e.path) == a),
        _ => None,
    }?;
    texture(&picture_name(item.icon.as_deref()?)?)
}

/// watchProfileIcon's catalogue part: the catalogue's arrival runs the SetDefault* rules (SetDefaultProfileIcon
/// seeds an empty icon). The rules gate themselves once per run, so every site may call it. False: the bundle has
/// no catalogue (the sites keep their trees' icons).
pub fn watch_profile_icon(cx: &mut Cx<'_>) -> bool {
    let Some(pool) = cosmetics(cx.view).map(|c| c.defaults.profile_icon_pool.len()) else { return false };
    // the shuffle's random source: one number per swap (pool - 1), from the menus' (the manager's) source, drawn
    // only when SetDefaultProfileIcon shuffles (its DoOnce open and the icon empty: once per run)
    let h = &cx.prefs.hydra;
    let n =
        if !h.defaults_run.profile_icon && super::hydra::profile_icon_empty(h) { pool.saturating_sub(1) } else { 0 };
    let mut draws: Vec<f64> = (0..n).map(|_| cx.view.manager.random()).collect();
    draws.reverse();
    let Some(c) = cosmetics(cx.view) else { return false };
    super::hydra::load_defaults(&mut cx.prefs.hydra, c, &mut || draws.pop().unwrap_or(0.0));
    true
}

/// UI_PartyMemberSlot Set Cover's load (@12368-12601): ProfileIcon.GetDynamicMaterial().SetTextureParameterValue
/// (Texture, the ProfileIconData's TextureRef) on MI_Profile (M_CharPortraits). `icon`: the item's `icon` path.
/// None: the bundle has no such texture or material.
pub fn profile_icon_material(view: &MenuView, icon: Option<&str>) -> Option<(String, Mid)> {
    let tex = picture_name(icon?)?;
    let b = &view.bundle;
    let entry = b.textures.get(&tex)?.entry().to_string();
    let path = b.material_path("MI_Profile")?.to_string();
    let mut mid = Mid::of(&path);
    mid.set_texture("Texture", &entry);
    Some((path, mid))
}

/// The own profile as the stats file has it, under the player's name and icon. A file that
/// failed its check (`StatsProfile::reset`, said once) is only logged: the build's text for it (a save file that
/// could not be loaded) does not say what happened here, and the page types no text of its own.
pub fn load_own_profile(cx: &mut Cx<'_>) -> ProfileWire {
    let s: StatsProfile = cx.view.session.stats.profile(OWN_ID);
    if let Some(r) = s.reset {
        let how = match r {
            StatsReset::Bad => "its last good copy is used",
            StatsReset::Empty => "the stats start again",
        };
        eprintln!("profile: the stats file failed its check: {how}");
    }
    ProfileWire {
        id: OWN_ID.to_string(),
        name: cx.prefs.profile.name.clone(),
        icon: my_profile_icon_asset(&cx.prefs.hydra),
        totals: s.totals,
        trackers: s.trackers,
        matches: s.matches,
    }
}

/// The account level table (PGI_GlobalData AccountLevelUpTable; none: the bundle has no menu data).
pub fn account_levels(view: &MenuView) -> &[crate::bundle::AccountLevelRow] {
    view.bundle.menu.as_ref().map_or(&[], |m| m.account_levels.as_slice())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bundle::{CosmeticItem, Cosmetics};

    fn icon(slug: &str) -> CosmeticItem {
        CosmeticItem {
            slug: slug.into(),
            path: format!("/Game/X/PI_{slug}.PI_{slug}"),
            icon: Some(format!("/Game/UI/T_{slug}.T_{slug}")),
            ..CosmeticItem::default()
        }
    }

    fn tex(name: &str) -> Option<String> {
        name.strip_prefix("T_").map(|s| format!("store/{s}"))
    }

    /// A human draws the icon it equipped (its slug, or the asset the server handed on), a bot the one it was
    /// drawn, and a player or bot with none keeps the nameplate's own.
    #[test]
    fn banner_icon_by_who() {
        let c = Cosmetics { profile_icon: vec![icon("a"), icon("b")], ..Cosmetics::default() };
        let of = |of| banner_icon_texture(&c, of, tex);
        assert_eq!(of(BannerIconOf::Slug("b")).as_deref(), Some("store/b"));
        assert_eq!(of(BannerIconOf::Asset("PI_a")).as_deref(), Some("store/a"));
        assert_eq!(of(BannerIconOf::Slug("")), None);
        assert_eq!(of(BannerIconOf::Slug("nope")), None);
        assert_eq!(of(BannerIconOf::Asset("")), None);
    }
}
