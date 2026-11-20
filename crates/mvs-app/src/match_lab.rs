//! The Lab inside a match: the session its menu sees, the pause with the config changes an
//! unpause applies, the spawn delay and the restart.
//!
//! The Lab's menu is a screen of the match (`pause::MatchMenu`); this module is the state under it, free of the
//! window and the renderer: `LabState` is what `Cx::lab` gives the widgets, `LabControl` the pause and spawn logic the
//! match loop steps and answers with `LabEffect`s.
//!
//! The queue's Wait in the Lab (`app.queue`: the match-found chain, Back To Matchmaking, the matchmaking timer) is
//! the host's (lab_host.rs) on the hub's queue; here its flags the menu reads and sets.
//!
//! Not ported: the Lab's bots' ?debug keys (`labBots`; a bot's new behaviour from the config is run.rs's
//! `update_lab_bots`), the ahead job that warms the next fighters' effects (the restart's load does it), the views'
//! meshes made during the delay.

use mvs_protocol::lobby_rules::MutatorSet;
use mvs_sim::World;
use mvs_sim::fighter::common::latent_frames;
use mvs_ui::menu::MenuScreen;
use mvs_ui::menu::training_config::{
    LabConfig, LabPlayerConfig, LabSession, LabToggles, empty_lab_player, lab_players_equal,
};
use mvs_ui::slate::WidgetId;

use crate::lab_setup::{LabInputs, LabRoll, lab_roll, lab_roster_skins, lab_slots};

/// The session the Lab's menu sees: TrainGameMode's AuthorityTrainingConfig and the broadcast
/// of it back to the menu.
#[derive(Debug)]
pub struct LabState {
    pub config: LabConfig,
    pub toggles: LabToggles,
    me: String,
    /// TrainingGameState.BroadcastAuthorityConfig: the config the menu has not taken yet.
    broadcast: Option<LabConfig>,
    /// PS.bRespondingToConfigUpdate: set around the menu's handling of a broadcast.
    responding: bool,
    /// BPI_Pause(on) the menu asked for, taken by the match loop (the menu's call cannot reach the match's pause while
    /// its widget is running).
    pause_request: Option<bool>,
    /// The config changed since the loop last read it: TrainGameMode Handle Training Config Updated sets the world's
    /// same-attack penalty and instant cooldowns from it.
    config_changed: bool,
    /// bIsMatchmaking: the queue the Lab waits beside is searching (app.queue.searching), set by the host each frame.
    pub matchmaking: bool,
    /// The quit dialog's OK while matchmaking (TrainGameMode Back To Matchmaking), taken by the host.
    back_request: bool,
}

impl LabState {
    /// The Lab's session on `config` (the toggles of a kept session carried).
    pub fn new(config: LabConfig, toggles: Option<LabToggles>) -> LabState {
        LabState {
            config,
            toggles: toggles.unwrap_or(LabToggles { hitstun_combo: false, juggle_combo: false, hazards: true }),
            me: "local".to_owned(),
            broadcast: None,
            responding: false,
            pause_request: None,
            config_changed: true,
            matchmaking: false,
            back_request: false,
        }
    }

    /// Back To Matchmaking asked since the last call.
    pub fn take_back_request(&mut self) -> bool {
        std::mem::take(&mut self.back_request)
    }

    /// The pause the menu asked for since the last call.
    pub fn take_pause_request(&mut self) -> Option<bool> {
        self.pause_request.take()
    }

    /// Whether the config changed since the last call (the loop copies it into the World).
    pub fn take_config_changed(&mut self) -> bool {
        std::mem::take(&mut self.config_changed)
    }

    /// The menu's handling of the last broadcast is over (the frame's end).
    pub fn end_response(&mut self) {
        self.responding = false;
    }

    /// The broadcast the menu has not taken (the loop gives the pause menu's panels a first one when it opens).
    pub fn broadcast(&mut self) {
        self.broadcast = Some(self.config.clone());
    }
}

impl LabSession for LabState {
    fn config(&self) -> &LabConfig {
        &self.config
    }

    fn me(&self) -> &str {
        &self.me
    }

