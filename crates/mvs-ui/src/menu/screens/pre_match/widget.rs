//! UI_PreMatch's controller: the character grid, the
//! players' states, the timer, the confirm, the perk page and the reveal, and the match start.
//!
//! Offline only: the queue (matchmaking, the party, the found match and the return from the Lab) and the online
//! rematch are owed with the online menus and are not here; the screen runs the custom game's rematch path
//! (bIsRematch and bIsCustomGame both true) and the arcade's (a run's first entry and its rematches, the run in
//! `Session::arcade`). The calls made through hooks (the previews, the
//! level's transitions, the voices, the announcer, the end of the screen) go out as scene commands, `PreMatchCall`s
//! and an `Exit` that the screen's glue takes (`take_calls`, `take_exit`).
//!
//! Deviation: the level's world points (CheckMark1..4, PerkInfo1..4) are projected by the program once a frame
//! (`set_projection`); the widget places its plates and cards at the projection it last got where the build projects at
//! the moment of the call.

use std::any::Any;
use std::cell::RefCell;
use std::collections::HashMap;

use mvs_render::menuscene::{PreMatchSkinData, SeqCmd};
use mvs_settings::Settings;
use serde_json::json;

use super::child_widgets::{
    InputLabelSet, PMConfirm, PMEditEmotes, PMGameplayTips, PMModeLabel, PMPlayerInfo, PMSkinSelector, PMStateInfo,
};
use super::child_widgets::{ROW_DEFAULT, ROW_REMATCH};
use super::setup::{AnnouncerLine, Exit, PreMatchCall, spec_of};
use super::setup::{PreMatchPlayer, PreMatchSetup};
use crate::bundle::{RosterEntry, SkinInfo};
use crate::menu::arcade::BOT_ID;
use crate::menu::controller::{Controller, Cx, Handler};
use crate::menu::lobby::PreMatchPick;
use crate::menu::lobby::loadout::{BadgeRef, local_badges, picked_skin, same_skin_name};
use crate::menu::manager::{DialogParams, Manager, TransitionParams};
use crate::menu::online::{NetPick, OnlineCmd, OnlineEvent, PartyRole, PreMatchCmd, PreMatchEvent, Registered};
use crate::menu::online_text::kick_text;
use crate::menu::own_profile::{BannerIconOf, banner_icon_texture};
use crate::menu::scene::SceneCmd;
use crate::menu::screens::lab_selectors::{
    GridItemList, PMCharacterInfo, TileData, TileEvent, run_on, run_tile, selected_skin, set_opacity, set_vis,
};
use crate::menu::screens::move_list::{show_movelist, social_panel_uses_shortcut};
use crate::menu::stat_trackers::stat_tracker_count;
use crate::menu::view::MenuView;
use crate::menu::widgets::common::{apply_founder_name_treatment, fill_page, page_frame_offset};
use crate::menu::widgets::controls::buttons::{ButtonMain, ButtonSecondary};
use crate::menu::widgets::controls::grid_item::GridItemState;
use crate::menu::widgets::controls::input_label::InputLabelParams;
use crate::menu::widgets::controls::interactor::{self, batch_set_context, set_interactable};
use crate::menu::widgets::controls::{bind, find, wd};
use crate::menu::widgets::nav::{self, GlobalNav};
use crate::menu::widgets::perks::{
    LoadoutSelection, PMEditPerks, PerksEquipper, PerksLoadout, perk_data, perk_pictures, remembered_perks,
    show_loadout,
};
use crate::menu::widgets::social::SocialPanel;
use crate::slate::{Vis, WidgetId};

// ---------- the screen's session ----------

thread_local! {
    /// preMatchSession.setup: what the screen is given, set by the flow before UI_PreMatch is created and None at the
    /// end.
    static SESSION: RefCell<Option<PreMatchSetup>> = const { RefCell::new(None) };
}

/// preMatchSession.setup = `setup`.
pub fn set_session(setup: Option<PreMatchSetup>) {
    SESSION.with(|s| *s.borrow_mut() = setup);
}

/// preMatchSession.setup.
pub fn session() -> Option<PreMatchSetup> {
    SESSION.with(|s| s.borrow().clone())
}

/// lastChosen: the SaveGame slot "PreviewCharacter" (Save Chosen Characters PM@113061), a roster slug; None before
/// one is saved.
pub fn last_chosen(prefs: &Settings) -> Option<String> {
    let c = &prefs.profile.last_character;
    (!c.is_empty()).then(|| c.clone())
}

/// lastChosen = `slug`, kept in the settings file with the rest (the build's save slot is the machine's too).
pub fn set_last_chosen(prefs: &mut Settings, slug: String) {
    prefs.profile.last_character = slug;
}

// ---------- the states ----------

// UI_PreMatchStateEnum byte values (the enum asset's Names map)
/// The queue path's words (Game.locres keys of their text consts): the headers "Waiting for Party Member" PM@44008,
/// "Matchmaking" PM@41000 and "Match Found!" PM@107827, the cancel button's "Preparing Matchmaking" PM@35737 and
/// "Cancel Matchmaking" PM@41802, the wait's "Cancelling Matchmaking" PM@70325.
const TEXT_WAITING_PARTY: &str = "95E574A740310F99692078A3C2453D67";
const TEXT_MATCHMAKING: &str = "B9AC9A8948C3ACDF8B3E5DA5E4A82CAA";
const TEXT_MATCH_FOUND: &str = "5E1C1B8C4D63E7920B6ED3AE10B6606C";
const TEXT_PREPARING_MM: &str = "89F1994547FAFB5E9F1607B86A14C46C";
const TEXT_CANCEL_MM: &str = "0763A83D4677C16C179919B5EC02ADFA";
const TEXT_CANCELLING_MM: &str = "F9C536F345D5FE4C0B530B920032D2AA";
/// The other headers' words: "Select a Fighter" PM@46519, "Waiting for Other Players" PM@44090 (state 4) and
/// PM@39880 (state 7), "Revealing Perks" PM@36602, "Waiting" PM@36020, "Select Perks" PM@64955, "MATCH IS
/// STARTING..." PM@28613; the kick's "A player has disconnected." PM@18238 / PM@84306; the input labels' "Select"
/// PM@94320 and "Back" PM@93734.
const TEXT_SELECT_FIGHTER: &str = "7FE7EADE459F5AA350AB61AD9F9F0BD4";
const TEXT_WAITING_OTHERS: &str = "8DC040ED435F6AF33DC7679AC245E896";
const TEXT_WAITING_OTHERS_PERKS: &str = "8912C0694D4E1C7CDB032BA7F8B781DD";
const TEXT_REVEALING_PERKS: &str = "E5260A5249A0EB1FA2E5DDB74D046BE1";
const TEXT_WAITING: &str = "6A7F5D6044CB93A3FB55D595BE2E0675";
const TEXT_SELECT_PERKS: &str = "1407C68A42494F0E8D45BB8A7C9F24C9";
const TEXT_MATCH_STARTING: &str = "DEAA49E84B73B979A873AC8736B033C5";
const TEXT_DISCONNECTED: &str = "03E841C5447AF60E6ABDEFB32E457A13";
const TEXT_LABEL_SELECT: &str = "708DDA7A452649A6E26B3C9C2A805AF2";
const TEXT_LABEL_BACK: &str = "4F74DBD2421A43C46631479F0400E605";
/// Update Global Nav Exit Button's tooltips by state PM@111915-112769 (0-10; 11 has none): "Return Home" in 0, 1
/// and an arcade rematch's 2 (`NAV_EXIT_ARCADE_REMATCH`), "Back" in the rest, each its own text const.
const NAV_EXIT_TEXT: [&str; 11] = [
    "A6B60EDD44AE542DCC64728BB55E7931",
    "1C96E0A84BB92B173217F580DFE3BFEC",
    "015139DA4F7F1805B8FC0F9120463B49",
    "A9EA7408449015A9A2D24A91F7416B4D",
    "1C706FF14605BE05DD2A818E3B20124D",
    "A375BA6243E52E024D46C9BEEB86C9F7",
    "CACFB2534096B74DCC5881B55F1CCA00",
    "E88AF4E74BA955B79B6DECBC353C0F93",
    "C95D32F5408A75A46E6476819E59FA89",
    "8616455B4A41C530540AA5A64CF5CED3",
    "F27F67974A5C3C682BFFE1BA92C07900",
];
const NAV_EXIT_ARCADE_REMATCH: &str = "C03D7CFA4194807805C1C4911B652D6A";

const LOADING: i32 = 0;
const CHOOSING_CHARACTER: i32 = 1;
const CHOOSING_SKIN: i32 = 2;
const CONFIRMING_CHARACTER: i32 = 3;
const CHARACTER_CONFIRMED: i32 = 4;
const MATCHMAKING: i32 = 5;
const CHOOSING_PERKS: i32 = 6;
const PERKS_CONFIRMED: i32 = 7;
const REVEALING_PERKS: i32 = 8;
const READY: i32 = 9;
const IDLE: i32 = 10;
const STARTING_MATCHMAKING: i32 = 11;

// ShowPerksNow's SetInfo PM@64955, the lock backstop PM@58233, OnAllPlayersLockedPerks' wait PM@78163 and the reveal
// PM@39585 (not the arcade)
const PERK_TIME_S: f64 = 40.0;
const PERK_BACKSTOP_S: f64 = 15.0;
// RematchTimeoutHandle PM@51907: a rematch's wait for every pick after the local timer's lock
const REMATCH_TIMEOUT_S: f64 = 15.0;
const PERKS_LOCKED_WAIT_S: f64 = 2.0;
const ARCADE_REMATCH_PERKS_LOCKED_WAIT_S: f64 = 0.2;
const REVEAL_S: f64 = 7.0;
const ARCADE_REVEAL_S: f64 = 5.0;
// state 0's Delay PM@49770: 0.1 s in arcade, 1.5 s otherwise
const LOADING_S: f64 = 1.5;
const ARCADE_LOADING_S: f64 = 0.1;
/// The solo Leave Arcade? dialog (PM@71585-71996): title, body, Yes, No.
const LEAVE_ARCADE: [&str; 4] = [
    "FBCC9F9342939533CE2551AB7A59CA2B",
    "87D3C6274F15E72F82FD218013075375",
    "3CE05CA54C9809647363DAB7CEDAF071",
    "7CD1B5F2421F206C25899EBC843D938D",
];
// The bots' perk lock: CoopVsAIManager OnPlayersReadyForMatchmaking CO@6475 -> PerkSelectDelay 5.0 s -> ReadyBots
// CO@2827 (AICharacterSelector's OnPerksSelectionNeeded is never raised natively)
const BOT_PERKS_S: f64 = 5.0;

/// The perk loadouts: P{i} is the player of Player Info i (GetWidgetsForIndex PM, the Account IDs it gives them).
const LOADOUTS: [&str; 4] = ["UI_Perks_LoadoutP0", "UI_Perks_LoadoutP1", "UI_Perks_LoadoutP2", "UI_Perks_LoadoutP3"];
const ALLY_PREVIEW: &str = "UI_Perks_LoadoutAllyPreview";
/// The widget each level point places: PerkLocations[i] ("PerkInfo{i+1}") the loadout [P0, P2, P1, P3][i]
/// (ApplyPerkLocations PM@50993), CheckmarkLocations[i] the Player Info of the same index (PM@12511).
const LOCATION_WIDGET: [usize; 4] = [0, 2, 1, 3];

/// The reveal's loadouts by team style PM@38842-38979.
fn revealed(team_style: u8) -> &'static [usize] {
    match team_style {
        0 => &[1],
        1 => &[1, 3],
        _ => &[1, 2, 3],
    }
}

/// Deviation: the "PerkInfo1..4" points come from the level (BPI_SendWorldLocation PM@100567-100984), whose scene
/// does not send them, so none arrives. The port's stand-in points, in PerkLocations order (P0, P2, P1, P3), canvas
/// units of the 2560 x 1440 box, by team style.
const PERK_STAND_IN: [[[f64; 2]; 4]; 3] = [
    [[640.0, 900.0], [1920.0, 900.0], [1920.0, 900.0], [1920.0, 900.0]],
    [[360.0, 640.0], [920.0, 640.0], [1640.0, 640.0], [2200.0, 640.0]],
    [[360.0, 900.0], [920.0, 900.0], [1640.0, 900.0], [2200.0, 900.0]],
];

/// HideAllWidgets PM@31892 (the names of the port's bundle).
const HIDE_LIST: [&str; 21] = [
    "CharacterGrid",
    "CancelMM_Ghost",
    "UI_PM_SkinSelectorP0",
    "UI_PM_ConfirmP0",
    "EditPerksP0",
    "ShowMoveListP0",
    "UI_PM_ReadyUp_0",
    "UI_PM_ReadyUp__2",
    "UI_PM_ReadyUp__1",
    "UI_Perks_Equipper_P0",
    "UI_Perks_LoadoutAllyPreview",
    "UI_PM_CharacterInfo_V2_P0",
    "UI_PM_GameplayTips",
    "buttonClaim",
    "UI_PM_Cosmetics",
    "UI_PM_BoostInfo",
    "UI_RestedXPBar",
    "CharGridShadow",
    "DoubleRewardContainer",
    "UI_Perks_AllyUnlockedInfo",
    "perksBGDimmer",
];

/// Each state's exceptions to HideAllWidgets PM@33274-33295.
fn hide_except(state: i32) -> &'static [&'static str] {
    match state {
        1..=3 => &["DoubleRewardContainer"],
        4 | 5 | 11 => &["EditPerksP0", "ShowMoveListP0", "UI_Perks_Equipper_P0", "DoubleRewardContainer"],
        6..=9 => &["UI_Perks_LoadoutAllyPreview"],
        _ => &[],
    }
}

/// A queue bot's profile icon by its account id (bot1..bot3): PGI_CoOpVsAIManager SetBotProfileIcons (@0-694)
/// writes BotProfileIcons[BotAccountIDs[i]] (class defaults) on each bot's Hydra user, which FetchHydraUser keeps
/// for the pre-match's BP_Banner_PM Set Profile Icon @415. Unverified: BotAccountIDs follow GetRandomBotLoadouts'
/// order (@5302), read as the server's bot1..bot3.
fn queue_bot_icon(account_id: &str) -> Option<&'static str> {
    match account_id {
        "bot1" => Some("ProfileIcon_Reindog_1"),
        "bot2" => Some("ProfileIcon_CN_AT_IceKing_1"),
        "bot3" => Some("ProfileIcon_DC_BAT_Batman_1"),
        _ => None,
    }
}

// UpdateInputLabels PM@93665: "Back" on UICancel, "Select" on UISubmit
fn label(action: &str, text: &str) -> InputLabelParams {
    InputLabelParams {
        action: action.to_string(),
        modifier: "None".into(),
        label: text.to_string(),
        player_index: 0,
        automatic_input_type: true,
        fixed_input_type: 0,
        tint: [0.68, 0.8, 0.85, 1.0],
        text_size: 14.0,
        key_size: 16.0,
        icon_size: [32.0, 32.0],
        custom_icon: None,
    }
}

/// The face-off's Seq_AnimInRed / Seq_AnimInFFA_New event times (free for all: the second list).
const FACE_OFF_TIMES: [(&str, f64); 4] = [("2", 0.0), ("3", 1.2167), ("1", 1.3), ("ShowVS", 1.4667)];
const FACE_OFF_TIMES_FFA: [(&str, f64); 4] = [("2", 1.2167), ("3", 1.3167), ("1", 1.3833), ("ShowFFA", 1.4667)];

// The handlers of the events the screen binds (`handle`) and of its delays (`resume`).
const H_TIME_EXPIRED: u32 = 1;
const H_GRID_FOCUSED: u32 = 2;
const H_GRID_SELECTED: u32 = 3;
const H_GRID_POPULATED: u32 = 4;
const H_SKIN_FOCUSED: u32 = 5;
const H_SKIN_SELECTED: u32 = 6;
const H_CONFIRM: u32 = 7;
const H_BACK_TO_LOBBY: u32 = 8;
const H_EDIT_PERKS: u32 = 9;
const H_MOVE_LIST: u32 = 10;
const H_CANCEL_MM: u32 = 11;
const H_CLICKABLE: u32 = 12;
const H_SUBMIT2: u32 = 13;
const H_SUBMIT3: u32 = 14;
const H_CANCEL: u32 = 15;
const H_EQ_SAVED: u32 = 16;
const H_EQ_ENTERED: u32 = 17;
const H_EQ_EXITED: u32 = 18;
const H_NAV_EXIT: u32 = 19;
const H_LEAVE_ARCADE: u32 = 20;
const H_WAIT_LAB: u32 = 21;
const H_MAP_REVEAL: u32 = 22;
const H_CLICKABLE_FOCUS: u32 = 23;
const H_CLICKABLE_UNFOCUS: u32 = 24;
const H_EQ_PERK: u32 = 25;
const H_EQ_CLEARED: u32 = 26;

const D_INIT: u32 = 100;
const D_LOADING: u32 = 101;
const D_CLICK_VO: u32 = 102;
const D_NAV: u32 = 103;
const D_GRID_WAIT: u32 = 104;
const D_GRID_FOCUS: u32 = 105;
const D_TO_SKIN: u32 = 106;
const D_CONFIRM_VO: u32 = 107;
const D_REMATCH_TIMEOUT: u32 = 108;
const D_PERKS_FOCUS: u32 = 109;
const D_PLAYERS_FOUND: u32 = 110;
const D_FACE_OFF_DONE: u32 = 111;
const D_TO_PERKS: u32 = 112;
const D_REVEAL: u32 = 113;
const D_ALL_PERKS: u32 = 114;
const D_BACKSTOP: u32 = 115;
const D_READY_1: u32 = 116;
const D_CLASH_IMPACT: u32 = 117;
const D_CLASH_FLASH: u32 = 118;
const D_START: u32 = 119;
const D_PLAYERS_READY: u32 = 120;
const D_START_PERKS: u32 = 121;
const D_REFRESH_STACKS: u32 = 122;
/// A delay of a player: the id plus its index in `sels`.
const D_BOT_LOCK_A: u32 = 0x200;
const D_BOT_LOCK_B: u32 = 0x300;
const D_BOT_PERKS: u32 = 0x400;
/// A face-off sequence event: the id plus its index in the times list.
const D_SEQ: u32 = 0x500;
/// The no-response timer of a cancel (the count of the cancel in the low byte).
const D_CANCEL_NR: u32 = 0x600;
/// CancelMatchmakingNoResponse's Delay PM@52622.
const CANCEL_NO_RESPONSE_S: f64 = 80.0;

// ---------- the players ----------

/// A player of RematchPlayerData with the widget index MapPlayersToWidgets gives it and its selections.
#[derive(Clone, Debug)]
struct Sel {
    /// The player's index in the set-up.
    player: usize,
    /// The Player Info it fills (-1 none: a spectator).
    widget: i32,
    character: Option<RosterEntry>,
    skin: String,
    locked: bool,
    /// bPerksLocked and the selection's Perks (the four slots, an empty one None).
    perks_locked: bool,
    perks: Vec<Option<String>>,
}

/// MapPlayersToWidgets(Players): self -> widget 0, a team-mate -> 2, enemies from 1 on (step 2 in 2v2); a bot's
/// selection is its row's fighter, or one of the ported ones for a row whose fighter the port does not have
/// (`random` rolls it).
#[cfg(test)]
fn build_sels(setup: &PreMatchSetup, roster: &[RosterEntry], random: &mut dyn FnMut() -> f64) -> Vec<Sel> {
    build_sels_keep(setup, roster, random, None)
}

/// MapPlayersToWidgets with the local selection to carry over (`keep`: the queue's found players, PM@114147).
/// A queue's found player names its pick; online the server resolved a bot's fighter to a ported one (no roll, which
/// would differ between pages).
fn build_sels_keep(
    setup: &PreMatchSetup,
    roster: &[RosterEntry],
    random: &mut dyn FnMut() -> f64,
    keep: Option<&Sel>,
) -> Vec<Sel> {
    let queue = setup.net.as_ref().is_some_and(|n| n.queue.is_some());
    let online_rematch = setup.net.as_ref().is_some_and(|n| n.queue.is_none());
    let mine = setup.players[setup.me].team;
    let have: Vec<&RosterEntry> = roster.iter().filter(|r| r.fighter.is_some()).collect();
    let mut next = 1;
    let mut out = Vec::new();
    for (i, player) in setup.players.iter().enumerate() {
        if i == setup.me
            && let Some(k) = keep
        {
            out.push(Sel {
                player: i,
                widget: 0,
                character: k.character.clone(),
                skin: k.skin.clone(),
                locked: k.locked,
                perks_locked: false,
                perks: Vec::new(),
            });
            continue;
        }
        let widget = if i == setup.me {
            0
        } else if player.team == 4 {
            -1
        } else if player.team == mine && setup.team_style != 2 {
            2
        } else {
            let w = next;
            next += if setup.team_style == 1 { 2 } else { 1 };
            w
        };
        // a bot's selection (botSelects): its row's fighter, or one of the ported ones
        let named = player.is_bot || (queue && i != setup.me);
        let roll = player.is_bot && !online_rematch;
        let entry = if named {
            have.iter().find(|r| r.fighter == player.character).map(|r| (*r).clone()).or_else(|| {
                if !roll || have.is_empty() {
                    return None;
                }
                let at = ((random() * have.len() as f64).floor() as usize).min(have.len() - 1);
                Some(have[at].clone())
            })
        } else {
            None
        };
        let skin = match &entry {
            Some(e) if e.fighter == player.character => player.skin.clone(),
            _ => String::new(),
        };
        out.push(Sel {
            player: i,
            widget,
            character: entry,
            skin,
            locked: false,
            perks_locked: false,
            perks: Vec::new(),
        });
    }
    out
}

/// A bot's perks: SimulatePerkUnlockSystem at level 8 over the character's recommended
/// perks (the signature at slot 0, the general ones in order at slots 1-3).
fn unlock_slots(signature: Option<String>, general: &[String]) -> Vec<Option<String>> {
    vec![signature, general.first().cloned(), general.get(1).cloned(), general.get(2).cloned()]
}

/// lobbyBotPerks(RECOMMENDED_PERKS, fighter). Deviation: the build's custom-lobby bots take none; a queue bot's, the
/// full unlock for the character it plays. Unverified: the table's IsEnabledInDevelopmentEnvironment filter
/// (GetRecommendedPerks @336) is not in the bundle: every recommended perk counts; the last signature wins (@427-553).
fn lobby_bot_perks(view: &MenuView, fighter: &str) -> Vec<Option<String>> {
    let rec = view.fighters().get(fighter).and_then(|f| f.perks.as_ref()).map(|p| p.recommended.clone());
    let mut signature = None;
    let mut general = Vec::new();
    for id in rec.unwrap_or_default() {
        if perk_data(view, Some(&id)).is_some_and(|p| p.info.category == 3) {
            signature = Some(id);
        } else {
            general.push(id);
        }
    }
    unlock_slots(signature, &general)
}

