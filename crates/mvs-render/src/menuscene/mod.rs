//! The menus' levels drawn behind their screens: the main menu's scene, the PreMatch level
//! with its preview actors, banners and sequences, and the podium after a match. The program drives them through
//! the `Renderer::menu_*` calls; with no level set every call does nothing and the frame is the program's own.

pub mod banner;
pub mod format;
pub mod main_menu;
mod notify_vfx;
pub mod podium;
pub mod pre_match;
pub mod preview;
pub mod sequence;

use mvs_view::{Camera, FighterPose, MeshPose};

use self::format::{DirLight, EventArg, LevelName, SceneData, SpotLight};
use crate::gpu::{Gpu, Texture};
use crate::renderer::Passes;
use crate::scene::LightSet;
use crate::view::ViewMembers;
use crate::{Assets, Error, FrameTimes, Renderer, Result};

/// The passes a menu level's frame runs: no priority captures (no PFGCamera), no decals and no fog (the levels have
/// no DecalActor and no fog actor).
pub(crate) const MENU_PASSES: Passes = Passes {
    captures: false,
    custom_depth: true,
    decals: false,
    lighting: true,
    fog: false,
    translucency: true,
    particles: true,
    post: true,
};

/// The main menu sub-level PreMatch's MapBuildDataRegistry (its PersistentLevel's MapBuildData): the volumetric
/// lightmap the main menu's and the PreMatch level's previews take.
// Unverified: which streamed level's volumetric lightmap the scene uses is 4.26 source.
const PREMATCH_BUILT_DATA: &str = "/Game/Panda_Main/PreMatch/PreMatch_BuiltData.PreMatch_BuiltData";

/// The podium level's MapBuildDataRegistry, and the suffix of its sky
/// entry (PODIUM_SKY_LEVEL, `sky_entry_path`): the podium's fighters take its volumetric lightmap and sky light.
const PODIUM_BUILT_DATA: &str =
    "/Game/Panda_Main/Maps/Map_PodiumStage/Map_PodiumStage_New_BuiltData.Map_PodiumStage_New_BuiltData";
const PODIUM_SKY: &str = "/Map_PodiumStage/Map_PodiumStage_New.Map_PodiumStage_New";

/// The level the menus show (OpenLevel's New_MainMenu, PreMatch, Map_PodiumStage_New), or none.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum MenuLevel {
    #[default]
    None,
    MainMenu,
    PreMatch,
    Podium,
}

/// A preview actor's fighter (BPI_ChangeFighter's Skin and TeamIndex).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PreviewSpec {
    /// The roster key (protocol roster CHARACTERS).
    pub key: String,
    /// The SkinData name; "" the default skin.
    pub skin: String,
    /// TeamIndex.
    pub team: u8,
}

/// A call into the shown level from the screen over it.
#[derive(Clone, Debug, PartialEq)]
pub enum SeqCmd {
    /// BPI_PreMatchTransition(name) to the PreMatch level.
    Transition(String),
    /// The found match's map, whose picture the level fetches for NastyMapBGChange.
    PreloadMap(String),
    /// The game state's SkinDataReceived.
    SkinData(Box<PreMatchSkinData>),
    /// A player's cosmetics came after the skin data: the banners and profile icons read again.
    Cosmetics,
    /// The podium's scene for a match's end (the scene's constructor): its intro sequence starts.
    Podium(Box<PodiumStart>),
}

/// The game state's Teammates / Enemies arrays and the usernames, by preview slot
/// (PreMatchSkinData).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PreMatchSkinData {
    pub teammates: Vec<Option<PreviewSpec>>,
    pub enemies: Vec<Option<PreviewSpec>>,
    pub usernames: Vec<String>,
}

/// What the podium is made with (the scene's constructor arguments).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PodiumStart {
    /// The lobby's team style: 2 plays Seq_WinnerIntro_FFA, the others Seq_WinnerIntro_2v2.
    pub team_style: u8,
    /// The result lists (SortPodiumDataResults).
    pub winners: Vec<PodiumRecord>,
    pub losers: Vec<PodiumRecord>,
    /// The local player won: the colour and trophy planes take the Blue material instance, else the Red one.
    pub you_won: bool,
}