    fn toggles(&mut self) -> &mut LabToggles {
        &mut self.toggles
    }

    /// TrainGameMode Handle Training Config Updated, then TrainingGameState.BroadcastAuthorityConfig.
    fn request_config_update(&mut self, config: LabConfig) {
        if self.responding {
            return;
        }
        self.config = config;
        self.config_changed = true;
        self.broadcast();
    }

    fn request_authority_config(&mut self) {
        self.broadcast();
    }

    fn request_kick_player(&mut self, account_id: &str) {
        let Some(i) = self.config.players.iter().position(|p| !p.account_id.is_empty() && p.account_id == account_id)
        else {
            return;
        };
        self.config.players[i] = empty_lab_player();
        self.broadcast();
    }

    fn pause(&mut self, on: bool) {
        self.pause_request = Some(on);
    }

    fn take_config(&mut self) -> Option<LabConfig> {
        let c = self.broadcast.take()?;
        self.responding = true;
        Some(c)
    }

    fn is_matchmaking(&self) -> bool {
        self.matchmaking
    }

    fn back_to_matchmaking(&mut self) {
        self.back_request = true;
    }
}

/// What the Lab's menu and pause keep across a restart: the session, the screen with its
/// pause menu widget (the pointer is the window's: UiShared).
#[derive(Debug)]
pub struct LabMenu {
    pub session: LabState,
    pub screen: MenuScreen,
    /// UI_LAB_PauseMenu.
    pub pause_menu: WidgetId,
    /// The Lab Tools' panel and what they hold (lab_tools.rs); None without the UI.
    pub tools: Option<crate::lab_tools::LabTools>,
    /// UI_Lab_MatchmakingTimer over the collapsed menu while the queue searches (kept with the menu over a restart).
    pub queue_timer: Option<mvs_ui::menu::screens::lab_timer::LabTimerHandle>,
    /// The menu's clock at its last frame, s: the next match's goes on from it (a wall clock, the
    /// browser's, which a restart does not set back), so the times its managers keep (the last modal's dismissal, the
    /// navigation timers, the Lab Tools' refresh) stay behind it.
    pub clock: f64,
}

/// What the loop does for a step of the Lab's control.
#[derive(Clone, Debug, PartialEq)]
pub enum LabEffect {
    /// The pause went on or off (UI_Game MatchSetPause): the menu shown or hidden, its sound mix, the match's tick
    /// stopped or run.
    Paused(bool),
    /// TrainGameMode Destroy All Players: AbductPlayer on every fighter (effect and sound).
    AbductAll,
    /// An unpause onto another map (TrainingContainer OnMapLoadRequested): the match ends and the next loads that map
    /// ("Loading New Level"), on the roll.
    MapLoad(LabRoll),
    /// A spawn that keeps the roster: every fighter takes its slot's starting damage and the World resets (labReset).
    Reset { starting_damage: Vec<f64> },
    /// A spawn that changes the roster: the match ends and the restart's set-up takes the config, on the roll.
    Restart(LabRoll),
    /// BPI_EnterPreMatch's end (OpenLevel PreMatch): the Lab's level left for the queue's pre-match screen.
    Leave,
}

/// The pause and the spawn delay of the Lab.
#[derive(Debug)]
pub struct LabControl {
    pub paused: bool,
    /// UI_Game.MatchSetPause (@14953): Can Pause is false in the frame of an unpause; the loop clears it each frame.
    pub unpaused_this_frame: bool,
    /// TrainingGameState Respond To Pause Status: ConfigAtLastPause (@2788).
    config_at_pause: Vec<LabPlayerConfig>,
    map_at_pause: String,
    /// (the port's own) The mutators at the pause: another set at the unpause respawns into a restart.
    mutators_at_pause: MutatorSet,
    /// Ticks of HandleAllPlayerDataAcked's Delay(1.0); 0 = no spawn pending.
    spawn_left: u32,
    /// What the match runs with (rosterOf at its start).
    roster: String,
    rolled: LabRoll,
    /// (the port's own) The roll of a map picked in the paused menu, made with the pick for the load ahead of the
    /// unpause, and the config it was made for: the unpause's map load takes it while the config is the same.
    map_roll: Option<(LabConfig, LabRoll)>,
}

