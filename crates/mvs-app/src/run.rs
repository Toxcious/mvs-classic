//! The match loop: the fixed 60 Hz steps from the frame's
//! time, per step the devices read, the World stepped, the views placed, the effects, the HUD and the camera ticked;
//! per frame the views and camera at the step's alpha, the renderer's picture, the HUD and the UI on the back
//! buffer, presented. It runs on the shell's window, renderer, devices, sound and UI (app.rs).

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;

use mvs_audio::tape::post::Line;
use mvs_data::GameData;
use mvs_render::fx::Fx;
use mvs_render::fx::surfaces::level_surfaces;
use mvs_render::scenery::{SceneryMatch, level_emitters};
use mvs_render::warm::WarmLists;
use mvs_sim::ai::bt::{BtBot, LAB_BOTS};
use mvs_sim::ai::expert::{EXPERT_LEVEL, Workers};
use mvs_sim::events::EndPhase;
use mvs_sim::input::NO_INPUT;
use mvs_sim::setup::{MatchPlayer, Rules, TeamStyle};
use mvs_sim::world::flow::podium_data;
use mvs_sim::{Event, InputState, MatchSetup, World};
use mvs_sim_trace::Book;
use mvs_trace::record::InputRecord;
use mvs_trace::{Trace, TraceWriter};
use mvs_ui::element::{BoxEl, DrawEffects, Element, ElementList, PaintGeometry};
use mvs_ui::hud::away::AwayOverlay;
use mvs_ui::hud::world_widget::WorldWidgets;
use mvs_ui::hud::{Anchor, Hud, HudArcade, HudFrame, HudPlayer, HudSetup, LABEL_ACTIONS, LabelInput, Slots};
use mvs_ui::results::{EndOfGame, EogPlayer};
use mvs_ui::text::Fonts;
use mvs_view::{ActorPose, Camera, FighterPose, FrameInput, FrameTime, HudPoint, MatchCamera, ViewPoser};

use crate::app::{Frame, MatchEnd, MatchKind, MatchStart, Shell, UiShared};
use crate::audio::scopes::Index;
use crate::audio::{BeginOpts, MatchAudio, TapeWriter, play};
use crate::input::{DeviceKind, Devices};
use crate::lab_host::{LabHost, Taken};
use crate::lab_setup::{LabCarry, LabRoll};
use crate::lab_tools::LabTools;
use crate::match_chat::{BoxKey, MatchChat, Overlay as ChatOverlay, Submit};
use crate::match_lab::LabEffect;
use crate::pause::{PauseHost, has_pause_menu};
use crate::prepare::{Pending, PrepKey, PrepareJob, Prepared};
use crate::rumble::MatchRumble;
use crate::script::Gate;
use crate::tutorial::{MatchParts, TutorialHost};
use crate::waves::{EndBeat, EndStep};
use crate::{Error, Result};
use mvs_ui::bindings::Bindings;
use mvs_ui::menu::MenuInput;
use mvs_ui::menu::training_config::{LAB_EXPERT, LabConfig};
use unicode_segmentation::UnicodeSegmentation;

/// The sim's step, s.
pub const STEP: f64 = 1.0 / 60.0;
/// A frame's real time at most, s (a long stall is not caught up).
const MAX_DT: f64 = 0.25;
/// PreTravel's fade to black, s (opacity over 0.3 s linear, the menus' type 0 transition).
const FADE: f64 = 0.3;
/// The UI layer of the fade and of the perf line: over every HUD widget.
const TOP_LAYER: i32 = 1 << 20;
/// A match runs on an open store.
const NO_STORE: &str = "a match runs on an open store";

/// How a match runs beside what it is (the command line's capture and timing options).
#[derive(Debug)]
pub struct MatchOptions {
    /// `--shot`: the ticks whose frame is written as a PNG to `out`; the clock held at one step a frame.
    pub shots: Vec<u32>,
    /// Where the shots and the results' shot are written.
    pub out: PathBuf,
    /// `--trace-out`: the file a replay's own trace (the Rust World's record of the match) is written to; none
    /// without it.
    pub trace_out: Option<PathBuf>,
    /// `--real-pace`: a replay stepped by real time (the 60 Hz accumulator, presented synced to the display) instead of
    /// one step a frame, for frame-cost runs of a known match; its `--shot` pictures are of the first frame drawn at
    /// or past each tick, on that clock (the HUD and views between two ticks, as a played match draws them).
    pub real_pace: bool,
    /// `--throttle`: the frame's CPU time stretched this many times by a spin before the present (1: none), as a
    /// CPU throttle slows its main thread.
    pub throttle: f64,
    /// `--shot-results`: the results screen's frame this many seconds after it opens written as
    /// `<out>/results.png`, then the match left.
    pub results_shot: Option<f64>,
}

impl Default for MatchOptions {
    fn default() -> MatchOptions {
        MatchOptions {
            shots: Vec::new(),
            out: PathBuf::from("."),
            trace_out: None,
            real_pace: false,
            throttle: 1.0,
            results_shot: None,
        }
    }
}

/// The match's set-up when no trace gives one: 1v1, Shaggy against Shaggy, two ring-outs or 60 s, its flow on (the
/// trace match m4-flow's).
// Deviation: no menus pick the match; the second player has no bot (no AI is ported), so it stands.
pub fn default_setup() -> MatchSetup {
    let p = || MatchPlayer { character: Some("shaggy".to_owned()), ..MatchPlayer::default() };
    MatchSetup {
        team_style: TeamStyle::OneVsOne,
        players: vec![p(), p()],
        rules: Some(Rules { knockouts_to_win: 2.0, duration: 60.0, stocks: false }),
        flow: true,
        ..MatchSetup::default()
    }
}

/// The match's own parts drawn by the UI crate (the shared ones are the shell's).
struct MatchUi {
    hud: Hud,
    hud_fonts: Fonts,
    /// The scenery's world-space widgets (the Court's scoreboard), painted with the HUD's fonts.
    widgets: WorldWidgets,
}

/// What `MatchLoop::load_world` gives: the World, the replay when the match is one, the seed, the parts a loader
/// thread built.
struct Loaded {
    world: World,
    replay: Option<Replay>,
    seed: i32,
    ready: Option<(ViewPoser, Box<crate::prepare::Parts>)>,
}

/// A trace's inputs driving the loop and the frames it writes.
struct Replay {
    trace: Trace,
    book: Book,
    next: usize,
    /// `--trace-out`: the file the match's own frames are written to at the leave, and its writer.
    out: Option<(PathBuf, TraceWriter)>,
}

/// Where a match's ticks come from.
enum Ticks {
    /// The devices and the bots, one World step a 60 Hz tick.
    Local,
    /// A trace's inputs driving every player.
    Replay(Box<Replay>),
    /// An online session: a frame steps the World as its rollback needs (back to the confirmed line, several ticks
    /// again, the predicted ones) and the views present once.
    Online(Box<crate::online::Online>),
}

/// Where the match is between its load and its end.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Phase {
    Playing,
    Over,
}

/// What the Lab's frame leaves for the loop.
enum LabStep {
    Stay,
    Leave,
    Restart(Box<LabCarry>),
}

/// A Lab restart's match loading on the loader (`crate::prepare`).
struct LabNext {
    config: LabConfig,
    rolled: LabRoll,
    map_load: bool,
    pending: Pending,
    /// The control asked for the restart: the match ends into it once the load is ready.
    asked: bool,
}

/// A match from its load to its results.
pub struct MatchLoop {
    world: World,
    /// The stage key the renderer draws (the World's own: its scenery reads the World while they agree).
    stage_key: String,
    poser: ViewPoser,
    cam: MatchCamera,
    fx: Fx,
    rumble: MatchRumble,
    ui: Option<Box<MatchUi>>,
    ticks: Ticks,
    /// What the match is (its end's way back).
    kind: MatchKind,
    usernames: Vec<String>,
    real_pace: bool,
    /// A scripted run plays the match: one tick a frame, as the script's stepped clock (script.rs).
    scripted: bool,
    throttle: f64,
    results_shot: Option<f64>,
    shots: Vec<u32>,
    out: PathBuf,
    /// The local player.
    me: usize,
    /// Each player's "Attached UI v3 Bottom" / "Top" RelativeLocation.Z.
    ui_z: Vec<(f64, f64)>,
    acc: f64,
    hud_time: f64,
    /// The match's real clock, s (from the first drawn frame).
    real: f64,
    started: bool,
    load_start: Instant,
    /// The load's steps (`mvs: load`).
    pub load_laps: crate::perf::LoadLaps,
    fade_start: Option<f64>,
    phase: Phase,
    /// Travel came with the UI up: the match ends in the results screen.
    to_results: bool,
    inputs: Vec<InputState>,
    /// Each player's bot controller (None: a human's slot); empty in a replay, whose trace drives every player.
    bots: Vec<Option<BtBot>>,
    /// The EBotBehavior each Lab bot's controller was made of (0 none): a slot's new behaviour in the Lab's config
    /// makes it a new controller at the next Spawn All Players (`lab_reset`).
    lab_bot_behaviors: Vec<u8>,
    fighters: Vec<FighterPose>,
    actors: Vec<ActorPose>,
    /// The frame's ticks' events.
    events: Vec<Event>,
    /// The match's glue to the shell's sound engine (None with `--no-audio`).
    match_audio: Option<MatchAudio>,
    /// The glue's calls not yet handed to the sound engine.
    calls: Vec<Line>,
    /// The pause menu over a match that has one (Custom, Local Play).
    pause: Option<PauseHost>,
    /// The Lab's menu and control (a Lab match).
    lab: Option<LabHost>,
    /// A spawn the control ended that changes the roster or the map: the match ends with it.
    lab_effect: Option<LabEffect>,
    /// TrainGameMode Destroy All Players, run at the next tick (its events are the tick's).
    abduct_pending: bool,
    /// The Lab's next match loading on the loader: ahead of its restart, or asked for and waited on.
    lab_next: Option<LabNext>,
    /// The level goes on into a Lab restart: its ambience stays.
    keep_level: bool,
    /// The pause menu's elements of the frame, drawn over the match's UI with the menus' fonts.
    menu_list: ElementList,
    /// (the port's own) The match's text chat: an online match's from its go, a local online lobby match's from its
    /// first frame; None in every other match.
    chat: Option<MatchChat>,
    /// Enter was held at the last frame (a key repeat opens no box).
    chat_enter: bool,
    /// The tick an input script's `win` ends the match at (120 ticks stepped, then the harness's
    /// podium: HandleWinLoss for team 0, the match run on to its end).
    win_at: Option<i32>,
    /// A tutorial match's widgets over it and its program side; None in any other match.
    tutorial: Option<TutorialHost>,
    /// (the port's own) A Waves run's end beat; None in any other match and in a replay.
    waves_end: Option<EndBeat>,
}

impl std::fmt::Debug for MatchLoop {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MatchLoop").field("tick", &self.world.flow.tick).field("phase", &self.phase).finish()
    }
}

/// What a match's HUD set-up reads besides its World, taken from the shell (a loader thread's job takes a copy).
#[derive(Clone, Debug)]
pub(crate) struct HudInputs {
    pub lab: bool,
    pub arcade: Option<HudArcade>,
    pub ui: mvs_settings::UiSettings,
}

/// The HUD inputs of a match of `kind` on the shell.
pub(crate) fn hud_inputs(sh: &Shell, kind: &MatchKind) -> HudInputs {
    // An arcade round's stage (AXFunctions GetArcadeInfo: the round played; GetNumArcadeLevels).
    let arcade = matches!(kind, MatchKind::Arcade)
        .then(|| sh.session.arcade.info())
        .flatten()
        .map(|r| HudArcade { round: r.round, levels: mvs_ui::menu::arcade::num_arcade_levels(r.difficulty) });
    HudInputs { lab: matches!(kind, MatchKind::Lab { .. }), arcade, ui: sh.settings.ui.clone() }
}

/// A HUD a loader thread made, with the set-up it was made for (`MatchLoop::ui_parts` takes it when the match's is
/// the same).
pub struct HudMade {
    pub bundle: mvs_ui::bundle::Bundle,
    pub setup: HudSetup,
    pub hud: Hud,
    /// The bundle's pictures and UI materials the loader warmed for the match's fighters (None: not warmed there).
    pub warmed: Option<mvs_ui::element::HudWarmed>,
}

