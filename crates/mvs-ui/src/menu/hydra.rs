//! The Hydra preferences: what the Collection
//! equips and the match reads, with the cosmetics catalogue the choosers list (the menu bundle's) and the SetDefault*
//! rules. The preferences themselves are `mvs_settings::HydraSettings` (the settings file): the taunt and skin
//! setters are there, the rest here.
//!
//! Deviation: the build keeps these on the Hydra server; the port keeps them in its settings file, and the profile
//! icon too (GameProfileManager set_profile_icon is server-held in the build).
//!
//! No change listeners: a widget that shows a preference compares it in its tick.

use mvs_settings::{AssetRef, HydraSettings};

use crate::bundle::{CosmeticItem, Cosmetics};
use crate::menu::view::MenuView;

/// The catalogue of the menu bundle; None for a bundle without one (the hud bundle, a store imported before it).
pub fn cosmetics(view: &MenuView) -> Option<&Cosmetics> {
    cosmetics_in(&view.bundle)
}

/// `cosmetics` of a bundle (a caller that holds the bundle's Arc while it changes the view).
pub fn cosmetics_in(b: &crate::Bundle) -> Option<&Cosmetics> {
    let c = &b.menu.as_ref()?.cosmetics;
    (!c.lists().iter().all(|l| l.is_empty()) || !c.characters.is_empty()).then_some(c)
}

/// A catalogue item by slug: taunts first, as PGI_GlobalData GetTaunts lists them before the other classes, then
/// the stickers, ring-outs, announcers, banners, profile icons and badges (the first item of a slug wins).
pub fn cosmetic_by_slug<'a>(c: &'a Cosmetics, slug: &str) -> Option<&'a CosmeticItem> {
    [&c.taunt, &c.emote, &c.ringout, &c.announcer, &c.banner, &c.profile_icon, &c.stat_tracker]
        .into_iter()
        .flat_map(|l| l.iter())
        .find(|e| e.slug == slug)
}

/// An item as the preferences hold it: {Slug, AssetPath}.
pub fn ref_of(e: &CosmeticItem) -> AssetRef {
    AssetRef { slug: e.slug.clone(), path: e.path.clone() }
}

/// Deviation: every item is owned: BPI_IsAssetOwnedBySlug? / IsOwned? answer Owned true for any slug, so Change Any
/// Unowned Selections to Default never resets a slot and no tile is locked.
pub fn is_cosmetic_owned(_slug: &str) -> bool {
    true
}

/// A catalogue picture's name in the menu bundle: the asset name of a build path (`/Game/A/T_X.T_X` -> `T_X`,
/// `/Game/A/B.B:Sub` -> `B`); None for an empty one.
pub fn picture_name(path: &str) -> Option<String> {
    let tail = path.rsplit('/').next().unwrap_or(path);
    let name = tail.split(['.', ':']).next().unwrap_or(tail);
    (!name.is_empty()).then(|| name.to_string())
}

/// GlobalEquippables' single slots.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GlobalKey {
    RingOutVfx,
    AnnouncerPack,
    Banner,
}

/// Set Ring Out Vfx / SetAnnouncerPack / Set Banner.
pub fn set_global_equippable(h: &mut HydraSettings, key: GlobalKey, r: AssetRef) {
    let slot = match key {
        GlobalKey::RingOutVfx => &mut h.ring_out_vfx,
        GlobalKey::AnnouncerPack => &mut h.announcer_pack,
        GlobalKey::Banner => &mut h.banner,
    };
    *slot = r;
}

/// SetStatTracker(data, slot 0-2): another slot is refused.
pub fn set_stat_tracker(h: &mut HydraSettings, slot: usize, r: AssetRef) {
    if let Some(s) = h.stat_trackers.get_mut(slot) {
        *s = r;
    }
}

/// GameProfileManager SetMyProfileIcon(slug, path).
pub fn set_profile_icon(h: &mut HydraSettings, r: AssetRef) {
    h.profile_icon = r;
}

// The SetDefault* rules: with force they write; without, once each in a run (a DoOnce, `HydraSettings::defaults_run`)
// and only into an empty slot, setting DefaultsAdjusted.

/// A gate of `defaults_run`: with force, open; else the first call passes the DoOnce (DefaultsAdjusted set) and
/// writes only into an empty slot.
fn default_gate(gate: &mut bool, adjusted: &mut bool, force: bool, empty: bool) -> bool {
    if force {
        return true;
    }
    if *gate {
        return false;
    }
    *gate = true;
    *adjusted = true;
    empty
}

fn default_ref(c: &Cosmetics, slug: &str) -> Option<AssetRef> {
    cosmetic_by_slug(c, slug).map(ref_of)
}

