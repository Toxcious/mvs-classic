//! The menus' screens on the shell: the title and the menus' levels, the menu music,
//! the fades between them (the manager's fader, painted by the view), a menu exit turned into the program's next
//! screen.
//!
//! The screens draw over the menus' level (the renderer's `menu_*` calls: the main menu's scene with its preview
//! character); the title, which shows none, over black. A store without the menus' scenes, or
//! a level that does not open, leaves the menus on black (logged once).

use std::time::Instant;

use mvs_audio::tape::post::{Call, Line};
use mvs_data::fighter::VoRow;
use mvs_net::service::api::{MenuFighter, PartyMember};
use mvs_render::Renderer;
use mvs_render::fx::Fx;
use mvs_render::menuscene::{MenuEvent, MenuLevel, PreviewSpec, SeqCmd};
use mvs_ui::menu::input::MenuPad;
use mvs_ui::menu::rich_text::RichTexts;
use mvs_ui::menu::scene::SceneCmd;
use mvs_ui::menu::screens::main_menu::{self, PartnerClick, PartnerNameplate};
use mvs_ui::menu::screens::pre_match::setup::{PreMatchEnd, PreMatchStart};
use mvs_ui::menu::screens::pre_match::{self, PreMatchScreen};
use mvs_ui::menu::screens::welcome::Welcome;
use mvs_ui::menu::scroll::ScrollBars;
use mvs_ui::menu::widgets::social_context_menu;
use mvs_ui::menu::{MenuExit, MenuInput, MenuRun, MenuScreen, MenuView};

use crate::app::{Frame, Go, MatchKind, MatchStart, MenuReturn, Shell, UiShared};
use crate::menu_audio::{AnnouncerEquip, MenuAudio};
use crate::pre_match_voice::PreMatchVoice;
use crate::script::{Fed, GATE_SETTLE, Gate, SETTLE_FRAMES};
use crate::{Error, Result};

/// The port's CalendarManager.GetActiveEvents (BP_EventRewardsLibrary Get Active Events) for a `menu_background`
/// setting (mvs-settings `UiSettings::menu_background`): the chosen Event Lookup Table slug alone, whose material the
/// main menu's background draws (MainMenuBackground UpdateBGForEvents); none for the background's own picture.
// Deviation: the build's list comes from the Hydra server's calendar (empty offline, so the default Trophy Room
// picture); the port has no calendar and lists the background the player picks with the home screen's switch,
// Festiversus (event_002, MI_MenuBG_Event002) by default (the user's choice). Only the background follows it: the
// event's PLAY snow strip and home screen buttons are not ported.
pub fn active_events(menu_background: &str) -> Vec<String> {
    if menu_background == mvs_settings::MENU_BACKGROUND_OWN { Vec::new() } else { vec![menu_background.to_owned()] }
}

/// The main menu's Player3 actor (BP_CharacterPreview_3 at Start_Char_2), where the party partner stands.
const PARTNER_SLOT: usize = 2;

/// The ms since `t` (a readout section's time).
fn ms_since(t: Instant) -> f64 {
    t.elapsed().as_secs_f64() * 1e3
}

/// The 60 Hz step a scripted run's clock moves by.
const STEP: f64 = 1.0 / 60.0;
/// The Delay before BPI_PlayMusOST, on the menu's frames.
const OST_DELAY: f64 = 0.2;
/// The frames the title is left alone before a `title` script's first.
const TITLE_SETTLE: u32 = 2;
const NO_UI: &str = "the menus need the UI (--no-ui)";

/// How a scripted run reaches its first frame: `title` leaves the title two frames;
/// `menu` presses the title's button, opens the main menu and lets it settle 360 frames; none of those are the
/// script's.
#[derive(Clone, Copy, Debug, Default)]
pub struct ScriptGate {
    /// The title is pressed by the run.
    pub press: bool,
    /// Frames left on the title before the script's first.
    pub title: u32,
    /// Frames left on the main menu before the script's first.
    pub menu: u32,
    /// Frames the main menu has been up while the script waits at a `menu` gate.
    pub menu_up: u32,
}

impl ScriptGate {
    pub fn new(menu_start: bool) -> ScriptGate {
        if menu_start {
            ScriptGate { press: true, title: 0, menu: SETTLE_FRAMES, menu_up: 0 }
        } else {
            ScriptGate { press: false, title: TITLE_SETTLE, menu: 0, menu_up: 0 }
        }
    }
}

/// The screen a menu host shows.
#[derive(Debug)]
enum Level {
    /// The WelcomeScreen level: the title.
    Title(Welcome),
    /// New_MainMenu.
    Menu,
    /// The PreMatch level: the pre-match screen and its voices.
    PreMatch(Box<PreMatchLevel>),
}

/// The pre-match screen over the PreMatch level.
#[derive(Debug)]
pub struct PreMatchLevel {
    screen: PreMatchScreen,
    voice: PreMatchVoice,
}

/// A menu level running (the title's or the menus').
#[derive(Debug)]
pub struct MenuHost {
    pub screen: MenuScreen,
    level: Level,
    /// The view's clock, s.
    clock: f64,
    /// The frames the host has run.
    frames: u32,
    /// The time left before BPI_PlayMusOST (the main menu's).
    ost: f64,
    /// The menus' effects (the previews' spawn effects), while a level is shown.
    fx: Option<Fx>,
    /// The menus' level failed (logged): the menus draw on black from then on.
    scene_failed: bool,
    /// The Player1 preview's clickable voice.
    vo: ClickVo,
    /// The Collection's announcer equip line.
    equip: AnnouncerEquip,
    /// The main menu's party partner (Player3 and the nameplate).
    partner: PartyPartner,
}

/// PreMatch's OnPartyChanged on the main menu (@22253-26421), on the leader's game and the member's alike: the
/// partner's fighter on the Player3 actor and its nameplate; on the pre-match the nameplate alone
/// (`PreMatchScreen::partner`: UpdatePlayerBadge @9872 / @14876 or the main menu's rule).
#[derive(Debug, Default)]
struct PartyPartner {
    /// The member shown (None: none asked for).
    shown: Option<PartyMember>,
    /// The fighter its game said its main menu shows, as Player3 last took it.
    look: Option<MenuFighter>,
    /// The fighter this player's main menu shows on Player1 (`MenuHost::open_main_menu`), for the partner's game.
    own: Option<MenuFighter>,
    /// The nameplate, made at the first partner (None before, or with no such class in the bundle).
    nameplate: Option<PartnerNameplate>,
    /// BP_SocialMenu_3's BannerClick, made with the nameplate.
    click: Option<PartnerClick>,
    /// The account the click target is on for (None: off).
    click_for: Option<String>,
    made: bool,
}

/// What a frame asks of the party partner's parts (PartyPartner::frame).
struct PartnerAsk<'a> {
    /// The level is up.
    up: bool,
    partner: Option<&'a PartyMember>,
    /// The fighter the partner's own main menu shows (None: its game sent none).
    look: Option<&'a MenuFighter>,
    /// Player3 follows the partner (the main menu's), else the nameplate alone (the pre-match's).
    fighter: bool,
    /// BP_Banner_PM_Player2's Enable Social Menu: the click target on for the partner's account.
    click: bool,
}