/// The match's HUD set-up.
pub(crate) fn hud_setup(inputs: &HudInputs, world: &World, me: usize, online: bool, usernames: &[String]) -> HudSetup {
    let setup = &world.setup;
    let arcade = inputs.arcade;
    if crate::waves::is_waves(world) {
        return crate::waves::hud_setup(world, me, usernames, &inputs.ui);
    }
    // A mode's match has no fighters at the start: its HUD players are the set-up's, the
    // pawns bound to their slots later.
    if setup.mode.is_some() {
        let players = setup
            .players
            .iter()
            .enumerate()
            .map(|(i, p)| HudPlayer {
                team: p.team.unwrap_or(0),
                bot: p.bot,
                username: usernames.get(i).cloned().unwrap_or_default(),
                character: p.character.clone(),
            })
            .collect();
        return HudSetup {
            mode: 5,
            me,
            players,
            #[allow(clippy::cast_possible_truncation)]
            max_score: setup.rules.map_or(-1, |r| r.knockouts_to_win as i32),
            stocks: setup.rules.is_some_and(|r| r.stocks),
            local_play: setup.local_play,
            online,
            arcade,
            ui: inputs.ui.clone(),
        };
    }
    let players = world
        .players
        .iter()
        .enumerate()
        .map(|(i, &f)| {
            let fi = world.fighter(f);
            let p = setup.players.get(i);
            HudPlayer {
                team: u8::try_from(fi.team.team).unwrap_or(0),
                bot: p.is_some_and(|p| p.bot),
                // (a dev match's: '', no accounts give them; the Lab's own player: '')
                username: if inputs.lab && i == me {
                    String::new()
                } else {
                    usernames.get(i).cloned().unwrap_or_default()
                },
                character: Some(world.names.str(fi.ch).to_owned()),
            }
        })
        .collect();
    HudSetup {
        // UI context mode: 1 1v1, 2 2v2, 3 free for all, 4 the Lab (no versus card, no score), 5 a
        // tutorial (TutorialGameMode SetUIContextMode(5) @61 / @5356).
        mode: if inputs.lab {
            4
        } else if matches!(setup.mode, Some(mvs_sim::setup::MatchMode::Tutorial { .. })) {
            5
        } else {
            setup.team_style as u8 + 1
        },
        me,
        players,
        #[allow(clippy::cast_possible_truncation)]
        max_score: if inputs.lab { -1 } else { setup.rules.map_or(-1, |r| r.knockouts_to_win as i32) },
        stocks: setup.rules.is_some_and(|r| r.stocks),
        local_play: setup.local_play,
        online,
        arcade,
        ui: inputs.ui.clone(),
    }
}

/// The UI preferences the match's views read at its start: the team skins, the outline gate, and the
/// colour bindings of the HUD palette and ColorMe).
pub(crate) fn view_prefs(u: &mvs_settings::UiSettings) -> mvs_view::skins::ViewPrefs {
    mvs_view::skins::ViewPrefs {
        team_skins: u.world_use_team_colored_skins,
        outlines: mvs_view::skins::OutlinePrefs {
            ally: u.world_show_ally_outlines,
            enemy: u.world_show_enemy_outlines,
            me: u.world_show_my_outline,
        },
        colors: mvs_ui::menu::prefs::color_rows(u).map(str::to_owned),
    }
}

/// An Expert bot's decisions put on the program's Expert workers: made at the first Expert bot, sized to the PC
/// (`Workers::for_this_pc`; none on one core, where they run inline) and kept for the program's life. The bot gives
/// the same inputs either way; the frame's thread only waits where a decision is due and its worker is not done.
fn with_workers(bot: &mut BtBot) {
    static WORKERS: std::sync::OnceLock<Option<Arc<Workers>>> = std::sync::OnceLock::new();
    if bot.expert.is_some() {
        bot.set_workers(WORKERS.get_or_init(|| Workers::for_this_pc().map(Arc::new)).as_ref());
    }
}

/// The keyboard's held keys and presses let go (the chat box's keys reach no match input): each
/// keyboard device reset as at a match's start.
// Deviation: a device of the keyboard and the first pad (kbm+pad0) loses its pad's presses of the frame as well.
fn release_keyboard(devices: &mut Devices) {
    let slots: Vec<usize> = devices.slots().collect();
    for i in slots {
        if let Some(d) = devices.device(i)
            && matches!(d.kind, DeviceKind::Kbm | DeviceKind::KbmPad0)
        {
            d.reset();
        }
    }
}

/// "Attached UI v3 Bottom" / "Top"_GEN_VARIABLE's RelativeLocation.Z: the pawn's own component, else BaseFighter's.
fn widget_z(data: &GameData, key: &str, which: &str) -> f64 {
    let name = format!("Attached UI v3 {which}_GEN_VARIABLE");
    let Some(c) = data.chars.get(key) else { return 0.0 };
    let comps = &c.fighter.components;
    let z = |m: &mvs_data::fighter::Component| m.p.as_obj()?.get("RelativeLocation")?.as_obj()?.get("Z")?.as_f64();
    comps.pawn.get(&name).and_then(z).or_else(|| comps.base.get(&name).and_then(z)).unwrap_or(0.0)
}

/// The top widget's relative Z while a Polymorph is on fighter `f`, `None`
/// with none. Buff_PolymorphParent BeginPlay step 7 (PP@4156-4454) BPI_SetUIRelativeLocationTop(UITopOffset /
/// Mesh.RelativeScale3D / CapsuleComponent.RelativeScale3D) on the victim; EndPlay puts the saved one back (PP@460).
/// UITopOffset.Z over the pawn mesh's origin in world units, as the relative Z `anchor` takes on the
/// victim's own view (its mesh Z and scale, the capsule's Z scale on the mesh offset).
fn polymorph_top(w: &World, f: mvs_sim::fighter::FighterId, p: &HudPoint) -> Option<f64> {
    let fi = w.fighter(f);
    let poly = fi.shapes.polymorph.as_ref()?;
    let d = mvs_sim::fighter::common::buff_def(w, mvs_sim::actors::ActorRef::Fighter(f), poly.cls);
    let off = d.and_then(|d| d.get("UITopOffset")?.as_obj()?.get("Z")?.as_f64()).unwrap_or(0.0);
    let zs = f64::from(fi.body.actor_scale.map_or(fi.body.size_scale, |a| a[1]));
    Some((p.mesh_z * (zs - 1.0) + off) / p.mesh_scale)
}

/// What the input labels of player index `p` read: the binding set of its
/// device (the keyboard's set 0) and its LastInteractionType; a player index with no device the set of its own index
/// and type 0.
fn label_input(devices: &Devices, bindings: &Bindings, p: usize, me: usize, local_play: bool) -> LabelInput {
    let d = devices.label_device(p, me, local_play);
    let set = d.map_or(p, |d| if d.kind == DeviceKind::Kbm { 0 } else { d.set });
    LabelInput {
        kind: d.map_or(0, |d| d.last_interaction),
        slots: LABEL_ACTIONS.map(|a| Slots::break_apart(bindings.action_mapping(a, set))),
    }
}

/// The match's EndOfGameData (Init New EOG UI's, from AddPlayerPodiumData per player); `peers`
/// each slot's online id (match.go's, for the podium's toasts; '' past its end).
fn end_of_game(w: &World, me: usize, usernames: &[String], peers: &[String]) -> EndOfGame {
    let players = podium_data(w);
    let info = (0..players.len())
        .map(|i| {
            let p = w.setup.players.get(i);
            EogPlayer {
                character: w.players.get(i).map_or_else(String::new, |&f| w.names.str(w.fighter(f).ch).to_owned()),
                skin: p.and_then(|p| p.skin.clone()).unwrap_or_default(),
                banner: p.and_then(|p| p.banner.clone()),
                id: peers.get(i).cloned().unwrap_or_default(),
                bot: p.is_some_and(|p| p.bot),
                local_name: None,
            }
        })
        .collect();
    EndOfGame {
        team_style: w.setup.team_style as u8,
        local_play: w.setup.local_play,
        stocks: w.setup.rules.is_some_and(|r| r.stocks),
        me,
        // (a command-line match has no names: your own record takes the dev harness's name)
        names: (0..players.len())
            .map(|i| match usernames.get(i) {
                Some(n) => n.clone(),
                None if i == me => mvs_ui::menu::custom_text::LOCAL_PLAYER_NAME.to_owned(),
                None => String::new(),
            })
            .collect(),
        info,
        arcade: None,
        players,
    }
}

/// A HUD anchor: the fighter's point with the widget component's Z (mesh scale applied) and the
/// playerIndex depth offset, projected and rounded to whole pixels (SWorldWidgetScreenLayer's Tick 142617ff0
/// RoundToInt); its distance to the view origin in cm as a float32 (the slot's ZOrder is its negative).
pub fn anchor(cam: &Camera, size: [u32; 2], p: &HudPoint, z: f64) -> Anchor {
    let ue = [p.d, -p.x, p.y + p.mesh_z + z * p.mesh_scale];
    let rel = [ue[0] - cam.origin[0], ue[1] - cam.origin[1], ue[2] - cam.origin[2]];
    let dist = (rel[0] * rel[0] + rel[1] * rel[1] + rel[2] * rel[2]).sqrt() as f32;
    let row = |v: [f64; 4], m: &mvs_view::M4| {
        let mut o = [0.0; 4];
        for (j, oj) in o.iter_mut().enumerate() {
            *oj = v[0] * m[0][j] + v[1] * m[1][j] + v[2] * m[2][j] + v[3] * m[3][j];
        }
        o
    };
    let view = row([rel[0], rel[1], rel[2], 1.0], &cam.translated_world_to_view);
    let clip = row(view, &cam.view_to_clip);
    let (nx, ny) = (clip[0] / clip[3], clip[1] / clip[3]);
    let [w, h] = size.map(f64::from);
    Anchor { x: (((nx + 1.0) / 2.0) * w + 0.5).floor() as i32, y: (((1.0 - ny) / 2.0) * h + 0.5).floor() as i32, dist }
}

/// The stage a MapData opens: its first level's stage by the menu bundle's table.
/// `default` where the menu data has no such map or the build has no stage of its level (it warns once and
/// opens its default stage), or no menu is loaded (`--no-ui`).
pub(crate) fn stage_of_map(sh: &Shell, map: &str, default: String) -> String {
    let Some(ui) = &sh.ui else { return default };
    let Some(data) = ui.menu.menu.as_ref().and_then(|m| m.maps.get(map)) else {
        eprintln!("mvs: map {map}: not in the menu data, the default stage opens");
        return default;
    };
    let level = data.levels.first().map_or("", String::as_str);
    match ui.menu.stage_of_level(level) {
        Some(k) => k.to_owned(),
        None => {
            eprintln!("mvs: map {map}: level {level} is no stage of the build, the default stage opens");
            default
        }
    }
}
/// The match's World (and its replay or the parts a loader thread prepared): its own function, so the debug
/// build's frame of `new` does not hold a copy of each of these large values.
#[allow(clippy::too_many_arguments)]
fn load_world(
    sh: &Shell,
    kind: &MatchKind,
    o: &MatchOptions,
    stage: &mut String,
    map: Option<&str>,
    setup: &MatchSetup,
    seed: i32,
    me: usize,
    playing_online: bool,
    prefs: &mvs_view::skins::ViewPrefs,
    level_kept: bool,
    prepared: Option<Box<Prepared>>,
    laps: &mut crate::perf::LoadLaps,
) -> Result<Loaded> {
    let store = sh.store().ok_or_else(|| Error::Match(NO_STORE.into()))?;
    match kind {
        MatchKind::Replay { name } => {
            let path = mvs_sim_trace::run::default_traces().join(format!("{name}.trace"));
            let mut trace = Trace::read(&path).map_err(|e| Error::Match(format!("{}: {e}", path.display())))?;
            mvs_sim_trace::drawn_with_cast(&mut trace.header.setup);
            // (the trace's own stage: world_for loads the header's, so the picture draws it too)
            *stage = trace.header.stage.clone().unwrap_or_else(|| mvs_sim_trace::TRACE_STAGE.to_owned());
            let (w, book) = mvs_sim_trace::world_for(&trace.header, store).map_err(Error::Match)?;
            #[allow(clippy::cast_possible_truncation)]
            let seed = trace.header.world_seed as i32;
            Ok(Loaded {
                world: w,
                replay: Some(Replay {
                    trace,
                    book,
                    next: 0,
                    out: o.trace_out.clone().map(|p| (p, TraceWriter::new())),
                }),
                seed,
                ready: None,
            })
        }
        _ => {
            if let Some(m) = map {
                *stage = stage_of_map(sh, m, std::mem::take(stage));
            }
            // The parts a loader thread built for this match (`crate::prepare`), when they are this start's.
            let key = PrepKey {
                setup: setup.clone(),
                seed,
                stage: stage.clone(),
                me,
                online: playing_online,
                prefs: prefs.clone(),
                level_kept,
            };
            match prepared {
                Some(p) if p.key == key => {
                    let Prepared { world, poser, parts, laps: steps, .. } = *p;
                    let steps: Vec<String> = steps.iter().map(|(n, ms)| format!("{n} {ms:.0}")).collect();
                    laps.notes.push(format!("off the frame: {}", steps.join(", ")));
                    Ok(Loaded { world, replay: None, seed, ready: Some((poser, Box::new(parts))) })
                }
                other => {
                    if other.is_some() {
                        eprintln!("mvs: the match prepared off the frame was for another start: loaded here");
                    }
                    let keys = mvs_sim_trace::keys_of(setup);
                    let refs: Vec<&str> = keys.iter().map(String::as_str).collect();
                    let data = GameData::load(store, &refs, stage)?;
                    laps.lap("data");
                    let w = World::new(Arc::new(data), seed, setup)
                        .map_err(|e| Error::Match(format!("the bind: {e:?}")))?;
                    Ok(Loaded { world: w, replay: None, seed, ready: None })
                }
            }
        }
    }
}