/// A record of the podium's lists.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PodiumRecord {
    /// The roster key.
    pub key: String,
    /// The SkinData name; "" the default.
    pub skin: String,
    /// The banner's ColorType - 1 (0 your team, 1 another).
    pub color: u8,
    /// The BannerData slug (None: none).
    pub banner: Option<String>,
}

/// What a level tells the program (the scenes' host callbacks), taken with `Renderer::take_menu_events`.
#[derive(Clone, Debug, PartialEq)]
pub enum MenuEvent {
    /// An Ak event, global or at a UE location (PF_Post_Event_At_Location).
    Sound { event: String, at: Option<[f64; 3]> },
    /// A podium sequence's director call (PodiumEvent, ShowFighter, AnimBannerIn ...).
    Podium { call: String, args: Vec<EventArg> },
    /// SequenceUI_Event (ShowVS, ShowFFA) of a pre-match sequence.
    SequenceUi(String),
    /// BPI_SendWorldLocation (CheckMark1..4, PerkInfo1..4): a world place the screen projects with `menu_project`.
    WorldLocation { id: String, at: [f64; 3] },
    /// A sequence's Fade track: the screen's fader alpha.
    Fade(f64),
    /// A preview actor's fighter settled and its load is asked.
    PreviewSettled { slot: usize, key: String },
    /// BPI_TransitionComplete(name) of the pre-match level: the face-off
    /// sequence ended ("FaceOff").
    TransitionComplete(String),
}

/// A place of the podium the end-of-game widgets stand at (nametagPoint, toastButtonPoint).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ScenePoint {
    Fighter { index: usize, winner: bool },
    Nametag { index: usize, winner: bool },
    ToastButton { index: usize, winner: bool },
}

/// A world space widget the scene draws from the menus' render target of it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SceneWidget {
    /// The podium's WBP_3dTeam.
    WinnerText,
    /// A podium banner's Stat Sheet.
    StatSheet { index: usize, winner: bool },
    /// A pre-match banner's NametagWidget, by preview slot.
    Nametag(usize),
}

/// What a level hands the frame: its camera, the posed previews, its meshes and its lights.
#[derive(Debug, Default)]
pub struct MenuFrame {
    /// The output's size, pixels (set before the level and the previews fill the frame).
    pub size: [u32; 2],
    pub camera: Option<Camera>,
    /// Each preview's view with its lighting channel (0 none of the level's lights, 1, 2).
    pub fighters: Vec<(FighterPose, u8)>,
    /// The planes, banner pieces and glow shells, each with its TranslucencySortPriority.
    pub meshes: Vec<(MeshPose, i32)>,
    /// Meshes drawn into custom depth and stencil only (the banners' masks, their stencil the banner's index), before
    /// the translucent pass.
    pub custom_depth: Vec<MeshPose>,
    /// The rigs' spot lights by lighting channel.
    pub spots: Vec<(u8, SpotLight)>,
    /// The podium's directional lights by lighting channel.
    pub directional: Vec<(u8, DirLight)>,
    /// The world-space widget components' quads (the podium's winner text and stat sheets).
    pub widgets: Vec<crate::scenery::WidgetQuad>,
}

/// What a level's tick may touch.
pub struct LevelCx<'a> {
    pub gpu: &'a Gpu,
    pub assets: &'a Assets,
    pub vfx: Option<&'a mut mvs_vfx::Player>,
    pub events: &'a mut Vec<MenuEvent>,
    /// The preview actors (the main menu's Player1, the pre-match's four).
    pub previews: &'a mut preview::PreviewSet,
}

/// The shown level.
#[derive(Debug)]
pub(crate) enum LevelScene {
    MainMenu(Box<main_menu::MainMenuLevel>),
    PreMatch(Box<pre_match::PreMatchLevel>),
    Podium(Box<podium::PodiumLevel>),
}