/// The dev view's set-up (?menu=UI_PreMatch): a 1v1 against a bot.
fn stand_in(cx: &Cx<'_>) -> PreMatchSetup {
    PreMatchSetup {
        players: vec![
            PreMatchPlayer {
                account_id: "local".into(),
                username: "Player".into(),
                team: 0,
                is_bot: false,
                ..PreMatchPlayer::default()
            },
            PreMatchPlayer {
                account_id: "bot-1".into(),
                username: String::new(),
                team: 1,
                is_bot: true,
                character: cx.view.fighters().keys().next().cloned(),
                ..PreMatchPlayer::default()
            },
        ],
        me: 0,
        team_style: 0,
        leader: true,
        map: cx.view.lab_map().map(|m| m.name),
        level: String::new(),
        set_score: None,
        arcade: None,
        net: None,
    }
}

/// A canvas point's id: (a PerkInfo, its index 0-3). The ids are /^PerkInfo([1-4])$/i and /^CheckMark([1-4])$/.
fn point_id(id: &str) -> Option<(bool, usize)> {
    let (perk, rest) = if id.len() > 8 && id[..8].eq_ignore_ascii_case("PerkInfo") {
        (true, &id[8..])
    } else {
        (false, id.strip_prefix("CheckMark")?)
    };
    let n: usize = rest.parse().ok()?;
    if rest.len() == 1 && (1..=4).contains(&n) { Some((perk, n - 1)) } else { None }
}

fn point_key(perk: bool, i: usize) -> String {
    if perk { format!("PerkInfo{}", i + 1) } else { format!("CheckMark{}", i + 1) }
}

fn set_vis_all(cx: &mut Cx<'_>, names: &[&str], v: Vis) {
    for n in names {
        set_vis(cx, n, v);
    }
}

/// IsVisible(): the widget's own visibility.
fn shown(cx: &mut Cx<'_>, w: WidgetId) -> bool {
    !matches!(wd(cx, w).vis, Vis::Collapsed | Vis::Hidden)
}

/// UI_Interactor of a button child.
fn interactor_of(cx: &Cx<'_>, w: WidgetId) -> WidgetId {
    cx.view.tree().find_in(w, "UI_Interactor")
}

/// UI_PreMatch.
#[derive(Debug)]
pub struct PreMatch {
    pub state: i32,
    setup: PreMatchSetup,
    sels: Vec<Sel>,
    /// CanBack?: true on the custom game path.
    can_back: bool,
    rematch_time: f64,
    previewed: Option<RosterEntry>,
    /// The selector's last valid previewed character (a fighter the port has).
    last_valid: Option<RosterEntry>,
    previewed_skin: String,
    /// The last valid previewed skin, with `last_valid` (an invalid pick falls back to both).
    last_valid_skin: String,
    all_characters_chosen: bool,
    found_once: bool,
    ready_once: bool,
    kicked: bool,
    /// The Delay(0.25) of VO_Clickable's OnClick PM@114939 is running, and the preview widget it was clicked on.
    click_vo_pending: bool,
    click_vo_widget: i32,
    diamond_once: bool,
    grid_bound: bool,
    /// UI_GridItemList's per-player flags, kept here: the port's list has the Lab's paths only.
    grid_enabled: bool,
    grid_interactable: bool,
    grid_has_focused: bool,
    shadow_start: f64,
    /// The level's world points by id (CheckMark1..4, PerkInfo1..4) and their projections the program last gave.
    points: HashMap<String, [f64; 3]>,
    projection: HashMap<String, Option<[f64; 2]>>,
    /// The banners' nametag widgets (WBP_3dText) by preview slot: the widget and its index among the view's world
    /// widgets.
    plates: Vec<(WidgetId, usize)>,
    /// PlayersFound was sent (the level hides PartnerNameplate_PM there, PreMatch @27058).
    players_found: bool,
    checkmarks_dirty: bool,
    perks_dirty: bool,
    all_perks_selected: bool,
    /// ShowPerksNow's fade in of the header (PM@65284): its start time.
    header_fade_start: f64,
    equipper_bound: bool,
    /// Which of the loadouts (P0-P3, the ally preview) were used.
    loadout_built: [bool; 5],
    backstop_pending: bool,
    nav: Option<WidgetId>,
    social: Option<WidgetId>,
    frame_offset: [f64; 2],
    /// A level is behind the screen (SceneCmds are taken): its sequences time the face-off and the clash. Off, the
    /// screen runs the decoded times itself.
    level: bool,
    exit: Option<Exit>,
    calls: Vec<PreMatchCall>,
    /// The picture owners held for the perk page and the cards.
    held: Vec<String>,
    /// The queue's: MatchFound, RequestedMatchmakingCancel, the matchmaking handler's MatchmakingStartTime (PM@66969)
    /// and CancelMatchmakingTimerHandle (a count: a timer whose count moved is cleared).
    match_found: bool,
    requested_cancel: bool,
    matchmaking_start: f64,
    cancel_timer: u32,
    /// The queue's match.start perks by slot, held until state 6 (PreMatch.allPlayersLockedPerks).
    start_perks: Option<Vec<Vec<String>>>,
    map_reveal_bound: bool,
}

impl PreMatch {
    /// The controller of UI_PreMatch.
    pub fn make(_cx: &mut Cx<'_>) -> Box<dyn Controller> {
        Box::new(PreMatch {
            state: IDLE,
            setup: PreMatchSetup::default(),
            sels: Vec::new(),
            can_back: true,
            rematch_time: 0.0,
            previewed: None,
            last_valid: None,
            previewed_skin: String::new(),
            last_valid_skin: String::new(),
            all_characters_chosen: false,
            found_once: false,
            ready_once: false,
            kicked: false,
            click_vo_pending: false,
            click_vo_widget: 0,
            diamond_once: false,
            grid_bound: false,
            grid_enabled: false,
            grid_interactable: true,
            grid_has_focused: false,
            shadow_start: -1.0,
            points: HashMap::new(),
            projection: HashMap::new(),
            plates: Vec::new(),
            players_found: false,
            checkmarks_dirty: false,
            perks_dirty: false,
            all_perks_selected: false,
            header_fade_start: -1.0,
            equipper_bound: false,
            loadout_built: [false; 5],
            backstop_pending: false,
            nav: None,
            social: None,
            frame_offset: [0.0, 0.0],
            level: true,
            exit: None,
            calls: Vec::new(),
            held: Vec::new(),
            match_found: false,
            requested_cancel: false,
            matchmaking_start: 0.0,
            cancel_timer: 0,
            start_perks: None,
            map_reveal_bound: false,
        })
    }

    // ---------- what the screen's glue calls ----------

    /// A level is behind the screen (true: its transition goes out as `SceneCmd::Sequence(SeqCmd::Transition)` and
    /// its sequences call `sequence_event` / `transition_complete`); false: the screen's timed fallback, as when the
    /// level's scene failed.
    pub fn set_level(&mut self, level: bool) {
        self.level = level;
    }

    /// BPI_SendWorldLocation(id, Location) PM@100347 / PM@100567-100984: the world point is kept and projected at
    /// each place the widget uses it (`set_projection`). Unverified: without the scene no location arrives and the
    /// plates keep their tree slots.
    pub fn send_world_location(&mut self, id: &str, at: [f64; 3]) {
        let Some((perk, i)) = point_id(id) else { return };
        self.points.insert(point_key(perk, i), at);
        if perk {
            self.perks_dirty = true;
        } else {
            self.checkmarks_dirty = true;
        }
    }

    /// The point `id` as the program projected it now: canvas units of the 2560 x 1440 box, None off the screen.
    pub fn set_projection(&mut self, id: &str, at: Option<[f64; 2]>) {
        let Some((perk, i)) = point_id(id) else { return };
        self.projection.insert(point_key(perk, i), at);
    }

    /// The nametag widget of each preview slot (BP_Banner_PM NametagWidget, Space Screen, draw size 370 x 100), made
    /// with the level's banners: the player's name (WBP_3dText SetName T3D @15-310: PlayerName, then the founder
    /// treatment) and, on a 1v1 / 2v2 banner of Player3 or Player4 (Set User Data: neither Player1 nor Player2),
    /// FlipIcons. Deviation: FlipIcons (WBP_3dText @524) is not applied: the icons keep their order.
    fn set_plates(&mut self, cx: &mut Cx<'_>, names: &[String]) {
        if self.plates.is_empty() {
            for _ in 0..names.len() {
                let w = MenuView::create(cx, "WBP_3dText");
                let i = MenuView::add_world_widget(cx, w);
                cx.view.world[i].size = Some([370.0, 100.0]);
                self.plates.push((w, i));
            }
        }
        for (slot, name) in names.iter().enumerate() {
            let (w, _) = self.plates[slot];
            let label = cx.view.tree().find_in(w, "PlayerName");
            wd(cx, label).text = name.clone();
            apply_founder_name_treatment(cx, label);
        }
    }

    /// Whether PlayersFound was sent to the level (the partner nameplate hides there, PreMatch @27058).
    pub fn players_found_sent(&self) -> bool {
        self.players_found
    }

    /// The plates' world widget indices by preview slot, for the program to place.
    pub fn plate_widgets(&self) -> Vec<usize> {
        self.plates.iter().map(|p| p.1).collect()
    }

    /// The preview slot (banner) of each player of the set-up by its index there, the found match's on a queue
    /// (None: a spectator), and this player's index; None before MapPlayersToWidgets ran.
    pub fn preview_slots(&self) -> Option<(Vec<Option<usize>>, usize)> {
        if self.sels.is_empty() {
            return None;
        }
        let mut slots = vec![None; self.setup.players.len()];
        for s in self.sels.iter().filter(|s| (0..=3).contains(&s.widget)) {
            if let Some(v) = slots.get_mut(s.player) {
                *v = Some(self.preview_index(s.widget));
            }
        }
        Some((slots, self.setup.me))
    }

    /// The points the level sent, for the program to project.
    pub fn world_points(&self) -> Vec<(String, [f64; 3])> {
        let mut v: Vec<(String, [f64; 3])> = self.points.iter().map(|(k, p)| (k.clone(), *p)).collect();
        v.sort_by(|a, b| a.0.cmp(&b.0));
        v
    }

    /// The screen's end, once it has one (taken).
    pub fn take_exit(&mut self) -> Option<Exit> {
        self.exit.take()
    }

    /// The calls into the voices and the announcer, in order (taken).
    pub fn take_calls(&mut self) -> Vec<PreMatchCall> {
        std::mem::take(&mut self.calls)
    }

    // ---------- the tree ----------

    fn state_info(cx: &Cx<'_>) -> WidgetId {
        find(cx, "UI_PM_StateInfo")
    }
    fn grid(cx: &Cx<'_>) -> WidgetId {
        find(cx, "CharacterGrid")
    }
    fn confirm_w(cx: &Cx<'_>) -> WidgetId {
        find(cx, "UI_PM_ConfirmP0")
    }
    fn skin_selector_w(cx: &Cx<'_>) -> WidgetId {
        find(cx, "UI_PM_SkinSelectorP0")
    }
    fn mode_label_w(cx: &Cx<'_>) -> WidgetId {
        find(cx, "UI_PM_ModeLabel")
    }
    fn back_to_lobby_w(cx: &Cx<'_>) -> WidgetId {
        find(cx, "Btn_BackToCustomLobby")
    }
    fn player_info_w(cx: &Cx<'_>, i: usize) -> WidgetId {
        find(cx, &format!("UI_PM_PlayerInfo_{i}"))
    }
    fn cancel_button_w(cx: &Cx<'_>) -> WidgetId {
        find(cx, "CancelMM_Ghost")
    }
    fn wait_in_lab_w(cx: &Cx<'_>) -> WidgetId {
        find(cx, "Button_WaitInLab")
    }

    fn me(&self) -> &Sel {
        &self.sels[self.setup.me]
    }

    fn me_mut(&mut self) -> &mut Sel {
        let at = self.setup.me;
        &mut self.sels[at]
    }

    fn is_me(&self, i: usize) -> bool {
        i == self.setup.me
    }

    /// a rematch's 45 s formula.
    /// "Is Arcade Rematch?" (fn @46-129): IsArcadeModeActive (PreMatchSetup.arcade) and bIsRematch (its rematch).
    fn arcade_rematch(&self) -> bool {
        self.setup.arcade.as_ref().is_some_and(|r| r.rematch)
    }

    /// A state's time limit: the rematch's 45 s formula, a queue's 0 (PM@46473 / PM@43221 not a rematch).
    fn time_left(&self, cx: &Cx<'_>) -> f64 {
        if self.queue() {
            return 0.0;
        }
        (45.0 - (cx.now() - self.rematch_time)).trunc()
    }

    /// The queue's path (PreMatchSetup.queue).
    fn queue(&self) -> bool {
        self.setup.net.as_ref().is_some_and(|n| n.queue.is_some())
    }

    /// The rematch path online (PreMatchSetup.online): a custom lobby's start or a queue set's rematch.
    fn online_rematch(&self) -> bool {
        self.setup.net.as_ref().is_some_and(|n| n.queue.is_none())
    }

    /// The server rules the locks: the queue's path, or the rematch path online.
    fn server(&self) -> bool {
        self.setup.net.is_some()
    }

    fn party_role(&self) -> Option<PartyRole> {
        self.setup.net.as_ref().and_then(|n| n.queue.as_ref()).and_then(|q| q.party)
    }

    /// A hook of the online paths (the program answers it; an offline screen has none).
    fn hook(&self, cx: &mut Cx<'_>, cmd: PreMatchCmd) {
        if self.server() {
            cx.online(OnlineCmd::PreMatch(cmd));
        }
    }

    /// MatchmakingHandler.bIsMatchmaking (PM@4676, PM@15790, PM@110554).
    fn is_matchmaking(cx: &Cx<'_>) -> bool {
        cx.view.session.online.pre_match.as_ref().is_some_and(|p| p.searching)
    }

    fn net_pick(c: &RosterEntry, skin: &str) -> Option<NetPick> {
        c.fighter.clone().map(|character| NetPick { character, skin: skin.to_string() })
    }

    /// The perk page (UI_Perks_Equipper_P0), its events bound at its first use.
    fn equipper(&mut self, cx: &mut Cx<'_>) -> WidgetId {
        let eq = find(cx, "UI_Perks_Equipper_P0");
        if !self.equipper_bound {
            self.equipper_bound = true;
            let me = cx.w;
            cx.visit::<PerksEquipper, _>(eq, |e, _| {
                // (the port's own) the local player's loadout is kept on this machine (perks.rs remembered_perks)
                e.remember = true;
                // InitEventBindings: PerksSavedEvent -> OnPerksLocked PM@65992
                e.perks_saved_event.add(Handler { owner: me, id: H_EQ_SAVED });
                e.entered_event.add(Handler { owner: me, id: H_EQ_ENTERED });
                e.exited_event.add(Handler { owner: me, id: H_EQ_EXITED });
                e.perk_equipped_event.add(Handler { owner: me, id: H_EQ_PERK });
                e.cleared_event.add(Handler { owner: me, id: H_EQ_CLEARED });
            });
            self.apply_equipper_props(cx);
        }
        eq
    }

    /// Loadout `i` (0-3) or the ally preview (4), used from here on.
    fn loadout(&mut self, cx: &mut Cx<'_>, i: usize) -> WidgetId {
        let lo = find(cx, if i < 4 { LOADOUTS[i] } else { ALLY_PREVIEW });
        self.loadout_built[i] = true;
        if i == 0 {
            cx.visit::<PerksLoadout, _>(lo, |l, _| l.remember = true);
        }
        lo
    }

    /// The used loadouts (P0-P3, the ally preview).
    fn built_loadouts(&self, cx: &Cx<'_>) -> Vec<WidgetId> {
        (0..5)
            .filter(|&i| self.loadout_built[i])
            .map(|i| find(cx, if i < 4 { LOADOUTS[i] } else { ALLY_PREVIEW }))
            .collect()
    }

    /// The ally's Sel in 2v2.
    fn ally(&self) -> Option<usize> {
        if self.setup.team_style == 1 { self.sels.iter().position(|s| s.widget == 2) } else { None }
    }

    // ---------- the hooks ----------

    /// game mode BPI_GetPreviewActor(preview).BPI_ChangeFighter(Skin, TeamIndex, false) PM@67281 (UpdateFighterPreview).
    fn change_fighter(&self, cx: &mut Cx<'_>, preview: usize, c: &RosterEntry, skin: &str, team: u8) {
        cx.scene(SceneCmd::Preview { slot: preview, spec: spec_of(c, skin, team) });
    }

    /// BPI_PreMatchTransition(name): the level's, when one is behind the screen.
    fn transition_hook(&self, cx: &mut Cx<'_>, name: &str) {
        if self.level {
            cx.scene(SceneCmd::Sequence(SeqCmd::Transition(name.to_string())));
        }
    }

    fn team_of(&self, s: &Sel) -> u8 {
        self.setup.players[s.player].team
    }

    // ---------- OnInitialized and Construct ----------

    /// OnInitialized PM@57701 -> InitEventBindings PM@59372-60162 (the events the screen binds).
    fn bind_events(&mut self, cx: &mut Cx<'_>) {
        let me = cx.w;
        let h = |id| Handler { owner: me, id };
        let si = Self::state_info(cx);
        cx.visit::<PMStateInfo, _>(si, |s, _| s.on_time_expired.add(h(H_TIME_EXPIRED)));
        let grid = Self::grid(cx);
        cx.visit::<GridItemList, _>(grid, |g, _| {
            g.focused_event.add(h(H_GRID_FOCUSED));
            g.selected_event.add(h(H_GRID_SELECTED));
            g.populated_event.add(h(H_GRID_POPULATED));
        });
        let ss = Self::skin_selector_w(cx);
        cx.visit::<PMSkinSelector, _>(ss, |s, _| {
            s.skin_focused_event.add(h(H_SKIN_FOCUSED));
            s.skin_selected_event.add(h(H_SKIN_SELECTED));
        });
        let c = Self::confirm_w(cx);
        cx.visit::<PMConfirm, _>(c, |c, _| c.click_event.add(h(H_CONFIRM)));
        let b = Self::back_to_lobby_w(cx);
        cx.visit::<ButtonSecondary, _>(b, |b, _| b.on_click.add(h(H_BACK_TO_LOBBY)));
        // Button_WaitInLab OnClick PM@115633: no state test, no sound
        let wait = Self::wait_in_lab_w(cx);
        cx.visit::<ButtonMain, _>(wait, |b, _| b.on_click.add(h(H_WAIT_LAB)));
        let ep = find(cx, "EditPerksP0");
        cx.visit::<PMEditPerks, _>(ep, |e, _| e.click_event.add(h(H_EDIT_PERKS)));
        let ml = find(cx, "ShowMoveListP0");
        cx.visit::<PMEditEmotes, _>(ml, |e, _| e.click_event.add(h(H_MOVE_LIST)));
        let cm = Self::cancel_button_w(cx);
        cx.visit::<ButtonSecondary, _>(cm, |b, _| b.on_click.add(h(H_CANCEL_MM)));
        // VO_Clickable OnClick PM@114784
        let vo = find(cx, "VO_Clickable");
        bind(cx, vo, |s| &mut s.on_click, H_CLICKABLE);
        // VO_Clickable OnFocus / OnUnfocus PM@117915 / PM@117963
        bind(cx, vo, |s| &mut s.on_focus, H_CLICKABLE_FOCUS);
        bind(cx, vo, |s| &mut s.on_unfocus, H_CLICKABLE_UNFOCUS);
        let m = &mut cx.view.manager;
        m.on_submit2.add(h(H_SUBMIT2));
        m.on_submit3.add(h(H_SUBMIT3));
        m.on_cancel.add(h(H_CANCEL));
    }

    /// Deviation: the screen fills the window at any aspect ratio: the 2560x1440 SizeBox_2 under its ScaleBox.
    fn fill(&mut self, cx: &mut Cx<'_>) {
        let sbox = find(cx, "SizeBox_2");
        fill_page(cx, sbox);
        let [dx, dy] = page_frame_offset(cx);
        // the ally preview is laid out in the tree and moved by ShiftAllyPerkPosition's margin track, which writes its
        // offsets each frame: its render translation carries the band's offset instead
        let ally = find(cx, ALLY_PREVIEW);
        wd(cx, ally).transform.translation = [dx, -dy];
        if dx == self.frame_offset[0] && dy == self.frame_offset[1] {
            return;
        }
        self.frame_offset = [dx, dy];
        self.apply_player_info_locations(cx);
        self.apply_perk_locations(cx);
    }

    /// Init chain PM@9541 after Construct's Delay(0.2) PM@22555.
    fn init(&mut self, cx: &mut Cx<'_>) {
        // FindOrCreateGlobalNavAndSocialPanel(true, not bIsRematch) PM@22652
        let queue = self.queue();
        self.find_or_create_global_nav_and_social_panel(cx, true, queue);
        // PM@13983: bIsCustomGame AND the leader: the leader's early SetState(1) PM@14774 and the Back to Lobby button
        // PM@15010 / PM@15099. The arcade is no custom game (LaunchCoopVsAILobby MH@10780 sets bIsCustomGame false).
        let custom_leader = self.setup.leader && self.setup.arcade.is_none();
        if custom_leader {
            self.find_or_create_global_nav_and_social_panel(cx, true, true);
            self.set_state(cx, CHOOSING_CHARACTER);
        }
        let b = Self::back_to_lobby_w(cx);
        wd(cx, b).vis = if custom_leader { Vis::Visible } else { Vis::Collapsed };
        let bi = interactor_of(cx, b);
        set_interactable(cx, bi, custom_leader);
        // PM@12511 Player Infos reset; PM@11922 MapPlayersToWidgets(RematchPlayerData, false)
        for i in 0..4 {
            let pi = Self::player_info_w(cx, i);
            cx.visit::<PMPlayerInfo, _>(pi, |p, cx| {
                p.set_player_info(cx, "", "");
                p.set_character(cx, None);
            });
            // PM@13217 Overlay_0.SetRenderOpacity(0): a plate shows only when ForceShowDiamond plays ShowAnim
            let ov = cx.view.tree().find_in(pi, "Overlay_0");
            wd(cx, ov).opacity = 0.0;
        }
        self.map_players_to_widgets(cx, false);
        self.update_header_layout(cx);
        // PM@15437, then PM@16104 / PM@16531: the announcer's intro, skipped in the arcade (PM@15790-16090)
        // (the arcade's screens read the run again, not the round the last match played)
        let arcade = self.setup.arcade.is_some();
        if arcade {
            cx.view.session.arcade.drop_round();
        }
        // (the return from the Lab: the count-up goes on from the handler's MatchmakingStartTime, PM@41115)
        if let Some(r) = self.setup.net.as_ref().and_then(|n| n.queue.as_ref()).and_then(|q| q.resume.as_ref()) {
            self.matchmaking_start = r.start;
        }
        self.set_state(cx, LOADING);
        cx.sink.set_state("state_gameflow", "state_gameflow_characterselect");
        if !Self::is_matchmaking(cx) && !arcade {
            self.calls.push(PreMatchCall::Announcer(AnnouncerLine::Intro));
        }
        self.bot_selects(cx);
        // (the port's) the screen has its players: an online session's messages can reach it
        self.hook(cx, PreMatchCmd::ScreenUp);
    }