impl PartyPartner {
    /// One frame of the main menu: `partner` the party's first member other than this player. A partner arriving
    /// or changing, or the fighter its main menu shows (`look`) changing: the Player3 actor's
    /// BPI_LoadPreferredCharacter (`main_menu::partner_character`: that fighter, else HarleyWithJacket,
    /// @23860-24103); alone, its BPI_ClearFighter (@25788). The nameplate shown with the member's name and icon
    /// (@24851, @25059-25217) or hidden (@26040), and placed. Only while the main menu's level is up (`up`). A
    /// partner that fails to load is logged and shows nothing; the level stays up.
    // Unverified: the build runs this at each of Hydra's OnPartyChanged with a partner, and its Force true replays
    // the light beam each time; the port loads when the partner arrives or changes, or the fighter its main menu
    // shows changes, and clears when it goes.
    // (an unanswered invitee is in the build's InvitedMembers, not JoinedMembers: the partner shows from the accept,
    // as the port's party has joined members only)
    fn frame(&mut self, screen: &mut MenuScreen, run: &mut MenuRun<'_>, r: &mut Renderer, ask: PartnerAsk<'_>) {
        let PartnerAsk { up, partner, look, fighter, click } = ask;
        // BannerMover2 at PartnerNameplate_MM until CharSelect's MoveMMToCharSelect (@19194 -> @36462) takes it to
        // PartnerNameplate_PM; the port's pre-match level runs SetupCharSelect as it opens.
        // Deviation: the click target takes the pre-match's end place at once, not MMToCharSelect's lerp to it.
        let mover = if fighter { main_menu::PARTNER_MOVER_MM } else { main_menu::PARTNER_MOVER_PM };
        if !up {
            // (a level let go takes its previews with it: the partner is asked for again on the main menu's return)
            self.shown = None;
            self.look = None;
            self.click_for = None;
            if let Some(n) = &mut self.nameplate {
                n.set(screen, run, None);
                n.place(&mut screen.view, None);
            }
            if let Some(c) = &mut self.click {
                c.set(screen, run, None);
                c.place(&mut screen.view, None, [None; 4]);
            }
            return;
        }
        let look = partner.and(look);
        if fighter && (self.shown.as_ref().map(|m| &m.id) != partner.map(|m| &m.id) || self.look.as_ref() != look) {
            Self::fighter(screen, r, partner.map(|_| look));
        }
        self.look = look.cloned();
        if self.shown.as_ref() != partner {
            self.shown = partner.cloned();
            if partner.is_some() && !self.made {
                self.made = true;
                self.nameplate = main_menu::PartnerNameplate::new(screen, run);
                self.click = PartnerClick::new(screen, run);
            }
            if let Some(n) = &mut self.nameplate {
                n.set(screen, run, partner.map(|m| (m.name.as_str(), m.icon.as_str())));
            }
        }
        let click_for = partner.filter(|_| click).map(|m| m.id.clone());
        if click_for != self.click_for {
            self.click_for = click_for;
            if let Some(c) = &mut self.click {
                c.set(screen, run, self.click_for.as_deref());
            }
        }
        if let Some(n) = &self.nameplate {
            // Deviation: on the main menu the build leaves the plate at PartnerNameplate_PM (PARTNER_NAMEPLATE_AT),
            // which its camera (CameraTarget_MainMenu) shows far to the right of the partner; the port carries the
            // plate's place beside the partner on the fighter select (Player3 at Start_Char_2 moved by SetupCharSelect,
            // at its skin's CharSelect_Friend) to where Player3 is drawn on the main menu (its MainMenu_Friend, fitted),
            // so it sits on the partner as there (asked for by the user). No fighter shown: the build's place.
            let at = main_menu::PARTNER_NAMEPLATE_AT;
            let at = if fighter { r.menu_char_select_follow(PARTNER_SLOT, at).unwrap_or(at) } else { at };
            n.place(&mut screen.view, r.menu_screen_place(at));
        }
        if let Some(c) = &self.click {
            let at = main_menu::partner_click_at(mover);
            let [hw, hh] = main_menu::PARTNER_CLICK_HALF;
            let edge = |d: [f64; 3]| r.menu_screen_place([at[0] + d[0], at[1] + d[1], at[2] + d[2]]).map(|p| p.0);
            let edges = [edge([0.0, -hw, 0.0]), edge([0.0, hw, 0.0]), edge([0.0, 0.0, -hh]), edge([0.0, 0.0, hh])];
            let centre = r.menu_screen_place(at);
            c.place(&mut screen.view, centre, edges);
            // BP_SocialMenu ShowContextMenu @574-1056: ShowSocialActionsMenu(the account, BannerClick's screen
            // position, slug '') with Omit Actions [3, 7, 10]
            for id in c.take_clicks(&mut screen.view) {
                let Some((p, _)) = centre else { continue };
                screen.with_cx(run, |cx| {
                    let at = social_context_menu::pixels_to_viewport(cx, p);
                    let d = social_context_menu::player_data(cx, &id);
                    social_context_menu::show_social_actions_menu(cx, d, at, "", &[3, 7, 10]);
                });
            }
        }
    }

    /// Player3's fighter: with a partner (`partner` the fighter its main menu shows, if its game sent one), the
    /// partner's (`main_menu::partner_character`), or none.
    fn fighter(screen: &MenuScreen, r: &mut Renderer, partner: Option<Option<&MenuFighter>>) {
        let spec = if let Some(look) = partner {
            let sent = look.map(|f| (f.key.as_str(), f.skin.as_str()));
            let Some((key, skin)) = main_menu::partner_character(&screen.view, sent) else {
                eprintln!("mvs: the main menu's party partner: no fighter {sent:?} nor harley HarleyWithJacket");
                return;
            };
            Some(PreviewSpec { key, skin, team: 0 })
        } else {
            None
        };
        if let Err(e) = r.menu_preview(PARTNER_SLOT, spec) {
            eprintln!("mvs: the main menu's party partner: {e}");
        }
    }
}

/// The main menu's Player1 voice.
#[derive(Debug, Default)]
struct ClickVo {
    /// The shown fighter's VO rows.
    rows: Vec<VoRow>,
    /// The row last played.
    recent: f64,
    /// The menus' clock until which the line speaks, s.
    until: f64,
}

/// The scope that holds the main menu character's click lines.
const CLICK_SCOPE: &str = "menuClick";

impl ClickVo {
    /// Holds the rows' `clickable` lines, lazy (read when first posted: a click starts them, not a frame). The
    /// default character's click lines are held in a scope of their own here; the port sets no match scope before a
    /// match, so without this every click's line was dropped as not loaded.
    fn hold(&self, audio: &mut Option<mvs_audio::Audio>, tick: u32) {
        if audio.is_none() {
            return;
        }
        let mut lazy: Vec<String> = Vec::new();
        for r in self.rows.iter().filter(|r| r.0.eq_ignore_ascii_case("clickable")) {
            if let Some(ev) = r.3.as_ref().filter(|e| !e.is_empty()).map(|e| e.to_lowercase())
                && !lazy.contains(&ev)
            {
                lazy.push(ev);
            }
        }
        let call = Call::Scope { name: CLICK_SCOPE.to_owned(), keep: Vec::new(), lazy };
        crate::audio::send(audio, &Line { tick, call });
    }
}

/// A menu view on the shell's UI parts at the window's size, the session handed in.
pub fn menu_view(shell: &mut Shell) -> Result<MenuView> {
    let ui = shell.ui.as_ref().ok_or_else(|| Error::Match(NO_UI.into()))?;
    let mut view = MenuView::new(ui.menu.clone(), ui.text.clone(), shell.swap.size);
    view.session = std::mem::take(&mut shell.session);
    give_parts(&mut view, ui);
    Ok(view)
}

/// The view's scroll boxes and rich texts (every menu view has them: the main menus' and the match's and the Lab's).
pub fn give_parts(view: &mut MenuView, ui: &UiShared) {
    view.set_parts(Box::new(ScrollBars::new(ui.menu.clone())), Box::new(RichTexts::new(ui.menu.clone())));
}

/// The context the menus run in: the UI's parts, the settings and the sink.
pub fn menu_run<'a>(
    ui: &'a mut UiShared,
    settings: &'a mut mvs_settings::Settings,
    sink: &'a mut MenuAudio<'_>,
) -> MenuRun<'a> {
    let UiShared { menu_fonts, bindings, pointer, .. } = ui;
    MenuRun { fonts: menu_fonts, prefs: settings, bindings, sink, pointer, lab: None, pause: None, tutorial: None }
}

impl MenuHost {
    /// The title screen (UI_Welcome's level).
    pub fn title(shell: &mut Shell) -> Result<MenuHost> {
        let view = menu_view(shell)?;
        shell.music.preload(&mut shell.audio);
        let Shell { ui, settings, audio, music, window, .. } = shell;
        let ui = ui.as_mut().ok_or_else(|| Error::Match(NO_UI.into()))?;
        let mut sink =
            MenuAudio { audio, tick: music.tick, window: Some(crate::apply::window_info(window)), equip: None };
        let mut run = menu_run(ui, settings, &mut sink);
        // (WelcomeScreen is one of UI_Cursor's levels: the game's cursor shows over the title)
        let mut screen = MenuScreen::new(view, &mut run);
        screen.pointer_only();
        let welcome = Welcome::show(&mut screen, &mut run);
        // the WelcomeScreen level Blueprint's BPI_StartMusic and the switch (the native sound is never locked)
        music.title(audio);
        Ok(MenuHost {
            screen,
            level: Level::Title(welcome),
            clock: 0.0,
            frames: 0,
            ost: OST_DELAY,
            fx: None,
            scene_failed: false,
            vo: ClickVo::default(),
            equip: AnnouncerEquip::default(),
            partner: PartyPartner::default(),
        })
    }