impl MatchLoop {
    /// The match loaded on the shell: its World, the stage on the renderer, the views, the camera, the effects, the
    /// match's sound, the humans' devices, the HUD.
    pub fn new(sh: &mut Shell, start: MatchStart, o: &MatchOptions) -> Result<MatchLoop> {
        let load_start = Instant::now();
        let mut laps = crate::perf::LoadLaps::new();
        let MatchStart { setup, mut kind, seed, mut stage, map, me, usernames, humans, prepared, lab_ahead } = start;
        // (a match prepared off the frame began its load at its job: `PrepareJob::of`)
        if prepared.is_none() {
            sh.begin_load();
        }
        let size = sh.swap.size;
        let kept_level = sh.kept_level.take();
        // (a Lab fighter change keeps the level: its scenery and the particle player with the level's systems go on)
        let kept =
            kept_level.filter(|_| sh.scenery_open && matches!(&kind, MatchKind::Lab { carry: Some(c) } if !c.map_load));
        let level_kept = kept.is_some();
        // (PlayingOnline: a relay match)
        let playing_online = matches!(kind, MatchKind::Online { .. });
        let prefs = view_prefs(&sh.settings.ui);
        let store = sh.store().ok_or_else(|| Error::Match(NO_STORE.into()))?;
        let Loaded { mut world, replay, seed, ready } = load_world(
            sh,
            &kind,
            o,
            &mut stage,
            map.as_deref(),
            &setup,
            seed,
            me,
            playing_online,
            &prefs,
            level_kept,
            prepared,
            &mut laps,
        )?;
        laps.lap("world");
        let (poser, mut parts) = match ready {
            Some((poser, parts)) => (poser, Some(parts)),
            None => (ViewPoser::new_local(store, &world, me, playing_online, &prefs)?, None),
        };
        laps.lap("poser");
        let mut cam = MatchCamera::new(&world, f64::from(size[0]) / f64::from(size[1]));
        // (a prepared match's effects are made, their level begun: `PrepareJob::run`)
        let made_fx = parts.as_mut().and_then(|p| p.fx.take());
        let fx_made = made_fx.is_some();
        #[allow(clippy::cast_sign_loss)]
        let mut fx = match (kept, made_fx) {
            (Some(k), _) => Fx::resume_level(store, seed as u32, k)?,
            (None, Some(f)) => f,
            (None, None) => Fx::new(store, seed as u32)?,
        };
        if !fx_made {
            fx.set_local(me, playing_online);
            fx.team_skins = sh.settings.ui.world_use_team_colored_skins;
        }
        laps.lap("fx");
        // The level's Emitter actors on the particle player, before the first step.
        if !level_kept && !fx_made {
            fx.begin_level(&level_emitters(store, &stage)?, level_surfaces(store, &stage)?)?;
            laps.lap("level");
        }
        // The Lab's menu: a restart's takes over the one before it, with its camera and music (the new map's begin anew).
        // (LoadInputBindings: the Controls screen's saved bindings, else the defaults)
        let bindings = Bindings::load(world.data.input_defaults(), sh.settings.input.bindings.as_ref());
        let mut online = match &mut kind {
            MatchKind::Online { net } => net.take(),
            _ => None,
        };
        if let Some(o) = &mut online {
            o.set_stage_key(&world.data.stage_key);
        }
        let carry = match &mut kind {
            MatchKind::Lab { carry } => carry.take().map(|b| *b),
            _ => None,
        };
        // The match the level's actors read: this machine's player, GetAllPlayers'
        // order and the seed of what the authority draws for every peer, set for every match, a kept level's too
        // Unverified: GetAllPlayers (PlayerDataManager, native) is taken in the players' slot order (the Lab's config
        // slots, a lobby's players).
        // Deviation: the seed the level's blocks draw from is the World's, where an offline page draws one at random.
        let matched = {
            let n = world.players.len();
            let slot_of = carry.as_ref().map_or_else(|| (0..n).collect(), |c| crate::lab_setup::lab_slots(&c.config));
            let mut players: Vec<usize> = (0..n).collect();
            players.sort_by_key(|&i| slot_of.get(i).copied().unwrap_or(usize::MAX));
            #[allow(clippy::cast_sign_loss)]
            SceneryMatch { me, players, seed: seed as u32 }
        };
        let (bots, lab_bot_behaviors) = if replay.is_some() {
            (Vec::new(), Vec::new())
        } else {
            Self::bots_of(&mut world, carry.as_ref(), matches!(kind, MatchKind::Lab { .. }))
        };
        let (lab, taken) = if matches!(kind, MatchKind::Lab { .. }) && replay.is_none() && sh.ui.is_some() {
            let name = usernames.first().cloned().unwrap_or_else(|| "Player".to_owned());
            let same = world.flow.same_attack_penalty;
            let (h, t) = LabHost::new(sh, &world.setup, &name, carry, lab_ahead, bindings.clone(), same)?;
            // A restart's new World starts with the penalty on and no instant cooldowns: the kept config's flags
            // are put back (the config the switches show).
            crate::match_lab::config_to_world(&mut world, h.config());
            (Some(h), t)
        } else {
            (None, Taken::default())
        };
        laps.lap("lab");
        if let Some(c) = taken.camera {
            cam = c;
            cam.rig.resize(f64::from(size[0]) / f64::from(size[1]));
        }
        let hud_bundle = parts.as_mut().and_then(|p| p.hud.take());
        let ui_parts = match &sh.ui {
            Some(s) => Some(Self::ui_parts(sh, s, &world, me, playing_online, &usernames, &kind, hud_bundle)?),
            None => None,
        };
        laps.lap("hud");
        let renderer = sh.renderer.as_mut().ok_or_else(|| Error::Match(NO_STORE.into()))?;
        if !level_kept {
            // (a scenery the last match left open, one that left no finish, ends before the stage changes)
            if std::mem::take(&mut sh.scenery_open) {
                renderer.scenery_end(None);
            }
            match parts.as_mut().and_then(|p| p.stage.take()) {
                Some(p) => renderer.set_stage_prepared(p)?,
                None => renderer.set_stage(&stage)?,
            }
            // The scenery's begin, after the level's emitters spawn:
            // the match, the camera's centre before its first tick.
            renderer.scenery_begin(Some(&mut fx.player), Some(&matched), Some(cam.rig.center));
            sh.scenery_open = true;
        } else {
            renderer.scenery_set_match(Some(&matched));
        }
        // The warm-up (the characters' mesh and actor warm-up, one empty draw): what the match may draw made
        // before its first frame, so no first spawn makes it in play.
        laps.lap("stage");
        match parts.as_mut().and_then(|p| p.warm.take()) {
            Some((lists, made)) => {
                // (a level that goes on: the lists of the player it resumed, which the job's could not read)
                let lists = if level_kept { WarmLists::of(&world, &poser, &mut fx) } else { lists };
                laps.notes.push(format!("warmed off the frame in {:.0} ms", made.ms));
                renderer.install_warm(&lists, made)?;
            }
            None => {
                renderer.prewarm_match(&world, &poser, &mut fx)?;
            }
        }
        laps.lap("warm");
        // The match's sound: its scopes, the stage's ambience, the music, the scenery's sink (after the
        // scenery's begin).
        let mut calls = Vec::new();
        // (locally controlled: in a net match only this machine's player is)
        let net_me = if playing_online {
            Some(i32::try_from(me).map_err(|_| Error::Match("player index".into()))?)
        } else {
            None
        };
        // (the glue a loader thread began, when the scenery's events and the kept music it assumed hold)
        let scenery_events = renderer.scenery_sound_events();
        let begun = parts
            .as_mut()
            .and_then(|p| p.audio.take())
            .filter(|(m, _)| m.scenery_events() == scenery_events.as_slice() && taken.music.is_some() == level_kept);
        let match_audio = match (&sh.audio, begun) {
            (Some(audio), Some((mut m, begin))) => {
                if let Some(k) = &taken.music {
                    m.resume_music(k);
                }
                calls.extend(begin);
                m.attach(renderer, Some(audio.rtpc_mirror()));
                Some(m)
            }
            (Some(audio), None) => {
                // (the InitBank's index and the announcer pack a prepared match read)
                let known = parts.as_mut().and_then(|p| p.known.take().map(|k| (k, p.announcer.take())));
                let (known, announcer) = match known {
                    Some(k) => k,
                    None => {
                        // (the game state's ReceiveBeginPlay: this machine's announcer pack)
                        let pack = &sh.settings.hydra.announcer_pack.path;
                        let announcer = match mvs_data::stored_announcer_pack(renderer.assets().store(), pack) {
                            Ok(p) => Some(p),
                            Err(e) => {
                                eprintln!("mvs: the announcer pack {pack}: {e}");
                                None
                            }
                        };
                        (Index::new(audio.init().events.keys().cloned()), announcer)
                    }
                };
                let opts = BeginOpts { kept: taken.music.clone(), net_me, me, announcer, ..BeginOpts::default() };
                let rtpc = Some(audio.rtpc_mirror());
                Some(MatchAudio::begin_known(&world, renderer, known, rtpc, opts, &mut calls)?)
            }
            (None, _) => None,
        };
        // The first input (the first input, after the audio's resume): the music clock starts (the begin's)
        // and PandaGameInstance StartMusic, which a match opened without the title screen has not had yet.
        if match_audio.is_some() && taken.music.is_none() {
            let tick = u32::try_from(world.flow.tick).unwrap_or(0);
            calls.extend(sh.music.start_calls().into_iter().map(|call| Line { tick, call }));
        }
        laps.lap("audio");
        // (the scopes a loader thread prepared: the begin's scope calls install them)
        if let (Some(a), Some(p)) = (&mut sh.audio, parts.as_mut()) {
            a.offer_scopes(std::mem::take(&mut p.scopes), std::mem::take(&mut p.music));
        }
        play(&mut sh.audio, &mut calls);
        laps.lap("scopes");
        if let Some(a) = &mut sh.audio {
            laps.notes.extend(a.scope_stats.drain(..).map(|s| s.line()));
            let left = a.take_offers();
            if !left.is_empty() {
                laps.notes.push(format!("prepared scopes no call took: {}", left.join(", ")));
            }
        }
        let mut ui_parts = ui_parts;
        // (the scenery's world-space widgets: their targets and glyphs, painted once)
        if let (Some(u), Some(s)) = (&mut ui_parts, sh.ui.as_mut()) {
            u.widgets.warm(renderer, &mut s.ui, &mut u.hud_fonts)?;
        }
        // A replay's trace drives every player: no human has a device.
        let seated_pairs = humans.as_ref().map(|h| h.iter().map(|&(slot, _, set)| (slot, set)).collect::<Vec<_>>());
        let seated = humans.unwrap_or_else(|| vec![(me, DeviceKind::KbmPad0, 0)]);
        sh.devices.seat(bindings.clone(), if replay.is_some() { &[] } else { &seated });
        if let Some(d) = sh.devices.device(me) {
            d.reset();
        }
        // The HUD's input labels read each player index's device and bindings; a
        // replay's labels stay the keyboard form with the default keys, as the recorded tapes have them.
        if let (Some(u), Some(s), None) = (&mut ui_parts, &sh.ui, &replay) {
            let lp = world.setup.local_play;
            let players = (0..world.players.len()).map(|p| label_input(&sh.devices, &bindings, p, me, lp)).collect();
            u.hud.set_label_input(&s.menu, players);
        }
        // (a tutorial's widgets, made before the pause menu's and under it)
        let tutorial = if matches!(kind, MatchKind::Tutorial) && replay.is_none() && sh.ui.is_some() {
            Some(TutorialHost::new(sh, bindings.clone())?)
        } else {
            None
        };
        let mut pause = if has_pause_menu(&kind) && replay.is_none() && sh.ui.is_some() {
            Some(PauseHost::new(sh, &world.setup, me, seated_pairs.as_deref(), bindings.clone())?)
        } else {
            None
        };
        if let Some(p) = &mut pause {
            p.pause.tutorial = matches!(kind, MatchKind::Tutorial);
        }
        let waves_end = matches!(kind, MatchKind::Waves { .. }) && replay.is_none();
        let rumble = MatchRumble::new(&sh.devices);
        let ui_z = world
            .players
            .iter()
            .map(|&f| {
                let key = world.names.str(world.fighter(f).ch);
                (widget_z(&world.data, key, "Bottom"), widget_z(&world.data, key, "Top"))
            })
            .collect();
        let shots = {
            let mut s = o.shots.clone();
            s.sort_unstable();
            s
        };
        if !shots.is_empty() || replay.is_some() {
            std::fs::create_dir_all(&o.out).map_err(|e| Error::Match(format!("{}: {e}", o.out.display())))?;
        }
        laps.lap("rest");
        laps.notes.push(sh.trim_unused());
        eprintln!("mvs: load {}", laps.line());
        Ok(MatchLoop {
            stage_key: mvs_render::stage::stage_key(&stage),
            world,
            poser,
            cam,
            fx,
            rumble,
            ui: ui_parts,
            ticks: match (replay, online) {
                (Some(r), _) => Ticks::Replay(Box::new(r)),
                (None, Some(o)) => Ticks::Online(o),
                (None, None) => Ticks::Local,
            },
            kind,
            usernames,
            real_pace: o.real_pace,
            scripted: sh.script.is_some(),
            throttle: o.throttle.max(1.0),
            results_shot: o.results_shot,
            shots,
            out: o.out.clone(),
            me,
            ui_z,
            acc: 0.0,
            hud_time: 0.0,
            real: 0.0,
            started: false,
            load_start,
            load_laps: laps,
            fade_start: None,
            phase: Phase::Playing,
            to_results: false,
            inputs: Vec::new(),
            bots,
            lab_bot_behaviors,
            fighters: Vec::new(),
            actors: Vec::new(),
            events: Vec::new(),
            match_audio,
            calls,
            pause,
            lab,
            lab_effect: None,
            abduct_pending: false,
            lab_next: None,
            keep_level: false,
            menu_list: ElementList::default(),
            chat: None,
            chat_enter: false,
            win_at: None,
            tutorial,
            waves_end: waves_end.then(EndBeat::default),
        })
    }

