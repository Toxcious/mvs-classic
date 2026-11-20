//! The match HUD: what the World and its events show, per tick
//! (`observe`) and per frame (`draw`).
//!
//! The path ported: the players' fighters and the mode pawns bound to a slot after the start (`attach` /
//! `detach`), a minion's queued pop texts, offline (no online match, every pawn locally controlled), the shared
//! widgets, the cooldowns, the pawns' custom UIs, the shared pop texts and those the render modules raise (Arya's
//! pie, LeBron's pass: read off the World after each observe). The characters' own parts (chars/), the
//! dash targets, the skill shot targets and the item indicators are drawn in their places of the paint order and
//! named on the HUD tape. The other events are passed over where they are matched below.

mod attached;
mod attached_top;
pub mod away;
mod chars;
mod common;
mod countdown;
mod custom_text;
mod custom_ui;
mod dash_target;
mod fixed;
mod indicator;
mod input_label;
mod pop;
mod skill_shot;
mod victory;
pub mod world_widget;

use std::collections::{HashMap, HashSet, VecDeque};

use indexmap::IndexMap;
use mvs_sim::actors::world_actor::WorldActorState;
use mvs_sim::combat::pop_text::GraphPopText;
use mvs_sim::consts::MAX_EVADE_COUNT;
use mvs_sim::events::{EndPhase, Fid, PopKind, PopParams as SimPop, UiStateOp};
use mvs_sim::fighter::FighterId;
use mvs_sim::fighter::control::ability_row;
use mvs_sim::names::Name;
use mvs_sim::{Event, World};
use mvs_view::Camera;

use crate::Result;
use crate::bundle::{Bundle, Rgba};
use crate::element::{BlurEl, DrawEffects, Element, ElementList, PaintGeometry};
use crate::slate::{HAlign, Tree, VAlign, WidgetId};
use crate::text::{Fonts, GameText};

pub use attached::dpi_scale;
use attached::{AttachedBottom, COOLDOWN_ANIMS, HudAbility, HudAbilityOp, proto};
use attached_top::{AttachedTop, StateParams};
use chars::CharHud;
pub(crate) use common::PlayerNumbers as LocalPlayNumbers;
use common::{Anim, BrokerPlayer, PlayerNumbers, Prefs, brush_path, grouped, palette_ui};
use countdown::Countdown;
use dash_target::DashTargets;
use fixed::{Scoreboard, ScoreboardSetup};
use indicator::ItemIndicators;
use input_label::{InputLabels, LabelParams, LabelSources, fighter_key, pickup_item_action};
pub use input_label::{LABEL_ACTIONS, LabelInput, Slots};
use pop::{Place, PopId, PopParams, PopTexts};
use skill_shot::SkillshotTargets;
use victory::VictoryText;

/// The Lab's combo texts (TrainGameMode OnPlayerJuggled -> TrainingGameState DisplayComboCounts): the Lab menu's
/// Show Juggle Combo and Show Hitstun Combo, and each fighter's PlayerIndex (its config slot, by sim id).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct LabCombos {
    pub juggle: bool,
    pub hitstun: bool,
    pub slot_of: Vec<usize>,
}

/// A player of the match as the HUD shows it (HudSetup.players).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct HudPlayer {
    pub team: u8,
    pub bot: bool,
    pub username: String,
    /// The fighter's roster name; None: the default one.
    pub character: Option<String>,
}

/// The match's HUD setup.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct HudSetup {
    /// The HUD's context mode (ContextMode: 1 to 3 a versus match by its team style, 4 the Lab, 5 a tutorial; the
    /// port's own `MODE_WAVES`).
    pub mode: u8,
    /// The local player's index.
    pub me: usize,
    pub players: Vec<HudPlayer>,
    /// KnockoutsToWin.
    pub max_score: i32,
    /// The experimental stock rule: max_score is each player's lives.
    pub stocks: bool,
    /// A Local Play match (IsLocalPlay).
    pub local_play: bool,
    /// A relay match (PlayingOnline).
    pub online: bool,
    /// An arcade round (IsArcadeModeActive): MatchInfo's and the countdown's "Stage N of M".
    pub arcade: Option<HudArcade>,
    /// The UI preferences (Struct_Preferences_UI) as the match's start has them: the widgets' RespondToPreferences
    /// and the colour bindings PaletteForIndex resolves through.
    pub ui: mvs_settings::UiSettings,
}

/// (the port's own) The HUD context mode of a Waves run: the player's portrait alone, the run's counter and clock.
pub const MODE_WAVES: u8 = 6;

/// (the port's own) A Waves run as the HUD reads it each frame (`Hud::set_waves`).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct WavesHud {
    /// Enemies rung out so far.
    pub ring_outs: u32,
    /// The count that clears the run (100 Ring-Outs), or None (Endless).
    pub target: Option<u32>,
    /// The run's clock, seconds.
    pub seconds: f64,
}

/// An arcade round as the HUD reads it (AXFunctions GetArcadeInfo's round, GetNumArcadeLevels).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct HudArcade {
    /// 1-based.
    pub round: i32,
    pub levels: i32,
}

/// A fighter's HUD anchor on the frame: its projected point rounded to whole pixels and
/// its distance from the camera, cm.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Anchor {
    pub x: i32,
    pub y: i32,
    pub dist: f32,
}

/// One drawn frame's inputs.
#[derive(Clone, Copy, Debug)]
pub struct HudFrame<'a> {
    /// The back buffer's size, pixels.
    pub size: [u32; 2],
    /// The HUD's game time and the real time, seconds. The game time is the match loop's: a clock that adds the
    /// step (1/60 s, f64) once per World step, as given to observe (not ticks / 60: the sum rounds its own way),
    /// plus the frame's accumulator (the time not yet stepped).
    pub hud_time: f64,
    pub real: f64,
    /// Per HUD pawn (`Hud::pawns`: the players by index, then the mode pawns attached later): the bottom (name,
    /// damage) and the top (states) anchors; None (or none given) for a pawn not shown.
    pub bottom: &'a [Option<Anchor>],
    pub top: &'a [Option<Anchor>],
    pub camera: &'a Camera,
    /// The items' pickup labels: the item's id and its place (UE cm).
    pub items: &'a [(i32, [f64; 3])],
    /// The shown skill shot indicators: the actor's id and its place (UE cm).
    pub skillshots: &'a [(i32, [f64; 3])],
    /// World::round_time_remaining.
    pub round_time_remaining: f64,
    /// The level's warm-up frames (StartCountdown has not run yet): the versus card stays at its start and none of
    /// its event keys fires.
    pub hold: bool,
}

/// The match loop's step: a render module's pop text shows from the next tick.
const STEP: f64 = 1.0 / 60.0;

/// The string of a Game.locres key (the FText of a Blueprint text constant, namespace empty).
// Unverified: a key the player's Game.locres lacks shows an empty text; the build would show the constant's source
// string, which the port does not hold.
/// UI_IGv3_MatchInfo OnUIContextMode's modeLabel texts by context mode (EX_TextConst @305 "1 vs 1", @1207 "Teams",
/// @1293 "Free For All", @1386 "The Lab"; @85 empties it first, and context 5 has no case).
const MODE_LABEL: [Option<&str>; 6] = [
    None,
    Some("74FEC5C647302F903817209C6A5D3A6D"),
    Some("C7CABD0742F98E8FE0903BA2AF8CD590"),
    Some("C40967F5429C097943763CAE189982D1"),
    Some("AC1B5FC244BF54F5F3B571928D287CDF"),
    None,
];

fn game_text(text: &GameText, key: &str) -> String {
    text.get("", key).unwrap_or_default().to_string()
}

/// "Stage {Number} of {NumRounds}": UI_IGv3_MatchInfo EX_TextConst @978.
const MI_ARCADE_STAGE: &str = "EC318B5F4DE663B69918359482637629";
/// "Stage {Round} of {NumRounds}": UI_Countdown EX_TextConst @4096.
const CD_ARCADE_STAGE: &str = "14A21FC9406DD1FEBD39F087A33446D5";

/// An arcade stage line: the text's Format with the round under `round_arg` and the levels under NumRounds.
fn arcade_stage(text: &GameText, key: &str, round_arg: &str, a: HudArcade) -> String {
    game_text(text, key).replace(round_arg, &a.round.to_string()).replace("{NumRounds}", &a.levels.to_string())
}

/// The widget animations and the bundle's lookups the widgets read after `Hud::new` (the bundle is not kept).
#[derive(Debug, Default)]
struct Res {
    /// The object paths of the bundle's UI materials and textures by the bundle's names (a brush resource a
    /// feeder names; a state's icon and subicon are shown only when the bundle has them).
    brushes: HashMap<String, String>,
    pct_pulse: Anim,
    pct_grey: Anim,
    pct_grey_legacy: Anim,
    pct_grey_mine: Anim,
    pct_pulse_grey: Anim,
    charge_intro: Anim,
    state_pulse: Anim,
    state_enter: Anim,
    struggle_pulse: Anim,
    struggle_delta: Anim,
    portrait_score: Anim,
    /// LeaderAnim_TopHUD / _BotHUD.
    portrait_lead: [Anim; 2],
    /// LeaderLoop_TopHUD / _BotHUD.
    portrait_loop: [Anim; 2],
    pip_score: Anim,
    pips_warning: Anim,
    clock_warning: Anim,
    board_enter: Anim,
    board_exit: Anim,
    label_enter: Anim,
    label_attention: Anim,
    label_exit: Anim,
    pop_enter: Anim,
    pop_exit: Anim,
    /// UI_IGv3_Cooldown's animations (attached.rs COOLDOWN_ANIMS).
    cooldown: [Anim; 4],
    /// UI_FinnCharge MaxCharge, BarShake, ExplosionPulse.
    finn_max: Anim,
    finn_shake: Anim,
    finn_pulse: Anim,
    /// UI_IGv3_PlayerMarker DodgeMeterEmpty (attached.rs: OnArmorUnavailable).
    marker_meter_empty: Anim,
    /// What the input labels read of the players' input (set_label_input).
    labels: LabelSources,
}

impl Res {
    /// The object path of the brush resource the bundle names `name`.
    fn brush(&self, name: &str) -> Option<String> {
        self.brushes.get(name).cloned()
    }

    fn new(b: &Bundle) -> Res {
        let a = |class: &str, name: &str| Anim::of(b, class, name);
        let pct = "UI_IGv3_Percentage";
        let portrait = "UI_IGv3_CharacterPortrait";
        Res {
            brushes: b
                .materials
                .keys()
                .chain(b.textures.keys())
                .filter_map(|n| Some((n.clone(), brush_path(b, n)?)))
                .collect(),
            pct_pulse: a(pct, "Pulse"),
            pct_grey: a(pct, "ShowGreyHealth"),
            pct_grey_legacy: a(pct, "ShowGreyHealth_Legacy"),
            pct_grey_mine: a(pct, "ShowMyGreyHealth"),
            pct_pulse_grey: a(pct, "PulseWhileGrey"),
            charge_intro: a("UI_IGv3_Charge", "IntroAnim"),
            state_pulse: a("UI_IGv3_State", "Pulse"),
            state_enter: a("UI_IGv3_State", "EnterAnim"),
            struggle_pulse: a("UI_IGv3_Struggle", "InputPulse"),
            struggle_delta: a("UI_IGv3_Struggle", "DeltaAnim"),
            portrait_score: a(portrait, "OnScoreChanged"),
            portrait_lead: [a(portrait, "LeaderAnim_TopHUD"), a(portrait, "LeaderAnim_BotHUD")],
            portrait_loop: [a(portrait, "LeaderLoop_TopHUD"), a(portrait, "LeaderLoop_BotHUD")],
            pip_score: a("UI_IGv3_ScorePip", "ScoreAnim"),
            pips_warning: a("UI_IGv3_ScorePips", "WarningAnim"),
            clock_warning: a("UI_IGv3_Clock", "WarningAnim"),
            board_enter: a("UI_IGv3_Scoreboard", "EnterAnim"),
            board_exit: a("UI_IGv3_Scoreboard", "ExitAnim"),
            label_enter: a("UI_PopInputLabel", "EnterAnim"),
            label_attention: a("UI_PopInputLabel", "AttentionAnim"),
            label_exit: a("UI_PopInputLabel", "ExitAnim"),
            pop_enter: a("UI_PopText", "EnterAnim"),
            pop_exit: a("UI_PopText", "ExitAnim"),
            cooldown: COOLDOWN_ANIMS.map(|n| a("UI_IGv3_Cooldown", n)),
            finn_max: a(custom_ui::CLASS, "MaxCharge"),
            finn_shake: a(custom_ui::CLASS, "BarShake"),
            finn_pulse: a(custom_ui::CLASS, "ExplosionPulse"),
            marker_meter_empty: a("UI_IGv3_PlayerMarker", "DodgeMeterEmpty"),
            labels: LabelSources::default(),
        }
    }
}

/// The Blueprint text constants the HUD's feeders show, by their Game.locres keys (each one's class and statement).
#[derive(Debug, Default)]
pub(crate) struct Texts {
    /// PandaGameState ubergraph @7255, @6611, @6683.
    match_point: String,
    blue_advantage: String,
    red_advantage: String,
    /// Comp_Pawn_HitBoxHandler SpawnDodgePopText @214.
    dodged: String,
    /// BaseFighter WallFatigueFX @184.
    wall_fatigue: String,
    /// Comp_Pawn_DamageManager @11206, Comp_State_Manager @7577.
    attack_decay: String,
    buff_decay: String,
    /// Buff_HealOverTime @850.
    healing: String,
    /// Buff_Caught @939, @1836.
    caught: String,
    jumps_reset: String,
    /// Buff_LastStand @1233.
    last_stand: String,
    /// Buff_RefreshingKnockOut @2089, @2211.
    specials_refreshed: String,
    air_refreshed: String,
    /// Buff_TransferDebuffs @1067.
    transferred: String,
    /// Buff_Enlarge SpawnVFX @99.
    morphized: String,
    /// Pawn_C017 RageCallout @23115.
    protect_friends: String,
    /// Comp_C023A_AllyAttach ClenseAttachAlly @1462.
    cleanse: String,
    /// C023A_ForceDance_Debuff @344.
    dancing: String,
    /// Buff_AryaBackStab BackStabUIMulticast @163.
    backstab: String,
    /// Pawn_C017's AbilityAmmoUpdated handler's PopTextMulticast params (@24324..24869).
    bolt: String,
    /// Finn_Shop_Boots_Buff's Format @419 ("Speed Up x{StackAmount}").
    speed_up: String,
    /// FinnHighFiveListener's class default PopText (read @1929).
    high_five: String,
    /// Buff_Finn_GoingOutOfBusiness @2744.
    going_out: String,
    /// c016 Buff_BasketballPass SpawnPopupText @20.
    pass_received: String,
    /// AryaKillPie @2156.
    arya_pie: String,
    /// TrainingGameState AttemptJuggleComboPopup's Format (EX_TextConst @651): the juggle count and damage on two
    /// lines ({JuggleCount}, {JuggleDamage}).
    juggle_combo: String,
    /// AttemptHitstunComboPopup's Format: the hitstun combo's ({HitstunCount}, {HitstunDamage}).
    hitstun_combo: String,
    /// Rick's portal label: PortalParent's EX_TextConst @13879.
    pub(crate) teleport: String,
    /// Velma's call-police label: Pawn_Velma's EX_TextConst @4528.
    pub(crate) solve_mystery: String,
    /// The Down modifier as UI_InputLabel writes it (the switch @768: Format("{modifier} + ") of the Down text,
    /// upper-cased).
    pub(crate) down_prefix: String,
    /// Buff_TurnEnemyAwayOnHit SharpenVisuals @1118.
    turn_attack: String,
    /// Buff_AllyTornadoed's Pop Text Params default (its text).
    reflect: String,
    /// Buff_Cheese BeginPlay @134 and UpdateCheeseStacks @2929.
    cheese_damage: String,
    cheese_two: String,
    /// TimedScoreGameMode @2606 (the respawn's).
    ffa_rungout: String,
    /// Buff_ScoobySnack SpawnVFX's PopText @225 (key 13E6807641BB9335474E83BB40D9FF6F).
    enraged: String,
}