impl LabControl {
    /// The control of a match whose roster is `roster` (`lab_roster_skins`) and whose roll is `rolled`.
    pub fn new(roster: String, rolled: LabRoll) -> LabControl {
        LabControl {
            paused: false,
            unpaused_this_frame: false,
            config_at_pause: Vec::new(),
            mutators_at_pause: MutatorSet::default(),
            map_at_pause: String::new(),
            spawn_left: 0,
            roster,
            rolled,
            map_roll: None,
        }
    }

    /// The paused menu holds another map than the one at the pause: the config and roll its unpause will load
    /// (what a load ahead of the map load is made for). The roll is made once per config.
    // Deviation: the build streams the map's levels from the unpause (OnMapLoadRequested); the port begins its load
    // off the frame thread at the pick, which shows nothing until the unpause.
    pub fn map_ahead(&mut self, st: &LabState, i: &mut LabInputs<'_>) -> Option<(LabConfig, LabRoll)> {
        if !self.paused || st.config.selected_map == self.map_at_pause {
            self.map_roll = None;
            return None;
        }
        if !self.map_roll.as_ref().is_some_and(|(c, _)| *c == st.config) {
            self.map_roll = Some((st.config.clone(), lab_roll(i, &st.config)));
        }
        self.map_roll.clone()
    }

    /// The roll the running match was set up with.
    pub fn rolled(&self) -> &LabRoll {
        &self.rolled
    }

    /// The roster the running match was set up with (`lab_roster_skins`).
    pub fn roster(&self) -> &str {
        &self.roster
    }

    /// Spawn All Players is pending (the delay runs).
    pub fn spawning(&self) -> bool {
        self.spawn_left > 0
    }

    /// The config changed: Spawn All Players after the Delay (a Delay that is already running is not started again).
    fn respawn(&mut self, st: &LabState, i: &mut LabInputs<'_>, out: &mut Vec<LabEffect>) {
        if self.spawn_left > 0 {
            return;
        }
        out.push(LabEffect::AbductAll);
        // (latentFrames counts whole ticks: its value is an integer)
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let n = latent_frames(1.0) as u32;
        self.spawn_left = n;
        self.rolled = lab_roll(i, &st.config);
    }

    /// UI_Game.MatchSetPause(on) as the Lab runs it, after the game's checks (Can Pause).
    // Unverified: the pause is taken as a full stop of the world's ticks (ServerPauseGame is native).
    // The unpause: TrainingGameState.UnpauseConfigChangeCheck compares every slot with the config at the pause; one
    // changed slot -> Spawn All Players: AbductPlayer on every fighter (effect and sound), then
    // HandleAllPlayerDataAcked's Delay(1.0), then every slot is spawned as at the Lab's start, with its starting
    // damage. Of the config, starting damage is applied in place; a change of the slots' fighters, skins, teams, bots
    // or perks restarts the match (`lab_roster_skins`).
    pub fn set_pause(&mut self, st: &LabState, on: bool, i: &mut LabInputs<'_>) -> Vec<LabEffect> {
        let mut out = Vec::new();
        if on == self.paused || self.unpaused_this_frame {
            return out;
        }
        self.paused = on;
        if on {
            self.config_at_pause = st.config.players.clone();
            self.mutators_at_pause = st.config.mutators;
            self.map_at_pause = st.config.selected_map.clone();
            self.map_roll = None;
            out.push(LabEffect::Paused(true));
            return out;
        }
        out.push(LabEffect::Paused(false));
        // UnpauseConfigChangeCheck: another map first, and then nothing else: GameMode.OnMapLoadRequested
        // (TrainingContainer @3435).
        if st.config.selected_map != self.map_at_pause {
            let roll = match self.map_roll.take() {
                Some((c, roll)) if c == st.config => roll,
                _ => lab_roll(i, &st.config),
            };
            out.push(LabEffect::MapLoad(roll));
            return out;
        }
        self.map_roll = None;
        // ArePlayerConfigsEqual per slot, the new config's first.
        let changed = st
            .config
            .players
            .iter()
            .enumerate()
            .any(|(n, p)| self.config_at_pause.get(n).is_none_or(|old| !lab_players_equal(p, old)))
            || st.config.mutators != self.mutators_at_pause;
        if changed {
            self.respawn(st, i, &mut out);
        }
        self.unpaused_this_frame = true;
        out
    }