    /// The menus, back from a match at `ret` (the main menu, or the custom lobby kept).
    pub fn new(shell: &mut Shell, ret: MenuReturn) -> Result<MenuHost> {
        let mut view = menu_view(shell)?;
        if let Some(lobby) = ret.lobby {
            view.session.lobby = Some(lobby);
        }
        // (an arcade run ends at the main menu: its card starts a new one)
        view.session.arcade.leave();
        let player1 = main_menu::preferred_character(&view, &shell.settings);
        let Shell { ui, settings, audio, music, window, .. } = shell;
        let ui = ui.as_mut().ok_or_else(|| Error::Match(NO_UI.into()))?;
        let mut sink =
            MenuAudio { audio, tick: music.tick, window: Some(crate::apply::window_info(window)), equip: None };
        let mut run = menu_run(ui, settings, &mut sink);
        let mut screen = MenuScreen::new(view, &mut run);
        main_menu::show(&mut screen, &mut run);
        music.menu(audio);
        let mut host = MenuHost {
            screen,
            level: Level::Menu,
            clock: 0.0,
            frames: 0,
            ost: OST_DELAY,
            fx: None,
            scene_failed: false,
            vo: ClickVo { until: -1.0, ..ClickVo::default() },
            equip: AnnouncerEquip::default(),
            partner: PartyPartner::default(),
        };
        if let Some(r) = shell.renderer.as_mut() {
            host.open_main_menu(r, player1);
        }
        host.vo.hold(&mut shell.audio, shell.music.tick);
        Ok(host)
    }

    /// New_MainMenu's level behind the main menu: its scene, and BeginPlay's
    /// BPI_LoadPreferredCharacter on the Player1 actor: `player1`, the fighter and skin of
    /// `main_menu::preferred_character` (none: the default character); the menus'
    /// effects player. A fighter that fails to load is logged and leaves the level up: its default skin is tried,
    /// then the actor shows none (as character select's change_fighter logs a failed preview and goes on).
    fn open_main_menu(&mut self, r: &mut Renderer, player1: Option<(String, String)>) {
        let (key, skin) = player1.unwrap_or_else(|| (mvs_sim::world::DEFAULT_CHARACTER.to_owned(), String::new()));
        let key = key.as_str();
        let opened = (|| -> Result<Fx> {
            r.set_menu_level(MenuLevel::MainMenu, self.fx.as_mut().map(|f| &mut f.player))?;
            Ok(Fx::new(r.assets().store(), 1)?)
        })();
        match opened {
            Ok(fx) => self.fx = Some(fx),
            Err(e) => self.scene_lost(r, &e),
        }
        let mut shown = false;
        self.partner.own = None;
        if !self.scene_failed {
            let skins = if skin.is_empty() { vec![skin] } else { vec![skin, String::new()] };
            for skin in skins {
                let spec = PreviewSpec { key: key.to_owned(), skin: skin.clone(), team: 0 };
                match r.menu_preview(0, Some(spec)) {
                    Ok(()) => {
                        shown = true;
                        // (the party partner's game shows it on its Player3)
                        self.partner.own = Some(MenuFighter { key: key.to_owned(), skin });
                        break;
                    }
                    Err(e) => eprintln!("mvs: the main menu's fighter {key} {skin:?}: {e}"),
                }
            }
        }
        // the character's VO rows for the click line
        let rows = (|| -> Result<Vec<VoRow>> {
            if !shown {
                return Ok(Vec::new());
            }
            let c = mvs_data::char_data(r.assets().store(), key)?;
            Ok(c.fighter.vo.clone().unwrap_or_default())
        })();
        self.vo.rows = rows.unwrap_or_else(|e| {
            eprintln!("mvs: the main menu character's voice: {e}");
            Vec::new()
        });
    }

    /// The menus' level failed: logged once, the level and its effects let go, the menus on black from then on.
    fn scene_lost(&mut self, r: &mut Renderer, e: &Error) {
        if !self.scene_failed {
            eprintln!("mvs: the menus' level: {e}; the menus draw on black");
        }
        self.scene_failed = true;
        self.fx = None;
        if let Err(e) = r.set_menu_level(MenuLevel::None, None) {
            eprintln!("mvs: the menus' level: {e}");
        }
    }

    /// The screens' calls into the level behind them, in order (none once the level failed).
    fn apply_scene(&mut self, r: &mut Renderer, sink: &mut MenuAudio<'_>, cmds: Vec<SceneCmd>) -> Result<()> {
        for cmd in cmds {
            if self.scene_failed {
                return Ok(());
            }
            match cmd {
                SceneCmd::ClickVo => {
                    if let Some(line) = self.click_vo(r)
                        && mvs_ui::menu::MenuSink::has_event(sink, &line)
                    {
                        mvs_ui::menu::MenuSink::post(sink, &line);
                    }
                }
                other => Self::apply_cmd(&mut self.fx, r, other)?,
            }
        }
        Ok(())
    }

    /// One of the screens' calls into the level (the effects player is the menus' one, made with the first level).
    fn apply_cmd(fx: &mut Option<Fx>, r: &mut Renderer, cmd: SceneCmd) -> Result<()> {
        match cmd {
            SceneCmd::Level(l) => {
                if l != r.menu_level() {
                    // the effects go with the level
                    if let Some(fx) = fx {
                        fx.clear();
                    }
                }
                r.set_menu_level(l, fx.as_mut().map(|f| &mut f.player))?;
                if l != MenuLevel::None && fx.is_none() {
                    *fx = Some(Fx::new(r.assets().store(), 1)?);
                }
            }
            SceneCmd::Preview { slot, spec } => r.menu_preview(slot, spec)?,
            SceneCmd::Sequence(s) => r.menu_sequence(s)?,
            SceneCmd::Banner { slot, slug } => r.menu_banner(slot, slug.as_deref())?,
            SceneCmd::BannerIcon { slot, path } => r.menu_banner_icon(slot, path.as_deref()),
            SceneCmd::Fade(a) => r.set_post_fade(a),
            SceneCmd::Hover { slot, on } => r.menu_hover(slot, on),
            SceneCmd::TeamStyle(s) => r.set_menu_team_style(s),
            // (the main menu's click voice, taken by apply_scene)
            SceneCmd::ClickVo => {}
        }
        Ok(())
    }

    /// The Player1 preview's `clickable` line with the affinity_level_01 tag, none while the
    /// last speaks. Deviation: the end is the line's length from the data, not the engine's callback;
    /// VoiceOverEnabled is taken as on; the line is posted without a position, not on the actor's AkComponent.
    // Deviation: the time is the menus' clock (a scripted run's frames), not a wall clock.
    fn click_vo(&mut self, r: &Renderer) -> Option<String> {
        let vo = &mut self.vo;
        if vo.rows.is_empty() || !r.menu_has_fighter(0) || self.clock < vo.until {
            return None;
        }
        let mut random = crate::lab_setup::random_source();
        let pick = mvs_sim::fighter::match_side::select_vo(
            &vo.rows,
            "clickable",
            &["affinity_level_01"],
            mvs_sim::math::narrow(vo.recent),
            |n| mvs_sim::math::narrow((random() * f64::from(n)).floor()),
        );
        let (_, _, _, ev, dur) = vo.rows.get(usize::try_from(pick).ok()?)?;
        let ev = ev.clone()?;
        vo.recent = f64::from(pick);
        if let Some(d) = dur
            && !ev.contains("eff")
        {
            vo.until = self.clock + d;
        }
        Some(ev)
    }

    /// The level's notes of the frame: its sounds posted (the rest is the pre-match and podium screens').
    fn drain_events(r: &mut Renderer, sink: &mut MenuAudio<'_>) {
        for e in r.take_menu_events() {
            if let MenuEvent::Sound { event, at } = e {
                sink.post_at(&event, at);
            }
        }
    }