impl Texts {
    fn new(text: &GameText) -> Texts {
        let t = |k: &str| game_text(text, k);
        Texts {
            match_point: t("97DF85FE4DFD44F6D49E27B494C2B076"),
            blue_advantage: t("299C84CF4B1C3DCA9AFF6AB2929B6241"),
            red_advantage: t("4F91679A419902F724AB81A816E9D7C1"),
            dodged: t("971319344D2CEB27CF5231B4A56DA76C"),
            wall_fatigue: t("6C0DABC944F75B535798D7ACC3BB496F"),
            attack_decay: t("05C117C846251BB38B67D1AF9E0B5EC7"),
            buff_decay: t("9FC2DC624CE82E08F71FC98D21CDAB53"),
            healing: t("BB18D2A7443B70AC0E590D93BD539813"),
            caught: t("178EAE6E4AAB9E41064C3CB3311331A2"),
            jumps_reset: t("EC7B0C4E478BBF64BEB8699550D6EB80"),
            last_stand: t("65412CEB40DCDA9968BC4C9943ADF4EF"),
            specials_refreshed: t("916D830B41FF75C48AD1B9AF41DB9721"),
            air_refreshed: t("71AFA5834EA643019B993DAB4B8246C5"),
            transferred: t("9F5BFE5147F21F19200FB580F730CFF8"),
            morphized: t("48C721854824BD3DB58B4D93620F0BFF"),
            protect_friends: t("A882DC564AC074A4349DB5BBEB9A39B2"),
            cleanse: t("BD31705548BDB473C88DA7AAC5C1CD8A"),
            dancing: t("C229C3084D6AC8152106608A5D4090D2"),
            backstab: t("7B93DA074CF73B7020F14ABECFE80FED"),
            bolt: t("29A8D2AC486AA6BC16C3BF95C0A83354"),
            speed_up: t("43BF77BA4F27BFA39D8264A8F6FF3595"),
            high_five: t("[82F205CF4E0CC23987524F9A1F2858FD]"),
            going_out: t("2EFA65834D76E37D476A4F8117DAA264"),
            pass_received: t("E3CCB3504CA8D9683C1AD6B895F280E0"),
            arya_pie: t("99942E0346EECB7FFD0CB78B7BC38AA2"),
            juggle_combo: t("306380A74235B284A62BE8BE72E79F26"),
            hitstun_combo: t("087386E54D678E5D2F9032AACBBA9278"),
            teleport: t("20735CD54C0C8414BB8C60BFBBC61D21"),
            solve_mystery: t("F000BDD549C550EA15EBD0A2FCD51878"),
            down_prefix: t("7F1D1BC742809C58145B67995339C8CA")
                .replace("{modifier}", &t("9FDC516D47362F6A29B7B4A7165B093D"))
                .to_uppercase(),
            turn_attack: t("1ED294654DF10DA7E21083920ED3ACEB"),
            reflect: t("43B0EB124D3E7ACB549E2BA4641AD045"),
            cheese_damage: t("07DD8CEB405E51012D83F59BCC90C331"),
            cheese_two: t("32E15E3246812A63326477BE9959BC7B"),
            ffa_rungout: t("1F9E8F114FCEA621FCF0E98682472D4F"),
            // (the key has no entry in the build's Game.locres, so FText shows the constant's source string: the
            // decode's literal, used where the player's table lacks it too)
            enraged: text.get("", "13E6807641BB9335474E83BB40D9FF6F").unwrap_or("Enraged").to_string(),
        }
    }
}

/// What the HUD last sent a fighter's widgets (FighterHud.sent).
#[derive(Clone, Copy, Debug, Default)]
struct Sent {
    damage: f64,
    grey: i32,
    evade: f64,
    dead: bool,
    /// The sim's chargeUi write count and value at the last send; None before the first observe.
    charge: Option<(u32, (mvs_sim::Real, mvs_sim::Real))>,
    /// Pawn_C017's feeds (observe_hover_rage): the SetFuelLevel calls seen (-1: none yet) and the rage callout shown.
    fuel_calls: i64,
    callout: bool,
}

impl Sent {
    fn fresh() -> Sent {
        Sent { fuel_calls: -1, ..Sent::default() }
    }
}

/// A pawn's pop text queue entry (BaseFighter AddPopTextToQueue): the params and the game time its turn ends.
#[derive(Clone, Debug)]
struct Queued {
    p: PopParams,
    until: f64,
}

/// The match's HUD.
#[derive(Debug)]
pub struct Hud {
    res: Res,
    texts: Texts,
    /// The hud bundle's Game target strings (a graph's pop text's key).
    bundle_texts: IndexMap<String, String>,
    setup: HudSetup,
    players: Vec<BrokerPlayer>,
    /// UI_Functions Color for Team, by team index.
    team_colors: [Rgba; 4],
    /// Per HUD pawn: the players by index, then each mode pawn attached after the start, in attach order: its sim
    /// id (None once detached: its widgets are kept unseen, never drawn) and its
    /// broker's player index.
    pawns: Vec<Option<Fid>>,
    slot_of: Vec<usize>,
    huds: Vec<AttachedBottom>,
    tops: Vec<AttachedTop>,
    /// What a pawn attached later is made of: the prototypes (the bottom's with UI_FinnCharge in customContainer
    /// and its root, when the bundle has the class), the preferences at build, context 5 and the players' names.
    bottom: Tree,
    bottom_custom: Option<(Tree, WidgetId)>,
    top: Tree,
    ui: Prefs,
    ctx5: bool,
    /// (the port's own) The Waves run's counter, set each frame by the app in context `MODE_WAVES`.
    waves: Option<WavesHud>,
    names: PlayerNumbers,
    /// A minion's own queue (BPI_PopText on the Watermelon, the pawn's AddPopTextToQueue), by its id.
    minion_queues: IndexMap<Fid, VecDeque<Queued>>,
    /// The widgets made from a minion's queue, with its id: each follows the minion while it lives.
    minion_pops: Vec<(Fid, PopId)>,
    /// Each sim pawn's 'uiState' and 'uiAbility' / 'abilityUiCreate' calls not played yet, in order, by its id
    /// (a list per pawn record from the pawn's creation, gone with it). A
    /// mode pawn's wait for its attach and play at its first observe after it.
    ui_ops: IndexMap<Fid, Vec<UiStateOp>>,
    ability_ops: IndexMap<Fid, Vec<(i32, HudAbilityOp)>>,
    sent: Vec<Sent>,
    facing_left: Vec<bool>,
    scoreboard: Scoreboard,
    pops: PopTexts,
    inputs: InputLabels,
    /// UI_Countdown (not in the Lab).
    countdown: Option<Countdown>,
    /// The countdown card ended (UI_Countdown @5786: BPI_SetCanPause(true) on PlayerControllers 0..3).
    countdown_finished: bool,
    /// UI_VictoryText, from MatchEnded.
    victory: Option<VictoryText>,
    /// The card UI_Game's MatchEnded makes, by the winning team: made with the HUD (the bundle is not kept).
    victories: Vec<(i32, VictoryText)>,
    /// Each fighter's actor location (UE), set by each observe: the pop texts' and labels' attach points.
    actors: Vec<Option<[f64; 3]>>,
    pop_queues: Vec<VecDeque<Queued>>,
    /// Comp_Pawn_DamageManager.DodgePopTexts, per fighter.
    dodge_pops: Vec<Vec<PopId>>,
    /// Buff_AryaBackStab's AllowPop, per dealer: the HUD time of the dealer's last Backstab! text.
    backstab_at: Vec<Option<f64>>,
    /// The Lab's combo texts' toggles (None outside the Lab), set by the match before each observe.
    lab_combos: Option<LabCombos>,
    /// TrainingGameState JugglePopTexts / HitstunPopTexts: one widget per hit fighter, by sim id.
    juggle_pops: HashMap<Fid, PopId>,
    hitstun_pops: HashMap<Fid, PopId>,
    /// AryaKillPie's "Arya's Pie" widgets, by the pie actor's sequence number, while the pie lives.
    pie_pops: IndexMap<i32, PopId>,
    /// Each pawn's pass buff class (Buff_BasketballPass / Buff_IgnitedBasketballPass) at the last observe, by sim id.
    passes: HashMap<Fid, Name>,
    /// The pawns that had Buff_ScoobySnack at the last observe (snack_pops).
    snacks: HashSet<Fid>,
    /// The viewport's AddToViewport(0) count (the pop texts' and labels' paint order).
    order: u64,
    /// The events the widgets posted (PF_Post_Global_Event) since the caller last took them.
    sounds: Vec<String>,
    /// The characters' own parts.
    chars: Vec<CharHud>,
    skillshots: SkillshotTargets,
    dash_targets: DashTargets,
    indicators: ItemIndicators,
    /// The parts the last draw painted, in paint order.
    drawn: Vec<Part>,
}

/// A HUD part a draw painted: a fighter's widgets, the viewport's widgets, a pop text or a label by its place in
/// its list, a character part's root (the part's place in `chars`, the root's in its `roots`), a skill shot target,
/// a dash target or an item indicator by its place.
#[derive(Clone, Copy, Debug)]
enum Part {
    Char(usize, usize),
    Bottom(usize),
    Top(usize),
    Skill(usize),
    Dash(usize),
    Indicator(usize),
    Scoreboard,
    Victory,
    Pop(usize),
    Label(usize),
    Countdown,
}

/// UI_Functions Color for Team, as a palette index: the Lab uses the team index; Free For All gives my team 0 and the
/// others 1 + their place among the other team indexes; two teams give my team 0, the other 1.
fn team_palette(setup: &HudSetup, team: u8) -> usize {
    let mine = setup.players[setup.me].team;
    match setup.mode {
        4 => usize::from(team),
        3 if team == mine => 0,
        3 => (0..4u8).filter(|&t| t != mine).position(|t| t == team).map_or(0, |k| 1 + k),
        _ => usize::from(team != mine),
    }
}

/// UI_Functions Color for Player: online IsMe ? 4 : the local player's team 0, another 1; offline in training or
/// with exactly one human player IsMe ? 4 : team index, otherwise the team index.
fn player_palette(setup: &HudSetup, player: usize) -> usize {
    if setup.online {
        let mine = setup.players[setup.me].team;
        return if player == setup.me { 4 } else { usize::from(setup.players[player].team != mine) };
    }
    let one_human = setup.mode == 4 || setup.players.iter().filter(|p| !p.bot).count() == 1;
    if one_human && player == setup.me { 4 } else { usize::from(setup.players[player].team) }
}

/// The struct a queued feeder fills (BPI_PopText: through the pawn's queue).
fn queued(fighter: usize, text: &str, color: Rgba, lifetime: f64, location: [f64; 3]) -> (PopParams, bool) {
    (PopParams::attached(fighter, text, lifetime, color, location), true)
}

/// UI_Functions ShowMatchWarning: two viewport pop texts, the primary with the stinger and the portrait.
fn warning_pops(
    primary: &str,
    secondary: &str,
    secondary_color: Rgba,
    team_color: Rgba,
    portrait: Option<&str>,
) -> [PopParams; 2] {
    let base = |text: &str, lifetime: f64, color: Rgba, location: [f64; 3], scale: f64, delay: f64| {
        let mut p = PopParams::attached(usize::MAX, text, lifetime, color, location);
        p.scale = scale;
        p.delay = delay;
        p.place = Place::Viewport;
        p
    };
    let big = portrait.is_none();
    let mut first = base(primary, 3.0, [1.0; 4], [0.5, 0.2, 0.0], if big { 2.25 } else { 1.5 }, 0.0);
    first.team_color = Some(team_color);
    first.audio = Some("ui_stinger_matchpoint".to_string());
    first.icon_top = portrait.map(str::to_string);
    let second_at = if big { [0.5, 0.27, 0.0] } else { [0.5, 0.3, 0.0] };
    let second = base(secondary, 3.0 - 0.25, secondary_color, second_at, if big { 1.25 } else { 1.0 }, 0.25);
    [first, second]
}

/// The pixel a world place (UE cm) projects to on a frame of `size` pixels (ProjectWorldToScreen); None behind the
/// camera (the reversed projection's NDC z under 0).
fn project(cam: &Camera, size: [f64; 2], p: [f64; 3]) -> Option<[f64; 2]> {
    let t = [0, 1, 2].map(|i| p[i] - cam.origin[i]);
    let m = &cam.translated_world_to_view;
    let v: [f64; 4] = std::array::from_fn(|j| t[0] * m[0][j] + t[1] * m[1][j] + t[2] * m[2][j] + m[3][j]);
    let c = &cam.view_to_clip;
    let clip: [f64; 4] = std::array::from_fn(|j| (0..4).map(|k| v[k] * c[k][j]).sum());
    if clip[2] / clip[3] < 0.0 {
        return None;
    }
    let (x, y) = (clip[0] / clip[3], clip[1] / clip[3]);
    Some([(x + 1.0) / 2.0 * size[0], (1.0 - y) / 2.0 * size[1]])
}

/// A minion's actor location (UE): its pop texts' attach point.
/// A minion's actor location (UE) by its id: None once gone or dead.
fn minion_at(w: &World, id: Fid) -> Option<[f64; 3]> {
    let f = w.fighter(w.by_id(id)?);
    (f.life.dead == 0.0).then_some([0.0, f64::from(-f.body.x), f64::from(f.body.y)])
}

/// Whether a sim id has a pawn record: every fighter but a minion.
fn has_record(w: &World, id: Fid) -> bool {
    w.by_id(id).is_some_and(|f| !w.fighter(f).life.is_minion)
}

/// A fighter's Comp_Pawn_AbilityManager Abilities as the cooldown widgets read them (each slot's
/// `abilityRow`, the template's or the one a graph wrote over it).
fn abilities_of(w: &World, fid: FighterId, res: &Res) -> Vec<HudAbility> {
    let n = i32::try_from(w.char_data(fid).def.abilities.len()).unwrap_or(0);
    (0..n).filter_map(|k| ability_row(w, fid, k)).map(|r| HudAbility::of(w, &r, |s| res.brush(s))).collect()
}

/// A HUD pawn's entry of a per-pawn list: the place `k` given again, or the next one.
fn put<T>(v: &mut Vec<T>, k: usize, x: T) {
    if k < v.len() {
        v[k] = x;
    } else {
        v.push(x);
    }
}

/// A pawn's AttachedBottom: with its pawn's custom UI for a local human (custom_ui.rs), else the plain one.
#[allow(clippy::too_many_arguments)]
fn bottom_for(
    bottom: &Tree,
    custom: Option<&(Tree, WidgetId)>,
    character: &str,
    b: &BrokerPlayer,
    abilities: &[HudAbility],
    ui: &Prefs,
    ctx5: bool,
    names: &PlayerNumbers,
    res: &Res,
) -> AttachedBottom {
    let c = custom_ui::custom_ui(character).filter(|_| b.is_local && !b.is_bot);
    match (c, custom) {
        (Some(c), Some((t, w))) => AttachedBottom::new(t, Some((c, *w)), b, abilities, ui, ctx5, names, res),
        _ => AttachedBottom::new(bottom, None, b, abilities, ui, ctx5, names, res),
    }
}