    /// The level's AICharacterSelector of each bot: the native OnCharacterSelectionNeeded @819 takes Array_Random of
    /// the class's 12 CharacterData and MakeCharacterSelection @997; OnSelectionAILoaded: Delay(0.2) @1017 ->
    /// LockInCharacter @81; OnSkinSelectionNeeded @1072: MakeSkinSelection @285, Delay(0.2) @304 -> LockInSkin @92.
    /// Unverified: when the three native events fire is not decoded: read as at the screen's init, one after the
    /// other with no time between (0.4 s to the locked skin). Whether a lobby row's bot keeps the row's character and
    /// skin or takes the selector's random one is native too: the port keeps the row's (the match plays what the
    /// lobby shows), and rolls among the ported fighters only for a row whose fighter the port does not have.
    fn bot_selects(&mut self, cx: &mut Cx<'_>) {
        // online the server locks the bots (a team-mate's lock comes as pm.locked, the rest with pm.all)
        if self.online_rematch() {
            return;
        }
        let me = cx.w;
        for (i, s) in self.sels.iter().enumerate() {
            let p = &self.setup.players[s.player];
            if !p.is_bot || p.team == 4 || s.character.as_ref().and_then(|c| c.fighter.as_ref()).is_none() {
                continue;
            }
            cx.view.delay(0.2, Handler { owner: me, id: D_BOT_LOCK_A + i as u32 });
        }
    }

    /// MapPlayersToWidgets(Players, ShowEnemySelections?): an enemy's character only with ShowEnemySelections.
    fn map_players_to_widgets(&mut self, cx: &mut Cx<'_>, show_enemies: bool) {
        self.map_players_keep(cx, show_enemies, None);
    }

    /// MapPlayersToWidgets with the local selection to carry over (the queue's found players, PM@114147).
    fn map_players_keep(&mut self, cx: &mut Cx<'_>, show_enemies: bool, keep: Option<Sel>) {
        if self.sels.is_empty() {
            let roster = cx.view.roster().to_vec();
            let mut random = || cx.view.manager.random();
            self.sels = build_sels_keep(&self.setup, &roster, &mut random, keep.as_ref());
        }
        let style = i32::from(self.setup.team_style);
        for (i, s) in self.sels.clone().into_iter().enumerate() {
            if !(0..=3).contains(&s.widget) {
                continue;
            }
            let pi = Self::player_info_w(cx, s.widget as usize);
            let p = self.setup.players[s.player].clone();
            let enemy = s.widget != 0 && s.widget != 2;
            let badges = self.badges_of(cx, i);
            cx.visit::<PMPlayerInfo, _>(pi, |info, cx| {
                info.team_style = style;
                info.set_player_info(cx, &p.account_id, &p.username);
                if !enemy || show_enemies {
                    info.set_character(cx, s.character.as_ref());
                }
                info.set_stat_badge_info(cx, &badges);
            });
        }
    }

    /// The stat badges MapPlayersToWidgets gives a plate (CharacterSelectPlayerData StatTracker*Slug / Count): a
    /// bot's as its cosmetics were drawn; the local player's own (its preferences and its counts); none for another
    /// player.
    fn badges_of(&self, cx: &mut Cx<'_>, at: usize) -> Vec<BadgeRef> {
        let p = &self.setup.players[self.sels[at].player];
        if let Some(c) = &p.net_cosmetics {
            return c.badges.iter().map(|b| BadgeRef { slug: b.slug.clone(), count: f64::from(b.count) }).collect();
        }
        if let Some(c) = &p.cosmetics {
            return c.badges.clone();
        }
        if !self.is_me(at) {
            return Vec::new();
        }
        let bundle = cx.view.bundle.clone();
        let Some(menu) = bundle.menu.as_ref() else { return Vec::new() };
        let hydra = &cx.prefs.hydra;
        // (the stats file is read only for a badge equipped)
        if hydra.stat_trackers.iter().all(|r| r.slug.is_empty()) {
            return local_badges(hydra, &menu.cosmetics, &mut |_| 0.0);
        }
        let stats = &mut cx.view.session.stats;
        let trackers = stats.data().trackers.clone();
        let hard = stats.arcade().hard_completions;
        local_badges(hydra, &menu.cosmetics, &mut |e| stat_tracker_count(e, &menu.cosmetics, &trackers, hard))
    }

    /// UpdateHeaderLayout PM@74964.
    fn update_header_layout(&self, cx: &mut Cx<'_>) {
        let solo = self.setup.team_style == 0;
        for i in 0..2 {
            let pi = Self::player_info_w(cx, i);
            wd(cx, pi).vis = Vis::Visible;
        }
        for i in 2..4 {
            let pi = Self::player_info_w(cx, i);
            wd(cx, pi).vis = if solo { Vis::Collapsed } else { Vis::Visible };
        }
        if solo {
            set_vis(cx, "playerInfoSoloSpacer", Vis::Visible);
        } else {
            set_vis(cx, "playerInfoTeamSpacer", Vis::Visible);
            set_vis(cx, "playerInfoSoloSpacer", Vis::Collapsed);
        }
    }

    /// PlayerIndexToPreviewIndex: the 1v1 opponent uses the fourth preview slot.
    fn preview_index(&self, widget: i32) -> usize {
        if widget == 1 && self.setup.team_style == 0 { 3 } else { widget.max(0) as usize }
    }

    /// HideAllWidgets PM@61072 -> PM@32932.
    fn hide_all_widgets(&self, cx: &mut Cx<'_>, except: &[&str]) {
        for n in HIDE_LIST {
            if !except.contains(&n) {
                set_vis(cx, n, Vis::Collapsed);
            }
        }
    }

    // ---------- SetState ----------

    /// SetState PM@57128.
    pub fn set_state(&mut self, cx: &mut Cx<'_>, next: i32) {
        if next == self.state {
            return;
        }
        self.state = next;
        Manager::unfocus_all(cx, 0);
        self.hide_all_widgets(cx, hide_except(next));
        if self.equipper_bound {
            self.apply_equipper_props(cx);
        }
        self.update_global_nav_exit_button(cx);
        self.update_input_labels(cx);
        self.apply_player_info_locations(cx);
        self.hide_wait_in_lab(cx);
        let si = Self::state_info(cx);
        cx.visit::<PMStateInfo, _>(si, |s, cx| {
            s.show_matchmaking_spinner(cx, false);
            s.update_match_info(cx);
        });
        self.enter_state(cx, next);
        self.update_confirm_button_state(cx);
        // PM@57603: a transition still up (the level's fade) goes on to the fade from black
        if cx.view.manager.is_transitioning() {
            Manager::transition(
                cx,
                TransitionParams { kind: 1, force: true, level_options: "Lobby".into(), ..TransitionParams::default() },
            );
        }
        let guid = cx.view.manager.new_guid();
        self.update_context(cx, &guid);
    }

    /// SetState's equipper flags: Cancel Will Confirm in the states before the perks (but 4), Ally Mode Allowed in
    /// 2v2.
    fn apply_equipper_props(&self, cx: &mut Cx<'_>) {
        let eq = find(cx, "UI_Perks_Equipper_P0");
        let s = self.state;
        let tree = cx.view.tree_mut();
        tree.set_prop(eq, "Cancel Will Confirm", json!(s <= CONFIRMING_CHARACTER || s == MATCHMAKING));
        tree.set_prop(eq, "Ally Mode Allowed", json!(self.setup.team_style == 1));
    }

    /// The state switch PM@34417.
    fn enter_state(&mut self, cx: &mut Cx<'_>, state: i32) {
        let me = cx.w;
        let si = Self::state_info(cx);
        match state {
            LOADING => {
                // PM@49633: Delay(1.5, the arcade's 0.1 PM@49770) -> the arcade's ShowArcadeLabelHeader PM@7105 and its
                // rematch PM@7142 (arcade_rematch_entry), else PM@7478: OnCharacterFocused(previewed or last chosen,
                // 0), SetState(1) PM@7701, and in arcade SetupArcadeRivals PM@7757
                cx.visit::<PMStateInfo, _>(si, |s, cx| s.set_info(cx, "", 0.0, false, 10.0, false, ""));
                let wait = if self.setup.arcade.is_some() { ARCADE_LOADING_S } else { LOADING_S };
                cx.view.delay(wait, Handler { owner: me, id: D_LOADING });
            }
            CHOOSING_CHARACTER => {
                // PM@49543 -> PM@97188 -> PM@79490: the 45 s clock starts at the first entry PM@78057; the next map
                // PM@76314; the header PM@76582; the grid PM@77367 (populated at every entry).
                // Deviation (the port's cut of the home's banners): DoubleRewardContainer Collapsed (HitTestInvisible
                // in the build, PM@49594), its three designer banners drawn over one another as on the home.
                set_vis(cx, "DoubleRewardContainer", Vis::Collapsed);
                if self.rematch_time.abs() < 1e-6 {
                    self.rematch_time = cx.now();
                }
                let map = self.setup.map.as_deref().and_then(|m| cx.view.map(m));
                if let Some(map) = map {
                    let (name, exp) = (map.data.display_name.clone(), map.data.experimental);
                    cx.visit::<PMStateInfo, _>(si, |s, cx| s.set_map_name(cx, &name, exp));
                    self.transition_hook(cx, &map.data.level);
                }
                // (a queue: the flow row "Default", and no map before the match is found)
                let row = if self.queue() { ROW_DEFAULT } else { ROW_REMATCH };
                cx.visit::<PMStateInfo, _>(si, |s, cx| {
                    s.display_timer = row.display_timer;
                    s.set_info_struct(cx, &row);
                });
                self.grid_bound = true;
                let roster: Vec<TileData> = cx.view.roster().iter().cloned().map(TileData::Roster).collect();
                let grid = Self::grid(cx);
                cx.visit::<GridItemList, _>(grid, |g, cx| g.populate(cx, roster));
                wd(cx, grid).vis = Vis::SelfHitTestInvisible;
                self.set_grid_interactable(cx, true);
            }
            CHOOSING_SKIN => {
                // PM@46473: header PM@47304 ("" in arcade, PM@47175), Wwise PM@47870, the page's widgets
                // PM@48166-48394
                let left = self.time_left(cx);
                let header = if self.setup.arcade.is_some() {
                    String::new()
                } else {
                    cx.view.game_text(TEXT_SELECT_FIGHTER).to_string()
                };
                cx.visit::<PMStateInfo, _>(si, |s, cx| s.set_info(cx, &header, left, false, 10.0, false, ""));
                cx.post("ui_characterselect_confirm");
                set_vis_all(
                    cx,
                    &[
                        "UI_PM_ConfirmP0",
                        "UI_PM_SkinSelectorP0",
                        "EditPerksP0",
                        "ShowMoveListP0",
                        "UI_PM_Cosmetics",
                        "UI_PM_BoostInfo",
                    ],
                    Vis::Visible,
                );
                set_vis(cx, "UI_RestedXPBar", Vis::SelfHitTestInvisible);
                // (the claim button's label and visibility follow the unclaimed rewards PM@48826-49453: none offline)
                set_vis(cx, "buttonClaim", Vis::Collapsed);
                self.layout_character_infos(cx);
            }
            // PM@44873: no SetState(3) call exists in the class
            CONFIRMING_CHARACTER => {}
            CHARACTER_CONFIRMED => {
                // PM@43221 (a rematch): the header PM@44389; the arcade's "" (PM@43811), after
                // ArcadeSetLockedCharacter PM@43743 and SetupArcadeRivals PM@43311
                if self.setup.arcade.is_some() {
                    self.arcade_locked(cx);
                    cx.visit::<PMStateInfo, _>(si, |s, cx| s.set_info(cx, "", 0.0, false, 10.0, false, ""));
                } else if self.queue() {
                    // a queue's: no time, no spinner
                    let t = cx.view.game_text(TEXT_WAITING_PARTY).to_string();
                    cx.visit::<PMStateInfo, _>(si, |s, cx| s.set_info(cx, &t, 0.0, false, 10.0, false, ""));
                } else {
                    let left = self.time_left(cx);
                    let t = cx.view.game_text(TEXT_WAITING_OTHERS).to_string();
                    cx.visit::<PMStateInfo, _>(si, |s, cx| s.set_info(cx, &t, left, false, 10.0, false, ""));
                }
            }
            CHOOSING_PERKS => {
                // PM@40377: DismissModals, the notifications, GN_PerkSpacer Collapsed, the ally preview's Update
                // Character Info PM@40548
                Manager::dismiss_all_modals(cx);
                Manager::dismiss_all_notifications(cx);
                set_vis(cx, "GN_PerkSpacer", Vis::Collapsed);
                if let Some(a) = self.ally() {
                    let lo = self.loadout(cx, 4);
                    let locked = self.sels[a].perks_locked;
                    self.fill_loadout(cx, lo, a, locked);
                    cx.visit::<PerksLoadout, _>(lo, |l, cx| l.update_character_info(cx));
                }
                // a rematch PM@40666: ShowPerksNow at once; a queue PM@40681-40745: the map's reveal first, then ShowPerksNow
                // (MapRevealFinished). (The port's: a queue with no map data shows the perks at once.)
                let map = self.setup.map.as_deref().and_then(|m| cx.view.map(m));
                match map {
                    Some(map) if self.queue() => {
                        let me = cx.w;
                        if !self.map_reveal_bound {
                            self.map_reveal_bound = true;
                            cx.visit::<PMStateInfo, _>(si, |s, _| {
                                s.map_reveal_finished.add(Handler { owner: me, id: H_MAP_REVEAL });
                            });
                        }
                        let (name, exp, level) =
                            (map.data.display_name.clone(), map.data.experimental, map.data.level.clone());
                        cx.visit::<PMStateInfo, _>(si, |s, cx| s.set_map_name(cx, &name, exp));
                        self.transition_hook(cx, &level);
                    }
                    _ => self.show_perks_now(cx),
                }
                // (the port's: a queue's match.start that came before this state is applied now)
                if self.start_perks.is_some() {
                    let me = cx.w;
                    cx.view.delay(0.0, Handler { owner: me, id: D_START_PERKS });
                }
            }
            PERKS_CONFIRMED => {
                // PM@39691: "Waiting for Other Players" (wrapped: not free for all), InputContainer Collapsed, the
                // local loadout's Enter, and in 2v2 the ally preview's Exit Ally Header and ShiftAllyPerkPosition
                let wrap = self.setup.team_style != 2 && !self.queue();
                let t = cx.view.game_text(TEXT_WAITING_OTHERS_PERKS).to_string();
                cx.visit::<PMStateInfo, _>(si, |s, cx| s.set_state(cx, &t, wrap, ""));
                set_vis(cx, "InputContainer", Vis::Collapsed);
                self.refresh_loadout_stacks(cx);
                let lo = self.loadout(cx, 0);
                cx.visit::<PerksLoadout, _>(lo, |l, cx| l.enter(cx));
                if self.setup.team_style == 1 {
                    let ally = self.loadout(cx, 4);
                    cx.visit::<PerksLoadout, _>(ally, |l, cx| l.exit_ally_header(cx));
                    cx.view.play(me, "ShiftAllyPerkPosition", false, 1.0);
                }
            }
            REVEALING_PERKS => {
                // PM@36437: the notifications, no spinner, "Revealing Perks" with no time, ui_menu_title_dropdown,
                // the others' loadouts' Enter PM@38819-39543, ApplyPerkLocations, then Delay(7.0, the arcade's 5.0)
                // PM@39585 -> SetState(9)
                Manager::dismiss_all_notifications(cx);
                let t = cx.view.game_text(TEXT_REVEALING_PERKS).to_string();
                cx.visit::<PMStateInfo, _>(si, |s, cx| {
                    s.show_matchmaking_spinner(cx, false);
                    s.set_info(cx, &t, 0.0, false, 10.0, false, "");
                });
                cx.post("ui_menu_title_dropdown");
                // Unverified: the others' perks reach their loadouts natively (OnPerksDetermined, not in the
                // pseudo); the port fills every other player's card here from the lock it has (a bot's).
                for i in 0..self.sels.len() {
                    let w = self.sels[i].widget;
                    if !self.is_me(i) && (1..=3).contains(&w) {
                        let lo = self.loadout(cx, w as usize);
                        self.fill_loadout(cx, lo, i, true);
                    }
                }
                // Refresh Loadout Stacks PM@37098 (2v2). Unverified: the build runs it before the reveal's other
                // branches; here after the fill above, which stands for the native OnPerksDetermined.
                self.refresh_loadout_stacks(cx);
                for &i in revealed(self.setup.team_style) {
                    let lo = self.loadout(cx, i);
                    cx.visit::<PerksLoadout, _>(lo, |l, cx| l.enter(cx));
                }
                self.apply_perk_locations(cx);
                let wait = if self.setup.arcade.is_some() { ARCADE_REVEAL_S } else { REVEAL_S };
                cx.view.delay(wait, Handler { owner: me, id: D_REVEAL });
            }
            READY => self.ready(cx),
            // PM@36020
            IDLE => {
                let t = cx.view.game_text(TEXT_WAITING).to_string();
                cx.visit::<PMStateInfo, _>(si, |s, cx| s.set_info(cx, &t, 0.0, false, 10.0, false, ""));
            }
            STARTING_MATCHMAKING => {
                // PM@34958 (no header of its own: the last one stays, with the spinner)
                self.match_found = false;
                Manager::dismiss_all_notifications(cx);
                cx.visit::<PMStateInfo, _>(si, |s, cx| s.show_matchmaking_spinner(cx, true));
                self.transition_hook(cx, "Matchmaking");
                let cancel = Self::cancel_button_w(cx);
                wd(cx, cancel).vis = Vis::Visible;
                let t = cx.view.game_text(TEXT_PREPARING_MM).to_string();
                cx.visit::<ButtonSecondary, _>(cancel, |b, cx| b.set_label(cx, &t));
                let ci = interactor_of(cx, cancel);
                set_interactable(cx, ci, false);
                // Save Chosen Characters PM@35893
                if let Some(slug) = self.me().character.as_ref().map(|c| c.slug.clone()) {
                    set_last_chosen(cx.prefs, slug);
                }
                // Button_WaitInLab Visible PM@35907, no shimmer PM@35945
                let wait = Self::wait_in_lab_w(cx);
                wd(cx, wait).vis = Vis::Visible;
                cx.visit::<ButtonMain, _>(wait, |_, cx| ButtonMain::enable_shimmer(cx, false));
            }
            MATCHMAKING => {
                // PM@40809: the count-up from the start of matchmaking PM@41115, the spinner PM@41420, Cancel PM@41764
                let secs = (cx.now() - self.matchmaking_start).trunc();
                let t = cx.view.game_text(TEXT_MATCHMAKING).to_string();
                cx.visit::<PMStateInfo, _>(si, |s, cx| {
                    s.set_info(cx, &t, secs, true, 10.0, false, "");
                    s.show_matchmaking_spinner(cx, true);
                });
                let cancel = Self::cancel_button_w(cx);
                wd(cx, cancel).vis = Vis::Visible;
                let t = cx.view.game_text(TEXT_CANCEL_MM).to_string();
                cx.visit::<ButtonSecondary, _>(cancel, |b, cx| b.set_label(cx, &t));
                let ci = interactor_of(cx, cancel);
                set_interactable(cx, ci, true);
                set_vis(cx, "UI_PM_BoostInfo", Vis::Visible);
                // UI_PM_GameplayTips SelfHitTestInvisible PM@41993 and Show Next Tip PM@42031
                let tips = find(cx, "UI_PM_GameplayTips");
                wd(cx, tips).vis = Vis::SelfHitTestInvisible;
                cx.visit::<PMGameplayTips, _>(tips, |t, cx| t.show_next_tip(cx));
                // Button_WaitInLab PM@42107-42402: Visible, interactable, the shimmer on, TimeBetweenShines 1.5.
                // Not ported: the Player Infos' SetWaiting / SetStatus("Waiting...") (both draw nothing in the
                // build's tree).
                let wait = Self::wait_in_lab_w(cx);
                wd(cx, wait).vis = Vis::Visible;
                let wi = interactor_of(cx, wait);
                set_interactable(cx, wi, true);
                cx.visit::<ButtonMain, _>(wait, |_, cx| {
                    ButtonMain::enable_shimmer(cx, true);
                    let q = find(cx, "WBP_Quadrilateral");
                    let w = wd(cx, q);
                    if let Some(quad) = &mut w.quad {
                        match quad.iter_mut().find(|e| e.0 == "TimeBetweenShines") {
                            Some(e) => e.1 = 1.5,
                            None => quad.push(("TimeBetweenShines".into(), 1.5)),
                        }
                    }
                    if let Some(m) = &mut w.mid {
                        m.set_scalar("TimeBetweenShines", 1.5);
                    }
                });
            }
            _ => {}
        }
    }

    // ---------- UpdateContext, the input labels, the character infos ----------

    /// UpdateContext PM@90124-92210.
    fn update_context(&mut self, cx: &mut Cx<'_>, guid: &str) {
        cx.view.manager.set_root_context_for_level(guid);
        let wait = Self::wait_in_lab_w(cx);
        let si = Self::state_info(cx);
        let change = cx.view.tree().find_in(si, "Button_ChangeQueue");
        let list = [interactor_of(cx, wait), interactor_of(cx, change)];
        batch_set_context(cx, &list, guid);
        let s = self.state;
        if s == CHOOSING_CHARACTER {
            let grid = Self::grid(cx);
            cx.visit::<GridItemList, _>(grid, |g, cx| g.update_context(cx, guid));
        } else if s == CHOOSING_SKIN || s == CONFIRMING_CHARACTER || s == CHARACTER_CONFIRMED {
            let c = Self::confirm_w(cx);
            cx.visit::<PMConfirm, _>(c, |c, cx| c.update_context(cx, guid));
            let ss = Self::skin_selector_w(cx);
            cx.visit::<PMSkinSelector, _>(ss, |c, cx| c.update_context(cx, guid));
            let ml = find(cx, "ShowMoveListP0");
            cx.visit::<PMEditEmotes, _>(ml, |e, cx| e.update_context(cx, guid));
            let ep = find(cx, "EditPerksP0");
            cx.visit::<PMEditPerks, _>(ep, |e, cx| e.update_context(cx, guid, 0));
        } else if s == MATCHMAKING {
            let cm = Self::cancel_button_w(cx);
            let i = interactor_of(cx, cm);
            interactor::set_context(cx, i, guid);
        }
        if self.equipper_bound {
            let eq = self.equipper(cx);
            cx.visit::<PerksEquipper, _>(eq, |e, cx| e.update_context(cx, guid));
        }
        for lo in self.built_loadouts(cx) {
            cx.visit::<PerksLoadout, _>(lo, |l, cx| {
                let p = l.player_index;
                l.update_context(cx, guid, p);
            });
        }
        // PM@92081-92120 and PM@93029-93068: the nav (as its main context) and the social panel
        if let Some(n) = self.nav.filter(|&n| !cx.view.is_destroyed(n)) {
            cx.visit::<GlobalNav, _>(n, |n, cx| n.update_context_global_nav(cx, guid, true, false));
        }
        if let Some(sp) = self.social.filter(|&n| !cx.view.is_destroyed(n)) {
            cx.visit::<SocialPanel, _>(sp, |p, cx| p.update_context(cx, guid));
        }
        let vo = find(cx, "VO_Clickable");
        let b = Self::back_to_lobby_w(cx);
        let bi = interactor_of(cx, b);
        batch_set_context(cx, &[vo, bi], guid);
    }