    /// The skill shot indicators shown (the chars' `skillshots`): the actors' places in the
    /// drawn frame between two ticks, by actor id. `seated`: the slots with a local seated device.
    /// AimPunch and the Superman marker: a seated owner's. Rick's portal aimer: the
    /// locally controlled ones, a seated owner's only when the match is online.
    // Deviation: Rick's portal aimer is placed at its tick's position, not interpolated from the step before (the
    // render's `px`).
    fn skillshots(world: &World, seated: &[bool], online: bool, alpha: f32) -> Vec<(i32, [f64; 3])> {
        use mvs_sim::actors::world_actor::WorldActorState;
        let a = f64::from(alpha);
        let human = |f: mvs_sim::fighter::FighterId| {
            let slot = usize::try_from(world.fighter(f).id).unwrap_or(usize::MAX);
            seated.get(slot).copied().unwrap_or(false)
        };
        let mut out = Vec::new();
        for (_, ap) in world.actors.aim_punches.iter() {
            if !ap.dead && human(ap.owner) {
                out.push((
                    ap.seq,
                    [
                        0.0,
                        -(f64::from(ap.prev_x) + f64::from(ap.x - ap.prev_x) * a),
                        f64::from(ap.prev_y) + f64::from(ap.y - ap.prev_y) * a,
                    ],
                ));
            }
        }
        for (_, act) in world.actors.actors.iter() {
            if act.dead {
                continue;
            }
            match &act.state {
                WorldActorState::AimMarker(m) if human(act.owner) => {
                    out.push((
                        act.seq,
                        [
                            0.0,
                            -(f64::from(m.prev_x) + f64::from(m.x - m.prev_x) * a),
                            f64::from(m.prev_y) + f64::from(m.y - m.prev_y) * a,
                        ],
                    ));
                }
                WorldActorState::PortalAimer(m) if m.locally_controlled && (!online || human(act.owner)) => {
                    out.push((act.seq, [0.0, f64::from(-m.x), f64::from(m.y)]));
                }
                _ => {}
            }
        }
        out
    }

    /// The match's player bots: a Lab slot's controller is its bot entry of the Lab's menu (`lab_bot_of` the config's
    /// difficulty, recovery before the menu has set one); a match bot is `matchBot(12345 + slot, level)`. The trees
    /// go on the World, which a minion's controller reads too. With them, the EBotBehavior each Lab bot was made of
    /// (0 a human's slot or a match bot). A mode's match gets the trees with no player bot: its pawns' controllers
    /// (a tutorial's bags, a Waves run's enemies) are the sim's and read them there.
    fn bots_of(world: &mut World, carry: Option<&LabCarry>, lab: bool) -> (Vec<Option<BtBot>>, Vec<u8>) {
        let n = world.setup.players.len();
        // (a Waves run's bot in the player's seat is the sim's own, as its enemies are)
        if !world.setup.players.iter().any(|p| p.bot) || crate::waves::is_waves(world) {
            if world.setup.mode.is_some() {
                world.bot_trees = Some(Arc::new(world.data.bot_trees.clone()));
            }
            return (vec![None; n], vec![0; n]);
        }
        let mut behaviors = vec![0; n];
        let trees = Arc::new(world.data.bot_trees.clone());
        world.bot_trees = Some(trees.clone());
        let lab_slots = carry.map(|c| crate::lab_setup::lab_slots(&c.config));
        let bots = world
            .setup
            .players
            .iter()
            .enumerate()
            .map(|(i, p)| {
                if !p.bot {
                    return None;
                }
                let mut bot = match (&lab_slots, carry) {
                    (Some(slots), Some(c)) => {
                        let b = c.config.players[slots[i]].bot_difficulty.max(1);
                        behaviors[i] = b;
                        Self::lab_bot_of(&trees, b)
                    }
                    _ if lab => {
                        behaviors[i] = 1;
                        Self::lab_bot_of(&trees, 1)
                    }
                    _ => {
                        #[allow(clippy::cast_possible_truncation)]
                        let level = p.difficulty.unwrap_or(0.0) as i32;
                        BtBot::match_bot(12345 + i32::try_from(i).expect("a few players"), &trees, level)
                    }
                };
                bot.idle = carry.is_some_and(|c| c.bot_idle);
                with_workers(&mut bot);
                Some(bot)
            })
            .collect();
        (bots, behaviors)
    }

    /// The Lab's bot of an EBotBehavior n (1 and up): LAB_BOTS[n - 1], seed 12345; LAB_EXPERT, the port's own, the
    /// Expert bot (the Hard entry with the Expert layer, `matchBot` at the Expert level).
    fn lab_bot_of(trees: &Arc<mvs_data::BotTrees>, behavior: u8) -> BtBot {
        if behavior == LAB_EXPERT {
            return BtBot::match_bot(12345, trees, EXPERT_LEVEL);
        }
        BtBot::lab_bot(12345, trees, &LAB_BOTS[usize::from(behavior.saturating_sub(1))])
    }

    #[allow(clippy::too_many_arguments)] // the HUD's inputs, with the HUD a loader made from them
    fn ui_parts(
        sh: &Shell,
        s: &UiShared,
        world: &World,
        me: usize,
        online: bool,
        usernames: &[String],
        kind: &MatchKind,
        made: Option<HudMade>,
    ) -> Result<Box<MatchUi>> {
        let store = sh.store().ok_or_else(|| Error::Match(NO_STORE.into()))?;
        let hud_setup = hud_setup(&hud_inputs(sh, kind), world, me, online, usernames);
        // (the loader's warm read the same bundle and the same World's fighters: it holds for a HUD made again)
        let warmed = made.as_ref().and_then(|m| m.warmed);
        let (hud_bundle, hud) = match made {
            Some(m) if m.setup == hud_setup => (m.bundle, m.hud),
            other => {
                let bundle = match other {
                    Some(m) => m.bundle,
                    None => mvs_ui::bundle::Bundle::load(store, "hud")?,
                };
                let hud = Hud::new(&bundle, &s.text, world, &hud_setup)?;
                (bundle, hud)
            }
        };
        let hud_fonts = Fonts::new(store, &hud_bundle)?;
        // (the HUD's pictures and UI materials made with the load, so its first frames draw them)
        if let Some(w) = warmed {
            eprintln!("mvs: hud warmed off the frame: {} textures, {} materials", w.textures, w.materials);
        } else if let Some(r) = &sh.renderer {
            let chars = crate::waves::hud_chars(world);
            let (t, w) = (Instant::now(), mvs_ui::element::warm_hud(r.assets(), &hud_bundle, &chars)?);
            eprintln!(
                "mvs: hud warm {} textures, {} materials, {:.0} ms",
                w.textures,
                w.materials,
                ms(t, Instant::now())
            );
        }
        let widgets = WorldWidgets::new(&hud_bundle);
        Ok(Box::new(MatchUi { hud, hud_fonts, widgets }))
    }

    /// The clock is held at one step a frame (`--shot`, `--replay`, each without `--real-pace`).
    fn fixed(&self) -> bool {
        self.scripted || (self.replaying() && !self.real_pace) || (!self.shots.is_empty() && !self.real_pace)
    }

    /// A trace drives the match.
    fn replaying(&self) -> bool {
        matches!(self.ticks, Ticks::Replay(_))
    }

    /// The Lab Tools of a Lab match.
    fn lab_tools(&self) -> Option<&LabTools> {
        self.lab.as_ref().and_then(|l| l.menu.tools.as_ref())
    }

    /// The match left early (the window closed, a timed run's end).
    pub fn end(&mut self, sh: &mut Shell) -> Result<()> {
        self.finish(sh)
    }

    /// One 60 Hz tick: [`MatchLoop::step`], then [`MatchLoop::present_tick`].
    fn tick(&mut self, sh: &mut Shell) -> Result<()> {
        let t0 = Instant::now();
        let Some(recorded) = self.step(sh)? else { return Ok(()) };
        let t1 = Instant::now();
        sh.perf.section("sim:step", ms(t0, t1));
        if let (Ticks::Replay(r), Some(rec)) = (&mut self.ticks, recorded)
            && let Some((_, out)) = &mut r.out
        {
            let mut ours = mvs_sim_trace::frame_of(&self.world, &r.book, &self.inputs, &self.world.events);
            // (the inputs are the trace's own: written as recorded)
            ours.frame.inputs = rec;
            out.push(&ours.frame);
        }
        self.present_tick(sh)?;
        sh.perf.section("sim:show", ms(t1, Instant::now()));
        Ok(())
    }

    /// The step part of a tick: the inputs read (the devices, or the trace's), the bots' thinking, the
    /// World stepped, the Lab's tick. The step's events are left in the World for the present. `None` when a replay's
    /// trace is over (the match with it); else a replay's recorded inputs of the tick.
    fn step(&mut self, sh: &mut Shell) -> Result<Option<Option<Vec<InputRecord>>>> {
        let n = self.world.players.len();
        self.inputs.clear();
        self.inputs.resize(n, NO_INPUT);
        let mut recorded = None;
        match &mut self.ticks {
            Ticks::Replay(r) => {
                if r.next >= r.trace.len() {
                    self.phase = Phase::Over;
                    return Ok(None);
                }
                let fr = r.trace.frame(r.next).map_err(|e| Error::Match(e.to_string()))?;
                self.inputs.clear();
                self.inputs.extend(fr.inputs.iter().map(|i| mvs_sim_trace::input_of(i, &self.world, &r.book)));
                // (a tutorial trace's page calls ahead of the frame's step: IntroComplete, Advance, the end screen's)
                mvs_sim_trace::page_calls(&mut self.world, &r.trace.header, r.next);
                r.next += 1;
                recorded = Some(fr.inputs);
            }
            Ticks::Local => {
                sh.devices.read_prefs(&sh.settings.input);
                sh.devices.poll();
                // (the Lab Tools' panel has the pads while it is open, and until their buttons are up after a close)
                if self.lab.as_ref().and_then(|l| l.menu.tools.as_ref()).is_some_and(LabTools::pad_claimed) {
                    for slot in 0..n {
                        if let Some(d) = sh.devices.device(slot) {
                            d.pad = None;
                        }
                    }
                }
                for slot in 0..n {
                    if self.bots.get(slot).is_some_and(Option::is_some) {
                        continue;
                    }
                    if let Some(d) = sh.devices.device(slot) {
                        self.inputs[slot] = d.read();
                    }
                }
                // A mode's match has no fighters at the start: its one input is the local player's, which a 'player'
                // pawn controller reads. Game input off (a tutorial widget holding the UI's): what the
                // device took is dropped.
                // Deviation: the gate is the input itself (NO_INPUT while off); the build's
                // BPI_DisableGameInput sets the PlayerController's input mode to UI only.
                if self.world.setup.mode.is_some() {
                    let on = self.tutorial.as_ref().is_none_or(TutorialHost::game_input);
                    let read = sh.devices.device(self.me).map_or(NO_INPUT, |d| d.read());
                    self.inputs.clear();
                    self.inputs.push(if on { read } else { NO_INPUT });
                }
            }
            Ticks::Online(_) => unreachable!("an online match ticks through online_ticks"),
        }
        self.world.events.clear();
        // (the events the tutorial's calls made since the last tick go with this tick's, ahead of them: the World keeps
        // them for its next splice)
        if let Some(t) = &mut self.tutorial {
            self.world.events.extend(t.take_carried());
        }
        if std::mem::take(&mut self.abduct_pending) {
            mvs_sim::world::flow::abduct_all(&mut self.world);
        }
        // (the Lab Tools' writes, between ticks)
        let tools_on = sh.settings.lab.tools_on;
        if let Some(t) = self.lab.as_ref().and_then(|l| l.menu.tools.as_ref()) {
            t.apply_pins(&mut self.world, tools_on);
        }
        // (a bot's slot is the controller's; it thinks before the step)
        for (slot, bot) in self.bots.iter_mut().enumerate() {
            if let Some(bot) = bot {
                let me = self.world.players[slot];
                self.inputs[slot] = bot.think(&mut self.world, me);
            }
        }
        self.world.step(&self.inputs);
        if let Some(t) = self.lab.as_ref().and_then(|l| l.menu.tools.as_ref()) {
            t.apply_pins(&mut self.world, tools_on);
        }
        if let Some(e) = self.lab.as_mut().and_then(|l| l.tick(sh)) {
            match e {
                LabEffect::Reset { starting_damage } => self.lab_reset(sh, &starting_damage),
                e => self.lab_effect = Some(e),
            }
        }
        Ok(Some(recorded))
    }