impl Hud {
    pub fn new(b: &Bundle, text: &GameText, w: &World, setup: &HudSetup) -> Result<Hud> {
        let n = setup.players.len();
        let bound = crate::menu::prefs::color_rows(&setup.ui);
        let pal = |i: usize| palette_ui(b, i, bound);
        let mine = setup.players[setup.me].team;
        // Local Play (BaseFighter GetLocality): Is Me = IsMe? AND NOT IsLocalPlay (@857), Is Local = IsLocallyControlled
        // OR IsLocalPlay, Is Ally relative to PC0's player data (@728); the focus colour is the team colour (SelectColor
        // @155899); the username is GetLocalPlayUsername(PlayerIndex).Username for every fighter, bots too.
        let lp = setup.local_play;
        let names = PlayerNumbers::new(text);
        let players: Vec<BrokerPlayer> = setup
            .players
            .iter()
            .enumerate()
            .map(|(i, p)| BrokerPlayer {
                is_me: !lp && i == setup.me,
                is_bot: p.bot,
                // (offline: every pawn is locally controlled)
                is_local: true,
                is_ally: p.team == mine,
                team: pal(team_palette(setup, p.team)),
                focus: pal(if lp { team_palette(setup, p.team) } else { player_palette(setup, i) }),
                username: if lp { names.local_play_username(i).username } else { p.username.clone() },
                // BaseFighter @154343: Broker.SetCharacterForPlayer(PlayerIndex, HydraCharacterData)
                portrait: b.portraits.get(p.character.as_deref().unwrap_or("shaggy")).and_then(|m| brush_path(b, m)),
                index: i,
            })
            .collect();
        let ui = Prefs::of(&setup.ui);
        let ctx5 = setup.mode == 5;
        let mut res = Res::new(b);
        res.labels = LabelSources::named(text.key_names().clone());
        let abilities: Vec<Vec<HudAbility>> =
            (0..n).map(|i| w.players.get(i).map_or_else(Vec::new, |&f| abilities_of(w, f, &res))).collect();
        let bottom = proto(b, "UI_IGv3_AttachedBottom", "AttachedBottom");
        let top = proto(b, "UI_IGv3_AttachedTop", "AttachedTop");
        // OnCustomUI: the pawn's widget added to customContainer (an overlay slot: Left / Top, no padding).
        let bottom_custom = b.widgets.contains_key(custom_ui::CLASS).then(|| {
            let mut t = bottom.clone();
            let cc = t.find("customContainer");
            let c = t.instantiate_in(b, cc, custom_ui::CLASS, custom_ui::NAME);
            let x = t.widget_mut(c);
            (x.h_align, x.v_align) = (HAlign::Left, VAlign::Top);
            (t, c)
        });
        let huds = (0..n)
            .map(|i| {
                let character = setup.players[i].character.as_deref().unwrap_or("shaggy");
                let c = bottom_custom.as_ref();
                bottom_for(&bottom, c, character, &players[i], &abilities[i], &ui, ctx5, &names, &res)
            })
            .collect();
        let tops = (0..n).map(|i| AttachedTop::new(&top, &players[i], i)).collect();
        let team_colors = [0, 1, 2, 3].map(|t| pal(team_palette(setup, t)));
        // PFGPlayerController's ReceiveBeginPlay -> CreateUI -> EnableHUD creates UI_Game (HUD time 0); UI_Game
        // Update Team Colors: Broker.SetColorForTeam(0 / 1, Color for Team(i)).
        let sb = ScoreboardSetup {
            mode: setup.mode,
            max_score: setup.max_score,
            team_colors: [team_colors[0], team_colors[1]],
            stocks: setup.stocks,
            names: names.clone(),
            mode_label: MODE_LABEL
                .get(usize::from(setup.mode))
                .copied()
                .flatten()
                .map_or_else(String::new, |k| game_text(text, k)),
            stage_label: setup.arcade.map(|a| arcade_stage(text, MI_ARCADE_STAGE, "{Number}", a)),
        };
        let scoreboard = Scoreboard::new(b, &players, 0.0, &sb, &ui, &abilities);
        // (contexts 1 to 3: the versus levels' InToCountdown; the Lab and the tutorial level make none)
        let stage = setup.arcade.map(|a| arcade_stage(text, CD_ARCADE_STAGE, "{Round}", a));
        let countdown = (1..4).contains(&setup.mode).then(|| {
            Countdown::new(b, text, usize::from(setup.mode - 1), setup.max_score, setup.stocks, stage.as_deref())
        });
        // UI_Game MatchEnded: EnemyWin = not a spectator, not local play, my team is not the winner, and
        // (IsPlayingOnline or Is1v1BotMatch); the port's local custom game takes IsPlayingOnline as true (a match
        // server's client in the build). The card's colour is GetPaletteEntry(1).UI for EnemyWin, else entry 0's.
        // Local play (@6897-7040, @8375): EnemyWin is false and the card is the winner's team colour.
        let mut teams: Vec<u8> = setup.players.iter().map(|p| p.team).collect();
        teams.sort_unstable();
        teams.dedup();
        let victories = teams
            .into_iter()
            .map(|team| {
                let v = if lp {
                    VictoryText::new(b, text, false, pal(team_palette(setup, team)))
                } else {
                    VictoryText::new(b, text, team != mine, pal(usize::from(team != mine)))
                };
                (i32::from(team), v)
            })
            .collect();
        let texts = Texts::new(text);
        let chars = CharHud::for_match(b, setup, &players, &texts);
        Ok(Hud {
            res,
            pawns: (0..n).map(|i| i32::try_from(i).ok()).collect(),
            slot_of: (0..n).collect(),
            bottom,
            bottom_custom,
            top,
            ui,
            ctx5,
            waves: None,
            names,
            minion_queues: IndexMap::new(),
            minion_pops: Vec::new(),
            ui_ops: IndexMap::new(),
            ability_ops: IndexMap::new(),
            texts,
            bundle_texts: b.texts.clone(),
            setup: setup.clone(),
            players,
            team_colors,
            huds,
            tops,
            sent: vec![Sent::fresh(); n],
            facing_left: vec![false; n],
            scoreboard,
            pops: PopTexts::new(proto(b, "UI_PopText", "PopText")),
            inputs: InputLabels::new(proto(b, "UI_PopInputLabel", "InputUI")),
            countdown,
            // (the port's own Waves context has no countdown card: its controllers may pause from the start)
            countdown_finished: setup.mode == MODE_WAVES,
            victory: None,
            victories,
            actors: vec![None; n],
            pop_queues: vec![VecDeque::new(); n],
            dodge_pops: vec![Vec::new(); n],
            backstab_at: vec![None; n],
            lab_combos: None,
            juggle_pops: HashMap::new(),
            hitstun_pops: HashMap::new(),
            pie_pops: IndexMap::new(),
            passes: HashMap::new(),
            snacks: HashSet::new(),
            order: 0,
            sounds: Vec::new(),
            chars,
            skillshots: SkillshotTargets::new(b),
            dash_targets: DashTargets::new(b),
            indicators: ItemIndicators::new(b, pal(0)),
            drawn: Vec::new(),
        })
    }

    /// The versus card (UI_Countdown StartCountdown) from the match's first drawn frame, at real time `real`.
    pub fn start_card(&mut self, real: f64) {
        if let Some(c) = &mut self.countdown {
            c.begin(real);
        }
    }

    /// Whether the countdown card has ended (hud.onCountdownFinished): the match's controllers may pause from then.
    pub fn countdown_finished(&self) -> bool {
        self.countdown_finished
    }

    /// The events the HUD's widgets posted (PF_Post_Global_Event) by observe and draw since the last call, taken;
    /// nothing plays them yet.
    pub fn take_sounds(&mut self) -> Vec<String> {
        std::mem::take(&mut self.sounds)
    }

    /// (the port's own) A fighter's scoreboard portrait as last painted, in device pixels of the HUD canvas
    /// (x, y, w, h): the match chat's bubbles stand at it. None: no slot, or none painted yet.
    pub fn slot_rect(&self, player: usize) -> Option<[f64; 4]> {
        self.scoreboard.slot_rect(player)
    }

    /// UI_Broker SetPauseHoldForPlayer(controller id, fraction) (PFGPlayerController ReceiveTick @166883): the
    /// scoreboard's UI_IGv3_PauseHold keeps the value of its own player index (0). -> whether the drawn value changed.
    pub fn pause_hold(&mut self, player: i32, value: f64) -> bool {
        self.scoreboard.pause_hold(player, value)
    }

    /// (the port's own) The Waves run's counter for the frames that follow (context `MODE_WAVES`).
    pub fn set_waves(&mut self, w: Option<WavesHud>) {
        self.waves = w;
    }

    /// What the input labels read of the players' input: `menu`, the bundle with
    /// InputButtonIconsTable (the menu's table and its textures are the HUD's data); `players`, each
    /// player index's input. Until it is given, every label is the keyboard form with the default keys.
    pub fn set_label_input(&mut self, menu: &Bundle, players: Vec<LabelInput>) {
        let icons = menu.input_icons.keys().filter_map(|k| Some((k.clone(), menu.input_icon(k)?.to_owned()))).collect();
        let names = self.res.labels.names().clone();
        self.res.labels = LabelSources::new(icons, players, names);
    }

    /// A player index's GetLastInteractionType (0 gamepad, 1 keyboard, 2 mouse) from now on: a label whose type
    /// changed redraws at its next tick (UI_InputLabel Refresh).
    pub fn set_input_type(&mut self, player: usize, kind: u8) {
        self.res.labels.set_kind(player, kind);
    }

    /// A mode pawn's PlayerDataReadyServer (the sim's slotBound): the pawn's two attached widgets, made on its slot's
    /// broker (UI_IGv3_AttachedBottom / Top with that player index) and added to the screen layer as a new pawn's
    /// components are (bottom then top, after the slots there are). A pawn bound again takes the new slot.
    /// Many pawns may share a slot (the Waves enemies). A new pawn takes the first place a detached one left, so the
    /// HUD's per-pawn lists hold no more places than pawns attached at once.
    pub fn attach(&mut self, w: &World, pawn: Fid, slot: usize) {
        let (Some(b), Some(fid)) = (self.players.get(slot).cloned(), w.by_id(pawn)) else { return };
        let n = self.players.len();
        let waves = self.setup.mode == MODE_WAVES;
        // (a Waves pawn the HUD already attached to that slot, by the sweep of `observe`, keeps its place)
        if self.k_of(pawn).is_some_and(|k| k < n || waves && self.slot_of[k] == slot) {
            return;
        }
        self.detach(pawn);
        let abilities = abilities_of(w, fid, &self.res);
        let character = w.names.str(w.fighter(fid).ch);
        let c = self.bottom_custom.as_ref();
        // (the port's own) In a Waves run the local player's fighter is a mode pawn: it takes its slot's own place,
        // so what reads a player index (the character parts' labels, the attached pop texts) finds it there.
        let k = if waves && slot == self.setup.me {
            // (its scoreboard slot was made with no abilities: a mode World has no players)
            self.scoreboard.set_abilities(slot, &b, &abilities, &self.ui);
            slot
        } else {
            (n..self.pawns.len()).find(|&k| self.pawns[k].is_none()).unwrap_or(self.pawns.len())
        };
        let (ui, ctx5) = (&self.ui, self.ctx5);
        let hud = bottom_for(&self.bottom, c, character, &b, &abilities, ui, ctx5, &self.names, &self.res);
        put(&mut self.huds, k, hud);
        put(&mut self.tops, k, AttachedTop::new(&self.top, &b, k));
        put(&mut self.pawns, k, Some(pawn));
        put(&mut self.slot_of, k, slot);
        put(&mut self.sent, k, Sent::fresh());
        put(&mut self.facing_left, k, false);
        put(&mut self.actors, k, None);
        put(&mut self.pop_queues, k, VecDeque::new());
        put(&mut self.dodge_pops, k, Vec::new());
        put(&mut self.backstab_at, k, None);
    }

    /// The pawn is destroyed (by `observe`, for a mode pawn the
    /// World no longer holds): its widget components go with it. A label it holds (the rage callout) stays, where
    /// it was last placed, as its pop texts do; what the HUD kept of it by its id goes.
    pub fn detach(&mut self, pawn: Fid) {
        let Some(k) = self.k_of(pawn).filter(|&k| k >= self.players.len()) else { return };
        self.pawns[k] = None;
        self.pop_queues[k].clear();
        self.dodge_pops[k].clear();
        self.actors[k] = None;
        // (its place is free for the next pawn attached: the widgets still on it let go of it)
        self.pops.release(k);
        self.inputs.release(k);
        self.snacks.remove(&pawn);
        self.passes.remove(&pawn);
        self.juggle_pops.remove(&pawn);
        self.hitstun_pops.remove(&pawn);
    }

    /// The sim ids of the HUD's pawns, in the order of HudFrame's anchors (None: a detached pawn's place).
    pub fn pawns(&self) -> &[Option<Fid>] {
        &self.pawns
    }

    /// The HUD pawn of a sim fighter id (None: a minion or an unbound pawn).
    fn k_of(&self, fighter: Fid) -> Option<usize> {
        self.pawns.iter().position(|&p| p == Some(fighter))
    }

    /// The broker's player data of a HUD pawn's slot (UI_Functions GetUIBrokerForFighter, GetDataForPlayer).
    fn broker(&self, k: usize) -> Option<&BrokerPlayer> {
        self.slot_of.get(k).and_then(|&s| self.players.get(s))
    }

    /// The player index of a sim fighter id (None: a minion or a mode pawn).
    fn player(&self, fighter: i32) -> Option<usize> {
        usize::try_from(fighter).ok().filter(|&i| i < self.players.len())
    }

    /// The player index a fighter's item labels go by: `player`'s; (the port's own) in a Waves run, where the
    /// player's fighter is a mode pawn too, an attached pawn's slot.
    fn item_player(&self, fighter: i32) -> Option<usize> {
        if self.setup.mode == MODE_WAVES {
            return self.k_of(fighter).map(|k| self.slot_of[k]);
        }
        self.player(fighter)
    }

    /// UI_Functions PopText with a level actor as the attach actor (a stage block's
    /// SceneryHost.pop): shown at once at `now`, attached to its fighter or to the level actor's location.
    pub fn scenery_pop(&mut self, s: mvs_render::scenery::PopSpec, now: f64) {
        let mut p = PopParams::attached(s.fighter.unwrap_or(0), &s.text, s.lifetime, s.text_color, s.location);
        p.scale = s.scale;
        p.justification = s.justification;
        p.delay = s.delay;
        p.icon_right = s.icon_right;
        if let Some(at) = s.at {
            p.place = Place::Actor(at);
        }
        self.pops.add(p, now, &mut self.order);
    }

    /// UI_Functions PopText of the tutorial's widgets and actor: shown at once at
    /// `now`, in the viewport (no attach pawn) or on its pawn; a pawn the HUD does not hold shows nothing.
    pub fn tutorial_pop(&mut self, p: &crate::menu::tutorial::TutorialPopText, now: f64) {
        let k = match p.attach {
            Some(f) => match self.k_of(f) {
                Some(k) => k,
                None => return,
            },
            None => usize::MAX,
        };
        let mut q = PopParams::attached(k, &p.text, p.lifetime, p.color, p.location);
        q.scale = p.scale;
        q.justification = p.justification;
        if p.attach.is_none() {
            q.place = Place::Viewport;
        }
        self.pops.add(q, now, &mut self.order);
    }