    /// UpdateInputLabels PM@93665: inputLabels.Params by state PM@95169, Populate PM@95546. inputLabel_MoveList is
    /// collapsed on both branches of its state test PM@95675 (the jump's target is the next instruction).
    fn update_input_labels(&self, cx: &mut Cx<'_>) {
        let set = find(cx, "inputLabels");
        let s = self.state;
        let select = cx.view.game_text(TEXT_LABEL_SELECT).to_string();
        let back = cx.view.game_text(TEXT_LABEL_BACK).to_string();
        let params = if s == CHOOSING_CHARACTER || s == CHOOSING_SKIN || s == CONFIRMING_CHARACTER {
            if self.can_back && s != CHOOSING_CHARACTER {
                vec![label("UISubmit", &select), label("UICancel", &back)]
            } else {
                vec![label("UISubmit", &select)]
            }
        } else if s == MATCHMAKING {
            vec![label("UICancel", &back)]
        } else {
            Vec::new()
        };
        cx.visit::<InputLabelSet, _>(set, |l, cx| {
            l.params = params;
            l.populate(cx);
        });
        set_vis(cx, "inputLabel_MoveList", Vis::Collapsed);
    }

    /// LayoutCharacterInfos PM@78269, and the shadow's ease PM@4061-4448 (Ease(0, 0.9, elapsed / 1.0 s, EaseInOut,
    /// exponent 1.0: linear), stepped by a Delay(0) loop.
    fn layout_character_infos(&mut self, cx: &mut Cx<'_>) {
        let vis = if self.state == CHOOSING_CHARACTER { Vis::HitTestInvisible } else { Vis::Collapsed };
        set_vis(cx, "CharGridShadow", vis);
        set_vis(cx, "UI_PM_CharacterInfo_V2_P0", vis);
        self.shadow_start = cx.now();
    }

    // ---------- name plates at the world anchors ----------

    /// The projection of the stored point `key`: None when no point arrived, Some(None) off the screen or not yet
    /// projected.
    fn projected(&self, key: &str) -> Option<Option<[f64; 2]>> {
        self.points.get(key).map(|_| self.projection.get(key).copied().flatten())
    }

    /// CheckmarkLocations[i] places Player Infos [0, 2, 1, 3][i] (PM@12511); a point off the screen stops the loop
    /// (PM@30621 pop_flow).
    fn apply_player_info_locations(&self, cx: &mut Cx<'_>) {
        for (i, &widget) in LOCATION_WIDGET.iter().enumerate() {
            match self.projected(&point_key(false, i)) {
                None => continue,
                Some(None) => return,
                Some(Some(c)) => {
                    let pi = Self::player_info_w(cx, widget);
                    let slot = &mut wd(cx, pi).offsets;
                    slot.l = c[0] + self.frame_offset[0];
                    slot.t = c[1] + self.frame_offset[1];
                }
            }
        }
    }

    /// ApplyPerkLocations PM@50352: each loadout at its point projected now + (0, 10) PM@50932, the loop stopping at
    /// a point off the screen (PM@50790 pop_flow). (The port's stand-in points where none arrived: PERK_STAND_IN.)
    /// Deviation: a 2v2 rematch's P2 at PerkLocations[1] - (0, 20) PM@51414-51770 (every custom game here) is left
    /// out: it lifts the ally's name and level out of line with the others, under its perk icons.
    fn apply_perk_locations(&self, cx: &mut Cx<'_>) {
        let stand = &PERK_STAND_IN[usize::from(self.setup.team_style).min(2)];
        for (i, &widget) in LOCATION_WIDGET.iter().enumerate() {
            let p = match self.projected(&point_key(true, i)) {
                None => stand[i],
                Some(None) => return,
                Some(Some(p)) => p,
            };
            let w = find(cx, LOADOUTS[widget]);
            let slot = &mut wd(cx, w).offsets;
            slot.l = p[0] + self.frame_offset[0];
            slot.t = p[1] + 10.0 + self.frame_offset[1];
        }
    }

    // ---------- the grid ----------

    /// GetLastChosenCharacter(0): the save slot's character, else Get Characters(true, false)[0].
    fn last_chosen_character(&self, cx: &Cx<'_>) -> Option<RosterEntry> {
        let last = last_chosen(cx.prefs);
        let roster = cx.view.roster();
        roster.iter().find(|r| last.as_deref().is_some_and(|l| r.slug == l)).or_else(|| roster.first()).cloned()
    }

    /// UI_GridItemList.SetInteractable(0, b): each tile's first interactor. (Run, not visit: a click on a tile
    /// calls this while the grid emits its selected event.)
    fn set_grid_interactable(&mut self, cx: &mut Cx<'_>, b: bool) {
        self.grid_interactable = b;
        let grid = Self::grid(cx);
        run_on::<GridItemList>(cx, grid, move |g, cx| {
            for it in g.items() {
                // (the tile the click selected is busy: its own call runs when its event returns)
                run_tile(cx, it, move |t, cx| {
                    if let Some(&i) = t.interactors(cx).first() {
                        set_interactable(cx, i, b);
                    }
                });
            }
        });
    }

    /// Some tile is entering.
    fn grid_entering(cx: &mut Cx<'_>) -> bool {
        let grid = Self::grid(cx);
        let items = cx.visit::<GridItemList, _>(grid, |g, _| g.items()).unwrap_or_default();
        items
            .into_iter()
            .any(|it| cx.visit_dyn(it, |c, _| c.as_grid_item().map(|t| t.state().is_entering)).flatten() == Some(true))
    }

    /// PopulatedEvent PM@111888 -> PM@19570.
    fn on_character_grid_populated(&mut self, cx: &mut Cx<'_>) {
        if !self.grid_bound {
            return;
        }
        self.grid_wait(cx);
    }

    fn grid_wait(&mut self, cx: &mut Cx<'_>) {
        if self.state != CHOOSING_CHARACTER {
            return;
        }
        let me = cx.w;
        if Self::grid_entering(cx) || Manager::is_modal_active(cx.view) {
            cx.view.delay(0.1, Handler { owner: me, id: D_GRID_WAIT });
            return;
        }
        self.layout_character_infos(cx);
        // AddPlayer(0, false) @13789
        self.set_grid_interactable(cx, true);
        self.grid_enabled = true;
        cx.view.delay(0.1, Handler { owner: me, id: D_GRID_FOCUS });
    }

    fn grid_focus(&mut self, cx: &mut Cx<'_>) {
        if self.state != CHOOSING_CHARACTER || cx.view.manager.real_mouse_active(0) || cx.view.manager.cursor_mode[0] {
            return;
        }
        Manager::unfocus_all(cx, 0);
        self.grid_has_focused = false;
        let c = self.previewed.clone().or_else(|| self.last_chosen_character(cx));
        self.set_focused_item(cx, c.as_ref());
    }

    /// UI_GridItemList.SetFocusedItem @15815.
    fn set_focused_item(&mut self, cx: &mut Cx<'_>, c: Option<&RosterEntry>) {
        if self.grid_has_focused {
            return;
        }
        let grid = Self::grid(cx);
        let slug = c.map(|c| c.slug.clone());
        cx.visit::<GridItemList, _>(grid, |g, cx| {
            let at = match &slug {
                Some(s) => g.index_for_data(|d| d.roster().is_some_and(|r| &r.slug == s)),
                None => Some(0),
            };
            if let Some(at) = at {
                g.focus_index(cx, 0, at);
            }
        });
    }

    /// FocusedEvent PM@97068.
    fn on_character_grid_focused(&mut self, cx: &mut Cx<'_>, ev: &TileEvent) {
        if ev.player != 0 {
            return;
        }
        self.grid_has_focused = true;
        let c = ev.data.as_ref().and_then(TileData::roster).cloned();
        self.on_character_focused(cx, c.as_ref());
    }

    /// OnCharacterFocused PM@114230: HandleCharacterFocused, UpdateWidgetsForCharacterSelection PM@61218, the preview
    /// PM@67047. The Random tile has no CharacterData: the info panel's RandomCharacter, the previewed character
    /// stays.
    fn on_character_focused(&mut self, cx: &mut Cx<'_>, c: Option<&RosterEntry>) {
        let panel = find(cx, "UI_PM_CharacterInfo_V2_P0");
        let Some(c) = c else {
            cx.visit::<PMCharacterInfo, _>(panel, |p, cx| p.random_character(cx));
            return;
        };
        self.previewed = Some(c.clone());
        if c.fighter.is_some() {
            self.last_valid = Some(c.clone());
        }
        self.me_mut().character = Some(c.clone());
        let pi = Self::player_info_w(cx, 0);
        cx.visit::<PMPlayerInfo, _>(pi, |p, cx| p.set_character(cx, Some(c)));
        cx.visit::<PMCharacterInfo, _>(panel, |p, cx| p.set_character(cx, c));
        // GetSavedSelectedSkin @0-632 (the Hydra preference, else the default), as its name
        let fi = c.fighter.as_deref().and_then(|f| cx.view.fighters().get(f));
        let default = fi.and_then(|f| selected_skin(f, &cx.prefs.hydra)).map(|s| s.name.clone()).unwrap_or_default();
        self.previewed_skin = picked_skin(fi, &default);
        if c.fighter.is_some() {
            self.last_valid_skin = self.previewed_skin.clone();
        }
        // Deviation: the skin focus's team is the local player's (not team 0)
        let team = self.team_of(self.me());
        let skin = self.previewed_skin.clone();
        if let Some(p) = Self::net_pick(c, &skin) {
            self.hook(cx, PreMatchCmd::LocalPreview(p));
        }
        self.change_fighter(cx, self.preview_index(0), c, &skin, team);
        self.update_confirm_button_state(cx);
    }

    /// SelectedEvent PM@96444 -> OnCharacterSelectedBase PM@60800. The build's IsLocked branch opens the shop's
    /// UI_ContextualUnlockCharacter; the port's lock (a fighter that is not ported) selects nothing.
    fn on_character_grid_selected(&mut self, cx: &mut Cx<'_>, ev: &TileEvent) {
        if ev.player != 0 || Manager::is_modal_active(cx.view) {
            return;
        }
        let Some(c) = ev.data.as_ref().and_then(TileData::roster).cloned() else { return };
        if ev.locked {
            let item = ev.item;
            cx.visit_dyn(item, |ctl, cx| {
                if let Some(t) = ctl.as_grid_item() {
                    GridItemState::deselect(t, cx);
                }
            });
            return;
        }
        if self.previewed.as_ref().map(|p| &p.slug) != Some(&c.slug) {
            self.on_character_focused(cx, Some(&c));
        }
        let prev = self.previewed.clone();
        let ss = Self::skin_selector_w(cx);
        cx.visit::<PMSkinSelector, _>(ss, |s, cx| s.set_character(cx, prev.as_ref()));
        self.set_grid_interactable(cx, false);
        let me = cx.w;
        cx.view.delay(0.05, Handler { owner: me, id: D_TO_SKIN });
    }

    /// OnSkinFocused(Skin, 0) PM@65615; HandleSkinFocused PM@65963: the previewed skin (the one the timer's lock
    /// takes, native 1410a36e0).
    fn on_skin_focused(&mut self, cx: &mut Cx<'_>, skin: &SkinInfo) {
        let me = cx.w;
        if !cx.view.visible_in_hierarchy(me) {
            return;
        }
        let Some(prev) = self.previewed.clone() else { return };
        let fi = prev.fighter.as_deref().and_then(|f| cx.view.fighters().get(f));
        let name = picked_skin(fi, &skin.name);
        self.previewed_skin = name.clone();
        if self.last_valid.as_ref().map(|l| &l.slug) == Some(&prev.slug) {
            self.last_valid_skin = name.clone();
        }
        // Deviation: the skin focus's team is the local player's (not team 0)
        let team = self.team_of(self.me());
        self.change_fighter(cx, self.preview_index(0), &prev, &name, team);
        if let Some(p) = Self::net_pick(&prev, &name) {
            self.hook(cx, PreMatchCmd::LocalPreview(p));
        }
    }

    // ---------- lock-in ----------

    /// CanLockCharacter(previewed): native. Unverified: the duplicate rule is not decoded; the port reads 0 (can
    /// lock), so ShowDuplicatesNotification PM@129457 never shows.
    fn can_lock_character(&self) -> i32 {
        0
    }

    fn update_confirm_button_state(&self, cx: &mut Cx<'_>) {
        let state = self.can_lock_character();
        let c = Self::confirm_w(cx);
        cx.visit::<PMConfirm, _>(c, |c, cx| c.set_can_confirm_state(cx, state));
    }

    /// OnConfirmP0 PM@89806 -> OnCharacterConfirmed(0) PM@63371.
    fn on_confirm_p0(&mut self, cx: &mut Cx<'_>) {
        set_vis_all(cx, &["UI_PM_ConfirmP0", "UI_PM_SkinSelectorP0", "buttonClaim"], Vis::Collapsed);
        if self.state != CHOOSING_SKIN || self.previewed.as_ref().is_none_or(|p| p.fighter.is_none()) {
            return;
        }
        let ss = Self::skin_selector_w(cx);
        if let Some(Some(skin)) = cx.visit::<PMSkinSelector, _>(ss, |s, _| s.selected_skin.clone()) {
            self.on_skin_focused(cx, &skin);
        }
        let (c, skin) = (self.previewed.clone(), self.previewed_skin.clone());
        let at = self.setup.me;
        if let Some(c) = c {
            self.lock(cx, at, &c, &skin);
        }
        self.set_state(cx, CHARACTER_CONFIRMED);
        // PM@22900, PM@64413. OnCharacterConfirmed PM@63371 has this one caller (PM@89920, PlayerIndex 0) and sets
        // state 4 before its sound: PM@63935 takes the PlayerIndex 0 branch and, the state being 4, PM@64413. The
        // other branch (PM@64469-64688 -> ui_characterselect_select PM@64899) is never reached.
        let me = cx.w;
        cx.view.delay(0.25, Handler { owner: me, id: D_CONFIRM_VO });
        cx.post("ui_characterselect_select_locked");
        self.check_all_locked(cx);
    }

    fn lock(&mut self, cx: &mut Cx<'_>, at: usize, c: &RosterEntry, skin: &str) {
        let s = &mut self.sels[at];
        s.character = Some(c.clone());
        s.skin = skin.to_string();
        s.locked = true;
        if self.is_me(at)
            && let Some(p) = Self::net_pick(c, skin)
        {
            self.hook(cx, PreMatchCmd::LocalLocked(p));
        }
    }

    /// Native OnPlayerLockedCharacter PM@102623 for another account (a bot's selector here): UpdateConfirmButtonState,
    /// and a team-mate's pick shows at once PM@102995 (an opponent's at the face-off). No sound.
    fn player_locked(&mut self, cx: &mut Cx<'_>, at: usize) {
        let (fighter, skin) = match self.sels.get(at) {
            Some(s) if !self.is_me(at) => (s.character.as_ref().and_then(|c| c.fighter.clone()), s.skin.clone()),
            _ => return,
        };
        let Some(fighter) = fighter else { return };
        let Some(c) = cx.view.roster().iter().find(|r| r.fighter.as_deref() == Some(fighter.as_str())).cloned() else {
            return;
        };
        self.lock(cx, at, &c, &skin);
        self.update_confirm_button_state(cx);
        if self.sels[at].widget == 2 {
            let pi = Self::player_info_w(cx, 2);
            cx.visit::<PMPlayerInfo, _>(pi, |p, cx| p.set_character(cx, Some(&c)));
            let team = self.team_of(&self.sels[at]);
            self.change_fighter(cx, 2, &c, &skin, team);
        }
        self.check_all_locked(cx);
    }

    /// The character select manager's "everyone has locked" (native): OnPlayersReadyToMatchmake PM@103042.
    fn check_all_locked(&self, cx: &mut Cx<'_>) {
        // online the server says it (online_players_ready)
        if self.online_rematch() {
            return;
        }
        let waiting = self.sels.iter().any(|s| self.setup.players[s.player].team != 4 && !s.locked);
        if self.all_characters_chosen || waiting {
            return;
        }
        let me = cx.w;
        cx.view.delay(0.0, Handler { owner: me, id: D_PLAYERS_READY });
    }

    // ---------- Submit2 / Submit3 / Cancel ----------

    /// OnSubmit2 PM@103427: player 0 only, not while the social panel uses the shortcut binding (PM@103517), then
    /// ShowMoveList's body.
    fn on_submit2(&mut self, cx: &mut Cx<'_>, player: i32) {
        if player != 0 || social_panel_uses_shortcut(cx) {
            return;
        }
        self.show_move_list(cx);
    }

    /// ShowMoveList PM@103532: nothing while a modal is active (PM@103641) or GetPreviewedCharacter(0) is not valid
    /// (PM@103750), else the move list of that character (PM@103760-104051). A roster character the port lacks, or
    /// Random, has no fighter: no move list.
    fn show_move_list(&self, cx: &mut Cx<'_>) {
        if Manager::is_modal_active(cx.view) {
            return;
        }
        let c = match &self.previewed {
            Some(p) if p.fighter.is_some() => Some(p.clone()),
            _ => self.sels.get(self.setup.me).and_then(|s| s.character.clone()),
        };
        if let Some(f) = c.and_then(|c| c.fighter) {
            show_movelist(cx, &f);
        }
    }

    /// OnSubmit3 PM@110314: nothing while the perk page shows (PM@110369) or a modal is active (PM@110493);
    /// otherwise the confirm shortcut.
    fn on_submit3(&mut self, cx: &mut Cx<'_>) {
        if self.equipper_bound {
            let eq = self.equipper(cx);
            if cx.view.visible_in_hierarchy(eq) {
                return;
            }
        }
        if Manager::is_modal_active(cx.view) {
            return;
        }
        // while the matchmaking handler's bIsMatchmaking (PM@110554), Wait in the Lab when its interactor is
        // interactable (PM@110657 -> PM@110788)
        if Self::is_matchmaking(cx) {
            let wait = Self::wait_in_lab_w(cx);
            let wi = interactor_of(cx, wait);
            if cx.view.manager.interactor(wi).is_some_and(|s| s.get_interactable()) {
                self.transition_to_training(cx);
            }
            return;
        }
        if self.state != CHOOSING_SKIN || self.can_lock_character() != 0 {
            return;
        }
        self.on_confirm_p0(cx);
    }

    /// UICancel's step back (PM@68602-69398): the Wwise event, then the StepBack body PM@69718. A dialog or modal
    /// keeps it (PM@68755, PM@68891), and WaitingForPerksPanel? stops it (PM@69043): the perk page takes UICancel
    /// itself. (Not ported here: StepBackDelayActive PM@68602 and the social panel's IsOpen? PM@69398.)
    fn on_cancel(&mut self, cx: &mut Cx<'_>, player: i32) {
        let me = cx.w;
        if player != 0
            || Manager::is_modal_active(cx.view)
            || Manager::is_dialog_active(cx.view)
            || !cx.view.visible_in_hierarchy(me)
        {
            return;
        }
        if self.waiting_for_perks_panel(cx) {
            return;
        }
        cx.post("ui_menu_step_back");
        self.step_back(cx, false);
    }

    /// StepBack(0) PM@69718 (CanBack? false skips UICancel's, PM@69029 -> PM@74837; the nav's exit, PM@108657, goes
    /// straight to the body). The arcade's state 1 PM@72258 and an arcade rematch's state 2 PM@70623: the Leave
    /// Arcade? dialog. (The nav's exit, `from_nav`, skips CanBack for the arcade's dialog alone; its other states
    /// keep the test.)
    fn step_back(&mut self, cx: &mut Cx<'_>, from_nav: bool) {
        if self.kicked {
            return;
        }
        let leave = self.setup.arcade.is_some()
            && (self.state == CHOOSING_CHARACTER || (self.state == CHOOSING_SKIN && self.arcade_rematch()));
        if leave && (from_nav || self.can_back) {
            self.arcade_leave_dialog(cx);
            return;
        }
        if !self.can_back {
            return;
        }
        if self.state == CHOOSING_SKIN {
            self.set_state(cx, CHOOSING_CHARACTER);
        }
        // state 4: nothing in a rematch (a confirmed pick cannot be taken back); state 1 PM@72654:
        // GetHasSelected(0) -> CharacterGrid.StepBack(0, previewed) @15693
        else if self.state == CHOOSING_CHARACTER && self.grid_enabled && !self.grid_interactable {
            let grid = Self::grid(cx);
            let slug = self.previewed.as_ref().map(|p| p.slug.clone());
            cx.visit::<GridItemList, _>(grid, |g, cx| {
                g.deselect_all(cx);
                let at = slug.as_ref().and_then(|s| g.index_for_data(|d| d.roster().is_some_and(|r| &r.slug == s)));
                if let Some(at) = at {
                    g.focus_index(cx, 0, at);
                }
            });
            self.set_grid_interactable(cx, true);
        }
        // a queue's state 1 with no selection, alone in the party (PM@74020): the main menu
        else if self.queue() && self.state == CHOOSING_CHARACTER {
            self.kick_back_to_main_menu(cx, "");
        }
        // a queue's state 3 / 4 PM@70560: HandleSelectionsUnlock(OnClearedSkin) -> PM@79445 CanBack true, SetState(1)
        // PM@77690 (the port's unlock answers at once)
        else if self.queue() && (self.state == CONFIRMING_CHARACTER || self.state == CHARACTER_CONFIRMED) {
            self.unlock_mine(cx);
            self.set_state(cx, CHOOSING_CHARACTER);
        }
        // a queue's state 5 PM@70073: Can Cancel Matchmaking? (the handler's, native; Unverified: read as "not
        // found"), SetWaiting(true, "Cancelling Matchmaking"), the 80 s no-response timer PM@52622,
        // HandleSelectionsUnlock PM@52748 -> ClearedForCancel PM@98645: CancelMM_Ghost and UI_PM_BoostInfo Collapsed,
        // Toggle Matchmaking(false) PM@80383 (the party owner). (The port's unlock answers at once.)
        else if self.queue() && self.state == MATCHMAKING && !self.match_found {
            let t = cx.view.game_text(TEXT_CANCELLING_MM).to_string();
            Manager::set_waiting(cx, true, &t, false, 0.0);
            self.can_back = false;
            self.requested_cancel = true;
            self.cancel_timer += 1;
            let me = cx.w;
            cx.view.delay(CANCEL_NO_RESPONSE_S, Handler { owner: me, id: D_CANCEL_NR + (self.cancel_timer & 0xFF) });
            self.unlock_mine(cx);
            let cancel = Self::cancel_button_w(cx);
            wd(cx, cancel).vis = Vis::Collapsed;
            set_vis(cx, "UI_PM_BoostInfo", Vis::Collapsed);
            self.hook(cx, PreMatchCmd::CancelMatchmaking);
        }
    }

    // ---------- the queue ----------

    /// HandleSelectionsUnlock for the local pick: the character select manager lets go of it.
    fn unlock_mine(&mut self, cx: &mut Cx<'_>) {
        if !self.me().locked {
            return;
        }
        self.me_mut().locked = false;
        self.hook(cx, PreMatchCmd::LocalUnlocked);
    }

