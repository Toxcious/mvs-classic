//! What the screens ask of the level behind them (the pre-match setup, the podium, the main menu's character):
//! commands the view keeps in order until the program takes them
//! (`MenuView::take_scene_cmds`) and hands them to the renderer's `menu_*` calls.

use mvs_render::menuscene::{MenuLevel, PreviewSpec, SeqCmd};

/// A call into the menus' level.
#[derive(Clone, Debug, PartialEq)]
pub enum SceneCmd {
    /// The level to show (`Renderer::set_menu_level`).
    Level(MenuLevel),
    /// A preview actor's fighter, or none (`Renderer::menu_preview`: BPI_ChangeFighter, BPI_ClearFighter).
    Preview { slot: usize, spec: Option<PreviewSpec> },
    /// A call into the shown level (`Renderer::menu_sequence`).
    Sequence(SeqCmd),
    /// A preview slot's banner: its BannerData slug, None the default (`Renderer::menu_banner`).
    Banner { slot: usize, slug: Option<String> },
    /// A preview slot's banner profile icon: a store texture path, None the nameplate's own
    /// (`Renderer::menu_banner_icon`).
    BannerIcon { slot: usize, path: Option<String> },
    /// The post chain's fade to black, 0 none, 1 black (`Renderer::set_post_fade`).
    Fade(f32),
    /// UI_Functions.CharacterHover's actor part: a preview actor's hover light on or off (`Renderer::menu_hover`).
    Hover { slot: usize, on: bool },
    /// UI_Homescreen_v2 VO_Clickable's OnClick on the Player1 preview: its `clickable` voice line, posted by the
    /// program when one plays.
    ClickVo,
    /// The match's team style (ETeamStyle) the pre-match level and previews read (`Renderer::set_menu_team_style`:
    /// 0 1v1, 1 2v2, 2 FFA; None no config): a custom lobby's is the one its start derives
    /// (`lobby_rules::LobbyRules::match_setup`), not the lobby's own style. Sent when the pre-match opens and when the
    /// style changes.
    TeamStyle(Option<u8>),
}