/// The renderer's menu state.
#[derive(Debug, Default)]
pub struct MenuScenes {
    level: MenuLevel,
    scene: Option<LevelScene>,
    previews: preview::PreviewSet,
    events: Vec<MenuEvent>,
    /// The lobby's team style the PreMatch level and the previews read (None: no config), and UI_Cursor's
    /// MousePosNormalized the banners' parallax reads; a new PreMatch level takes both.
    team_style: Option<u8>,
    cursor: [f64; 2],
    /// CalendarManager.GetActiveEvents' slugs (`Renderer::set_menu_active_events`) the main menu's background reads
    /// when a level opens.
    active_events: Vec<String>,
    /// The post chain's fade to black (1 black), set by the screens' fader.
    pub fade: f32,
    /// The VignetteIntensity a level's camera overrides (None: the post's own).
    pub vignette: Option<f32>,
    /// The last drawn frame's lights (its rigs' spots and directional lights).
    pub(crate) lights: LightSet,
    /// PreMatch's volumetric lightmap as View members, loaded at the first level that shows previews.
    vlm: Option<ViewMembers>,
    /// The podium level's volumetric lightmap and sky light as View members, and the sky light, loaded with the level.
    podium: Option<(ViewMembers, crate::scene::SkyLight)>,
    /// The podium's lights' static shadow depth maps (its MapBuildDataRegistry's, by LightGuid) the translucency
    /// volume's inject reads, loaded with the level.
    podium_shadows: Option<crate::lighting::tlv::StaticShadows>,
    /// The camera of the last tick or draw (`menu_project`'s).
    pub(crate) camera: Option<Camera>,
    /// The last drawn frame's world-space widget quads (`MenuFrame::widgets`).
    pub(crate) quads: Vec<crate::scenery::WidgetQuad>,
    /// The levels' clock, seconds (the materials' and the View's time).
    pub(crate) time: f64,
    pub(crate) last_dt: f64,
}

impl MenuScenes {
    pub fn level(&self) -> MenuLevel {
        self.level
    }

    /// The static shadow maps of the shown level's lights (the podium's; the other levels' lights have none).
    pub(crate) fn tlv_shadows(&self) -> crate::lighting::tlv::TlvShadows<'_> {
        let podium = (self.level == MenuLevel::Podium).then_some(self.podium_shadows.as_ref()).flatten();
        crate::lighting::tlv::TlvShadows::Menu(podium)
    }

    /// The View members of a menu level's frame: `base` without the stage's, with PreMatch's volumetric lightmap
    /// dimmed for the previews (the main menu and the PreMatch level).
    pub(crate) fn members(&self, base: &ViewMembers) -> ViewMembers {
        let mut m = crate::stage::without_stage_members(base);
        if let (Some((v, _)), MenuLevel::Podium) = (&self.podium, self.level) {
            // (the podium's fighters: its own lightmap and sky light)
            m.rows.extend(v.rows.iter().cloned());
            m.textures.extend(v.textures.iter().cloned());
            return m;
        }
        if let Some(v) = &self.vlm
            && matches!(self.level, MenuLevel::MainMenu | MenuLevel::PreMatch)
        {
            m.rows.extend(v.rows.iter().cloned());
            m.textures.extend(v.textures.iter().cloned());
        }
        m
    }

    /// The post settings of a menu level's frame: the defaults with the fade and the camera's vignette.
    pub(crate) fn post_view(&self) -> crate::post::PostView {
        crate::post::PostView::menu(self.fade, self.vignette)
    }

    /// The frame's lights: each rig's spots and each directional light on its lighting channel.
    pub(crate) fn set_lights(&mut self, frame: &MenuFrame) {
        self.lights = LightSet {
            directional: frame.directional.iter().map(|(c, l)| crate::stage::menu_directional(l, 1 << c)).collect(),
            radial: frame.spots.iter().map(|(c, l)| crate::stage::menu_spot(l, 1 << c)).collect(),
            sky: self.podium.as_ref().filter(|_| self.level == MenuLevel::Podium).map(|(_, s)| s.clone()),
            view: ViewMembers::default(),
        };
    }
}