    /// OnPlayersReadyToMatchmake, not a rematch PM@88323 -> PM@68200: CanBack false; the party owner:
    /// MatchmakingStartTime PM@66969, SetState(11) PM@68346, StartMatchmaking PM@68410. A member (GetIAmPartyOwner
    /// false PM@68332 -> PM@66752): MatchmakingStartTime alone; its state stays 4 ("Waiting for Party Member") until
    /// the party's OnMatchmakingStarted.
    /// Deviation: a party's page runs this at its own lock (a page sees only its own: the program tells the other
    /// with the party's says) and stays in state 4 with CanBack kept, so its Back takes the pick back while the
    /// party's search has not started; the leader's state 11 comes with PartyStarted, once both picks are locked.
    fn start_matchmaking(&mut self, cx: &mut Cx<'_>) {
        self.matchmaking_start = cx.now();
        if self.party_role().is_some() {
            return self.hook(cx, PreMatchCmd::StartMatchmaking);
        }
        self.can_back = false;
        self.set_state(cx, STARTING_MATCHMAKING);
        self.hook(cx, PreMatchCmd::StartMatchmaking);
    }

    /// (the port's own) The party leader's search starting, both picks locked (partyGo): the owner's PM@68200 path.
    fn party_started(&mut self, cx: &mut Cx<'_>) {
        if self.state != CHARACTER_CONFIRMED || self.kicked || self.match_found {
            return;
        }
        self.can_back = false;
        self.set_state(cx, STARTING_MATCHMAKING);
    }

    /// (the port's own) The party's search stopped by the other member (partyStopped): the unasked
    /// OnMatchmakingCancel (PM@81099: state 4, the lock kept); from state 11, the same before the search has started.
    fn party_stopped(&mut self, cx: &mut Cx<'_>) {
        if self.kicked || self.match_found {
            return;
        }
        if self.state == MATCHMAKING {
            return self.on_matchmaking_cancel(cx);
        }
        if self.state != STARTING_MATCHMAKING {
            return;
        }
        self.set_state(cx, CHARACTER_CONFIRMED);
        self.can_back = true;
    }

    /// Native OnMatchmakingStarted PM@115610: SetState(5), then PM@113705 CanBack = not MatchFound. (A party member's
    /// comes in state 4: it sets no state 11, start_matchmaking.)
    fn on_matchmaking_started(&mut self, cx: &mut Cx<'_>) {
        let from =
            if self.party_role() == Some(PartyRole::Member) { CHARACTER_CONFIRMED } else { STARTING_MATCHMAKING };
        if self.state != from || self.kicked {
            return;
        }
        self.set_state(cx, MATCHMAKING);
        self.can_back = !self.match_found;
    }

    /// Native OnMatchmakingCancel PM@103338, in state 5 only -> PM@81502. Unverified: the queue's cancel is read as
    /// Status 3 (cancelled); a found match takes no cancel (PM@80668: the wait ends, nothing else).
    fn on_matchmaking_cancel(&mut self, cx: &mut Cx<'_>) {
        if self.state != MATCHMAKING || self.kicked {
            return;
        }
        self.cancel_timer += 1;
        if self.match_found {
            Manager::set_waiting(cx, false, "", false, 0.0);
            self.requested_cancel = false;
            return;
        }
        // PM@80861
        self.can_back = false;
        if self.requested_cancel {
            // the player's own: BPI_PreMatchTransition("CancelMatchmaking"), HandleSelectionsUnlock PM@81070 ->
            // PM@81147 CanBack true, SetWaiting(false), SetState(1)
            self.transition_hook(cx, "CancelMatchmaking");
            self.unlock_mine(cx);
            self.can_back = true;
            Manager::set_waiting(cx, false, "", false, 0.0);
            self.requested_cancel = false;
            self.set_state(cx, CHOOSING_CHARACTER);
        } else {
            // PM@81099: not asked for: back to the confirmed state
            self.set_state(cx, CHARACTER_CONFIRMED);
            self.requested_cancel = false;
            self.can_back = true;
        }
    }

    /// CancelMatchmakingNoResponse PM@81408 -> PM@79624: the wait ends, Back works again, the state stays, and
    /// BPI_PreMatchTransition("CancelMatchmaking") PM@79858.
    fn cancel_no_response(&mut self, cx: &mut Cx<'_>, count: u32) {
        if self.cancel_timer & 0xFF != count {
            return;
        }
        Manager::set_waiting(cx, false, "", false, 0.0);
        self.can_back = true;
        self.requested_cancel = false;
        self.transition_hook(cx, "CancelMatchmaking");
    }

    /// Native Handle All Players Registered PM@113900 (OnAllPlayersRegistered): MapPlayersToWidgets(Selections, true)
    /// at once, then PM@80421: MatchFound, CanBack false, the wait and the cancel timer ended, PlayersFound,
    /// CancelMM_Ghost and UI_PM_BoostInfo Collapsed, DismissModals. players[i] is fighter slot i, me the local one's;
    /// the team style is the found match's (the multiqueue's search drew the first selected mode's until now).
    /// Not ported: the map's preload (PM@85401-85627), which the level does when it opens.
    fn on_all_players_registered(&mut self, cx: &mut Cx<'_>, reg: &Registered) {
        if self.match_found || !self.queue() || self.kicked {
            return;
        }
        // (the return from the Lab, a find inside state 0's 1.5 s: the build's Selections already hold the local lock)
        self.restore_pick(cx);
        let mut mine = self.me().clone();
        mine.player = reg.me;
        self.setup.players = reg.players.clone();
        self.setup.me = reg.me;
        self.setup.map = Some(reg.map.clone()).filter(|m| !m.is_empty());
        self.setup.level = reg.map.clone();
        let restyled = self.setup.team_style != reg.team_style;
        self.setup.team_style = reg.team_style;
        if restyled {
            self.update_header_layout(cx);
        }
        self.sels.clear();
        // the local pick stays; the others' as the queue sent them, all locked
        self.map_players_keep(cx, true, Some(mine));
        for s in &mut self.sels {
            s.locked = true;
        }
        self.match_found = true;
        self.can_back = false;
        Manager::set_waiting(cx, false, "", false, 0.0);
        self.cancel_timer += 1;
        self.requested_cancel = false;
        self.players_found(cx);
        self.reset_loadouts(cx, false);
        // (the reveal's cards: every player's perk pictures from Match Found)
        let all: Vec<usize> = (0..self.sels.len()).collect();
        self.hold_perk_pictures(cx, &all);
        let cancel = Self::cancel_button_w(cx);
        wd(cx, cancel).vis = Vis::Collapsed;
        set_vis(cx, "UI_PM_BoostInfo", Vis::Collapsed);
        Manager::dismiss_all_modals(cx);
    }

    /// State 0's restore path PM@5079, a queue's screen opened with its search running (the return from the Lab):
    /// the local entry with its Character and Skin (restore_pick); UI_PM_StateInfo SelfHitTestInvisible PM@6887; then
    /// AreAllPlayersRegistered PM@6925: Handle All Players Registered PM@7015, else SetState(5) PM@7048 (states 11 and
    /// StartMatchmaking skipped).
    fn restore_selections(&mut self, cx: &mut Cx<'_>) {
        self.restore_pick(cx);
        let si = Self::state_info(cx);
        wd(cx, si).vis = Vis::SelfHitTestInvisible;
        if self.match_found {
            return;
        }
        let Some(reg) = cx.view.session.online.pre_match.as_ref().and_then(|p| p.registered.clone()) else {
            return self.set_state(cx, MATCHMAKING);
        };
        self.on_all_players_registered(cx, &reg);
        if let Some(perks) = reg.perks {
            self.all_players_locked_perks(cx, perks);
        }
    }

    /// The restore's local pick (PM@5709-6770), once: a screen opened on the return with the queue's lock.
    fn restore_pick(&mut self, cx: &mut Cx<'_>) {
        let pick = self
            .setup
            .net
            .as_ref()
            .and_then(|n| n.queue.as_ref())
            .and_then(|q| q.resume.as_ref())
            .and_then(|x| x.pick.clone());
        if self.me().locked {
            return;
        }
        let Some(pick) = pick else { return };
        let Some(c) = Self::roster_of(cx, &pick.character) else { return };
        self.on_character_focused(cx, Some(&c));
        if !pick.skin.is_empty() && pick.skin != self.previewed_skin {
            self.previewed_skin = pick.skin.clone();
            self.last_valid_skin = pick.skin.clone();
            let team = self.team_of(self.me());
            self.change_fighter(cx, self.preview_index(0), &c, &pick.skin, team);
        }
        // (the lock the queue holds: no OnConfirmP0, no sound, no matchmaking start)
        let at = self.setup.me;
        self.lock(cx, at, &c, &pick.skin);
    }

    /// BPI_TransitionToTraining PGI@488 (the click PM@115633 and OnSubmit3's PM@110788): DismissAllModals,
    /// DismissAllDialogs, DismissAllNotifications, Transition{TrainingContainer, type 0, options "Training"}. Not
    /// ported: SetDefaultTaunts(false) PGI@534, SetIsJoinable(false) PGI@1161, TryInLab = false PGI@1362, Remove Main
    /// Menu PGI@1302 (the queue's screen is not hosted in the main menu).
    fn transition_to_training(&mut self, cx: &mut Cx<'_>) {
        Manager::dismiss_all_modals(cx);
        Manager::dismiss_all_dialogs(cx);
        Manager::dismiss_all_notifications(cx);
        Manager::transition(
            cx,
            TransitionParams {
                next_level: "TrainingContainer".into(),
                kind: 0,
                delay: 0.0,
                force: false,
                level_options: "Training".into(),
                ..TransitionParams::default()
            },
        );
    }

    // ---------- the online pre-match's news ----------

    /// What the program's online client tells the screen this frame (preMatchSession.live's calls).
    fn take_online(&mut self, cx: &mut Cx<'_>) {
        if !self.server() || cx.view.session.online.events.is_empty() {
            return;
        }
        let events: Vec<PreMatchEvent> = cx
            .view
            .session
            .online
            .events
            .iter()
            .filter_map(|e| if let OnlineEvent::PreMatch(p) = e { Some(p.clone()) } else { None })
            .collect();
        for e in events {
            match e {
                PreMatchEvent::Previewed { account, pick } => self.player_previewed(cx, &account, &pick),
                PreMatchEvent::Cosmetics { account, cosmetics } => self.player_cosmetics(cx, &account, cosmetics),
                PreMatchEvent::Locked { account, pick } => self.online_player_locked(cx, &account, &pick),
                PreMatchEvent::PlayersReady { picks, map } => self.online_players_ready(cx, &picks, &map),
                PreMatchEvent::ReadyPerks { account } => self.player_ready_perks(cx, &account),
                PreMatchEvent::AllLockedPerks { perks } => self.all_players_locked_perks(cx, perks),
                PreMatchEvent::Kick(note) => self.kick_back_to_main_menu(cx, &kick_text(&note)),
                PreMatchEvent::MatchmakingStarted => self.on_matchmaking_started(cx),
                PreMatchEvent::MatchmakingCancel => self.on_matchmaking_cancel(cx),
                PreMatchEvent::AllRegistered(reg) => {
                    self.on_all_players_registered(cx, &reg);
                    if let Some(perks) = reg.perks {
                        self.all_players_locked_perks(cx, perks);
                    }
                }
                PreMatchEvent::PartyStarted => self.party_started(cx),
                PreMatchEvent::PartyStopped => self.party_stopped(cx),
            }
        }
    }

    fn sel_of_account(&self, account: &str) -> Option<usize> {
        self.sels.iter().position(|s| self.setup.players[s.player].account_id == account)
    }

    fn roster_of(cx: &Cx<'_>, character: &str) -> Option<RosterEntry> {
        cx.view.roster().iter().find(|r| r.fighter.as_deref() == Some(character)).cloned()
    }

    /// Native OnPlayerLockedCharacter PM@102623 for another account (the online side's entry): see player_locked.
    fn online_player_locked(&mut self, cx: &mut Cx<'_>, account: &str, pick: &NetPick) {
        let Some(at) = self.sel_of_account(account) else { return };
        let Some(c) = Self::roster_of(cx, &pick.character) else { return };
        if self.is_me(at) {
            return;
        }
        self.sels[at].character = Some(c);
        self.sels[at].skin = pick.skin.clone();
        self.player_locked(cx, at);
    }

    /// Native OnPlayerPreviewedCharacter PM@102033 / OnPlayerPreviewedSkin PM@102246 for another account (online,
    /// pm.preview): on the rematch path a team-mate's alone, on its plate and its preview (slot 2) until its lock.
    fn player_previewed(&mut self, cx: &mut Cx<'_>, account: &str, pick: &NetPick) {
        let Some(at) = self.sel_of_account(account) else { return };
        let Some(c) = Self::roster_of(cx, &pick.character) else { return };
        if self.is_me(at) || self.sels[at].widget != 2 || self.sels[at].locked || self.all_characters_chosen {
            return;
        }
        let pi = Self::player_info_w(cx, 2);
        cx.visit::<PMPlayerInfo, _>(pi, |p, cx| p.set_character(cx, Some(&c)));
        let team = self.team_of(&self.sels[at]);
        self.change_fighter(cx, 2, &c, &pick.skin, team);
        self.update_confirm_button_state(cx);
    }

    /// Online: every slot locked (pm.all, OnPlayersReadyToMatchmake PM@103042 with the server's Selections), picks by
    /// fighter slot, and the map. The server's picks stand over the page's (a lock the server took at its deadline,
    /// or one of its own that came late); a local player it locked goes to state 10 as HandleTimerExpired's
    /// SetState(10).
    /// Deviation: the map shows here (the build knows it from the rematch start, RematchMapPath; the server tells it
    /// with the reveal).
    fn online_players_ready(&mut self, cx: &mut Cx<'_>, picks: &[NetPick], map: &str) {
        if !self.online_rematch() || self.all_characters_chosen || self.kicked {
            return;
        }
        if !map.is_empty() {
            self.setup.map = Some(map.to_string());
            self.setup.level = map.to_string();
            if let Some(md) = cx.view.map(map) {
                let (name, exp, level) = (md.data.display_name.clone(), md.data.experimental, md.data.level.clone());
                let si = Self::state_info(cx);
                cx.visit::<PMStateInfo, _>(si, |s, cx| s.set_map_name(cx, &name, exp));
                self.transition_hook(cx, &level);
            }
        }
        for at in 0..self.sels.len() {
            let Some(p) = picks.get(at) else { continue };
            let Some(c) = Self::roster_of(cx, &p.character) else { continue };
            if self.team_of(&self.sels[at]) == 4 {
                continue;
            }
            if !self.is_me(at) {
                let s = &mut self.sels[at];
                s.character = Some(c);
                s.skin = p.skin.clone();
                s.locked = true;
                continue;
            }
            let was = self.sels[at].locked;
            let same =
                self.sels[at].character.as_ref().map(|x| &x.slug) == Some(&c.slug) && self.sels[at].skin == p.skin;
            if !was || !same {
                let s = &mut self.sels[at];
                s.character = Some(c.clone());
                s.skin = p.skin.clone();
                s.locked = true;
                self.previewed = Some(c.clone());
                self.previewed_skin = p.skin.clone();
                let pi = Self::player_info_w(cx, 0);
                cx.visit::<PMPlayerInfo, _>(pi, |pl, cx| pl.set_character(cx, Some(&c)));
                let team = self.team_of(&self.sels[at]);
                self.change_fighter(cx, self.preview_index(0), &c, &p.skin, team);
            }
            if !was && self.state < CHARACTER_CONFIRMED {
                self.set_state(cx, IDLE);
            }
        }
        self.on_players_ready_to_matchmake(cx);
    }

    /// Online: a player's banner, badges and icon as the server handed them on (pm.locked, pm.all): kept on its
    /// MapPlayersToWidgets data (PreMatchPlayer.net_cosmetics).
    fn player_cosmetics(&mut self, cx: &mut Cx<'_>, account: &str, cosmetics: mvs_protocol::online::PlayerCosmetics) {
        let Some(at) = self.sel_of_account(account) else { return };
        let player = self.sels[at].player;
        self.setup.players[player].net_cosmetics = Some(cosmetics);
        // (the port's) a player's badges arrive with its cosmetics, after MapPlayersToWidgets: its plate takes them then
        let w = self.sels[at].widget;
        if (0..=3).contains(&w) {
            let badges = self.badges_of(cx, at);
            let pi = Self::player_info_w(cx, w as usize);
            cx.visit::<PMPlayerInfo, _>(pi, |p, cx| p.set_stat_badge_info(cx, &badges));
        }
        // hooks.cosmetics: the level reads the banners again
        if self.found_once {
            self.send_banners(cx);
        }
    }

    /// Online pm.ready: a human's perks are in (OnPlayerLockedPerks PM@53598, the "Ready" status); the perks come with
    /// match.start.
    fn player_ready_perks(&mut self, cx: &mut Cx<'_>, account: &str) {
        let Some(at) = self.sel_of_account(account) else { return };
        if !self.online_rematch() || self.is_me(at) || self.kicked {
            return;
        }
        self.player_locked_perks(cx, at);
    }

    /// The manager's "every player locked": the server's word (match.start) where it rules.
    fn check_all_perks(&mut self, cx: &mut Cx<'_>) {
        if self.server() && self.start_perks.is_none() {
            return;
        }
        if self.sels.iter().all(|x| self.setup.players[x.player].team == 4 || x.perks_locked) {
            self.on_all_players_locked_perks(cx);
        }
    }

    /// The queue's match.start, standing for the native locks and OnAllPlayersLockedPerks (the server starts when
    /// every human's perks are in, or at PREMATCH_MS): each player's four slots by fighter slot, '' an empty one (a
    /// bot's as the server made it). A start that comes before state 6 (the face-off, a page behind a hidden tab) is
    /// held until state 6's entry; the page then runs through 7 and 8.
    fn all_players_locked_perks(&mut self, cx: &mut Cx<'_>, perks: Vec<Vec<String>>) {
        if !self.server() || self.kicked || self.start_perks.is_some() {
            return;
        }
        self.start_perks = Some(perks);
        if (CHOOSING_PERKS..=REVEALING_PERKS).contains(&self.state) {
            self.apply_start_perks(cx);
        }
    }

    fn apply_start_perks(&mut self, cx: &mut Cx<'_>) {
        let Some(perks) = self.start_perks.clone() else { return };
        if self.kicked {
            return;
        }
        let slots = |i: usize| -> Vec<Option<String>> {
            perks
                .get(i)
                .map(|p| p.iter().take(4).map(|x| Some(x.clone()).filter(|x| !x.is_empty())).collect())
                .unwrap_or_default()
        };
        for i in 0..self.sels.len() {
            if self.is_me(i) {
                continue;
            }
            // (online a human's status came with pm.ready, its perks come here)
            if self.sels[i].perks_locked && !self.online_rematch() {
                continue;
            }
            self.sels[i].perks = slots(i);
            if self.sels[i].perks_locked && self.ally() == Some(i) {
                let lo = self.loadout(cx, 4);
                self.fill_loadout(cx, lo, i, true);
            }
            self.player_locked_perks(cx, i);
        }
        self.check_all_perks(cx);
        if self.me().perks_locked || self.state != CHOOSING_PERKS {
            return;
        }
        // the local player had not locked: the page's picks when it is up (its Ready, as the timer's), else the
        // server's slots of the player
        if self.equipper_bound {
            let eq = self.equipper(cx);
            if cx.view.visible_in_hierarchy(eq) {
                cx.visit::<PerksEquipper, _>(eq, |e, cx| e.ready(cx, true));
                return;
            }
        }
        let mine = slots(self.setup.me);
        self.lock_mine(cx, mine);
    }

    // ---------- the global nav and the social panel ----------

    /// FindOrCreateGlobalNavAndSocialPanel(Global Nav, Social Panel) PM@98764-99778. The nav: UI_Functions
    /// FindOrCreateGlobalNav(true), the newest visible WBP_GlobalNav (the main menu's on the seamless path), else a
    /// new one, added to CanvasPanel_ PM@99181 (AutoSize false, ZOrder 1, anchors (0,0)-(0,1), offsets 0); then
    /// UI_MyProfileIcon.ShowXPBar(true) PM@98943 and Delay(0) PM@99024 -> PM@22696: UpdateGlobalNavExitButton, and
    /// in a rematch ToggleSettingsOnly PM@22788. The social panel: FindOrCreateSocialPanel(true); a new one is added
    /// only when not bIsRematch, else it is never shown.
    /// Unverified: the new nav's slot is 0 wide (Right offset 0 with equal X anchors), read as the data says.
    fn find_or_create_global_nav_and_social_panel(&mut self, cx: &mut Cx<'_>, want_nav: bool, want_social: bool) {
        let me = cx.w;
        if want_nav {
            let g = match nav::find_global_nav(cx) {
                Some(g) => g,
                None => {
                    let w = MenuView::create(cx, "WBP_GlobalNav");
                    self.add_to_canvas(cx, w, 1, [0.0, 0.0, 0.0, 1.0]);
                    w
                }
            };
            self.nav = Some(g);
            // the nav's profile icon shows its XP bar
            let icon = nav::GlobalNav::profile_icon(cx, g);
            cx.visit::<nav::PartyMemberSlot, _>(icon, |s, cx| s.show_xp_bar(cx, true));
            cx.view.delay(0.0, Handler { owner: me, id: D_NAV });
        }
        if want_social {
            let found = nav::live(cx, "UI_SocialPanel").into_iter().rev().find(|&p| cx.view.visible_in_hierarchy(p));
            if found.is_some() {
                self.social = found;
            }
        }
    }

    /// UI_Functions AddWidgetToCanvas(CanvasPanel_, w, AutoSize false, ZOrder, anchors, offsets 0).
    fn add_to_canvas(&self, cx: &mut Cx<'_>, w: WidgetId, z: i32, anchors: [f64; 4]) {
        let canvas = find(cx, "CanvasPanel_");
        MenuView::add_child(cx, canvas, w, None);
        let x = wd(cx, w);
        x.paint_order = z;
        x.auto_size = false;
        x.anchors = anchors;
        x.offsets = crate::slate::Margin::default();
        x.align = [0.0, 0.0];
    }

    /// Update Global Nav Exit Button PM@111915-113056 (from SetState PM@34168 and the find's Delay): the back arrow
    /// with the state's tooltip ("Return Home" in 0 and 1, and an arcade rematch's 2 PM@112727-112769, "Back" in 2 to
    /// 10, "" in 11), ExitEvent cleared and bound to OnGlobalNavExit PM@111391-111446, and the button shown in states
    /// 1 to 5 PM@111646.
    fn update_global_nav_exit_button(&self, cx: &mut Cx<'_>) {
        let Some(n) = self.nav.filter(|&n| !cx.view.is_destroyed(n)) else { return };
        let s = self.state;
        let key = if s == CHOOSING_SKIN && self.arcade_rematch() {
            Some(NAV_EXIT_ARCADE_REMATCH)
        } else {
            usize::try_from(s).ok().and_then(|i| NAV_EXIT_TEXT.get(i).copied())
        };
        let tip = key.map(|k| cx.view.game_text(k).to_string()).unwrap_or_default();
        let me = cx.w;
        let on = (CHOOSING_CHARACTER..=MATCHMAKING).contains(&s);
        cx.visit::<GlobalNav, _>(n, |g, cx| {
            g.customize_exit_button(cx, 1, &tip);
            g.exit_event.clear();
            g.exit_event.add(Handler { owner: me, id: H_NAV_EXIT });
            g.toggle_exit_button(cx, on);
        });
    }