    /// After each World step: the step's events and the fighters' state at the HUD's game time and the real time.
    pub fn observe(&mut self, w: &World, events: &[Event], hud_time: f64, real: f64) {
        let now = hud_time;
        let n = self.players.len();
        // (a mode pawn the World no longer holds is detached; so is one whose id another fighter took since, as the
        // pawns' counter wraps over a long Waves run: a fighter not bound to the pawn's slot)
        for k in n..self.pawns.len() {
            let Some(id) = self.pawns[k] else { continue };
            let slot = i32::try_from(self.slot_of[k]).ok();
            if w.by_id(id).is_none_or(|f| w.fighter(f).life.is_minion || w.fighter(f).life.slot != slot) {
                self.detach(id);
            }
        }
        // (a pawn record's op lists go with the pawn)
        self.ui_ops.retain(|&id, _| w.by_id(id).is_some());
        self.ability_ops.retain(|&id, _| w.by_id(id).is_some());
        // (the port's own) A Waves run binds its player's pawn while the World is made, before the app's first step
        // clears the events: every bound pawn with no place yet is attached, ahead of this step's events.
        if self.setup.mode == MODE_WAVES {
            for f in w.all_ids() {
                let x = w.fighter(f);
                let Some(slot) = x.life.slot.and_then(|s| usize::try_from(s).ok()) else { continue };
                if !x.life.is_minion && self.k_of(x.id).is_none() {
                    self.attach(w, x.id, slot);
                }
            }
        }
        for e in events {
            self.event(w, e, now, real);
        }
        // BaseFighter AddPopTextToQueue / ApplyPopText: one text at a time per pawn, the next after the shown one's
        // lifetime.
        for q in &mut self.pop_queues {
            if q.front().is_none_or(|x| now < x.until) {
                continue;
            }
            q.pop_front();
            if let Some(x) = q.front_mut() {
                x.until = now + x.p.lifetime;
                self.pops.add(x.p.clone(), now, &mut self.order);
            }
        }
        // (a minion's: only while one has a text queued; a gone minion's queue goes with it)
        let pops = &mut self.pops;
        let order = &mut self.order;
        let made = &mut self.minion_pops;
        self.minion_queues.retain(|&id, q| {
            let Some(at) = minion_at(w, id) else { return false };
            if q.front().is_none_or(|x| now < x.until) {
                return true;
            }
            q.pop_front();
            let Some(x) = q.front_mut() else { return false };
            x.until = now + x.p.lifetime;
            x.p.place = Place::Actor(at);
            made.push((id, pops.add(x.p.clone(), now, order)));
            true
        });
        // (every widget made so follows its minion each frame; without one it keeps its last place)
        self.minion_pops.retain(|&(id, p)| {
            if !pops.available(p) {
                return false;
            }
            if let Some(at) = minion_at(w, id) {
                pops.set_actor(p, at);
            } else {
                pops.hold(p);
            }
            true
        });
        // (the pawns attached by this step's events too: the op lists are the pawn record's; in a Waves run the
        // player's place holds its mode pawn, `attach`)
        let waves = self.setup.mode == MODE_WAVES;
        for k in 0..self.pawns.len() {
            let Some(id) = self.pawns[k] else { continue };
            let fid = if k < n && !waves { w.players.get(k).copied() } else { w.by_id(id) };
            let Some(fid) = fid else { continue };
            let f = w.fighter(fid);
            self.actors[k] = Some([0.0, f64::from(-f.body.x), f64::from(f.body.y)]);
            let slot = self.slot_of[k];
            let s = &mut self.sent[k];
            let h = &mut self.huds[k];
            let top = &mut self.tops[k];
            let dead = f.life.dead != 0.0;
            let respawned = s.dead && !dead;
            if dead != s.dead {
                s.dead = dead;
                h.set_visible(!dead);
                top.set_visible(!dead);
                // AbductPlayer's defeat only hides the UI
                if dead && !f.life.abducted {
                    h.set_percentage(-1, now, real);
                    self.scoreboard.set_percentage(slot, -1, now, real);
                }
            }
            // Comp_Armor UpdateTempHealthVisuals (ubergraph @2850): SetGreyPercentageForPlayer(FTrunc(the total of
            // CaculateTempHealth: HealthAmount + total, in the packs' order)).
            // Deviation: read once a tick, so a total that leaves and comes back inside one tick plays no animation.
            let mut grey = 0.0;
            for pk in &f.stats.temp_health {
                grey += pk.amount;
            }
            let grey = grey.trunc() as i32;
            if grey != s.grey {
                s.grey = grey;
                h.set_grey_percentage(grey, real);
                self.scoreboard.set_grey_percentage(slot, grey, real);
            }
            let damage = f.stats.damage;
            if !dead && (f64::from(damage) != s.damage || respawned) {
                h.set_percentage(damage.trunc() as i32, now, real);
                self.scoreboard.set_percentage(slot, damage.trunc() as i32, now, real);
            }
            s.damage = f64::from(damage);
            // SetChargeAttackForPlayer, its Start Time moved onto the HUD's game clock.
            // Sent on every sim write of chargeUi, an equal value included (the write count), as the build does: Iron
            // Giant's resent (0, 0) hides the fuel ring until the next component tick (observe_hover_rage). The value
            // is compared too: a rollback restores it but not the count (not state).
            let charge = f.charge.charge_ui;
            let sent = Some((f.charge.charge_ui_writes, charge));
            if s.charge != sent {
                s.charge = sent;
                h.set_charge_attack(
                    now - f64::from(f.clock.time - charge.0),
                    f64::from(charge.1),
                    charge.1 == 0.0,
                    -1.0,
                );
            }
            self.facing_left[k] = f.body.facing_dir < 0.0;
            for (slot_n, op) in self.ability_ops.get_mut(&id).map(std::mem::take).unwrap_or_default() {
                let row = matches!(op, HudAbilityOp::Create { .. })
                    .then(|| ability_row(w, fid, slot_n))
                    .flatten()
                    .map(|r| HudAbility::of(w, &r, |x| self.res.brush(x)));
                let h = &mut self.huds[k];
                h.ability_op(slot_n, &op, row.as_ref(), now, real, &mut self.sounds);
                self.scoreboard.ability_op(slot, slot_n, &op, row.as_ref(), now, real, &mut self.sounds);
            }
            self.observe_hover_rage(w, fid, k, dead, real);
            // Pawn_WonderWomanV3 MulticastMeterValue @3361-3388: MeterUI.UpdateRunChargeProgressBar(MeterPercentage),
            // the custom UI's bar (a local human's pawn only).
            if let Some(ww) = mvs_sim::chars::wonder_woman::state::wonder_woman_of(w, fid) {
                self.huds[k].custom_value(f64::from(ww.meter), real);
            }
            // A pawn's own feed of its custom UI, entered with the widget (custom_ui.rs).
            self.huds[k].observe_custom(w, fid, real, &self.res);
            let top = &mut self.tops[k];
            // A respawned pawn has fresh widgets: its states start collapsed.
            if respawned {
                top.reset(&self.res.labels);
            }
            for op in &self.ui_ops.get_mut(&id).map(std::mem::take).unwrap_or_default() {
                match op {
                    UiStateOp::Add(p) => {
                        let p = StateParams {
                            id: w.names.str(p.id).to_string(),
                            state_type: p.state_type,
                            display_type: p.display_type,
                            current_value: f64::from(p.current_value),
                            max_value: f64::from(p.max_value),
                            icon_material: w.names.str(p.icon_material).to_string(),
                            active: p.active,
                            pop_text: w.names.str(p.pop_text).to_string(),
                            auto_remove_time: f64::from(p.auto_remove_time),
                            subicon: p.subicon.map(|x| w.names.str(x).to_string()),
                        };
                        top.add_state(&self.res, p, now, real, &mut self.pops, &mut self.order);
                    }
                    UiStateOp::Remove(id) => top.remove_state(w.names.str(*id)),
                }
            }
            let evade = f.evade.evade_count;
            let s = &mut self.sent[k];
            if f64::from(evade) != s.evade {
                s.evade = f64::from(evade);
                self.huds[k].set_evade_decay(f64::from(1.0 - evade / MAX_EVADE_COUNT), now);
                self.scoreboard.set_evade_decay(slot, f64::from(1.0 - evade / MAX_EVADE_COUNT), now);
            }
        }
        self.dash_targets.observe(w, &self.players, &self.setup);
        self.indicators.observe(w, &self.players, common::PlayerMap::of(&self.setup), now);
        for c in &mut self.chars {
            c.observe(w, now, real, &mut self.inputs, &mut self.pops, &self.texts.jumps_reset, &mut self.order);
        }
        // The render modules' pop texts (after the HUD's observe, shown from the next tick): the
        // modules' actors, then their pawn effects.
        self.pie_pops(w, now + STEP);
        self.pass_pops(w, now + STEP);
        self.snack_pops(w, now + STEP);
    }

    /// Buff_ScoobySnack's BPI After Status Update Multicast with BuffApplied (@1304 -> SpawnVFX @98, not on a
    /// dedicated server): UI_Functions PopText @225-733 called directly, the Enraged text, lifetime 1.0, text_color
    /// (1, 0.18552, 0.155671, 1), location (0, 0, 225), space 3, scale 1, centred, no delay, icons or audio, attached
    /// to the owner. The sim has no event of the buff's apply: seen as the
    /// buff (with frames left) on a pawn that had none at the last observe.
    // Deviation: shown wherever the buff is applied (Arya's own on a kill), as the graph does, not only in a match that
    // holds the snack's particle systems (a fighter whose kit gives the buff has a team mate).
    fn snack_pops(&mut self, w: &World, now: f64) {
        const SNACK: &str = "Buff_ScoobySnack_C";
        for k in 0..self.pawns.len() {
            let Some(id) = self.pawns[k] else { continue };
            let has = w.by_id(id).is_some_and(|fid| {
                w.fighter(fid).buffs.buffs.iter().any(|b| b.frames > 0.0 && w.names.str(b.cls) == SNACK)
            });
            let had = if has { !self.snacks.insert(id) } else { !self.snacks.remove(&id) };
            if has && !had {
                let p =
                    PopParams::attached(k, &self.texts.enraged, 1.0, [1.0, 0.18552, 0.155671, 1.0], [0.0, 0.0, 225.0]);
                self.pops.add(p, now, &mut self.order);
            }
        }
    }

    /// AryaKillPie's VisualIndicator (BeginPlay @1031 -> @1679): UI_Functions.PopText @2156-2659 on every machine (no
    /// local test), "Arya's Pie", lifetime 1.25, text_color (1, 0.18552, 0.155671, 1), location (0, 0, 150), space 3,
    /// scale 1, centred, no delay, icons or audio, attach actor the pie (pieView). Made at the first sight of a live
    /// pie; the widget follows the pie's actor location and keeps its last place once the pie is gone.
    fn pie_pops(&mut self, w: &World, now: f64) {
        if self.pie_pops.is_empty() && w.actors.actors.iter().next().is_none() {
            return;
        }
        let mut seen = Vec::new();
        for (_, a) in w.actors.actors.iter() {
            let WorldActorState::KillPie(d) = &a.state else { continue };
            if a.dead || w.names.str(a.cls) != "AryaKillPie_C" {
                continue;
            }
            seen.push(a.seq);
            let at = [0.0, f64::from(-d.x), f64::from(d.y)];
            if let Some(&id) = self.pie_pops.get(&a.seq) {
                self.pops.set_actor(id, at);
                continue;
            }
            let k = self.k_of(a.owner_id).unwrap_or(usize::MAX);
            let mut p =
                PopParams::attached(k, &self.texts.arya_pie, 1.25, [1.0, 0.18552, 0.155671, 1.0], [0.0, 0.0, 150.0]);
            p.place = Place::Actor(at);
            let id = self.pops.add(p, now, &mut self.order);
            self.pie_pops.insert(a.seq, id);
        }
        // K2_DestroyActor: the widget's attach actor is gone.
        let pops = &mut self.pops;
        self.pie_pops.retain(|seq, id| {
            let keep = seen.contains(seq);
            if !keep {
                pops.hold(*id);
            }
            keep
        });
    }

    /// c016 Buff_BasketballPass / Buff_IgnitedBasketballPass (on the team mate who caught LeBron's ball): BPI After
    /// Status Update Multicast @2919 BuffApplied -> SpawnVFX @2934: an owner whose Instigator (the pawn itself)
    /// IsLocallyControlled @84-166 gets SpawnPopupText @20-534: UI_Functions PopText "Pass Received", lifetime 1,
    /// text_color (0.183437, 1, 0.593957, 1), location (0, 0, 250), space 3, scale 1, centred, no delay, icons or
    /// audio, attached to the owner. The sim has no event of the buff's
    /// status update: seen as a pass class on a pawn that had none (or the other one) at the last observe.
    fn pass_pops(&mut self, w: &World, now: f64) {
        const PASS: [&str; 2] = ["Buff_BasketballPass_C", "Buff_IgnitedBasketballPass_C"];
        for k in 0..self.pawns.len() {
            let Some(id) = self.pawns[k] else { continue };
            let Some(fid) = w.by_id(id) else { continue };
            let f = w.fighter(fid);
            let pass = if f.life.dead != 0.0 {
                None
            } else {
                f.buffs.buffs.iter().find(|b| b.frames > 0.0 && PASS.contains(&w.names.str(b.cls))).map(|b| b.cls)
            };
            let last = match pass {
                Some(c) => self.passes.insert(id, c),
                None => self.passes.remove(&id),
            };
            if pass.is_some() && pass != last && self.broker(k).is_some_and(|b| b.is_local) {
                let c = [0.183437, 1.0, 0.593957, 1.0];
                let p = PopParams::attached(k, &self.texts.pass_received, 1.0, c, [0.0, 0.0, 250.0]);
                self.pops.add(p, now, &mut self.order);
            }
        }
    }

    /// Comp_C017_Hover_Rage and Pawn_C017 (Iron Giant) towards the UI, read off the fighter after each step.
    ///
    /// Fuel: SetFuelLevel(FuelAmount / FuelMaxAmount) @4716 -> Broker.SetChargeAttackForPlayer(player, 0, 0, false,
    /// fraction) @4939, at the end of every component tick (ReceiveTick @2651, 0.1 s), from UseFuel @5124 and
    /// ResetFuelLevel @5005: UI_IGv3_Charge shows the fraction as a specific value while it is above 0 and below 1.
    /// A charged attack's own call (time based) lasts until the next component tick writes over it: the build's
    /// graph, kept. HoverRage.fuel_ui counts the SetFuelLevel calls and fuel_ui_value is the last one's argument: a
    /// tick with any call sends the last, whether or not the value changed. One send for several calls of a tick:
    /// OnChargeAttack @1885-2190 only stores its arguments and Tick @2191 reads them. The first observe sends the
    /// count's start as a call (the value 0 before any call: not shown).
    // Unverified: the component's place among the owner's ticks is not read, so neither is the order against a charge
    // notify's call inside a frame (the sim's: the montage's notifies, then the component tick).
    ///
    /// Rage: the custom UI's bar (a local human's pawn only). Rage callout, on a locally controlled pawn's tick
    /// (@12585 -> @3940): while RageCalloutGate (set with IsMaxRage by MaxRage @13997; cleared entering weapon mode
    /// @7609 / @8423, at the reset @21851 and at the defeat @22347, the places the sim clears is_max_rage) a walking
    /// pawn without the label gets RageCallout(true), one that left the ground loses it (RageCallout(false)); weapon
    /// mode @8360 and the defeat @22321 call RageCallout(false) themselves. RageCallout(true) @22650: PopInputLabel
    /// with Action 1, no modifier, Label "Protect Friends!", TextFontInfo Display 1 16, KeyFontInfo
    /// NormsCond-ExtraBoldItalic 16, icon 32 x 32, the struct's default Tint; positioning Lifetime 0, Location (0, 0,
    /// 550), centred, space 3, attached to the pawn. The label's PlayerIndex is the pawn's slot's (@23012-23198).
    /// UI_Functions.PopInputLabel @213-372 makes no widget for a pawn that is bot controlled.
    fn observe_hover_rage(&mut self, w: &World, fid: FighterId, k: usize, dead: bool, real: f64) {
        let Some(g) = mvs_sim::chars::iron_giant::state::giant_of(w, fid) else { return };
        let Some(hr) = &g.hover_rage else { return };
        let s = &mut self.sent[k];
        if i64::from(hr.fuel_ui) != s.fuel_calls {
            s.fuel_calls = i64::from(hr.fuel_ui);
            self.huds[k].set_charge_attack(0.0, 0.0, false, f64::from(hr.fuel_ui_value));
        }
        self.huds[k].custom_value(f64::from(hr.rage_fraction()), real);
        if !self.broker(k).is_some_and(|b| b.is_local && !b.is_bot) {
            return;
        }
        // (the label's key and PlayerIndex are the pawn's player index: its id, or its slot's in a Waves run)
        let pawn = common::PlayerMap::of(&self.setup).index(w, fid).unwrap_or(usize::MAX);
        let want = g.is_max_rage && !dead && mvs_sim::fighter::grounded(w, fid);
        let s = &mut self.sent[k];
        if want == s.callout {
            return;
        }
        s.callout = want;
        if want {
            let p = LabelParams {
                action: 1,
                location: [0.0, 0.0, 550.0],
                label: self.texts.protect_friends.clone(),
                text_size: 16.0,
                attach: Some(k),
                player: pawn,
                icon_size: 32.0,
                custom_icon: None,
                modifier: String::new(),
            };
            self.inputs.show_for(fighter_key(pawn), p, real, &mut self.order, &self.res.labels);
        } else {
            self.inputs.hide_for(fighter_key(pawn));
        }
    }