    /// The pre-match screen over the PreMatch level, opened by a custom lobby's Start:
    /// the pre-match voice, the preview hooks, the screen).
    pub fn pre_match(shell: &mut Shell, start: PreMatchStart) -> Result<MenuHost> {
        let view = menu_view(shell)?;
        let Shell { ui, settings, audio, music, window, renderer, .. } = shell;
        let ui = ui.as_mut().ok_or_else(|| Error::Match(NO_UI.into()))?;
        let store = renderer.as_ref().ok_or_else(|| Error::Match("no store is open".into()))?.assets().store();
        let keys: Vec<&str> = start.setup.players.iter().filter_map(|p| p.character.as_deref()).collect();
        let pack = &settings.hydra.announcer_pack.path;
        let voice = PreMatchVoice::new(store, audio, music.tick, &start.setup.level, &keys, pack);
        let mut sink =
            MenuAudio { audio, tick: music.tick, window: Some(crate::apply::window_info(window)), equip: None };
        let mut run = menu_run(ui, settings, &mut sink);
        let mut screen = MenuScreen::new(view, &mut run);
        let pm = pre_match::show(&mut screen, &mut run, start);
        Ok(MenuHost {
            screen,
            level: Level::PreMatch(Box::new(PreMatchLevel { screen: pm, voice })),
            clock: 0.0,
            frames: 0,
            ost: 0.0,
            fx: None,
            scene_failed: false,
            vo: ClickVo::default(),
            equip: AnnouncerEquip::default(),
            partner: PartyPartner::default(),
        })
    }

    /// The window's input of the frame in the order it came, with the first pad's state.
    pub(crate) fn live_input(shell: &mut Shell) -> Vec<MenuInput> {
        let input = shell.window.take_menu_input();
        crate::pause::with_pads(shell, input)
    }

    /// A scripted frame's input: nothing while the run settles (the title's press, the menu's 360 frames), then the
    /// script's frame. The script's frame number is the second.
    fn scripted_input(&mut self, shell: &mut Shell) -> (Vec<MenuInput>, Option<u32>) {
        let mut input = Vec::new();
        // (a `menu` gate presses the title again each frame, ignored while the title fades in)
        let menu_gate = shell.script.as_ref().is_some_and(|s| s.player.waiting() == Some(Gate::Menu));
        let gate = &mut shell.script_gate;
        let open = match &self.level {
            Level::Title(_) if gate.press || menu_gate => {
                input.push(MenuInput::Key { key: "Enter", down: true, repeat: false, player: 0 });
                false
            }
            Level::Title(_) => {
                let open = gate.title == 0;
                gate.title = gate.title.saturating_sub(1);
                open
            }
            Level::PreMatch(_) => true,
            Level::Menu => {
                let open = gate.menu == 0;
                gate.menu = gate.menu.saturating_sub(1);
                open
            }
        };
        let Some(s) = shell.script.as_mut().filter(|_| open) else { return (input, None) };
        // A `menu` gate: once the main menu is up, GATE_SETTLE frames, then the
        // script goes on; the frames of the wait are not the script's. (A `lab` gate is the match's.)
        if s.player.waiting() == Some(Gate::Menu) && matches!(self.level, Level::Menu) {
            gate.menu_up += 1;
            if gate.menu_up <= GATE_SETTLE {
                return (input, None);
            }
            gate.menu_up = 0;
            s.player.release();
        }
        if s.player.waiting().is_some() {
            return (input, None);
        }
        let n = s.player.frame();
        let fed = s.feed();
        for fed in fed {
            match fed {
                Fed::Input(i) => input.push(i),
                Fed::Pad(p) => s.pad = Some(MenuPad { axes: p.axes, buttons: p.buttons }),
            }
        }
        // (a script's pad stub: a `pad` line's state holds from its frame on, read by each frame's poll)
        if let Some(state) = s.pad {
            input.push(MenuInput::Pad { pad: 0, player: 0, state });
        }
        // (a gate's frame: the events before the gate are fed now, the frame is the script's once it is released)
        if s.player.waiting().is_some() {
            return (input, None);
        }
        (input, Some(n))
    }