impl Renderer {
    /// Opens `level` behind the menus (its entry loaded), or lets the shown one go (None).
    pub fn set_menu_level(&mut self, level: MenuLevel, vfx: Option<&mut mvs_vfx::Player>) -> Result<()> {
        if level == self.menu.level {
            return Ok(());
        }
        if self.menu.scene.is_some() {
            // Dispose: the previews let go; the caller clears its effects player first.
            self.menu.previews.dispose(None);
        }
        // Dispose: the level's emitters (the effects player's `clear` keeps level systems) and
        // banner elements go with it.
        if let Some(LevelScene::PreMatch(l)) = &mut self.menu.scene {
            let mut cx = LevelCx {
                gpu: &self.gpu,
                assets: &self.assets,
                vfx,
                events: &mut self.menu.events,
                previews: &mut self.menu.previews,
            };
            l.dispose(&mut cx);
        }
        self.menu.scene = None;
        self.menu.level = MenuLevel::None;
        self.menu.previews.set_main_menu_up(false);
        self.menu.vignette = None;
        // The fader is back to clear when a screen lets its level go.
        self.menu.fade = 0.0;
        self.menu.events.clear();
        let store = self.assets().store();
        let scene = match level {
            MenuLevel::None => return Ok(()),
            MenuLevel::MainMenu => {
                let e = format::load_level(store, LevelName::MainMenu)?;
                LevelScene::MainMenu(Box::new(main_menu::MainMenuLevel::new(
                    &self.gpu,
                    self.assets(),
                    e,
                    &self.menu.active_events,
                )?))
            }
            MenuLevel::PreMatch => {
                let e = format::load_level(store, LevelName::PreMatch)?;
                let mut l = pre_match::PreMatchLevel::new(&self.gpu, self.assets(), e, &self.menu.active_events)?;
                l.team_style = self.menu.team_style.unwrap_or(0);
                l.cursor = self.menu.cursor;
                self.menu.previews.set_team_style(self.menu.team_style);
                l.open(&mut self.menu.previews, &mut self.menu.events, &self.assets)?;
                LevelScene::PreMatch(Box::new(l))
            }
            MenuLevel::Podium => {
                let e = format::load_level(store, LevelName::Podium)?;
                LevelScene::Podium(Box::new(podium::PodiumLevel::new(&self.gpu, self.assets(), e)?))
            }
        };
        if self.menu.podium.is_none() && level == MenuLevel::Podium {
            // (a failed load leaves the fighters out, never the level)
            match crate::stage::menu_podium_light(self, PODIUM_BUILT_DATA, PODIUM_SKY) {
                Ok(light) => self.menu.podium = Some(light),
                Err(e) => eprintln!("mvs: the podium's lightmap and sky light: {e}: the level opens without them"),
            }
        }
        if let (None, LevelScene::Podium(l)) = (&self.menu.podium_shadows, &scene) {
            let lights = l.data.lights.values().flatten().filter_map(|d| Some((d.name.as_str(), d.guid.as_deref()?)));
            match crate::lighting::tlv::level_shadows(&self.gpu, self.assets().store(), PODIUM_BUILT_DATA, lights) {
                Ok(s) => self.menu.podium_shadows = Some(s),
                Err(e) => eprintln!("mvs: the podium's static shadow maps: {e}: its lights inject unshadowed"),
            }
        }
        if self.menu.vlm.is_none() && matches!(level, MenuLevel::MainMenu | MenuLevel::PreMatch) {
            self.menu.vlm = Some(crate::stage::menu_vlm_members(self, PREMATCH_BUILT_DATA)?);
        }
        // The main menu's CineCamera overrides VignetteIntensity; the post's own after it.
        // The pre-match screen runs on a MenuScene of the main menu's data, so it takes the main
        // menu camera's too. The podium sets its own.
        self.menu.vignette = match &scene {
            LevelScene::MainMenu(l) => l.data.camera.vignette.map(|v| v as f32),
            LevelScene::PreMatch(_) => match format::load_level(self.assets().store(), LevelName::MainMenu)?.data {
                SceneData::MainMenu(d) => d.camera.vignette.map(|v| v as f32),
                _ => None,
            },
            LevelScene::Podium(l) => l.data.camera.vignette.map(|v| v as f32),
        };
        // (the main menu's frame, kept for the MainMenu placements the pre-match lerps from)
        if let LevelScene::MainMenu(l) = &scene {
            let c = &l.data.camera;
            self.menu.previews.set_fit_frame(Some(preview::FitFrame::of_camera(c.loc, c.rot, c.fov, c.aspect)));
        }
        self.menu.scene = Some(scene);
        self.menu.level = level;
        // (New_MainMenu's level is open with UI_MainMenu up: PandaGameInstance InMainMenu's widget conjuncts)
        self.menu.previews.set_main_menu_up(level == MenuLevel::MainMenu);
        self.menu.camera = None;
        Ok(())
    }