    /// The HUD's cases of a World event.
    fn event(&mut self, w: &World, e: &Event, now: f64, real: f64) {
        match e {
            // UI_Broker AddStateForPlayer / RemoveStateForPlayer, by pawn, in their order.
            Event::UiState { fighter, op } if has_record(w, *fighter) => {
                self.ui_ops.entry(*fighter).or_default().push(op.clone());
            }
            // UI_Broker OnStruggleForPlayer: through a Client RPC of the pawn's HitstunHandler, run where the pawn's
            // owner is: offline on this machine for every pawn (a mode pawn's through its slot's broker).
            Event::StruggleUi { fighter, time_left, total, hide_input } => {
                if let Some(k) = self.k_of(*fighter).filter(|&k| self.broker(k).is_some_and(|b| b.is_local)) {
                    self.tops[k].on_struggle(
                        f64::from(*time_left),
                        f64::from(*total),
                        2,
                        *hide_input,
                        &self.res.labels,
                    );
                }
            }
            Event::ItemUiCreate { item, fighter, action, on_ground, actor } => {
                if !*actor && self.item_ui_for(Some(*fighter), 0, *action) {
                    let i = self.item_player(*fighter).unwrap_or(0);
                    let src = &self.res.labels;
                    // Deviation: the item manager's tick makes a reachable item's label with action 0 (Attack) where
                    // its owner is local (standalone: the Lab, Local Play); the port's labels always read the player's
                    // pickup action, as the build's do online (where only SetItemUiVisibility's action 10 is made).
                    let a = pickup_item_action(i, src);
                    self.inputs.create(i64::from(*item), a, *on_ground, real, &mut self.order, src);
                }
            }
            Event::ItemUi { item, fighter, hide, on_ground, actor } => {
                if !*actor && self.item_ui_for(Some(*fighter), 1, 0) {
                    let i = self.item_player(*fighter).unwrap_or(0);
                    let key = i64::from(*item);
                    let src = &self.res.labels;
                    self.inputs.create(key, pickup_item_action(i, src), *on_ground, real, &mut self.order, src);
                    if !*hide {
                        self.inputs.show_item_for(key, pickup_item_action(i, src), i, src);
                    }
                    self.inputs.hide(key, *hide, real);
                }
            }
            Event::ItemUiExit { item, fighter, actor } => {
                if !*actor && self.item_ui_for(*fighter, 2, 0) {
                    self.inputs.exit(i64::from(*item), real);
                }
            }
            Event::TeamScore { team, score, is_leader, tied } => {
                // (the stock rule has no score to show: the lives are the Stocks event's)
                // Unverified: Free For All's leader texts (BPI_SetIsLeadingFFA, the New Leader warning) are not
                // ported.
                if !self.setup.stocks {
                    let team = usize::try_from(*team).unwrap_or(usize::MAX);
                    self.scoreboard.score(team, *score as i32, *is_leader, *tied, real);
                }
            }
            Event::Stocks { fighter, stocks } => {
                if let Some(i) = self.player(*fighter) {
                    self.scoreboard.stocks(i, i64::from(*stocks));
                }
            }
            // PandaGameState DisplayMatchPointClient (@6611-7255): ShowMatchWarning(MATCH POINT!, the advantage
            // text, Color for Team(the scoring team), (0, 0, 0, 0)): offline the team's name.
            Event::MatchPointShow { team } => {
                let adv = if *team == 0 { &self.texts.blue_advantage } else { &self.texts.red_advantage };
                let color = self.team_colors[usize::try_from(*team).unwrap_or(0).min(3)];
                for p in warning_pops(&self.texts.match_point, adv, color, [0.0; 4], None) {
                    self.pops.add(p, now, &mut self.order);
                }
            }
            // UI_Game MatchEnded(WinningTeamIndex): the victory card, and the scoreboard leaves.
            Event::MatchEnd { phase: EndPhase::GameEnd, team } => {
                if let Some(k) = self.victories.iter().position(|v| v.0 == *team) {
                    self.victory = Some(self.victories.swap_remove(k).1);
                }
                self.scoreboard.hide(real);
            }
            Event::PopText { fighter, kind, stacks, gain, params } => {
                self.pop_text(w, *fighter, *kind, f64::from(*stacks), *gain, params.as_ref(), now);
            }
            // Buff_Enlarge SpawnVFX @99-664: UI_Functions PopText called directly, attached to the owner.
            Event::EnlargeFx { fighter, begin: true } => {
                if let Some(k) = self.k_of(*fighter) {
                    let p = PopParams::attached(
                        k,
                        &self.texts.morphized,
                        2.0,
                        [0.1875, 1.0, 0.404397, 1.0],
                        [0.0, 0.0, 300.0],
                    );
                    self.pops.add(p, now, &mut self.order);
                }
            }
            Event::SlotBound { fighter, slot } => {
                if let Ok(slot) = usize::try_from(*slot) {
                    self.attach(w, *fighter, slot);
                }
            }
            Event::UiAbility { fighter, n, op } if has_record(w, *fighter) => {
                self.ability_ops.entry(*fighter).or_default().push((*n, HudAbilityOp::Op(*op)));
            }
            // Comp_Pawn_AbilityManager CreateAbilityUI (@10491-10657), in the order of the slot's other UI calls.
            Event::AbilityUiCreate { fighter, slot, icon, max_count } if has_record(w, *fighter) => {
                let icon = if *icon == Name::NONE { None } else { self.res.brush(w.names.str(*icon)) };
                let op = HudAbilityOp::Create { icon: icon.unwrap_or_default(), max_count: *max_count };
                self.ability_ops.entry(*fighter).or_default().push((*slot, op));
            }
            Event::Backstab { fighter, victim } => self.backstab(*fighter, *victim, now),
            // TrainGameMode.OnPlayerJuggled (@64-341): the Lab only, a juggler that is no bot's (a pawn with a player
            // controller).
            Event::ComboCounts { fighter, juggler, juggle_count, juggle_damage, hitstun_count, hitstun_damage } => {
                let bot = usize::try_from(*juggler).ok().and_then(|j| self.setup.players.get(j)).is_some_and(|p| p.bot);
                if !bot {
                    #[allow(clippy::cast_possible_truncation)]
                    let counts = [
                        (i64::from(*juggle_count), *juggle_damage as i64),
                        (i64::from(*hitstun_count), *hitstun_damage as i64),
                    ];
                    self.combo_counts(*fighter, counts, now);
                }
            }
            // Pawn_C017 binds Comp_Pawn_AbilityManager.AbilityAmmoUpdated (Index, CurrentAmmo, Increase); its handler
            // (ubergraph @26889): Index 0 and Increase -> @24324..24869 PopTextMulticast -> @24257 UI_Functions
            // PopText(params, self) on every client: "+1 Bolt", lifetime
            // 0.5, text_color (1, 0.726714, 0, 1), location (0, 0, 500), space 3 on the pawn, scale 1.25, centred,
            // no delay or icons, audio sfx_cha_finn_backpack_treasure_collect (posted by the widget's Show). No other
            // pawn binds it; the other slots and a loss show nothing.
            Event::AbilityAmmo { fighter, slot: 0, increase: true, .. }
                if w.by_id(*fighter).is_some_and(|f| w.names.str(w.fighter(f).ch) == "irongiant") =>
            {
                // (a pawn the HUD has no record of: its widget has no place, its Show still posts)
                let k = self.k_of(*fighter).unwrap_or(usize::MAX);
                let mut p = PopParams::attached(k, &self.texts.bolt, 0.5, [1.0, 0.726714, 0.0, 1.0], [0.0, 0.0, 500.0]);
                p.scale = 1.25;
                p.audio = Some("sfx_cha_finn_backpack_treasure_collect".to_string());
                self.pops.add(p, now, &mut self.order);
            }
            _ => {}
        }
    }

    /// Whether this machine shows an item label call: kind 0 create, 1 show / hide, 2 exit.
    /// The Lab is standalone: every pawn's calls but a show, the local player's show; Local Play the same with each
    /// human's show; a custom game is joined on a dedicated server: the local fighter's SetItemUiVisibility alone
    /// (it makes the label too, action 10).
    fn item_ui_for(&self, fighter: Option<i32>, kind: u8, action: u8) -> bool {
        let Some(f) = fighter else { return true };
        let me = i32::try_from(self.setup.me).unwrap_or(-1);
        if self.setup.mode == 4 {
            kind != 1 || f == me
        } else if self.setup.local_play {
            kind != 1 || self.player(f).is_some_and(|i| !self.players[i].is_bot)
        } else {
            // (a Waves run: as a custom game, the player's pawn by its slot)
            self.item_player(f) == Some(self.setup.me) && (kind != 0 || action == 10)
        }
    }

    /// The pawn's queue (BaseFighter AddPopTextToQueue), for a locally controlled pawn.
    fn queue_pop(&mut self, p: PopParams, now: f64) {
        let Some(q) = self.pop_queues.get_mut(p.fighter) else { return };
        q.push_back(Queued { until: now + p.lifetime, p: p.clone() });
        if q.len() == 1 {
            self.pops.add(p, now, &mut self.order);
        }
    }

    /// A minion's own queue (BPI_PopText on the Watermelon, the pawn's AddPopTextToQueue), by its id, the widget
    /// attached to its actor location. BPI_PopText's HasAuthority OR IsLocallyControlled: every pawn offline; online
    /// the minion is the server's AI, neither on a client. Its queue goes with the pawn (observe).
    fn queue_minion_pop(&mut self, id: Fid, mut p: PopParams, at: [f64; 3], now: f64) {
        if !self.setup.local_play && self.setup.online {
            return;
        }
        p.place = Place::Actor(at);
        let q = self.minion_queues.entry(id).or_default();
        q.push_back(Queued { until: now + p.lifetime, p: p.clone() });
        if q.len() == 1 {
            let shown = self.pops.add(p, now, &mut self.order);
            self.minion_pops.push((id, shown));
        }
    }

    /// UI_Functions PopText with the feeders' UI_PopTextParams (FighterHud.popText). Of a minion's texts (a pawn
    /// without player data) only the queued ones (BPI_PopText) are shown: the direct feeders return at
    /// GetUIBrokerForFighter's Success, which a pawn without MatchPlayerData fails.
    // Unverified: WALL FATIGUE, which tests no Success, is left out for a minion: native unknown.
    #[allow(clippy::too_many_arguments)]
    fn pop_text(
        &mut self,
        w: &World,
        fighter: i32,
        kind: PopKind,
        stacks: f64,
        gain: Option<bool>,
        params: Option<&SimPop>,
        now: f64,
    ) {
        let Some(i) = self.k_of(fighter) else {
            if !w.by_id(fighter).is_some_and(|m| w.fighter(m).life.is_minion) {
                return;
            }
            // Deviation: a dead minion's text (a widget with no place is never drawn) is not made.
            let Some(at) = minion_at(w, fighter) else { return };
            if kind == PopKind::DodgedOff {
                return;
            }
            if kind == PopKind::NotifyText {
                if let Some(SimPop::Graph(g)) = params
                    && g.queued
                {
                    let p = self.graph_pop(w, usize::MAX, g);
                    self.queue_minion_pop(fighter, p, at, now);
                }
                return;
            }
            if let Some((p, true)) = self.pop_for(kind, stacks, gain, usize::MAX, params) {
                self.queue_minion_pop(fighter, p, at, now);
            }
            return;
        };
        if kind == PopKind::DodgedOff {
            // Comp_Pawn_DamageManager DestroyDodgePopTextMulticast: Destroy on every widget of the hit fighter's
            // DodgePopTexts (no Exit animation), then Array_Clear.
            for id in std::mem::take(&mut self.dodge_pops[i]) {
                self.pops.destroy(id);
            }
            return;
        }
        if kind == PopKind::NotifyText {
            // A Blueprint graph's params; LocalOnly: offline every pawn is locally controlled.
            // Unverified: AnimNotify_PopText's own params (its one user is Jake) are not ported.
            if let Some(SimPop::Graph(g)) = params {
                let p = self.graph_pop(w, i, g);
                if g.queued {
                    self.queue_pop(p, now);
                } else {
                    self.pops.add(p, now, &mut self.order);
                }
            }
            return;
        }
        let Some((p, is_queued)) = self.pop_for(kind, stacks, gain, i, params) else { return };
        // (Comp_State_Manager BuffDecayUI @7062 and HitDecayUIMulticast @8749: ui_ability_sameattack after the text
        // for a locally controlled dealer, posted on the dealer by the match's audio, mvs-app audio.rs)
        if is_queued {
            self.queue_pop(p, now);
        } else if kind == PopKind::Dodged {
            // (the widget joins the dodger's DodgePopTexts) A widget that ended by itself is dropped from the list
            // here (the build keeps the stale reference).
            let pops = &self.pops;
            self.dodge_pops[i].retain(|&id| pops.available(id));
            let id = self.pops.add(p, now, &mut self.order);
            self.dodge_pops[i].push(id);
        } else {
            self.pops.add(p, now, &mut self.order);
        }
    }

    /// Buff_AryaBackStab BackStabUIMulticast @27-813 (FighterHud.backstab): with AllowPop and a locally controlled
    /// Instigator of the buff's owner (the dealer) UI_Functions PopText called directly (not queued) @163-681:
    /// "Backstab!", lifetime 0.5, text_color (1, 0.18552, 0.155671, 1), location (0, 0, 200), space 3, scale 1,
    /// centred, no delay, no icons, attached to the victim, audio sfx_cha_arya_backstab_hit @575 (posted by
    /// UI_PopText.Show). Then AllowPop = false @747 and Delay(1.0) @758 -> AllowPop = true @15: the buff's own
    /// variable, so per dealer, in game time. Locally controlled as the Jumps Reset text takes it (a bot's pawn is,
    /// offline).
    fn backstab(&mut self, dealer: Fid, victim: Fid, now: f64) {
        let Some(k) = self.k_of(dealer).filter(|&k| self.broker(k).is_some_and(|b| b.is_local)) else { return };
        // a time behind the last one: the match was started again (the Lab's reset)
        if let Some(last) = self.backstab_at[k]
            && now >= last
            && now - last < 1.0
        {
            return;
        }
        self.backstab_at[k] = Some(now);
        // (a victim the HUD has no record of: its widget has no place, its Show still posts)
        let v = self.k_of(victim).unwrap_or(usize::MAX);
        let mut p = PopParams::attached(v, &self.texts.backstab, 0.5, [1.0, 0.18552, 0.155671, 1.0], [0.0, 0.0, 200.0]);
        p.audio = Some("sfx_cha_arya_backstab_hit".to_string());
        self.pops.add(p, now, &mut self.order);
    }

    /// The Lab's combo texts (FighterHud.comboCounts): TrainingGameState DisplayComboCounts (ubergraph @7355-7586),
    /// AttemptJuggleComboPopup then AttemptHitstunComboPopup, each behind its toggle (bDisplayJuggleComboDamage /
    /// bDisplayHitstunComboDamage). Each keeps one widget per hit fighter (JugglePopTexts / HitstunPopTexts): one still
    /// Available For Re Use gets Init again with the new values, else UI_Functions PopText makes one and the map takes
    /// it. Params of both: lifetime 1.5, text_color (1, 0.502886, 0, 1), space 2, scale 1, centred, no delay, icons
    /// or audio; the text the Format with the two integers (FText::AsNumber: grouped). X =
    /// GetPlayerUIPositionFractional(the hit fighter) @0-737: by its PlayerIndex, (0.3, 0.7, 0.1, 0.85) with up to 2
    /// connected players and (0.1, 0.7, 0.3, 0.85) with more (another index: 0). Y: the juggle text at 0.2, the
    /// hitstun text at 0.3 while the juggle toggle is on, else 0.2; with hud-positionBottom 0.8, 0.7 / 0.8.
    /// `counts`: (JuggleCount, JuggleDamage), (HitstunComboCount, HitstunComboDamage), the damages truncated.
    fn combo_counts(&mut self, fighter: Fid, counts: [(i64, i64); 2], now: f64) {
        let Some(c) = &self.lab_combos else { return };
        let (juggle, hitstun) = (c.juggle, c.hitstun);
        let index = usize::try_from(fighter).ok().and_then(|i| c.slot_of.get(i)).copied();
        let xs = if self.setup.players.len() > 2 { [0.1, 0.7, 0.3, 0.85] } else { [0.3, 0.7, 0.1, 0.85] };
        let x = index.and_then(|i| xs.get(i)).copied().unwrap_or(0.0);
        let bottom = self.setup.ui.hud_position_bottom;
        let k = self.k_of(fighter).unwrap_or(usize::MAX);
        let pop = |hud: &mut Hud, juggles: bool, text: String, y: f64| {
            let mut p = PopParams::attached(k, &text, 1.5, [1.0, 0.502886, 0.0, 1.0], [x, y, 0.0]);
            p.place = Place::Viewport;
            let map = if juggles { &hud.juggle_pops } else { &hud.hitstun_pops };
            match map.get(&fighter).copied() {
                Some(id) if hud.pops.available(id) => hud.pops.init_again(id, p, now),
                _ => {
                    let id = hud.pops.add(p, now, &mut hud.order);
                    let map = if juggles { &mut hud.juggle_pops } else { &mut hud.hitstun_pops };
                    map.insert(fighter, id);
                }
            }
        };
        let fill = |f: &str, a: &str, b: &str, (n, d): (i64, i64)| f.replace(a, &grouped(n)).replace(b, &grouped(d));
        if juggle {
            let t = fill(&self.texts.juggle_combo, "{JuggleCount}", "{JuggleDamage}", counts[0]);
            pop(self, true, t, if bottom { 0.8 } else { 0.2 });
        }
        if hitstun {
            let t = fill(&self.texts.hitstun_combo, "{HitstunCount}", "{HitstunDamage}", counts[1]);
            let y = match (bottom, juggle) {
                (true, true) => 0.7,
                (true, false) => 0.8,
                (false, true) => 0.3,
                (false, false) => 0.2,
            };
            pop(self, false, t, y);
        }
    }

    /// The Lab's combo texts' toggles and the fighters' PlayerIndex (None: not the Lab).
    pub fn set_lab_combos(&mut self, c: Option<LabCombos>) {
        self.lab_combos = c;
    }

    /// A graph's UI_PopTextParams (FighterHud.notifyPop): the hud bundle's string of its key, else the data's
    /// LocalizedString or SourceString; FText Format's named Int arguments grouped; space 0 the world point, any
    /// other attached to the fighter.
    fn graph_pop(&self, w: &World, fighter: usize, g: &GraphPopText) -> PopParams {
        let key = w.names.str(g.text_key);
        let mut text = match self.bundle_texts.get(key) {
            Some(s) => s.clone(),
            None => g.localized_string.or(g.source_string).map(|n| w.names.str(n).to_string()).unwrap_or_default(),
        };
        for (name, v) in &g.args {
            text = text.replace(&format!("{{{}}}", w.names.str(*name)), &grouped(*v as i64));
        }
        let world = g.space == 0;
        let mut p = PopParams::attached(
            fighter,
            &text,
            f64::from(g.lifetime),
            g.color.map(f64::from),
            if world { [0.0; 3] } else { g.location.map(f64::from) },
        );
        p.scale = f64::from(g.scale);
        p.justification = g.justification;
        p.delay = f64::from(g.delay);
        p.audio = g.audio.map(|a| w.names.str(a).to_string());
        if world {
            p.place = Place::World(Some(g.location.map(f64::from)));
        }
        p
    }

