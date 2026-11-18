//! The main menu after the title screen: what the New_MainMenu level Blueprint and
//! PandaGameInstance.ShowMainMenu do: Create(UI_MainMenu) + AddToViewport(0), the cursor shown, and a type 1
//! transition when one is still up (the title screen leaves the fader black). The widget's THE LAB button ends in
//! OpenLevel(TrainingContainer) behind a fade to black; the program goes on to the match from there.

use mvs_settings::Settings;

use crate::menu::controller::{Cx, MenuRun};
use crate::menu::hydra::picture_name;
use crate::menu::manager::{Manager, TransitionParams};
use crate::menu::own_profile::profile_icon_by_asset;
use crate::menu::screen::MenuScreen;
use crate::menu::screens::lab_selectors::selected_skin;
use crate::menu::session::HOMESCREEN_LOCAL_PLAY;
use crate::menu::view::MenuView;
use crate::menu::widgets::common::{apply_founder_name_treatment, color_for_index};
use crate::menu::widgets::modals::show_modal_generic;
use crate::menu::widgets::social_menu::SocialMenu;
use crate::slate::{Vis, WidgetId};

/// SkinData'Skin_WonderWoman' of New_MainMenu @193 (BPI_LoadPreferredCharacter): the build's Player1, its fighter key.
const BUILD_CHARACTER: &str = "wonderwoman";

/// The Player1 actor's fighter and skin (a SkinData name, "" the default) as the port's BPI_LoadPreferredCharacter
/// takes them: the fighter the player chose last (the "PreviewCharacter" save slot) in their selected skin; with none
/// saved or not in this store, the build's Skin_WonderWoman (her default skin, whatever skin the player selected for
/// her); without her, the roster's first fighter in this store, its default skin.
/// Deviation: the build loads Skin_WonderWoman whatever was chosen (New_MainMenu @193; the exe names no preferred
/// character); the port shows the player's last pick, as the beta's earlier builds did.
pub fn preferred_character(view: &MenuView, prefs: &Settings) -> Option<(String, String)> {
    let last = &prefs.profile.last_character;
    let saved = view.roster().iter().find(|r| !last.is_empty() && r.slug == *last).and_then(|r| r.fighter.as_ref());
    if let Some((key, f)) = saved.and_then(|k| view.fighters().get_key_value(k)) {
        let skin = selected_skin(f, &prefs.hydra).filter(|s| !s.is_default).map(|s| s.name.clone());
        return Some((key.clone(), skin.unwrap_or_default()));
    }
    let key =
        view.fighters().contains_key(BUILD_CHARACTER).then(|| BUILD_CHARACTER.to_owned()).or_else(|| {
            view.roster().iter().filter_map(|r| r.fighter.clone()).find(|k| view.fighters().contains_key(k))
        })?;
    Some((key, String::new()))
}

/// The party partner's fighter of PreMatch's OnPartyChanged (@23758 LoadAsset_Blocking of the constant
/// /Game/Panda_Main/Characters/HarleyQuinn/skins/HarleyWithJacket): its key and its SkinData name.
const PARTNER_CHARACTER: &str = "harley";
const PARTNER_SKIN: &str = "HarleyWithJacket";

/// The Player3 actor's fighter and skin ("" the default) for a party partner on the main menu: `sent`, the fighter
/// key and SkinData name the partner's own main menu shows (its game's MenuFighter), when this store has that
/// fighter (a skin it lacks: the fighter's default skin); else HarleyWithJacket. None: neither is in this store.
// Deviation: the build loads HarleyWithJacket for every partner: PreMatch @23299-23758 reads the partner's
// LastPlayedCharacterSlug and its first skin, but both outcomes of the JumpIfNot at @23744 land on @23758, which loads
// the constant (raw bytecode). The port shows the partner's own main-menu fighter; a partner whose
// game sent none (an older one) gets the build's constant.
pub fn partner_character(view: &MenuView, sent: Option<(&str, &str)>) -> Option<(String, String)> {
    if let Some((key, skin)) = sent
        && let Some(f) = view.fighters().get(key)
    {
        let skin = f.skins.iter().find(|s| !skin.is_empty() && s.name == skin && !s.is_default);
        return Some((key.to_owned(), skin.map(|s| s.name.clone()).unwrap_or_default()));
    }
    let f = view.fighters().get(PARTNER_CHARACTER)?;
    let s = f.skins.iter().find(|s| s.name == PARTNER_SKIN)?;
    Some((PARTNER_CHARACTER.to_owned(), if s.is_default { String::new() } else { s.name.clone() }))
}