    /// One world tick of the unpaused match (labTick): the spawn delay counts down and ends in a spawn.
    pub fn tick(&mut self, st: &LabState, i: &LabInputs<'_>) -> Option<LabEffect> {
        if self.spawn_left == 0 {
            return None;
        }
        self.spawn_left -= 1;
        if self.spawn_left > 0 {
            return None;
        }
        // spawnAllPlayers
        if lab_roster_skins(i, &st.config, &self.rolled) != self.roster {
            return Some(LabEffect::Restart(self.rolled.clone()));
        }
        let slots = lab_slots(&st.config);
        let starting_damage = slots.iter().map(|&s| st.config.players[s].starting_damage).collect();
        Some(LabEffect::Reset { starting_damage })
    }

    /// The frame's start: the unpause's flag cleared (UI_Game's Delay(0.0) -> false).
    pub fn frame(&mut self) {
        self.unpaused_this_frame = false;
    }
}

/// TrainGameMode Handle Training Config Updated: the World's same-attack penalty and
/// every player's instant cooldowns follow the config (read at the next hit and the next ability check; a running
/// cooldown runs on).
pub fn config_to_world(w: &mut World, c: &LabConfig) {
    w.flow.same_attack_penalty = c.same_attack_penalty;
    for f in w.players.clone() {
        w.fighter_mut(f).abilities.instant_cooldowns = c.instant_cooldowns;
    }
}