    /// One frame: the window's input to the screen, its frame, the element list drawn on the back buffer, the
    /// present; the screen's exit as the program's next screen.
    pub fn frame(&mut self, shell: &mut Shell, f: &Frame) -> Result<Go> {
        if let Some(size) = f.resized {
            self.screen.view.resize(size);
        }
        let scripted = shell.script.is_some();
        let waits_loads = shell.waits_loads();
        let (mut input, script_frame) = if scripted {
            self.clock = f64::from(self.frames) * STEP;
            self.scripted_input(shell)
        } else {
            self.clock += f.dt;
            (Self::live_input(shell), None)
        };
        let dt = if scripted { STEP } else { f.dt };
        self.frames += 1;
        shell.music.tick = shell.music.tick.wrapping_add(1);

        online_before(shell, &mut self.screen.view);
        let Shell {
            ui,
            settings,
            audio,
            music,
            session,
            window,
            swap,
            renderer,
            script,
            script_out,
            perf,
            online,
            online_feed,
            real_pace,
            ..
        } = shell;
        let ui = ui.as_mut().ok_or_else(|| Error::Match(NO_UI.into()))?;
        let mut list = std::mem::take(&mut ui.list);
        list.clear();
        let mut go = Go::Stay;
        let mut posted = false;
        let exit;
        // the pre-match level's: the calls and the previews' settles for the voices, the screen's end
        let mut pm_calls = Vec::new();
        let mut pm_cmds = Vec::new();
        let mut pm_settled = Vec::new();
        let mut pm_sounds = Vec::new();
        let mut pm_music = None;
        let mut pm_end = None;
        {
            let mut sink = MenuAudio {
                audio,
                tick: music.tick,
                window: Some(crate::apply::window_info(window)),
                equip: Some(&mut self.equip),
            };
            let mut run = menu_run(ui, settings, &mut sink);
            // (the script's `do` actions of the frame, before its input)
            if let Some(s) = script.as_mut() {
                let n = s.player.frame().saturating_sub(1);
                for (what, args) in s.player.take_dos() {
                    if what == "click" {
                        let name = args.get(0).and_then(serde_json::Value::as_str).unwrap_or_default().to_owned();
                        s.clicks.push((name, args.get(1).and_then(serde_json::Value::as_u64)));
                        continue;
                    }
                    let hub = online.as_ref().map(|h| h.borrow());
                    run_do(&mut self.screen, &mut run, hub.as_deref(), n, &what, &args);
                }
                script_click(s, &self.screen, n, &mut input);
            }
            let t_screen = Instant::now();
            // the screen layer's world widgets (the pre-match nametags) fade with the level (view.rs world_opacity)
            self.screen.view.world_opacity = renderer.as_ref().map_or(1.0, |r| 1.0 - f64::from(r.post_fade()));
            exit = match &mut self.level {
                Level::Title(w) => {
                    // The title takes the press of its button alone; its screen is pointer_only: the pointer and
                    // the sticks move the game's cursor, a key or a button only names the device and turns cursor
                    // mode as elsewhere.
                    // Deviation: the build's WelcomeScreen takes every key and its cursor would click under a key
                    // press; the port's Welcome driver takes the press, so the manager navigates and clicks nothing.
                    w.input(&mut self.screen, &mut run, &input);
                    self.screen.frame(&mut run, &input, self.clock, &mut list);
                    posted = w.take_pressed(&mut self.screen);
                    if w.removed(&mut self.screen) {
                        go = Go::Menu(MenuReturn::default());
                    }
                    None
                }
                Level::Menu => self.screen.frame(&mut run, &input, self.clock, &mut list),
                Level::PreMatch(p) => {
                    let exit = self.screen.frame(&mut run, &input, self.clock, &mut list);
                    pm_calls = p.screen.take_calls(&mut self.screen);
                    if let Some(r) = renderer.as_mut() {
                        pm_cmds = self.screen.view.take_scene_cmds();
                        for cmd in pm_cmds.clone() {
                            if let Err(e) = Self::apply_cmd(&mut self.fx, r, cmd) {
                                eprintln!("mvs: menu level: {e}");
                            }
                        }
                        // a level that failed (no level behind the screen) leaves the face-off to the screen's
                        // own times (PreMatch.timedFaceOff)
                        if r.menu_level() != mvs_render::menuscene::MenuLevel::PreMatch {
                            p.screen.set_level(&mut self.screen, false);
                        }
                        // UI_Cursor's MousePosNormalized: the banners' parallax
                        r.set_menu_cursor(run.pointer.mouse_pos_normalized);
                        if let Err(e) = r.menu_tick(dt, self.fx.as_mut().map(|f| &mut f.player)) {
                            eprintln!("mvs: menu level: {e}");
                        }
                        for ev in r.take_menu_events() {
                            match ev {
                                mvs_render::menuscene::MenuEvent::PreviewSettled { slot, key } => {
                                    pm_settled.push((slot, key));
                                }
                                mvs_render::menuscene::MenuEvent::Sound { event, at } => pm_sounds.push((event, at)),
                                // the sequence's Fade track is the post chain's
                                mvs_render::menuscene::MenuEvent::Fade(a) => r.set_post_fade(a as f32),
                                other => p.screen.scene_event(&mut self.screen, &mut run, &other),
                            }
                        }
                        // (the level's music switch: SetupCharSelect's mus_menu_chaselect)
                        pm_music = r.take_menu_music();
                        let plates = r.menu_plates();
                        p.screen.project(
                            &mut self.screen,
                            &mut run,
                            |at| r.menu_project(at),
                            &plates,
                            |at| r.menu_screen_place(at),
                        );
                    }
                    match exit {
                        Some(MenuExit::OpenLevel { level, options }) => {
                            pm_end = Some(p.screen.take_end(&mut self.screen, &mut run, &level));
                            Some(MenuExit::OpenLevel { level, options })
                        }
                        other => other,
                    }
                }
            };
            if let Some(r) = renderer.as_mut() {
                let at = r.menu_level();
                // (this player's main-menu fighter, for the partner's game: the last one its main menu showed)
                if let Some(h) = online.as_ref() {
                    h.borrow_mut().set_menu_fighter(self.partner.own.clone());
                }
                let look = online.as_ref().and_then(|h| h.borrow().party_partner_fighter().cloned());
                let (up, partner, fighter, click) = match &mut self.level {
                    Level::Menu => {
                        let partner = online.as_ref().and_then(|h| h.borrow().party_partner().cloned());
                        let on = partner.is_some();
                        (!self.scene_failed && at == MenuLevel::MainMenu, partner, true, on)
                    }
                    // Deviation: the build's queue pre-match is the main menu's PreMatch level going on (Player3's
                    // HarleyWithJacket stays); the port opens the level again and shows the nameplate and the click
                    // target alone on it (Player3 is the screen's own preview slot).
                    Level::PreMatch(p) if at == MenuLevel::PreMatch => {
                        let party = self.screen.view.session.online.party.clone();
                        // (the level's PlayTransition of the frame, before the party's change of it)
                        for cmd in &pm_cmds {
                            if let SceneCmd::Sequence(SeqCmd::Transition(t)) = cmd {
                                p.screen.partner_transition(t, party.as_ref());
                            }
                        }
                        let partner = p.screen.partner(&mut self.screen, party.as_ref());
                        (true, partner, false, p.screen.partner_click_on())
                    }
                    _ => (false, None, false, false),
                };
                let ask = PartnerAsk { up, partner: partner.as_ref(), look: look.as_ref(), fighter, click };
                self.partner.frame(&mut self.screen, &mut run, r, ask);
            }
            perf.section("menu", ms_since(t_screen));
            if posted {
                mvs_ui::menu::MenuSink::post(run.sink, "ui_menu_welcome_play");
            }
            if exit.is_none() && matches!(go, Go::Stay) && self.screen.view.session.quit {
                go = Go::Quit;
            }
            if exit.is_some() || !matches!(go, Go::Stay) {
                self.screen.remove(&mut run);
            }
        }
        if let Some(h) = online.as_ref() {
            online_after(h, settings, ui, &mut self.screen.view, &mut go);
            // (the online pre-match's or the queue's match, at the screen's level)
            let to_match = matches!(&exit, Some(MenuExit::OpenLevel { level, .. })
                if level != "New_MainMenu" && level != "TrainingContainer");
            // (taken in a statement of its own: a let chain's borrow would hold the hub through the start)
            let started = if to_match { h.borrow_mut().started.take() } else { None };
            if let Some(m) = started {
                let view = &self.screen.view;
                let drawable = |c: &str| view.fighters().contains_key(c);
                if let Some(start) = crate::online::glue::match_start_of(h, &m, &drawable, online_feed.clone()) {
                    go = Go::Load(Box::new(start));
                }
            }
            // (the PreMatch level on an online pre-match: an online lobby's start)
            if matches!(&exit, Some(MenuExit::OpenLevel { level, .. }) if level == "PreMatch")
                && matches!(go, Go::Stay)
                && let Some(start) =
                    online_pre_match(&h.borrow(), &settings.profile.name, &mut self.screen.view.session)
            {
                go = Go::PreMatch(Box::new(start));
            }
            if exit.is_some() || pm_end.is_some() {
                let level = match &exit {
                    Some(MenuExit::OpenLevel { level, .. }) => Some(level.as_str()),
                    _ => None,
                };
                h.borrow_mut().screen_done(level);
            }
        }
        if let Some(exit) = exit
            && matches!(go, Go::Stay)
        {
            let fighters = ui.menu.menu.as_ref().map(|m| m.fighters.clone()).unwrap_or_default();
            let roster = ui.menu.menu.as_ref().map_or(&[][..], |m| m.roster.as_slice());
            // (Wait in the Lab: the queue's lock is the Lab's owner)
            let mine = online.as_ref().and_then(|h| h.borrow().lab_queue().and_then(|q| q.mine.clone()));
            let store = renderer.as_ref().map(|r| r.assets().store());
            go = Self::go_of(&mut self.screen.view, exit, settings, &fighters, roster, mine.as_ref(), store);
        }
        // (the Collection's announcer equip line, its pack read from the store)
        self.equip.frame(renderer.as_ref().map(|r| r.assets().store()), audio, music.tick, self.clock);
        if !pm_sounds.is_empty() {
            // the level's sounds posted, as the drain of the other levels does
            let mut sink =
                MenuAudio { audio, tick: music.tick, window: Some(crate::apply::window_info(window)), equip: None };
            for (event, at) in &pm_sounds {
                sink.post_at(event, *at);
            }
        }
        if let Some(value) = pm_music {
            music.level_switch(audio, value);
        }
        if let Level::PreMatch(p) = &mut self.level {
            // (Back to Lobby: the lobby is in the session, the main menu shows it)
            if matches!(pm_end, Some(PreMatchEnd::BackToLobby)) {
                go = Go::Menu(MenuReturn::default());
            }
            if let Some(store) = renderer.as_ref().map(|r| r.assets().store()) {
                p.voice.tick = music.tick;
                p.voice.scene_cmds(store, audio, &pm_cmds);
                for (slot, key) in &pm_settled {
                    p.voice.settled(store, audio, *slot, key);
                }
                p.voice.calls(store, audio, self.clock, pm_calls);
            }
            if pm_end.is_some() {
                p.voice.end(audio);
            }
        }
        if matches!(self.level, Level::Menu) && self.ost > 0.0 {
            self.ost -= dt;
            if self.ost <= 0.0 {
                music.play_mus_ost(audio);
            }
        }
        // (an active screen with the game's cursor owns the pointer: the main menu's and the pre-match's, not the title)
        window.set_cursor_hidden(ui.pointer.os_cursor_hidden());

        // the draw: the level (black with none), the elements over it, the picture of a shot due, the present
        let renderer = renderer.as_mut().ok_or_else(|| Error::Match("no store is open".into()))?;
        let mut sink =
            MenuAudio { audio, tick: music.tick, window: Some(crate::apply::window_info(window)), equip: None };
        // (the PreMatch level's screen took its commands above, with its events: its level is ticked there)
        renderer.set_menu_wait_loads(waits_loads);
        let cmds = self.screen.view.take_scene_cmds();
        if let Err(e) = self.apply_scene(renderer, &mut sink, cmds) {
            self.scene_lost(renderer, &e);
        }
        let pre_match = matches!(self.level, Level::PreMatch(_));
        let back = swap.back_buffer();
        let t_scene = Instant::now();
        let mut drawn = false;
        if renderer.menu_level() != MenuLevel::None {
            // (the level's tick, then its frame under the screens)
            let shown = (|| {
                if !pre_match {
                    renderer.menu_tick(dt, self.fx.as_mut().map(|f| &mut f.player))?;
                }
                renderer.menu_draw(self.fx.as_mut().map(|f| &mut f.player), Some(back))
            })();
            if !pre_match {
                Self::drain_events(renderer, &mut sink);
            }
            match shown {
                Ok(times) => {
                    drawn = true;
                    if let Some(t) = times {
                        perf.gpu(t.gpu, &t.passes);
                    }
                }
                Err(e) => self.scene_lost(renderer, &e.into()),
            }
        }
        if !drawn && let Some(rtv) = &back.rtv {
            // SAFETY: a render target view of the swap chain's back buffer, on this device's context.
            unsafe { renderer.gpu().ctx.ClearRenderTargetView(rtv, &[0.0, 0.0, 0.0, 1.0]) };
        }
        perf.section("scene", ms_since(t_scene));
        let (gpu, assets) = (renderer.gpu(), renderer.assets());
        // `stat fps`'s line (Show Framerate) and the ping line of an online session; not on a scripted run's
        // pictures unless it is paced (they are not compared then)
        let lines = !scripted || *real_pace;
        let fps = crate::fps::line(settings.graphics.show_fps && lines, perf.readout.as_ref());
        let ping = crate::fps::ping_line(online.as_ref().filter(|_| lines).and_then(|h| h.borrow().ping_ms()));
        crate::fps::draw(&mut list, &mut ui.menu_fonts, swap.size, crate::fps::LAYER, fps.as_ref(), ping.as_ref());
        ui.ui.time = Some(self.clock);
        // (a scripted run's pictures are compared: they wait for their pictures, unless the run is paced)
        ui.ui.stream_pictures = !scripted || *real_pace;
        let t_ui = Instant::now();
        ui.timer.begin_frame(gpu);
        ui.ui.draw(gpu, assets, &mut ui.menu_fonts, &list, back)?;
        ui.timer.mark(gpu, "ui");
        ui.timer.end_frame(gpu);
        if let Some(m) = ui.timer.read(gpu) {
            for (n, v) in m {
                perf.gpu_pass(n, v);
            }
        }
        perf.section("ui", ms_since(t_ui));
        ui.list = list;
        let mut over = false;
        if let (Some(n), Some(s)) = (script_frame, script.as_mut()) {
            let t_tape = Instant::now();
            let mut tape = self.screen.tape_frame();
            tape.frame = n;
            if s.painted(tape) {
                let px = renderer.read_back(back)?;
                let [w, h] = swap.size;
                let path = script_out.join(format!("{}-f{n}.png", s.player.script().name));
                mvs_render::image::write_png(&path, w, h, &px).map_err(Error::Match)?;
                eprintln!("mvs: {}", path.display());
            }
            over = s.done();
            perf.section("tape", ms_since(t_tape));
        }
        // (a scripted run presents unsynced, its frames as fast as they come, unless it is paced: --real-pace)
        swap.present(!scripted || *real_pace)?;
        if over && let Some(s) = script.take() {
            let (now, made) = self.screen.view.widget_counts();
            eprintln!("mvs: menu widgets {now} held of {made} made");
            s.finish()?;
            go = Go::Quit;
        }
        if !matches!(go, Go::Stay) {
            *session = std::mem::take(&mut self.screen.view.session);
            // (the scene and its effects let go with the menus)
            if let Some(fx) = &mut self.fx {
                fx.clear();
            }
            if let Err(e) = renderer.set_menu_level(MenuLevel::None, self.fx.as_mut().map(|f| &mut f.player)) {
                eprintln!("mvs: the menus' level: {e}");
            }
            self.fx = None;
        }
        Ok(go)
    }