/// The main menu screen: UI_MainMenu's widget.
#[derive(Debug)]
pub struct MainMenuScreen {
    pub widget: WidgetId,
}

/// `lobby`: back from a custom game match, the lobby it was started from opens over the menu.
pub fn show(screen: &mut MenuScreen, run: &mut MenuRun<'_>) -> MainMenuScreen {
    screen.with_cx(run, |cx| {
        // the title screen's fade to black
        cx.view.manager.fader = 1.0;
        let widget = MenuView::add(cx, "UI_MainMenu");
        // ShowMainMenu (PGI @36304): ResetMatchFlow @36324 (IsLocalPlay false from here on); @38744 a custom game's
        // lobby again, else the homescreen queue's Local Play entry (put there by BPI_LocalPlayBegin) is taken out and
        // BPI_ShowLocalPlayUI (@39057 -> @35962) opens UI_LocalPlay ("localPlay") when it is not open, with the saved
        // prefs
        cx.view.session.set_is_local_play(false);
        if cx.view.session.lobby.is_some()
            || crate::menu::lobby::with_carry(|c| c.kept_net.as_ref().is_some_and(|k| k.alive()))
        {
            Manager::show_modal(cx, "WBP_CustomGame", "customGame", "", "");
        } else if cx.view.session.take_from_homescreen_queue(HOMESCREEN_LOCAL_PLAY)
            && cx.view.manager.get_modal("localPlay").is_none()
        {
            show_modal_generic(cx, false, "UI_LocalPlay", "localPlay", "");
        }
        cx.view.show_cursor(true);
        if cx.view.manager.is_transitioning() {
            Manager::transition(
                cx,
                TransitionParams { next_level: String::new(), kind: 1, delay: 0.0, force: true, ..Default::default() },
            );
        }
        MainMenuScreen { widget }
    })
}

/// The class of PartnerNameplate_PM's Nameplate (a screen-space WidgetComponent, DrawSize 500 x 500,
/// bDrawAtDesiredSize, Pivot the class default (0.5, 0.5): the view's world widgets are laid out at their desired
/// size about the point).
const NAMEPLATE_CLASS: &str = "UI_PM_PartnerNameplate";

/// The Nameplate component's world place: the TargetPoint PartnerNameplate_PM (290, 230, -20) and the component's
/// RelativeLocation (0, 30, 236.5) (PreMatch.json exports). The actor does not move with BannerMover2.
pub const PARTNER_NAMEPLATE_AT: [f64; 3] = [290.0, 260.0, 216.5];

/// The party partner's nameplate (PreMatch's PartnerNameplate_PM, UI_PM_PartnerNameplate) on the main menu and the
/// pre-match: shown with a partner (OnPartyChanged @24851, SetActorHiddenInGame(false)), hidden alone (@26040).
#[derive(Debug)]
pub struct PartnerNameplate {
    w: WidgetId,
    /// Its index among the view's world widgets.
    world: usize,
    shown: bool,
}

impl PartnerNameplate {
    /// The widget made with the level (the component's widget is made at BeginPlay), hidden. PreConstruct (@492-543):
    /// WBP_Quadrilateral.SetColorAndOpacity(PaletteForIndex(0).base). None: the bundle has no such class.
    pub fn new(screen: &mut MenuScreen, run: &mut MenuRun<'_>) -> Option<PartnerNameplate> {
        if !screen.view.bundle.widgets.contains_key(NAMEPLATE_CLASS) {
            eprintln!("menu: no widget class {NAMEPLATE_CLASS}: the main menu shows no partner nameplate");
            return None;
        }
        Some(screen.with_cx(run, |cx| {
            let w = MenuView::create(cx, NAMEPLATE_CLASS);
            let world = MenuView::add_world_widget(cx, w);
            let quad = cx.view.tree().find_in(w, "WBP_Quadrilateral");
            let c = color_for_index(cx, 0);
            cx.view.tree_mut().widget_mut(quad).color = c;
            PartnerNameplate { w, world, shown: false }
        }))
    }