    /// OnGlobalNavExit -> BPI_GlobalNavStepBack PM@108657: ui_menu_step_back, the perk page's Exit while
    /// WaitingForPerksPanel? holds (PM@108712), then StepBack PM@69718.
    fn on_global_nav_exit(&mut self, cx: &mut Cx<'_>) {
        cx.post("ui_menu_step_back");
        if self.waiting_for_perks_panel(cx) {
            let eq = self.equipper(cx);
            cx.visit::<PerksEquipper, _>(eq, |e, cx| e.exit(cx));
        }
        self.step_back(cx, true);
    }

    /// WaitingForPerksPanel? (function, @0-534): UI_Perks_Equipper_P0's IsBusy? OR IsVisible() (its own visibility,
    /// not the hierarchy's). A page not built yet is neither.
    fn waiting_for_perks_panel(&mut self, cx: &mut Cx<'_>) -> bool {
        if !self.equipper_bound {
            return false;
        }
        let eq = self.equipper(cx);
        cx.visit::<PerksEquipper, _>(eq, |e, cx| e.is_busy(cx)).unwrap_or(false) || shown(cx, eq)
    }

    /// ExitGlobalNav PM@95829-96443 (MatchFoundState): the nav to the manager's Ignore Context, Collapsed; the social
    /// panel the same PM@96216; UnfocusAll(0..3) PM@96188; UI_PM_StateInfo.HandleGlobalNavVisibility(false) and the
    /// two spacers Collapsed PM@96074. (SetIsEnabled(false): the port has no enabled state on a widget; Collapsed
    /// leaves it no input.)
    fn exit_global_nav(&mut self, cx: &mut Cx<'_>) {
        let ignore = cx.view.manager.ignore_context.clone();
        if let Some(n) = self.nav.filter(|&n| !cx.view.is_destroyed(n)) {
            cx.visit::<GlobalNav, _>(n, |g, cx| g.update_context_global_nav(cx, &ignore, true, false));
            wd(cx, n).vis = Vis::Collapsed;
        }
        if let Some(sp) = self.social.filter(|&n| !cx.view.is_destroyed(n)) {
            cx.visit::<SocialPanel, _>(sp, |p, cx| p.update_context(cx, &ignore));
            wd(cx, sp).vis = Vis::Collapsed;
        }
        for p in 0..=3 {
            Manager::unfocus_all(cx, p);
        }
        let si = Self::state_info(cx);
        cx.visit::<PMStateInfo, _>(si, |s, cx| s.handle_global_nav_visibility(cx, false));
        set_vis(cx, "GlobalNavSpacer", Vis::Collapsed);
        set_vis(cx, "GN_PerkSpacer", Vis::Collapsed);
    }

    /// HideWaitInLab @0 / @38.
    fn hide_wait_in_lab(&self, cx: &mut Cx<'_>) {
        let wait = Self::wait_in_lab_w(cx);
        wd(cx, wait).vis = Vis::Collapsed;
        let i = interactor_of(cx, wait);
        set_interactable(cx, i, false);
    }

    // ---------- the timer ----------

    /// OnTimeExpired PM@57789.
    fn on_time_expired(&mut self, cx: &mut Cx<'_>) {
        Manager::dismiss_all_modals(cx);
        match self.state {
            LOADING => {
                self.set_state(cx, CHOOSING_CHARACTER);
                return;
            }
            // PM@58100-58330: 5 -> SetState(6); 6 -> the equipper's Ready(true), then the backstop; 7 -> the
            // backstop; 8 -> SetState(9)
            MATCHMAKING => {
                self.set_state(cx, CHOOSING_PERKS);
                return;
            }
            CHOOSING_PERKS => {
                let eq = self.equipper(cx);
                cx.visit::<PerksEquipper, _>(eq, |e, cx| e.ready(cx, true));
                self.perk_backstop(cx);
                return;
            }
            PERKS_CONFIRMED => {
                self.perk_backstop(cx);
                return;
            }
            REVEALING_PERKS => {
                self.set_state(cx, READY);
                return;
            }
            _ => {}
        }
        if self.state < CHOOSING_CHARACTER || self.state > CHARACTER_CONFIRMED {
            return;
        }
        // PM@58349: the local bSkinLocked and not AllPerksSelectedNotificationReceived (never set before state 6)
        if self.me().locked {
            return;
        }
        self.set_state(cx, IDLE);
        // HandleTimerExpired PM@58501 (native 141220f90 -> 14106e260 -> 1410a36e0): the local player is locked to the
        // focused pick, else to the last valid one (a fighter the port has). The port's own after those: the last
        // chosen one, the first ported fighter. The skin: the previewed one (HandleSkinFocused), the last valid one
        // with the last valid character, else the pick's stored one.
        let last = self.last_chosen_character(cx);
        let roster = cx.view.roster().to_vec();
        let pick = [self.previewed.clone(), self.last_valid.clone(), last]
            .into_iter()
            .flatten()
            .chain(roster)
            .find(|r| r.fighter.is_some());
        let Some(pick) = pick else { return };
        let same_previewed = self.previewed.as_ref().map(|p| &p.slug) == Some(&pick.slug);
        let kept = if same_previewed {
            self.previewed_skin.clone()
        } else if self.last_valid.as_ref().map(|p| &p.slug) == Some(&pick.slug) {
            self.last_valid_skin.clone()
        } else {
            String::new()
        };
        if !same_previewed {
            self.on_character_focused(cx, Some(&pick));
        }
        if !kept.is_empty() && kept != self.previewed_skin {
            self.previewed_skin = kept.clone();
            self.last_valid_skin = kept.clone();
            let team = self.team_of(self.me());
            self.change_fighter(cx, self.preview_index(0), &pick, &kept, team);
        }
        let (at, skin) = (self.setup.me, self.previewed_skin.clone());
        self.lock(cx, at, &pick, &skin);
        self.check_all_locked(cx);
        // RematchTimeoutHandle PM@51907: 15 s for every pick, then "A player has disconnected." {03E841C5}
        // (OnPlayersReadyToMatchmake clears it). Not the queue's path (its state 1-4 has no timer).
        if !self.queue() {
            let me = cx.w;
            cx.view.delay(REMATCH_TIMEOUT_S, Handler { owner: me, id: D_REMATCH_TIMEOUT });
        }
    }

    // ---------- the face-off and the exit ----------

    /// OnPlayersReadyToMatchmake PM@103042 -> PM@83376 -> PlayersFound PM@88016.
    fn on_players_ready_to_matchmake(&mut self, cx: &mut Cx<'_>) {
        if self.queue() {
            if self.state == CHARACTER_CONFIRMED && !self.kicked {
                self.start_matchmaking(cx);
            }
            return;
        }
        if self.all_characters_chosen {
            return;
        }
        self.all_characters_chosen = true;
        // The arcade (not a rematch PM@88323: CanBack? false, then state 11 and RecreateArcadeRun PM@35470, state 5's
        // ShowArcadeProgression PM@41457). Deviation: the port's run is made already, so states 11 and 5 (the
        // spinner, the search) are skipped: the pips show here and the face-off follows at once, as a rematch's.
        if self.setup.arcade.is_some() {
            self.can_back = false;
            let si = Self::state_info(cx);
            cx.visit::<PMStateInfo, _>(si, |s, cx| {
                s.show_arcade_progression(cx);
                s.update_arcade_details(cx);
            });
        }
        self.players_found(cx);
        // PM@83430-84054: the cards' info, their perks hidden until their Enter, no ready check, no spinner
        self.reset_loadouts(cx, true);
        self.bot_perk_selects(cx);
    }

    /// PlayersFound's loadout set-up: a rematch's PM@83430-84054 (readyCheck): Update Character Info of P0-P3 and the
    /// ally preview, Toggle Perks Display(false) on P0-P3 and (true) on the ally preview; the rematch's
    /// ShowReadyCheck(false) and ShowStatusSpinner(false) on P0-P3.
    fn reset_loadouts(&mut self, cx: &mut Cx<'_>, ready_check: bool) {
        for i in 0..4 {
            let lo = self.loadout(cx, i);
            let sel = self.sels.iter().position(|x| x.widget == i as i32).and_then(|a| self.selection_of(cx, a));
            cx.visit::<PerksLoadout, _>(lo, |l, cx| {
                if sel.is_some() {
                    l.selection = sel;
                }
                l.update_character_info(cx);
                l.toggle_perks_display(cx, false);
                if ready_check {
                    l.set_status(cx, false, false);
                }
            });
        }
        let ally = self.loadout(cx, 4);
        let sel = self.ally().and_then(|a| self.selection_of(cx, a));
        cx.visit::<PerksLoadout, _>(ally, |l, cx| {
            if sel.is_some() {
                l.selection = sel;
            }
            l.update_character_info(cx);
            l.toggle_perks_display(cx, true);
        });
    }

    /// PlayersFound PM@88016: a rematch's at once (PM@83376).
    fn players_found(&mut self, cx: &mut Cx<'_>) {
        if self.found_once {
            return;
        }
        self.found_once = true;
        if self.queue() {
            cx.post("ui_map_matchfound");
            // MatchFoundState, not a rematch PM@107827
            let si = Self::state_info(cx);
            let t = cx.view.game_text(TEXT_MATCH_FOUND).to_string();
            cx.visit::<PMStateInfo, _>(si, |s, cx| s.set_info(cx, &t, -1.0, false, 10.0, false, ""));
        }
        // MatchFoundState PM@77509 -> PM@107981 (no header text in a rematch), PM@77559
        set_vis_all(
            cx,
            &[
                "UI_PM_CharacterInfo_V2_P0",
                "UI_PM_ConfirmP0",
                "UI_PM_SkinSelectorP0",
                "UI_PM_GameplayTips",
                "buttonClaim",
                "ShowMoveListP0",
                "EditPerksP0",
                "UI_PM_Cosmetics",
                "UI_RestedXPBar",
                "VO_Clickable",
                "CharGridShadow",
                "UI_Perks_LoadoutAllyPreview",
                "UI_PM_BoostInfo",
            ],
            Vis::Collapsed,
        );
        Manager::dismiss_all_modals(cx);
        // ExitGlobalNav PM@108622, then HideWaitInLab PM@108637
        self.exit_global_nav(cx);
        self.hide_wait_in_lab(cx);
        let b = Self::back_to_lobby_w(cx);
        wd(cx, b).vis = Vis::Collapsed;
        let me = cx.w;
        cx.view.delay(2.0, Handler { owner: me, id: D_PLAYERS_FOUND });
    }

    /// PlayersFound's Delay(2) PM@18791: the opponents are revealed here.
    fn opponents_revealed(&mut self, cx: &mut Cx<'_>) {
        self.map_players_to_widgets(cx, true);
        self.players_found = true;
        if self.level {
            // The level's: the game state's skin data by preview slot (SkinDataReceived), then PlayersFound, whose
            // sequence shows each preview's fighter (BPI_ShowPlayer)
            let mut d = PreMatchSkinData {
                teammates: vec![None; 4],
                enemies: vec![None; 4],
                usernames: vec![String::new(); 4],
            };
            let my_team = self.team_of(self.me());
            for s in &self.sels {
                let i = self.preview_index(s.widget);
                if s.widget < 0 || i > 3 {
                    continue;
                }
                let p = &self.setup.players[s.player];
                d.usernames[i] = p.username.clone();
                if let Some(c) = &s.character {
                    let spec = spec_of(c, &s.skin, p.team);
                    if p.team == my_team {
                        d.teammates[i] = spec;
                    } else {
                        d.enemies[i] = spec;
                    }
                }
            }
            let names = d.usernames.clone();
            cx.scene(SceneCmd::Sequence(SeqCmd::SkinData(Box::new(d))));
            self.set_plates(cx, &names);
            self.send_banners(cx);
            self.transition_hook(cx, "PlayersFound");
            return;
        }
        self.timed_face_off(cx);
    }

    /// bannerSlugOf: the local player's equipped banner, another's as the server handed it
    /// on (net_cosmetics), a bot's drawn one. The icon: a queue bot's by its account id (`queue_bot_icon`).
    fn send_banners(&mut self, cx: &mut Cx<'_>) {
        let queue = self.queue();
        let own = crate::menu::lobby::loadout::local_banner(&cx.prefs.hydra);
        for (at, s) in self.sels.iter().enumerate() {
            let i = self.preview_index(s.widget);
            if s.widget < 0 || i > 3 {
                continue;
            }
            let p = &self.setup.players[s.player];
            let slug = if at == self.setup.me {
                own.clone()
            } else if let Some(c) = &p.net_cosmetics {
                c.banner.slug.clone()
            } else {
                p.cosmetics.as_ref().map(|c| c.banner.clone()).unwrap_or_default()
            };
            cx.scene(SceneCmd::Banner { slot: i, slug: (!slug.is_empty()).then_some(slug) });
            let path = {
                let bundle = cx.view.bundle.clone();
                let mine = cx.prefs.hydra.profile_icon.slug.clone();
                let bot_icon = if queue && p.is_bot { queue_bot_icon(&p.account_id) } else { None };
                let of = if at == self.setup.me {
                    BannerIconOf::Slug(&mine)
                } else if let Some(icon) = bot_icon {
                    BannerIconOf::Asset(icon)
                } else if let Some(c) = &p.net_cosmetics {
                    // (a bot's drawn from its seed carries the slug alone: cosmetics_of_loadout)
                    if c.icon.asset.is_empty() {
                        BannerIconOf::Slug(&c.icon.slug)
                    } else {
                        BannerIconOf::Asset(&c.icon.asset)
                    }
                } else {
                    BannerIconOf::Slug(p.cosmetics.as_ref().map_or("", |c| c.icon.as_str()))
                };
                crate::menu::hydra::cosmetics_in(&bundle).and_then(|c| {
                    banner_icon_texture(c, of, |name| bundle.textures.get(name).map(|t| t.entry().to_string()))
                })
            };
            cx.scene(SceneCmd::BannerIcon { slot: i, path });
        }
    }

    /// No level (or one that failed after PlayersFound went to it): the opponents' previews, and the level's
    /// Seq_AnimInRed / Seq_AnimInFFA_New event times, each ShowPlayer(index) sending BPI_SequenceEvent(index).
    pub fn timed_face_off(&mut self, cx: &mut Cx<'_>) {
        if self.kicked {
            return;
        }
        for i in 0..self.sels.len() {
            let s = self.sels[i].clone();
            if s.widget > 0
                && let Some(c) = &s.character
            {
                self.change_fighter(cx, self.preview_index(s.widget), c, &s.skin, self.team_of(&s));
            }
        }
        let me = cx.w;
        let times = if self.setup.team_style == 2 { &FACE_OFF_TIMES_FFA } else { &FACE_OFF_TIMES };
        for (i, (_, t)) in times.iter().enumerate() {
            cx.view.delay(*t, Handler { owner: me, id: D_SEQ + i as u32 });
        }
        // the sequence's end at 2.0 s -> the level's OnAnimInComplete @38233: BPI_TransitionComplete("FaceOff")
        cx.view.delay(2.0, Handler { owner: me, id: D_FACE_OFF_DONE });
    }

    /// BPI_SequenceEvent(Name) PM@85647, from the level's sequences.
    pub fn sequence_event(&mut self, cx: &mut Cx<'_>, name: &str) {
        if matches!(name, "1" | "2" | "3") {
            // PM@86534: the index's Player Info (in 1v1: 1 -> 2, 2 -> 1), and once the local one's PM@31516
            let n: usize = name.parse().unwrap_or(0);
            let at = if self.setup.team_style == 0 {
                match n {
                    2 => 1,
                    1 => 2,
                    n => n,
                }
            } else {
                n
            };
            let pi = Self::player_info_w(cx, at);
            cx.visit::<PMPlayerInfo, _>(pi, |p, cx| p.force_show_diamond(cx));
            if !self.diamond_once {
                self.diamond_once = true;
                let p0 = Self::player_info_w(cx, 0);
                cx.visit::<PMPlayerInfo, _>(p0, |p, cx| p.force_show_diamond(cx));
            }
            return;
        }
        if name == "ShowFFA" {
            // PM@87170
            let w = Self::mode_label_w(cx);
            let x = wd(cx, w);
            x.anchors = [1.0, 0.5, 1.0, 0.5];
            x.align = [0.5, 0.5];
            x.offsets.l = -200.0;
            x.offsets.t = 0.0;
        }
        if name != "ShowVS" && name != "ShowFFA" {
            // "ToCharSelectFinished" (PM@85939 -> @85954): BPI_PreMatchTransition("UpdateLocations"), then
            // UI_Perks_LoadoutP2 ShowStatusSpinner(IsPartyFull || bIsRematch) PM@86391-86451 and ShowReadyCheck(false)
            // PM@86496. Any other name ("" from the camera lerp's end) ends at @85953.
            // Unverified: IsPartyFull (GetPartyData, native) is read as being in a party of two (the port's queue
            // party role), as the build's NumJoined > 1.
            if name == "ToCharSelectFinished" {
                self.transition_hook(cx, "UpdateLocations");
                let spinner = super::is_rematch(&self.setup) || self.party_role().is_some();
                let p2 = find(cx, LOADOUTS[2]);
                cx.visit::<PerksLoadout, _>(p2, |l, cx| {
                    l.show_status_spinner(cx, spinner);
                    l.show_ready_check(cx, false);
                });
            }
            return;
        }
        // PM@87518 (a rematch): every Player Info's diamond, the mode label, Delay(2.0) -> SetState(6) PM@21826
        for i in 0..4 {
            if self.queue() {
                break;
            }
            let pi = Self::player_info_w(cx, i);
            cx.visit::<PMPlayerInfo, _>(pi, |p, cx| p.force_show_diamond(cx));
        }
        let ml = Self::mode_label_w(cx);
        cx.visit::<PMModeLabel, _>(ml, |m, cx| m.enter(cx));
        let me = cx.w;
        cx.view.delay(2.0, Handler { owner: me, id: D_TO_PERKS });
    }

    /// BPI_TransitionComplete(Type) PM@110261: "FaceOff" PM@109798 shows the loadouts P0 and P1, and P2 / P3 but in
    /// 1v1, then ApplyPerkLocations. Unverified: what fills them is native (OnPerksDetermined, not decoded); the port
    /// gives P0 the local player's saved loadout here and the others theirs at the reveal.
    pub fn transition_complete(&mut self, cx: &mut Cx<'_>, name: &str) {
        if name != "FaceOff" || self.kicked {
            return;
        }
        let solo = self.setup.team_style == 0;
        for i in 0..4 {
            let lo = self.loadout(cx, i);
            wd(cx, lo).vis = if i >= 2 && solo { Vis::Collapsed } else { Vis::Visible };
            let a = self.sels.iter().position(|x| x.widget == i as i32);
            let sel = a.and_then(|a| self.selection_of(cx, a));
            let fighter = a.and_then(|a| self.sels[a].character.as_ref().and_then(|c| c.fighter.clone()));
            cx.visit::<PerksLoadout, _>(lo, |l, cx| {
                l.selection = sel;
                l.update_character_info(cx);
            });
            if i == 0 {
                if let Some(a) = a {
                    self.sels[a].perks = remembered_perks(cx, fighter.as_deref());
                }
                let f = fighter.clone();
                cx.visit::<PerksLoadout, _>(lo, |l, cx| {
                    l.character = f.clone();
                    l.set_character(cx, f.as_deref());
                });
            } else {
                show_loadout(cx, lo, fighter.as_deref(), &[]);
            }
        }
        self.apply_perk_locations(cx);
    }

    // ---------- the perks (states 6-8) ----------

    /// The loadout's info line from the selection (the selector's character, skin and level). Deviation: the build
    /// shows the server's mastery level; the port shows 49 for a human and its own BOT tag for a bot.
    fn selection_of(&self, cx: &Cx<'_>, a: usize) -> Option<LoadoutSelection> {
        let s = &self.sels[a];
        let c = s.character.as_ref()?;
        let skins =
            c.fighter.as_deref().and_then(|f| cx.view.fighters().get(f)).map(|f| f.skins.as_slice()).unwrap_or(&[]);
        let skin = if s.skin.is_empty() { None } else { skins.iter().find(|k| same_skin_name(&k.name, &s.skin)) }
            .or_else(|| skins.iter().find(|k| k.is_default));
        Some(LoadoutSelection {
            name: c.display_name.clone(),
            skin: skin.map(|k| k.display_name.clone()).unwrap_or_default(),
            level: if self.setup.players[s.player].is_bot { None } else { Some(49) },
        })
    }

    /// A player's card: the selection's info and the perks it locked (none yet: empty slots).
    fn fill_loadout(&self, cx: &mut Cx<'_>, lo: WidgetId, a: usize, perks: bool) {
        let sel = self.selection_of(cx, a);
        cx.visit::<PerksLoadout, _>(lo, |l, _| l.selection = sel);
        let s = &self.sels[a];
        let fighter = s.character.as_ref().and_then(|c| c.fighter.clone());
        let list = if perks { s.perks.clone() } else { Vec::new() };
        show_loadout(cx, lo, fighter.as_deref(), &list);
    }

    /// ShowPerksNow PM@93369 -> PM@64955: "Select Perks", 40 s; the header's fade in over 0.5 s (a Delay(0) loop,
    /// PM@65284 -> PM@1925); then PM@2489: the page: Toggle Perks Equipper(true) when the equipper is not up PM@3139
    /// / PM@4045; a 2v2: the ally preview shown and entered, ApplyPerkLocations. (UpdatePlayerWidgetStatus and the
    /// plates' SetWaiting draw nothing in the build's tree; the ally mode's info and re-equip are ally sharing: off.)
    fn show_perks_now(&mut self, cx: &mut Cx<'_>) {
        let si = Self::state_info(cx);
        let t = cx.view.game_text(TEXT_SELECT_PERKS).to_string();
        cx.visit::<PMStateInfo, _>(si, |s, cx| s.set_info(cx, &t, PERK_TIME_S, false, 10.0, false, ""));
        let hc = cx.view.tree().find_in(si, "StateInfoHeaderContainer");
        wd(cx, hc).opacity = 0.0;
        self.header_fade_start = cx.now();
    }

    fn after_header_fade(&mut self, cx: &mut Cx<'_>) {
        if self.state != CHOOSING_PERKS || self.kicked {
            return;
        }
        let eq = self.equipper(cx);
        if !cx.view.visible_in_hierarchy(eq) {
            let fighter = self.me().character.as_ref().and_then(|c| c.fighter.clone());
            cx.visit::<PerksEquipper, _>(eq, |e, cx| e.set_character(cx, fighter.as_deref()));
            let at = self.setup.me;
            self.hold_perk_pictures(cx, &[at]);
            self.toggle_perks_equipper(cx, true);
        }
        if self.setup.team_style == 1 {
            let lo = self.loadout(cx, 4);
            wd(cx, lo).vis = Vis::SelfHitTestInvisible;
            cx.visit::<PerksLoadout, _>(lo, |l, cx| l.enter(cx));
            self.apply_perk_locations(cx);
            // PM@3532 -> PM@3840: Delay(0.5) -> Refresh Loadout Stacks PM@3895
            let me = cx.w;
            cx.view.delay(0.5, Handler { owner: me, id: D_REFRESH_STACKS });
            return;
        }
        self.apply_perk_locations(cx);
    }