    /// The present part of a tick, on the World as stepped and the events in `world.events`:
    /// the views placed, the effects stepped (their ring-out anchors to the camera), the HUD's observe, the camera's
    /// step, the sound; then the events go to the app's part and the frame's list (the World's left empty). An online
    /// frame that stepped several ticks presents once, with its session's events there.
    fn present_tick(&mut self, sh: &mut Shell) -> Result<()> {
        self.poser.place(&self.world);
        // (the skins' own intros' sounds by the views' clocks, read once a tick whether the sound runs or not)
        let mut intro_sounds = Vec::new();
        self.poser.intro_sounds(&mut intro_sounds);
        // (the effects ride the poses at the latest tick; a ring-out's anchor is the camera's before its step; the
        // camera as the last frame placed it)
        self.poser.set_camera(self.cam.rig.root, self.cam.rig.pov.up);
        self.poser.present(&self.world, 1.0, &mut self.fighters, &mut self.actors);
        // (the scenery's tick before the particles': what a block spawns first ticks with them)
        let renderer = sh.renderer.as_mut().expect(NO_STORE);
        // (the World goes to the scenery when its stage is the one drawn: the match's stage is loaded from
        // the World's own key, so they agree unless a stage change is added that leaves a World running)
        let shown = self.world.data.stage_key == self.stage_key;
        renderer.scenery_tick(STEP, Some(&mut self.fx.player), shown.then_some(&self.world));
        if shown {
            renderer.scenery_platforms(&self.world);
        }
        // (each DI_Visual reads the World's showDI)
        self.fx.show_di = sh.settings.ui.world_show_di;
        self.fx.camera = Some(mvs_render::fx::CameraRoot::of(&self.cam.rig));
        self.fx.step(&self.world, &self.fighters, &self.actors)?;
        for a in self.fx.ko_anchors.drain(..) {
            self.cam.ko_anchor(a.y, a.z, a.hold);
        }
        self.hud_time += STEP;
        self.label_types(&sh.devices);
        // (the scenery's tick, after the HUD's observe, pops at the HUD's time + STEP: MatchHost.pop)
        let pops = renderer.scenery_pops();
        if let Some(u) = &mut self.ui {
            // (the Lab's combo texts, behind the Lab menu's toggles)
            u.hud.set_lab_combos(self.lab.as_ref().map(LabHost::combos));
            // (the port's own) a Waves run's counter and clock (a replay's too)
            u.hud.set_waves(mvs_sim::modes::waves::status(&self.world).map(|s| crate::waves::hud_of(&s)));
            u.hud.observe(&self.world, &self.world.events, self.hud_time, self.real);
            for p in pops {
                u.hud.scenery_pop(p, self.hud_time + STEP);
            }
        }
        self.cam.step(&self.world);
        if let Some(m) = &mut self.match_audio {
            m.tick_with(&self.world, &self.world.events, &intro_sounds, &mut self.fx, &mut self.calls);
            play(&mut sh.audio, &mut self.calls);
        }
        self.fx.ko_picks.clear();
        let events = std::mem::take(&mut self.world.events);
        for e in &events {
            self.event(e, &mut sh.devices);
        }
        self.tutorial_tick(sh, &events);
        self.events.extend(events);
        Ok(())
    }

    /// A tutorial's part of a tick's events: the step machine's calls to its
    /// widgets in order, the finish's save, then what follows the run's state (the PlayerController's CanPause and
    /// game input, the RTPC tutorial_controlled_pawn).
    fn tutorial_tick(&mut self, sh: &mut Shell, events: &[Event]) {
        let Some(t) = &mut self.tutorial else { return };
        // (the tick's pops at the HUD time before this tick's step: the events are handled before
        // the HUD time advances)
        let mut m = MatchParts {
            world: &mut self.world,
            hud: self.ui.as_mut().map(|u| &mut u.hud),
            hud_time: self.hud_time - STEP,
            paused: false,
        };
        t.tick_events(sh, &mut m, events);
        if let Some((pawn, value)) = t.after_tick(&self.world, self.me)
            && let Some(a) = &mut self.match_audio
        {
            a.tutorial_rtpc(self.world.flow.tick, pawn, value, &mut self.calls);
            play(&mut sh.audio, &mut self.calls);
        }
        if t.take_reset_inputs() {
            sh.devices.drop_queued();
        }
    }

    /// The tutorial widgets' frame (their screen under the pause menu's): the calls their buttons make, the
    /// step machine's calls those raise, the end screen's way back to the main menu (UI_TutorialEndScreen
    /// BackToMainMenu: the pause screen's transition, its AdvTutShortcut first).
    fn tutorial_frame(&mut self, sh: &mut Shell) {
        let paused = self.pause.as_ref().is_some_and(|p| p.pause.paused);
        let Some(t) = &mut self.tutorial else { return };
        // (the port's own) a scripted run's keys of the match's frames reach the widgets while they are up and the
        // chat, which takes them first, is not open (no chat in an offline match): a script presses the cards' buttons
        if t.screen.active()
            && let Some(s) = sh.script.as_mut()
        {
            t.input.append(&mut s.match_input);
        }
        let mut m = MatchParts {
            world: &mut self.world,
            hud: self.ui.as_mut().map(|u| &mut u.hud),
            hud_time: self.hud_time,
            paused,
        };
        let out = t.frame(sh, &mut m, self.real);
        if out.reset_inputs {
            sh.devices.drop_queued();
        }
        if let Some(adv) = out.exit {
            if adv {
                sh.session.adv_tut_shortcut = true;
            }
            match &mut self.pause {
                Some(p) => p.to_main_menu(sh),
                None => self.phase = Phase::Over,
            }
        }
    }

    /// (the port's own) A Waves run's end: a beat once the sim says the run is over, the fade at its last moment,
    /// then the run's record saved and the match over, to the main menu (`over`: no results screen). Run after
    /// each tick. A run left from the pause menu before the sim settled it saves nothing.
    fn waves_frame(&mut self, sh: &mut Shell) {
        let Some(e) = &mut self.waves_end else { return };
        #[allow(clippy::cast_possible_truncation)] // (0.3 s: 18 ticks)
        let fade = (FADE / STEP).round() as i32;
        match e.frame(mvs_sim::modes::waves::status(&self.world), self.world.flow.tick, fade) {
            EndStep::None => {}
            EndStep::Fade => self.fade_start = Some(self.real),
            EndStep::Finish(s) => {
                crate::waves::finish(&mut sh.session, &self.world, &s);
                self.phase = Phase::Over;
            }
        }
    }

    /// Each player index's LastInteractionType given to the HUD's input labels (they read the device's at each
    /// refresh).
    fn label_types(&mut self, devices: &Devices) {
        let Some(u) = &mut self.ui else { return };
        let lp = self.world.setup.local_play;
        for p in 0..self.world.players.len() {
            let kind = devices.label_device(p, self.me, lp).map_or(0, |d| d.last_interaction);
            u.hud.set_input_type(p, kind);
        }
    }

    /// The app's part of a step's events: the end's phases and the rumble.
    fn event(&mut self, e: &Event, devices: &mut Devices) {
        match *e {
            // GameEnd: UI_Game's MatchEnded (the victory card) comes through the HUD's observe.
            Event::MatchEnd { phase: EndPhase::PreTravel, .. } => self.fade_start = Some(self.real),
            // Unverified: what the build shows after a match with no lobby is not read; this one opens the results as
            // a custom match's.
            Event::MatchEnd { phase: EndPhase::Travel, .. } => {
                self.phase = Phase::Over;
                self.to_results = self.ui.is_some() && !self.replaying();
                if let Ticks::Online(o) = &mut self.ticks {
                    o.ended();
                }
                // (the match's fade ends with it: the results screen fades in by its own fader)
                self.fade_start = None;
            }
            Event::Rumble { .. } | Event::RumbleStop { .. } if !self.replaying() => {
                self.rumble.event(e, &self.world.data, &self.world.names, devices);
            }
            _ => {}
        }
    }

    /// A scripted run's device input fed since the last take, to the match's devices.
    fn script_devices(sh: &mut Shell) {
        if let Some(s) = sh.script.as_mut() {
            let input = std::mem::take(&mut s.device_input);
            sh.devices.script_input(input);
        }
    }

    /// One frame: the steps due by the frame's real time, then the draw and the present; the match's end once it
    /// and its results are over.
    pub fn frame(&mut self, sh: &mut Shell, f: &Frame) -> Result<Option<MatchEnd>> {
        // BPI_EnableGameInput (PFGPlayerController 137366): game-only input and bShowMouseCursor false. The OS cursor
        // is not shown over the window for as long as the match runs; the pause menu, the Lab's
        // menu, the Move List and the Lab Tools' panel draw the game's own cursor while they are open.
        sh.window.set_cursor_hidden(true);
        // (a scripted run's keys, buttons and pad reach the match's devices as the window's would: before the chat's
        // box, which takes the keyboard while it is open, and taken every frame, so none is left for the next match)
        Self::script_devices(sh);
        // (the port's own) the match chat's box first: while it is open its keys are its own, none reaches the match,
        // the pause menu or Escape's end
        self.chat_frame(sh);
        self.script_win(sh);
        // Escape: the pause menu's key where the match has one. A match with none (online, a server's, a replay) takes
        // no Escape, as in the build: UI_Game makes no UI_PauseMenu there and Can Pause is false (UG @3679, @500).
        // (the port's own) a run with no menus at all (no UI loaded) ends on Escape: it has no other way out.
        if self.pause.is_none() && self.lab.is_none() && sh.ui.is_none() && sh.window.pressed("Escape") {
            if matches!(self.ticks, Ticks::Online(_)) {
                // (an online match's end tells the other player)
                self.finish(sh)?;
            }
            return Ok(Some(MatchEnd::Quit));
        }
        if self.pause.is_some() && self.pause_frame(sh, f) {
            return self.over(sh);
        }
        if self.lab.is_some() {
            match self.lab_frame(sh)? {
                LabStep::Stay => {}
                LabStep::Leave => return self.over(sh),
                LabStep::Restart(carry) => return Ok(Some(MatchEnd::LabRestart(carry))),
            }
        }
        // (the Lab's own scripted frames: their input, fed in lab_frame)
        Self::script_devices(sh);
        // (a key pressed in the match is not the menus' press)
        sh.window.take_menu_keys();
        if sh.window.take_blur() {
            // every pad's effects stopped and its motors off when the window loses the keyboard
            self.rumble.stop_all(&mut sh.devices);
        }
        if let Some(size) = f.resized {
            self.cam.rig.resize(f64::from(size[0]) / f64::from(size[1]));
        }
        let dt = if self.fixed() { STEP } else { f.dt.clamp(0.0, MAX_DT) };
        if self.started {
            self.real += dt;
        }
        self.events.clear();
        let paused =
            self.pause.as_ref().is_some_and(|p| p.pause.paused) || self.lab.as_ref().is_some_and(|l| l.control.paused);
        if self.phase == Phase::Playing && self.started && !paused && matches!(self.ticks, Ticks::Online(_)) {
            self.online_ticks(sh, dt)?;
        } else if self.phase == Phase::Playing && self.started && !paused {
            // (the Lab Tools' clock, slower or faster)
            let scale = self.lab_tools().map_or(1.0, |t| t.time_scale(sh.settings.lab.tools_on));
            let one_a_frame = self.fixed() && scale == 1.0;
            if one_a_frame {
                // (exactly one step a frame)
                self.acc = STEP;
            } else {
                self.acc += dt * scale;
            }
            while self.acc >= STEP && self.phase == Phase::Playing && self.lab_effect.is_none() {
                self.tick(sh)?;
                self.acc -= STEP;
                // (each tick's: a catch-up frame of many ticks skips no part of the beat)
                self.waves_frame(sh);
            }
            if one_a_frame {
                self.acc = 0.0;
            }
        }
        if let Ticks::Online(o) = &mut self.ticks
            && let Some(why) = o.note()
        {
            eprintln!("mvs: online: {why}");
            self.phase = Phase::Over;
        }
        self.rumble.frame(dt, &mut sh.devices);
        if self.phase == Phase::Over {
            return self.over(sh);
        }
        self.tutorial_frame(sh);
        self.draw(sh, dt)?;
        let shot = self.shots.first().is_some_and(|&t| i64::from(self.world.flow.tick) >= i64::from(t));
        if shot {
            self.shot(sh)?;
        }
        sh.script_frame(self.started)?;
        if self.throttle > 1.0 {
            // The frame's CPU work so far taken (throttle - 1) more times, spun on this thread.
            let cpu = f.cpu_start.elapsed().mul_f64(self.throttle - 1.0);
            let spin = Instant::now();
            while spin.elapsed() < cpu {
                std::hint::spin_loop();
            }
        }
        // Presented synced to the display (no frame cap, no setting); a held clock (shots, replay) does not wait.
        sh.swap.present(!self.fixed())?;
        if !self.started && self.online_gate()? {
            // The countdown card's clock starts at the first drawn frame.
            self.started = true;
            if let Some(l) = &mut self.lab {
                // (the port's own) a map load's match: the music back in under the cover
                if l.map_load_active {
                    MatchAudio::map_cover_end(self.world.flow.tick, sh.settings.audio.music, &mut self.calls);
                    play(&mut sh.audio, &mut self.calls);
                }
                l.reveal(sh);
            }
            eprintln!("mvs: start to first frame {:.0} ms", self.load_start.elapsed().as_secs_f64() * 1e3);
            self.chat = self.chat_of(sh);
            if let Some(u) = &mut self.ui {
                u.hud.start_card(self.real);
            }
        }
        // A replay that writes a trace or a sound tape runs to its end; one taken for pictures ends at its last.
        let writes = TapeWriter::open_now() || matches!(&self.ticks, Ticks::Replay(r) if r.out.is_some());
        if shot && self.shots.is_empty() && !(self.replaying() && writes) {
            return self.over(sh);
        }
        Ok(None)
    }