    /// OnPartyChanged's nameplate: with a partner (`name`, `icon` its ProfileIconPath asset name), shown (@24851) and
    /// SetUsernameAndProfileImage(Username, ProfileIconPath, "", ShowCharacterName false, User) (@25059-25217);
    /// alone, hidden (@26040).
    pub fn set(&mut self, screen: &mut MenuScreen, run: &mut MenuRun<'_>, partner: Option<(&str, &str)>) {
        self.shown = partner.is_some();
        let Some((name, icon)) = partner else { return };
        let w = self.w;
        screen.with_cx(run, |cx| set_username_and_profile_image(cx, w, name, icon));
    }

    /// The component's place this frame: `at` the projected point and distance of [`PARTNER_NAMEPLATE_AT`] (None:
    /// off the screen or no level); a hidden nameplate has none.
    pub fn place(&self, view: &mut MenuView, at: Option<([f64; 2], f64)>) {
        view.place_world_widget(self.world, if self.shown { at } else { None });
    }
}

/// The class of BP_SocialMenu's BannerClick (main_menu.rs's partner click target).
const CLICK_CLASS: &str = "WBP_3dClick";

/// Where a partner puts BannerMover2 (LerpLocation alpha 1): PartnerNameplate_MM (285, 290, -10) on the main menu
/// (@24249-24499), PartnerNameplate_PM (290, 230, -20) on the pre-match (@9345-9445, @14058-14158).
pub const PARTNER_MOVER_MM: [f64; 3] = [285.0, 290.0, -10.0];
pub const PARTNER_MOVER_PM: [f64; 3] = [290.0, 230.0, -20.0];

/// BP_SocialMenu_3's BannerClick with a partner, BannerMover2 at `mover` at scale 0.4 (@25354, @9960, @15013):
/// BP_Banner_PM_Player2 at the mover (no RelativeLocation), BP_SocialMenu_3 at (-195, 67, 370) on it and BannerClick
/// at (5, 0, 250) on that (PreMatch.json, BP_SocialMenu.json), no rotation on the chain: the mover + 0.4 x
/// (-190, 67, 620).
pub fn partner_click_at(mover: [f64; 3]) -> [f64; 3] {
    [mover[0] - 0.4 * 190.0, mover[1] + 0.4 * 67.0, mover[2] + 0.4 * 620.0]
}

/// BannerClick's quad: DrawSize 300 x 500 at the desired size (WBP_3dClick's SizeBox_1 300 x 500), world space
/// (no Space set), its plane the component's Y (width) and Z (height), at the chain's scale 0.4: half extents in
/// world units along Y and Z.
pub const PARTNER_CLICK_HALF: [f64; 2] = [0.4 * 150.0, 0.4 * 250.0];

/// The party partner's click target (PreMatch's BP_SocialMenu_3, its BannerClick a WBP_3dClick): enabled with the
/// partner's account (main menu @25399 Enable Social Menu(true), @25607 Set Account ID; pre-match @10005-10169,
/// @15058-15266), disabled alone (@26040-26421) and at PlayersFound (@13286). Its click is BP_SocialMenu's ShowContextMenu (widgets/social_menu.rs; not ported: no social
/// actions menu).
// Deviation: BannerClick is a world-space component drawn invisible (Widget3DPassThrough_Translucent) that the
// cursor hits on its quad; the port hit-tests the screen rectangle about the quad's projected edges, as a
// screen-space world widget of that size. It is placed only while a partner is there (alone, the build's stays
// at FaceOff_Char_2 with its interactor off).
#[derive(Debug)]
pub struct PartnerClick {
    w: WidgetId,
    world: usize,
    on: bool,
}

impl PartnerClick {
    /// The widget made with the partner (the build's at BeginPlay). None: the bundle has no such class.
    pub fn new(screen: &mut MenuScreen, run: &mut MenuRun<'_>) -> Option<PartnerClick> {
        if !screen.view.bundle.widgets.contains_key(CLICK_CLASS) {
            eprintln!("menu: no widget class {CLICK_CLASS}: the main menu partner takes no click");
            return None;
        }
        Some(screen.with_cx(run, |cx| {
            let w = MenuView::create(cx, CLICK_CLASS);
            let world = MenuView::add_world_widget(cx, w);
            PartnerClick { w, world, on: false }
        }))
    }

    /// Enable Social Menu(on) and, with a partner, Set Account ID(its id).
    pub fn set(&mut self, screen: &mut MenuScreen, run: &mut MenuRun<'_>, account: Option<&str>) {
        self.on = account.is_some();
        let (w, on) = (self.w, self.on);
        screen.with_cx(run, |cx| {
            cx.visit::<SocialMenu, _>(w, |s, cx| {
                s.enable(cx, on);
                if let Some(id) = account {
                    s.set_account_id(id);
                }
            });
        });
    }