/// ROV_Default (PreferencesManager @10282).
pub fn set_default_ring_out(h: &mut HydraSettings, c: &Cosmetics, force: bool) {
    let Some(r) = default_ref(c, &c.defaults.ringout) else { return };
    let d = &mut h.defaults_run;
    if default_gate(&mut d.ring_out, &mut d.adjusted, force, h.ring_out_vfx.slug.is_empty()) {
        set_global_equippable(h, GlobalKey::RingOutVfx, r);
    }
}

/// DefaultAnnouncerPack (@8944).
pub fn set_default_announcer(h: &mut HydraSettings, c: &Cosmetics, force: bool) {
    let Some(r) = default_ref(c, &c.defaults.announcer) else { return };
    let d = &mut h.defaults_run;
    if default_gate(&mut d.announcer, &mut d.adjusted, force, h.announcer_pack.slug.is_empty()) {
        set_global_equippable(h, GlobalKey::AnnouncerPack, r);
    }
}

/// Banner_NewDefault (@8631).
pub fn set_default_banner(h: &mut HydraSettings, c: &Cosmetics, force: bool) {
    let Some(r) = default_ref(c, &c.defaults.banner) else { return };
    let d = &mut h.defaults_run;
    if default_gate(&mut d.banner, &mut d.adjusted, force, h.banner.slug.is_empty()) {
        set_global_equippable(h, GlobalKey::Banner, r);
    }
}

/// StatTrackingBundle_Default into slot 0 only (@3215).
pub fn set_default_stat_tracker(h: &mut HydraSettings, c: &Cosmetics, force: bool) {
    let Some(r) = default_ref(c, &c.defaults.stat_tracker) else { return };
    let d = &mut h.defaults_run;
    if default_gate(&mut d.stat_tracker, &mut d.adjusted, force, h.stat_trackers[0].slug.is_empty()) {
        set_stat_tracker(h, 0, r);
    }
}

/// The icon SetDefaultProfileIcon replaces: none, or the gizmo one (EqualEqual_StriStri @9882: without case).
pub fn profile_icon_empty(h: &HydraSettings) -> bool {
    let cur = &h.profile_icon.slug;
    cur.is_empty() || cur.eq_ignore_ascii_case("profile_icon_amb_gre_gizmo_1")
}

/// SetDefaultProfileIcon (@9347-10008): when the icon is empty or profile_icon_amb_gre_gizmo_1 (EqualEqual_StriStri
/// @9882: compared without case), Get Rand Default Profile Icon (@0-846: PGI_GlobalData.ProfileIcons shuffled, the
/// first IsUnlockedByDefault one, else ProfileIcon_Default) through SetMyProfileIcon. `random`: Array_Shuffle's
/// source, [0, 1) (menu state, no sim: the manager's).
pub fn set_default_profile_icon(h: &mut HydraSettings, c: &Cosmetics, force: bool, random: &mut dyn FnMut() -> f64) {
    let empty = profile_icon_empty(h);
    let d = &mut h.defaults_run;
    if !default_gate(&mut d.profile_icon, &mut d.adjusted, force, empty) {
        return;
    }
    let mut pool = c.defaults.profile_icon_pool.clone();
    for i in (1..pool.len()).rev() {
        // Math.floor(Math.random() * (i + 1))
        let j = ((random() * (i + 1) as f64).floor() as usize).min(i);
        pool.swap(i, j);
    }
    let pick = pool
        .iter()
        .filter_map(|s| cosmetic_by_slug(c, s))
        .find(|e| e.def)
        .or_else(|| cosmetic_by_slug(c, &c.defaults.profile_icon));
    if let Some(e) = pick {
        set_profile_icon(h, ref_of(e));
    }
}

/// SetDefaultTaunts(force) (@4209-7935): once without force (DoOnce, DefaultsAdjusted). For each character of the
/// catalogue (the port's: GlobalCharacterData characters the port has not got have no taunts to name) with a
/// CharacterPreferences entry, slots 0..(force ? 3 : 0) take GetFirstDefaultTaunt when the slot is missing, empty or
/// emote_global, or when forced; without an entry, slot 0 takes it. SetUpCharacterDefaults is SetDefaultTaunts(false)
/// (PreferencesManager @43; UI_MainMenu @76, PandaGameInstance @534 and UI_PreMatch @60937 call it too: the
/// once-gate makes those no-ops after the first).
pub fn set_default_taunts(h: &mut HydraSettings, c: &Cosmetics, force: bool) {
    if !force {
        if h.defaults_run.taunts {
            return;
        }
        h.defaults_run.taunts = true;
    }
    h.defaults_run.adjusted = true;
    for (slug, ch) in &c.characters {
        let Some(entry) = h.characters.get(slug) else {
            h.set_character_taunt(slug, 0, ch.default_taunt.clone());
            continue;
        };
        let last = if force { 3 } else { 0 };
        let fill: Vec<usize> = (0..=last)
            .filter(|&i| {
                let s = entry.taunts.get(i).map_or("", |t| t.slug.as_str());
                force || s.is_empty() || s == "emote_global"
            })
            .collect();
        for i in fill {
            h.set_character_taunt(slug, i, ch.default_taunt.clone());
        }
    }
}