    /// Refresh Loadout Stacks PM@117595 (2v2 alone): the local loadout (the perk page's, GetEquippedPerks(true)) is
    /// the ally preview's and P2's Ally Equipped Perks, the ally preview's those of P0 and the perk page, and the
    /// two opponents' P1 and P3 each other's.
    fn refresh_loadout_stacks(&mut self, cx: &mut Cx<'_>) {
        if self.setup.team_style != 1 {
            return;
        }
        let ids = |l: &mut PerksLoadout, _: &mut Cx<'_>| -> Vec<String> {
            l.equipped_perks(true).into_iter().flatten().map(|p| p.id).collect()
        };
        let eq = find(cx, "UI_Perks_Equipper_P0");
        let own_lo = cx.view.tree().find_in(eq, "UI_Perks_Loadout");
        let ally_lo = find(cx, ALLY_PREVIEW);
        let [p0, p1, p2, p3] = LOADOUTS.map(|n| find(cx, n));
        let set = |cx: &mut Cx<'_>, w: WidgetId, list: &[String]| {
            cx.visit::<PerksLoadout, _>(w, |l, cx| l.set_ally_equipped(cx, list));
        };
        // PM@117659-117774
        let own = cx.visit::<PerksLoadout, _>(own_lo, ids).unwrap_or_default();
        set(cx, ally_lo, &own);
        set(cx, p2, &own);
        // PM@117404-117545
        let ally = cx.visit::<PerksLoadout, _>(ally_lo, ids).unwrap_or_default();
        set(cx, p0, &ally);
        cx.visit::<PerksEquipper, _>(eq, |e, cx| e.set_ally_equipped(cx, &ally));
        // PM@117217-117358
        let one = cx.visit::<PerksLoadout, _>(p1, ids).unwrap_or_default();
        set(cx, p3, &one);
        let three = cx.visit::<PerksLoadout, _>(p3, ids).unwrap_or_default();
        set(cx, p1, &three);
    }

    /// Toggle Perks Equipper(Visible) PM@120218: [perksBGDimmer, the equipper] Visible / Collapsed, and Enter(-1).
    fn toggle_perks_equipper(&mut self, cx: &mut Cx<'_>, show: bool) {
        let eq = self.equipper(cx);
        let v = if show { Vis::Visible } else { Vis::Collapsed };
        set_vis(cx, "perksBGDimmer", v);
        wd(cx, eq).vis = v;
        if show {
            cx.visit::<PerksEquipper, _>(eq, |e, cx| e.enter(cx, -1));
        }
    }

    /// The pictures of the perk page and the cards of these players (pictures held for the screen's life).
    fn hold_perk_pictures(&mut self, cx: &mut Cx<'_>, sels: &[usize]) {
        let mut names: Vec<String> = Vec::new();
        for &a in sels {
            let f = self.sels[a].character.as_ref().and_then(|c| c.fighter.clone());
            for n in perk_pictures(cx.view, f.as_deref()) {
                if !names.contains(&n) {
                    names.push(n);
                }
            }
        }
        let owner =
            format!("prematch-perks-{}", sels.iter().map(|&a| self.sels[a].widget.to_string()).collect::<String>());
        cx.view.hold_pictures(&owner, names.iter().map(String::as_str));
        if !self.held.contains(&owner) {
            self.held.push(owner);
        }
    }

    /// OnPerksButton(0) PM@97308 (EditPerksP0 in states 2-5): not while the page is busy or up; Confirm, the perks
    /// button, the skin strip, the move list and cosmetics Collapsed, the page Visible and Enter(-1).
    fn on_perks_button(&mut self, cx: &mut Cx<'_>) {
        let eq = self.equipper(cx);
        let busy = cx.visit::<PerksEquipper, _>(eq, |e, cx| e.is_busy(cx)).unwrap_or(false);
        if busy || shown(cx, eq) {
            return;
        }
        set_vis_all(
            cx,
            &["UI_PM_ConfirmP0", "EditPerksP0", "UI_PM_SkinSelectorP0", "ShowMoveListP0", "UI_PM_Cosmetics"],
            Vis::Collapsed,
        );
        // (the port's: the equipper's character is the previewed one, as the build's selector gives it)
        let fighter = self.previewed.as_ref().and_then(|p| p.fighter.clone());
        cx.visit::<PerksEquipper, _>(eq, |e, cx| e.set_character(cx, fighter.as_deref()));
        let at = self.setup.me;
        self.hold_perk_pictures(cx, &[at]);
        wd(cx, eq).vis = Vis::Visible;
        cx.visit::<PerksEquipper, _>(eq, |e, cx| e.enter(cx, -1));
    }

    /// OnPerksLocked(PresetIndex) PM@65992 (the equipper's PerksSavedEvent): state 2 PM@66453: Confirm, the perks
    /// button, the skin strip, the move list and cosmetics Visible again; 3-5 PM@66218: the perks button and the move
    /// list; (both then the button's focus after 0.2 s PM@66398); state 6 PM@31282: the local OnPlayerLockedPerks,
    /// the local plate's diamond PM@31516, the players' focus dropped PM@31672, then SetState(7).
    fn on_perks_locked(&mut self, cx: &mut Cx<'_>, ids: Vec<Option<String>>) {
        let s = self.state;
        if s == CHOOSING_SKIN {
            set_vis_all(
                cx,
                &["UI_PM_ConfirmP0", "EditPerksP0", "UI_PM_SkinSelectorP0", "ShowMoveListP0", "UI_PM_Cosmetics"],
                Vis::Visible,
            );
        } else if (CONFIRMING_CHARACTER..=MATCHMAKING).contains(&s) {
            set_vis_all(cx, &["EditPerksP0", "ShowMoveListP0"], Vis::Visible);
        } else if s == CHOOSING_PERKS {
            // HandlePerksLocked (the server's): the four slots, '' an empty one
            self.hook(cx, PreMatchCmd::PerksLocked(ids.iter().map(|x| x.clone().unwrap_or_default()).collect()));
            self.lock_mine(cx, ids);
            return;
        } else {
            return;
        }
        let me = cx.w;
        cx.view.delay(0.2, Handler { owner: me, id: D_PERKS_FOCUS });
    }

    fn lock_mine(&mut self, cx: &mut Cx<'_>, ids: Vec<Option<String>>) {
        let at = self.setup.me;
        self.sels[at].perks = ids;
        let p0 = Self::player_info_w(cx, 0);
        cx.visit::<PMPlayerInfo, _>(p0, |p, cx| p.force_show_diamond(cx));
        for p in 0..=3 {
            Manager::unfocus_all(cx, p);
        }
        let lo = self.loadout(cx, 0);
        self.fill_loadout(cx, lo, at, true);
        self.set_state(cx, PERKS_CONFIRMED);
        self.player_locked_perks(cx, at);
    }

    /// The bots' perks (a local game): CoopVsAIManager OnPlayersReadyForMatchmaking CO@6475 -> PerkSelectDelay 5.0 s
    /// -> ReadyBots CO@2827, here from OnPlayersReadyToMatchmake (every character locked). Unverified: ReadyBots' lock
    /// reaching OnPlayerLockedPerks is native (not traced). The perks are a custom lobby bot's (lobby_bot_perks), the
    /// list the match takes too (menu/lobby). Deviation: the build's custom-lobby bots lock 9.0 s after
    /// OnAllMatchPlayersRegistered, not 5.0 s.
    fn bot_perk_selects(&mut self, cx: &mut Cx<'_>) {
        let me = cx.w;
        for (i, s) in self.sels.iter().enumerate() {
            let p = &self.setup.players[s.player];
            if !p.is_bot || p.team == 4 || s.character.as_ref().and_then(|c| c.fighter.as_ref()).is_none() {
                continue;
            }
            cx.view.delay(BOT_PERKS_S, Handler { owner: me, id: D_BOT_PERKS + i as u32 });
        }
    }

    /// Native OnPlayerLockedPerks(AccountID) PM@53598 -> PM@52777: the player's plate SetStatus (no text drawn: the
    /// build's tree), its loadout's ready check PM@53263; then the manager's "every player locked" ->
    /// OnAllPlayersLockedPerks. Deviation: the ally preview's perks are the ally's lock here (ally sharing, which
    /// fills it in the build, is off).
    fn player_locked_perks(&mut self, cx: &mut Cx<'_>, a: usize) {
        if self.sels[a].perks_locked {
            return;
        }
        self.sels[a].perks_locked = true;
        let w = self.sels[a].widget;
        if (0..=3).contains(&w) {
            let lo = self.loadout(cx, w as usize);
            cx.visit::<PerksLoadout, _>(lo, |l, cx| l.set_status(cx, true, false));
        }
        if self.ally() == Some(a) {
            let lo = self.loadout(cx, 4);
            self.fill_loadout(cx, lo, a, true);
            // Deviation: stands for the ally preview's AllyPerksPreviewedEvent -> Refresh Loadout Stacks PM@117153
            // (the preview is filled from the ally's lock here, not by ally sharing).
            self.refresh_loadout_stacks(cx);
        }
        self.check_all_perks(cx);
    }

    /// OnAllPlayersLockedPerks PM@103318 -> PM@78118: AllPerksSelectedNotificationReceived, CanBack false,
    /// Delay(2.0, an arcade rematch's 0.2) -> SetState(8) (from state 7 alone).
    fn on_all_players_locked_perks(&mut self, cx: &mut Cx<'_>) {
        if self.all_perks_selected {
            return;
        }
        self.all_perks_selected = true;
        self.can_back = false;
        let me = cx.w;
        let wait = if self.arcade_rematch() { ARCADE_REMATCH_PERKS_LOCKED_WAIT_S } else { PERKS_LOCKED_WAIT_S };
        cx.view.delay(wait, Handler { owner: me, id: D_ALL_PERKS });
    }

    /// OnTimeExpired's state 6 / 7 PM@58233: Delay(15.0) -> PM@18098: still not every perk in:
    /// BPI_KickBackToMainMenu("A player has disconnected.", true, false) PM@18238. (A Delay node that is running
    /// takes no second start.)
    fn perk_backstop(&mut self, cx: &mut Cx<'_>) {
        if self.backstop_pending {
            return;
        }
        self.backstop_pending = true;
        let me = cx.w;
        cx.view.delay(PERK_BACKSTOP_S, Handler { owner: me, id: D_BACKSTOP });
    }

    /// BPI_KickBackToMainMenu PM@74020 and the 15 s waits' error (the main menu with a notification of the text).
    fn kick_back_to_main_menu(&mut self, cx: &mut Cx<'_>, text: &str) {
        self.cancel_timer += 1;
        Manager::set_waiting(cx, false, "", false, 0.0);
        Manager::dismiss_all_modals(cx);
        if self.kicked {
            return;
        }
        self.kicked = true;
        self.hook(cx, PreMatchCmd::Kick);
        self.exit = Some(Exit::Kick(text.to_string()));
        Manager::unfocus_all(cx, 0);
        Self::to_main_menu(cx);
    }

    /// BPI_KickBackToMainMenu's travel PGI@83617-83839 (as Back to Lobby's).
    fn to_main_menu(cx: &mut Cx<'_>) {
        Manager::transition(
            cx,
            TransitionParams {
                next_level: "New_MainMenu".into(),
                kind: 0,
                delay: 0.0,
                force: true,
                level_options: "Lobby".into(),
                ..TransitionParams::default()
            },
        );
    }

    /// State 9 PM@36345 -> PM@28541 (a DoOnce) -> PM@7791.
    fn ready(&mut self, cx: &mut Cx<'_>) {
        if self.ready_once {
            return;
        }
        self.ready_once = true;
        let si = Self::state_info(cx);
        let t = cx.view.game_text(TEXT_MATCH_STARTING).to_string();
        cx.visit::<PMStateInfo, _>(si, |s, cx| s.set_info(cx, &t, 0.0, false, 10.0, true, ""));
        cx.post("ui_menu_title_ready");
        self.calls.push(PreMatchCall::Announcer(AnnouncerLine::Outro));
        let me = cx.w;
        cx.view.delay(1.0, Handler { owner: me, id: D_READY_1 });
    }

    fn ready_after_one_second(&mut self, cx: &mut Cx<'_>) {
        let ml = Self::mode_label_w(cx);
        cx.visit::<PMModeLabel, _>(ml, |m, cx| m.exit(cx));
        Manager::dismiss_all_modals(cx);
        // Save Chosen Characters PM@113061
        if let Some(slug) = self.me().character.as_ref().map(|c| c.slug.clone()) {
            set_last_chosen(cx.prefs, slug);
        }
        // the loadouts' Exit (a card never used has nothing to fly out)
        for lo in self.built_loadouts(cx) {
            cx.visit::<PerksLoadout, _>(lo, |l, cx| l.exit(cx));
        }
        let me = cx.w;
        cx.view.play(me, "FadeOut", false, 1.0);
        // PM@8496: PM_NewSlants plays Seq_Clash (free for all: Seq_Clash_FFA_New), 3.0667 s. No scene: the sounds of
        // its master audio track at their section starts.
        if self.level {
            self.transition_hook(cx, "MatchReady");
        } else {
            cx.post("ui_prematch_clash_leadin");
            cx.view.delay(1.0833, Handler { owner: me, id: D_CLASH_IMPACT });
            cx.view.delay(2.0, Handler { owner: me, id: D_CLASH_FLASH });
        }
        // PM@8545 -> BPI_CleanUpAndStartMatch PM@8700
        cx.view.delay(3.0, Handler { owner: me, id: D_START });
    }

    // ---------- the arcade ----------

    /// The solo dialog PM@71585-71996; buttonLeftClicked -> OnArcadeQuitConfirmed PM@121252: SetIsEnabled(false),
    /// BPI_KickBackToMainMenu("", true, false), BPI_PlayMusOST (the main menu's music). The run is dropped, nothing
    /// saved. (The party's dialog PM@70986-71420 is not ported: the arcade is solo.)
    fn arcade_leave_dialog(&mut self, cx: &mut Cx<'_>) {
        let [title, body, yes, no] = LEAVE_ARCADE.map(|k| cx.view.game_text(k).to_string());
        let id = Manager::show_dialog(
            cx,
            DialogParams { title, body, button_left_label: yes, button_right_label: no, ..DialogParams::default() },
        );
        let me = cx.w;
        if let Some(d) = cx.view.dialog(id) {
            d.button_left_clicked().add(Handler { owner: me, id: H_LEAVE_ARCADE });
        }
    }

    /// "Is Arcade Rematch?" true PM@7142-7466: RefreshIndexMappings, ShowArcadeProgression PM@7189, the run's fighter
    /// focused and selected (OnCharacterFocused(ArcadeGetLockedCharacter) PM@7271, OnCharacterSelectedBase PM@7345),
    /// SetState(2) PM@7374, HideState PM@7392, the header SelfHitTestInvisible PM@7428, CanBack? false PM@7466: the
    /// fighter stays for the whole run, the skin is picked again.
    fn arcade_rematch_entry(&mut self, cx: &mut Cx<'_>) {
        let si = Self::state_info(cx);
        cx.visit::<PMStateInfo, _>(si, |s, cx| s.show_arcade_progression(cx));
        let fighter = cx
            .view
            .session
            .arcade
            .run
            .as_ref()
            .and_then(|r| r.character.clone())
            .or_else(|| self.setup.arcade.as_ref().and_then(|r| r.character.clone()));
        let c = fighter.and_then(|f| cx.view.roster().iter().find(|r| r.fighter.as_deref() == Some(&f)).cloned());
        if let Some(c) = c {
            self.on_character_focused(cx, Some(&c));
            let ss = Self::skin_selector_w(cx);
            cx.visit::<PMSkinSelector, _>(ss, |s, cx| s.set_character(cx, Some(&c)));
            self.set_grid_interactable(cx, false);
        }
        self.show_arcade_map(cx);
        self.set_state(cx, CHOOSING_SKIN);
        cx.visit::<PMStateInfo, _>(si, |s, cx| s.hide_state(cx));
        wd(cx, si).vis = Vis::SelfHitTestInvisible;
        self.can_back = false;
    }

    /// SetupArcadeRivals PM@118900 (arcade, not a rematch): the CoOpVsAIManager's rival list made the slug of
    /// PlayerInfo_0's character's ArcadeRivalSlug PM@119084, then UpdateRivalPortrait PM@119218 and
    /// UpdateArcadeDetails PM@119307. (A party's second rival PM@119558 is not ported.)
    fn setup_arcade_rivals(&mut self, cx: &mut Cx<'_>) {
        if self.setup.arcade.is_none() || self.arcade_rematch() {
            return;
        }
        let slug = self.me().character.as_ref().map(|c| c.slug.clone());
        let rival = slug.and_then(|s| {
            let menu = cx.view.bundle.menu.as_ref()?;
            let f = menu.fighters.values().find(|f| f.slug == s)?;
            Some(f.arcade.rival.clone()).filter(|r| !r.is_empty())
        });
        let si = Self::state_info(cx);
        cx.visit::<PMStateInfo, _>(si, |s, cx| {
            s.update_rival_portrait(cx, rival.as_deref());
            s.update_arcade_details(cx);
        });
    }

    /// State 4 in arcade: ArcadeSetLockedCharacter(PlayerInfo_0.Character) PM@43743 (`ArcadeState::lock`: the first
    /// lock draws the run's rounds) and SetupArcadeRivals PM@43311. Then (the port's own) the round's bot on the
    /// screen: the build's comes with the match the backend finds (state 11, RecreateArcadeRun PM@35470, then Handle
    /// All Players Registered); the port's is the run's plan, locked at once, and the round's map.
    fn arcade_locked(&mut self, cx: &mut Cx<'_>) {
        let fighter = self.me().character.as_ref().and_then(|c| c.fighter.clone());
        if let Some(f) = fighter {
            let bundle = cx.view.bundle.clone();
            if let Some(menu) = bundle.menu.as_ref() {
                let skin = self.me().skin.clone();
                cx.view.session.arcade.lock(menu, &f, &skin);
            }
        }
        self.setup_arcade_rivals(cx);
        let Some(plan) = cx.view.session.arcade.run.as_ref().and_then(|r| r.plan()).cloned() else { return };
        if self.sels.iter().any(|x| self.setup.players[x.player].is_bot) {
            return;
        }
        self.setup.players.push(PreMatchPlayer {
            account_id: BOT_ID.into(),
            username: plan.bot_name.clone(),
            team: 1,
            is_bot: true,
            character: Some(plan.bot_character.clone()),
            skin: plan.bot_skin.clone(),
            cosmetics: None,
            net_cosmetics: None,
        });
        let entry =
            cx.view.roster().iter().find(|r| r.fighter.as_deref() == Some(plan.bot_character.as_str())).cloned();
        let locked = entry.is_some();
        self.sels.push(Sel {
            player: self.setup.players.len() - 1,
            widget: 1,
            character: entry,
            skin: plan.bot_skin.clone(),
            locked,
            perks_locked: false,
            perks: Vec::new(),
        });
        self.map_players_to_widgets(cx, false);
        if !plan.map.is_empty() {
            let level = cx.view.map(&plan.map).map(|m| m.data.level).unwrap_or_default();
            self.setup.map = Some(plan.map.clone());
            self.setup.level = level;
        }
        self.show_arcade_map(cx);
    }

    /// Unverified: where an arcade round's map shows is not traced (the build's state 1, which names a rematch's map,
    /// is skipped on an arcade rematch, and a first entry's map comes from the backend's match); the port names it
    /// and sends it to the level at the rematch's entry and at the first lock.
    fn show_arcade_map(&mut self, cx: &mut Cx<'_>) {
        let Some(md) = self.setup.map.as_deref().and_then(|m| cx.view.map(m)) else { return };
        let (name, exp) = (md.data.display_name.clone(), md.data.experimental);
        let si = Self::state_info(cx);
        cx.visit::<PMStateInfo, _>(si, |s, cx| s.set_map_name(cx, &name, exp));
        self.transition_hook(cx, &md.data.level);
    }

    fn start_match(&mut self, cx: &mut Cx<'_>) {
        if self.kicked {
            return;
        }
        let picks: Vec<PreMatchPick> = self
            .sels
            .iter()
            .filter(|s| {
                self.setup.players[s.player].team != 4 && s.character.as_ref().is_some_and(|c| c.fighter.is_some())
            })
            .map(|s| PreMatchPick {
                account_id: self.setup.players[s.player].account_id.clone(),
                character: s.character.as_ref().and_then(|c| c.fighter.clone()).unwrap_or_default(),
                skin: s.skin.clone(),
                perks: s.perks.clone(),
            })
            .collect();
        // (the round as it is played, for the end of game)
        if self.setup.arcade.is_some() {
            cx.view.session.arcade.keep_round();
        }
        // hooks.startMatch: the screen's end with the picks, then the match's map
        let mine = self.me().character.as_ref().and_then(|c| Self::net_pick(c, &self.me().skin));
        if let Some(p) = mine {
            self.hook(cx, PreMatchCmd::StartMatch(p));
        }
        self.exit = Some(Exit::StartMatch(picks));
        Manager::unfocus_all(cx, 0);
        let level = if self.setup.level.is_empty() { "Map".to_string() } else { self.setup.level.clone() };
        Manager::transition(
            cx,
            TransitionParams {
                next_level: level,
                kind: 0,
                delay: 0.0,
                force: false,
                level_options: "Lobby".into(),
                ..TransitionParams::default()
            },
        );
    }

    /// The leader's Back to Lobby (BPI_TransitionToMainMenu PM@111338).
    fn back_to_lobby(&mut self, cx: &mut Cx<'_>) {
        self.hook(cx, PreMatchCmd::BackToLobby);
        self.exit = Some(Exit::BackToLobby);
        Self::to_main_menu(cx);
    }
}

impl Controller for PreMatch {
    fn on_initialized(&mut self, cx: &mut Cx<'_>) {
        self.setup = session().unwrap_or_else(|| stand_in(cx));
        // PM@57701: the mode label's team style, which picks its text and EnterAnim
        let style = i32::from(self.setup.team_style);
        let ml = Self::mode_label_w(cx);
        cx.visit::<PMModeLabel, _>(ml, |m, _| m.team_style = style);
        self.bind_events(cx);
    }

    fn destruct(&mut self, cx: &mut Cx<'_>) {
        let me = cx.w;
        let m = &mut cx.view.manager;
        m.on_submit2.remove_owner(me);
        m.on_submit3.remove_owner(me);
        m.on_cancel.remove_owner(me);
        for owner in std::mem::take(&mut self.held) {
            cx.view.release_pictures(&owner);
        }
    }

    /// Construct PM@61163: Delay(0.2), then the init chain PM@9541.
    fn construct(&mut self, cx: &mut Cx<'_>) {
        self.fill(cx);
        self.hide_all_widgets(cx, &[]);
        // UI_QuestTabs: the build shows it in states 1-5 and 11 (PM@34210 -> @82056-83098). Deviation (no starter
        // missions): Collapsed in every state from the start, as on the home, else its SectionTab's orange Div strip
        // shows with no quest data.
        set_vis(cx, "UI_QuestTabs", Vis::Collapsed);
        let me = cx.w;
        cx.view.delay(0.2, Handler { owner: me, id: D_INIT });
    }