/// HoldToPauseComplete's gate for the Lab (@162570-163652): not while a modal is active, nor in the 0.25 s after one
/// went (the Cancel that closes the Move List is the pause key), not while the pause is on or a map load shows, and
/// not in the controller's first 3 s (a restart is past them).
pub fn can_pause_lab(
    world_tick: u32,
    restarted: bool,
    paused: bool,
    map_load_active: bool,
    modal_active: bool,
    since_modal_gone: f64,
) -> bool {
    if modal_active || since_modal_gone < 0.25 {
        return false;
    }
    !paused && !map_load_active && (world_tick > 3 * 60 || restarted)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lab_setup::{BotPool, LabInputs};
    use indexmap::IndexMap;
    use mvs_settings::Settings;

    fn config() -> LabConfig {
        let players = vec![
            LabPlayerConfig {
                account_id: "local".into(),
                character: Some("shaggy".into()),
                perks: vec![None; 4],
                ..empty_lab_player()
            },
            LabPlayerConfig {
                account_id: "Bot1".into(),
                character: Some("shaggy".into()),
                bot_difficulty: 1,
                team_index: 1,
                perks: vec![None; 4],
                ..empty_lab_player()
            },
            empty_lab_player(),
            empty_lab_player(),
        ];
        LabConfig { players, selected_map: "map_training".into(), ..LabConfig::default() }
    }

    fn with_inputs<R>(f: impl FnOnce(&mut LabInputs<'_>) -> R) -> R {
        let (s, fighters, pool) = (Settings::default(), IndexMap::new(), BotPool::default());
        let mut rng = || 0.5;
        let mut i = LabInputs { settings: &s, fighters: &fighters, pool: &pool, random: &mut rng, first: [None, None] };
        f(&mut i)
    }

    #[test]
    fn unpause_unchanged_does_not_spawn() {
        with_inputs(|i| {
            let st = LabState::new(config(), None);
            let mut c = LabControl::new(lab_roster_skins(i, &st.config, &LabRoll::default()), LabRoll::default());
            assert_eq!(c.set_pause(&st, true, i), [LabEffect::Paused(true)]);
            assert_eq!(c.set_pause(&st, false, i), [LabEffect::Paused(false)]);
            assert!(!c.spawning());
            // (the unpause's own frame cannot pause again)
            assert!(c.set_pause(&st, true, i).is_empty());
            c.frame();
            assert_eq!(c.set_pause(&st, true, i), [LabEffect::Paused(true)]);
        });
    }

    #[test]
    fn changed_damage_spawns_in_place_after_the_delay() {
        with_inputs(|i| {
            let mut st = LabState::new(config(), None);
            let mut c = LabControl::new(lab_roster_skins(i, &st.config, &LabRoll::default()), LabRoll::default());
            c.set_pause(&st, true, i);
            let mut new = st.config.clone();
            new.players[1].starting_damage = 50.0;
            st.request_config_update(new);
            let e = c.set_pause(&st, false, i);
            assert_eq!(e, [LabEffect::Paused(false), LabEffect::AbductAll]);
            assert!(c.spawning());
            let mut n = 1;
            let spawn = loop {
                if let Some(e) = c.tick(&st, i) {
                    break e;
                }
                n += 1;
                assert!(n < 200);
            };
            assert_eq!(n, 60);
            assert_eq!(spawn, LabEffect::Reset { starting_damage: vec![0.0, 50.0] });
        });
    }

    #[test]
    fn changed_roster_restarts() {
        with_inputs(|i| {
            let mut st = LabState::new(config(), None);
            let mut c = LabControl::new(lab_roster_skins(i, &st.config, &LabRoll::default()), LabRoll::default());
            c.set_pause(&st, true, i);
            let mut new = st.config.clone();
            new.players[1].team_index = 0;
            st.request_config_update(new);
            c.set_pause(&st, false, i);
            let spawn = (0..100).find_map(|_| c.tick(&st, i));
            assert!(matches!(spawn, Some(LabEffect::Restart(_))));
        });
    }

    /// Mutators ticked in the pause: the unpause respawns into a restart whose set-up runs them.
    #[test]
    fn other_mutators_restart_with_them() {
        with_inputs(|i| {
            let mut st = LabState::new(config(), None);
            let mut c = LabControl::new(lab_roster_skins(i, &st.config, &LabRoll::default()), LabRoll::default());
            c.set_pause(&st, true, i);
            let mut new = st.config.clone();
            new.mutators = MutatorSet::default().with(2, true).with(6, true);
            st.request_config_update(new);
            let e = c.set_pause(&st, false, i);
            assert!(e.contains(&LabEffect::AbductAll));
            let spawn = (0..100).find_map(|_| c.tick(&st, i));
            let Some(LabEffect::Restart(roll)) = spawn else { panic!("a restart: {spawn:?}") };
            let setup = crate::lab_setup::lab_setup(i, &st.config, &roll);
            assert_eq!(setup.mutators, st.config.mutators.names());
            assert_eq!(setup.mutators.len(), 2);
        });
    }

    #[test]
    fn a_map_picked_in_the_pause_loads_ahead_on_its_roll() {
        with_inputs(|i| {
            let mut st = LabState::new(config(), None);
            let mut c = LabControl::new(String::new(), LabRoll::default());
            assert!(c.map_ahead(&st, i).is_none());
            c.set_pause(&st, true, i);
            assert!(c.map_ahead(&st, i).is_none());
            let mut new = st.config.clone();
            new.selected_map = "map_other".into();
            st.request_config_update(new);
            let (config, roll) = c.map_ahead(&st, i).expect("another map");
            assert_eq!(config, st.config);
            let e = c.set_pause(&st, false, i);
            assert_eq!(e, [LabEffect::Paused(false), LabEffect::MapLoad(roll)]);
            assert!(c.map_roll.is_none());
        });
    }

    #[test]
    fn another_map_loads() {
        with_inputs(|i| {
            let mut st = LabState::new(config(), None);
            let mut c = LabControl::new(String::new(), LabRoll::default());
            c.set_pause(&st, true, i);
            let mut new = st.config.clone();
            new.selected_map = "map_other".into();
            st.request_config_update(new);
            let e = c.set_pause(&st, false, i);
            assert!(matches!(e[..], [LabEffect::Paused(false), LabEffect::MapLoad(_)]));
        });
    }

    #[test]
    fn a_broadcast_taken_blocks_updates_until_the_frame_ends() {
        let mut st = LabState::new(config(), None);
        st.request_authority_config();
        assert!(st.take_config().is_some());
        let mut c = st.config.clone();
        c.instant_cooldowns = true;
        st.request_config_update(c.clone());
        assert!(!st.config.instant_cooldowns);
        st.end_response();
        st.request_config_update(c);
        assert!(st.config.instant_cooldowns);
        assert!(st.take_config().is_some());
    }

    /// The Lab Tools' Instant Cooldowns and Same Attack Penalty (the panel's write is the session's config update)
    /// reach the World the loop copies them into, a reset (Spawn All Players) keeps them, and a slot on its cooldown
    /// triggers for the player and not for the bot (needs the dev store).
    #[test]
    fn the_rules_reach_the_world() {
        use mvs_sim::fighter::control::{
            game_mode_cooldown_disabled, has_ability_resources, set_ability_cooldown, trigger_ability_check,
        };
        use mvs_sim::setup::{MatchPlayer, MatchSetup};
        let player = |bot| MatchPlayer { character: Some("finn".to_owned()), bot, ..MatchPlayer::default() };
        let ms = MatchSetup { players: vec![player(false), player(true)], ..MatchSetup::default() };
        let Some(mut h) = mvs_sim_trace::harness::fresh(&ms, 1) else { return };
        let (me, bot) = (h.w.players[0], h.w.players[1]);
        let mut st = LabState::new(config(), None);
        // (a new Lab's first frame copies its config)
        assert!(st.take_config_changed());
        config_to_world(&mut h.w, &st.config);
        assert!(h.w.flow.same_attack_penalty == st.config.same_attack_penalty);
        assert!(!game_mode_cooldown_disabled(&h.w, me));
        // the panel's toggles: LabTools::apply's writes
        let c = LabConfig { instant_cooldowns: true, ..st.config.clone() };
        st.request_config_update(c);
        let c = LabConfig { same_attack_penalty: false, ..st.config.clone() };
        st.request_config_update(c);
        assert!(st.take_config_changed());
        config_to_world(&mut h.w, &st.config);
        assert!(!h.w.flow.same_attack_penalty);
        mvs_sim::world::reset::reset(&mut h.w);
        assert!(!h.w.flow.same_attack_penalty);
        assert!(game_mode_cooldown_disabled(&h.w, me));
        assert!(!game_mode_cooldown_disabled(&h.w, bot));
        // (a slot of the character's ability containers: one past them has its resources always)
        let slot = (0..8)
            .find(|&k| {
                set_ability_cooldown(&mut h.w, bot, k, 5.0);
                !has_ability_resources(&h.w, bot, k)
            })
            .expect("a slot with an ability");
        set_ability_cooldown(&mut h.w, me, slot, 5.0);
        assert!(trigger_ability_check(&mut h.w, me, slot));
        assert!(!trigger_ability_check(&mut h.w, bot, slot));
        // off again
        let c = LabConfig { instant_cooldowns: false, same_attack_penalty: true, ..st.config.clone() };
        st.request_config_update(c);
        assert!(st.take_config_changed());
        config_to_world(&mut h.w, &st.config);
        assert!(h.w.flow.same_attack_penalty);
        assert!(!trigger_ability_check(&mut h.w, me, slot));
    }

    #[test]
    fn kick_empties_the_slot() {
        let mut st = LabState::new(config(), None);
        st.request_kick_player("Bot1");
        assert!(st.config.players[1].account_id.is_empty());
        assert!(st.take_config().is_some());
    }

    #[test]
    fn the_pause_key_waits_three_seconds() {
        assert!(!can_pause_lab(100, false, false, false, false, 10.0));
        assert!(can_pause_lab(181, false, false, false, false, 10.0));
        assert!(can_pause_lab(0, true, false, false, false, 10.0));
        assert!(!can_pause_lab(181, false, false, false, true, 10.0));
        assert!(!can_pause_lab(181, false, false, false, false, 0.1));
    }
}