    /// The UI_PopTextParams of a 'popText' event and whether it goes through the pawn's queue (BPI_PopText) or is
    /// shown at once (FighterHud.popFor). fighter: the HUD pawn (usize::MAX: a minion); params: the event's own
    /// (gizmoForceDance's owner location); gain: a finnTreasure event's (the wallet grew; None: true). None: a text
    /// of a feeder not ported (Free For All's score texts, other
    /// characters' own).
    fn pop_for(
        &self,
        kind: PopKind,
        stacks: f64,
        gain: Option<bool>,
        fighter: usize,
        params: Option<&SimPop>,
    ) -> Option<(PopParams, bool)> {
        let x = &self.texts;
        let focus = self.broker(fighter).map_or([1.0; 4], |b| b.focus);
        let now1 = |text: &str, lifetime: f64, color: Rgba, location: [f64; 3]| {
            (PopParams::attached(fighter, text, lifetime, color, location), false)
        };
        let decay = |text: &str, scale: f64| {
            let (mut p, q) = now1(text, 1.0, focus, [650.0, 0.0, 325.0]);
            p.scale = scale;
            p.justification = 2;
            p.icon_right = self.res.brush("MI_State_SameAtk");
            (p, q)
        };
        Some(match kind {
            // Buff_LastStand @1233-1745; Buff_RefreshingKnockOut @2089-2802.
            PopKind::LastStand => queued(fighter, &x.last_stand, [1.0, 0.0, 0.035091, 1.0], 1.0, [0.0, 0.0, 200.0]),
            PopKind::SpecialsRefreshed => {
                queued(fighter, &x.specials_refreshed, [0.0, 1.0, 0.25, 1.0], 0.5, [0.0, 0.0, 300.0])
            }
            PopKind::AirRefreshed => queued(fighter, &x.air_refreshed, [0.0, 1.0, 0.25, 1.0], 0.5, [0.0, 0.0, 300.0]),
            // Buff_Caught @939-1446 / @1836-2349.
            PopKind::Caught => queued(fighter, &x.caught, [0.0, 1.0, 0.388188, 1.0], 0.5, [0.0, 0.0, 300.0]),
            PopKind::JumpsReset => {
                queued(fighter, &x.jumps_reset, [1.0, 0.895364, 0.016631, 1.0], 0.5, [0.0, 0.0, 250.0])
            }
            // Buff_TransferDebuffs @1067-1571.
            PopKind::Transferred => queued(fighter, &x.transferred, [0.0, 1.0, 0.204562, 1.0], 1.0, [0.0, 0.0, 250.0]),
            // Comp_Pawn_HitBoxHandler SpawnDodgePopText @0-1014: called directly on every machine.
            PopKind::Dodged => {
                let (mut p, q) = now1(&x.dodged, 0.5, focus, [0.0, 0.0, 300.0]);
                p.icon_right = self.res.brush("MI_State_Dodged");
                (p, q)
            }
            // BaseFighter WallFatigueFX.
            PopKind::WallFatigue => {
                let (mut p, q) = now1(&x.wall_fatigue, 0.5, focus, [0.0, 0.0, 300.0]);
                p.scale = 1.1;
                (p, q)
            }
            // Buff_HealOverTime ReceiveBeginPlay.
            PopKind::Healing => now1(&x.healing, 1.0, [0.069092, 0.807292, 0.21956, 1.0], [0.0, 0.0, 250.0]),
            // Comp_State_Manager BuffDecayUI (@7228-8147).
            PopKind::BuffDecay => decay(&x.buff_decay, 0.8),
            // HitDecayUIMulticast: scale 0.7 at 4 stacks, 0.8 above 4, 0.9 above 5, 1.0 above 6.
            PopKind::AttackDecay => {
                let scale = if stacks > 6.0 {
                    1.0
                } else if stacks > 5.0 {
                    0.9
                } else if stacks > 4.0 {
                    0.8
                } else {
                    0.7
                };
                decay(&x.attack_decay, scale)
            }
            // Gizmo's. ClenseAttachAlly's BPI_PopText (Comp_C023A_AllyAttach
            // @1462-1971): queued per pawn, lifetime 1.0, text_color (0, 1, 0.204562, 1), location (0, 0, 250),
            // space 3, scale 1, centred, no delay, icons or audio, attached to the owner (Gizmo).
            PopKind::GizmoAttach => queued(fighter, &x.cleanse, [0.0, 1.0, 0.204562, 1.0], 1.0, [0.0, 0.0, 250.0]),
            // C023A_ForceDance_Debuff's BuffApplied branch (@260-849, not on a dedicated server, the owner a
            // BP_FighterCharacter with a UI broker @173-246): UI_Functions PopText called directly (not queued),
            // lifetime 1.0, text_color (1, 0.00687, 0, 1), location = the owner's actor location @294 in space 0
            // (world: PositionForPopParams projects Location itself @320, placed once in Init), scale 1, centred, no
            // delay, no attach actor, audio sfx_cha_C023a_silenced_cast @751. The event's params carry the owner's x /
            // actor y at the apply.
            PopKind::GizmoForceDance => {
                let (mut p, q) = now1(&x.dancing, 1.0, [1.0, 0.00687, 0.0, 1.0], [0.0, 0.0, 0.0]);
                p.audio = Some("sfx_cha_C023a_silenced_cast".to_string());
                p.place = Place::World(match params {
                    Some(SimPop::At { x, y }) => Some([0.0, f64::from(-*x), f64::from(*y)]),
                    _ => None,
                });
                (p, q)
            }
            // FinnTreasureBuff @148-973: UI_Functions PopText called directly on the buff's
            // owner, space 3, scale 1, centred, no delay, lifetime 0.5, location (0, 0, 250). text: Conv_IntToText(stacks
            // x 100, grouping true), "000" at 0 stacks; text_color (1, 0.883929, 0, 1) on a gain, else (1, 0, 0.009362,
            // 1); icon_left UI_Coin_03 above 10 stacks, UI_Coin_02 above 5, else UI_Coin_01; audio
            // sfx_cha_finn_backpack_treasure_collect, sfx_misc_silence_onesecond at 0 stacks.
            PopKind::FinnTreasure => {
                #[allow(clippy::cast_possible_truncation)] // (a whole count of coins)
                let text = if stacks == 0.0 { "000".to_string() } else { grouped((stacks * 100.0) as i64) };
                let color = if gain.unwrap_or(true) { [1.0, 0.883929, 0.0, 1.0] } else { [1.0, 0.0, 0.009362, 1.0] };
                let (mut p, q) = now1(&text, 0.5, color, [0.0, 0.0, 250.0]);
                let icon = if stacks > 10.0 {
                    "UI_Coin_03"
                } else if stacks > 5.0 {
                    "UI_Coin_02"
                } else {
                    "UI_Coin_01"
                };
                p.icon_left = self.res.brush(icon);
                let audio =
                    if stacks == 0.0 { "sfx_misc_silence_onesecond" } else { "sfx_cha_finn_backpack_treasure_collect" };
                p.audio = Some(audio.to_string());
                (p, q)
            }
            // Buff_Finn_GoingOutOfBusiness @2744-3267 (Finn's GoingOutOfBusiness perk): BPI_PopText on the owner,
            // lifetime 1.0, text_color (1, 0.790325, 0, 1), location (0, 0, 300).
            PopKind::FinnSale => queued(fighter, &x.going_out, [1.0, 0.790325, 0.0, 1.0], 1.0, [0.0, 0.0, 300.0]),
            // FinnHighFiveListener @1929-2399 -> Pawn_Finn PopTextMulticast @5278: UI_Functions PopText directly,
            // the listener's PopText, lifetime 0.5, text_color (0, 0.518294, 1, 1), location (0, 0, 250), space 3,
            // scale 1, centred, no delay, icons or audio.
            PopKind::FinnHighFive => now1(&x.high_five, 0.5, [0.0, 0.518294, 1.0, 1.0], [0.0, 0.0, 250.0]),
            // Finn_Shop_Boots_Buff @15-989 -> PopTextMulticast: UI_Functions PopText directly on the owner, Format(
            // "Speed Up x{StackAmount}") with the stack count, lifetime 0.5, text_color the owner's FocusColor,
            // location (0, 0, 300), space 3, scale 1, centred, no delay, icon_left Finn_Shop_Boot, audio the buff's
            // Audio (class default none).
            // Unverified: the count is the event's stacks as taken here; the graph formats Max(CurrentStackAmount,
            // 1) @162.
            PopKind::FinnBoots => {
                #[allow(clippy::cast_possible_truncation)] // (a whole stack count)
                let text = x.speed_up.replace("{StackAmount}", &grouped(stacks as i64));
                let (mut p, q) = now1(&text, 0.5, focus, [0.0, 0.0, 300.0]);
                p.icon_left = self.res.brush("Finn_Shop_Boot");
                (p, q)
            }
            // Buff_TurnEnemyAwayOnHit SharpenVisuals @1118-1630: called directly (not queued), lifetime 1, text_color
            // (0.655304, 0.169569, 0.692708, 1), location (0, 0, 250), space 3, scale 1, centred, no delay, icons or
            // audio, attached to the buff's owner.
            PopKind::TurnAttack => now1(&x.turn_attack, 1.0, [0.655304, 0.169569, 0.692708, 1.0], [0.0, 0.0, 250.0]),
            // Buff_AllyTornadoed MulticastActivateReflect @1478-1685 (Taz's TornadoReflect): BPI_PopText of its Pop
            // Text Params, lifetime 1.0, text_color (0.902247, 0, 1, 1), location (0, 0, 300).
            PopKind::ReflectProjectiles => {
                queued(fighter, &x.reflect, [0.902247, 0.0, 1.0, 1.0], 1.0, [0.0, 0.0, 300.0])
            }
            // Buff_Cheese: BPI_PopText on the owner, white, lifetime 0.5, location (0, 0, 200), space 3, scale 1,
            // centred, no delay, icons or audio: BeginPlay's @134-643 (stacks 0) and UpdateCheeseStacks' @2929-3432
            // (stacks 2).
            PopKind::TjCheese => {
                let text = if stacks == 2.0 { &x.cheese_two } else { &x.cheese_damage };
                queued(fighter, text, [1.0; 4], 0.5, [0.0, 0.0, 200.0])
            }
            // TimedScoreGameMode's Free For All score texts through BPI_PopText (queued per pawn): lifetime 0.75,
            // location (0, 0, 300), space 3, scale 1, centred, no delay, icons or audio. OnPlayerRingoutImpl
            // Concat_StrStr("+", Conv_IntToString(points), " Knock Out!") @4022-4062, text_color (1, 0.579265, 0, 1);
            // the assist's "+" .. " Assist!" @2406-2446, (0, 0.475719, 1, 1); the respawn's text constant @2606,
            // (1, 0, 0.052997, 1). (The two concatenations are string literals of the graph: no Game.locres entry.)
            // Deviation: the port shows "+N Ringout!" where the graph has " Knock Out!".
            PopKind::FfaRingout | PopKind::FfaAssist => {
                #[allow(clippy::cast_possible_truncation)] // (a whole point count)
                let n = stacks as i64;
                let (tail, color) = if kind == PopKind::FfaRingout {
                    (" Knock Out!", [1.0, 0.579265, 0.0, 1.0])
                } else {
                    (" Assist!", [0.0, 0.475719, 1.0, 1.0])
                };
                queued(fighter, &format!("+{n}{tail}"), color, 0.75, [0.0, 0.0, 300.0])
            }
            PopKind::FfaRungout => queued(fighter, &x.ffa_rungout, [1.0, 0.0, 0.052997, 1.0], 0.75, [0.0, 0.0, 300.0]),
            _ => return None,
        })
    }

    /// The frame's HUD elements.
    pub fn draw(&mut self, f: &HudFrame, fonts: &mut Fonts, out: &mut ElementList) {
        let (now, real) = (f.hud_time, f.real);
        let size = [f64::from(f.size[0]), f64::from(f.size[1])];
        let dpi = dpi_scale(size[0], size[1]);
        let m = self.huds.len();
        let cam = f.camera;
        let proj = |p: [f64; 3]| project(cam, size, p);
        self.drawn.clear();
        // World-space widget components, beneath the screen layer (Finn's shops, Tom's pointer).
        for (c, h) in self.chars.iter_mut().enumerate() {
            self.drawn.extend(h.draw_world(fonts, out, dpi, &proj).into_iter().map(|r| Part::Char(c, r)));
        }
        for k in 0..m {
            if self.pawns[k].is_none() {
                continue;
            }
            self.huds[k].tick(&self.res, now, real, self.facing_left[k]);
            self.tops[k].tick(&self.res, now, real, &mut self.pops);
        }
        // SWorldWidgetScreenLayer's Tick (142617ff0) gives each component's canvas slot ZOrder = -(distance from the
        // view origin to the component's world location): the nearer widget paints over the farther one, and equal
        // ones keep the order their slots were added in (bottom then top, fighter by fighter, then the spawned
        // actors' skill shot targets).
        // The skill shot targets are painted after the fighters' widgets, not sorted with them (a ledger row).
        // Unverified: a skill shot target's slot is taken as added after every fighter's (a pawn attached later, a
        // respawn's fresh widgets, are not ordered by their add).
        self.skillshots.tick(real, f.skillshots, &proj);
        let at = |k: usize| if k & 1 == 1 { f.top } else { f.bottom }.get(k >> 1).copied().flatten();
        let skills = &self.skillshots;
        let far = |k: usize| {
            if k < 2 * m { at(k).map_or(0.0, |a| a.dist) } else { skills.dist(k - 2 * m, cam.origin) }
        };
        let mut order: Vec<usize> = (0..2 * m).filter(|&k| self.pawns[k >> 1].is_some()).collect();
        order.extend(2 * m..2 * m + skills.len());
        order.sort_by(|&a, &b| far(b).total_cmp(&far(a)).then(a.cmp(&b)));
        for k in order {
            if k >= 2 * m {
                if self.skillshots.draw(k - 2 * m, fonts, out, dpi) {
                    self.drawn.push(Part::Skill(k - 2 * m));
                }
                continue;
            }
            let Some(p) = at(k) else { continue };
            let i = k >> 1;
            if k & 1 == 1 {
                if self.tops[i].draw(fonts, out, p.x, p.y, dpi) {
                    self.drawn.push(Part::Top(i));
                }
            } else if self.huds[i].draw(fonts, out, p.x, p.y, dpi) {
                self.drawn.push(Part::Bottom(i));
            }
        }
        // The spawned actors' other screen-space widget components, in the same layer.
        self.drawn.extend(
            self.dash_targets.draw(fonts, out, dpi, real, &proj, f.items, &self.res.labels).into_iter().map(Part::Dash),
        );
        for (c, h) in self.chars.iter_mut().enumerate() {
            self.drawn.extend(h.draw_screen(fonts, out, dpi, real, &proj).into_iter().map(|r| Part::Char(c, r)));
        }
        self.drawn.extend(self.indicators.draw(fonts, out, dpi, now, real, &proj).into_iter().map(Part::Indicator));
        // Deviation: UI_Game's first child, UI_ConnectionState (the connection icon in the top right corner), is not
        // drawn: the user's choice.
        if self.setup.mode == MODE_WAVES
            && let Some(r) = self.waves
        {
            let counter = custom_text::waves_counter(r.ring_outs, r.target);
            self.scoreboard.waves(&counter, &custom_text::waves_clock(r.seconds, r.target.is_some()));
        }
        self.scoreboard.tick(&self.res, now, real, f.round_time_remaining, &mut self.sounds);
        if self.scoreboard.draw(fonts, out, size[0], size[1], dpi) {
            self.drawn.push(Part::Scoreboard);
        }
        // UI_VictoryText: UI_Game's next child.
        if let Some(v) = &mut self.victory {
            v.tick(now, real, &mut self.sounds);
            if v.draw(fonts, out, size[0], size[1], dpi) {
                self.drawn.push(Part::Victory);
            }
        }
        // Pawn_Velma's GlassesOff_C (`chars/velma.rs`): AddToViewport(0) after UI_Game, a full-screen BackgroundBlur
        // of strength 2 over what is painted before it.
        // Unverified: painted before every PopText and input label, where the build paints the viewport's ZOrder 0
        // widgets in the order they were added.
        if self.chars.iter().any(CharHud::glasses_off) {
            let geometry = PaintGeometry { size, transform: [1.0, 0.0, 0.0, 1.0, 0.0, 0.0] };
            let layer = i32::try_from(out.elements.len()).unwrap_or(i32::MAX);
            out.push(layer, None, DrawEffects::NONE, Element::Blur(BlurEl { geometry, strength: 2.0 }));
        }
        // PopText and input label widgets are added to the viewport after UI_Game, all with ZOrder 0: they paint in
        // the order they were added.
        self.pops.tick(&self.res, now, real, &self.actors, &proj, size, &mut self.sounds);
        self.inputs.tick(&self.res, dpi, real, f.items, &self.actors, &proj);
        let mut pops: Vec<_> = self.pops.pops_mut().collect();
        let mut labels: Vec<_> = self.inputs.labels_mut().collect();
        let mut added: Vec<(u64, bool, usize)> = pops.iter().enumerate().map(|(k, p)| (p.order, false, k)).collect();
        added.extend(labels.iter().enumerate().map(|(k, l)| (l.order, true, k)));
        added.sort_unstable();
        for (_, label, k) in added {
            if label {
                if labels[k].draw(fonts, out, dpi) {
                    self.drawn.push(Part::Label(k));
                }
            } else if pops[k].draw(fonts, out, dpi) {
                self.drawn.push(Part::Pop(k));
            }
        }
        // UI_Countdown: AddToViewport(4000), above the rest.
        if let Some(c) = &mut self.countdown {
            c.tick(real, f.hold, &mut self.sounds);
            if c.draw(fonts, out, size[0], size[1], dpi) {
                self.drawn.push(Part::Countdown);
            }
            if c.done {
                self.countdown = None;
                self.countdown_finished = true;
            }
        }
    }