    /// The match's chat at its first frame: an online match's peers from its go, a local
    /// online lobby match's this player alone ('me' at its slot, the bots ''); None in any other match.
    fn chat_of(&self, sh: &Shell) -> Option<MatchChat> {
        let setup = &self.world.setup;
        let teams: Vec<u32> =
            (0..setup.players.len()).map(|i| u32::from(mvs_sim::setup::player_team(setup, i))).collect();
        let now = self.real * 1e3;
        match &self.ticks {
            Ticks::Online(o) => {
                let (me, mut peers, names) = o.chat_table()?;
                // (match.go's peers end at the last human's slot: the slots past it are bots, '' as the table has them)
                peers.resize(peers.len().max(teams.len()), String::new());
                Some(MatchChat::new(me, peers, names, teams, now))
            }
            _ if sh.online.as_ref().is_some_and(|h| h.borrow().local.is_some()) => {
                let peers = (0..setup.players.len())
                    .map(|i| if i == self.me { "me".to_owned() } else { String::new() })
                    .collect();
                Some(MatchChat::new(self.me, peers, self.usernames.clone(), teams, now))
            }
            _ => None,
        }
    }

    /// The chat's frame: the others' messages in, the box closed by the pause menu or the setting off, Enter
    /// / Shift+Enter (not a repeat) opening it with the held keys let go; while it is open the window's typed
    /// characters are its text and its Escape, Tab and Enter its keys, and nothing of the keyboard reaches the match.
    fn chat_frame(&mut self, sh: &mut Shell) {
        let Some(chat) = &mut self.chat else { return };
        let enabled = sh.settings.ui.chat_in_match;
        let paused = self.pause.as_ref().is_some_and(|p| p.pause.paused);
        let now = self.real * 1e3;
        // Deviation: the pause menu and the setting are read every frame (the chat polls the pause every 250 ms and
        // hears the setting's change).
        chat.settings_changed(enabled, paused);
        if let Ticks::Online(o) = &mut self.ticks {
            for (from, to, text) in o.take_chats() {
                chat.received(&from, to, &text, enabled, now);
            }
        }
        // (a scripted run's keys and characters of the match's frames)
        let scripted = sh.script.as_mut().map(|s| std::mem::take(&mut s.match_input)).unwrap_or_default();
        let script_enter = scripted.iter().any(|i| matches!(i, MenuInput::Key { key: "Enter", down: true, .. }));
        let enter = sh.window.held("Enter");
        let opened = !chat.is_open() && (sh.window.pressed("Enter") || script_enter) && !self.chat_enter;
        self.chat_enter = enter;
        if opened {
            let shift = sh.window.held("LeftShift") || sh.window.held("RightShift");
            if chat.open_box(shift, enabled, paused) {
                // (the opener's Enter and its character are the box's)
                sh.window.take_menu_input();
                sh.window.take_menu_keys();
                sh.devices.take_chars();
                release_keyboard(&mut sh.devices);
            }
            return;
        }
        if !chat.is_open() {
            return;
        }
        // Deviation: the box's text is typed at its end only: Backspace takes the last character back; no caret
        // movement, selection or paste (a text field has them).
        let mut text = chat.input.clone();
        for i in sh.window.take_menu_input().into_iter().chain(scripted) {
            match i {
                MenuInput::Char('\u{8}') => {
                    if let Some((at, _)) = text.grapheme_indices(true).next_back() {
                        text.truncate(at);
                    }
                }
                MenuInput::Char(c) if c.is_control() => {}
                MenuInput::Char(c) => text.push(c),
                MenuInput::Key { key, down: true, .. } => {
                    let k = match key {
                        "Escape" => BoxKey::Escape,
                        "Tab" => BoxKey::Tab,
                        "Enter" => BoxKey::Enter,
                        _ => continue,
                    };
                    chat.set_input(&text);
                    if let Some(Submit::Send { text, all }) = chat.key(k, enabled, now)
                        && let Ticks::Online(o) = &mut self.ticks
                    {
                        o.send_chat(&text, all);
                    }
                    if !chat.is_open() {
                        break;
                    }
                    text = chat.input.clone();
                }
                _ => {}
            }
        }
        if chat.is_open() {
            chat.set_input(&text);
        }
        sh.window.take_menu_keys();
        sh.devices.take_chars();
        release_keyboard(&mut sh.devices);
    }

    /// An input script's `win` (its Win gate): from the match's first frame 120 ticks on, HandleWinLoss for team 0
    /// (the dev harness's podium(0)); the gate is released when the match ends (`over`). Not in the Lab, which has no
    /// end.
    /// Dev only: in an online match the win is this game's own, not the session's (the referee and the other games
    /// do not hear it; the two-exe runs give each exe the same script, so each ends on the same tick).
    fn script_win(&mut self, sh: &Shell) {
        let wanted = sh.script.as_ref().is_some_and(|s| s.player.waiting() == Some(Gate::Win));
        if !wanted || !self.started || self.lab.is_some() {
            return;
        }
        let tick = self.world.flow.tick;
        let at = *self.win_at.get_or_insert(tick + 120);
        if tick >= at && !self.world.flow.match_over {
            mvs_sim::world::flow::handle_win_loss(&mut self.world, 0);
        }
    }

    /// An online match's start gate (Online::gate): true once its ticks may run; a match of another kind at once.
    fn online_gate(&mut self) -> Result<bool> {
        match &mut self.ticks {
            Ticks::Online(o) => o.gate(&self.world),
            _ => Ok(true),
        }
    }

    /// An online frame's ticks with a session: the session keeps level with the referee's clock
    /// itself. A tick that waited (its rulings late) ends the frame's ticks with at most half a step owed; one that
    /// skipped (ahead of the referee's clock) costs that tick alone; one that caught up owes no more.
    fn online_ticks(&mut self, sh: &mut Shell, dt: f64) -> Result<()> {
        self.acc += dt;
        while self.acc >= STEP && self.phase == Phase::Playing {
            let t0 = Instant::now();
            sh.devices.read_prefs(&sh.settings.input);
            sh.devices.poll();
            let device = sh.devices.device(self.me).map_or(NO_INPUT, |d| d.read());
            let Ticks::Online(o) = &mut self.ticks else { unreachable!("an online match") };
            let r = o.tick(&mut self.world, &mut self.bots, device);
            let t1 = Instant::now();
            sh.perf.section("sim:step", ms(t0, t1));
            if let Some(events) = r.events {
                self.world.events = events;
                self.present_tick(sh)?;
                sh.perf.section("sim:show", ms(t1, Instant::now()));
            }
            self.acc -= STEP;
            if r.waited && !r.skipped {
                self.acc = self.acc.min(STEP * 0.5);
                break;
            }
            if r.caught {
                self.acc = 0.0;
                break;
            }
        }
        Ok(())
    }

    /// The Lab's frame: the menu's, its effects on the match. A spawn that changes the roster or the map ends the match
    /// into its restart.
    fn lab_frame(&mut self, sh: &mut Shell) -> Result<LabStep> {
        if let Some(carry) = self.lab_swap_ready(sh)? {
            return Ok(LabStep::Restart(carry));
        }
        let tick = u32::try_from(self.world.flow.tick).unwrap_or(0);
        let Some(lab) = &mut self.lab else { return Ok(LabStep::Stay) };
        let out = lab.frame(sh, &mut self.world, tick, self.real);
        if let Some(u) = sh.ui.as_mut() {
            self.menu_list.elements = std::mem::take(&mut u.list.elements);
        }
        if out.config_changed {
            self.sync_lab_config();
        }
        let mut effects = out.effects;
        effects.extend(self.lab_effect.take());
        for e in effects {
            match e {
                LabEffect::Paused(true) => self.rumble.stop_all(&mut sh.devices),
                LabEffect::Paused(false) => {}
                LabEffect::AbductAll => {
                    self.abduct_pending = true;
                    self.lab_ahead(sh);
                }
                LabEffect::Reset { starting_damage } => self.lab_reset(sh, &starting_damage),
                LabEffect::MapLoad(roll) => {
                    // OnMapLoadRequested: the cover is up (LabHost); Destroy All Players at the next tick, the music
                    // and ambience out under the cover (the port's own)
                    self.abduct_pending = true;
                    if let Some(m) = &mut self.match_audio {
                        m.map_cover(self.world.flow.tick, &mut self.calls);
                        play(&mut sh.audio, &mut self.calls);
                    }
                    return self.lab_swap(sh, roll, true);
                }
                LabEffect::Restart(roll) => return self.lab_swap(sh, roll, false),
                // (the queue's pre-match screen follows on the return path: OnlineHub::lab_return)
                LabEffect::Leave => return Ok(LabStep::Leave),
            }
        }
        self.lab_map_ahead(sh);
        Ok(if out.leave { LabStep::Leave } else { LabStep::Stay })
    }

    /// A map picked in the paused menu: its match starts loading on the loader then, ahead of the unpause that asks
    /// for it; a load ahead the menu has left (the map set back, the config changed) is dropped.
    fn lab_map_ahead(&mut self, sh: &Shell) {
        let Some(lab) = &mut self.lab else { return };
        // (a restart asked for and waited on is the swap's, never replaced here)
        if self.lab_next.as_ref().is_some_and(|n| n.asked) {
            return;
        }
        let ahead = lab.map_ahead(sh);
        let ours = |n: &LabNext| n.map_load && !n.asked;
        match ahead {
            Some((config, rolled)) => {
                if !self.lab_next.as_ref().is_some_and(|n| ours(n) && n.config == config && n.rolled == rolled) {
                    self.lab_next = Self::lab_load(sh, config, rolled, true);
                }
            }
            None if self.lab_next.as_ref().is_some_and(ours) => self.lab_next = None,
            None => {}
        }
    }

    /// Spawn All Players began on another roster: its match starts loading on the loader during the spawn's delay.
    fn lab_ahead(&mut self, sh: &Shell) {
        let Some(lab) = &mut self.lab else { return };
        let Some((config, rolled)) = lab.restart_ahead(sh) else { return };
        if !self.lab_next.as_ref().is_some_and(|n| n.config == config && n.rolled == rolled && !n.map_load) {
            self.lab_next = Self::lab_load(sh, config, rolled, false);
        }
    }

    /// The match of `config` and `rolled` loading on the loader (None: nothing to load it with).
    fn lab_load(sh: &Shell, config: LabConfig, rolled: LabRoll, map_load: bool) -> Option<LabNext> {
        let start = crate::lab_setup::start_of(sh, &config, &rolled);
        let job = PrepareJob::of(sh, &start, !map_load)?;
        Some(LabNext { config, rolled, map_load, pending: Pending::start(job), asked: false })
    }

    /// The control's restart (a roster change, a map load): into the match loaded ahead when it is this one, else
    /// one loaded now; the match goes on until its load is ready (a scripted run waits for it), and a map load's
    /// until its cover is fully in.
    // A roster restart whose load is not ready at its tick (a fighter met for the first time) comes when it is, the
    // match before running on with its fighters abducted. A map load's match before runs on under the cover.
    fn lab_swap(&mut self, sh: &mut Shell, rolled: LabRoll, map_load: bool) -> Result<LabStep> {
        let Some(config) = self.lab.as_ref().map(|l| l.menu.session.config.clone()) else { return Ok(LabStep::Stay) };
        let same =
            self.lab_next.as_ref().is_some_and(|n| n.config == config && n.rolled == rolled && n.map_load == map_load);
        if !same {
            self.lab_next = Self::lab_load(sh, config, rolled.clone(), map_load);
        }
        let Some(n) = &mut self.lab_next else {
            return self.lab_restart(sh, rolled, map_load, None).map(LabStep::Restart);
        };
        n.asked = true;
        Ok(match self.lab_swap_ready(sh)? {
            Some(carry) => LabStep::Restart(carry),
            None => LabStep::Stay,
        })
    }