    /// The target's place this frame from the projected centre (`partner_click_at`) and its quad's four edge
    /// midpoints (`PARTNER_CLICK_HALF` along -Y, +Y, -Z, +Z), each a point in pixels and its distance (None: off the
    /// screen); off while disabled.
    pub fn place(&self, view: &mut MenuView, centre: Option<([f64; 2], f64)>, edges: [Option<[f64; 2]>; 4]) {
        let at = centre.filter(|_| self.on);
        if let (Some(_), [Some(l), Some(r), Some(b), Some(t)]) = (at, edges) {
            let w = (r[0] - l[0]).abs().max((t[0] - b[0]).abs());
            let h = (t[1] - b[1]).abs().max((r[1] - l[1]).abs());
            if let Some(e) = view.world.get_mut(self.world) {
                e.size = Some([w / view.dpi, h / view.dpi]);
            }
        }
        view.place_world_widget(self.world, at);
    }

    /// The clicks taken since the last take (the partner's account of each).
    pub fn take_clicks(&self, view: &mut MenuView) -> Vec<String> {
        view.controller::<SocialMenu>(self.w).map(SocialMenu::take_clicks).unwrap_or_default()
    }
}

/// UI_PM_PartnerNameplate SetUsernameAndProfileImage (@603-836, @10-487) with ShowCharacterName false: the
/// ProfileIconData of the path loaded; a valid one puts its TextureRef on ProfilePicQuad's material (Texture) and shows
/// it (4), else ProfilePicQuad collapsed; WBP_3dText.SetName(UserName, User) (PlayerName, then the founder
/// treatment, as the pre-match banners' plates); CharacterNameConstrain collapsed. The path is the member's
/// ProfileIconPath asset name (the party's card), looked up in the catalogue.
fn set_username_and_profile_image(cx: &mut Cx<'_>, w: WidgetId, name: &str, icon: &str) {
    let quad = cx.view.tree().find_in(w, "ProfilePicQuad");
    let tex = profile_icon_by_asset(cx.view, icon)
        .and_then(|item| picture_name(item.icon.as_deref()?))
        .and_then(|t| Some((cx.view.bundle.textures.get(&t)?.entry().to_string(), t)));
    match tex {
        Some((entry, t)) => {
            cx.view.load_texture(&t);
            let x = cx.view.tree_mut().widget_mut(quad);
            if let Some(m) = &mut x.mid {
                m.set_texture("Texture", &entry);
            }
            x.vis = Vis::SelfHitTestInvisible;
        }
        None => cx.view.tree_mut().widget_mut(quad).vis = Vis::Collapsed,
    }
    let text = cx.view.tree().find_in(w, "WBP_3dText");
    let label = cx.view.tree().find_in(text, "PlayerName");
    cx.view.tree_mut().widget_mut(label).text = name.to_owned();
    apply_founder_name_treatment(cx, label);
    let c = cx.view.tree().find_in(w, "CharacterNameConstrain");
    cx.view.tree_mut().widget_mut(c).vis = Vis::Collapsed;
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use mvs_settings::AssetRef;

    use super::*;
    use crate::bundle::{Bundle, FighterInfo, MenuData, RosterEntry, SkinInfo};
    use crate::text::GameText;

    /// A view whose roster lists `roster` (slug, fighter) and whose store has `fighters`, each with a default skin
    /// and a second one.
    fn view(roster: &[(&str, Option<&str>)], fighters: &[&str]) -> MenuView {
        let skin = |k: &str, n: &str, d: bool| SkinInfo {
            name: format!("{k}_{n}"),
            slug: format!("skin_{k}_{n}"),
            is_default: d,
            ..SkinInfo::default()
        };
        let menu = MenuData {
            roster: roster
                .iter()
                .map(|(s, f)| RosterEntry {
                    slug: (*s).to_owned(),
                    fighter: f.map(str::to_owned),
                    ..RosterEntry::default()
                })
                .collect(),
            fighters: fighters
                .iter()
                .map(|k| {
                    let info = FighterInfo {
                        slug: format!("character_{k}"),
                        skins: vec![skin(k, "S00", true), skin(k, "S01", false)],
                        ..FighterInfo::default()
                    };
                    ((*k).to_owned(), info)
                })
                .collect(),
            ..MenuData::default()
        };
        let bundle = Bundle { menu: Some(menu), ..Bundle::default() };
        MenuView::new(Arc::new(bundle), Arc::new(GameText::default()), [1920, 1080])
    }

    fn prefs(last: &str, skins: &[(&str, &str)]) -> Settings {
        let mut s = Settings::default();
        s.profile.last_character = last.to_owned();
        for (c, slug) in skins {
            s.hydra.set_character_skin(c, AssetRef { slug: (*slug).to_owned(), path: String::new() });
        }
        s
    }

    #[test]
    fn last_pick_then_the_builds_wonder_woman() {
        let roster = [
            ("character_shaggy", Some("shaggy")),
            ("character_wonderwoman", Some("wonderwoman")),
            ("character_batman", Some("batman")),
            ("character_none", None),
        ];
        let v = view(&roster, &["shaggy", "wonderwoman", "batman"]);
        let pick = |p: &Settings| preferred_character(&v, p);
        // the last pick in its selected skin; its default skin as ""
        let p = prefs("character_batman", &[("character_batman", "skin_batman_S01")]);
        assert_eq!(pick(&p), Some(("batman".into(), "batman_S01".into())));
        assert_eq!(pick(&prefs("character_batman", &[])), Some(("batman".into(), String::new())));
        // no pick, an unknown slug, a tile with no fighter: Skin_WonderWoman, whatever skin is selected for her
        let ww = Some(("wonderwoman".into(), String::new()));
        assert_eq!(pick(&prefs("", &[("character_wonderwoman", "skin_wonderwoman_S01")])), ww);
        assert_eq!(pick(&prefs("character_nobody", &[])), ww);
        assert_eq!(pick(&prefs("character_none", &[])), ww);
        // a pick whose fighter is not in the store, and no Wonder Woman: the roster's first fighter in the store
        let v = view(&roster, &["batman"]);
        assert_eq!(preferred_character(&v, &prefs("character_shaggy", &[])), Some(("batman".into(), String::new())));
        let v = view(&roster, &[]);
        assert_eq!(preferred_character(&v, &prefs("", &[])), None);
    }

    /// A view whose store has Harley alone, with these skins (name, default).
    fn harley(skins: &[(&str, bool)]) -> MenuView {
        let skins = skins
            .iter()
            .map(|(n, d)| SkinInfo { name: (*n).to_owned(), is_default: *d, ..SkinInfo::default() })
            .collect();
        let info = FighterInfo { slug: "character_harley".into(), skins, ..FighterInfo::default() };
        let menu = MenuData { fighters: [("harley".to_owned(), info)].into_iter().collect(), ..MenuData::default() };
        let bundle = Bundle { menu: Some(menu), ..Bundle::default() };
        MenuView::new(Arc::new(bundle), Arc::new(GameText::default()), [1920, 1080])
    }

    #[test]
    fn partner_is_harley_with_jacket() {
        let v = harley(&[("HarleyWithJacket", true), ("Harley_S01", false)]);
        assert_eq!(partner_character(&v, None), Some(("harley".into(), String::new())));
        let v = harley(&[("Harley_S01", true), ("HarleyWithJacket", false)]);
        assert_eq!(partner_character(&v, None), Some(("harley".into(), "HarleyWithJacket".into())));
        // no such skin, no Harley: no partner shown
        assert_eq!(partner_character(&harley(&[("Harley_S01", true)]), None), None);
        assert_eq!(partner_character(&view(&[], &["batman"]), None), None);
    }

    /// The fighter the partner's game sent, when this store has it: its skin, or its default skin ("") for a default
    /// or unknown one; a fighter not in this store falls back to HarleyWithJacket.
    #[test]
    fn partner_shows_the_fighter_it_sent() {
        let v = view(&[], &["batman", "harley"]);
        assert_eq!(partner_character(&v, Some(("batman", "batman_S01"))), Some(("batman".into(), "batman_S01".into())));
        assert_eq!(partner_character(&v, Some(("batman", ""))), Some(("batman".into(), String::new())));
        assert_eq!(partner_character(&v, Some(("batman", "batman_S00"))), Some(("batman".into(), String::new())));
        assert_eq!(partner_character(&v, Some(("batman", "nope"))), Some(("batman".into(), String::new())));
        let v = harley(&[("Harley_S01", true), ("HarleyWithJacket", false)]);
        assert_eq!(partner_character(&v, Some(("shaggy", ""))), Some(("harley".into(), "HarleyWithJacket".into())));
    }
}
