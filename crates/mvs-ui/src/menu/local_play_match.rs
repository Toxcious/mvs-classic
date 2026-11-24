//! The Local Play match start and the way back: PandaGameInstance
//! BPI_LocalPlayBegin (@35771), its homescreen queue entry 14 and BPI_SetIsLocalPlay, and
//! LocalPlayFacilitator.BeginMatch, which makes the match config and the local players from the Local Play
//! preferences (mvs-settings `LocalPlaySettings`) and opens one map of the selection.

use std::cell::RefCell;

use mvs_settings::Settings;
use mvs_sim::MatchSetup;
use mvs_sim::setup::{MatchPlayer, Rules, TeamStyle};

use super::lobby::loadout::{self, ring_out_asset_of};
use super::lobby::{LobbyMatch, LocalHuman};
use super::view::MenuView;

/// The menus' local players and pad events (the homescreen queue, the Local Play flag and the started
/// match are the session's).
/// Unverified: kept in a per-thread slot (the menus run on the program's main thread) until the menu host gives it
/// a field of its own.
#[derive(Debug)]
pub struct LocalPlayFlow {
    /// The menus' local players: index = local player, value = the device it holds.
    players: Vec<Option<Device>>,
    /// The device of the last menu input.
    last_menu: InputSource,
    /// Whether a live Local Play screen listens for pad plug / unplug (UI_LocalPlay's
    /// handler is its one listener).
    pad_listener: bool,
    /// The pad events the listener has not heard yet: (pad, connected, the local player that held the pad before the
    /// table changed).
    pad_events: Vec<PadEvent>,
}

/// A pad plugged in or out, as the Local Play screen hears it: `player` is the local
/// player that held the pad when it changed (GetPlayerIdForController before the id moves), None none.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PadEvent {
    pub pad: usize,
    pub connected: bool,
    pub player: Option<usize>,
}

/// An input device: 'kbm+pad0' is the build's device 0 (the Lab, a queue, a lobby, online);
/// Deviation: in Local Play the keyboard and mouse are a device of their own (`Kbm`), apart from pad 0.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Device {
    Kbm,
    KbmPad0,
    Pad(usize),
}

/// What sent a menu input: the keyboard or mouse, or a pad by its device index.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InputSource {
    Kbm,
    Pad(usize),
}

impl Default for LocalPlayFlow {
    fn default() -> LocalPlayFlow {
        LocalPlayFlow {
            players: vec![Some(Device::KbmPad0)],
            last_menu: InputSource::Kbm,
            pad_listener: false,
            pad_events: Vec::new(),
        }
    }
}

thread_local! {
    static FLOW: RefCell<LocalPlayFlow> = RefCell::new(LocalPlayFlow::default());
}

fn flow<R>(f: impl FnOnce(&mut LocalPlayFlow) -> R) -> R {
    FLOW.with(|c| f(&mut c.borrow_mut()))
}

// ---------- the menus' local players ----------

/// The menu's local players' devices (None: no device, -1). Outside Local Play only player 0 exists, with the
/// build's device 0. Begin Match keeps them, so slot i's human reads entry i.
pub fn local_player_devices() -> Vec<Option<Device>> {
    flow(|f| f.players.clone())
}

pub fn set_local_player_devices(d: Vec<Option<Device>>) {
    flow(|f| f.players = d);
}

/// The device of the last menu input: the app sets it with each input.
pub fn set_last_menu_device(s: InputSource) {
    flow(|f| f.last_menu = s);
}

/// `first`: the device index of the first standard-mapping pad ('kbm+pad0''s pad), -1 none.
fn holds(d: Device, s: InputSource, first: i64) -> bool {
    match (d, s) {
        (Device::Kbm | Device::KbmPad0, InputSource::Kbm) => true,
        (Device::KbmPad0, InputSource::Pad(p)) => i64::try_from(p).ok() == Some(first),
        (Device::Pad(q), InputSource::Pad(p)) => q == p,
        _ => false,
    }
}

/// UI_Manager UserIndexesForKeyEvent -> InputIndexToLocalUserIndex (@4423): the local player whose device sent the
/// input, the last match winning; None: no player (ProcessInput then does nothing).
pub fn local_player_of(s: InputSource, first: i64) -> Option<usize> {
    flow(|f| {
        f.players.iter().enumerate().filter(|(_, d)| d.is_some_and(|d| holds(d, s, first))).map(|(i, _)| i).next_back()
    })
}

/// FighterGameInstance GetPlayerIdForController (1410c4fe0): the first local player holding the device.
pub fn player_id_for_device(s: InputSource, first: i64) -> Option<usize> {
    flow(|f| f.players.iter().position(|d| d.is_some_and(|d| holds(d, s, first))))
}