    /// The restart asked for, once its load is ready. A load made for a roster or map the menu has since left (the
    /// roster set back to the running one while it loaded) is dropped, never installed; a toggle changed meanwhile
    /// (cooldowns, penalty) does not drop it: the carry takes the menu's config as it is.
    fn lab_swap_ready(&mut self, sh: &mut Shell) -> Result<Option<Box<LabCarry>>> {
        let current = self.lab.as_ref().map(|l| &l.menu.session.config);
        let left = |n: &LabNext| {
            current.is_none_or(|c| {
                c.players != n.config.players
                    || c.selected_map != n.config.selected_map
                    || c.mutators != n.config.mutators
            })
        };
        if self.lab_next.as_ref().is_some_and(|n| n.asked && left(n)) {
            self.lab_next = None;
            return Ok(None);
        }
        let Some(n) = self.lab_next.as_ref().filter(|n| n.asked) else { return Ok(None) };
        // (a map load's match before ends once its cover is fully in: the swap is never seen through it)
        if n.map_load && !self.lab.as_ref().is_some_and(LabHost::covered) {
            return Ok(None);
        }
        if sh.waits_loads() {
            let n = self.lab_next.take().expect("checked above");
            let prepared = n.pending.wait();
            return self.lab_restart(sh, n.rolled, n.map_load, prepared).map(Some);
        }
        let Some(prepared) = self.lab_next.as_mut().and_then(|n| n.pending.take()) else { return Ok(None) };
        let n = self.lab_next.take().expect("checked above");
        self.lab_restart(sh, n.rolled, n.map_load, prepared).map(Some)
    }

    /// TrainGameMode Handle Training Config Updated: the World's same-attack penalty and instant cooldowns follow the
    /// config (read at the next hit and the next ability).
    fn sync_lab_config(&mut self) {
        let Some(lab) = &self.lab else { return };
        crate::match_lab::config_to_world(&mut self.world, lab.config());
    }

    /// labReset: Spawn All Players on the roster as it is: every fighter takes its slot's starting damage and the World
    /// resets (new pawns), the effects and the sounds but the level's cleared.
    fn lab_reset(&mut self, sh: &mut Shell, starting_damage: &[f64]) {
        self.update_lab_bots();
        for (&f, &d) in self.world.players.clone().iter().zip(starting_damage) {
            self.world.fighter_mut(f).stats.starting_damage = mvs_sim::math::narrow(d);
        }
        mvs_sim::world::reset::reset(&mut self.world);
        // (the controllers are kept and possess the new pawns: BtBot.respawned)
        for bot in self.bots.iter_mut().flatten() {
            bot.respawned();
        }
        self.sync_lab_config();
        self.fx.clear();
        self.poser.reset();
        if let Some(m) = &mut self.match_audio {
            m.lab_reset(self.world.flow.tick, &mut self.calls);
            play(&mut sh.audio, &mut self.calls);
        }
    }

    /// UpdateBotConfigs at Spawn All Players (the oracle's spawnAllPlayers -> setBotEntry, app/match/lab.ts): a bot
    /// whose slot's behaviour changed in the config gets a new controller of it, with the old one's idle flag and
    /// clock; the others are kept.
    fn update_lab_bots(&mut self) {
        let (Some(lab), Some(trees)) = (&self.lab, self.world.bot_trees.clone()) else { return };
        let config = lab.config();
        let slots = crate::lab_setup::lab_slots(config);
        for (i, bot) in self.bots.iter_mut().enumerate() {
            let (Some(old), Some(&slot)) = (bot.as_mut(), slots.get(i)) else { continue };
            let want = config.players[slot].bot_difficulty;
            if want == 0 || self.lab_bot_behaviors.get(i) == Some(&want) {
                continue;
            }
            let mut new = Self::lab_bot_of(&trees, want);
            with_workers(&mut new);
            new.idle = old.idle;
            new.ticks = old.ticks;
            new.now = old.now;
            *old = new;
            if let Some(b) = self.lab_bot_behaviors.get_mut(i) {
                *b = want;
            }
        }
    }

    /// The match ends into the Lab's restart on the roll: the menu, camera and music go with it.
    fn lab_restart(
        &mut self,
        sh: &mut Shell,
        rolled: LabRoll,
        map_load: bool,
        prepared: Option<Box<Prepared>>,
    ) -> Result<Box<LabCarry>> {
        self.keep_level = !map_load;
        self.finish(sh)?;
        let lab = self.lab.take().expect("a Lab match");
        let size = sh.swap.size;
        let camera =
            std::mem::replace(&mut self.cam, MatchCamera::new(&self.world, f64::from(size[0]) / f64::from(size[1])));
        let music = self.match_audio.as_ref().map(MatchAudio::music_flow).unwrap_or_default();
        let toggles = lab.menu.session.toggles;
        let config = lab.menu.session.config.clone();
        let menu = lab.menu;
        Ok(Box::new(LabCarry { config, toggles, camera, music, bot_idle: false, rolled, menu, map_load, prepared }))
    }

    /// The pause menu's frame: the controllers' hold and Escape, the menu's frame painted into `menu_list`. True when
    /// the menu left the match.
    fn pause_frame(&mut self, sh: &mut Shell, f: &Frame) -> bool {
        let keys: Vec<&'static str> = if sh.window.pressed("Escape") { vec!["Escape"] } else { Vec::new() };
        sh.devices.poll();
        let Some(host) = &mut self.pause else { return false };
        // Each controller's pad is its human's device's ('kbm+pad0' the first pad, a pad device
        // its own, the keyboard alone none).
        let down: Vec<bool> = host
            .pause
            .ctrls
            .iter()
            .map(|c| {
                sh.devices
                    .get(c.slot)
                    .and_then(|d| d.pad.as_ref())
                    .is_some_and(|p| p.buttons & (1 << crate::pause::SPECIAL_RIGHT) != 0)
            })
            .collect();
        let kb = host.pause.ctrls.iter().position(|c| sh.devices.get(c.slot).is_some_and(|d| d.keyboard()));
        // CanPause from the countdown card's end (the HUD's onCountdownFinished), or in a tutorial, which has no
        // countdown, from UncontrolTutorialPawn's (pcCanPause).
        if self.ui.as_ref().is_some_and(|u| u.hud.countdown_finished())
            || self.tutorial.as_ref().is_some_and(TutorialHost::pc_can_pause)
        {
            host.pause.can_pause = true;
        }
        // UI_Game Can Pause's tutorial tests and the GameState's skippable step, as they stand at this frame
        if let Some(t) = &self.tutorial {
            host.pause.tutorial_can_pause = t.can_pause();
            host.pause.tutorial_skippable = mvs_sim::modes::tutorial::run::is_skippable(&self.world);
        }
        let tutorial_up = self.tutorial.as_ref().is_some_and(|t| t.screen.active());
        let tick = u32::try_from(self.world.flow.tick).unwrap_or(0);
        let escape_held = sh.window.held("Escape");
        let out = host.frame(sh, &keys, escape_held, &down, kb, tick, f.dt.clamp(0.0, MAX_DT), self.real, tutorial_up);
        // The menu's Skip Step (UI_PauseMenu @9173 -> @5337): the GameState's tutorial actors' BPI_SkipTutorialStep,
        // their step machine's calls to the widgets after it.
        for skip in host.pause.take_tutorial_skips() {
            let from = self.world.events.len();
            match skip {
                crate::pause::TutorialSkip::Step => mvs_sim::modes::tutorial::run::skip_step(&mut self.world),
                crate::pause::TutorialSkip::ToEnd => mvs_sim::modes::tutorial::run::skip_to_end(&mut self.world),
            }
            if let Some(t) = &mut self.tutorial {
                t.take_page_events(&mut self.world, from);
            }
        }
        if let Some(t) = &mut self.tutorial {
            t.input = out.rest;
        }
        // (UIBroker.SetPauseHoldForPlayer: the HUD's UI_IGv3_PauseHold keeps player 0's)
        if let Some(u) = self.ui.as_mut() {
            for &(id, frac) in &out.meters {
                u.hud.pause_hold(i32::try_from(id).unwrap_or(-1), frac);
            }
        }
        if let Some(u) = sh.ui.as_mut() {
            self.menu_list.elements = std::mem::take(&mut u.list.elements);
        }
        out.leave
    }

    /// The match and its results over: left, and where it goes.
    fn over(&mut self, sh: &mut Shell) -> Result<Option<MatchEnd>> {
        // (the port's own) a Waves run left from the pause menu once the sim had settled it, during its end beat:
        // its result is kept
        if let Some(e) = &mut self.waves_end
            && let Some(s) = e.settle(mvs_sim::modes::waves::status(&self.world))
        {
            crate::waves::finish(&mut sh.session, &self.world, &s);
        }
        // (each slot's online id from match.go, before the match's ticks go)
        let peers = match &self.ticks {
            Ticks::Online(o) => o.chat_table().map(|(_, p, _)| p).unwrap_or_default(),
            _ => Vec::new(),
        };
        // (the port's own) a Lab left under a map load's cover: the music faded out under it comes back
        if self.lab.as_ref().is_some_and(|l| l.map_load_active) {
            MatchAudio::map_cover_end(self.world.flow.tick, sh.settings.audio.music, &mut self.calls);
            play(&mut sh.audio, &mut self.calls);
        }
        // a match or a Lab left from its pause menu (Leave Match, the Lab's quit, a level opened from it) lets the pause
        // mix go (UI_LAB_PauseMenu Destruct @2684)
        if self.lab.as_ref().is_some_and(|l| l.control.paused) || self.pause.as_ref().is_some_and(|p| p.pause.paused) {
            MatchAudio::left_paused(self.world.flow.tick, &mut self.calls);
            play(&mut sh.audio, &mut self.calls);
        }
        self.finish(sh)?;
        // (a script's `win` goes on at the screen after the match, once the results are up)
        if let Some(s) = &mut sh.script
            && s.player.waiting() == Some(Gate::Win)
        {
            s.player.release();
        }
        let arcade = matches!(self.kind, MatchKind::Arcade);
        let then = match std::mem::take(&mut self.kind) {
            MatchKind::Custom { lobby } => MatchEnd::Lobby(lobby),
            MatchKind::Default
            | MatchKind::Replay { .. }
            | MatchKind::Lab { .. }
            | MatchKind::LocalPlay
            | MatchKind::Arcade
            | MatchKind::Online { .. }
            | MatchKind::Tutorial
            | MatchKind::Waves { .. } => MatchEnd::Home,
        };
        if !self.to_results {
            // Unverified: a match left from the pause menu ends the run with nothing
            // saved (the build's backend ruled a left arcade match; not read).
            if arcade {
                sh.session.arcade.leave();
            }
            return Ok(Some(then));
        }
        // (the results screen follows, and the end after it)
        let mut eog = end_of_game(&self.world, self.me, &self.usernames, &peers);
        // An arcade round's end advances the run; the end of game reads the
        // round as it was played.
        if arcade {
            eog.arcade = sh.session.arcade.round_played().cloned();
            let won = eog.players.get(eog.me).is_some_and(|p| p.is_winner);
            let mvs_ui::menu::session::Session { arcade, stats, .. } = &mut sh.session;
            arcade.round_result(won, stats);
        }
        Ok(Some(MatchEnd::Results(Box::new(crate::results::ResultsStart {
            eog,
            then,
            shot: self.results_shot,
            out: self.out.clone(),
        }))))
    }