    fn tick(&mut self, cx: &mut Cx<'_>, _dt: f64) {
        self.fill(cx);
        self.take_online(cx);
        // the points the level sent are placed once the program has projected them
        if self.checkmarks_dirty {
            self.apply_player_info_locations(cx);
            self.checkmarks_dirty =
                self.points.keys().any(|k| k.starts_with("CheckMark") && !self.projection.contains_key(k));
        }
        if self.perks_dirty {
            self.apply_perk_locations(cx);
            self.perks_dirty =
                self.points.keys().any(|k| k.starts_with("PerkInfo") && !self.projection.contains_key(k));
        }
    }

    fn timers(&mut self, cx: &mut Cx<'_>, _dt: f64) {
        if self.header_fade_start >= 0.0 {
            // ShowPerksNow's fade PM@1925-2484: Lerp(0, 1, clamp(elapsed / 0.5)), then the rest of it
            let a = ((cx.now() - self.header_fade_start) / 0.5).clamp(0.0, 1.0);
            let si = Self::state_info(cx);
            let hc = cx.view.tree().find_in(si, "StateInfoHeaderContainer");
            wd(cx, hc).opacity = a;
            if a >= 1.0 {
                self.header_fade_start = -1.0;
                self.after_header_fade(cx);
            }
        }
        if self.shadow_start < 0.0 {
            return;
        }
        let a = (cx.now() - self.shadow_start).min(1.0);
        set_opacity(cx, "CharGridShadow", 0.9 * a);
        if a >= 1.0 {
            self.shadow_start = -1.0;
        }
    }

    fn handle(&mut self, cx: &mut Cx<'_>, id: u32, arg: &dyn Any) {
        match id {
            H_TIME_EXPIRED => self.on_time_expired(cx),
            H_GRID_FOCUSED => {
                if let Some(ev) = arg.downcast_ref::<TileEvent>() {
                    self.on_character_grid_focused(cx, ev);
                }
            }
            H_GRID_SELECTED => {
                if let Some((ev, _)) = arg.downcast_ref::<(TileEvent, bool)>() {
                    self.on_character_grid_selected(cx, ev);
                }
            }
            H_GRID_POPULATED => self.on_character_grid_populated(cx),
            H_SKIN_FOCUSED => {
                if let Some(s) = arg.downcast_ref::<SkinInfo>() {
                    self.on_skin_focused(cx, s);
                }
            }
            H_SKIN_SELECTED => {
                if let Some(s) = arg.downcast_ref::<SkinInfo>() {
                    self.previewed_skin = s.name.clone();
                }
            }
            H_CONFIRM => self.on_confirm_p0(cx),
            H_BACK_TO_LOBBY => self.back_to_lobby(cx),
            H_EDIT_PERKS => self.on_perks_button(cx),
            H_MOVE_LIST => self.show_move_list(cx),
            H_CANCEL_MM => {
                // CancelMM_Ghost OnClick -> StepBack(0) PM@93374: the step-back sound PM@69663, then its body
                if !self.can_back || self.kicked {
                    return;
                }
                cx.post("ui_menu_step_back");
                self.step_back(cx, false);
            }
            H_CLICKABLE => {
                // BPI_GetPreviewActor(the click's Player Index), Delay(0.25) PM@114939 (a Delay that is running takes
                // no second click), then the actor's click line PM@23826 (Record UI Interact PM@23895 is analytics)
                if self.click_vo_pending {
                    return;
                }
                self.click_vo_pending = true;
                self.click_vo_widget = arg.downcast_ref::<i32>().copied().unwrap_or(0);
                let me = cx.w;
                cx.view.delay(0.25, Handler { owner: me, id: D_CLICK_VO });
            }
            // UI_Functions.CharacterHover(the focus's Player Index, Hovered): the hover light of the actors tagged
            // "Player{index + 1}" (the preview slot of that index), then on a hover ui_menu_character_hover
            H_CLICKABLE_FOCUS | H_CLICKABLE_UNFOCUS => {
                let on = id == H_CLICKABLE_FOCUS;
                if let Some(slot) = arg.downcast_ref::<i32>().and_then(|&p| usize::try_from(p).ok()) {
                    cx.scene(SceneCmd::Hover { slot, on });
                }
                if on {
                    cx.post("ui_menu_character_hover");
                }
            }
            H_SUBMIT2 => self.on_submit2(cx, arg.downcast_ref::<i32>().copied().unwrap_or(0)),
            H_SUBMIT3 => self.on_submit3(cx),
            H_CANCEL => {
                let p = arg.downcast_ref::<(i32, bool)>().map_or(0, |a| a.0);
                self.on_cancel(cx, p);
            }
            // OnPerksSavedP0 PM@115830: OnPerksLocked, then Refresh Loadout Stacks PM@115885
            H_EQ_SAVED => {
                if let Some(a) = arg.downcast_ref::<(i32, Option<String>, Vec<Option<String>>)>() {
                    self.on_perks_locked(cx, a.2.clone());
                }
                self.refresh_loadout_stacks(cx);
            }
            // PerksEquipperEntered PM@116292 -> PM@116090: perksBGDimmer Visible, UI_PM_StateInfo.GameModeInfo
            // Hidden, HandlePerksSelected(the page's loadout) PM@116188, Refresh Loadout Stacks PM@115885.
            // OnPerkEquipped PM@116010 (HandlePerkSelected) and OnPerksCleared PM@117168 (HandlePerksSelected of
            // four nulls) end in the same refresh.
            // Not ported: HandlePerkSelected / HandlePerksSelected, which tell the other players the perks being
            // chosen (no message of the protocol carries them; only the lock does).
            H_EQ_ENTERED => {
                set_vis(cx, "perksBGDimmer", Vis::Visible);
                let si = Self::state_info(cx);
                let g = cx.view.tree().find_in(si, "GameModeInfo");
                wd(cx, g).vis = Vis::Hidden;
                self.refresh_loadout_stacks(cx);
            }
            H_EQ_PERK | H_EQ_CLEARED => self.refresh_loadout_stacks(cx),
            // PerksEquipperExited PM@116354: perksBGDimmer Collapsed, GameModeInfo SelfHitTestInvisible, and
            // UI_Perks_AllyUnlockedInfo's Hide when it is visible PM@116297 -> PM@116053
            H_EQ_EXITED => {
                set_vis(cx, "perksBGDimmer", Vis::Collapsed);
                let si = Self::state_info(cx);
                let g = cx.view.tree().find_in(si, "GameModeInfo");
                wd(cx, g).vis = Vis::SelfHitTestInvisible;
                let info = find(cx, "UI_Perks_AllyUnlockedInfo");
                if cx.view.visible_in_hierarchy(info) {
                    wd(cx, info).vis = Vis::Collapsed;
                }
            }
            H_NAV_EXIT => self.on_global_nav_exit(cx),
            H_WAIT_LAB => self.transition_to_training(cx),
            H_MAP_REVEAL => {
                if self.state == CHOOSING_PERKS {
                    self.show_perks_now(cx);
                }
            }
            H_LEAVE_ARCADE => {
                if self.kicked {
                    return;
                }
                cx.view.session.arcade.leave();
                self.kick_back_to_main_menu(cx, "");
            }
            _ => {}
        }
    }

    fn resume(&mut self, cx: &mut Cx<'_>, id: u32) {
        match id {
            D_INIT => self.init(cx),
            D_LOADING => {
                if self.state != LOADING || self.kicked {
                    return;
                }
                // PM@4610: not the arcade and bIsMatchmaking -> the restore path PM@5079 (a queue's return from the Lab)
                if self.setup.arcade.is_none() && Self::is_matchmaking(cx) {
                    return self.restore_selections(cx);
                }
                if self.setup.arcade.is_some() {
                    let si = Self::state_info(cx);
                    cx.visit::<PMStateInfo, _>(si, |s, cx| s.show_arcade_label_header(cx));
                    if self.arcade_rematch() {
                        self.arcade_rematch_entry(cx);
                        return;
                    }
                }
                let c = self.previewed.clone().or_else(|| self.last_chosen_character(cx));
                self.on_character_focused(cx, c.as_ref());
                self.set_state(cx, CHOOSING_CHARACTER);
                self.setup_arcade_rivals(cx);
            }
            D_CLICK_VO => {
                self.click_vo_pending = false;
                let preview = self.preview_index(self.click_vo_widget);
                self.calls.push(PreMatchCall::PlayVo {
                    preview,
                    alias: "clickable".into(),
                    tags: "affinity_level_01".into(),
                    always: false,
                });
            }
            // Delay(0) PM@99024 -> PM@22696: UpdateGlobalNavExitButton, and in a rematch ToggleSettingsOnly PM@22788
            D_NAV => {
                self.update_global_nav_exit_button(cx);
                if let Some(n) = self.nav {
                    cx.visit::<GlobalNav, _>(n, |g, cx| g.toggle_settings_only(cx));
                }
            }
            D_GRID_WAIT => self.grid_wait(cx),
            D_GRID_FOCUS => self.grid_focus(cx),
            D_TO_SKIN => self.set_state(cx, CHOOSING_SKIN),
            D_CONFIRM_VO => {
                let preview = self.preview_index(0);
                self.calls.push(PreMatchCall::PlayVo {
                    preview,
                    alias: "chaselect".into(),
                    tags: String::new(),
                    always: true,
                });
            }
            D_REMATCH_TIMEOUT => {
                if !self.all_characters_chosen && !self.kicked {
                    let t = cx.view.game_text(TEXT_DISCONNECTED).to_string();
                    self.kick_back_to_main_menu(cx, &t);
                }
            }
            D_PERKS_FOCUS => {
                let ep = find(cx, "EditPerksP0");
                let b = cx.view.tree().find_in(ep, "UI_ButtonSecondary");
                if cx.view.visible_in_hierarchy(b) {
                    let i = interactor_of(cx, b);
                    interactor::set_focused(cx, i, false);
                }
            }
            D_PLAYERS_FOUND => self.opponents_revealed(cx),
            D_FACE_OFF_DONE => self.transition_complete(cx, "FaceOff"),
            D_TO_PERKS => {
                // (from the face-off's states alone: 4 a local game's, 10 its timed-out pick's)
                let s = self.state;
                if !self.kicked && (s == CHARACTER_CONFIRMED || s == IDLE || s == MATCHMAKING || s == LOADING) {
                    self.set_state(cx, CHOOSING_PERKS);
                }
            }
            D_REVEAL => {
                if self.state == REVEALING_PERKS && !self.kicked {
                    self.set_state(cx, READY);
                }
            }
            D_ALL_PERKS => {
                if !self.kicked && self.state == PERKS_CONFIRMED {
                    self.set_state(cx, REVEALING_PERKS);
                }
            }
            D_BACKSTOP => {
                self.backstop_pending = false;
                if self.all_perks_selected || self.kicked {
                    return;
                }
                let t = cx.view.game_text(TEXT_DISCONNECTED).to_string();
                self.kick_back_to_main_menu(cx, &t);
            }
            D_READY_1 => self.ready_after_one_second(cx),
            D_CLASH_IMPACT => cx.post("ui_prematch_clash_impact"),
            D_CLASH_FLASH => cx.post("ui_prematch_clash_flash"),
            D_START => self.start_match(cx),
            D_PLAYERS_READY => self.on_players_ready_to_matchmake(cx),
            D_START_PERKS => self.apply_start_perks(cx),
            D_REFRESH_STACKS => self.refresh_loadout_stacks(cx),
            _ => {
                let at = (id & 0xFF) as usize;
                match id & 0xFF00 {
                    D_CANCEL_NR => self.cancel_no_response(cx, id & 0xFF),
                    D_BOT_LOCK_A => {
                        let w = cx.w;
                        cx.view.delay(0.2, Handler { owner: w, id: D_BOT_LOCK_B + at as u32 });
                    }
                    D_BOT_LOCK_B => self.player_locked(cx, at),
                    D_BOT_PERKS => {
                        if self.kicked || self.ready_once || at >= self.sels.len() {
                            return;
                        }
                        // An arcade round's bot takes the round's perks (GetArcadeBotLoadouts: ArcadeRoundPlan
                        // bot_perks), the ones its match plays.
                        let run = self.setup.arcade.as_ref().and(cx.view.session.arcade.run.as_ref());
                        let plan_perks: Option<Vec<Option<String>>> = run
                            .and_then(|r| r.plan())
                            .map(|p| p.bot_perks.iter().map(|x| Some(x.clone()).filter(|x| !x.is_empty())).collect());
                        let fighter = self.sels[at].character.as_ref().and_then(|c| c.fighter.clone());
                        if let Some(perks) = plan_perks {
                            self.sels[at].perks = perks;
                        } else if let Some(f) = fighter {
                            self.sels[at].perks = lobby_bot_perks(cx.view, &f);
                        }
                        self.hold_perk_pictures(cx, &[at]);
                        self.player_locked_perks(cx, at);
                    }
                    D_SEQ => {
                        let times = if self.setup.team_style == 2 { &FACE_OFF_TIMES_FFA } else { &FACE_OFF_TIMES };
                        if let Some((name, _)) = times.get(at) {
                            self.sequence_event(cx, name);
                        }
                    }
                    _ => {}
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(slug: &str, fighter: Option<&str>) -> RosterEntry {
        RosterEntry {
            slug: slug.into(),
            display_name: slug.into(),
            fighter: fighter.map(str::to_string),
            ..RosterEntry::default()
        }
    }

    fn player(id: &str, team: u8, bot: bool, character: Option<&str>) -> PreMatchPlayer {
        PreMatchPlayer {
            account_id: id.into(),
            team,
            is_bot: bot,
            character: character.map(str::to_string),
            ..PreMatchPlayer::default()
        }
    }

    fn setup(style: u8, players: Vec<PreMatchPlayer>) -> PreMatchSetup {
        PreMatchSetup { players, me: 0, team_style: style, ..PreMatchSetup::default() }
    }

    #[test]
    fn widgets_1v1_the_opponent_is_widget_one() {
        let s = setup(0, vec![player("me", 0, false, None), player("b", 1, true, Some("shaggy"))]);
        let roster = [entry("shaggy", Some("shaggy")), entry("x", None)];
        let sels = build_sels(&s, &roster, &mut || 0.0);
        assert_eq!(sels.iter().map(|s| s.widget).collect::<Vec<_>>(), vec![0, 1]);
        assert_eq!(sels[1].character.as_ref().map(|c| c.slug.as_str()), Some("shaggy"));
        assert!(sels[0].character.is_none());
    }

    #[test]
    fn widgets_2v2_the_ally_is_two_and_enemies_step_by_two() {
        let s = setup(
            1,
            vec![
                player("me", 0, false, None),
                player("e1", 1, true, None),
                player("ally", 0, true, None),
                player("e2", 1, true, None),
                player("spec", 4, false, None),
            ],
        );
        let sels = build_sels(&s, &[entry("a", Some("a"))], &mut || 0.5);
        assert_eq!(sels.iter().map(|s| s.widget).collect::<Vec<_>>(), vec![0, 1, 2, 3, -1]);
    }

    #[test]
    fn widgets_ffa_enemies_count_up_from_one() {
        let s = setup(2, vec![player("me", 0, false, None), player("a", 1, true, None), player("b", 2, true, None)]);
        let sels = build_sels(&s, &[entry("a", Some("a"))], &mut || 0.0);
        assert_eq!(sels.iter().map(|s| s.widget).collect::<Vec<_>>(), vec![0, 1, 2]);
    }

    #[test]
    fn a_bot_with_no_ported_fighter_rolls_among_the_ported_ones_and_loses_the_row_skin() {
        let mut p = player("b", 1, true, Some("gone"));
        p.skin = "S02".into();
        let s = setup(0, vec![player("me", 0, false, None), p]);
        let roster = [entry("a", Some("a")), entry("c", Some("c"))];
        let sels = build_sels(&s, &roster, &mut || 0.99);
        assert_eq!(sels[1].character.as_ref().map(|c| c.slug.as_str()), Some("c"));
        assert_eq!(sels[1].skin, "");
        let mut q = player("b", 1, true, Some("a"));
        q.skin = "S02".into();
        let kept = build_sels(&setup(0, vec![player("me", 0, false, None), q]), &roster, &mut || 0.0);
        assert_eq!(kept[1].skin, "S02");
    }

    #[test]
    fn a_world_point_id_reads_like_the_tses_patterns() {
        assert_eq!(point_id("PerkInfo3"), Some((true, 2)));
        assert_eq!(point_id("perkinfo1"), Some((true, 0)));
        assert_eq!(point_id("CheckMark4"), Some((false, 3)));
        assert_eq!(point_id("checkmark4"), None);
        assert_eq!(point_id("CheckMark5"), None);
        assert_eq!(point_id("PerkInfo0"), None);
    }

    #[test]
    fn a_bot_unlocks_the_signature_and_three_general_perks() {
        let slots = unlock_slots(Some("sig".into()), &["a".into(), "b".into(), "c".into(), "d".into()]);
        assert_eq!(slots, vec![Some("sig".into()), Some("a".into()), Some("b".into()), Some("c".into())]);
        assert_eq!(unlock_slots(None, &[]), vec![None, None, None, None]);
    }

    #[test]
    fn each_state_hides_all_but_its_exceptions() {
        assert_eq!(hide_except(1), &["DoubleRewardContainer"]);
        assert_eq!(hide_except(7), &["UI_Perks_LoadoutAllyPreview"]);
        assert!(hide_except(10).is_empty());
        assert_eq!(hide_except(4).len(), 4);
    }

    #[test]
    fn the_ffa_face_off_has_its_own_times() {
        assert_eq!(FACE_OFF_TIMES_FFA[3], ("ShowFFA", 1.4667));
        assert_eq!(FACE_OFF_TIMES[3], ("ShowVS", 1.4667));
    }

    fn names(w: &crate::bundle::WData, out: &mut Vec<String>) {
        out.push(w.n.clone());
        for c in &w.c {
            names(c, out);
        }
        for c in w.ns.values() {
            names(c, out);
        }
    }

    /// Every widget the controller looks up by name is in UI_PreMatch's tree (`find` panics on a missing name).
    /// Needs the dev store; without one it says so and passes.
    #[test]
    fn the_names_the_controller_finds_are_in_the_bundle() {
        let Ok(store) = mvs_store::Store::open(mvs_store::dev_store_dir()) else {
            eprintln!("no dev store: skipped");
            return;
        };
        let Ok(bundle) = crate::Bundle::load(&store, "menu") else {
            eprintln!("no menu bundle: skipped");
            return;
        };
        let Some(class) = bundle.widgets.get("UI_PreMatch") else {
            eprintln!("no UI_PreMatch in the bundle: skipped");
            return;
        };
        let mut all = Vec::new();
        names(&class.root, &mut all);
        let mut wanted: Vec<String> = HIDE_LIST.iter().map(|s| s.to_string()).collect();
        wanted.extend(LOADOUTS.iter().map(|s| s.to_string()));
        wanted.extend(
            [
                "UI_PM_StateInfo",
                "UI_PM_ModeLabel",
                "Btn_BackToCustomLobby",
                "Button_WaitInLab",
                "SizeBox_2",
                "CanvasPanel_",
                "VO_Clickable",
                "inputLabels",
                "inputLabel_MoveList",
                "InputContainer",
                "GN_PerkSpacer",
                "GlobalNavSpacer",
                "UI_QuestTabs",
                "playerInfoSoloSpacer",
                "playerInfoTeamSpacer",
            ]
            .map(String::from),
        );
        wanted.extend((0..4).map(|i| format!("UI_PM_PlayerInfo_{i}")));
        let missing: Vec<&String> = wanted.iter().filter(|n| !all.contains(n)).collect();
        assert!(missing.is_empty(), "not in UI_PreMatch: {missing:?}");
    }

    /// The screen's words are in the player's Game.locres, the queue bots' icons in the catalogue, and the menu
    /// bundle has the gameplay tips, each in Game.locres (needs the dev store: skipped without it).
    #[test]
    fn the_words_icons_and_tips_are_in_the_store() {
        let Ok(store) = mvs_store::Store::open(mvs_store::dev_store_dir()) else { return };
        let (Ok(text), Ok(bundle)) = (crate::text::GameText::load(&store), crate::Bundle::load(&store, "menu")) else {
            return;
        };
        let mut keys = vec![
            TEXT_WAITING_PARTY,
            TEXT_MATCHMAKING,
            TEXT_MATCH_FOUND,
            TEXT_PREPARING_MM,
            TEXT_CANCEL_MM,
            TEXT_CANCELLING_MM,
            TEXT_SELECT_FIGHTER,
            TEXT_WAITING_OTHERS,
            TEXT_WAITING_OTHERS_PERKS,
            TEXT_REVEALING_PERKS,
            TEXT_WAITING,
            TEXT_SELECT_PERKS,
            TEXT_MATCH_STARTING,
            TEXT_DISCONNECTED,
            TEXT_LABEL_SELECT,
            TEXT_LABEL_BACK,
            NAV_EXIT_ARCADE_REMATCH,
            // WBP_MatchOptions SelectedLabel ({mode}:, Normal)
            "CDC94FB34FC59D8BC2BEDDB0B8C255ED",
            "40307D3A4C151424C26AB188EC2891FE",
        ];
        keys.extend(NAV_EXIT_TEXT);
        let missing: Vec<&str> = keys.into_iter().filter(|k| text.get("", k).is_none()).collect();
        assert!(missing.is_empty(), "not in Game.locres: {missing:?}");
        assert_eq!(text.get("", TEXT_SELECT_PERKS), Some("Select Perks"));
        let c = crate::menu::hydra::cosmetics_in(&bundle).expect("the catalogue");
        for id in ["bot1", "bot2", "bot3"] {
            let icon = queue_bot_icon(id).expect("an icon");
            assert!(
                c.profile_icon.iter().any(|e| crate::menu::lobby::loadout::asset_name(&e.path) == icon),
                "{icon} not in the catalogue"
            );
        }
        // every icon a custom game's bot can draw (a slug) finds its picture
        let pool = crate::menu::lobby::loadout::BotPool::of(c);
        assert!(!pool.icons.is_empty());
        let texture = |name: &str| bundle.textures.get(name).map(|t| t.entry().to_string());
        let blank: Vec<&String> =
            pool.icons.iter().filter(|s| banner_icon_texture(c, BannerIconOf::Slug(s), texture).is_none()).collect();
        assert!(blank.is_empty(), "bot icons with no picture: {blank:?}");
        let tips = &bundle.menu.as_ref().expect("the menu data").gameplay_tips;
        assert_eq!(tips.len(), 25);
        assert!(tips.iter().all(|k| text.get("", k).is_some_and(|t| !t.is_empty())));
    }

    #[test]
    fn the_session_is_set_and_cleared() {
        set_session(Some(PreMatchSetup { me: 1, ..PreMatchSetup::default() }));
        assert_eq!(session().map(|s| s.me), Some(1));
        set_session(None);
        assert!(session().is_none());
        let mut prefs = Settings::default();
        assert_eq!(last_chosen(&prefs), None);
        set_last_chosen(&mut prefs, "shaggy".into());
        assert_eq!(last_chosen(&prefs).as_deref(), Some("shaggy"));
    }
}
