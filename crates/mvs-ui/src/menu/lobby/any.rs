//! The two lobbies behind the one interface of the custom game page.

use mvs_protocol::lobby_rules::{LobbyOptions, LobbyPlayer, LobbyRules, OptionKey, OptionValue};

use super::net::NetLobby;
use super::{LobbyEvent, LocalLobby};
use crate::menu::online::{OnlineCmd, OnlineView};

/// The lobby of the open custom game page: the port's local one, or the online one.
#[derive(Debug)]
pub enum Lobby {
    Local(Box<LocalLobby>),
    Net(Box<NetLobby>),
}

impl Lobby {
    pub fn rules(&self) -> &LobbyRules {
        match self {
            Lobby::Local(l) => &l.rules,
            Lobby::Net(l) => l.rules(),
        }
    }

    pub fn net(&self) -> Option<&NetLobby> {
        match self {
            Lobby::Net(l) => Some(l),
            Lobby::Local(_) => None,
        }
    }

    pub fn net_mut(&mut self) -> Option<&mut NetLobby> {
        match self {
            Lobby::Net(l) => Some(l),
            Lobby::Local(_) => None,
        }
    }

    pub fn my_id(&self) -> &str {
        match self {
            Lobby::Local(l) => l.my_id(),
            Lobby::Net(l) => l.my_id(),
        }
    }

    pub fn attach(&mut self) {
        match self {
            Lobby::Local(l) => l.attach(),
            Lobby::Net(l) => l.attach(),
        }
    }

    pub fn detach(&mut self) {
        match self {
            Lobby::Local(l) => l.detach(),
            Lobby::Net(l) => l.detach(),
        }
    }

    /// The echoes due; an online lobby first takes the frame's view.
    pub fn take_events(&mut self, view: &OnlineView, random: &mut dyn FnMut() -> f64) -> Vec<LobbyEvent> {
        match self {
            Lobby::Local(l) => l.take_events(random),
            Lobby::Net(l) => {
                l.feed(view);
                l.take_events()
            }
        }
    }

    /// The commands the lobby queued for the program.
    pub fn take_cmds(&mut self) -> Vec<OnlineCmd> {
        match self {
            Lobby::Local(_) => Vec::new(),
            Lobby::Net(l) => l.take_cmds(),
        }
    }

    pub fn team_style(&self) -> u8 {
        self.rules().team_style
    }
    pub fn num_ringouts_for_win(&self) -> u32 {
        self.rules().num_ringouts_for_win
    }
    pub fn enabled_maps(&self) -> &[String] {
        &self.rules().enabled_maps
    }
    pub fn leader_id(&self) -> &str {
        &self.rules().leader_id
    }
    pub fn teams(&self) -> &[Vec<LobbyPlayer>; 2] {
        &self.rules().teams
    }
    pub fn players(&self) -> Vec<&LobbyPlayer> {
        self.rules().players()
    }
    pub fn options(&self) -> &LobbyOptions {
        &self.rules().options
    }
    pub fn online(&self) -> bool {
        matches!(self, Lobby::Net(_))
    }
    pub fn am_i_leader(&self) -> bool {
        self.rules().leader_id == self.my_id()
    }
    pub fn is_ready(&self, account_id: &str) -> bool {
        self.rules().ready.iter().any(|r| r == account_id)
    }
    pub fn get_available_player_slot_count(&self) -> usize {
        self.rules().get_available_player_slot_count()
    }
    pub fn get_start_match_blockers(&self) -> Vec<&'static str> {
        self.rules().get_start_match_blockers()
    }
    pub fn can_start_match(&self) -> bool {
        match self {
            Lobby::Local(l) => l.can_start_match(),
            Lobby::Net(l) => l.can_start_match(),
        }
    }
    pub fn has_ready_button(&self) -> bool {
        match self {
            Lobby::Local(l) => l.has_ready_button(),
            Lobby::Net(l) => l.has_ready_button(),
        }
    }
    pub fn set_ready(&mut self, on: bool) {
        match self {
            Lobby::Local(l) => l.set_ready(on),
            Lobby::Net(l) => l.set_ready(on),
        }
    }
    pub fn start(&mut self) {
        match self {
            Lobby::Local(l) => l.start(),
            Lobby::Net(l) => l.start(),
        }
    }
    pub fn set_team_style(&mut self, style: u8) {
        match self {
            Lobby::Local(l) => l.set_team_style(style),
            Lobby::Net(l) => l.set_team_style(style),
        }
    }
    pub fn set_match_duration(&mut self, sec: u32) {
        match self {
            Lobby::Local(l) => l.set_match_duration(sec),
            Lobby::Net(l) => l.set_match_duration(sec),
        }
    }
    pub fn set_num_ringouts_for_win(&mut self, n: u32) {
        match self {
            Lobby::Local(l) => l.set_num_ringouts_for_win(n),
            Lobby::Net(l) => l.set_num_ringouts_for_win(n),
        }
    }
    pub fn set_allow_hazards(&mut self, b: bool) {
        match self {
            Lobby::Local(l) => l.set_allow_hazards(b),
            Lobby::Net(l) => l.set_allow_hazards(b),
        }
    }
    pub fn set_maps(&mut self, maps: &[String]) {
        match self {
            Lobby::Local(l) => l.set_maps(maps),
            Lobby::Net(l) => l.set_maps(maps),
        }
    }
    pub fn set_option(&mut self, key: OptionKey, value: OptionValue) {
        match self {
            Lobby::Local(l) => l.set_option(key, value),
            Lobby::Net(l) => l.set_option(key, value),
        }
    }
    pub fn add_bot(&mut self, team: usize, character: &str, skin: &str, difficulty: i32) {
        match self {
            Lobby::Local(l) => l.add_bot(team, character, skin, difficulty),
            Lobby::Net(l) => l.add_bot(team, character, skin, difficulty),
        }
    }
    pub fn kick_player(&mut self, id: &str) {
        match self {
            Lobby::Local(l) => l.kick_player(id),
            Lobby::Net(l) => l.kick_player(id),
        }
    }
    pub fn join_team(&mut self, id: &str, team: usize) {
        match self {
            Lobby::Local(l) => l.join_team(id, team),
            Lobby::Net(l) => l.join_team(id, team),
        }
    }
    pub fn update_settings_for_bot(&mut self, id: &str, character: &str, difficulty: i32, skin: &str) {
        match self {
            Lobby::Local(l) => l.update_settings_for_bot(id, character, difficulty, skin),
            Lobby::Net(l) => l.update_settings_for_bot(id, character, difficulty, skin),
        }
    }
    pub fn promote_new_leader(&mut self, id: &str) {
        match self {
            Lobby::Local(l) => l.promote_new_leader(id),
            Lobby::Net(l) => l.promote_new_leader(id),
        }
    }
    pub fn invite(&mut self, id: &str) {
        match self {
            Lobby::Local(l) => l.invite(id),
            Lobby::Net(l) => l.invite(id),
        }
    }
    pub fn start_match(&mut self) {
        match self {
            Lobby::Local(l) => l.start_match(),
            Lobby::Net(l) => l.start_match(),
        }
    }
    pub fn leave(&mut self) {
        match self {
            Lobby::Local(l) => l.leave(),
            Lobby::Net(l) => l.leave(),
        }
    }
}