    /// A menu exit as the program's next screen (after the menu: the Lab, or the custom game
    /// lobby's or Local Play's match).
    fn go_of(
        view: &mut MenuView,
        exit: MenuExit,
        settings: &mvs_settings::Settings,
        fighters: &crate::lab_setup::Fighters,
        roster: &[mvs_ui::bundle::RosterEntry],
        mine: Option<&mvs_ui::menu::online::NetPick>,
        store: Option<&mvs_store::Store>,
    ) -> Go {
        let session = &mut view.session;
        match exit {
            MenuExit::Quit => Go::Quit,
            // (the launcher is a program of its own, open beside the game since its Play)
            MenuExit::Launcher => Go::Quit,
            MenuExit::Match(m) => Go::Load(Box::new(Self::start_of(session, *m, fighters, roster))),
            MenuExit::PreMatch(start) => Go::PreMatch(start),
            MenuExit::OpenLevel { level, options } => {
                eprintln!("mvs: menu opens level {level} ({options})");
                match level.as_str() {
                    // THE LAB: the first config's set-up.
                    "TrainingContainer" => {
                        let seed = std::time::SystemTime::now()
                            .duration_since(std::time::UNIX_EPOCH)
                            .map_or(1, |d| (d.as_nanos() & 0x7fff_ffff) as i32);
                        let setup = crate::lab_setup::first_setup(settings, fighters, roster, mine);
                        let usernames = crate::lab_setup::first_usernames(settings, fighters, &setup);
                        let mut start = MatchStart::new(setup, MatchKind::Lab { carry: None }, seed);
                        start.usernames = usernames;
                        Go::Load(Box::new(start))
                    }
                    "New_MainMenu" => Go::Menu(MenuReturn::default()),
                    // A tutorial's OpenLevel (UI_MainMenu BPI_LaunchTutorial): Map_TrophyRoom_Tutorial, whose
                    // WorldSettings' DefaultGameMode is TutorialGameMode_C, runs the game instance's tutorial actor
                    // (Intro when none was set). Without its data the main menu opens again.
                    mvs_ui::menu::tutorial::TUTORIAL_MAP => {
                        let tutorials = store.map(crate::tutorial::stored_tutorials).unwrap_or_default();
                        let actor = view.session.tutorial_actor.clone();
                        let seed = std::time::SystemTime::now()
                            .duration_since(std::time::UNIX_EPOCH)
                            .map_or(1, |d| (d.as_nanos() & 0x7fff_ffff) as i32);
                        match crate::tutorial::tutorial_match(
                            &tutorials,
                            actor.as_deref(),
                            settings,
                            view.roster(),
                            seed,
                        ) {
                            Some(start) => Go::Load(Box::new(start)),
                            None => Go::Menu(MenuReturn::default()),
                        }
                    }
                    // any other level: the Local Play match the facilitator made, else the custom lobby's
                    // (the taken Local Play match, else the kept lobby's match set-up)
                    _ => {
                        if let Some(started) = session.started.take() {
                            return Go::Load(Box::new(Self::start_of(session, started.lobby, fighters, roster)));
                        }
                        if let Some(m) = mvs_ui::menu::lobby::take_start() {
                            return Go::Load(Box::new(Self::start_of(session, *m, fighters, roster)));
                        }
                        eprintln!("mvs: no match for level {level}: back to the main menu");
                        Go::Menu(MenuReturn::default())
                    }
                }
            }
        }
    }

    /// The match a lobby, Local Play or an arcade run made as the program's: Local Play when it has its humans, else
    /// a Waves run (its set-up's mode; the seed its cast was drawn from, `WavesState::seed`), else
    /// the arcade's round when a run is in progress, else the kept custom lobby's (taken from
    /// the session, shown again at the match's end; an arcade round leaves it kept). Each bot is named by the lobby's
    /// bot username (its character's DisplayName, else the lobby's name); an arcade round's bot that has
    /// a name keeps it.
    fn start_of(
        session: &mut mvs_ui::menu::session::Session,
        mut m: mvs_ui::menu::lobby::LobbyMatch,
        fighters: &crate::lab_setup::Fighters,
        roster: &[mvs_ui::bundle::RosterEntry],
    ) -> MatchStart {
        let mut seed = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(1, |d| (d.as_nanos() & 0x7fff_ffff) as i32);
        let waves = match &m.setup.mode {
            Some(mvs_sim::setup::MatchMode::Waves { variant, .. }) => Some(*variant),
            _ => None,
        };
        let kind = if m.humans.is_some() {
            MatchKind::LocalPlay
        } else if let Some(variant) = waves {
            seed = session.waves.seed.take().unwrap_or(seed);
            MatchKind::Waves { variant }
        } else if session.arcade.live().is_some() && m.setup.arcade.is_some() {
            MatchKind::Arcade
        } else if let Some(lobby) = m.lobby.take().or_else(|| session.lobby.take()) {
            MatchKind::Custom { lobby }
        } else {
            MatchKind::Default
        };
        let arcade = matches!(kind, MatchKind::Arcade);
        for (n, p) in m.usernames.iter_mut().zip(&m.setup.players) {
            if p.bot && !(arcade && !n.is_empty()) {
                *n = mvs_ui::menu::screens::pre_match::bot_username(fighters, roster, p.character.as_deref(), n);
            }
        }
        let mut start = MatchStart::new(m.setup, kind, seed);
        start.me = m.me;
        start.usernames = m.usernames;
        start.map = m.map;
        // (each human on the device the devices table gave its local player)
        start.humans = m.humans.map(|hs| hs.iter().map(|h| (h.slot, device_kind(h.device), h.set)).collect());
        start
    }
}