    /// A preview actor's hover light on or off (BPI_OnHovered / BPI_OnUnhovered).
    pub fn menu_hover(&mut self, slot: usize, on: bool) {
        if self.menu.scene.is_some() {
            self.menu.previews.hover(slot, on);
        }
    }

    /// A preview actor has a fighter; none with no level.
    pub fn menu_has_fighter(&self, slot: usize) -> bool {
        self.menu.scene.is_some() && self.menu.previews.has_fighter(slot)
    }

    /// The shown level.
    pub fn menu_level(&self) -> MenuLevel {
        self.menu.level
    }

    /// The level's tick of `dt` seconds (the level's actors, then the previews', then
    /// the emitters on `vfx` when time moved).
    pub fn menu_tick(&mut self, dt: f64, mut vfx: Option<&mut mvs_vfx::Player>) -> Result<()> {
        let Some(scene) = &mut self.menu.scene else { return Ok(()) };
        let mut cx = LevelCx {
            gpu: &self.gpu,
            assets: &self.assets,
            vfx: vfx.as_deref_mut(),
            events: &mut self.menu.events,
            previews: &mut self.menu.previews,
        };
        match scene {
            LevelScene::MainMenu(l) => l.tick(&mut cx, dt)?,
            LevelScene::PreMatch(l) => l.tick(&mut cx, dt)?,
            LevelScene::Podium(l) => l.tick(&mut cx, dt)?,
        }
        // The sequence's fade is the post's
        if let LevelScene::Podium(l) = scene {
            self.menu.fade = l.fade;
        }
        self.menu.previews.tick(&self.gpu, &self.assets, vfx.as_deref_mut(), &mut self.menu.events, dt)?;
        if dt > 0.0
            && let Some(v) = vfx
        {
            v.tick(dt);
        }
        self.menu.time += dt;
        self.menu.last_dt = dt;
        // the camera of this frame for the projections made before its draw (the screen's world widgets, as
        // the results screen projects after the scene is rendered)
        self.menu.camera = self.menu.scene.as_ref().map(|s| match s {
            LevelScene::MainMenu(l) => l.camera(self.output),
            LevelScene::PreMatch(l) => l.camera(self.output),
            LevelScene::Podium(l) => l.camera(self.output),
        });
        Ok(())
    }

    /// A preview actor's fighter (BPI_ChangeFighter), or none (BPI_ClearFighter). The main menu's Player1 (slot 0)
    /// takes the fighter, its view in at once (New_MainMenu's
    /// BPI_LoadPreferredCharacter): placed at the skin's MainMenu_You, playing its HomescreenIdle and lit by its rig,
    /// all from the skin's preview entry. Every other slot loads on the loader, the main menu's Player3 (slot 2, the
    /// party partner) included: placed at MainMenu_Friend, playing its HomescreenIdle while the main menu is up.
    pub fn menu_preview(&mut self, slot: usize, spec: Option<PreviewSpec>) -> Result<()> {
        if self.menu.scene.is_none() {
            return Ok(());
        }
        self.offer_preview_warm();
        let store = self.assets.store();
        match spec {
            Some(s) if slot == 0 && self.menu.level == MenuLevel::MainMenu => {
                let view = mvs_view::PreviewView::new(store, &s.key, &s.skin)?;
                let asset = preview::preview_asset(store, &s.key, &s.skin)?;
                self.menu.previews.set_fighter(store, 0, view, asset, &s.skin)
            }
            Some(s) => self.menu.previews.change_fighter(store, slot, &s, false),
            None => {
                self.menu.previews.clear_fighter(slot);
                Ok(())
            }
        }
    }

    /// A call into the shown level.
    pub fn menu_sequence(&mut self, cmd: SeqCmd) -> Result<()> {
        self.offer_preview_warm();
        let Some(scene) = &mut self.menu.scene else { return Ok(()) };
        match scene {
            LevelScene::MainMenu(_) => Ok(()),
            LevelScene::PreMatch(l) => l.command(&mut self.menu.previews, &mut self.menu.events, &self.assets, cmd),
            LevelScene::Podium(l) => l.command(cmd),
        }
    }