/// UI_Manager ToggleMultiInput (@2539). On: every local player 0..3 made, each with no controller id given the first
/// free one. Off: players 3, 2, 1 removed; player 0 keeps its own.
/// Deviation: the keyboard and mouse are a device of their own. On, a player 0 still on the build's device 0 takes
/// the device of the last menu input, and a player with none the first free device of Kbm, pad 0, pad 1, pad 2, pad 3
/// (free while no player holds it, plugged in or not; the fifth device gets no player). Off, player 0 goes back to
/// 'kbm+pad0', the menus' one player outside Local Play.
pub fn toggle_multi_input(on: bool) {
    if !on {
        set_local_player_devices(vec![Some(Device::KbmPad0)]);
        return;
    }
    flow(|f| {
        let mut t: Vec<Option<Device>> = (0..4).map(|i| f.players.get(i).copied().flatten()).collect();
        if matches!(t[0], None | Some(Device::KbmPad0)) {
            t[0] = Some(match f.last_menu {
                InputSource::Kbm => Device::Kbm,
                InputSource::Pad(p) => Device::Pad(p),
            });
        }
        let mut free: Vec<Device> = [Device::Kbm, Device::Pad(0), Device::Pad(1), Device::Pad(2), Device::Pad(3)]
            .into_iter()
            .filter(|d| !t.contains(&Some(*d)))
            .collect();
        free.reverse();
        for slot in &mut t {
            if slot.is_none() {
                *slot = free.pop();
            }
        }
        f.players = t;
    });
}

/// FighterGameInstance HandleControllerConnectionChange (1410c6e40): OnControllerConnectionChange is broadcast first
/// (a live UI_LocalPlay hears it before any id moves: the event is queued for it with the player that held the pad,
/// `take_pad_events`); then a disconnect takes the id from the local player holding it unless that is player 0, and
/// a connect gives it to the first local player with none (the table changes at once).
/// Deviation: a connect of a pad some player holds is no connection change: no event and the table as it is.
/// Unverified: a listener is taken to run inside the call; here the Local Play screen hears the event at its next tick,
/// after the table moved (the player it reads is the one from before the move, as the build's handler sees it).
pub fn pad_connection_changed(pad: usize, connected: bool, first: i64) {
    let s = InputSource::Pad(pad);
    if connected && player_id_for_device(s, first).is_some() {
        return;
    }
    let player = player_id_for_device(s, -1);
    flow(|f| {
        if f.pad_listener {
            f.pad_events.push(PadEvent { pad, connected, player });
        }
    });
    apply_pad_connection(pad, connected);
}

/// The Local Play screen starts or stops listening for pad changes; stopping drops what it has not heard.
pub fn listen_pads(on: bool) {
    flow(|f| {
        f.pad_listener = on;
        if !on {
            f.pad_events.clear();
        }
    });
}

/// The pad events queued and not yet heard by the Local Play screen.
pub fn take_pad_events() -> Vec<PadEvent> {
    flow(|f| std::mem::take(&mut f.pad_events))
}

/// The table change that follows the broadcast (see `pad_connection_changed`).
fn apply_pad_connection(pad: usize, connected: bool) {
    let s = InputSource::Pad(pad);
    let at = player_id_for_device(s, -1);
    flow(|f| {
        if !connected {
            if let Some(i) = at.filter(|i| *i > 0) {
                f.players[i] = None;
            }
        } else if let Some(i) = f.players.iter().position(Option::is_none) {
            f.players[i] = Some(Device::Pad(pad));
        }
    });
}

/// D8 TranslateTeamStyleToGameModeAlias (exec 14087d9a0): Free For All "FreeForAll", every other style "Versus".
pub fn team_style_alias(style: u8) -> &'static str {
    if style == 2 { "FreeForAll" } else { "Versus" }
}

// ---------- LocalPlayFacilitator.BeginMatch (@1718 -> @15) ----------

/// What BeginMatch makes: the match, its MapData, the gameplay level and the game mode alias.
#[derive(Clone, Debug)]
pub struct LocalPlayStart {
    pub lobby: LobbyMatch,
    pub map: String,
    /// GetLevelAssetPath's ExtensionPart, '' when the menu data lacks it.
    pub level: String,
    pub alias: &'static str,
}