/// The app's device of a Local Play human's (the devices table's) device.
fn device_kind(d: mvs_ui::menu::local_play_match::Device) -> crate::input::DeviceKind {
    use crate::input::DeviceKind;
    use mvs_ui::menu::local_play_match::Device;
    match d {
        Device::Kbm => DeviceKind::Kbm,
        Device::KbmPad0 => DeviceKind::KbmPad0,
        Device::Pad(p) => DeviceKind::Pad(p),
    }
}

/// A script's `do` line: the actions the menus have, run on the screen; the
/// state reads logged with their answers. Not run (logged): the lobby's hand-set rules, the social
/// panel's hand fill, the join box's hand-over and the match's `win` (the harness's step and podium), which are
/// other screens' or the match's. The port's own `widgets` (the two-exe runs) logs the painted widgets; its `click` is
/// taken by `frame`.
fn run_do(
    screen: &mut MenuScreen,
    run: &mut MenuRun<'_>,
    hub: Option<&crate::online::hub::OnlineHub>,
    frame: u32,
    what: &str,
    args: &serde_json::Value,
) {
    use mvs_ui::menu::manager::Manager;
    let s = |i: usize| args.get(i).and_then(serde_json::Value::as_str).unwrap_or_default().to_owned();
    match what {
        "showModal" | "matchOptions" => {
            let (cls, id) =
                if what == "matchOptions" { ("WBP_MatchOptions".into(), "matchoptions".into()) } else { (s(0), s(1)) };
            // (a third argument is the modal's instructions; no script gives one)
            let instructions = s(2);
            screen.with_cx(run, |cx| Manager::show_modal(cx, &cls, &id, &instructions, ""));
        }
        "dismissModal" => {
            let id = args.as_str().unwrap_or_default().to_owned();
            screen.with_cx(run, |cx| Manager::dismiss_modal(cx, &id));
        }
        // the mode card's online tab (WBP_MatchOptions typeBar.setCurrentIndexByID('online'))
        "onlineTab" => screen.with_cx(run, |cx| {
            let Some(mo) = cx.view.manager.get_modal("matchoptions") else {
                eprintln!("mvs: script: frame {frame}: do onlineTab: no WBP_MatchOptions");
                return;
            };
            let bar = cx.view.tree().find_in(mo, "UI_SectionBar_Type");
            let done = cx.visit::<mvs_ui::menu::widgets::section_bar::SectionBar, _>(bar, |b, cx| {
                b.set_current_index_by_id(cx, "online");
            });
            if done.is_none() {
                eprintln!("mvs: script: frame {frame}: do onlineTab: no type bar");
            }
        }),
        "queueState" => queue_state(hub, frame),
        "modals" => {
            let up: Vec<&str> = ["matchoptions", "joinLobby", "customGame"]
                .into_iter()
                .filter(|id| screen.view.manager.get_modal(id).is_some())
                .collect();
            eprintln!("mvs: script: frame {frame}: do modals -> {}", up.join(" "));
        }
        // `click`, `widgets` and `joinCode` are the port's own: the two-exe runs' scripts press a widget by its name
        // wherever the screen put it, once it is shown.
        // the join box's GO with this code (the lobby's pending join, then the lobby)
        "joinCode" => {
            mvs_ui::menu::lobby::set_pending_join(Some(s(0)));
            screen.with_cx(run, |cx| Manager::show_modal(cx, "WBP_CustomGame", "customGame", "", ""));
        }
        "lobby" => screen.with_cx(run, |cx| {
            let Some(m) = cx.view.manager.get_modal("customGame") else {
                eprintln!("mvs: script: frame {frame}: do lobby -> no customGame");
                return;
            };
            let line = cx.visit::<mvs_ui::menu::widgets::custom_game::CustomGame, _>(m, |g, cx| g.probe(cx));
            eprintln!("mvs: script: frame {frame}: do lobby -> {}", line.unwrap_or_default());
        }),
        "widgets" => {
            let tape = screen.view.tape_frame(frame);
            let want = s(0);
            for r in &tape.roots {
                for w in r.widgets.iter().filter(|w| w.vis != 1 && w.vis != 2 && w.path.contains(want.as_str())) {
                    let text = w.text.as_deref().map(|t| format!(" {t:?}")).unwrap_or_default();
                    eprintln!("mvs: script: frame {frame}: widget {} {} at {:?}{text}", w.path, w.ty, widget_centre(w));
                }
            }
        }
        o => eprintln!("mvs: script: frame {frame}: do {o} {args}: not run on the menus"),
    }
}

/// The centre of a painted widget, device pixels.
fn widget_centre(w: &mvs_ui::tape::TapeWidget) -> [f64; 2] {
    let [a, b, c, d, e, f] = w.matrix;
    let [x, y] = [w.size[0] / 2.0, w.size[1] / 2.0];
    [a * x + c * y + e, b * x + d * y + f]
}

/// A script's `do click` lines on `screen` at the script's frame `n`: a click waits, frame by frame, for its widget
/// to be shown; its move, press and release go one a frame, so the widget is hovered before it is pressed.
pub(crate) fn script_click(s: &mut crate::script::ScriptRun, screen: &MenuScreen, n: u32, input: &mut Vec<MenuInput>) {
    if !s.pressing.is_empty() {
        input.push(s.pressing.remove(0));
    } else if let Some((name, nth)) = s.clicks.first().cloned() {
        let mut fed = click_widget(screen, n, &name, nth);
        if !fed.is_empty() {
            s.clicks.remove(0);
            input.push(fed.remove(0));
            s.pressing = fed;
        }
    }
}

/// A script's `do click [name, n]`: the mouse moved to the n-th (0 first) shown widget whose path ends in `name`, and
/// its left button pressed and let go there (none: no such widget shown yet).
fn click_widget(screen: &MenuScreen, frame: u32, name: &str, n: Option<u64>) -> Vec<MenuInput> {
    let tape = screen.view.tape_frame(frame);
    let suffix = format!("/{name}");
    let found = tape
        .roots
        .iter()
        .flat_map(|r| r.widgets.iter())
        .filter(|w| w.vis != 1 && w.vis != 2 && (w.path == name || w.path.ends_with(&suffix)))
        .nth(n.unwrap_or(0) as usize);
    let Some(w) = found else { return Vec::new() };
    let [x, y] = widget_centre(w);
    eprintln!("mvs: script: frame {frame}: do click {} at {x:.0},{y:.0}", w.path);
    vec![
        MenuInput::MouseMove { x, y },
        MenuInput::MouseButton { button: 0, down: true },
        MenuInput::MouseButton { button: 0, down: false },
    ]
}

/// A script's `do queueState`, logged; any screen runs it.
pub(crate) fn queue_state(hub: Option<&crate::online::hub::OnlineHub>, frame: u32) {
    let q = hub.and_then(|h| h.queue.as_ref());
    eprintln!(
        "mvs: script: frame {frame}: do queueState -> queue {} searching {} found {} started match {}",
        q.is_some(),
        q.is_some_and(|q| q.searching),
        q.is_some_and(|q| q.found.is_some()),
        hub.is_some_and(|h| h.started.is_some()),
    );
}

/// The online client's frame before the menus': the compatibility id and
/// the server tables once the store and the menu data are in, the settings read, the view set, a name the host core
/// took saved, each match record counted in the stats file.
fn online_before(shell: &mut Shell, view: &mut MenuView) {
    let Some(h) = shell.online.clone() else { return };
    let mut h = h.borrow_mut();
    if h.compat().is_empty()
        && let Some(store) = shell.store()
    {
        let compat = crate::online::glue::compat_id(store.manifest());
        crate::self_check::begin(&mut h, store.dir(), &compat);
        h.set_compat(compat);
    }
    crate::self_check::poll(&mut h);
    crate::online::link::poll(&mut h, &shell.data);
    // (the gate from the update feed, the store's import and the self check; the backends off while it is set)
    crate::online::gate::poll(&mut h, shell.store().map(|s| s.manifest()));
    if let Some(net) = &shell.net {
        crate::online::glue::apply_gate(net, h.gate());
        // (the launcher's change of the online settings, while the game runs)
        crate::online::glue::follow_settings(net);
    }
    if h.tables.maps.is_empty()
        && let Some(ui) = &shell.ui
    {
        crate::online::glue::bind_tables(&mut h, &ui.menu);
    }
    let now = h.clock();
    // (the session's view, its events the frame's: replaced each frame)
    view.session.online = h.frame(crate::online::glue::prefs_of(&shell.settings), now);
    if let Some(name) = h.saved_name.take() {
        shell.settings.profile.name = name;
    }
    if let Some(net) = &shell.net {
        crate::online::glue::keep_known_relays(net, &mut shell.settings);
    }
    let me = h.me();
    for r in std::mem::take(&mut h.records) {
        view.session.stats.add(&r, &me);
    }
    // profile.ask: the stats file's profile under this player's id, name and icon
    for to in std::mem::take(&mut h.profiles.asks) {
        let s = view.session.stats.profile(&me);
        let p = mvs_protocol::profile::ProfileWire {
            id: me.clone(),
            name: shell.settings.profile.name.clone(),
            icon: mvs_ui::menu::own_profile::my_profile_icon_asset(&shell.settings.hydra),
            totals: s.totals,
            trackers: s.trackers,
            matches: s.matches,
        };
        h.tell_profile(&to, Some(p));
    }
}