    /// The frame drawn on the back buffer: the views and camera at the step's alpha, the renderer, the HUD or the
    /// results, the fade, the perf line.
    fn draw(&mut self, sh: &mut Shell, dt: f64) -> Result<()> {
        let t0 = Instant::now();
        self.label_types(&sh.devices);
        let alpha = if self.fixed() { 1.0 } else { (self.acc / STEP) as f32 };
        let camera = self.cam.camera(alpha);
        self.poser.set_camera(self.cam.rig.root, self.cam.rig.pov.up);
        self.poser.present(&self.world, alpha, &mut self.fighters, &mut self.actors);
        let Shell { swap, renderer, perf, audio, ui, settings, .. } = sh;
        let renderer = renderer.as_mut().expect(NO_STORE);
        // (after the camera: what follows its centre as drawn)
        // (the platforms between the same two ticks as the fighters; the views that are the scenery's meshes)
        if self.world.data.stage_key == self.stage_key {
            renderer.platforms_at(f64::from(alpha));
        }
        let places = self.poser.scenery_places();
        for p in places {
            renderer.scenery_place(p.block, p.part, p.shown.as_ref().map(|(v, m)| (*v, *m)));
        }
        // (a part no view places this frame is hidden: the court's ball after its item goes, a Lab reset, the end)
        if !places.iter().any(|p| (p.block, p.part) == ("court", "ball")) {
            renderer.scenery_place("court", "ball", None);
        }
        renderer.scenery_frame(f64::from(alpha), Some(self.cam.rig.center), Some(&mut self.fx.player));
        if let (Some(u), Some(s)) = (&mut self.ui, ui.as_mut()) {
            u.widgets.paint(renderer, &mut s.ui, &mut u.hud_fonts)?;
        }
        self.fx.frame(&self.world, f64::from(alpha));
        // (the modules' frame posts, the scenery's, then the listener at the camera as drawn)
        if let Some(m) = &mut self.match_audio {
            m.frame(self.world.flow.tick, &self.cam.rig.pov, &mut self.fx, &mut self.calls);
            play(audio, &mut self.calls);
        }
        self.fx.sounds.clear();
        // (the Lab Tools' Hide Effects: the effects' particles, floor decals and component meshes not drawn, the
        // effects group; a stage's placed decals stay with its scenery)
        let hide_fx = settings.lab.tools_on && self.lab_tools().is_some_and(|t| t.hide_fx);
        if hide_fx {
            renderer.floor_decals.clear();
            renderer.set_component_meshes(&mvs_render::cmesh::ComponentMeshes::default());
        } else {
            renderer.floor_decals = self.fx.decals().to_vec();
            renderer.set_component_meshes(&self.fx.components);
        }
        // (the FreeForAll shrink, and a reset world's box)
        renderer.set_blast_box(&self.world.stage.blast);
        let forward = [0, 1, 2].map(|i| camera.translated_world_to_view[i][2]);
        self.fx.player.set_view(camera.origin, forward);
        let t1 = Instant::now();
        perf.section("views", ms(t0, t1));
        let (game, real) = if self.fixed() {
            let t = f64::from(self.world.flow.tick) * STEP;
            (self.fx.player.game_time(), t)
        } else {
            // The materials' time is the particle player's clock (a step per tick) plus the frame's part of a step, so
            // it moves smoothly between ticks.
            (self.fx.player.game_time() + self.acc, self.real)
        };
        let input = FrameInput {
            #[allow(clippy::cast_sign_loss)]
            tick: self.world.flow.tick.max(0) as u32,
            alpha,
            time: FrameTime { game: game as f32, real: real as f32, delta: dt as f32 },
            camera: &camera,
            fighters: &self.fighters,
            actors: &self.actors,
            events: &self.events,
        };
        let back = swap.back_buffer();
        let particles = if hide_fx { None } else { Some(&mut self.fx.player) };
        let times = renderer.draw(&input, particles, Some(back))?;
        let t2 = Instant::now();
        perf.section("render", ms(t1, t2));
        perf.gpu(times.gpu, &times.passes);
        let fixed = self.fixed();
        let (Some(u), Some(s)) = (&mut self.ui, ui.as_mut()) else { return Ok(()) };
        s.list.clear();
        let size = swap.size;
        let (gpu, assets) = (renderer.gpu(), renderer.assets());
        let fonts = {
            // Per HUD pawn (the players by index, then the mode pawns bound to a slot after the start): a detached
            // pawn's place, or one the World no longer holds, has none.
            let n = u.hud.pawns().len();
            let mut bottom = Vec::with_capacity(n);
            let mut top = Vec::with_capacity(n);
            for (k, id) in u.hud.pawns().iter().enumerate() {
                let Some(f) = id.and_then(|id| self.world.by_id(id)) else {
                    bottom.push(None);
                    top.push(None);
                    continue;
                };
                let p = self.poser.hud_point(&self.world, f, alpha);
                let (zb, zt) = match self.ui_z.get(k) {
                    Some(&z) => z,
                    None => {
                        let key = self.world.names.str(self.world.fighter(f).ch);
                        (widget_z(&self.world.data, key, "Bottom"), widget_z(&self.world.data, key, "Top"))
                    }
                };
                bottom.push(p.map(|p| anchor(&camera, size, &p, zb)));
                top.push(p.map(|p| anchor(&camera, size, &p, polymorph_top(&self.world, f, &p).unwrap_or(zt))));
            }
            let items: Vec<(i32, [f64; 3])> = self
                .world
                .actors
                .items
                .iter()
                .map(|(_, it)| {
                    let a = f64::from(alpha);
                    (
                        it.seq,
                        [
                            0.0,
                            -(f64::from(it.prev_x) + f64::from(it.x - it.prev_x) * a),
                            f64::from(it.prev_y) + f64::from(it.y - it.prev_y) * a,
                        ],
                    )
                })
                .collect();
            let seated: Vec<bool> = (0..self.world.players.len()).map(|i| sh.devices.get(i).is_some()).collect();
            let skillshots = Self::skillshots(&self.world, &seated, matches!(self.ticks, Ticks::Online(_)), alpha);
            let frame = HudFrame {
                size,
                hud_time: self.hud_time + self.acc,
                real,
                bottom: &bottom,
                top: &top,
                camera: &camera,
                items: &items,
                skillshots: &skillshots,
                round_time_remaining: f64::from(self.world.round_time_remaining()),
                hold: false,
            };
            u.hud.draw(&frame, &mut u.hud_fonts, &mut s.list);
            // (the port's own) an online match's away plates and "Reconnecting...", over the HUD
            if let Ticks::Online(o) = &self.ticks {
                let (away, left) = o.away();
                if !away.is_empty() || left.is_some() {
                    let hud = &u.hud;
                    let slot_rect = |p: usize| hud.slot_rect(p);
                    let o = AwayOverlay { size, layer: TOP_LAYER - 5, slot_rect: &slot_rect };
                    mvs_ui::hud::away::draw(&o, &away, left, &mut u.hud_fonts, &mut s.list);
                }
            }
            // (the port's own) the chat's bubbles at the scoreboard's slots and its box, over the HUD
            if let Some(c) = &mut self.chat {
                let hud = &u.hud;
                let slot_rect = |p: usize| hud.slot_rect(p);
                // Unverified: the caret blinks 500 ms on, 500 ms off (Chromium's text field; chat.css sets none).
                #[allow(clippy::cast_possible_truncation)]
                let caret = (real * 2.0).floor() as i64 % 2 == 0;
                let o = ChatOverlay {
                    size,
                    layer: TOP_LAYER - 3,
                    bottom: settings.ui.hud_position_bottom,
                    slot_rect: &slot_rect,
                    caret,
                };
                c.draw(&o, real * 1e3, &mut u.hud_fonts, &mut s.list);
            }
            // UI_Game's posts (PF_Post_Global_Event)
            let sounds = u.hud.take_sounds();
            if let Some(m) = &mut self.match_audio {
                m.hud_sounds(self.world.flow.tick, sounds, &mut self.calls);
                play(audio, &mut self.calls);
            }
            &mut u.hud_fonts
        };
        // The Lab Tools: the HUD hidden, the sim's shapes over the match.
        if self.lab.is_some() && settings.lab.tools_on {
            if self.lab.as_ref().and_then(|l| l.menu.tools.as_ref()).is_some_and(|t| t.hide_hud) {
                s.list.clear();
            }
            let at: Vec<_> = self.world.players.iter().map(|&f| self.poser.hud_point(&self.world, f, alpha)).collect();
            if let Some(t) = self.lab.as_mut().and_then(|l| l.menu.tools.as_mut()) {
                let mut o = crate::shapes::Overlay {
                    camera: &camera,
                    size,
                    list: &mut s.list,
                    fonts: &mut *fonts,
                    layer: TOP_LAYER - 1,
                    at: &at,
                    alpha: f64::from(alpha),
                };
                let show = t.show;
                crate::shapes::draw(&mut o, &mut t.shapes, &mut self.world, &show);
            }
        }
        // PreTravel's fade to black, over everything.
        if let Some(t) = self.fade_start {
            let a = ((real - t) / FADE).clamp(0.0, 1.0);
            let geometry = PaintGeometry { size: size.map(f64::from), transform: [1.0, 0.0, 0.0, 1.0, 0.0, 0.0] };
            let el = BoxEl { geometry, tint: [0.0, 0.0, 0.0, a], uv: [0.0, 0.0, 1.0, 1.0], ..BoxEl::default() };
            s.list.push(TOP_LAYER, None, DrawEffects::NONE, Element::Box(el));
        }
        // `stat fps`'s line (Show Framerate); not on a held clock's pictures.
        let held = !self.shots.is_empty() || (matches!(self.ticks, Ticks::Replay(_)) && !self.real_pace);
        if let Some(line) = crate::fps::line(settings.graphics.show_fps && !held, perf.readout.as_ref()) {
            crate::fps::draw(&mut s.list, fonts, size, TOP_LAYER + 1, &line);
        }
        let t3 = Instant::now();
        perf.section("hud", ms(t2, t3));
        // The UI materials' View time is the frame's real time.
        s.ui.time = Some(real);
        // (a held clock's pictures wait for their pictures)
        s.ui.stream_pictures = !fixed;
        s.timer.begin_frame(gpu);
        s.ui.draw(gpu, assets, fonts, &s.list, back)?;
        if let Some(t) = &self.tutorial
            && !t.list.elements.is_empty()
        {
            s.ui.draw(gpu, assets, &mut s.menu_fonts, &t.list, back)?;
        }
        if !self.menu_list.elements.is_empty() {
            s.ui.draw(gpu, assets, &mut s.menu_fonts, &self.menu_list, back)?;
        }
        s.timer.mark(gpu, "ui");
        s.timer.end_frame(gpu);
        if let Some(m) = s.timer.read(gpu) {
            for (n, v) in m {
                perf.gpu_pass(n, v);
            }
        }
        perf.section("ui", ms(t3, Instant::now()));
        Ok(())
    }

    /// The back buffer of the shot's tick written as `<out>/shot-<tick>.png`.
    fn shot(&mut self, sh: &mut Shell) -> Result<()> {
        let tick = self.shots.remove(0);
        let px = sh.renderer.as_ref().expect(NO_STORE).read_back(sh.swap.back_buffer())?;
        let [w, h] = sh.swap.size;
        let path = self.out.join(format!("shot-{tick}.png"));
        mvs_render::image::write_png(&path, w, h, &px).map_err(Error::Match)?;
        eprintln!("mvs: {}", path.display());
        Ok(())
    }

    /// The match left: the pads' motors off, a replay's trace written (to `--trace-out` only).
    fn finish(&mut self, sh: &mut Shell) -> Result<()> {
        self.rumble.stop_all(&mut sh.devices);
        // (the scenery's blocks hand back what they took, before the effects end; a Lab fighter change keeps the
        // level, the scenery outliving the match)
        // The scenery ends at every match end but a Lab fighter change (the renderer's set_stage rebuilds it for every
        // match).
        if !self.keep_level
            && std::mem::take(&mut sh.scenery_open)
            && let Some(r) = &mut sh.renderer
        {
            r.scenery_end(Some(&mut self.fx.player));
        }
        // (the actors' effects and the modules' ends)
        self.fx.end(&self.world);
        // (a kept level's particle player goes on into the next match with the level's systems alone, whose ids the
        // scenery's blocks hold)
        if self.keep_level
            && sh.scenery_open
            && let Some(store) = sh.store()
        {
            let kept = self.fx.keep_level(store)?;
            sh.kept_level = Some(kept);
        }
        if let Some(m) = &mut self.match_audio {
            m.end(self.world.flow.tick, self.keep_level, &mut self.calls);
            play(&mut sh.audio, &mut self.calls);
        }
        let ticks = std::mem::replace(&mut self.ticks, Ticks::Local);
        if let Ticks::Online(o) = ticks {
            o.finish();
            return Ok(());
        }
        if let Ticks::Replay(r) = ticks
            && let Some((path, out)) = r.out
        {
            let mut header = r.trace.header.clone();
            header.source = serde_json::json!({ "writer": "mvs" });
            std::fs::write(&path, out.finish(header)).map_err(|e| Error::Match(format!("{}: {e}", path.display())))?;
            eprintln!("mvs: {} ({} ticks)", path.display(), r.next);
        }
        Ok(())
    }
}

fn ms(a: Instant, b: Instant) -> f64 {
    (b - a).as_secs_f64() * 1e3
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_anchor_on_the_view_axis_is_the_centre() {
        // Looking down UE +X from the origin: view X right = UE Y, view Y up = UE Z, view Z forward = UE X.
        let mut twv = [[0.0; 4]; 4];
        twv[1][0] = 1.0;
        twv[2][1] = 1.0;
        twv[0][2] = 1.0;
        twv[3][3] = 1.0;
        // A reversed-Z projection with a 90 degree field of view: clip w = view z.
        let mut v2c = [[0.0; 4]; 4];
        v2c[0][0] = 1.0;
        v2c[1][1] = 1.0;
        v2c[3][2] = 10.0;
        v2c[2][3] = 1.0;
        let cam = Camera { origin: [-1000.0, 0.0, 0.0], translated_world_to_view: twv, view_to_clip: v2c };
        let p = HudPoint { x: 0.0, y: 0.0, d: 0.0, mesh_z: 0.0, mesh_scale: 1.0 };
        let a = anchor(&cam, [1920, 1080], &p, 0.0);
        assert_eq!((a.x, a.y, a.dist), (960, 540, 1000.0));
        // 500 cm up at 1000 cm: ndc y 0.5, a quarter of the height above the centre.
        let a = anchor(&cam, [1920, 1080], &HudPoint { y: 500.0, ..p }, 0.0);
        assert_eq!((a.x, a.y), (960, 270));
    }
}