    /// The widget trees the last draw painted, in paint order, each with its owner as the HUD tape names it
    /// (`AttachedBottom[i]`, `AttachedTop[i]`, `Scoreboard`, `VictoryText`, `PopText`, `PopInputLabel`,
    /// `Countdown`, the character parts' owners, `SkillshotTarget`, `DashTarget`, `Indicator`):
    /// the check against the tape. Good until the next observe or draw.
    pub fn painted(&self) -> Vec<(String, &Tree)> {
        let labels: Vec<_> = self.inputs.labels().collect();
        self.drawn
            .iter()
            .filter_map(|p| {
                Some(match *p {
                    Part::Char(c, r) => {
                        let (o, t) = self.chars.get(c)?.roots().into_iter().nth(r)?;
                        (o.to_string(), t)
                    }
                    Part::Skill(k) => ("SkillshotTarget".to_string(), self.skillshots.tree(k)?),
                    Part::Dash(k) => ("DashTarget".to_string(), self.dash_targets.tree(k)?),
                    Part::Indicator(k) => ("Indicator".to_string(), self.indicators.tree(k)?),
                    Part::Bottom(k) => (format!("AttachedBottom[{}]", self.pawns[k]?), &self.huds[k].t),
                    Part::Top(k) => (format!("AttachedTop[{}]", self.pawns[k]?), &self.tops[k].t),
                    Part::Scoreboard => ("Scoreboard".to_string(), &self.scoreboard.t),
                    Part::Victory => ("VictoryText".to_string(), &self.victory.as_ref()?.t),
                    Part::Pop(k) => ("PopText".to_string(), &self.pops.pops().get(k)?.t),
                    Part::Label(k) => ("PopInputLabel".to_string(), &labels.get(k)?.t),
                    // (the card is dropped by the draw that finds it done, and a done card is not painted)
                    Part::Countdown => ("Countdown".to_string(), &self.countdown.as_ref()?.t),
                })
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn setup(mode: u8, teams: &[u8], bots: &[bool]) -> HudSetup {
        HudSetup {
            mode,
            me: 0,
            players: teams
                .iter()
                .zip(bots)
                .map(|(&team, &bot)| HudPlayer { team, bot, ..HudPlayer::default() })
                .collect(),
            max_score: 2,
            ..HudSetup::default()
        }
    }

    #[test]
    fn palettes() {
        let s = setup(1, &[0, 1], &[false, false]);
        assert_eq!(team_palette(&s, 0), 0);
        assert_eq!(team_palette(&s, 1), 1);
        // two humans: the team index
        assert_eq!(player_palette(&s, 0), 0);
        let s = setup(1, &[0, 1], &[false, true]);
        assert_eq!(player_palette(&s, 0), 4);
        assert_eq!(player_palette(&s, 1), 1);
        let s = setup(3, &[1, 0, 2, 3], &[false, true, true, true]);
        assert_eq!(team_palette(&s, 1), 0);
        assert_eq!(team_palette(&s, 0), 1);
        assert_eq!(team_palette(&s, 2), 2);
        assert_eq!(team_palette(&s, 3), 3);
        // online, P2 local in 2v2: IsMe 4, the ally 0, the others 1
        let mut s = setup(2, &[0, 1, 1, 0], &[false, false, false, false]);
        s.online = true;
        s.me = 1;
        assert_eq!(player_palette(&s, 1), 4);
        assert_eq!(player_palette(&s, 2), 0);
        assert_eq!(player_palette(&s, 0), 1);
        assert_eq!(player_palette(&s, 3), 1);
    }

    /// An arcade round's stage lines from the player's Game.locres, and the countdown's form of them (needs the
    /// dev store: skipped without it).
    #[test]
    fn arcade_stage_lines() {
        let Ok(store) = mvs_store::Store::open(mvs_store::dev_store_dir()) else { return };
        let (Ok(text), Ok(b)) = (GameText::load(&store), Bundle::load(&store, "hud")) else { return };
        let a = HudArcade { round: 2, levels: 5 };
        assert_eq!(arcade_stage(&text, MI_ARCADE_STAGE, "{Number}", a), "Stage 2 of 5");
        let line = arcade_stage(&text, CD_ARCADE_STAGE, "{Round}", a);
        assert_eq!(line, "Stage 2 of 5");
        let c = Countdown::new(&b, &text, 0, 3, false, Some(&line));
        let win = c.t.find("WinCondition");
        assert_eq!(c.t.widget(win).text, "Stage 2 of 5");
        assert_eq!(c.t.widget(win).vis, crate::slate::Vis::SelfHitTestInvisible);
        let sc = c.t.find("ScoreContainer");
        assert_eq!(c.t.widget(sc).vis, crate::slate::Vis::Hidden);
    }

    /// Rick's and Velma's input label texts from the player's Game.locres (needs the dev store: skipped without it).
    #[test]
    fn char_label_texts() {
        let Ok(store) = mvs_store::Store::open(mvs_store::dev_store_dir()) else { return };
        let Ok(text) = GameText::load(&store) else { return };
        let t = Texts::new(&text);
        assert_eq!(t.teleport, "Teleport");
        assert_eq!(t.solve_mystery, "Solve the Mystery!");
        assert_eq!(t.down_prefix, "DOWN + ");
    }

    /// A Waves run's words: the counter with and without a target, the clock with hundredths against the clock.
    #[test]
    fn waves_words() {
        assert_eq!(custom_text::waves_counter(12, Some(100)), "RING-OUTS 12 / 100");
        assert_eq!(custom_text::waves_counter(0, None), "RING-OUTS 0");
        assert_eq!(custom_text::waves_clock(65.256, true), "1:05.25");
        assert_eq!(custom_text::waves_clock(65.999, false), "1:05");
        assert_eq!(custom_text::waves_clock(3599.999, true), "59:59.99");
        assert_eq!(custom_text::waves_clock(-1.0, true), "0:00.00");
        assert_eq!(custom_text::waves_clock(f64::NAN, false), "0:00");
    }

    /// The match's World and a Waves HUD over it: the player and the enemies' slot, a bot (needs the dev store).
    fn waves_hud() -> Option<(mvs_sim_trace::harness::Harness, Hud, Bundle)> {
        use mvs_sim::setup::{MatchPlayer, MatchSetup};
        let Ok(store) = mvs_store::Store::open(mvs_store::dev_store_dir()) else {
            eprintln!("note: no store at {}; nothing checked", mvs_store::dev_store_dir().display());
            return None;
        };
        let player = |c: &str| MatchPlayer { character: Some(c.to_owned()), ..MatchPlayer::default() };
        let ms = MatchSetup { players: vec![player("shaggy"), player("shaggy")], ..MatchSetup::default() };
        let h = mvs_sim_trace::harness::fresh(&ms, 1)?;
        let bundle = Bundle::load(&store, "hud").expect("the hud bundle");
        let text = GameText::load(&store).expect("the game text");
        let mut s = setup(MODE_WAVES, &[0, 1], &[false, true]);
        s.max_score = -1;
        let hud = Hud::new(&bundle, &text, &h.w, &s).expect("the HUD");
        Some((h, hud, bundle))
    }

    /// Context 6: no versus card (pausing from the start), the scoreboard with the player's slot alone, the run's
    /// counter in modeLabel and its clock in the clock, by variant (needs the dev store).
    #[test]
    fn a_waves_hud_shows_the_player_alone_and_the_run() {
        let Some((h, mut hud, bundle)) = waves_hud() else { return };
        let Ok(store) = mvs_store::Store::open(mvs_store::dev_store_dir()) else { return };
        let mut fonts = Fonts::new(&store, &bundle).expect("the fonts");
        assert!(hud.countdown.is_none() && hud.countdown_finished());
        let id: mvs_view::M4 = std::array::from_fn(|r| std::array::from_fn(|c| f64::from(u8::from(r == c))));
        let cam = Camera { origin: [0.0; 3], translated_world_to_view: id, view_to_clip: id };
        let mut draw = |hud: &mut Hud, t: f64| {
            let f = HudFrame {
                size: [1920, 1080],
                hud_time: t,
                real: t,
                bottom: &[],
                top: &[],
                camera: &cam,
                items: &[],
                skillshots: &[],
                round_time_remaining: 0.0,
                hold: false,
            };
            hud.draw(&f, &mut fonts, &mut ElementList::default());
        };
        hud.observe(&h.w, &[], 2.0, 2.0);
        hud.set_waves(Some(WavesHud { ring_outs: 12, target: Some(100), seconds: 65.256 }));
        draw(&mut hud, 2.0);
        draw(&mut hud, 2.1);
        let text = |hud: &Hud, under: &str, name: &str| {
            let t = &hud.scoreboard.t;
            t.widget(t.find_in(t.find(under), name)).text.clone()
        };
        let (mode, clock) = (("UI_IGv3_MatchInfo", "modeLabel"), ("UI_IGv3_Clock", "timeLabel"));
        assert_eq!(text(&hud, mode.0, mode.1), "RING-OUTS 12 / 100");
        assert_eq!(text(&hud, clock.0, clock.1), "1:05.25");
        assert!(hud.slot_rect(0).is_some(), "the player's slot is painted");
        assert!(hud.slot_rect(1).is_none(), "the enemies' slot has none");
        hud.set_waves(Some(WavesHud { ring_outs: 3, target: None, seconds: 65.9 }));
        draw(&mut hud, 2.2);
        assert_eq!(text(&hud, mode.0, mode.1), "RING-OUTS 3");
        assert_eq!(text(&hud, clock.0, clock.1), "1:05");
    }

    /// Hundreds of enemies bound to one slot, coming and going (six alive at most, the pawns' ids wrapping): each
    /// live one has its damage readout on the slot's broker, and the HUD's per-pawn lists keep no place for the gone
    /// (needs the dev store).
    #[test]
    fn waves_enemies_attach_and_detach_on_one_slot() {
        use mvs_sim::modes::waves::ENEMY_SLOT;
        use mvs_sim::world::pawns::{self, FirstTicks, PawnSpawn};
        let Some((mut h, mut hud, _)) = waves_hud() else { return };
        let ch = h.w.fighter(h.fighter(1)).ch;
        let mut alive = VecDeque::new();
        let mut t = 1.0;
        for i in 0..1000 {
            let at = PawnSpawn { character: ch, x: 0.0, y: 0.0, facing_left: false, at: FirstTicks::Leftover };
            let m = pawns::spawn_pawn(&mut h.w, at);
            pawns::bind_slot(&mut h.w, m, ENEMY_SLOT);
            h.w.fighter_mut(m).stats.damage = 37.0;
            alive.push_back(m);
            if alive.len() > 6 {
                let gone = alive.pop_front().expect("a pawn");
                pawns::despawn_pawn(&mut h.w, gone, false);
            }
            let events = std::mem::take(&mut h.w.events);
            t += 0.1;
            hud.observe(&h.w, &events, t, t);
            let n = hud.players.len();
            let live: Vec<_> = (n..hud.pawns.len()).filter(|&k| hud.pawns[k].is_some()).collect();
            assert_eq!(live.len(), alive.len(), "spawn {i}");
            for &m in &alive {
                let k = hud.k_of(h.w.fighter(m).id).expect("an attached enemy");
                assert_eq!(hud.slot_of[k], 1);
                let t = &hud.huds[k].t;
                let digit = t.find_in(t.find("UI_IGv3_Percentage"), "digit1");
                assert_eq!(t.widget(digit).text, "7", "its damage readout");
            }
        }
        assert!(hud.pawns.len() <= hud.players.len() + 7, "{} places", hud.pawns.len());
        assert_eq!(hud.huds.len(), hud.pawns.len());
        assert_eq!(hud.sent.len(), hud.pawns.len());
        assert!(hud.snacks.is_empty() && hud.passes.is_empty());
        // (the two players' and the live enemies' at most)
        assert!(hud.ui_ops.len() <= 8 && hud.ability_ops.len() <= 8);
    }

    /// A Waves run's player is a mode pawn bound to slot 0: it takes the player's own place, and the parts that look a
    /// player up (`PlayerMap`) find it by its slot; the enemies' slot stands for none of them. Outside Waves the
    /// players are found by their ids (needs the dev store).
    #[test]
    fn waves_player_found_by_its_slot() {
        use common::PlayerMap;
        use mvs_sim::world::pawns::{self, FirstTicks, PawnSpawn};
        let Some((mut h, mut hud, _)) = waves_hud() else { return };
        let ch = h.w.fighter(h.fighter(0)).ch;
        let spawn = |h: &mut mvs_sim_trace::harness::Harness, slot: i32| {
            let at = PawnSpawn { character: ch, x: 0.0, y: 0.0, facing_left: false, at: FirstTicks::Leftover };
            let m = pawns::spawn_pawn(&mut h.w, at);
            pawns::bind_slot(&mut h.w, m, slot);
            m
        };
        let me = spawn(&mut h, 0);
        let enemy = spawn(&mut h, 1);
        h.w.fighter_mut(me).body.x = 120.0;
        let events = std::mem::take(&mut h.w.events);
        hud.observe(&h.w, &events, 1.0, 1.0);
        let me_id = h.w.fighter(me).id;
        assert_eq!(hud.pawns()[0], Some(me_id), "the player's place");
        assert_eq!(hud.actors[0], Some([0.0, -120.0, f64::from(h.w.fighter(me).body.y)]));
        assert_eq!(hud.item_player(me_id), Some(0));
        assert_eq!(hud.item_player(h.w.fighter(enemy).id), Some(1));
        let waves = PlayerMap::of(&hud.setup);
        assert_eq!(waves.fighter(&h.w, 0), Some(me));
        assert_eq!(waves.fighter(&h.w, 1), None, "the enemies' slot");
        assert_eq!((waves.index(&h.w, me), waves.index(&h.w, enemy)), (Some(0), Some(1)));
        let versus = PlayerMap::of(&setup(1, &[0, 1], &[false, true]));
        assert_eq!(versus.fighter(&h.w, 1), Some(h.fighter(1)));
        assert_eq!(versus.index(&h.w, h.fighter(1)), Some(1));
        assert_eq!(versus.index(&h.w, me), usize::try_from(me_id).ok(), "a pawn's id, as before");
    }

    /// A Waves run's World binds its player while it is made and the app clears those events before the first step:
    /// the HUD still attaches the player at its place, with its damage on its readout and the scoreboard and its
    /// abilities on the scoreboard's cooldowns, and the director's enemies on their slot (needs the dev store).
    #[test]
    fn a_waves_run_attaches_its_player_from_the_start() {
        use mvs_sim::setup::WavesVariant;
        use mvs_sim_trace::waves as run;
        let Ok(store) = mvs_store::Store::open(mvs_store::dev_store_dir()) else {
            eprintln!("note: no store at {}; nothing checked", mvs_store::dev_store_dir().display());
            return;
        };
        let cast = ["shaggy", "batman", "superman", "wonderwoman", "taz"].map(String::from).into();
        let ws = run::bot_setup(WavesVariant::Endless, "superman", 3, cast);
        let data = run::data_for(&store, &ws, "trainingroomsmall").expect("the data");
        let mut w = run::world(&data, &ws, 11).expect("the World");
        w.events.clear();
        let bundle = Bundle::load(&store, "hud").expect("the hud bundle");
        let text = GameText::load(&store).expect("the game text");
        let mut s = setup(MODE_WAVES, &[0, 1], &[false, true]);
        s.max_score = -1;
        let mut hud = Hud::new(&bundle, &text, &w, &s).expect("the HUD");
        let player = w.waves.as_ref().and_then(|r| r.player).expect("the player");
        let n = hud.players.len();
        let mut t = 0.0;
        let mut step = |w: &mut World, hud: &mut Hud| {
            run::step(w);
            let events = std::mem::take(&mut w.events);
            t += 1.0 / 60.0;
            hud.observe(w, &events, t, t);
        };
        for _ in 0..240 {
            step(&mut w, &mut hud);
        }
        assert_eq!(hud.pawns()[0], Some(w.fighter(player).id), "the player's place");
        assert!(hud.pawns()[n..].iter().any(Option::is_some), "an enemy attached");
        let (slot, cooldowns) = hud.scoreboard.slot_parts(0).expect("the player's slot");
        assert_eq!(cooldowns, abilities_of(&w, player, &hud.res).len());
        assert!(cooldowns > 0, "the player's abilities on the scoreboard");
        // (set after the step: no hit moves it before the HUD reads it)
        w.fighter_mut(player).stats.damage = 37.0;
        hud.observe(&w, &[], t + 0.1, t + 0.1);
        let readout = &hud.huds[0].t;
        let digit = readout.find_in(readout.find("UI_IGv3_Percentage"), "digit1");
        assert_eq!(readout.widget(digit).text, "7", "the player's damage readout");
        let sb = &hud.scoreboard.t;
        let digit = sb.find_in(sb.find_in(slot, "UI_IGv3_Percentage"), "digit1");
        assert_eq!(sb.widget(digit).text, "7", "the player's scoreboard percentage");
    }

    #[test]
    fn warning_layout() {
        let [a, b] = warning_pops("A", "B", [1.0, 0.0, 0.0, 1.0], [0.0; 4], None);
        assert_eq!((a.scale, a.location, a.lifetime), (2.25, [0.5, 0.2, 0.0], 3.0));
        assert_eq!((b.scale, b.location, b.lifetime, b.delay), (1.25, [0.5, 0.27, 0.0], 2.75, 0.25));
        assert_eq!(a.place, Place::Viewport);
        assert_eq!(b.team_color, None);
    }

    #[test]
    fn stock_line() {
        assert_eq!(custom_text::stock_win_condition(0, 1), "Every player has 1 stock, last one standing wins");
        assert_eq!(custom_text::stock_win_condition(1, 3), "Every player has 3 stocks, last team standing wins");
    }

    #[test]
    fn projection() {
        // view axes = world axes; a reversed-Z perspective with near 10 (clip z = near, clip w = view z)
        let id: mvs_view::M4 = std::array::from_fn(|r| std::array::from_fn(|c| f64::from(u8::from(r == c))));
        let mut p = [[0.0; 4]; 4];
        p[0][0] = 1.0;
        p[1][1] = 1.0;
        p[3][2] = 10.0;
        p[2][3] = 1.0;
        let cam = Camera { origin: [0.0, 0.0, -100.0], translated_world_to_view: id, view_to_clip: p };
        assert_eq!(project(&cam, [200.0, 100.0], [0.0, 0.0, 0.0]), Some([100.0, 50.0]));
        assert_eq!(project(&cam, [200.0, 100.0], [50.0, 50.0, 0.0]), Some([150.0, 25.0]));
        assert_eq!(project(&cam, [200.0, 100.0], [0.0, 0.0, -200.0]), None);
    }

    /// Finn's own texts from Game.locres: Speed Up x{n} with the boot and the focus colour, High Five!! at once, Going
    /// Out Of Business! through the pawn's queue (needs the dev store).
    #[test]
    fn finn_pop_texts() {
        use mvs_sim::setup::{MatchPlayer, MatchSetup};
        let Ok(store) = mvs_store::Store::open(mvs_store::dev_store_dir()) else {
            eprintln!("note: no store at {}; nothing checked", mvs_store::dev_store_dir().display());
            return;
        };
        let player = |c: &str| MatchPlayer { character: Some(c.to_owned()), ..MatchPlayer::default() };
        let ms = MatchSetup { players: vec![player("finn"), player("shaggy")], ..MatchSetup::default() };
        let Some(h) = mvs_sim_trace::harness::fresh(&ms, 1) else { return };
        let bundle = Bundle::load(&store, "hud").expect("the hud bundle");
        let text = GameText::load(&store).expect("the game text");
        let hud = Hud::new(&bundle, &text, &h.w, &setup(1, &[0, 1], &[false, false])).expect("the HUD");
        let pop = |kind, stacks| hud.pop_for(kind, stacks, None, 0, None).expect("a text");
        let (boots, q) = pop(PopKind::FinnBoots, 2.0);
        assert_eq!((boots.text.as_str(), q, boots.location), ("Speed Up x2", false, [0.0, 0.0, 300.0]));
        assert!(boots.icon_left.as_deref().is_some_and(|i| i.ends_with("Finn_Shop_Boot")));
        assert_eq!(boots.text_color, hud.broker(0).expect("a broker").focus);
        let (five, q) = pop(PopKind::FinnHighFive, 0.0);
        assert_eq!((five.text.as_str(), q, five.lifetime), ("High Five!!", false, 0.5));
        let (sale, q) = pop(PopKind::FinnSale, 0.0);
        assert_eq!((sale.text.as_str(), q, sale.lifetime), ("Going Out Of Business!", true, 1.0));
    }

    /// Velma's GlassesOff blur: up only in an online match (IsPlayingOnline) for a live local Velma whose
    /// GlassesOffCount is above 0 (GlassesOff @2764 / GlassesOn @3081; needs the dev store). No trace runs an online
    /// Velma match.
    #[test]
    fn velma_glasses_off_gate() {
        use mvs_sim::setup::{MatchPlayer, MatchSetup};
        let Ok(store) = mvs_store::Store::open(mvs_store::dev_store_dir()) else { return };
        let player = |c: &str| MatchPlayer { character: Some(c.to_owned()), ..MatchPlayer::default() };
        let ms = MatchSetup { players: vec![player("velma"), player("shaggy")], ..MatchSetup::default() };
        let Some(mut h) = mvs_sim_trace::harness::fresh(&ms, 1) else { return };
        let bundle = Bundle::load(&store, "hud").expect("the hud bundle");
        let text = GameText::load(&store).expect("the game text");
        let up = |h: &mvs_sim_trace::harness::Harness, online: bool| {
            let mut s = setup(1, &[0, 1], &[false, false]);
            s.online = online;
            s.players[0].character = Some("velma".to_owned());
            let mut hud = Hud::new(&bundle, &text, &h.w, &s).expect("the HUD");
            hud.observe(&h.w, &[], 0.0, 0.0);
            hud.chars.iter().any(CharHud::glasses_off)
        };
        let v = h.fighter(0);
        assert!(!up(&h, true), "a count of 0: no blur");
        mvs_sim::chars::velma::state::velma_mut(&mut h.w, v).expect("a Velma").glasses_off_count = 1;
        assert!(up(&h, true), "online, a live Velma with the glasses off: the blur");
        assert!(!up(&h, false), "an offline match: no blur");
        h.w.fighter_mut(v).life.dead = 1.0;
        assert!(!up(&h, true), "a dead Velma: no blur");
    }

    /// The pops the HUD had no text for: Turn Attack, REFLECT PROJECTILES, Tom and Jerry's cheese, the Free For All
    /// score texts (needs the dev store).
    #[test]
    fn perk_and_ffa_pop_texts() {
        use mvs_sim::setup::{MatchPlayer, MatchSetup};
        let Ok(store) = mvs_store::Store::open(mvs_store::dev_store_dir()) else { return };
        let player = |c: &str| MatchPlayer { character: Some(c.to_owned()), ..MatchPlayer::default() };
        let ms = MatchSetup { players: vec![player("shaggy"), player("shaggy")], ..MatchSetup::default() };
        let Some(h) = mvs_sim_trace::harness::fresh(&ms, 1) else { return };
        let bundle = Bundle::load(&store, "hud").expect("the hud bundle");
        let text = GameText::load(&store).expect("the game text");
        let hud = Hud::new(&bundle, &text, &h.w, &setup(1, &[0, 1], &[false, false])).expect("the HUD");
        let pop = |kind, stacks| hud.pop_for(kind, stacks, None, 0, None).expect("a text");
        let cases = [
            (PopKind::TurnAttack, 0.0, "Turn Attack", false, 1.0),
            (PopKind::ReflectProjectiles, 0.0, "REFLECT PROJECTILES", true, 1.0),
            (PopKind::TjCheese, 0.0, "+ Damage", true, 0.5),
            (PopKind::TjCheese, 2.0, "+2", true, 0.5),
            (PopKind::FfaRingout, 2.0, "+2 Knock Out!", true, 0.75),
            (PopKind::FfaAssist, 1.0, "+1 Assist!", true, 0.75),
            (PopKind::FfaRungout, 0.0, "-1 Rungout!", true, 0.75),
        ];
        for (kind, stacks, want, queued, lifetime) in cases {
            let (p, q) = pop(kind, stacks);
            assert_eq!((p.text.as_str(), q, p.lifetime), (want, queued, lifetime), "{kind:?}");
        }
        assert_eq!(hud.texts.enraged, "Enraged");
    }

    /// Arya's kill pie: its "Arya's Pie" from Game.locres at the pie's first sight, on the pie, and only once (needs
    /// the dev store and the Rust baseline's arya-combat, where the pie is first seen at frame 5824).
    #[test]
    fn a_kill_pie_pops_its_text_on_the_pie() {
        let Ok(store) = mvs_store::Store::open(mvs_store::dev_store_dir()) else {
            eprintln!("note: no store at {}; nothing checked", mvs_store::dev_store_dir().display());
            return;
        };
        let Some(mut h) = mvs_sim_trace::harness::at_baseline("arya-combat", 5823) else { return };
        let bundle = Bundle::load(&store, "hud").expect("the hud bundle");
        let text = GameText::load(&store).expect("the game text");
        let mut hud = Hud::new(&bundle, &text, &h.w, &setup(1, &[0, 1], &[false, false])).expect("the HUD");
        assert_eq!(hud.texts.arya_pie, "Arya's Pie");
        let pies = |hud: &Hud| -> Vec<(Place, u64)> {
            hud.pops
                .pops()
                .iter()
                .filter(|t| t.p.text == hud.texts.arya_pie)
                .map(|t| (t.p.place.clone(), t.id))
                .collect()
        };
        hud.observe(&h.w, &[], 1.0, 1.0);
        assert!(pies(&hud).is_empty());
        h.step_recorded();
        let pie = |w: &World| {
            w.actors.actors.iter().find_map(|(_, a)| match &a.state {
                WorldActorState::KillPie(d) if !a.dead => Some([0.0, f64::from(-d.x), f64::from(d.y)]),
                _ => None,
            })
        };
        let at = pie(&h.w).expect("the pie at frame 5824");
        hud.observe(&h.w, &[], 1.1, 1.1);
        let made = pies(&hud);
        assert_eq!(made.len(), 1);
        assert_eq!(made[0].0, Place::Actor(at));
        h.step_recorded();
        hud.observe(&h.w, &[], 1.2, 1.2);
        let next = pies(&hud);
        assert_eq!(next.len(), 1);
        assert_eq!(next[0], (Place::Actor(pie(&h.w).expect("the pie")), made[0].1));
    }

    /// LeBron's pass buff on a pawn: one "Pass Received" from Game.locres when it comes, none while it stays (needs
    /// the dev store).
    #[test]
    fn a_pass_buff_pops_pass_received_once() {
        use mvs_sim::setup::{MatchPlayer, MatchSetup};
        let Ok(store) = mvs_store::Store::open(mvs_store::dev_store_dir()) else {
            eprintln!("note: no store at {}; nothing checked", mvs_store::dev_store_dir().display());
            return;
        };
        let player = |c: &str| MatchPlayer { character: Some(c.to_owned()), ..MatchPlayer::default() };
        let ms = MatchSetup { players: vec![player("lebron"), player("shaggy")], ..MatchSetup::default() };
        let Some(mut h) = mvs_sim_trace::harness::fresh(&ms, 1) else { return };
        let bundle = Bundle::load(&store, "hud").expect("the hud bundle");
        let text = GameText::load(&store).expect("the game text");
        let mut hud = Hud::new(&bundle, &text, &h.w, &setup(1, &[0, 1], &[false, false])).expect("the HUD");
        let pass = |hud: &Hud| hud.pops.pops().iter().filter(|t| t.p.text == hud.texts.pass_received).count();
        assert_eq!(hud.texts.pass_received, "Pass Received");
        hud.observe(&h.w, &[], 1.0, 1.0);
        assert_eq!(pass(&hud), 0);
        // (the buff on the second fighter: its class is LeBron's, bound for the match)
        let (f, cls) = (h.w.players[1], h.w.lit("Buff_BasketballPass_C"));
        mvs_sim::combat::buffs::spawn_buff(&mut h.w, f, cls, 20.0, Some(0), Some(f), true, None);
        hud.observe(&h.w, &[], 1.5, 1.5);
        assert_eq!(pass(&hud), 1);
        let p = hud.pops.pops().iter().find(|t| t.p.text == hud.texts.pass_received).expect("the pass text");
        assert_eq!((p.p.fighter, p.p.location, p.p.lifetime), (1, [0.0, 0.0, 250.0], 1.0));
        // still on: no second text
        hud.observe(&h.w, &[], 1.6, 1.6);
        assert_eq!(pass(&hud), 1);
    }

    /// The Lab's combo texts: each behind its toggle, a bot's hits left out, one widget per hit fighter given its
    /// values again, placed by the hit fighter's PlayerIndex (needs the dev store).
    #[test]
    fn the_lab_combo_texts_follow_their_toggles() {
        use mvs_sim::setup::{MatchPlayer, MatchSetup};
        let Ok(store) = mvs_store::Store::open(mvs_store::dev_store_dir()) else {
            eprintln!("note: no store at {}; nothing checked", mvs_store::dev_store_dir().display());
            return;
        };
        let player = |bot| MatchPlayer { character: Some("shaggy".to_owned()), bot, ..MatchPlayer::default() };
        let ms = MatchSetup { players: vec![player(false), player(true)], ..MatchSetup::default() };
        let Some(h) = mvs_sim_trace::harness::fresh(&ms, 1) else { return };
        let w = &h.w;
        let bundle = Bundle::load(&store, "hud").expect("the hud bundle");
        let text = GameText::load(&store).expect("the game text");
        let mut hud = Hud::new(&bundle, &text, w, &setup(4, &[0, 1], &[false, true])).expect("the HUD");
        let id = |n: usize| w.fighter(w.players[n]).id;
        let hit = |victim: usize, by: usize, n: i32, d: f32| Event::ComboCounts {
            fighter: id(victim),
            juggler: id(by),
            juggle_count: n,
            juggle_damage: d,
            hitstun_count: n + 1,
            hitstun_damage: d + 1000.0,
        };
        let texts = |hud: &Hud| -> Vec<(String, [f64; 3])> {
            hud.pops
                .pops()
                .iter()
                .map(|t| (t.p.text.replace(['\r', '\n'], "|").replace("||", "|"), t.p.location))
                .collect()
        };
        // outside the Lab, and with both toggles off: none
        hud.observe(w, &[hit(1, 0, 1, 5.0)], 1.0, 1.0);
        hud.set_lab_combos(Some(LabCombos { juggle: false, hitstun: false, slot_of: vec![0, 1] }));
        hud.observe(w, &[hit(1, 0, 1, 5.0)], 1.0, 1.0);
        assert!(texts(&hud).is_empty());
        // the juggle text alone
        hud.set_lab_combos(Some(LabCombos { juggle: true, hitstun: false, slot_of: vec![0, 1] }));
        hud.observe(w, &[hit(1, 0, 1, 5.0)], 1.0, 1.0);
        assert_eq!(texts(&hud), [("Air Juggle x1|Combo Damage: 5".to_owned(), [0.7, 0.2, 0.0])]);
        // both: the juggle widget given its new values, the hitstun one below it; the bot's hit shows nothing
        hud.set_lab_combos(Some(LabCombos { juggle: true, hitstun: true, slot_of: vec![0, 1] }));
        hud.observe(w, &[hit(1, 0, 2, 1234.0), hit(0, 1, 1, 5.0)], 1.5, 1.5);
        assert_eq!(
            texts(&hud),
            [
                ("Air Juggle x2|Combo Damage: 1,234".to_owned(), [0.7, 0.2, 0.0]),
                ("Hitstun Combo x3|Combo Damage: 2,234".to_owned(), [0.7, 0.3, 0.0]),
            ]
        );
        // the hitstun text alone, at the first fighter's place by its slot
        hud.set_lab_combos(Some(LabCombos { juggle: false, hitstun: true, slot_of: vec![1, 0] }));
        hud.observe(w, &[hit(1, 0, 3, 7.0)], 2.0, 2.0);
        assert_eq!(texts(&hud)[1], ("Hitstun Combo x4|Combo Damage: 1,007".to_owned(), [0.3, 0.2, 0.0]));
        assert!(hud.pops.pops().iter().all(|t| t.p.place == Place::Viewport && t.p.lifetime == 1.5));
    }
}