    /// The previews' next view load gets a warm-up job of the renderer's (the meshes, textures and shaders its
    /// first draw takes made on the loader with the view).
    fn offer_preview_warm(&mut self) {
        if !self.menu.previews.has_warm() {
            let job = self.warm_job(None);
            self.menu.previews.offer_warm(job);
        }
    }

    /// Fighter views made on the frame thread, in at the change (a scripted run, whose frames wait for no loader).
    pub fn set_menu_wait_loads(&mut self, wait: bool) {
        self.menu.previews.set_wait_loads(wait);
    }

    /// The lobby's team style (preMatchSession.setup.teamStyle: 0 1v1, 1 2v2, 2 FFA; None: no config) the PreMatch level
    /// and the previews read. Set before the pre-match opens and whenever the lobby's style changes.
    pub fn set_menu_team_style(&mut self, style: Option<u8>) {
        self.menu.team_style = style;
        self.menu.previews.set_team_style(style);
        if let Some(LevelScene::PreMatch(l)) = &mut self.menu.scene {
            l.team_style = style.unwrap_or(0);
        }
    }

    /// The active events' slugs in CalendarManager.GetActiveEvents' order (MainMenuBackground UpdateBGForEvents reads
    /// them through BP_EventRewardsLibrary Get Active Events): the main menu's background is the first one's with an
    /// Event Lookup Table entry, else its own. Read by the levels opened after the call, as the build reads them at
    /// the background's BeginPlay, and by the main menu level when it is shown; a shown pre-match keeps its
    /// material. None set: the build offline, the default.
    // Deviation: the shown main menu's background follows the call (the port's own background choice, mvs-settings
    // `UiSettings::menu_background`); the build's keeps its material until the level is made again.
    pub fn set_menu_active_events<S: AsRef<str>>(&mut self, slugs: &[S]) {
        self.menu.active_events = slugs.iter().map(|s| s.as_ref().to_owned()).collect();
        if let Some(LevelScene::MainMenu(l)) = &mut self.menu.scene {
            l.set_background(&self.menu.active_events);
        }
    }

    /// The backgrounds the main menu's plane can draw (`MainMenuScene::background_options`: "" its own, then each
    /// Event Lookup Table slug in the table's order), read from the store's MainMenu entry.
    pub fn menu_background_options(&self) -> Result<Vec<String>> {
        let e = format::load_level(self.assets().store(), LevelName::MainMenu)?;
        let SceneData::MainMenu(data) = e.data else {
            return Err(Error::Format { what: e.name, why: "not the MainMenu entry".into() });
        };
        Ok(data.background_options())
    }

    /// UI_Cursor's MousePosNormalized (0..1 over the window): what the pre-match banners' parallax reads.
    pub fn set_menu_cursor(&mut self, at: [f64; 2]) {
        self.menu.cursor = at;
        if let Some(LevelScene::PreMatch(l)) = &mut self.menu.scene {
            l.cursor = at;
        }
    }