/// The screens' online commands run, with this game's loadout for a character; the queue's pre-match screen opened
/// when one asked for it.
fn online_after(
    hub: &std::rc::Rc<std::cell::RefCell<crate::online::hub::OnlineHub>>,
    settings: &mvs_settings::Settings,
    ui: &UiShared,
    view: &mut MenuView,
    go: &mut Go,
) {
    let mut h = hub.borrow_mut();
    // (a join link's and a dev run's --online-open wait for the sign-ins, a relay trying again included, up to
    // PENDING_WAIT_MS: EOS's takes a second or more; and while online play is held for a check, an update or an
    // import: the gate)
    let now = h.clock();
    let held = h.pending_held(now);
    hand_link_join(&mut h, held, &mut view.session);
    let mut cmds = if held { Vec::new() } else { std::mem::take(&mut h.pending) };
    cmds.extend(view.take_online_cmds());
    {
        let bundle = &ui.menu;
        let stats = std::cell::RefCell::new(&mut view.session.stats);
        let loadout = |c: &str| {
            let mut s = stats.borrow_mut();
            let trackers = s.data().trackers.clone();
            let hard = s.arcade().hard_completions;
            let cat = bundle.menu.as_ref().map(|m| &m.cosmetics);
            crate::online::glue::local_net_loadout(settings, bundle, c, &mut |e| {
                cat.map_or(0.0, |cat| mvs_ui::menu::stat_trackers::stat_tracker_count(e, cat, &trackers, hard))
            })
        };
        for c in cmds {
            h.run(c, &loadout, now);
        }
    }
    if let Some(modes) = h.open_queue.take()
        && matches!(go, Go::Stay)
    {
        *go = Go::PreMatch(Box::new(queue_pre_match(&h, settings, modes, None)));
    }
}

/// A join link's code (or `--online-open join:`'s) to the main menu once the pending commands no longer wait
/// (`held`, this frame's `pending_held`): the main menu that is up, or the next one, opens the lobby screen on it
/// (`MainMenuState::link_join`), the join box's way.
pub fn hand_link_join(h: &mut crate::online::hub::OnlineHub, held: bool, session: &mut mvs_ui::menu::session::Session) {
    if !held && let Some(code) = h.link_join.take() {
        session.main_menu.link_join = Some(code);
    }
}

/// The pre-match screen of the online pre-match the hub holds (a queue
/// set's rematch, else an online lobby's start). A pre-match that is not a queue set's rematch empties the set's games
/// (UI_PreMatch @13647-13730: bIsRematch false clears EOGDataCache). None: no pre-match is pending.
pub fn online_pre_match(
    h: &crate::online::hub::OnlineHub,
    name: &str,
    session: &mut mvs_ui::menu::session::Session,
) -> Option<PreMatchStart> {
    use mvs_protocol::online::PmKind;
    let b = h.pending_pre_match()?;
    let rematch = b.kind == PmKind::Queue && b.rematch.is_some();
    if !rematch {
        session.eog.cache.clear();
    }
    // (a queue set's rematch: DetermineScore over the set's games)
    let set_score = (rematch && !session.eog.cache.is_empty()).then(|| {
        let (y, t) = session.eog.set_scores();
        (y.unsigned_abs(), t.unsigned_abs())
    });
    let leader = b.kind == PmKind::Lobby
        && mvs_ui::menu::lobby::with_carry(|c| c.kept_net.as_ref().is_some_and(|n| n.am_i_leader()));
    let setup = crate::online::pre_match::screen_setup(b, name, leader, set_score);
    Some(PreMatchStart { setup, lobby: None })
}

/// The queue's pre-match screen: this player alone, on the queue's modes; `resume`
/// the return from the Lab's (the search's start and the lock).
pub fn queue_pre_match(
    h: &crate::online::hub::OnlineHub,
    settings: &mvs_settings::Settings,
    modes: Vec<mvs_protocol::online::QueueMode>,
    resume: Option<mvs_ui::menu::online::QueueResume>,
) -> PreMatchStart {
    let party = h.view_party_role();
    let me = mvs_ui::menu::screens::pre_match::setup::PreMatchPlayer {
        account_id: {
            let m = h.me();
            if m.is_empty() { "local".to_owned() } else { m }
        },
        username: settings.profile.name.clone(),
        ..Default::default()
    };
    let team_style = modes.first().map_or(0, |m| mvs_protocol::queue_rules::queue_team_style(*m));
    let net =
        mvs_ui::menu::online::PreMatchNet { queue: Some(mvs_ui::menu::online::QueueSetup { modes, party, resume }) };
    let setup = mvs_ui::menu::screens::pre_match::setup::PreMatchSetup {
        players: vec![me],
        team_style,
        net: Some(net),
        ..Default::default()
    };
    PreMatchStart { setup, lobby: None }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::input::DeviceKind;
    use mvs_ui::menu::lobby::{LobbyMatch, LocalHuman};
    use mvs_ui::menu::local_play_match::Device;

    #[test]
    fn local_play_humans_keep_the_tables_devices() {
        // a table that swapped player 0 onto pad 1 and gave the keyboard to player 1
        let humans = vec![
            LocalHuman { slot: 0, device: Device::Pad(1), set: 0 },
            LocalHuman { slot: 1, device: Device::Kbm, set: 1 },
            LocalHuman { slot: 2, device: Device::KbmPad0, set: 2 },
        ];
        let mut session = mvs_ui::menu::session::Session::new(mvs_settings::stats::StatsFile::default());
        let m = LobbyMatch { humans: Some(humans), ..LobbyMatch::default() };
        let start = MenuHost::start_of(&mut session, m, &Default::default(), &[]);
        assert_eq!(
            start.humans,
            Some(vec![(0, DeviceKind::Pad(1), 0), (1, DeviceKind::Kbm, 1), (2, DeviceKind::KbmPad0, 2)])
        );
        assert!(matches!(start.kind, MatchKind::LocalPlay));
    }

    #[test]
    fn a_waves_set_up_starts_a_waves_run_on_the_menus_seed() {
        use mvs_sim::setup::{MatchMode, WavesVariant};
        let mut session = mvs_ui::menu::session::Session::new(mvs_settings::stats::StatsFile::default());
        session.waves.seed = Some(1234);
        let mut m = LobbyMatch::default();
        let variant = WavesVariant::Endless;
        m.setup.mode = Some(MatchMode::Waves { variant, cast: vec!["shaggy".into()] });
        let start = MenuHost::start_of(&mut session, m, &Default::default(), &[]);
        assert!(matches!(start.kind, MatchKind::Waves { variant: WavesVariant::Endless }));
        assert_eq!(start.seed, 1234);
        assert_eq!(session.waves.seed, None);
    }

    #[test]
    fn a_lobby_matchs_bots_take_their_fighters_display_name() {
        use mvs_sim::setup::MatchPlayer;
        use mvs_ui::bundle::{FighterInfo, RosterEntry};
        let mut fighters = crate::lab_setup::Fighters::default();
        fighters.insert("irongiant".into(), FighterInfo { display_name: "Iron Giant".into(), ..Default::default() });
        let roster = [RosterEntry { fighter: Some("arya".into()), display_name: "Arya".into(), ..Default::default() }];
        let p = |c: &str, bot: bool| MatchPlayer { character: Some(c.into()), bot, ..MatchPlayer::default() };
        let mut m = LobbyMatch::default();
        m.setup.players = vec![p("shaggy", false), p("irongiant", true), p("arya", true), p("taz", true)];
        m.usernames = ["Me", "Irongiant", "Arya", "Taz"].map(String::from).to_vec();
        let mut session = mvs_ui::menu::session::Session::new(mvs_settings::stats::StatsFile::default());
        let start = MenuHost::start_of(&mut session, m, &fighters, &roster);
        assert_eq!(start.usernames, ["Me", "Iron Giant", "Arya", "Taz"]);
    }
}