/// The defaults as the build's local preferences load runs them (PreferencesManager @10096: SetDefaultRingOut /
/// Announcer / StatTracker / Banner(false), SetUpCharacterDefaults, SetDefaultProfileIcon(false)) -> DefaultsAdjusted
/// (SaveAllPreferences then: the caller saves the settings).
/// (the port's own) the build runs them when the preferences load (@10188); the port when a screen first needs the
/// catalogue they name, before it lists it.
pub fn load_defaults(h: &mut HydraSettings, c: &Cosmetics, random: &mut dyn FnMut() -> f64) -> bool {
    set_default_ring_out(h, c, false);
    set_default_announcer(h, c, false);
    set_default_stat_tracker(h, c, false);
    set_default_banner(h, c, false);
    set_default_taunts(h, c, false);
    set_default_profile_icon(h, c, false, random);
    h.defaults_run.adjusted
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bundle::{CosCharacter, CosDefaults};

    fn item(slug: &str, def: bool) -> CosmeticItem {
        CosmeticItem { slug: slug.into(), path: format!("/Game/X/{slug}.{slug}"), def, ..CosmeticItem::default() }
    }

    fn catalogue() -> Cosmetics {
        let mut c = Cosmetics {
            ringout: vec![item("rov_default", true)],
            banner: vec![item("banner_default", true)],
            profile_icon: vec![item("icon_a", false), item("icon_b", true), item("icon_default", true)],
            taunt: vec![item("emote_global", true), item("taunt_shaggy", true)],
            defaults: CosDefaults {
                ringout: "rov_default".into(),
                banner: "banner_default".into(),
                announcer: "none".into(),
                profile_icon: "icon_default".into(),
                profile_icon_pool: vec!["icon_a".into(), "icon_b".into()],
                ..CosDefaults::default()
            },
            ..Cosmetics::default()
        };
        c.characters.insert(
            "character_shaggy".into(),
            CosCharacter {
                key: "shaggy".into(),
                default_taunt: ref_of(&item("taunt_shaggy", true)),
                ..CosCharacter::default()
            },
        );
        c
    }

    #[test]
    fn the_load_fills_empty_slots_once() {
        let c = catalogue();
        let mut h = HydraSettings {
            banner: AssetRef { slug: "mine".into(), path: "/Game/B.B".into() },
            ..HydraSettings::default()
        };
        assert!(load_defaults(&mut h, &c, &mut || 0.0));
        assert_eq!(h.ring_out_vfx.slug, "rov_default");
        // a slot set stays; an asset the catalogue lacks writes nothing
        assert_eq!(h.banner.slug, "mine");
        assert_eq!(h.announcer_pack.slug, "");
        assert_eq!(h.profile_icon.slug, "icon_b");
        assert_eq!(h.taunt_for_slot(0, "character_shaggy"), "taunt_shaggy");
        // the DoOnce: a second call without force does nothing
        h.ring_out_vfx = AssetRef::default();
        set_default_ring_out(&mut h, &c, false);
        assert_eq!(h.ring_out_vfx.slug, "");
        set_default_ring_out(&mut h, &c, true);
        assert_eq!(h.ring_out_vfx.slug, "rov_default");
    }

    #[test]
    fn taunts_replace_emote_global_and_force_fills_four() {
        let c = catalogue();
        let mut h = HydraSettings::default();
        h.set_character_taunt("character_shaggy", 0, ref_of(&item("emote_global", true)));
        set_default_taunts(&mut h, &c, false);
        assert_eq!(h.taunt_for_slot(0, "character_shaggy"), "taunt_shaggy");
        set_default_taunts(&mut h, &c, true);
        assert!((0..4).all(|i| h.taunt_for_slot(i, "character_shaggy") == "taunt_shaggy"));
    }

    #[test]
    fn pictures_and_lookups() {
        assert_eq!(picture_name("/Game/A/T_X.T_X").as_deref(), Some("T_X"));
        assert_eq!(picture_name("/Game/A/B.B:Sub").as_deref(), Some("B"));
        assert_eq!(picture_name(""), None);
        let c = catalogue();
        assert_eq!(cosmetic_by_slug(&c, "icon_b").map(|e| e.def), Some(true));
        assert!(cosmetic_by_slug(&c, "nothing").is_none());
    }
}