    /// The value of the global music switch `mus` the level asked since the last take (the PreMatch level's
    /// SetupCharSelect: mus_menu_chaselect); the program sets it on the menus' music.
    pub fn take_menu_music(&mut self) -> Option<&'static str> {
        match &mut self.menu.scene {
            Some(LevelScene::PreMatch(l)) => l.take_music(),
            _ => None,
        }
    }

    /// The pre-match banners' NametagWidget places (UE world) by preview slot, for the banners shown: the screen
    /// projects each with `menu_project` and draws its plate there. Empty with no PreMatch level.
    pub fn menu_plates(&self) -> Vec<(usize, [f64; 3])> {
        match &self.menu.scene {
            Some(LevelScene::PreMatch(l)) => l.plates(),
            _ => Vec::new(),
        }
    }

    /// The banner of a preview slot: its BannerData slug, or the default banner (None).
    pub fn menu_banner(&mut self, slot: usize, slug: Option<&str>) -> Result<()> {
        let Some(scene) = &mut self.menu.scene else { return Ok(()) };
        match scene {
            LevelScene::MainMenu(_) => Ok(()),
            LevelScene::PreMatch(l) => l.set_banner(slot, slug),
            LevelScene::Podium(l) => l.set_banner(slot, slug),
        }
    }

    /// A preview slot's banner profile icon: a store texture path, None the nameplate's own (the pre-match level's).
    pub fn menu_banner_icon(&mut self, slot: usize, path: Option<&str>) {
        if let Some(LevelScene::PreMatch(l)) = &mut self.menu.scene {
            l.set_banner_icon(&self.assets, slot, path);
        }
    }

    /// The post chain's fade to black (0 none, 1 black).
    pub fn set_post_fade(&mut self, alpha: f32) {
        self.menu.fade = alpha;
    }

    /// The post chain's fade to black as last set (0 none, 1 black).
    pub fn post_fade(&self) -> f32 {
        self.menu.fade
    }

    /// The vignette intensity over the post's own (None: the post's).
    pub fn set_vignette(&mut self, intensity: Option<f32>) {
        self.menu.vignette = intensity;
    }

    /// What the levels told since the last take, in order.
    pub fn take_menu_events(&mut self) -> Vec<MenuEvent> {
        std::mem::take(&mut self.menu.events)
    }

    /// A podium place in UE world space (None: no podium, or no fighter there).
    pub fn menu_point(&self, p: ScenePoint) -> Option<[f64; 3]> {
        match &self.menu.scene {
            Some(LevelScene::Podium(l)) => l.point(p),
            _ => None,
        }
    }

    /// A world place projected with the level's camera of the moment: canvas units of the 2560 x 1440 box
    /// (UI_Functions.WorldLocationToScreenPosition), None off the screen or with no level.
    pub fn menu_project(&self, at: [f64; 3]) -> Option<[f64; 2]> {
        self.menu.scene.as_ref()?;
        main_menu::project(self.menu.camera.as_ref()?, at, self.output)
    }

    /// Where a screen space WidgetComponent at `at` (UE world) stands with the level's camera of the moment: its
    /// projected point in pixels of the output (unrounded) and its distance from the view origin
    /// (SWorldWidgetScreenLayer Tick 142617ff0, the canvas slot's ZOrder); None off the screen or with no level.
    pub fn menu_screen_place(&self, at: [f64; 3]) -> Option<([f64; 2], f64)> {
        self.menu.scene.as_ref()?;
        let camera = self.menu.camera.as_ref()?;
        let p = main_menu::project_pixels(camera, at, self.output)?;
        let d = (0..3).map(|k| (at[k] - camera.origin[k]).powi(2)).sum::<f64>().sqrt();
        Some((p, d))
    }

    /// `at` (UE world), a place beside a preview slot's fighter on the PreMatch level's character select, carried to
    /// where the slot's fighter is drawn now (`PreviewSet::char_select_follow`); None with no level, or no fighter's
    /// preview entry in.
    pub fn menu_char_select_follow(&self, slot: usize, at: [f64; 3]) -> Option<[f64; 3]> {
        self.menu.scene.as_ref()?;
        self.menu.previews.char_select_follow(slot, at)
    }

    /// Whether the scene has a place for the world space widget `w` (none for a banner with
    /// no preview); the program makes no render target for one it has not.
    pub fn menu_shows(&self, w: SceneWidget) -> bool {
        match &self.menu.scene {
            Some(LevelScene::Podium(l)) => l.shows(w),
            _ => false,
        }
    }

    /// The texture a world space widget of the scene draws (None: hidden).
    pub fn menu_widget(&mut self, w: SceneWidget, texture: Option<&Texture>) -> Result<()> {
        let Some(scene) = &mut self.menu.scene else { return Ok(()) };
        match scene {
            LevelScene::MainMenu(_) => Ok(()),
            LevelScene::PreMatch(l) => l.widget(w, texture),
            LevelScene::Podium(l) => l.widget(w, texture),
        }
    }

    /// The shown level's frame drawn, its picture to `out`; None with no level (the program draws its own frame).
    pub fn menu_draw(
        &mut self,
        vfx: Option<&mut mvs_vfx::Player>,
        out: Option<&Texture>,
    ) -> Result<Option<FrameTimes>> {
        if self.menu.scene.is_none() {
            return Ok(None);
        }
        main_menu::draw(self, vfx, out).map(Some)
    }
}