/// LocalPlayFacilitator.BeginMatch (@1718 -> @15): None when the preferences give no team style or no map.
/// `devices`: the device each menu local player holds (the Local Play
/// screen makes four).
pub fn begin_local_play_match(
    view: &MenuView,
    settings: &Settings,
    devices: &[Option<Device>],
    random: &mut dyn FnMut() -> f64,
) -> Option<LocalPlayStart> {
    let prefs = &settings.local_play;
    let style = prefs.team_style;
    if style > 2 {
        return None;
    }
    // SetActiveMatchConfigFromLocalPlaySettings: CreateMatchConfigObject(style, 0, 4, 0, alias) (D9: QueueType 0
    // Unselected, Context 4 Local, ModeDifficulty 0), SetNumRingoutsForWin (0 in FFA, @440), SetMatchDuration(Time)
    // @579, SetAllowHazards @674. (the port's) QueueType and Context have no field in MatchSetup: nothing in the
    // port reads them; MatchSetup.local_play is the match flow's flag.
    let alias = team_style_alias(style);
    // BPI_CreateLocalPlayers (PGI @28113): local players 0..{1,3,3,1}[style], "Player n" each (CreatePlayerData
    // @18330; N4: the player index is the local player's)
    // Deviation: the port's own big matches: Teams and Free For All take every seat of the preferences, up to eight;
    // the seats past the pads' four are bots (the build's Local Play has four seats and no bot).
    let pads = mvs_settings::LOCAL_PLAY_PADS;
    let seats = prefs.players.len().clamp(pads, mvs_settings::LOCAL_PLAY_MAX);
    let count = [2, seats, seats, 2][usize::from(style)];
    let no_seat = mvs_settings::LocalPlayPlayer::default();
    let mut players = Vec::new();
    let mut usernames = Vec::new();
    let mut humans = Vec::new();
    let perk_ids = &view.bundle.menu.as_ref()?.perks;
    for i in 0..count {
        let s = prefs.players.get(i).unwrap_or(&no_seat);
        // SetPlayerSelections (@1286): AssignPlayerAndTeamIndex(index, teamIndex) @55, AssignFighter of the slug's
        // CharacterData @218, AssignSkin @641 (the skin skinReference loads @524: a path no skin of the fighter has
        // loads none, the default), the perks @1561-1681 (a slug the data lacks is a null entry, which the match
        // drops: perk_list). The ring-out VFX @285-479 from the slot's ringoutSlug and the taunts (no match-side
        // class reads the LocalPlay taunt slugs: every fighter of a character takes this machine's one Hydra set)
        // are put in below.
        let entry = view.roster().iter().find(|r| r.slug == s.character_slug);
        let fighter = entry.and_then(|e| e.fighter.as_deref());
        if fighter.is_none() {
            eprintln!(
                "menu: Local Play player {i}: character {} is not one the port has, the default one plays",
                if s.character_slug.is_empty() { "(none)" } else { &s.character_slug }
            );
        }
        let perks: Vec<String> = s
            .perk_slugs
            .iter()
            .map(|slug| {
                if slug.is_empty() {
                    String::new()
                } else {
                    perk_ids.iter().find(|(_, p)| p.slug == *slug).map(|(k, _)| k.clone()).unwrap_or_default()
                }
            })
            .collect();
        // (a path compared without case, as the soft reference's FName; the port's default skin carries '')
        let fi = fighter.and_then(|f| view.fighters().get(f));
        let skin = if s.skin_reference.is_empty() {
            String::new()
        } else {
            let name = fi
                .and_then(|f| f.skins.iter().find(|k| k.path.eq_ignore_ascii_case(&s.skin_reference)))
                .map_or("", |k| k.name.as_str());
            loadout::picked_skin(fi, name)
        };
        // (a bot's seat: its lobby difficulty as the match's level; every seat's team below the style's team count,
        // a bot's its own in Free For All: `LocalPlaySettings::seat_team`)
        let bot = i >= pads;
        let team = prefs.seat_team(i);
        players.push(MatchPlayer {
            bot,
            difficulty: bot.then(|| mvs_sim::math::narrow(f64::from(s.bot_difficulty.clamp(2, 6) - 2))),
            team: Some(team),
            character: fighter.map(str::to_string),
            skin: (!skin.is_empty()).then_some(skin),
            perks: mvs_sim::setup::perk_list(&perks),
            ..MatchPlayer::default()
        });
        usernames.push(if bot {
            mvs_protocol::lobby_rules::bot_name(fighter.unwrap_or(""))
        } else {
            format!("Player {}", i + 1)
        });
        // slot i is local player i, whose device is the menu's local player i's
        if let Some(device) = devices.get(i).copied().flatten().filter(|_| !bot) {
            humans.push(LocalHuman { slot: i, device, set: i });
        }
    }
    // Maps: GetMapsFromSlugs(prefs Maps), Array_Random, LoadAsset -> OnLoad (@1723)
    // (the port's own big matches: of the selection, the maps whose stage holds one; none starts no match)
    let team_style = match style {
        0 => TeamStyle::OneVsOne,
        1 => TeamStyle::TwoVsTwo,
        _ => TeamStyle::FreeForAll,
    };
    let big = count > pads;
    let maps = &view.bundle.menu.as_ref()?.maps;
    let names: Vec<&String> = prefs
        .maps
        .iter()
        .filter_map(|slug| maps.iter().find(|(_, m)| m.slug == *slug).map(|(n, _)| n))
        .filter(|n| !big || view.bundle.map_holds_big(n, team_style))
        .collect();
    if names.is_empty() {
        return None;
    }
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let pick = ((random() * names.len() as f64).floor() as usize).min(names.len() - 1);
    let map = names[pick].clone();
    let level = maps.get(&map).map(|m| m.level.clone()).unwrap_or_default(); // MapData.Level (GetLevelAssetPath)
    let mut setup = MatchSetup {
        team_style,
        players,
        rules: Some(Rules {
            knockouts_to_win: if style == 2 { 0.0 } else { mvs_sim::math::narrow(f64::from(prefs.ringouts)) },
            duration: mvs_sim::math::narrow(f64::from(prefs.time)),
            stocks: false,
        }),
        flow: true,
        allow_hazards: prefs.hazards,
        local_play: true,
        ..MatchSetup::default()
    };
    let drawable = |c: &str| view.fighters().contains_key(c);
    loadout::ported_setup(&mut setup, &drawable);
    loadout::with_local_loadout(
        &mut setup,
        &settings.hydra,
        view.roster(),
        &mut |_, i| ring_out_asset_of(&settings.hydra, &prefs.players.get(i).unwrap_or(&no_seat).ringout_slug),
        &mut |_, i| prefs.players.get(i).unwrap_or(&no_seat).banner_slug.clone(),
        &mut |_, i| Some(crate::menu::prefs::sim_input_prefs(&settings.input, i)),
    );
    let lobby = LobbyMatch { setup, me: 0, usernames, map: Some(map.clone()), humans: Some(humans), lobby: None };
    // OnLoad @1877: BPI_SetIsLocalPlay(true); BPI_SetMatchState("InMatch") @1994 (the port's match state is the
    // program's)
    Some(LocalPlayStart { lobby, map, level, alias })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bundle::{Bundle, FighterInfo, MapData, MenuData, RosterEntry};
    use crate::text::GameText;
    use std::sync::Arc;

    /// A view of one fighter and two maps: `map_big` on a stage that holds a big Free For All, `map_small` not.
    fn eight_seat_view() -> MenuView {
        let map = |slug: &str, level: &str| MapData {
            slug: slug.to_owned(),
            level: level.to_owned(),
            levels: vec![level.to_owned()],
            ..MapData::default()
        };
        let menu = MenuData {
            roster: vec![RosterEntry {
                slug: "character_shaggy".to_owned(),
                fighter: Some("shaggy".to_owned()),
                ..RosterEntry::default()
            }],
            fighters: [(
                "shaggy".to_owned(),
                FighterInfo { slug: "character_shaggy".to_owned(), ..FighterInfo::default() },
            )]
            .into_iter()
            .collect(),
            maps: [("map_big".to_owned(), map("s_big", "L_big")), ("map_small".to_owned(), map("s_small", "L_small"))]
                .into_iter()
                .collect(),
            ..MenuData::default()
        };
        let bundle = Bundle {
            menu: Some(menu),
            stages: vec![("L_big".to_owned(), "big".to_owned()), ("L_small".to_owned(), "small".to_owned())],
            big_stages: vec![("big".to_owned(), TeamStyle::FreeForAll), ("big".to_owned(), TeamStyle::TwoVsTwo)],
            ..Bundle::default()
        };
        MenuView::new(Arc::new(bundle), Arc::new(GameText::default()), [1920, 1080])
    }

    #[test]
    fn an_eight_seat_free_for_all_fills_the_seats_past_the_pads_with_bots() {
        let view = eight_seat_view();
        let mut settings = Settings::default();
        let lp = &mut settings.local_play;
        lp.team_style = 2;
        lp.maps = vec!["s_small".to_owned(), "s_big".to_owned()];
        lp.players = (0..8usize)
            .map(|i| mvs_settings::LocalPlayPlayer {
                character_slug: "character_shaggy".to_owned(),
                team_index: if i < 4 { u8::try_from(i).expect("a seat") } else { 0 },
                ..mvs_settings::LocalPlayPlayer::seat(i)
            })
            .collect();
        let devices: Vec<Option<Device>> = (0..4).map(|p| Some(Device::Pad(p))).collect();
        for r in [0.0, 0.99] {
            let start = begin_local_play_match(&view, &settings, &devices, &mut || r).expect("a start");
            let setup = &start.lobby.setup;
            assert_eq!(setup.team_style, TeamStyle::FreeForAll);
            assert_eq!(setup.players.len(), 8);
            let bots: Vec<bool> = setup.players.iter().map(|p| p.bot).collect();
            assert_eq!(bots, [false, false, false, false, true, true, true, true]);
            let teams: Vec<Option<u8>> = setup.players.iter().map(|p| p.team).collect();
            assert_eq!(teams, (0..8).map(Some).collect::<Vec<_>>(), "a team per seat");
            // (Medium, the default seat's difficulty: BotBehavior 4, the lobby's 2)
            assert!(setup.players[4..].iter().all(|p| p.difficulty == Some(2.0)));
            assert_eq!(start.lobby.usernames.len(), 8);
            assert_eq!(start.lobby.humans.as_ref().map(Vec::len), Some(4), "the pads hold the first four");
            assert_eq!(start.map, "map_big", "only the map whose stage holds eight");
        }
        // Teams: new seats past the pads alternate, so the sides come out even; a saved team past the two is the
        // alternating default
        let mut teams = settings.clone();
        teams.local_play.team_style = 1;
        teams.local_play.players = (0..8)
            .map(|i| mvs_settings::LocalPlayPlayer {
                character_slug: "character_shaggy".to_owned(),
                ..mvs_settings::LocalPlayPlayer::seat(i)
            })
            .collect();
        teams.local_play.players[2].team_index = 3;
        let start = begin_local_play_match(&view, &teams, &devices, &mut || 0.0).expect("a start");
        let got: Vec<Option<u8>> = start.lobby.setup.players.iter().map(|p| p.team).collect();
        assert_eq!(got, [0, 1, 0, 1, 0, 1, 0, 1].map(Some));
        assert_eq!(start.lobby.setup.team_style, TeamStyle::TwoVsTwo);
        // no selected map holds eight: no match
        settings.local_play.maps = vec!["s_small".to_owned()];
        assert!(begin_local_play_match(&view, &settings, &devices, &mut || 0.0).is_none());
        // four seats play on it
        settings.local_play.players.truncate(4);
        assert!(begin_local_play_match(&view, &settings, &devices, &mut || 0.0).is_some());
    }

    #[test]
    fn free_for_all_has_its_own_alias() {
        assert_eq!(team_style_alias(2), "FreeForAll");
        assert_eq!(team_style_alias(0), "Versus");
    }

    #[test]
    fn multi_input_gives_each_player_a_free_device() {
        set_local_player_devices(vec![Some(Device::KbmPad0)]);
        set_last_menu_device(InputSource::Kbm);
        toggle_multi_input(true);
        assert_eq!(
            local_player_devices(),
            vec![Some(Device::Kbm), Some(Device::Pad(0)), Some(Device::Pad(1)), Some(Device::Pad(2))]
        );
        assert_eq!(local_player_of(InputSource::Pad(1), 0), Some(2));
        assert_eq!(player_id_for_device(InputSource::Kbm, 0), Some(0));
        toggle_multi_input(false);
        assert_eq!(local_player_devices(), vec![Some(Device::KbmPad0)]);
    }

    #[test]
    fn pad_unplug_frees_a_player_but_not_player_zero() {
        set_local_player_devices(vec![Some(Device::Pad(0)), Some(Device::Pad(1)), None, None]);
        // no Local Play screen: the table changes, nothing is queued
        pad_connection_changed(1, false, 0);
        assert!(take_pad_events().is_empty());
        assert_eq!(local_player_devices()[1], None);
        set_local_player_devices(vec![Some(Device::Pad(0)), Some(Device::Pad(1)), None, None]);
        listen_pads(true);
        pad_connection_changed(1, false, 0);
        assert_eq!(take_pad_events(), vec![PadEvent { pad: 1, connected: false, player: Some(1) }]);
        assert_eq!(local_player_devices()[1], None);
        pad_connection_changed(0, false, 0);
        assert_eq!(local_player_devices()[0], Some(Device::Pad(0)));
        pad_connection_changed(2, true, 0);
        assert_eq!(local_player_devices()[1], Some(Device::Pad(2)));
        // a connect of a held pad is no change
        take_pad_events();
        pad_connection_changed(0, true, 0);
        assert!(take_pad_events().is_empty());
        listen_pads(false);
        set_local_player_devices(vec![Some(Device::KbmPad0)]);
    }
}
